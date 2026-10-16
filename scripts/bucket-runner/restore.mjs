/** Create-only, cache-first filesystem consumer. Never overwrites differing
 * operator bytes, never uploads, and never executes repository code.
 */
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, open, mkdir, mkdtemp, link, rm } from 'node:fs/promises';
import { dirname, join, isAbsolute, resolve } from 'node:path';
import { assertObjectKey, validateProfile, validateBinding } from './capabilities.mjs';
import { CAPS } from './adapters.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const safe = path => typeof path === 'string' && path.length <= 4096
  && !/[\\\x00-\x1f\x7f:]/.test(path) && path.split('/').every(p => p && p !== '.' && p !== '..' && !p.startsWith('.'));
const require = (ok, reason) => { if (!ok) throw new Error(reason); };
async function directory(path, create = false) {
  const chain = [];
  for (let p = path; ; p = dirname(p)) { chain.unshift(p); if (p === dirname(p)) break; }
  for (const p of chain) {
    let stat;
    try { stat = await lstat(p); }
    catch (error) {
      if (!create || error.code !== 'ENOENT') throw error;
      await mkdir(p, { mode: 0o700 }).catch(e => { if (e.code !== 'EEXIST') throw e; });
      stat = await lstat(p);
    }
    require(stat.isDirectory() && !stat.isSymbolicLink(), 'unsafe-restore-directory');
    require(stat.uid === 0 || stat.uid === process.geteuid(), 'foreign-restore-directory');
    require(!(stat.mode & 0o022) || (p !== path && stat.uid === 0 && (stat.mode & 0o1000)), 'writable-restore-directory');
  }
}
async function bytesAt(path, expected) {
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const before = await file.stat({ bigint: true });
    require(before.isFile() && before.nlink === 1n && before.uid === BigInt(process.geteuid())
      && !(before.mode & 0o022n), 'unsafe-restore-file');
    require(before.size === BigInt(expected.bytes) && before.size <= BigInt(CAPS.maxSourceBytes), 'restore-size-mismatch');
    const bytes = Buffer.alloc(expected.bytes + 1);
    let length = 0;
    while (length < bytes.length) {
      const read = await file.read(bytes, length, bytes.length - length, length);
      if (!read.bytesRead) break;
      length += read.bytesRead;
    }
    const after = await file.stat({ bigint: true });
    const current = await lstat(path, { bigint: true });
    require(!current.isSymbolicLink() && before.dev === current.dev && before.ino === current.ino
      && before.size === after.size && before.mtimeNs === after.mtimeNs && before.ctimeNs === after.ctimeNs,
    'restore-source-changed');
    require(length === expected.bytes && sha(bytes.subarray(0, length)) === expected.sha256, 'restore-digest-mismatch');
    return bytes.subarray(0, length);
  } finally { await file.close(); }
}
function entriesFor(manifest, binding) {
  require(manifest?.schemaVersion === 1 && manifest.bucket === 'dracon-lfs'
    && manifest.namespace === binding.namespace && manifest.repository === binding.repositoryId
    && Array.isArray(manifest.assets) && manifest.assets.length <= 20000, 'invalid-consumer-manifest');
  const ids = new Set(), paths = new Set();
  for (const entry of manifest.assets) {
    require(entry && typeof entry.id === 'string' && entry.id && !ids.has(entry.id)
      && safe(entry.sourcePath) && entry.sourcePath.startsWith('public/')
      && entry.logicalPath === entry.sourcePath && !paths.has(entry.sourcePath)
      && entry.regenerable === true && entry.visibility === 'private'
      && typeof entry.bucketed === 'boolean' && /^[a-f0-9]{64}$/.test(entry.sha256)
      && Number.isSafeInteger(entry.bytes) && entry.bytes > 0 && entry.bytes <= CAPS.maxSourceBytes,
    'invalid-consumer-entry');
    require(entry.objectKey === `${binding.namespace}/{sha256}.{ext}`, 'invalid-consumer-object-template');
    ids.add(entry.id); paths.add(entry.sourcePath);
  }
  return manifest.assets;
}

/** Caller has separately qualified installed runtime/bootstrap. Profile and
 * read-only transport are operator authority, not repository-selected providers.
 */
export async function restoreRepository({ root, profile, binding, manifest, transport }) {
  require(typeof root === 'string' && isAbsolute(root) && resolve(root) === root, 'invalid-restore-root');
  const authority = validateProfile(profile), trusted = validateBinding(authority, binding);
  require(transport && typeof transport.head === 'function' && typeof transport.download === 'function'
    && transport.putIfAbsent === undefined, 'read-only-consumer-transport-required');
  const entries = entriesFor(manifest, trusted);
  const keys = new Map(entries.map(entry => [entry.id, assertObjectKey(authority, trusted,
    `${trusted.namespace}/${entry.sha256}.${entry.sourcePath.split('.').at(-1).toLowerCase()}`)]));
  await directory(root);
  require((await lstat(root)).uid === process.geteuid(), 'foreign-restore-root');
  const outcomes = [];
  for (const entry of entries) {
    const target = join(root, entry.sourcePath);
    await directory(dirname(target), false).catch(error => {
      if (error.code !== 'ENOENT') throw error;
    });
    try {
      await bytesAt(target, entry);
      outcomes.push({ sourcePath: entry.sourcePath, status: 'already-present' });
      continue;
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    require(entry.bucketed, 'missing-local-only-asset');
    // Cache and final path must share a filesystem for atomic create-only link.
    await directory(dirname(target), true);
    require((await lstat(dirname(target))).dev === (await lstat(root)).dev, 'unsupported-restore-mount');
    const cache = join(root, '.bucket-cache');
    await directory(cache, true);
    const scratch = await mkdtemp(join(cache, 'restore-'));
    const staged = join(scratch, 'verified');
    try {
      await transport.download(keys.get(entry.id), staged);
      await bytesAt(staged, entry);
      await directory(dirname(target));
      try { await link(staged, target); }
      catch (error) {
        if (error.code !== 'EEXIST') throw error;
        await bytesAt(target, entry); // concurrent matching publication only
      }
      outcomes.push({ sourcePath: entry.sourcePath, status: 'restored' });
    } finally { await rm(scratch, { recursive: true, force: true }); }
  }
  return { ok: true, outcomes };
}
