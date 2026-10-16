import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm, copyFile, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { renderSolidPng, CONSUMER_ID, CONSUMER_VERSION } from './adapters.mjs';
import { planRepository as planWithBudget, publishRepository as publishWithBudget, publicationLock } from './publisher.mjs';

// Upload fixtures explicitly model measured pressure; missing real budgets fail closed.
const pressureBudget = { projectedBytes: 800, limitBytes: 1000 };
const planRepository = args => planWithBudget({ budget: pressureBudget, ...args });
const publishRepository = args => publishWithBudget({ budget: pressureBudget, ...args });

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

test('planRepository: idempotent re-plan retains unverified pending uploads', async t => {
  const { root } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const first = await planRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  const second = await planRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: first.plan.proposedManifest });
  assert.equal(first.toUpload.length, 1);
  // Planning is not upload verification: an unbucketed pending entry must
  // remain due on retry, while registration itself is byte-for-byte stable.
  assert.equal(second.toUpload.length, 1);
  assert.deepEqual(second.plan.proposedManifest, first.plan.proposedManifest);
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
  assert.ok(/snapshot|invalid-evidence|invalid|uploaded|evid/.test(result.reason || ''), `unexpected refusal: ${result.reason}`);
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

test('publicationLock: readiness waits for kernel ownership, not a grace timer', async t => {
  const { root } = await repo(t);
  const path = join(root, 'publication.lock');
  const release = await publicationLock(path);
  let acquired = false;
  const second = publicationLock(path, { waitMs: 1500 }).then(unlock => { acquired = true; return unlock; });
  try {
    await new Promise(resolve => setTimeout(resolve, 350));
    assert.equal(acquired, false, 'second writer must not enter while first owns the lock');
  } finally { await release(); }
  await (await second)();
});

test('publicationLock: rejects symlink and timeout without leaking ownership', async t => {
  const { root } = await repo(t);
  const path = join(root, 'publication.lock');
  const release = await publicationLock(path);
  try {
    await assert.rejects(publicationLock(path, { waitMs: 50 }), /publication lock held/);
    const link = join(root, 'alias.lock');
    await symlink(path, link);
    await assert.rejects(publicationLock(link), /ELOOP/);
  } finally { await release(); }
  await (await publicationLock(path))();
});

test('publishRepository: successful PUT with corrupt stored bytes cannot publish', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const trans = transport();
  trans.putIfAbsent = async key => { trans.objects.set(key, Buffer.from('corrupt')); };
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, false);
  await assert.rejects(readFile(manifestPath), { code: 'ENOENT' });
  assert.deepEqual(await readFile(join(root, 'authorized/icon.png')), renderSolidPng(brand));
});

test('publishRepository: concurrent creation of previously absent manifest is retained', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const trans = transport();
  const put = trans.putIfAbsent;
  const operatorBytes = '{"operatorEdit":true}\n';
  trans.putIfAbsent = async (key, file) => {
    await put(key, file);
    await writeFile(manifestPath, operatorBytes);
  };
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, false);
  assert.equal(result.reason, 'manifest-changed-concurrently');
  assert.equal(await readFile(manifestPath, 'utf8'), operatorBytes);
});

test('publishRepository: raced source is revalidated before any upload', async t => {
  const { root, manifestPath } = await repo(t);
  const path = join(root, 'authorized/icon.png');
  await writeFile(path, renderSolidPng(brand));
  const trans = transport();
  trans.head = async () => { await writeFile(path, Buffer.from('operator edit')); return false; };
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, false);
  assert.equal(trans.objects.size, 0, 'changed bytes must never be uploaded');
  assert.equal(await readFile(path, 'utf8'), 'operator edit');
});

test('publishRepository: generator changes during upload prevent stale references', async t => {
  const { root, manifestPath } = await repo(t);
  const path = join(root, 'authorized/icon.png');
  await writeFile(path, renderSolidPng(brand));
  const trans = transport();
  const download = trans.download;
  trans.download = async (key, file) => {
    await download(key, file);
    await writeFile(path, renderSolidPng({ ...brand, rgba: [1, 2, 3, 255] }));
  };
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(result.manifestWritten, false);
  await assert.rejects(readFile(manifestPath), { code: 'ENOENT' });
  assert.deepEqual(await readFile(path), renderSolidPng({ ...brand, rgba: [1, 2, 3, 255] }));
});

