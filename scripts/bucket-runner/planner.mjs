/** Read-only enrollment planning. No manifest, index, source or remote writes.
 * A plan is NOT publication authority: the publisher must lock/revalidate the
 * manifest and binding and verifyEvidence against its immutable upload snapshot.
 * In particular, retained legacy entries are not proof of current source bytes.
 */
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstat } from 'node:fs/promises';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { probeCandidate, CONSUMER_ID, CONSUMER_VERSION } from './adapters.mjs';
import { assetAuthorized, validateBinding, validateProfile } from './capabilities.mjs';

const execute = promisify(execFile);
const hash = text => createHash('sha256').update(text).digest('hex');
const media = /\.(?:png|jpe?g|webp|avif|gif|svg|mp3|wav|ogg|mp4|webm|glb|gltf)$/i;
const excluded = /^(?:\..*|node_modules|vendor|dist|build|out|target|coverage|cache|caches|tmp|temp|test|tests|__tests__|fixtures|audit|audits|evidence|secrets?|credentials?)$/i;
const safe = p => typeof p === 'string' && p.length > 0 && p.length <= 4096 && !isAbsolute(p)
  && !/[\\\x00-\x1f\x7f:]/.test(p) && p.split('/').every(s => s && s !== '.' && s !== '..');
const isExcluded = p => p.split('/').some(s => excluded.test(s));
const below = (p, root) => root === '.' || p.startsWith(`${root}/`);
const decode = bytes => new TextDecoder('utf-8', { fatal: true }).decode(bytes);
function records(bytes) {
  if (!bytes.length) return [];
  if (bytes.at(-1) !== 0) throw new Error('incomplete-git-output');
  return decode(bytes.subarray(0, -1)).split('\0');
}
async function exists(p) {
  try { await lstat(p); return true; } catch (e) { if (e.code === 'ENOENT') return false; throw e; }
}
async function inspectOwnership(root, path = '') {
  const target = path ? join(root, path) : root;
  const chain = [];
  for (let p = target; ; p = dirname(p)) { chain.unshift(p); if (p === dirname(p)) break; }
  for (const p of chain) {
    const s = await lstat(p);
    if (s.isSymbolicLink()) throw new Error('symlink-path');
    if (p !== target && !s.isDirectory()) throw new Error('not-directory');
    if (p === root && (!s.isDirectory() || s.uid !== process.geteuid())) throw new Error('invalid-root-ownership');
    if (p !== root && p.startsWith(`${root}/`) && s.isDirectory() && await exists(join(p, '.git'))) throw new Error('nested-git');
  }
}
function limits(options) {
  const defaults = { timeoutMs: 5000, maxOutputBytes: 8 * 1024 * 1024, maxCandidates: 20000 };
  if (!options || typeof options !== 'object' || Array.isArray(options)) throw new Error('invalid-options');
  for (const [key, value] of Object.entries(options)) {
    if (!(key in defaults) || !Number.isSafeInteger(value) || value < 1 || value > defaults[key]) throw new Error('invalid-options');
    defaults[key] = value;
  }
  return defaults;
}

/** Parent supplies trusted binding and a positively qualified adapter Consumer.
 * assetRoot is '.' or a safe repository subdirectory. Manifest paths are relative
 * to it; evidence paths are ALWAYS repository-relative. Options only lower caps.
 * No returned entry may be marked bucketed until separate verified publication.
 */
