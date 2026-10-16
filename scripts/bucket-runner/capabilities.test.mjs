import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, chmod, symlink, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validateProfile, loadProfile, verifyInstalledRunner, assetAuthorized,
  validateBinding, assertObjectKey, bindTransport } from './capabilities.mjs';

const profile = () => ({
  schemaVersion: 1, id: 'games-scoped-v1', enabled: true,
  runner: { path: '/installed/runner.mjs', sha256: 'a'.repeat(64), version: 1 },
  storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://s3.example.invalid',
    region: 'sbg', uploaderCredentialFile: '/operator/uploader.env', consumerCredentialFile: '/operator/reader.env' },
  allowedAssetTrees: ['/fleet/platform/web/games'], consumers: [{ id: 'filesystem-restore', version: 1 }]
});
const binding = () => ({ schemaVersion: 1, profileId: 'games-scoped-v1', repositoryId: 'repo-123', namespace: 'games/auto/repo-123' });
const key = `games/auto/repo-123/${'1'.repeat(64)}.png`;

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'bucket-capabilities-'));
  t.after(() => rm(dir, { force: true, recursive: true }));
  const runner = join(dir, 'runner.mjs'), path = join(dir, 'operator.json');
  const bytes = Buffer.from('export const syntheticRunnerFixture = true;\n');
  await writeFile(runner, bytes, { mode: 0o644 });
  const data = profile(); data.runner.path = runner;
  data.runner.sha256 = createHash('sha256').update(bytes).digest('hex');
  await writeFile(path, JSON.stringify(data), { mode: 0o600 });
  return { dir, path, runner, data };
}

test('profile authority is strict, immutable and independent of caller mutation', () => {
  const input = profile(), authority = validateProfile(input);
  input.storage.namespaceRoot = 'outside'; input.allowedAssetTrees.push('/');
  assert.equal(authority.storage.namespaceRoot, 'games');
  assert.deepEqual(authority.allowedAssetTrees, ['/fleet/platform/web/games']);
  assert.throws(() => { authority.storage.namespaceRoot = 'outside'; });
  for (const mutate of [
    p => { p.extra = 'repository-controlled'; }, p => { p.schemaVersion = 2; },
    p => { p.enabled = 'yes'; }, p => { p.runner.sha256 = 'BAD'; },
    p => { p.runner.path = 'repo/script.mjs'; }, p => { p.runner.version = 2; },
    p => { p.storage.bucket = 'dracon-master'; }, p => { p.storage.namespaceRoot = 'games/../outside'; },
    p => { p.storage.endpoint = 'http://storage.invalid'; },
    p => { p.storage.endpoint = 'https://user:secret@storage.invalid'; },
    p => { p.storage.endpoint = 'https://storage.invalid/?secret=1'; },
    p => { p.storage.consumerCredentialFile = p.storage.uploaderCredentialFile; },
    p => { p.storage.uploaderCredentialFile = '../credentials.env'; },
    p => { p.allowedAssetTrees = ['/']; }, p => { p.allowedAssetTrees = ['/fleet/../outside']; },
    p => { p.consumers = [{ id: 'unqualified', version: 1 }]; }
  ]) {
    const value = profile(); mutate(value); assert.throws(() => validateProfile(value));
  }
});

test('authorized subtrees cover new nested repositories without a per-repo registry', () => {
  const p = profile();
  assert.equal(assetAuthorized(p, '/fleet/platform', 'web/games/static/favicon.png'), true);
  assert.equal(assetAuthorized(p, '/fleet/platform/web/games/wip/new-game', 'static/new.png'), true);
  for (const [root, path] of [
    ['/fleet/other', 'static/new.png'], ['/fleet/platform', 'web/games-elsewhere/image.png'],
    ['/fleet/platform', '../escape.png'], ['/fleet/platform', '/absolute.png'],
    ['/fleet/platform', 'web/games/../private.png'], ['/fleet/platform', 'web/games/a\\b.png']
  ]) assert.equal(assetAuthorized(p, root, path), false);
  assert.equal(assetAuthorized({ ...p, enabled: false }, '/fleet/platform', 'web/games/static/favicon.png'), false);
});

