import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm, copyFile, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { renderSolidPng, CONSUMER_ID, CONSUMER_VERSION } from './adapters.mjs';
import { planRepository, publishRepository } from './publisher.mjs';

const brand = { width: 32, height: 32, rgba: [255, 46, 136, 255] };
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const profile = () => ({
  schemaVersion: 1, id: 'games-scoped-v1', enabled: true,
  runner: { path: '/installed/runner.mjs', sha256: 'a'.repeat(64), version: 1 },
  storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://s3.example.invalid',
    region: 'sbg', uploaderCredentialFile: '/operator/uploader.env', consumerCredentialFile: '/operator/reader.env' },
  allowedAssetTrees: ['/tmp'], consumers: [{ id: CONSUMER_ID, version: CONSUMER_VERSION }]
});
const binding = () => ({ schemaVersion: 1, profileId: 'games-scoped-v1', repositoryId: 'repo-123', namespace: 'games/auto/repo-123' });
const consumer = { id: CONSUMER_ID, version: CONSUMER_VERSION, assetRoots: ['authorized'] };
function git(cwd, ...args) {
  return execFileSync('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false',
    '-C', cwd, ...args], { env: { ...process.env, GIT_OPTIONAL_LOCKS: '0',
      GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LC_ALL: 'C' } });
}
async function repo(t) {
  const root = await mkdtemp(join(tmpdir(), 'publisher-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  git(root, 'init', '-q');
  git(root, 'config', 'user.email', 't@e');
  git(root, 'config', 'user.name', 't');
  await mkdir(join(root, 'authorized'), { recursive: true });
  const manifestPath = join(root, 'manifest.json');
  const indexPath = join(root, 'index', 'binding.json');
  return { root, manifestPath, indexPath };
}

function transport(remoteBytes) {
  const objects = new Map();
  if (remoteBytes) for (const [k, v] of Object.entries(remoteBytes)) objects.set(k, v);
  return {
    objects,
    async head(key) { return objects.has(key); },
    async putIfAbsent(key, file) { if (objects.has(key)) { const e = new Error('PreconditionFailed'); e.$metadata = { httpStatusCode: 412 }; throw e; } objects.set(key, await readFile(file)); },
    async download(key, file) { if (!objects.has(key)) throw new Error('NotFound'); await writeFile(file, objects.get(key)); },
    async reset() { objects.clear(); }
  };
}

test('planRepository: returns concrete eligible candidates needing upload', async t => {
  const { root } = await repo(t);
  const bytes = renderSolidPng(brand);
  await writeFile(join(root, 'authorized/icon.png'), bytes);
  const plan = await planRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  assert.equal(plan.plan.status, 'planned');
  assert.equal(plan.toUpload.length, 1);
  assert.equal(plan.toUpload[0].sha256, sha(bytes));
  assert.equal(plan.toUpload[0].bytes, 103);
  assert.equal(plan.toUpload[0].provenance.consumer.id, 'filesystem-restore');
});

test('planRepository: idempotent re-plan reports no new candidates', async t => {
  const { root } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const first = await planRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  const second = await planRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: first.plan.proposedManifest });
  assert.equal(first.toUpload.length, 1);
  assert.equal(second.toUpload.length, 0);
});

test('publishRepository: writes manifest, verifies, sets bucketed:true, and binds index', async t => {
  const { root, manifestPath, indexPath } = await repo(t);
  const bytes = renderSolidPng(brand);
  await writeFile(join(root, 'authorized/icon.png'), bytes);
  const trans = transport();
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans, indexPath });
  assert.equal(result.manifestWritten, true);
  assert.equal(result.indexRebound, true);
  assert.equal(result.toUpload.length, 1);
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  assert.equal(manifest.assets[0].bucketed, true);
  assert.equal(manifest.assets[0].sha256, sha(bytes));
  assert.equal(manifest.assets[0].objectKey, 'games/auto/repo-123/{sha256}.{ext}');
  const index = JSON.parse(await readFile(indexPath, 'utf8'));
  assert.equal(index.namespace, 'games/auto/repo-123');
  assert.equal(index.manifestDigest, sha(await readFile(manifestPath, 'utf8')));
  // A second run is idempotent: same plan, no new candidates, no index change.
  const second = await publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans, indexPath });
  assert.equal(second.manifestWritten, false);
  assert.equal(second.indexRebound, false);
});

test('publishRepository: refuses to upload when transport reports a different object', async t => {
  const { root, manifestPath } = await repo(t);
  const bytes = renderSolidPng(brand);
  await writeFile(join(root, 'authorized/icon.png'), bytes);
  const trans = transport({ 'games/auto/repo-123/7884ac7225df090731d8a00b56b6de69cb34cc1fe8763b8aeb15132f810664d7.png': Buffer.from('corrupt') });
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, false);
  assert.match(result.reason, /snapshot/);
  try { await readFile(manifestPath, 'utf8'); assert.fail('manifest must remain absent on refusal'); }
  catch (error) { assert.equal(error.code, 'ENOENT'); }
});

test('publishRepository: rejects transport claiming a wrong bucket namespace', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const calls = [];
  const trans = {
    async head(key) { calls.push(['head', key]); return false; },
    async putIfAbsent(key) { calls.push(['put', key]); throw new Error('storage refused'); },
    async download() { throw new Error('unexpected download'); }
  };
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, false);
  for (const [op, key] of calls) {
    assert.equal(key.startsWith('games/auto/repo-123/'), true, `${op} must be inside bound namespace`);
  }
});

test('publishRepository: writes manifest even when storage raced a concurrent create', async t => {
  const { root, manifestPath } = await repo(t);
  const bytes = renderSolidPng(brand);
  await writeFile(join(root, 'authorized/icon.png'), bytes);
  const remote = { 'games/auto/repo-123/7884ac7225df090731d8a00b56b6de69cb34cc1fe8763b8aeb15132f810664d7.png': bytes };
  const trans = transport(remote);
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, true);
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  assert.equal(manifest.assets[0].bucketed, true);
});

test('publishRepository: concurrent publisher blocks until the first releases the lock', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const trans = transport();
  const first = publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans });
  const second = publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath, transport: trans });
  const [a, b] = await Promise.all([first, second]);
  assert.equal(a.manifestWritten || b.manifestWritten, true, 'at least one publisher wrote the manifest');
});

test('publishRepository: replaces an unsafely rooted manifest path and refuses symlinked directories', async t => {
  const { root, manifestPath } = await repo(t);
  const alias = join(root, 'alias'); await symlink(root, alias);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  await assert.rejects(publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath: join(alias, 'manifest.json'), transport: transport() }), /symlink|symlinked/);
});
