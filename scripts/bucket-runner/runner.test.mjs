import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, copyFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRunner, validateRequest, LIMIT, DESCRIPTOR } from './runner.mjs';
import { MANIFEST, PREBUILD } from './consumer.mjs';
import { renderSolidPng } from './adapters.mjs';
const canonical = v => `${JSON.stringify(v, null, 2)}\n`;
const hash = b => createHash('sha256').update(b).digest('hex');
const sourceBundle = fileURLToPath(new URL('./runner.bundle.mjs', import.meta.url));
const budget = { projectedBytes: 800, limitBytes: 1000, largeBytes: 100000, pressureRatio: 0.8 };
const git = (root, ...args) => execFileSync('git', ['-C', root, ...args], { env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' }, stdio: 'pipe' });
async function fixture(t, managed = true) {
  const base = await mkdtemp(join(tmpdir(), 'bucket-cli-'));
  t.after(() => rm(base, { recursive: true, force: true }));
  const root = join(base, 'repo'), stateDirectory = join(base, 'state'), bundle = join(base, 'runner.bundle.mjs');
  await mkdir(root); await mkdir(stateDirectory, { mode: 0o700 }); await mkdir(join(root, 'public'));
  await copyFile(sourceBundle, bundle);
  git(root, 'init', '-q');
  const pkg = { name: 'fixture', scripts: { build: 'vite build', ...(managed ? { prebuild: PREBUILD } : {}) }, devDependencies: { vite: '7.1.2' } };
  await writeFile(join(root, 'package.json'), canonical(pkg));
  await writeFile(join(root, 'package-lock.json'), canonical({ lockfileVersion: 3, packages: {
    '': { devDependencies: { vite: '7.1.2' } }, 'node_modules/vite': { version: '7.1.2' } } }));
  await writeFile(join(root, 'index.html'), '<html></html>');
  await writeFile(join(root, 'public/icon.png'), renderSolidPng({ width: 32, height: 32, rgba: [255, 46, 136, 255] }));
  git(root, 'add', '--', 'package.json', 'package-lock.json', 'index.html', 'public/icon.png');
  git(root, '-c', 'user.name=DraconDev', '-c', 'user.email=dracsharp@gmail.com', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture');
  const remote = 'https://example.invalid/operator/repository.git';
  git(root, 'remote', 'add', 'origin', remote);
  const profilePath = join(base, 'profile.json');
  const profile = { schemaVersion: 1, id: 'test', enabled: true,
    runner: { path: bundle, sha256: hash(await readFile(bundle)), version: 1 },
    storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://example.invalid', region: 'sbg',
      uploaderCredentialFile: join(base, 'uploader'), consumerCredentialFile: join(base, 'reader') },
    allowedAssetTrees: [base], consumers: [{ id: 'filesystem-restore', version: 1 }] };
  await writeFile(profilePath, canonical(profile), { mode: 0o600 });
  const request = { schemaVersion: 1, operation: 'publish', root, profilePath, stateDirectory,
    remoteName: 'origin', trustedRemoteUrl: remote, budget, allowedCandidates: ['public/icon.png'] };
  const objects = new Map(), roles = []; let closes = 0;
  const factory = async (_p, _b, { role }) => {
    roles.push(role);
    return { head: async k => objects.has(k), download: async (k, p) => writeFile(p, objects.get(k)),
      ...(role === 'uploader' ? { putIfAbsent: async (k, p) => objects.set(k, await readFile(p)) } : {}),
      close: () => { closes++; } };
  };
  return { base, root, bundle, stateDirectory, profilePath, profile, pkg, request, objects, roles,
    factory, closes: () => closes, run: createRunner({ executingPath: bundle, transportFactory: factory }) };
}

test('runner schema is exact and has no implicit budgets or candidate universe', () => {
  const r = { schemaVersion: 1, operation: 'publish', root: '/repo', profilePath: '/profile', stateDirectory: '/state',
    remoteName: 'origin', trustedRemoteUrl: 'https://example.invalid/r', budget, allowedCandidates: [] };
  assert.equal(validateRequest(r), r);
  for (const key of Object.keys(r)) { const bad = { ...r }; delete bad[key]; assert.throws(() => validateRequest(bad), key); }
  for (const patch of [{ requestHandler: {} }, { transport: {} }, { root: '/repo/../other' }, { allowedCandidates: ['../secret'] },
    { allowedCandidates: ['x', 'x'] }, { allowedCandidates: Array(20001).fill('x') }, { budget: { projectedBytes: 1, limitBytes: 2 } }]) {
    assert.throws(() => validateRequest({ ...r, ...patch }));
  }
});

test('runner missing authority and plannable bootstrap never select storage or write state', async t => {
  const f = await fixture(t, false);
  for (const r of [{ ...f.request, profilePath: join(f.base, 'absent') }, { ...f.request, budget: undefined },
    { ...f.request, allowedCandidates: undefined }]) assert.equal((await f.run(r)).commitEligible, false);
  assert.equal((await f.run(f.request)).status, 'plannable');
  assert.deepEqual(f.roles, []);
  await assert.rejects(readFile(join(f.stateDirectory, 'bindings.json')));
  await assert.rejects(readFile(join(f.root, DESCRIPTOR)));
});

test('runner composes approved publish, stable binding/index, idempotence and reader-only new checkout', async t => {
  const f = await fixture(t);
  const source = await readFile(join(f.root, 'public/icon.png'));
  const result = await f.run(f.request);
  assert.equal(result.commitEligible, true, JSON.stringify(result));
  assert.equal(result.manifestSha256, hash(await readFile(join(f.root, MANIFEST))));
  assert.deepEqual(result.materializedPaths, ['public/icon.png']);
  assert.ok(result.generatedInternalPaths.includes(`${MANIFEST}.lock`));
  assert.equal((await f.run(f.request)).commitEligible, true);
  assert.equal(f.objects.size, 1);
  const reader = join(f.base, 'reader-repo');
  await mkdir(reader); await mkdir(join(reader, 'public')); git(reader, 'init', '-q');
  for (const name of ['package.json', 'package-lock.json', 'index.html', DESCRIPTOR, MANIFEST]) await copyFile(join(f.root, name), join(reader, name));
  const stateBefore = await readFile(join(f.stateDirectory, 'bindings.json'));
  const restored = await f.run({ schemaVersion: 1, operation: 'restore', root: reader, profilePath: f.profilePath });
  assert.equal(restored.status, 'restored', JSON.stringify(restored));
  assert.equal(f.roles.at(-1), 'consumer');
  assert.deepEqual(await readFile(join(reader, 'public/icon.png')), source);
  assert.deepEqual(await readFile(join(f.stateDirectory, 'bindings.json')), stateBefore);
  assert.equal(f.closes(), f.roles.length);
  // A portable descriptor does not enroll a different physical checkout for upload.
  assert.equal((await f.run({ ...f.request, root: reader })).commitEligible, false);
});

test('runner descriptor conflict is retained without selecting transport', async t => {
  const f = await fixture(t);
  const bytes = canonical({ schemaVersion: 1, profileId: 'test', repositoryId: 'foreign', namespace: 'games/foreign' });
  await writeFile(join(f.root, DESCRIPTOR), bytes);
  assert.equal((await f.run(f.request)).commitEligible, false);
  assert.equal(await readFile(join(f.root, DESCRIPTOR), 'utf8'), bytes);
  assert.deepEqual(f.roles, []);
});

test('runner consumer edit during upload refuses commit eligibility and preserves edit', async t => {
  const f = await fixture(t);
  const changed = canonical({ ...f.pkg, description: 'operator edit' });
  const run = createRunner({ executingPath: f.bundle, transportFactory: async (...args) => {
    const transport = await f.factory(...args), upload = transport.putIfAbsent;
    transport.putIfAbsent = async (...a) => { await upload(...a); await writeFile(join(f.root, 'package.json'), changed); };
    return transport;
  } });
  const result = await run(f.request);
  assert.equal(result.reason, 'consumer-changed');
  assert.equal(result.commitEligible, false);
  assert.equal(await readFile(join(f.root, 'package.json'), 'utf8'), changed);
  assert.equal(f.closes(), 1);
});

test('bundled CLI works from empty cwd without dependencies and bounds/sanitizes requests', async t => {
  const f = await fixture(t, false);
  const cwd = join(f.base, 'empty'); await mkdir(cwd);
  const execute = input => JSON.parse(execFileSync(process.execPath, [f.bundle], { cwd, input, env: { PATH: process.env.PATH, HOME: cwd }, timeout: 15000 }).toString());
  const version = JSON.parse(execFileSync(process.execPath, [f.bundle, '--version'], { cwd, env: {}, timeout: 15000 }));
  assert.equal(version.protocolVersion, 1);
  const result = execute(JSON.stringify({ schemaVersion: 1, operation: 'inspect', root: f.root, profilePath: f.profilePath }));
  assert.equal(result.status, 'plannable');
  assert.equal(result.runtimeQualified, true);
  for (const input of ['{SECRET', ' '.repeat(LIMIT + 1), JSON.stringify({ credential: 'SECRET' })]) {
    const refusal = execute(input); assert.equal(refusal.commitEligible, false); assert.ok(!JSON.stringify(refusal).includes('SECRET'));
  }
  execFileSync(process.execPath, [fileURLToPath(new URL('../build-bucket-runner.mjs', import.meta.url)), '--check'], { timeout: 30000 });
});
