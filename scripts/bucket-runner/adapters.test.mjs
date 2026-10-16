import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deflateSync } from 'node:zlib';
import { renderSolidPng, probeCandidate, verifyEvidence, CAPS, CONSUMER_ID, CONSUMER_VERSION } from './adapters.mjs';

const brand = { width: 32, height: 32, rgba: [255, 46, 136, 255] };
const sha = (b) => createHash('sha256').update(b).digest('hex');
async function fixture(t, recipe = brand) {
  const root = await mkdtemp(join(tmpdir(), 'solid-adapter-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, 'assets'));
  const bytes = renderSolidPng(recipe);
  await writeFile(join(root, 'assets/icon.png'), bytes);
  const context = { root, repositoryId: 'repo-123', relativePath: 'assets/icon.png', consumer: { id: CONSUMER_ID, version: CONSUMER_VERSION, assetRoots: ['assets'] } };
  return { root, bytes, context };
}
function crc32(b) {
  let c = 0xffffffff;
  for (const x of b) { c ^= x; for (let i = 0; i < 8; i++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1)); }
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const b = Buffer.alloc(data.length + 12); b.writeUInt32BE(data.length); b.write(type, 4); data.copy(b, 8);
  b.writeUInt32BE(crc32(b.subarray(4, -4)), b.length - 4); return b;
}
function customPng(raw, { width = 32, height = 32, level = 6, interlace = 0 } = {}) {
  const h = Buffer.alloc(13); h.writeUInt32BE(width); h.writeUInt32BE(height, 4); h[8] = 8; h[9] = 6; h[12] = interlace;
  return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', h), chunk('IDAT', deflateSync(raw, { level })), chunk('IEND', Buffer.alloc(0))]);
}
test('renderSolidPng: actual brand golden and bounded numeric recipe', () => {
  const bytes = renderSolidPng(brand);
  assert.equal(bytes.length, 103);
  assert.equal(sha(bytes), '7884ac7225df090731d8a00b56b6de69cb34cc1fe8763b8aeb15132f810664d7');
  assert.deepEqual(renderSolidPng({ width: 1, height: 2, rgba: [1, 2, 3, 0] }), customPng(Buffer.from([0, 1, 2, 3, 0, 0, 1, 2, 3, 0]), { width: 1, height: 2 }));
  for (const r of [{ ...brand, width: 0 }, { ...brand, width: 513 }, { ...brand, height: 1.5 }, { ...brand, rgba: [256, 0, 0, 0] }, { ...brand, rgba: Array(4) }, { ...brand, data: 'opaque' }]) assert.throws(() => renderSolidPng(r));
});
test('probeCandidate: deterministic evidence and other recipes', async (t) => {
  for (const recipe of [brand, { width: 7, height: 19, rgba: [3, 0, 249, 12] }]) {
    const { bytes, context } = await fixture(t, recipe);
    const a = await probeCandidate(context); const b = await probeCandidate(context);
    assert.equal(a.status, 'eligible'); assert.deepEqual(a, b); assert.deepEqual(a.evidence.recipe, recipe);
    assert.equal(verifyEvidence(a.evidence, bytes, context), sha(bytes));
  }
});
test('probeCandidate: malformed, bomb, oversized, non-solid and noncanonical PNGs', async (t) => {
  const { context, root, bytes } = await fixture(t);
  const raw = Buffer.alloc(32 * 129);
  const filtered = Buffer.from(raw); filtered[0] = 1;
  const nonsolid = Buffer.from(raw); nonsolid[8] = 1;
  const crcBad = Buffer.from(bytes); crcBad[29] ^= 1;
  const cases = [Buffer.from('not png'), bytes.subarray(0, 50), Buffer.concat([bytes, Buffer.from([0])]), crcBad,
    customPng(filtered), customPng(nonsolid), customPng(raw, { level: 0 }), customPng(raw, { interlace: 1 }),
    customPng(Buffer.alloc(100000), { width: 1, height: 1 }), customPng(raw, { width: 0xffffffff }), Buffer.alloc(CAPS.maxSourceBytes + 1)];
  for (const candidate of cases) {
    await writeFile(join(root, context.relativePath), candidate);
    assert.equal((await probeCandidate(context)).status, 'unsupported');
  }
});
test('verifyEvidence: edits and every authority/provenance field fail closed', async (t) => {
  const { bytes, context } = await fixture(t);
  const { evidence } = await probeCandidate(context);
  const changes = { schemaVersion: 2, adapterId: 'unknown', adapterVersion: 2, repositoryId: 'wrong', sourcePath: 'assets/other.png',
    sha256: '0'.repeat(64), bytes: bytes.length + 1, recipe: { ...brand, width: 31 }, recipeDigest: '0'.repeat(64),
    toolchain: { ...evidence.toolchain, node: 'other' }, inputDigests: ['untrusted'], consumer: { id: 'unknown', version: 1 } };
  for (const [key, value] of Object.entries(changes)) assert.throws(() => verifyEvidence({ ...evidence, [key]: value }, bytes, context), key);
  assert.throws(() => verifyEvidence({ ...evidence, extra: true }, bytes, context));
  assert.throws(() => verifyEvidence({ ...evidence, toolchain: { ...evidence.toolchain, zlib: 'other' } }, bytes, context));
  assert.throws(() => verifyEvidence(evidence, bytes, { ...context, repositoryId: 'different-owner' }));
  assert.throws(() => verifyEvidence(evidence, bytes, { ...context, consumer: { ...context.consumer, assetRoots: ['elsewhere'] } }));
  const edited = renderSolidPng({ ...brand, rgba: [0, 0, 0, 0] });
  assert.throws(() => verifyEvidence(evidence, edited, context));
  await writeFile(join(context.root, context.relativePath), edited);
  const renewed = await probeCandidate(context);
  assert.equal(renewed.status, 'eligible'); assert.notEqual(renewed.evidence.sha256, evidence.sha256);
});
test('probeCandidate: traversal, exclusions, consumers, symlinks and nested ownership', async (t) => {
  const { context, root, bytes } = await fixture(t);
  for (const relativePath of ['../icon.png', '/tmp/icon.png', 'assets/../icon.png', 'assets//icon.png', 'assets\\icon.png', 'assets/.secret/icon.png', 'assets/build/icon.png', 'assets/tests/icon.png', 'assets/audit/icon.png', 'assets/icon.png\0']) {
    assert.equal((await probeCandidate({ ...context, relativePath })).status, 'refused', relativePath);
  }
  assert.equal((await probeCandidate({ ...context, relativePath: 'assets/icon.jpg' })).status, 'unsupported');
  for (const consumer of [undefined, { id: 'arbitrary', version: 1, assetRoots: ['assets'] }, { ...context.consumer, version: 2 }]) assert.equal((await probeCandidate({ ...context, consumer })).status, 'unsupported');
  assert.equal((await probeCandidate({ ...context, consumer: { ...context.consumer, assetRoots: ['other'] } })).status, 'refused');
  await symlink(join(root, 'assets/icon.png'), join(root, 'assets/link.png'));
  assert.equal((await probeCandidate({ ...context, relativePath: 'assets/link.png' })).reason, 'symlink-path');
  await symlink(join(root, 'assets'), join(root, 'alias'));
  assert.equal((await probeCandidate({ ...context, relativePath: 'alias/icon.png', consumer: { ...context.consumer, assetRoots: ['alias'] } })).reason, 'symlink-path');
  assert.equal((await probeCandidate({ ...context, root: join(root, 'alias'), relativePath: 'assets/icon.png' })).reason, 'symlink-path');
  await mkdir(join(root, 'assets/nested'));
  await writeFile(join(root, 'assets/nested/.git'), 'gitdir: elsewhere');
  await writeFile(join(root, 'assets/nested/icon.png'), bytes);
  assert.equal((await probeCandidate({ ...context, relativePath: 'assets/nested/icon.png' })).reason, 'nested-git');
});
test('probeCandidate: missing and concurrently changed source are retryable', async (t) => {
  const { context, root } = await fixture(t, { width: 512, height: 512, rgba: [1, 2, 3, 4] });
  assert.deepEqual(await probeCandidate({ ...context, relativePath: 'assets/missing.png' }), { status: 'retryable', reason: 'source-disappeared' });
  let running = true;
  const writer = (async () => { while (running) await writeFile(join(root, context.relativePath), Buffer.alloc(4096, 7)); })();
  let changed = false;
  try {
    for (let i = 0; i < 40 && !changed; i++) changed = (await probeCandidate(context)).status === 'retryable';
  } finally { running = false; await writer; }
  assert.equal(changed, true, 'concurrent mutation must not gain eligibility');
});
