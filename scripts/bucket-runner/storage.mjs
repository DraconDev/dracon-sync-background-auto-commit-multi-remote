/** Private S3 transport. Only operator-selected profiles grant authority. */
import { S3Client, HeadObjectCommand, GetObjectCommand, PutObjectCommand } from '@aws-sdk/client-s3';
import { constants } from 'node:fs';
import { lstat, open } from 'node:fs/promises';
import { dirname, isAbsolute, resolve } from 'node:path';
import { Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { validateProfile, validateBinding, assertObjectKey, bindTransport } from './capabilities.mjs';
import { CAPS } from './adapters.mjs';

const identity = s => [s.dev, s.ino, s.mode, s.uid, s.size, s.mtimeNs, s.ctimeNs].join(':');
const require = (ok, message) => { if (!ok) throw new Error(message); };

/** Check each component, including /; writable ancestors are never authority. */
async function ancestors(path) {
  require(typeof path === 'string' && isAbsolute(path) && resolve(path) === path && !/[\x00-\x1f\x7f]/.test(path), 'unsafe storage file path');
  for (let current = dirname(path); ; current = dirname(current)) {
    const stat = await lstat(current);
    require(stat.isDirectory() && !stat.isSymbolicLink() && !(stat.mode & 0o022), 'unsafe storage file ancestor');
    if (current === dirname(current)) break;
  }
}
async function checkedOpen(path, flags, privateFile) {
  await ancestors(path);
  const file = await open(path, flags | constants.O_NOFOLLOW | constants.O_NONBLOCK, 0o600);
  try {
    const stat = await file.stat({ bigint: true });
    require(stat.isFile(), 'storage file must be regular');
    if (privateFile) require(stat.uid === BigInt(process.geteuid()) && !(stat.mode & 0o077n), 'storage file must be private and operator-owned');
    await ancestors(path);
    require(identity(stat) === identity(await lstat(path, { bigint: true })), 'storage file changed');
    return file;
  } catch (error) { await file.close(); throw error; }
}
async function snapshot(path, limit, privateFile) {
  const file = await checkedOpen(path, constants.O_RDONLY, privateFile);
  try {
    const before = await file.stat({ bigint: true });
    require(before.size > 0n && before.size <= BigInt(limit), 'storage file size refused');
    const bytes = Buffer.alloc(Number(before.size) + 1);
    let length = 0;
    while (length < bytes.length) {
      const read = await file.read(bytes, length, bytes.length - length, length);
      if (!read.bytesRead) break;
      length += read.bytesRead;
    }
    await ancestors(path);
    require(length === Number(before.size) && identity(before) === identity(await file.stat({ bigint: true }))
      && identity(before) === identity(await lstat(path, { bigint: true })), 'storage file changed');
    return bytes.subarray(0, length);
  } finally { await file.close(); }
}

/** No shell grammar, comments, exports, duplicates or additional fields.
 * @param {string} path @returns {Promise<{accessKeyId:string,secretAccessKey:string}>}
 */
async function credentials(path) {
  const bytes = await snapshot(path, 16384, true);
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    const fields = new Map();
    for (const line of text.trimEnd().split('\n')) {
      const match = /^(OVH_ACCESS_KEY_ID|OVH_SECRET_ACCESS_KEY)=([A-Za-z0-9+/=_-]+)$/.exec(line);
      require(match && !fields.has(match[1]), 'invalid scoped credential fields');
      fields.set(match[1], match[2]);
    }
    require(fields.size === 2, 'incomplete scoped credentials');
    return { accessKeyId: fields.get('OVH_ACCESS_KEY_ID'), secretAccessKey: fields.get('OVH_SECRET_ACCESS_KEY') };
  } finally { bytes.fill(0); }
}
function sanitized(error) {
  const status = error?.$metadata?.httpStatusCode;
  const safe = new Error(Number.isInteger(status) && status >= 400 && status <= 599
    ? `private storage request failed (${status})` : 'private storage operation refused or unavailable');
  if (Number.isInteger(status) && status >= 400 && status <= 599) safe.$metadata = { httpStatusCode: status };
  return safe;
}