test('publishRepository: rejects manifest escape and symlink ancestors before transport', async t => {
  const { root } = await repo(t);
  const outside = await mkdtemp(join(tmpdir(), 'publisher-outside-'));
  t.after(() => rm(outside, { recursive: true, force: true }));
  const trans = transport();
  await assert.rejects(publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath: join(outside, 'manifest.json'), transport: trans }), /Unsafe/);
  await symlink(outside, join(root, 'alias'));
  await assert.rejects(publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath: join(root, 'alias', 'manifest.json'), transport: trans }), /Symlink/);
  assert.equal(trans.objects.size, 0);
  await assert.rejects(readFile(join(outside, 'manifest.json')), { code: 'ENOENT' });
});

test('publishRepository: small assets register locally then route when measured pressure rises', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const trans = transport();
  const local = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans, budget: { projectedBytes: 10, limitBytes: 1000 } });
  assert.equal(local.manifestWritten, true);
  assert.equal(local.toUpload.length, 0);
  assert.equal(trans.objects.size, 0);
  const entry = JSON.parse(await readFile(manifestPath, 'utf8')).assets[0];
  assert.equal(entry.bucketed, false);
  const later = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(later.manifestWritten, true);
  assert.equal(later.toUpload.length, 1);
  const routed = JSON.parse(await readFile(manifestPath, 'utf8')).assets[0];
  assert.equal(routed.id, entry.id);
  assert.equal(routed.bucketed, true);
});

test('publishRepository: absent routing budget refuses before storage or manifest mutation', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  const trans = transport();
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans, budget: undefined });
  assert.equal(result.manifestWritten, false);
  assert.match(result.reason, /routing-budget/);
  assert.equal(trans.objects.size, 0);
  await assert.rejects(readFile(manifestPath), { code: 'ENOENT' });
});

test('publishRepository: explicit large threshold routes only qualified assets', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  await writeFile(join(root, 'authorized/not-media.txt'), Buffer.alloc(200));
  const trans = transport();
  const result = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans, budget: { projectedBytes: 10, limitBytes: 1000, largeBytes: 100 } });
  assert.equal(result.toUpload.length, 1);
  assert.equal(trans.objects.size, 1);
  assert.equal(JSON.parse(await readFile(manifestPath, 'utf8')).assets.length, 1);
});

test('publishRepository: changed managed asset renews proof and preserves old immutable object', async t => {
  const { root, manifestPath } = await repo(t);
  const source = join(root, 'authorized/icon.png');
  const original = renderSolidPng(brand);
  await writeFile(source, original);
  const trans = transport();
  const first = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(first.manifestWritten, true);
  const before = JSON.parse(await readFile(manifestPath, 'utf8')).assets[0];
  const changed = renderSolidPng({ ...brand, rgba: [2, 3, 4, 255] });
  await writeFile(source, changed);
  // Once bucketed, a changed eligible asset stays routed even at low pressure.
  const second = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans, budget: { projectedBytes: 10, limitBytes: 1000 } });
  assert.equal(second.manifestWritten, true);
  assert.equal(second.toUpload.length, 1);
  const after = JSON.parse(await readFile(manifestPath, 'utf8')).assets[0];
  assert.equal(after.id, before.id);
  assert.equal(after.sha256, sha(changed));
  assert.equal(after.provenance.sha256, sha(changed));
  assert.equal(after.bucketed, true);
  assert.deepEqual(trans.objects.get(`games/auto/repo-123/${before.sha256}.png`), original);
  assert.deepEqual(trans.objects.get(`games/auto/repo-123/${after.sha256}.png`), changed);
  assert.deepEqual(await readFile(source), changed);
});

test('publishRepository: unsupported edits retain previous manifest and edited source', async t => {
  const { root, manifestPath } = await repo(t);
  const source = join(root, 'authorized/icon.png');
  await writeFile(source, renderSolidPng(brand));
  const trans = transport();
  await publishRepository({ root, profile: profile(), binding: binding(), consumer, manifestPath, transport: trans });
  const before = await readFile(manifestPath);
  await writeFile(source, Buffer.from('unsupported operator bytes'));
  const second = await publishRepository({ root, profile: profile(), binding: binding(), consumer,
    manifestPath, transport: trans });
  assert.equal(second.manifestWritten, false);
  assert.equal(second.plan.outcomes[0].status, 'unsupported');
  assert.deepEqual(await readFile(manifestPath), before);
  assert.equal(await readFile(source, 'utf8'), 'unsupported operator bytes');
  assert.equal(trans.objects.size, 1);
});

test('publishRepository: refuses an unsafely rooted manifest path', async t => {
  const { root, manifestPath } = await repo(t);
  await writeFile(join(root, 'authorized/icon.png'), renderSolidPng(brand));
  await assert.rejects(publishRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.',
    manifestPath: 'relative.json', transport: transport() }), /invalid-manifest-path/);
});