export async function inspectRepository({ root, profile, binding, consumer, assetRoot = '.', manifest, repositoryId: explicitId } = {}, options = {}) {
  const repositoryId = explicitId ?? binding?.repositoryId ?? null;
  const outcomes = [];
  const base = { repositoryId, assetRoot, outcomes, added: [], proposedManifest: null,
    complete: false, publicationRequiresRevalidation: true };
  const fail = (status, reason) => ({ ...base, status, reason });
  let cap;
  try { cap = limits(options); } catch (e) { return fail('refused', e.message); }
  if (typeof root !== 'string' || !isAbsolute(root) || resolve(root) !== root || /[\\\x00-\x1f\x7f]/.test(root)
    || (assetRoot !== '.' && (!safe(assetRoot) || isExcluded(assetRoot)))) return fail('refused', 'invalid-root');
  // Drop ALL inherited Git overrides (including index, worktree, config injection,
  // object directories, discovery and executable helpers). Never invoke a shell.
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')));
  Object.assign(env, { GIT_OPTIONAL_LOCKS: '0', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LC_ALL: 'C' });
  const git = async args => (await execute('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-C', root, ...args],
    { env, encoding: 'buffer', timeout: cap.timeoutMs, maxBuffer: cap.maxOutputBytes, killSignal: 'SIGKILL' })).stdout;
  let paths, ignored, gitlinks;
  try {
    await inspectOwnership(root);
    if (!await exists(join(root, '.git'))) return fail('refused', 'phantom-checkout');
    const top = decode(await git(['rev-parse', '--show-toplevel']));
    if (top !== `${root}\n`) return fail('refused', 'repository-ownership-mismatch');
    const staged = records(await git(['ls-files', '--stage', '-z']));
    const tracked = [];
    gitlinks = [];
    for (const row of staged) {
      const match = /^(\d{6}) [a-f0-9]+ [0-3]\t([\s\S]+)$/.exec(row);
      if (!match) throw new Error('invalid-git-output');
      tracked.push(match[2]);
      if (match[1] === '160000') gitlinks.push(match[2]);
    }
    const untracked = records(await git(['ls-files', '--others', '--exclude-standard', '-z']));
    const trackedIgnored = records(await git(['ls-files', '--cached', '--ignored', '--exclude-standard', '-z']));
    const untrackedIgnored = records(await git(['ls-files', '--others', '--ignored', '--exclude-standard', '-z']));
    ignored = new Set([...trackedIgnored, ...untrackedIgnored]);
    paths = [...new Set([...tracked, ...untracked, ...ignored])].sort();
    // Bound the full enumeration, not just supported extensions; truncation is fatal.
    if (paths.length > cap.maxCandidates || staged.length > cap.maxCandidates) return fail('retryable', 'candidate-limit');
  } catch (e) {
    return fail('retryable', e.message === 'symlink-path' || e.message === 'invalid-root-ownership' ? e.message : 'git-inspection-failed');
  }
  let authority, trusted, authorityReason;
  try { authority = validateProfile(profile); trusted = validateBinding(authority, binding); }
  catch { authorityReason = 'unsupported-storage-scope'; }
  let proposal;
  try {
    if (manifest !== undefined) {
      if (!manifest || manifest.schemaVersion !== 1 || manifest.bucket !== 'dracon-lfs' || !Array.isArray(manifest.assets)
        || manifest.namespace !== binding?.namespace) return fail('refused', 'manifest-binding-mismatch');
      proposal = structuredClone(manifest);
    } else {
      proposal = { schemaVersion: 1, bucket: 'dracon-lfs', repository: repositoryId, namespace: binding.namespace, assets: [] };
    }
    const ids = new Set(), sources = new Set();
    for (const entry of proposal.assets) {
      if (!entry || typeof entry.id !== 'string' || !entry.id || !safe(entry.sourcePath)
        || ids.has(entry.id) || sources.has(entry.sourcePath)) return fail('refused', 'invalid-or-duplicate-manifest-entry');
      if (typeof entry.objectKey !== 'string' || !entry.objectKey.startsWith(`${binding.namespace}/`)) return fail('refused', 'manifest-binding-mismatch');
      ids.add(entry.id); sources.add(entry.sourcePath);
    }
  } catch { return fail('refused', 'invalid-manifest'); }
  const byPath = new Map(proposal.assets.map(entry => [assetRoot === '.' ? entry.sourcePath : `${assetRoot}/${entry.sourcePath}`, entry]));
  paths = [...new Set([...paths.filter(p => media.test(p)), ...byPath.keys()])].sort();
  if (paths.length > cap.maxCandidates) return fail('retryable', 'candidate-limit');
  const qualified = consumer?.id === CONSUMER_ID && consumer.version === CONSUMER_VERSION
    && Object.keys(consumer).sort().join(',') === 'assetRoots,id,version'
    && Array.isArray(consumer.assetRoots) && consumer.assetRoots.length > 0 && consumer.assetRoots.length <= 64
    && consumer.assetRoots.every(p => safe(p) && !isExcluded(p));
  for (const sourcePath of paths) {
    const previous = byPath.get(sourcePath);
    const result = (status, reason, extra = {}) => outcomes.push({ sourcePath, ...(previous ? { id: previous.id } : {}), status, reason, ...extra });
    if (!safe(sourcePath) || isExcluded(sourcePath)) { result('excluded', 'unsafe-or-excluded-path'); continue; }
    if (ignored.has(sourcePath)) { result('excluded', 'git-ignored'); continue; }
    if (!below(sourcePath, assetRoot)) { result('excluded', 'outside-asset-root'); continue; }
    if (gitlinks.some(p => sourcePath === p || sourcePath.startsWith(`${p}/`))) { result('excluded', 'nested-gitlink'); continue; }
    try { await inspectOwnership(root, sourcePath); }
    catch (e) {
      if (e.code === 'ENOENT' || e.code === 'ENOTDIR') result(previous ? 'missing' : 'retryable', previous ? 'missing-or-deleted-source' : 'source-disappeared');
      else if (['symlink-path', 'nested-git', 'not-directory'].includes(e.message)) result('excluded', e.message);
      else result('retryable', 'inspection-failed');
      continue;
    }
    if (authorityReason || !assetAuthorized(authority, root, sourcePath)) { result('unsupported', authorityReason ?? 'unsupported-storage-scope'); continue; }
    if (!qualified || !consumer.assetRoots.some(p => sourcePath.startsWith(`${p}/`))) { result('unsupported', 'unsupported-consumer'); continue; }
    const probe = await probeCandidate({ root, repositoryId: trusted.repositoryId, relativePath: sourcePath, consumer });
    if (probe.status !== 'eligible') {
      result(probe.status === 'refused' ? 'excluded' : probe.status, previous && probe.status === 'unsupported' ? 'unsupported-legacy-source' : probe.reason,
        { inspectionReason: probe.reason, ...(previous ? { priorProofReusable: false } : {}) });
      continue;
    }
    const evidence = probe.evidence;
    if (previous) {
      const same = previous.sha256 === evidence.sha256 && previous.bytes === evidence.bytes;
      result(same ? 'already-registered' : 'unsupported', same ? 'already-registered' : 'changed-legacy-source', { priorProofReusable: same, evidence });
      continue;
    }
    const id = `asset-${hash(JSON.stringify([trusted.repositoryId, sourcePath]))}`;
    if (proposal.assets.some(entry => entry.id === id)) return fail('refused', 'stable-id-collision');
    const localPath = assetRoot === '.' ? sourcePath : sourcePath.slice(assetRoot.length + 1);
    const entry = { id, kind: 'image', logicalPath: localPath, sourcePath: localPath,
      objectKey: `${trusted.namespace}/{sha256}.{ext}`, sha256: evidence.sha256, bytes: evidence.bytes,
      regenerable: true, visibility: 'private', bucketed: false,
      generator: { adapterId: evidence.adapterId, adapterVersion: evidence.adapterVersion,
        runner: { sha256: authority.runner.sha256, version: authority.runner.version }, inputs: [] }, provenance: evidence };
    proposal.assets.push(entry); base.added.push(id);
    result('eligible', 'new-registration', { id, evidence });
  }
  // Failures cannot masquerade as a complete usable enrollment plan.
  const retryable = outcomes.some(o => o.status === 'retryable');
  return { ...base, complete: !retryable, status: retryable ? 'retryable' : authorityReason ? 'unsupported' : 'planned',
    proposedManifest: retryable || authorityReason ? null : proposal };
}
