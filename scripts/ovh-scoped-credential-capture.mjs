#!/usr/bin/env node
/** One-time local capture bridge for an authorized OVH Manager session.
 * Browser sends RSA-OAEP ciphertext, not credentials in tool output or HTTP.
 * Runs for at most 10 minutes, binds loopback, and never logs request bodies.
 */
import { createServer } from 'node:http';
import { generateKeyPairSync, privateDecrypt, constants, randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync, chmodSync, existsSync, lstatSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(process.env.HOME, '.dracon');
const roles = new Set(['uploader', 'consumer']);
const token = randomBytes(24).toString('hex');
const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 4096 });
const key = publicKey.export({ type: 'spki', format: 'der' }).toString('base64');
const state = {};
const run = (program, args, input) => {
  const result = spawnSync(program, args, { cwd: root, input, timeout: 30000, maxBuffer: 4 * 1024 * 1024 });
  if (result.status !== 0 || result.error) throw new Error('Credential verification command refused');
  return result.stdout;
};
function capture(role, ciphertext) {
  if (state[role]) throw new Error('Role already captured');
  const bytes = privateDecrypt({ key: privateKey, padding: constants.RSA_PKCS1_OAEP_PADDING, oaepHash: 'sha256' }, Buffer.from(ciphertext, 'base64'));
  const data = JSON.parse(bytes.toString('utf8'));
  if (typeof data.access !== 'string' || typeof data.secret !== 'string' || !/^[a-zA-Z0-9/+_=.-]+$/.test(data.access) || !/^[a-zA-Z0-9/+_=.-]+$/.test(data.secret)) throw new Error('Invalid credential shape');
  const plaintext = Buffer.from(`OVH_ACCESS_KEY_ID=${data.access}\nOVH_SECRET_ACCESS_KEY=${data.secret}\n`);
  const name = `secrets/ovh/dracon-lfs-${role}.env`;
  const path = join(root, name);
  if (existsSync(path)) throw new Error('Existing credentials will not be replaced');
  const encrypted = run('dracon-warden', ['filter-clean', name], plaintext);
  if (!encrypted.includes('[DRACON_SECRET:') || encrypted.includes(data.access) || encrypted.includes(data.secret)) throw new Error('Encryption refused');
  const local = run('dracon-warden', ['filter-smudge', name], encrypted);
  if (!local.equals(plaintext)) throw new Error('Local recovery refused');
  // Prove the dedicated master recipient can recover EVERY encrypted line.
  const recovered = encrypted.toString('utf8').replace(/\[DRACON_SECRET:([^\]]+)\]/g, (_, payload) => {
    return run('age', ['-d', '-i', join(root, 'keys/master.age')], Buffer.from(payload, 'base64')).toString('utf8');
  });
  if (!Buffer.from(recovered).equals(plaintext)) throw new Error('Master recovery refused');
  for (const dir of [root, join(root, 'secrets')]) if (lstatSync(dir).isSymbolicLink()) throw new Error('Unsafe recovery directory');
  const directory = join(root, 'secrets/ovh');
  if (existsSync(directory) && lstatSync(directory).isSymbolicLink()) throw new Error('Unsafe credential directory');
  mkdirSync(directory, { recursive: true, mode: 0o700 }); chmodSync(directory, 0o700);
  // Start with encrypted worktree bytes: no daemon race can stage plaintext.
  writeFileSync(path, encrypted, { mode: 0o600, flag: 'wx' });
  run('git', ['add', '--', name]);
  const staged = run('git', ['show', `:${name}`]);
  if (!staged.includes('[DRACON_SECRET:') || staged.includes(data.access) || staged.includes(data.secret)) throw new Error('Encrypted index proof refused');
  // Normal runtime view, analogous to filter smudge, remains mode 0600.
  writeFileSync(path, plaintext, { mode: 0o600 }); chmodSync(path, 0o600);
  state[role] = { encrypted: true, localRecovery: true, masterRecovery: true, indexEncrypted: true };
  bytes.fill(0); plaintext.fill(0);
}
const server = createServer(async (request, response) => {
  response.setHeader('Access-Control-Allow-Origin', 'https://manager.eu.ovhcloud.com');
  response.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  response.setHeader('Access-Control-Allow-Headers', 'Content-Type');
  response.setHeader('Access-Control-Allow-Private-Network', 'true');
  response.setHeader('Cache-Control', 'no-store');
  if (request.method === 'OPTIONS') { response.writeHead(204); response.end(); return; }
  if (request.url !== `/${token}`) { response.writeHead(404); response.end(); return; }
  if (request.method === 'GET') { response.end(JSON.stringify({ publicKey: key, state })); return; }
  try {
    let body = '';
    for await (const chunk of request) { body += chunk; if (body.length > 4096) throw new Error('Oversized ciphertext'); }
    const data = JSON.parse(body);
    if (!roles.has(data.role) || typeof data.ciphertext !== 'string') throw new Error('Invalid capture');
    capture(data.role, data.ciphertext);
    response.end(JSON.stringify({ ok: true, state: state[data.role] }));
  } catch { response.writeHead(400); response.end(JSON.stringify({ ok: false, code: 'SECURE_CAPTURE_REFUSED' })); }
});
server.listen(19083, '127.0.0.1', () => {
  // Contains public key and one-time endpoint only; no private key/credentials.
  writeFileSync('/tmp/dracon-ovh-capture-public.json', JSON.stringify({ endpoint: `http://127.0.0.1:19083/${token}`, publicKey: key }), { mode: 0o600 });
  console.log('Scoped capture ready (public metadata in /tmp/dracon-ovh-capture-public.json)');
});
setTimeout(() => server.close(), 10 * 60 * 1000);
