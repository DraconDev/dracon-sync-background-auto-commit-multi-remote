import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, open } from 'node:fs/promises';
import { isAbsolute, join, parse, relative, resolve, sep } from 'node:path';
import { deflateSync, inflateSync } from 'node:zlib';

export const ADAPTER_ID = 'solid-rgba-png';
export const ADAPTER_VERSION = 1;
export const CONSUMER_ID = 'filesystem-restore';
export const CONSUMER_VERSION = 1;
export const CAPS = Object.freeze({ maxDimension: 512, maxSourceBytes: 2097152, maxRawBytes: 2097152 });
export const TOOLCHAIN = Object.freeze({ node: process.versions.node, zlib: process.versions.zlib });
const signature = Buffer.from('89504e470d0a1a0a', 'hex');
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const object = (x) => x !== null && typeof x === 'object' && !Array.isArray(x);
const keys = (x, names) => object(x) && Object.keys(x).sort().join(',') === [...names].sort().join(',');
const integer = (x, min, max) => Number.isSafeInteger(x) && x >= min && x <= max;

/** @typedef {{width:number,height:number,rgba:number[]}} SolidRecipe */
/** @typedef {{id:string,version:number,assetRoots:string[]}} Consumer */
/** @typedef {{root?:string,repo?:string,repositoryId:string,relativePath:string,consumer:Consumer}} Context */
/** @typedef {{schemaVersion:number,adapterId:string,adapterVersion:number,repositoryId:string,sourcePath:string,sha256:string,bytes:number,recipe:SolidRecipe,recipeDigest:string,toolchain:{node:string,zlib:string},inputDigests:never[],consumer:{id:string,version:number}}} Evidence */
/** @typedef {{status:'eligible',evidence:Evidence}|{status:'unsupported'|'retryable'|'refused',reason:string}} ProbeOutcome */

