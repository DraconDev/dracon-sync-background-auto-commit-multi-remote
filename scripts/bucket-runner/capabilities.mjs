/** Operator-owned fleet authority. Repository manifests never select credentials,
 * executable paths or storage namespaces. No credential file is read here.
 */
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, open } from 'node:fs/promises';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';

const PROFILE_LIMIT = 64 * 1024;
const RUNNER_LIMIT = 16 * 1024 * 1024;
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
function exact(value, fields) {
  return object(value) && Object.keys(value).sort().join(',') === [...fields].sort().join(',');
}
function require(condition, reason) { if (!condition) throw new Error(reason); }
function absolute(path) {
  return typeof path === 'string' && isAbsolute(path) && resolve(path) === path
    && !/[\\\x00-\x1f\x7f]/.test(path);
}
function namespace(path) {
  return typeof path === 'string' && path.length <= 240
    && path.split('/').every(part => /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,79}$/.test(part));
}
function relativeAsset(path) {
  return typeof path === 'string' && path.length <= 4096 && !isAbsolute(path)
    && !/[\\\x00-\x1f\x7f:]/.test(path)
    && path.split('/').every(part => part && part !== '.' && part !== '..');
}
function within(root, path) {
  const tail = relative(root, path);
  return tail === '' || (!isAbsolute(tail) && tail !== '..' && !tail.startsWith(`..${sep}`));
}
function freeze(value) {
  if (object(value) || Array.isArray(value)) {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}

/** @typedef {{schemaVersion:1,id:string,enabled:boolean,runner:{path:string,sha256:string,version:1},storage:{bucket:string,namespaceRoot:string,endpoint:string,region:string,uploaderCredentialFile:string,consumerCredentialFile:string},allowedAssetTrees:string[],consumers:{id:string,version:1}[]}} CapabilityProfile */
/** Validate a profile chosen by the operator, NOT repository data.
 * Asset trees authorize subtrees, not individual repository enrollment records.
 * Returned authority is an immutable copy, unaffected by later caller mutation.
 * @param {unknown} value @returns {CapabilityProfile}
 */
export function validateProfile(value) {
  require(exact(value, ['schemaVersion', 'id', 'enabled', 'runner', 'storage', 'allowedAssetTrees', 'consumers']), 'invalid fleet capability profile');
  require(value.schemaVersion === 1 && typeof value.enabled === 'boolean'
    && typeof value.id === 'string' && /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,79}$/.test(value.id), 'invalid fleet capability identity');
  const runner = value.runner;
  require(exact(runner, ['path', 'sha256', 'version']) && absolute(runner.path)
    && typeof runner.sha256 === 'string' && /^[a-f0-9]{64}$/.test(runner.sha256)
    && runner.version === 1, 'invalid installed runner binding');
  const storage = value.storage;
  require(exact(storage, ['bucket', 'namespaceRoot', 'endpoint', 'region', 'uploaderCredentialFile', 'consumerCredentialFile']), 'invalid storage authority');
  require(storage.bucket === 'dracon-lfs' && namespace(storage.namespaceRoot), 'unauthorized bucket or invalid namespace root');
  require(typeof storage.endpoint === 'string', 'invalid storage endpoint');
  let endpoint;
  try { endpoint = new URL(storage.endpoint); } catch { throw new Error('invalid storage endpoint'); }
  require(endpoint.protocol === 'https:' && !endpoint.username && !endpoint.password && !endpoint.search
    && !endpoint.hash && endpoint.pathname === '/' && endpoint.hostname, 'storage endpoint must be credential-free HTTPS origin');
  require(typeof storage.region === 'string' && /^[a-z0-9-]{1,40}$/.test(storage.region)
    && absolute(storage.uploaderCredentialFile) && absolute(storage.consumerCredentialFile)
    && storage.uploaderCredentialFile !== storage.consumerCredentialFile, 'explicit separate scoped capability files required');
  require(Array.isArray(value.allowedAssetTrees) && value.allowedAssetTrees.length > 0 && value.allowedAssetTrees.length <= 64
    && value.allowedAssetTrees.every(path => absolute(path) && path !== '/')
    && new Set(value.allowedAssetTrees).size === value.allowedAssetTrees.length, 'invalid authorized asset trees');
  require(Array.isArray(value.consumers) && value.consumers.length === 1
    && exact(value.consumers[0], ['id', 'version']) && value.consumers[0].id === 'filesystem-restore'
    && value.consumers[0].version === 1, 'unsupported fleet consumer capability');
  return freeze(structuredClone(value));
}

