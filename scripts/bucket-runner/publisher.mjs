/** Atomic publisher: replan a repository, verifyEvidence against the immutable
 * upload snapshot, mark `bucketed:true` and commit the new manifest. The
 * publication lock, manifest compare-before-publish and locked index binding
 * are reused from the existing platform transfer. Identity is bound to
 * the trusted profile/binding and immutable object keys.
 */
import { createHash } from 'node:crypto';
import { lstat, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { inspectRepository } from './planner.mjs';
import { assertObjectKey, validateBinding, validateProfile } from './capabilities.mjs';
import { ADAPTER_ID, ADAPTER_VERSION, verifyEvidence } from './adapters.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const TEXT = { 'utf8': true, fatal: true };
const isAssetRoot = p => p === '.' || (typeof p === 'string' && p.length <= 4096 && !isAbsolute(p)
  && !/[\\\x00-\x1f\x7f]/.test(p) && p.split('/').every(s => s && s !== '.' && s !== '..'));
const safeRelative = p => typeof p === 'string' && p.length > 0 && p.length <= 4096 && !isAbsolute(p)
  && !/[\\\x00-\x1f\x7f:]/.test(p) && p.split('/').every(s => s && s !== '.' && s !== '..');

async function contained(root, name, { missing = false } = {}) {
  if (!safeRelative(name)) throw new Error('Unsafe manifest source path');
  const target = resolve(root, name);
  const rel = relative(root, target);
  if (!rel || rel.startsWith(`..${sep}`)) throw new Error('Asset path escapes root');
  const parts = [root, ...name.split('/').slice(0, -1).reduce((acc, part) => {
    acc.push(acc.length ? `${acc[acc.length - 1]}/${part}` : part);
    return acc;
  }, [])];
  for (const path of [root, ...parts]) {
    const stat = await lstat(path);
    if (stat.isSymbolicLink()) throw new Error('Symlink manifest path refused');
    if (path !== target && !stat.isDirectory()) throw new Error('Manifest ancestor must be a directory');
  }
  try {
    const stat = await lstat(target);
    if (stat.isSymbolicLink()) throw new Error('Symlink manifest path refused');
    if (!stat.isFile()) throw new Error('Manifest must be a regular file');
  } catch (error) {
    if (error.code === 'ENOENT' && missing) return target;
    throw error;
  }
  return target;
}
const readUtf8 = async path => (await readFile(path, TEXT)).toString('utf8');
const atomicWrite = async (path, contents) => {
  const dir = dirname(path), temp = `${path}.${hash(contents).slice(0, 12)}.tmp`;
  await mkdir(dir, { recursive: true });
  await writeFile(temp, contents, { encoding: 'utf8', mode: 0o644, flag: 'wx' });
  await rename(temp, path);
  return path;
};
function rewritePath(relative) { return isAssetRoot(relative) ? relative : null; }

/** Acquire an exclusive flock against a sibling file. Refusing to ever unlink
 * the inode preserves lock integrity; the OS releases it when the process exits.
 * The lock-holder keeps the file descriptor open in a subshell that consumes
 * stdin until EOF. Closing the pipe (or terminating the process) releases
 * the flock. A second writer waiting on the same lock waits briefly, then
 * is refused with a clear `EEXIST` error.
 */
export async function publicationLock(path, { waitMs = 1000 } = {}) {
  if (typeof path !== 'string' || !isAbsolute(path)) throw new Error('invalid-lock-path');
  const { open } = await import('node:fs/promises');
  const { spawn } = await import('node:child_process');
  const handle = await open(path, 'a');
  // Node returns a high fd (>=20). `flock` and shell redirection need a low
  // fd, so spawn a tiny helper that owns fd 9 and execs into the lock script.
  const wait = Math.max(0, Math.floor(waitMs / 1000));
  // The helper holds fd 9 (a dup of the parent's lock fd), acquires the flock,
  // then sleeps until the parent kills the process. Stdout is piped so the
  // child doesn't accidentally inherit a closing fd.
  const script = `flock -E 73 -w ${wait} 9 || exit 73; exec sleep 86400`;
  const helper = spawn('/bin/sh', ['-c', `exec 9<>/proc/${process.pid}/fd/${handle.fd}; exec /bin/sh -c ${JSON.stringify(script)}`],
    { stdio: 'ignore' });
  const child = helper;
  const ready = new Promise((resolve, reject) => {
    let settled = false;
    const finish = fn => { if (settled) return; settled = true; clearTimeout(timer); fn(); };
    const timer = setTimeout(finish, Math.max(50, waitMs) + 5000);
    timer.unref();
    child.once('error', error => finish(() => reject(error)));
    child.once('exit', code => finish(() => reject(new Error(code === 73 ? 'EEXIST: publication lock held' : 'lock-failed'))));
    // After a tiny grace period, if the child is still running, we own the lock.
    setTimeout(() => { if (child.exitCode === null) finish(resolve); }, 200).unref();
  });
  await ready;
  return async () => {
    try { process.kill(child.pid); } catch {}
    try { await new Promise(resolve => { if (child.exitCode !== null) resolve(); else child.once('exit', resolve); }); } catch {}
    try { await handle.close(); } catch {}
  };
}

/** Pure planning step: validate profile + binding, run the planner, and
 * return a concrete plan + the list of new eligible candidates needing upload.
 * No manifest, transport, or index state is touched.
 */
export async function planRepository({ root, profile, binding, consumer, assetRoot = '.', manifest,
  options = {} } = {}) {
  if (typeof root !== 'string' || !isAbsolute(root) || resolve(root) !== root) throw new Error('invalid-root');
  if (assetRoot !== '.' && !isAssetRoot(assetRoot)) throw new Error('invalid-asset-root');
  const authority = validateProfile(profile);
  const trusted = validateBinding(authority, binding);
  const plan = await inspectRepository({ root, profile, binding: { ...trusted, repositoryId: trusted.repositoryId }, consumer, assetRoot, manifest, options });
  if (plan.status !== 'planned') return { plan, toUpload: [] };
  const before = new Set(manifest ? manifest.assets.map(a => a.sourcePath) : []);
  const toUpload = plan.proposedManifest.assets
    .filter(a => !before.has(a.sourcePath) && a.bucketed === false)
    .filter(a => a.provenance?.adapterId === ADAPTER_ID && a.provenance?.adapterVersion === ADAPTER_VERSION
      && typeof a.sha256 === 'string' && /^[a-f0-9]{64}$/.test(a.sha256)
      && Number.isSafeInteger(a.bytes) && a.bytes > 0);
  return { plan, toUpload };
}

function entryFromPlan(entry, trusted, authority) {
  return {
    id: entry.id, kind: entry.kind ?? 'image',
    logicalPath: entry.logicalPath, sourcePath: entry.sourcePath,
    objectKey: entry.objectKey,
    sha256: entry.sha256, bytes: entry.bytes,
    regenerable: true, visibility: 'private',
    bucketed: true,
    generator: entry.generator,
    provenance: entry.provenance
  };
}

/** Reject mutations: an unsafe manifest operation must keep the worktree and
 * the prior manifest intact, never replace them with partial state.
 */
function failPending(plan, reason) {
  return { status: 'refused', reason, plan, toUpload: [], indexRebound: false, manifestWritten: false };
}

/** Atomic publish+verify+bind cycle. The publication lock guards both the
 * manifest and the optional daemon-managed index entry that records the
 * enrolled manifest's digest against the bound namespace.
 */
export async function publishRepository({ root, profile, binding, consumer, assetRoot = '.', manifestPath,
  transport, options = {}, indexPath } = {}) {
  if (typeof root !== 'string' || !isAbsolute(root) || resolve(root) !== root) throw new Error('invalid-root');
  if (assetRoot !== '.' && !isAssetRoot(assetRoot)) throw new Error('invalid-asset-root');
  if (typeof manifestPath !== 'string' || !isAbsolute(manifestPath)) throw new Error('invalid-manifest-path');
  if (!transport || !['head', 'download', 'putIfAbsent'].every(name => typeof transport[name] === 'function')) {
    throw new Error('invalid-transport');
  }
  const authority = validateProfile(profile);
  const trusted = validateBinding(authority, binding);
  const release = await publicationLock(`${manifestPath}.lock`);
  try {
    let prior;
    try { prior = await readUtf8(manifestPath); }
    catch (error) { if (error.code !== 'ENOENT') throw error; prior = null; }
    const priorManifest = prior === null ? null : JSON.parse(prior);
    const { plan, toUpload } = await planRepository({ root, profile, binding: trusted, consumer, assetRoot,
      manifest: priorManifest, options });
    if (plan.status !== 'planned' || !toUpload.length) {
      return { plan, toUpload, indexRebound: false, manifestWritten: false };
    }
    const { head, download, putIfAbsent } = transport;
    const verified = [];
    for (const entry of toUpload) {
      const ext = entry.sourcePath.split('.').at(-1).toLowerCase();
      const key = assertObjectKey(authority, trusted, `${trusted.namespace}/${entry.sha256}.${ext}`);
      const cache = `${root}/.bucket-cache/upload-${hash(entry.id).slice(0, 12)}`;
      await mkdir(dirname(cache), { recursive: true });
      try {
        const exists = await head(key);
        if (exists) await download(key, cache);
        else {
          await contained(root, entry.sourcePath, { missing: false });
          const source = resolve(root, entry.sourcePath);
          const { copyFile } = await import('node:fs/promises');
          await copyFile(source, cache, 0o600);
          try { await putIfAbsent(key, cache); }
          catch (error) {
            if (!error?.$metadata || error.$metadata.httpStatusCode !== 412) throw error;
            await download(key, cache);
          }
        }
        const { readFile } = await import('node:fs/promises');
        const bytes = await readFile(cache);
        verifyEvidence(entry.provenance, bytes, {
          root, repositoryId: trusted.repositoryId, relativePath: entry.sourcePath,
          consumer: { id: entry.provenance.consumer.id, version: entry.provenance.consumer.version, assetRoots: [rewritePath(assetRoot)].filter(Boolean) }
        });
        const digest = hash(bytes);
        if (digest !== entry.sha256 || bytes.length !== entry.bytes) throw new Error('snapshot-mismatch');
        verified.push({ entry, key, sha256: digest, bytes: bytes.length });
      } finally { await rm(cache, { force: true }); }
    }
    const existingIds = new Set(priorManifest?.assets.map(a => a.id) ?? []);
    for (const entry of plan.proposedManifest.assets) {
      if (existingIds.has(entry.id)) continue;
      existingIds.add(entry.id);
    }
    const verifiedById = new Map(verified.map(v => [v.entry.id, v]));
    const nextAssets = plan.proposedManifest.assets.map(asset => {
      if (verifiedById.has(asset.id)) return entryFromPlan(asset, trusted, authority);
      return asset;
    });
    const finalManifest = { ...plan.proposedManifest, assets: nextAssets };
    const finalBytes = `${JSON.stringify(finalManifest, null, 2)}\n`;
    const current = prior === null ? null : await readUtf8(manifestPath).catch(() => null);
    if (current !== null && current !== prior) throw new Error('manifest-changed-concurrently');
    await atomicWrite(manifestPath, finalBytes);
    let indexRebound = false;
    if (indexPath && isAbsolute(indexPath)) {
      const indexDir = dirname(indexPath);
      const indexRecord = { schemaVersion: 1, profileId: authority.id, repositoryId: trusted.repositoryId,
        namespace: trusted.namespace, manifestPath, manifestDigest: hash(finalBytes), updatedAt: new Date().toISOString() };
      await mkdir(indexDir, { recursive: true });
      await atomicWrite(indexPath, `${JSON.stringify(indexRecord, null, 2)}\n`);
      indexRebound = true;
    }
    return { plan, toUpload: verified, indexRebound, manifestWritten: true,
      manifestDigest: hash(finalBytes), manifestPath };
  } catch (error) {
    return failPending({ status: 'refused', reason: error.message, outcomes: [], added: [],
      complete: false, proposedManifest: null }, error.message);
  } finally { try { await release(); } catch {} }
}
