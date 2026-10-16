import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink, lstat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { renderSolidPng } from './adapters.mjs';
import { restoreRepository } from './restore.mjs';

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'restore-consumer-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const profile = { schemaVersion: 1, id: 'fixture', enabled: true,
    runner: { path: '/installed/runner', sha256: 'a'.repeat(64), version: 1 },
    storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://example.invalid',
      region: 'sbg', uploaderCredentialFile: '/operator/upload', consumerCredentialFile: '/operator/read' },
    allowedAssetTrees: ['/tmp'], consumers: [{ id: 'filesystem-restore', version: 1 }] };
  const binding = { schemaVersion: 1, profileId: 'fixture', repositoryId: 'repo', namespace: 'games/repo' };
  const bytes = renderSolidPng({ width: 32, height: 32, rgba: [1, 2, 3, 255] });
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const manifest = { schemaVersion: 1, bucket: 'dracon-lfs', namespace: binding.namespace,
    repository: binding.repositoryId, assets: [{ id: 'asset-1', sourcePath: 'public/icon.png', logicalPath: 'public/icon.png',
      sha256, bytes: bytes.length, bucketed: true, visibility: 'private', regenerable: true,
      objectKey: `${binding.namespace}/{sha256}.{ext}` }] };
  const calls = [];
  const transport = { async head() { throw new Error('unnecessary head'); }, async download(key, file) {
    calls.push(key); await writeFile(file, bytes, { mode: 0o600 });
  } };
  const request = { root, profile, binding, manifest, transport };
  return { ...request, request, bytes, calls, target: join(root, 'public/icon.png') };
}

test('consumer restore: fresh path verifies before create and matching local bytes need no network', async t => {
  const f = await fixture(t);
  assert.equal((await restoreRepository(f.request)).outcomes[0].status, 'restored');
  assert.deepEqual(await readFile(f.target), f.bytes);
  assert.equal(f.calls.length, 1);
  const before = await lstat(f.target);
  assert.equal((await restoreRepository(f.request)).outcomes[0].status, 'already-present');
  assert.equal((await lstat(f.target)).ino, before.ino);
  assert.equal(f.calls.length, 1);
});

test('consumer restore: differing operator bytes never overwritten or uploaded', async t => {
  const f = await fixture(t);
  await mkdir(join(f.root, 'public'));
  await writeFile(f.target, 'operator edits');
  await assert.rejects(restoreRepository(f.request), /mismatch/);
  assert.equal(await readFile(f.target, 'utf8'), 'operator edits');
  assert.equal(f.calls.length, 0);
  await assert.rejects(restoreRepository({ ...f.request, transport: { ...f.transport, async putIfAbsent() {} } }), /read-only/);
});

test('consumer restore: corruption or interruption leaves final destination absent and retry succeeds', async t => {
  const f = await fixture(t);
  await assert.rejects(restoreRepository({ ...f.request, transport: { ...f.transport,
    async download(key, file) { await writeFile(file, 'corrupt'); } } }), /mismatch/);
  await assert.rejects(readFile(f.target), { code: 'ENOENT' });
  await assert.rejects(restoreRepository({ ...f.request, transport: { ...f.transport,
    async download() { throw new Error('interrupted'); } } }), /interrupted/);
  await assert.rejects(readFile(f.target), { code: 'ENOENT' });
  assert.equal((await restoreRepository(f.request)).ok, true);
});

test('consumer restore: missing local-only asset is explicit and never requested remotely', async t => {
  const f = await fixture(t);
  f.manifest.assets[0].bucketed = false;
  await assert.rejects(restoreRepository(f.request), /missing-local-only/);
  assert.equal(f.calls.length, 0);
  await mkdir(join(f.root, 'public'));
  await writeFile(f.target, f.bytes);
  assert.equal((await restoreRepository(f.request)).ok, true);
});

test('consumer restore: unsafe entries and symlink destinations refuse without replacing target', async t => {
  const f = await fixture(t);
  f.manifest.assets[0].sourcePath = '../escape.png';
  await assert.rejects(restoreRepository(f.request), /consumer-entry/);
  f.manifest.assets[0].sourcePath = 'public/icon.png';
  const outside = await mkdtemp(join(tmpdir(), 'restore-outside-'));
  t.after(() => rm(outside, { recursive: true, force: true }));
  await symlink(outside, join(f.root, 'public'));
  await assert.rejects(restoreRepository(f.request), /unsafe-restore-directory/);
  await assert.rejects(readFile(join(outside, 'icon.png')), { code: 'ENOENT' });
  assert.equal(f.calls.length, 0);
});

test('consumer restore: concurrent differing destination creation is retained', async t => {
  const f = await fixture(t);
  const download = f.transport.download;
  f.transport.download = async (key, path) => { await download(key, path); await writeFile(f.target, 'concurrent edit'); };
  await assert.rejects(restoreRepository(f.request), /mismatch/);
  assert.equal(await readFile(f.target, 'utf8'), 'concurrent edit');
});
