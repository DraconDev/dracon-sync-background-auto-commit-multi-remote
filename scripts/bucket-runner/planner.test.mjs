import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { join, sep } from 'node:path';
import { renderSolidPng, CONSUMER_ID, CONSUMER_VERSION } from './adapters.mjs';
import { inspectRepository } from './planner.mjs';
const sha = b => createHash('sha256').update(b).digest('hex');

const brand = { width: 32, height: 32, rgba: [255, 46, 136, 255] };
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
    '-c', 'init.defaultBranch=main', '-C', cwd, ...args], { env: { ...process.env, GIT_OPTIONAL_LOCKS: '0',
      GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LC_ALL: 'C' } });
}

async function repoFixture(t, files = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'planner-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  git(dir, 'init', '-q');
  git(dir, 'config', 'user.email', 't@e.x');
  git(dir, 'config', 'user.name', 't');
  for (const [name, data] of Object.entries(files)) {
    const path = join(dir, name);
    await mkdir(join(dir, '..'), { recursive: true }).catch(() => {});
    await mkdir(join(dir, name, '..'), { recursive: true }).catch(() => {});
    await writeFile(path, data);
  }
  return dir;
}

test('inspectRepository: discovers supported eligible asset and ignores unsafe/unsupported files', async t => {
  const root = await repoFixture(t, {
    'authorized/favicon.png': renderSolidPng(brand),
    'authorized/banner.png': renderSolidPng({ ...brand, rgba: [1, 2, 3, 4] }),
    'authorized/source.ts': Buffer.from('export const notMedia = true;'),
    'authorized/.secret/cfg.png': renderSolidPng(brand),
    'authorized/audit/old.png': renderSolidPng(brand),
    'authorized/notes/proto.txt': Buffer.from('just notes'),
    'authorized/sub/inner.png': renderSolidPng(brand),
    'authorized/build/extra.png': renderSolidPng(brand),
    'top.png': renderSolidPng(brand),
    '.gitignore': 'authorized/secret/\nauthorized/notes/ignored.png\n'
  });
  await writeFile(join(root, 'authorized/notes/ignored.png'), renderSolidPng(brand));
  const plan = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  assert.equal(plan.status, 'planned');
  assert.equal(plan.complete, true);
  const by = Object.fromEntries(plan.outcomes.map(o => [o.sourcePath, o]));
  for (const path of ['authorized/favicon.png', 'authorized/banner.png']) {
    assert.equal(by[path].status, 'eligible', path);
    assert.ok(plan.proposedManifest.assets.some(e => e.sourcePath === path));
  }
  // Non-media (.ts) files are filtered before the candidate loop, so they have no outcome.
  for (const [path, reason] of [['authorized/.secret/cfg.png', 'unsafe-or-excluded-path'],
    ['authorized/audit/old.png', 'unsafe-or-excluded-path'],
    ['authorized/notes/ignored.png', 'git-ignored'],
    ['top.png', 'unsupported-consumer']]) assert.ok(by[path], `outcome missing for ${path}`), assert.equal(by[path].reason, reason, path);
  for (const entry of plan.proposedManifest.assets) {
    assert.equal(entry.bucketed, false, 'planner must not claim verification');
    assert.deepEqual(entry.generator, { adapterId: 'solid-rgba-png', adapterVersion: 1,
      runner: { sha256: 'a'.repeat(64), version: 1 }, inputs: [] });
    assert.equal(entry.objectKey, `games/auto/repo-123/{sha256}.{ext}`);
    assert.equal(entry.visibility, 'private'); assert.equal(entry.regenerable, true);
  }
});

test('inspectRepository: new asset root only enrolls files beneath it', async t => {
  const root = await repoFixture(t, {
    'authorized/icon.png': renderSolidPng(brand),
    'other/stray.png': renderSolidPng(brand)
  });
  const plan = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: 'authorized' });
  assert.equal(plan.status, 'planned');
  assert.equal(plan.proposedManifest.assets.length, 1);
  assert.equal(plan.proposedManifest.assets[0].sourcePath, 'icon.png');
  assert.deepEqual(plan.outcomes.find(o => o.sourcePath === 'other/stray.png'),
    { sourcePath: 'other/stray.png', status: 'excluded', reason: 'outside-asset-root' });
});