function validRecipe(r) {
  return keys(r, ['width', 'height', 'rgba']) && integer(r.width, 1, CAPS.maxDimension)
    && integer(r.height, 1, CAPS.maxDimension) && Array.isArray(r.rgba)
    && r.rgba.length === 4 && [0, 1, 2, 3].every((i) => integer(r.rgba[i], 0, 255))
    && r.height * (1 + r.width * 4) <= CAPS.maxRawBytes;
}
const recipeDigest = (r) => hash(JSON.stringify([r.width, r.height, ...r.rgba]));
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const b = Buffer.alloc(data.length + 12);
  b.writeUInt32BE(data.length); b.write(type, 4, 4, 'ascii'); data.copy(b, 8);
  b.writeUInt32BE(crc32(b.subarray(4, -4)), b.length - 4);
  return b;
}
/** Render only bounded numeric recipes, never opaque encoded data. @param {SolidRecipe} recipe @returns {Buffer} */
export function renderSolidPng(recipe) {
  if (!validRecipe(recipe)) throw new TypeError('invalid solid PNG recipe');
  const { width, height, rgba } = recipe;
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 6;
  const raw = Buffer.alloc(height * (1 + width * 4));
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) raw.set(rgba, y * (1 + width * 4) + 1 + x * 4);
  }
  return Buffer.concat([signature, chunk('IHDR', header), chunk('IDAT', deflateSync(raw, { level: 6 })), chunk('IEND', Buffer.alloc(0))]);
}
function parseRecipe(bytes) {
  if (bytes.length > CAPS.maxSourceBytes || !bytes.subarray(0, 8).equals(signature)) return null;
  let offset = 8;
  const chunks = [];
  for (const type of ['IHDR', 'IDAT', 'IEND']) {
    if (offset + 12 > bytes.length) return null;
    const n = bytes.readUInt32BE(offset);
    if (n > CAPS.maxSourceBytes || offset + n + 12 > bytes.length || bytes.toString('ascii', offset + 4, offset + 8) !== type) return null;
    if (crc32(bytes.subarray(offset + 4, offset + 8 + n)) !== bytes.readUInt32BE(offset + 8 + n)) return null;
    chunks.push(bytes.subarray(offset + 8, offset + 8 + n)); offset += n + 12;
  }
  const [h, compressed, end] = chunks;
  if (offset !== bytes.length || h.length !== 13 || end.length || !h.subarray(8).equals(Buffer.from([8, 6, 0, 0, 0]))) return null;
  const recipe = { width: h.readUInt32BE(0), height: h.readUInt32BE(4), rgba: [0, 0, 0, 0] };
  if (!validRecipe(recipe)) return null;
  const expected = recipe.height * (1 + recipe.width * 4);
  let raw;
  try { raw = inflateSync(compressed, { maxOutputLength: expected }); } catch { return null; }
  if (raw.length !== expected) return null;
  recipe.rgba = [...raw.subarray(1, 5)];
  // Exact re-render comparison excludes non-solid rows, filters, metadata, trailing streams and noncanonical compression.
  return renderSolidPng(recipe).equals(bytes) ? recipe : null;
}
const excluded = /^(?:\..*|node_modules|vendor|dist|build|out|target|coverage|cache|caches|tmp|temp|test|tests|__tests__|fixtures|audit|audits|evidence|secrets?|credentials?)(?:$)/i;
function safePath(p) {
  return typeof p === 'string' && p.length > 0 && p.length <= 4096 && !isAbsolute(p)
    && !/[\\\x00-\x1f\x7f:]/.test(p) && p.split('/').every((s) => s && s !== '.' && s !== '..' && !excluded.test(s));
}
class Refusal extends Error {}
function binding(c) {
  if (!object(c)) throw new Refusal('invalid-binding');
  const root = c.root ?? c.repo;
  if (typeof root !== 'string' || !isAbsolute(root) || resolve(root) !== root || /[\x00-\x1f\x7f]/.test(root)
    || (c.root !== undefined && c.repo !== undefined && c.root !== c.repo)
    || typeof c.repositoryId !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/.test(c.repositoryId)) throw new Refusal('invalid-ownership');
  if (!safePath(c.relativePath)) throw new Refusal('unsafe-path');
  if (!c.relativePath.endsWith('.png')) throw new Refusal('unsupported-extension');
  const consumer = c.consumer;
  if (!keys(consumer, ['id', 'version', 'assetRoots']) || consumer.id !== CONSUMER_ID || consumer.version !== CONSUMER_VERSION) throw new Refusal('unsupported-consumer');
  if (!Array.isArray(consumer.assetRoots) || !consumer.assetRoots.length || consumer.assetRoots.length > 64 || !consumer.assetRoots.every(safePath)
    || !consumer.assetRoots.some((p) => c.relativePath.startsWith(`${p}/`))) throw new Refusal('outside-consumer-roots');
  return { root, repositoryId: c.repositoryId, relativePath: c.relativePath, consumer: { id: consumer.id, version: consumer.version } };
}
async function exists(p) {
  try { await lstat(p); return true; } catch (e) { if (e.code === 'ENOENT') return false; throw e; }
}
async function inspectPath(b) {
  const leaf = join(b.root, b.relativePath);
  let current = parse(leaf).root;
  const parts = relative(current, leaf).split(sep);
  for (let i = 0; i < parts.length; i++) {
    current = join(current, parts[i]);
    const s = await lstat(current, { bigint: true });
    if (s.isSymbolicLink()) throw new Refusal('symlink-path');
    if (i < parts.length - 1 && !s.isDirectory()) throw new Refusal('not-directory');
    if (current !== b.root && current.startsWith(`${b.root}${sep}`) && s.isDirectory() && await exists(join(current, '.git'))) throw new Refusal('nested-git');
    if (i === parts.length - 1) {
      if (!s.isFile()) throw new Refusal('not-regular-file');
      return s;
    }
  }
}
const identity = (s) => [s.dev, s.ino, s.size, s.mode, s.mtimeNs, s.ctimeNs].join(':');
function outcomeError(e) {
  if (e instanceof Refusal) return { status: e.message === 'unsupported-consumer' || e.message === 'unsupported-extension' ? 'unsupported' : 'refused', reason: e.message };
  return { status: 'retryable', reason: ['ENOENT', 'ESTALE', 'ENOTDIR'].includes(e.code) ? 'source-disappeared' : 'inspection-failed' };
}
/** Bounded read-only probe. Parent supplies positive ownership and qualified consumer authority.
 * @param {Context} context @returns {Promise<ProbeOutcome>}
 */
