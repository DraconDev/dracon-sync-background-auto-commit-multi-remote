/** Plain-Vite bootstrap-text qualification only; NOT installed CLI, storage, or
 * executed lifecycle proof. Entrypoint is npm run build with lifecycle enabled.
 * Inspection never executes repository code or writes files. JSON must already
 * equal JSON.stringify(value, null, 2) + '\n': noncanonical/duplicate-key JSON is
 * unsupported rather than silently reformatted. No installer is provided: the
 * parent must lock, compare expected bytes/digest, write, then inspect afresh.
 */
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, open, readdir } from 'node:fs/promises';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { promisify } from 'node:util';

export const PREBUILD = 'dracon-sync bucket-assets restore';
export const MANIFEST = 'bucket-assets.manifest.json';
const execute = promisify(execFile);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const proofs = new WeakMap();
const config = /^(?:vite|svelte)\.config\.(?:js|mjs|cjs|ts|mts|cts)$/;
const pinnedVite = /^(?:6|7)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/;
const own = (o, key) => Object.hasOwn(o, key);
const object = o => o !== null && typeof o === 'object' && !Array.isArray(o);
const fail = (reason, retry = false) => { throw Object.assign(new Error(reason), { retry }); };
const stamp = s => [s.dev, s.ino, s.uid, s.gid, s.mode, s.nlink, s.size, s.mtimeNs, s.ctimeNs].map(String).join(':');
const stat = p => lstat(p, { bigint: true });
function owned(s, directory = false) {
  if (s.isSymbolicLink()) fail('symlink-path');
  if (s.uid !== BigInt(process.geteuid()) || (s.mode & 0o022n)) fail('unsafe-ownership-or-permissions');
  if (directory ? !s.isDirectory() : !s.isFile()) fail('unexpected-file-type');
  if (!directory && s.nlink !== 1n) fail('hardlink-ambiguity');
}
async function readStable(path, cap) {
  const before = await stat(path);
  owned(before);
  if (before.size > BigInt(cap)) fail('file-size-limit');
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const opened = await handle.stat({ bigint: true });
    if (stamp(before) !== stamp(opened)) fail('source-changed', true);
    const buffer = Buffer.alloc(cap + 1);
    let length = 0;
    while (length < buffer.length) {
      const { bytesRead } = await handle.read(buffer, length, buffer.length - length, length);
      if (!bytesRead) break;
      length += bytesRead;
    }
    if (length > cap) fail('file-size-limit');
    if (stamp(before) !== stamp(await handle.stat({ bigint: true })) || stamp(before) !== stamp(await stat(path))) fail('source-changed', true);
    return { bytes: buffer.subarray(0, length), identity: stamp(before) };
  } finally { await handle.close(); }
}
function json(bytes, name) {
  let parsed;
  try { parsed = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)); }
  catch { fail(`malformed-${name}`); }
  if (!object(parsed) || !bytes.equals(Buffer.from(`${JSON.stringify(parsed, null, 2)}\n`))) fail(`noncanonical-or-duplicate-${name}`);
  return parsed;
}
async function ancestors(root) {
  for (let p = root; ; p = dirname(p)) {
    const s = await stat(p);
    if (s.isSymbolicLink() || !s.isDirectory()) fail('symlink-or-invalid-ancestor');
    if (p === root) owned(s, true);
    else {
      // Shared sticky temp ancestors are allowed; writable nonsticky ancestors
      // or foreign non-root owners could redirect a supposedly stable checkout.
      if ((s.uid !== 0n && s.uid !== BigInt(process.geteuid())) || ((s.mode & 0o022n) && !(s.mode & 0o1000n))) fail('unsafe-ancestor');
      const names = await readdir(p);
      if (names.includes('package.json')) fail('workspace-or-nested-package');
      if (names.includes('.git')) fail('nested-git');
    }
    if (p === dirname(p)) break;
  }
}
async function snapshot(root) {
  await ancestors(root);
  const entries = [];
  const files = new Map();
  let count = 0;
  async function walk(relative, depth) {
    if (depth > 16) fail('directory-depth-limit');
    const path = relative ? join(root, relative) : root;
    const before = await stat(path);
    owned(before, true);
    const names = (await readdir(path)).sort();
    for (const name of names) {
      if (++count > 4096) fail('directory-entry-limit');
      const rel = relative ? `${relative}/${name}` : name;
      const s = await stat(join(root, rel));
      if (s.isSymbolicLink()) fail('symlink-path');
      if (name === '.git') {
        if (relative) fail('nested-git');
        owned(s, true); // linked worktrees/gitfiles deliberately unsupported
        entries.push([rel, stamp(s)]);
        continue;
      }
      owned(s, s.isDirectory());
      entries.push([rel, stamp(s)]);
      if (relative && name === 'package.json') fail('workspace-or-nested-package');
      if (!relative && config.test(name)) fail(`unsupported-config:${name}`);
      if (s.isDirectory()) {
        if (rel !== 'node_modules') await walk(rel, depth + 1);
      } else files.set(rel, true);
    }
    if (stamp(before) !== stamp(await stat(path))) fail('source-changed', true);
    entries.push([relative || '.', stamp(before)]);
  }
  await walk('', 0);
  if (!entries.some(([p]) => p === '.git')) fail('missing-git-root');
  for (const legacy of ['bucket-assets.json', '.dracon/bucket-assets.json']) if (files.has(legacy)) fail('conflicting-legacy-authority');
  for (const name of ['.npmrc', 'npm-shrinkwrap.json', 'yarn.lock', 'pnpm-lock.yaml', 'bun.lock', 'bun.lockb']) {
    if (files.has(name)) fail(`unsupported-package-lifecycle:${name}`);
  }
  if (!files.has('index.html')) fail('missing-root-index');
  if (!entries.some(([p]) => p === 'public') || files.has('public')) fail('missing-default-public-directory');
  const reads = {};
  for (const name of ['package.json', 'package-lock.json', 'index.html', MANIFEST]) {
    if (!files.has(name)) {
      if (name !== MANIFEST) fail(`missing-${name}`);
      reads[name] = null;
    } else reads[name] = await readStable(join(root, name), 256 * 1024);
  }
  return { reads, fingerprint: digest(JSON.stringify({ entries, reads: Object.entries(reads).map(([p, r]) => [p, r && [r.identity, digest(r.bytes)]]) })) };
}
async function gitRoot(root) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')));
  Object.assign(env, { GIT_OPTIONAL_LOCKS: '0', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LC_ALL: 'C' });
  const { stdout } = await execute('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-C', root, 'rev-parse', '--show-toplevel'],
    { env, encoding: 'utf8', timeout: 5000, maxBuffer: 65536, killSignal: 'SIGKILL' });
  if (stdout !== `${root}\n`) fail('git-root-mismatch');
  const { stdout: stage } = await execute('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-C', root, 'ls-files', '--stage', '-z'],
    { env, encoding: 'utf8', timeout: 5000, maxBuffer: 1024 * 1024, killSignal: 'SIGKILL' });
  if (stage.split('\0').some(row => row.startsWith('160000 '))) fail('nested-gitlink');
}
function qualify(reads) {
  const pkg = json(reads['package.json'].bytes, 'package-json');
  const lock = json(reads['package-lock.json'].bytes, 'package-lock');
  if (own(pkg, 'workspaces')) fail('workspace-or-nested-package');
  if (pkg.packageManager !== undefined && !/^npm@\d+\.\d+\.\d+$/.test(pkg.packageManager)) fail('unsupported-package-manager');
  if (!object(pkg.scripts) || pkg.scripts.build !== 'vite build') fail('unsupported-build-script');
  if (own(pkg.scripts, 'assets:restore') || Object.entries(pkg.scripts).some(([key, value]) => key !== 'prebuild' && key !== 'build' && typeof value === 'string' && /(?:bucket-assets|assets[: -]restore|restore[: -]assets)/i.test(value))) fail('conflicting-restore-integration');
  if (own(pkg.scripts, 'prebuild') && pkg.scripts.prebuild !== PREBUILD) fail('occupied-prebuild');
  const deps = [pkg.dependencies, pkg.devDependencies];
  if (deps.some(d => d !== undefined && !object(d))) fail('invalid-dependencies');
  if (deps.some(d => d && Object.keys(d).some(k => /svelte|vite-plugin|@vitejs\/plugin/i.test(k)))) fail('unsupported-framework-or-plugin');
  const declarations = deps.filter(d => d && own(d, 'vite')).map(d => d.vite);
  if (declarations.length !== 1 || typeof declarations[0] !== 'string' || !pinnedVite.test(declarations[0])) fail('unsupported-vite-version');
  const version = declarations[0];
  const section = pkg.dependencies?.vite ? 'dependencies' : 'devDependencies';
  if (![2, 3].includes(lock.lockfileVersion) || !object(lock.packages) || lock.packages['']?.[section]?.vite !== version || lock.packages['node_modules/vite']?.version !== version || lock.packages['node_modules/vite']?.link === true) fail('vite-lock-mismatch');
  return pkg;
}

