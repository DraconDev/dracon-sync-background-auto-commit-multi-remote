/** Trusted local enrollment authority; descriptors are comparisons, never grants.
 * No repository writes, network operations, or credential reads occur here.
 */
import { randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { constants } from 'node:fs';
import { lstat, open, rename, unlink } from 'node:fs/promises';
import { dirname, isAbsolute, relative, resolve, sep, join } from 'node:path';
import { validateProfile, validateBinding } from './capabilities.mjs';

const execute = promisify(execFile);
const LIMIT = 1024 * 1024;
const uid = process.geteuid();
const require = (ok, reason) => { if (!ok) throw new Error(reason); };
const exact = (v, keys) => v && typeof v === 'object' && !Array.isArray(v)
  && Object.keys(v).sort().join(',') === [...keys].sort().join(',');
const absolute = p => typeof p === 'string' && isAbsolute(p) && resolve(p) === p && !/[\\\x00-\x1f\x7f]/.test(p);
const within = (root, p) => { const tail = relative(root, p); return tail === '' || (!isAbsolute(tail) && tail !== '..' && !tail.startsWith(`..${sep}`)); };
const inode = s => `${s.dev}:${s.ino}`;
const stamp = s => `${inode(s)}:${s.mode}:${s.size}:${s.mtimeNs}:${s.ctimeNs}`;

/** Refuse links throughout the chain; writable ancestors must be root-owned
 * sticky directories (e.g. /tmp), not attacker-replaceable parents.
 */
async function directory(path, privateLeaf = false) {
  require(absolute(path), 'invalid-directory-path');
  const chain = [];
  for (let p = path; ; p = dirname(p)) { chain.unshift(p); if (p === dirname(p)) break; }
  let leaf;
  for (const p of chain) {
    const s = await lstat(p, { bigint: true });
    require(s.isDirectory() && !s.isSymbolicLink(), 'unsafe-directory-symlink');
    require(s.uid === BigInt(uid) || s.uid === 0n, 'wrong-directory-owner');
    require((s.mode & 0o022n) === 0n || (p !== path && s.uid === 0n && (s.mode & 0o1000n) !== 0n), 'writable-directory');
    leaf = s;
  }
  require(leaf.uid === BigInt(uid), 'wrong-directory-owner');
  if (privateLeaf) require((leaf.mode & 0o777n) === 0o700n, 'state-directory-must-be-private');
  return leaf;
}

async function privateFile(path, missing = false) {
  let s;
  try { s = await lstat(path, { bigint: true }); }
  catch (error) { if (missing && error.code === 'ENOENT') return null; throw error; }
  require(s.isFile() && !s.isSymbolicLink() && s.uid === BigInt(uid)
    && (s.mode & 0o777n) === 0o600n && s.nlink === 1n, 'unsafe-state-file');
  return s;
}

async function git(root, args, optional = false) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')));
  env.GIT_TERMINAL_PROMPT = '0'; env.GIT_OPTIONAL_LOCKS = '0';
  try {
    const { stdout } = await execute('git', ['-C', root, ...args], { env, timeout: 5000, maxBuffer: 16384, encoding: 'utf8' });
    return stdout.replace(/\n$/, '');
  } catch (error) { if (optional && error.code === 1) return null; throw new Error('git-inspection-refused', { cause: error }); }
}

async function inspect(root, remoteName, trustedRemoteUrl) {
  await directory(root);
  require(await git(root, ['rev-parse', '--show-toplevel']) === root, 'ambiguous-repository-root');
  require(await git(root, ['rev-parse', '--is-bare-repository']) === 'false', 'bare-repository-refused');
  await git(root, ['symbolic-ref', '-q', 'HEAD']);
  const common = await git(root, ['rev-parse', '--path-format=absolute', '--git-common-dir']);
  const stat = await directory(common);
  const head = await git(root, ['rev-parse', '--verify', 'HEAD^{commit}']);
  require(/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(head), 'invalid-head');
  const url = await git(root, ['config', '--get-all', `remote.${remoteName}.url`]);
  const push = await git(root, ['config', '--get-all', `remote.${remoteName}.pushurl`], true);
  require(url === trustedRemoteUrl && (push === null || push === trustedRemoteUrl), 'trusted-remote-mismatch');
  // Rewrites could send the selected raw URL to another host. Never infer trust.
  require(await git(root, ['remote', 'get-url', '--all', remoteName]) === trustedRemoteUrl
    && await git(root, ['remote', 'get-url', '--push', '--all', remoteName]) === trustedRemoteUrl, 'rewritten-remote-refused');
  return { common, commonIdentity: inode(stat), head };
}

function records(value, profile) {
  require(exact(value, ['schemaVersion', 'records']) && value.schemaVersion === 1 && Array.isArray(value.records)
    && value.records.length <= 1000, 'malformed-binding-state');
  const ids = new Set(), physical = new Set(), namespaces = new Set();
  for (const record of value.records) {
    require(exact(record, ['binding', 'commonIdentity', 'remoteName', 'trustedRemoteUrl', 'anchor']), 'malformed-binding-record');
    // A state directory belongs to one profile; another profile cannot reuse it.
    const binding = validateBinding(profile, record.binding);
    require(/^\d+:\d+$/.test(record.commonIdentity) && typeof record.commonIdentity === 'string'
      && typeof record.remoteName === 'string' && /^[A-Za-z0-9_-]{1,80}$/.test(record.remoteName)
      && validUrl(record.trustedRemoteUrl) && typeof record.anchor === 'string'
      && /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(record.anchor), 'malformed-binding-record');
    require(!ids.has(binding.repositoryId) && !physical.has(record.commonIdentity) && !namespaces.has(binding.namespace), 'duplicate-binding-state');
    ids.add(binding.repositoryId); physical.add(record.commonIdentity); namespaces.add(binding.namespace);
  }
  return value.records;
}
const validUrl = url => typeof url === 'string' && url.length > 0 && url.length <= 4096 && !/[\s\x00-\x1f\x7f]/.test(url) && !url.startsWith('-');