async function regularNoLinks(path) {
  require(absolute(path), 'unsafe operator capability path');
  const chain = [];
  for (let current = path; ; current = dirname(current)) {
    chain.unshift(current);
    if (current === dirname(current)) break;
  }
  for (let i = 0; i < chain.length; i++) {
    const stat = await lstat(chain[i], { bigint: true });
    require(!stat.isSymbolicLink(), 'operator capability path refuses symlinks');
    require(i === chain.length - 1 ? stat.isFile() : stat.isDirectory(), 'operator capability path must be regular');
  }
}
const identity = stat => [stat.dev, stat.ino, stat.size, stat.mode, stat.mtimeNs, stat.ctimeNs].join(':');
async function boundedOperatorFile(path, limit, privateFile) {
  await regularNoLinks(path);
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const before = await file.stat({ bigint: true });
    require(before.isFile() && before.uid === BigInt(process.geteuid()), 'operator capability file ownership refused');
    require((before.mode & 0o022n) === 0n, 'operator capability file must not be group/world writable');
    if (privateFile) require((before.mode & 0o077n) === 0n, 'fleet capability profile must be private (0600)');
    require(before.size > 0n && before.size <= BigInt(limit), 'operator capability file exceeds size bound');
    const buffer = Buffer.alloc(Number(before.size) + 1);
    let length = 0;
    while (length < buffer.length) {
      const result = await file.read(buffer, length, buffer.length - length, length);
      if (!result.bytesRead) break;
      length += result.bytesRead;
    }
    const after = await file.stat({ bigint: true });
    await regularNoLinks(path);
    const current = await lstat(path, { bigint: true });
    require(identity(before) === identity(after) && identity(before) === identity(current)
      && length === Number(before.size), 'operator capability file changed during inspection');
    return buffer.subarray(0, length);
  } finally { await file.close(); }
}

/** Explicit operator profile path only. Never searches repo config or ambient
 * AWS environment variables. Credential references are not opened or logged.
 */
export async function loadProfile(path) {
  const bytes = await boundedOperatorFile(path, PROFILE_LIMIT, true);
  let value;
  try { value = JSON.parse(bytes.toString('utf8')); } catch { throw new Error('invalid fleet capability JSON'); }
  return validateProfile(value);
}

/** A complete bundled runner is required: hash a bounded single artifact before
 * selection. Integration must execute a private immutable installation, not a
 * repository-supplied script or a dependency imported from a sibling checkout.
 */
export async function verifyInstalledRunner(profile) {
  const authority = validateProfile(profile);
  const bytes = await boundedOperatorFile(authority.runner.path, RUNNER_LIMIT, false);
  const actual = createHash('sha256').update(bytes).digest('hex');
  require(actual === authority.runner.sha256, 'installed runner digest mismatch');
  return { path: authority.runner.path, sha256: actual, version: authority.runner.version };
}

/** Pure prefilter; filesystem identity/provenance and consumer qualification are
 * separate required gates. In-scope paths are not automatically eligible.
 */
export function assetAuthorized(profile, repositoryRoot, sourcePath) {
  const authority = validateProfile(profile);
  if (!authority.enabled || !absolute(repositoryRoot) || !relativeAsset(sourcePath)) return false;
  const target = resolve(repositoryRoot, sourcePath);
  return within(repositoryRoot, target) && authority.allowedAssetTrees.some(root => within(root, target));
}

/** @typedef {{schemaVersion:1,profileId:string,repositoryId:string,namespace:string}} RepositoryBinding */
export function validateBinding(profile, binding) {
  const authority = validateProfile(profile);
  require(authority.enabled, 'fleet bucket discovery disabled');
  require(exact(binding, ['schemaVersion', 'profileId', 'repositoryId', 'namespace']) && binding.schemaVersion === 1
    && binding.profileId === authority.id && typeof binding.repositoryId === 'string'
    && /^[a-zA-Z0-9][a-zA-Z0-9._:-]{0,199}$/.test(binding.repositoryId)
    && namespace(binding.namespace)
    && (binding.namespace === authority.storage.namespaceRoot || binding.namespace.startsWith(`${authority.storage.namespaceRoot}/`)), 'repository storage binding refused');
  return freeze(structuredClone(binding));
}

/** Validate EVERY storage request independently of repository manifest claims. */
export function assertObjectKey(profile, binding, key) {
  const trusted = validateBinding(profile, binding);
  require(typeof key === 'string' && key.startsWith(`${trusted.namespace}/`), 'object outside repository namespace');
  const suffix = key.slice(trusted.namespace.length + 1);
  require(/^[a-f0-9]{64}\.(?:png|jpg|jpeg|webp|avif|gif|svg|mp3|wav|ogg|mp4|webm|glb|gltf)$/.test(suffix), 'invalid immutable object identity');
  return key;
}

/** Wrap a previously selected scoped transport. The caller supplies a binding
 * loaded from trusted daemon state, never a descriptor accepted on its own.
 * No delete, list-all-buckets or privilege-provisioning operation is exposed.
 */
export function bindTransport(transport, profile, binding, { readOnly = false } = {}) {
  const authority = validateProfile(profile);
  const trusted = validateBinding(authority, binding);
  require(typeof readOnly === 'boolean', 'invalid transport capability mode');
  const methods = readOnly ? ['head', 'download'] : ['head', 'download', 'putIfAbsent'];
  require(transport && methods.every(name => typeof transport[name] === 'function'), 'invalid private transport');
  const scoped = {
    head(key) { return transport.head(assertObjectKey(authority, trusted, key)); },
    download(key, file) { return transport.download(assertObjectKey(authority, trusted, key), file); }
  };
  if (!readOnly) scoped.putIfAbsent = (key, file) => transport.putIfAbsent(assertObjectKey(authority, trusted, key), file);
  return Object.freeze(scoped);
}