/** Outcomes: supported (on-disk managed text), plannable (NO consumer authority),
 * unsupported (concrete conservative exclusion), retryable (unstable/failed I/O).
 * All successful evidence is process-local and revalidated, never caller proof.
 * Static adapter scope only; parent MUST separately qualify installed runtime.
 */
export async function inspectConsumer({ root, entrypoint = 'npm run build', lifecycleScripts = true } = {}) {
  const base = { scope: 'plain-vite-bootstrap-text-v1', entrypoint: 'npm run build', manifestPath: MANIFEST,
    runtimeQualified: false, consumer: null, plan: null, evidence: null };
  const result = (status, reason) => ({ ...base, status, reason });
  if (entrypoint !== 'npm run build' || lifecycleScripts !== true) return result('unsupported', 'unsupported-lifecycle-entrypoint');
  if (typeof root !== 'string' || !isAbsolute(root) || resolve(root) !== root || /[\\\x00-\x1f\x7f]/.test(root)) return result('unsupported', 'invalid-root');
  try {
    const first = await snapshot(root);
    await gitRoot(root);
    const pkg = qualify(first.reads);
    const second = await snapshot(root);
    if (first.fingerprint !== second.fingerprint) fail('source-changed', true);
    const evidence = Object.freeze({ root, snapshotDigest: first.fingerprint, packageDigest: digest(first.reads['package.json'].bytes) });
    const managed = own(pkg.scripts, 'prebuild');
    proofs.set(evidence, { root, fingerprint: first.fingerprint, managed });
    const output = { ...base, status: managed ? 'supported' : 'plannable', reason: managed ? 'managed-bootstrap-text-verified' : 'missing-prebuild', evidence };
    if (managed) output.consumer = Object.freeze({ id: 'filesystem-restore', version: 1, assetRoots: Object.freeze(['public']) });
    else {
      pkg.scripts.prebuild = PREBUILD;
      output.plan = Object.freeze({ path: 'package.json', expectedDigest: evidence.packageDigest,
        expectedBytes: first.reads['package.json'].bytes.toString('utf8'),
        proposedBytes: `${JSON.stringify(pkg, null, 2)}\n`,
        mode: Number((await stat(join(root, 'package.json'))).mode & 0o777n), requiresExclusiveMutationLock: true, requiresFreshInspection: true });
    }
    return output;
  } catch (e) {
    return result(e.retry || e.code ? 'retryable' : 'unsupported', e.retry ? e.message : e.code ? `inspection-failed:${e.code}` : e.message);
  }
}

/** Reject forged/copy-serialized evidence and compare the full inspection again.
 * A successful revalidation is still not a lock or publication/runtime proof.
 */
export async function revalidateConsumer(evidence) {
  const proof = proofs.get(evidence);
  if (!proof) return { status: 'unsupported', reason: 'unrecognized-consumer-evidence', consumer: null };
  const fresh = await inspectConsumer({ root: proof.root });
  if (!fresh.evidence || fresh.evidence.snapshotDigest !== proof.fingerprint) return { status: 'retryable', reason: 'source-changed', consumer: null };
  return fresh;
}