export async function probeCandidate(context) {
  let file;
  try {
    const b = binding(context);
    const before = await inspectPath(b);
    if (before.size > BigInt(CAPS.maxSourceBytes)) return { status: 'unsupported', reason: 'source-too-large' };
    file = await open(join(b.root, b.relativePath), constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    if (identity(before) !== identity(await file.stat({ bigint: true }))) return { status: 'retryable', reason: 'source-changed' };
    const bytes = Buffer.alloc(Number(before.size) + 1);
    let length = 0;
    while (length < bytes.length) {
      const { bytesRead } = await file.read(bytes, length, bytes.length - length, length);
      if (!bytesRead) break;
      length += bytesRead;
    }
    let after;
    try { after = await inspectPath(b); }
    catch (e) {
      if (e instanceof Refusal) return { status: 'retryable', reason: 'source-changed' };
      throw e;
    }
    if (identity(before) !== identity(await file.stat({ bigint: true })) || identity(before) !== identity(after) || length !== Number(before.size)) return { status: 'retryable', reason: 'source-changed' };
    const source = bytes.subarray(0, length);
    const recipe = parseRecipe(source);
    if (!recipe) return { status: 'unsupported', reason: 'unsupported-png-encoding' };
    return { status: 'eligible', evidence: { schemaVersion: 1, adapterId: ADAPTER_ID, adapterVersion: ADAPTER_VERSION,
      repositoryId: b.repositoryId, sourcePath: b.relativePath, sha256: hash(source), bytes: source.length,
      recipe, recipeDigest: recipeDigest(recipe), toolchain: { ...TOOLCHAIN }, inputDigests: [], consumer: b.consumer } };
  } catch (e) { return outcomeError(e); }
  finally { if (file) await file.close(); }
}
/** Verify untrusted evidence against an immutable upload Buffer and parent-owned binding.
 * Returns the expected SHA-256; throws on any mismatch. Does not read or restore the source.
 * @param {unknown} evidence @param {Buffer} uploadSnapshot @param {Context} trustedBinding @returns {string}
 */
export function verifyEvidence(evidence, uploadSnapshot, trustedBinding) {
  const b = binding(trustedBinding);
  const e = evidence;
  if (!keys(e, ['schemaVersion', 'adapterId', 'adapterVersion', 'repositoryId', 'sourcePath', 'sha256', 'bytes', 'recipe', 'recipeDigest', 'toolchain', 'inputDigests', 'consumer'])
    || e.schemaVersion !== 1 || e.adapterId !== ADAPTER_ID || e.adapterVersion !== ADAPTER_VERSION
    || e.repositoryId !== b.repositoryId || e.sourcePath !== b.relativePath
    || !keys(e.consumer, ['id', 'version']) || e.consumer.id !== b.consumer.id || e.consumer.version !== b.consumer.version
    || !keys(e.toolchain, ['node', 'zlib']) || e.toolchain.node !== TOOLCHAIN.node || e.toolchain.zlib !== TOOLCHAIN.zlib
    || !Array.isArray(e.inputDigests) || e.inputDigests.length !== 0 || !validRecipe(e.recipe)
    || typeof e.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(e.sha256)
    || e.recipeDigest !== recipeDigest(e.recipe) || !integer(e.bytes, 1, CAPS.maxSourceBytes)
    || !Buffer.isBuffer(uploadSnapshot) || uploadSnapshot.length !== e.bytes) throw new Error('invalid-evidence');
  const expected = renderSolidPng(e.recipe);
  if (expected.length !== e.bytes || hash(expected) !== e.sha256 || !expected.equals(uploadSnapshot)) throw new Error('snapshot-mismatch');
  return e.sha256;
}