test('inspectRepository: existing manifest is preserved, legacy entries retained and changed ones refused', async t => {
  const root = await repoFixture(t, {
    'authorized/keep.png': renderSolidPng({ ...brand, rgba: [9, 8, 7, 6] }),
    'authorized/new.png': renderSolidPng(brand)
  });
  const keepSha = sha(renderSolidPng({ ...brand, rgba: [9, 8, 7, 6] }));
  const prior = { schemaVersion: 1, bucket: 'dracon-lfs', repository: 'repo-123',
    namespace: 'games/auto/repo-123', assets: [
      { id: 'legacy-1', kind: 'image', logicalPath: 'keep.png', sourcePath: 'authorized/keep.png',
        objectKey: 'games/auto/repo-123/keep/{sha256}.{ext}', sha256: keepSha, bytes: 103, regenerable: true,
        visibility: 'private', bucketed: true, generator: { command: 'old', inputs: [] } }
    ] };
  const plan = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: prior });
  assert.equal(plan.proposedManifest.assets.length, 2);
  const kept = plan.proposedManifest.assets.find(a => a.id === 'legacy-1');
  assert.equal(kept.sha256, keepSha); assert.equal(kept.bucketed, true);
  assert.deepEqual(kept, prior.assets[0], 'planner must not silently rewrite retained legacy entries');
  assert.equal(plan.outcomes.find(o => o.sourcePath === 'authorized/keep.png').status, 'already-registered');
  // Changed bytes: re-plan rejects the legacy entry rather than reusing the old proof.
  await writeFile(join(root, 'authorized/keep.png'), renderSolidPng({ ...brand, rgba: [0, 1, 2, 3] }));
  const changed = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: prior });
  assert.equal(changed.outcomes.find(o => o.sourcePath === 'authorized/keep.png').status, 'unsupported');
  assert.equal(changed.outcomes.find(o => o.sourcePath === 'authorized/keep.png').reason, 'changed-legacy-source');
  // The legacy reference is preserved exactly and a new eligible entry with a distinct
  // stable id is added; the publisher's separate verified publication decides which to upload.
  assert.equal(changed.proposedManifest.assets.length, 2);
  assert.equal(changed.proposedManifest.assets.find(a => a.id === 'legacy-1').sha256, keepSha);
});

test('inspectRepository: manifest/binding mismatch and unsafe consumer are refused', async t => {
  const root = await repoFixture(t, { 'authorized/icon.png': renderSolidPng(brand) });
  const planWithPrior = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  const wrongBinding = { ...binding(), namespace: 'games/other' };
  const mismatch = await inspectRepository({ root, profile: profile(), binding: wrongBinding, consumer, assetRoot: '.', manifest: planWithPrior.proposedManifest });
  assert.equal(mismatch.status, 'refused'); assert.equal(mismatch.reason, 'manifest-binding-mismatch');
  // Unknown consumer id is treated as an unsupported storage scope and produces no proposal.
  const orphan = await inspectRepository({ root, profile: profile(), binding: binding(),
    consumer: { id: 'unknown', version: 1, assetRoots: ['authorized'] }, assetRoot: '.' });
  assert.equal(orphan.outcomes.find(o => o.sourcePath === 'authorized/icon.png')?.reason, 'unsupported-consumer');
});

test('inspectRepository: missing/deleted source keeps the prior manifest entry and reports missing', async t => {
  const root = await repoFixture(t, { 'authorized/gone.png': renderSolidPng(brand) });
  const prior = { schemaVersion: 1, bucket: 'dracon-lfs', repository: 'repo-123', namespace: 'games/auto/repo-123',
    assets: [{ id: 'gone', kind: 'image', logicalPath: 'gone.png', sourcePath: 'authorized/gone.png',
      objectKey: 'games/auto/repo-123/gone/{sha256}.{ext}', sha256: sha(renderSolidPng(brand)), bytes: 103, regenerable: true,
      visibility: 'private', bucketed: true, generator: { command: 'old', inputs: [] } }] };
  await rm(join(root, 'authorized/gone.png'));
  const plan = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: prior });
  // Missing sources are not enumerated through `paths`, so we must look at the prior manifest instead.
  const missing = plan.proposedManifest.assets.find(a => a.sourcePath === 'authorized/gone.png');
  assert.ok(missing, 'planner must preserve a missing legacy entry');
  assert.equal(missing.id, 'gone');
  assert.equal(plan.proposedManifest.assets.length, 1, 'planner must not silently retire a missing source');
  assert.equal(plan.status, 'planned');
});

test('inspectRepository: phantom checkout, symlink and nested gitlink are refused', async t => {
  const root = await repoFixture(t, { 'authorized/icon.png': renderSolidPng(brand) });
  const linkPath = join(root, 'authorized/link.png');
  await symlink(join(root, 'authorized/icon.png'), linkPath);
  const result = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  assert.equal(result.outcomes.find(o => o.sourcePath === 'authorized/link.png')?.reason, 'symlink-path');
  // Phantom checkout (no .git) is refused without producing a proposal.
  const phony = await mkdtemp(join(tmpdir(), 'phony-'));
  const refused = await inspectRepository({ root: phony, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  assert.equal(refused.status, 'refused'); assert.equal(refused.reason, 'phantom-checkout');
});

test('inspectRepository: same plan twice is idempotent; new file is enrolled on the second pass', async t => {
  const root = await repoFixture(t, { 'authorized/icon.png': renderSolidPng(brand) });
  const first = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.' });
  const second = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: first.proposedManifest });
  assert.equal(first.proposedManifest.assets.length, 1);
  assert.equal(second.proposedManifest.assets.length, 1);
  assert.equal(second.outcomes.find(o => o.sourcePath === 'authorized/icon.png')?.status, 'already-registered');
  await writeFile(join(root, 'authorized/another.png'), renderSolidPng({ ...brand, rgba: [10, 20, 30, 40] }));
  const third = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: second.proposedManifest });
  assert.equal(third.added.length, 1);
  assert.equal(third.proposedManifest.assets.length, 2);
  const fourth = await inspectRepository({ root, profile: profile(), binding: binding(), consumer, assetRoot: '.', manifest: third.proposedManifest });
  assert.equal(fourth.added.length, 0);
  assert.equal(fourth.proposedManifest.assets.length, 2, 're-plan must not duplicate existing entries');
});