async function readState(path, profile) {
  const before = await privateFile(path, true);
  if (!before) return [];
  require(before.size > 0n && before.size <= BigInt(LIMIT), 'oversized-binding-state');
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    require(stamp(await file.stat({ bigint: true })) === stamp(before), 'binding-state-changed');
    const bytes = Buffer.alloc(Number(before.size) + 1);
    let length = 0;
    while (length < bytes.length) { const read = await file.read(bytes, length, bytes.length - length, length); if (!read.bytesRead) break; length += read.bytesRead; }
    require(length === Number(before.size) && stamp(await file.stat({ bigint: true })) === stamp(before)
      && stamp(await privateFile(path)) === stamp(before), 'binding-state-changed');
    return records(JSON.parse(bytes.subarray(0, length).toString('utf8')), profile);
  } finally { await file.close(); }
}

async function writeState(stateDirectory, path, entries) {
  const bytes = JSON.stringify({ schemaVersion: 1, records: entries });
  require(Buffer.byteLength(bytes) <= LIMIT, 'binding-state-capacity');
  const temporary = join(stateDirectory, `.bindings-${randomUUID()}.tmp`);
  const file = await open(temporary, constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW, 0o600);
  try { await file.writeFile(bytes); await file.sync(); } finally { await file.close(); }
  try {
    await privateFile(path, true);
    await rename(temporary, path);
    const parent = await open(stateDirectory, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW);
    try { await parent.sync(); } finally { await parent.close(); }
  } finally { await unlink(temporary).catch(error => { if (error.code !== 'ENOENT') throw error; }); }
}

/** @typedef {{stateDirectory:string,profile:import('./capabilities.mjs').CapabilityProfile,repositoryRoot:string,remoteName:string,trustedRemoteUrl:string,binding?:import('./capabilities.mjs').RepositoryBinding}} BindingRequest */
/** Bind only operator-selected ownership facts. Optional portable binding must
 * match existing trusted physical state; it can never create/adopt authority.
 * @param {BindingRequest} request
 * @returns {Promise<{status:'bound'|'existing',binding:import('./capabilities.mjs').RepositoryBinding}>}
 * @throws On any conflict, unsupported repository or failed inspection.
 */
export async function bindRepository(request) {
  require(request && exact(request, ['stateDirectory', 'profile', 'repositoryRoot', 'remoteName', 'trustedRemoteUrl',
    ...(Object.hasOwn(request, 'binding') ? ['binding'] : [])]), 'invalid-binding-request');
  const { stateDirectory, repositoryRoot, remoteName, trustedRemoteUrl } = request;
  const profile = validateProfile(request.profile);
  require(profile.enabled, 'fleet bucket discovery disabled');
  require(absolute(repositoryRoot) && absolute(stateDirectory) && /^[A-Za-z0-9_-]{1,80}$/.test(remoteName)
    && typeof remoteName === 'string' && validUrl(trustedRemoteUrl), 'invalid-repository-authority');
  require(profile.allowedAssetTrees.some(tree => within(tree, repositoryRoot)), 'repository-outside-authorized-asset-tree');
  require(!within(repositoryRoot, stateDirectory), 'state-inside-repository');
  const supplied = Object.hasOwn(request, 'binding') ? validateBinding(profile, request.binding) : null;
  const stateIdentity = inode(await directory(stateDirectory, true));
  const lockPath = join(stateDirectory, 'bindings.lock');
  await privateFile(lockPath, true);
  const { publicationLock } = await import('./publisher.mjs');
  const release = await publicationLock(lockPath, { waitMs: 10000 });
  try {
    require(inode(await directory(stateDirectory, true)) === stateIdentity, 'state-directory-changed');
    await privateFile(lockPath);
    const inspected = await inspect(repositoryRoot, remoteName, trustedRemoteUrl);
    require(!within(inspected.common, stateDirectory), 'state-inside-git-directory');
    const path = join(stateDirectory, 'bindings.json');
    const entries = await readState(path, profile);
    const existing = entries.find(record => record.commonIdentity === inspected.commonIdentity);
    if (existing) {
      require(existing.remoteName === remoteName && existing.trustedRemoteUrl === trustedRemoteUrl, 'stored-remote-conflict');
      require(!supplied || JSON.stringify(supplied) === JSON.stringify(validateBinding(profile, existing.binding)), 'descriptor-binding-conflict');
      await git(repositoryRoot, ['merge-base', '--is-ancestor', existing.anchor, inspected.head]);
    } else require(!supplied, 'descriptor-without-trusted-physical-binding');
    const repositoryId = existing?.binding.repositoryId ?? randomUUID();
    const binding = validateBinding(profile, existing?.binding ?? { schemaVersion: 1, profileId: profile.id,
      repositoryId, namespace: `${profile.storage.namespaceRoot}/${repositoryId}` });
    const again = await inspect(repositoryRoot, remoteName, trustedRemoteUrl);
    require(again.commonIdentity === inspected.commonIdentity && again.head === inspected.head
      && inode(await directory(stateDirectory, true)) === stateIdentity, 'repository-or-state-changed');
    if (!existing) await writeState(stateDirectory, path, [...entries, { binding, commonIdentity: inspected.commonIdentity,
      remoteName, trustedRemoteUrl, anchor: inspected.head }]);
    return { status: existing ? 'existing' : 'bound', binding };
  } finally { await release(); }
}