test('every object request is bound to trusted namespace, not manifest claims', async () => {
  const calls = [];
  const transport = Object.fromEntries(['head', 'download', 'putIfAbsent'].map(method => [method, (...args) => { calls.push([method, ...args]); return Promise.resolve(true); }]));
  const p = profile(), b = binding();
  assert.equal(validateBinding(p, b).repositoryId, 'repo-123');
  const scoped = bindTransport(transport, p, b);
  b.namespace = 'outside'; p.storage.namespaceRoot = 'outside';
  await scoped.head(key); await scoped.download(key, '/cache/file'); await scoped.putIfAbsent(key, '/snapshot/file');
  assert.equal(calls.length, 3);
  for (const bad of [
    `games/other/${'1'.repeat(64)}.png`, `games/auto/repo-123-other/${'1'.repeat(64)}.png`,
    `games/auto/repo-123/../${'1'.repeat(64)}.png`, `${key}?redirect=outside`,
    `games/auto/repo-123/${'1'.repeat(12)}.png`, key.replace('.png', '.key'), key.replace('.png', '.ts')
  ]) {
    for (const method of ['head', 'download', 'putIfAbsent']) assert.throws(() => scoped[method](bad, '/file'));
  }
  assert.equal(calls.length, 3, 'refused requests never reach transport');
  assert.equal(scoped.delete, undefined); assert.equal(scoped.listAllBuckets, undefined);
  assert.throws(() => assertObjectKey(profile(), { ...binding(), profileId: 'wrong' }, key));
  assert.throws(() => assertObjectKey(profile(), { ...binding(), namespace: 'other' }, key));
  assert.throws(() => assertObjectKey({ ...profile(), enabled: false }, binding(), key));
});

test('consumer transport has no write operation even if underlying client can write', async () => {
  let writes = 0;
  const scoped = bindTransport({ head: async () => true, download: async () => true,
    putIfAbsent: async () => { writes++; } }, profile(), binding(), { readOnly: true });
  await scoped.head(key); await scoped.download(key, '/file');
  assert.equal(scoped.putIfAbsent, undefined); assert.equal(writes, 0);
});

test('operator loader enforces ownership permissions and runner hash without reading credential files', async t => {
  const f = await fixture(t);
  // Credential paths intentionally do not exist. Selection must not open them.
  const loaded = await loadProfile(f.path);
  assert.equal((await verifyInstalledRunner(loaded)).sha256, f.data.runner.sha256);
  await chmod(f.path, 0o644);
  await assert.rejects(loadProfile(f.path), /private/);
  await chmod(f.path, 0o600);
  await writeFile(f.runner, 'tampered runner');
  await assert.rejects(verifyInstalledRunner(loaded), /digest mismatch/);
  await chmod(f.runner, 0o666);
  await assert.rejects(verifyInstalledRunner(loaded), /writable/);
});

test('operator loader refuses symlink ancestors/leaves, malformed and oversized profiles', async t => {
  const f = await fixture(t);
  const link = join(f.dir, 'link.json'); await symlink(f.path, link);
  await assert.rejects(loadProfile(link), /symlink/);
  const real = join(f.dir, 'real'); await mkdir(real);
  await writeFile(join(real, 'profile.json'), JSON.stringify(f.data), { mode: 0o600 });
  const alias = join(f.dir, 'alias'); await symlink(real, alias);
  await assert.rejects(loadProfile(join(alias, 'profile.json')), /symlink/);
  await writeFile(f.path, 'invalid JSON'); await assert.rejects(loadProfile(f.path), /JSON/);
  await writeFile(f.path, ' '.repeat(65537)); await assert.rejects(loadProfile(f.path), /size bound/);
});