/** Explicit profile + binding only; requestHandler is a trusted test/embedding seam,
 * never a repository/CLI option. SDK redirect following and retrying are disabled.
 * @param {import('./capabilities.mjs').CapabilityProfile} profile
 * @param {import('./capabilities.mjs').RepositoryBinding} binding
 * @param {{role:'consumer'|'uploader',timeoutMs?:number,requestHandler?:object}} options
 */
export async function createPrivateS3Transport(profile, binding, { role, timeoutMs = 120000, requestHandler } = {}) {
  const authority = validateProfile(profile);
  const trusted = validateBinding(authority, binding);
  require(role === 'consumer' || role === 'uploader', 'explicit storage role required');
  require(Number.isSafeInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 120000, 'invalid storage timeout');
  let scopedCredentials;
  try { scopedCredentials = await credentials(role === 'consumer' ? authority.storage.consumerCredentialFile : authority.storage.uploaderCredentialFile); }
  catch { throw new Error('scoped credential file refused'); }
  const client = new S3Client({ region: authority.storage.region, endpoint: authority.storage.endpoint,
    credentials: scopedCredentials, forcePathStyle: true, maxAttempts: 1, followRegionRedirects: false,
    ...(requestHandler ? { requestHandler } : {}) });
  const Bucket = authority.storage.bucket;
  let closed = false;
  const active = new Set();
  async function operation(action) {
    require(!closed, 'storage transport closed');
    const controller = new AbortController();
    active.add(controller);
    let timer;
    const aborted = new Promise((_, reject) => {
      controller.signal.addEventListener('abort', () => reject(new Error('storage deadline exceeded')), { once: true });
      timer = setTimeout(() => controller.abort(), timeoutMs);
    });
    try { return await Promise.race([action(controller.signal), aborted]); }
    catch (error) { controller.abort(); throw sanitized(error); }
    finally { clearTimeout(timer); active.delete(controller); }
  }
  const key = Key => assertObjectKey(authority, trusted, Key);
  const transport = {
    head(Key) {
      key(Key);
      return operation(async signal => {
        try { await client.send(new HeadObjectCommand({ Bucket, Key }), { abortSignal: signal }); return true; }
        catch (error) { if (error?.$metadata?.httpStatusCode === 404) return false; throw error; }
      });
    },
    download(Key, path) {
      key(Key);
      return operation(async signal => {
        let body, file, output;
        try {
          const result = await client.send(new GetObjectCommand({ Bucket, Key }), { abortSignal: signal });
          body = result.Body;
          require(body && typeof body.destroy === 'function', 'missing storage response stream');
          require(!signal.aborted, 'storage deadline exceeded');
          require(result.ContentLength === undefined || (Number.isSafeInteger(result.ContentLength)
            && result.ContentLength >= 0 && result.ContentLength <= CAPS.maxSourceBytes), 'remote object size refused');
          file = await checkedOpen(path, constants.O_WRONLY | constants.O_CREAT, true);
          require(!signal.aborted, 'storage deadline exceeded');
          await file.truncate(0);
          let length = 0;
          const limit = new Transform({ transform(chunk, encoding, done) {
            length += chunk.length;
            done(length > CAPS.maxSourceBytes ? new Error('remote object exceeds size bound') : null, chunk);
          } });
          output = file.createWriteStream();
          await pipeline(body, limit, output, { signal });
        } finally { body?.destroy(); output?.destroy(); await file?.close(); }
      });
    },
    putIfAbsent(Key, path) {
      key(Key);
      return operation(async signal => {
        const bytes = await snapshot(path, CAPS.maxSourceBytes, false);
        require(!signal.aborted, 'storage deadline exceeded');
        await client.send(new PutObjectCommand({ Bucket, Key, Body: bytes, ContentLength: bytes.length, IfNoneMatch: '*' }), { abortSignal: signal });
      });
    }
  };
  const scoped = bindTransport(transport, authority, trusted, { readOnly: role === 'consumer' });
  return Object.freeze({ ...scoped, close() { closed = true; for (const controller of active) controller.abort(); client.destroy(); } });
}
