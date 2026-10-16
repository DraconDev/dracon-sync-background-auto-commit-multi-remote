import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, chmod, rm, rename, symlink, writeFile, readFile, stat, cp } from 'node:fs/promises';
import { join } from 'node:path';
import { bindRepository } from './bindings.mjs';

const URL = 'https://example.invalid/operator/game.git';
function git(root, ...args) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')));
  return execFileSync('git', ['-C', root, ...args], { env, timeout: 5000, stdio: ['ignore', 'pipe', 'pipe'] }).toString().trim();
}
async function repo(root) {
  await mkdir(root); git(root, 'init', '-b', 'main');
  git(root, 'config', 'user.name', 'Binding Fixture');
  git(root, 'config', 'user.email', 'binding-fixture@example.invalid');
  git(root, 'config', 'commit.gpgsign', 'false');
  git(root, 'config', 'core.hooksPath', '/dev/null');
  git(root, 'remote', 'add', 'origin', URL);
  git(root, 'commit', '--allow-empty', '-m', 'fixture anchor');
}
async function fixture(t) {
  const base = await mkdtemp('/tmp/binding-fixture-');
  t.after(() => rm(base, { recursive: true, force: true }));
  const trees = join(base, 'games'); await mkdir(trees);
  const stateDirectory = join(base, 'private'); await mkdir(stateDirectory, { mode: 0o700 });
  const repositoryRoot = join(trees, 'one'); await repo(repositoryRoot);
  const profile = { schemaVersion: 1, id: 'fixture', enabled: true,
    runner: { path: '/opt/fixture/runner', sha256: 'a'.repeat(64), version: 1 },
    storage: { bucket: 'dracon-lfs', namespaceRoot: 'games/fixture', endpoint: 'https://example.invalid', region: 'fixture-1',
      uploaderCredentialFile: '/private/fixture/upload', consumerCredentialFile: '/private/fixture/read' },
    allowedAssetTrees: [trees], consumers: [{ id: 'filesystem-restore', version: 1 }] };
  return { base, trees, stateDirectory, repositoryRoot, profile, remoteName: 'origin', trustedRemoteUrl: URL };
}
const request = f => Object.fromEntries(['stateDirectory', 'repositoryRoot', 'profile', 'remoteName', 'trustedRemoteUrl'].map(k => [k, f[k]]));

test('first binding is private, durable, idempotent and retained after physical rename', async t => {
  const f = await fixture(t), input = request(f);
  const first = await bindRepository(input);
  assert.equal(first.status, 'bound');
  assert.match(first.binding.repositoryId, /^[a-f0-9-]{36}$/);
  assert.equal(first.binding.namespace, `games/fixture/${first.binding.repositoryId}`);
  assert.equal((await stat(join(f.stateDirectory, 'bindings.json'))).mode & 0o777, 0o600);
  assert.equal((await stat(join(f.stateDirectory, 'bindings.lock'))).mode & 0o777, 0o600);
  git(f.repositoryRoot, 'commit', '--allow-empty', '-m', 'descendant');
  assert.deepEqual(await bindRepository({ ...input, binding: first.binding }), { status: 'existing', binding: first.binding });
  const moved = join(f.trees, 'renamed'); await rename(f.repositoryRoot, moved);
  assert.deepEqual((await bindRepository({ ...input, repositoryRoot: moved })).binding, first.binding);
});

test('descriptor never enrolls unrelated or copied .git repositories', async t => {
  const f = await fixture(t), input = request(f);
  const first = await bindRepository(input);
  const other = join(f.trees, 'other'); await repo(other);
  await assert.rejects(bindRepository({ ...input, repositoryRoot: other, binding: first.binding }), /descriptor-without/);
  const copy = join(f.trees, 'copy'); await cp(f.repositoryRoot, copy, { recursive: true });
  await assert.rejects(bindRepository({ ...input, repositoryRoot: copy, binding: first.binding }), /descriptor-without/);
  const independent = await bindRepository({ ...input, repositoryRoot: copy });
  assert.notEqual(independent.binding.repositoryId, first.binding.repositoryId);
  await assert.rejects(bindRepository({ ...input, binding: independent.binding }), /descriptor-binding-conflict/);
  const private2 = join(f.base, 'private2'); await mkdir(private2, { mode: 0o700 });
  await assert.rejects(bindRepository({ ...input, stateDirectory: private2, binding: first.binding }), /descriptor-without/);
});

test('remote selection and enrollment anchor fail closed on drift', async t => {
  const f = await fixture(t), input = request(f);
  await assert.rejects(bindRepository({ ...input, trustedRemoteUrl: `${URL}-wrong` }), /remote-mismatch/);
  await bindRepository(input);
  git(f.repositoryRoot, 'remote', 'set-url', 'origin', `${URL}-changed`);
  await assert.rejects(bindRepository(input), /remote-mismatch/);
  await assert.rejects(bindRepository({ ...input, trustedRemoteUrl: `${URL}-changed` }), /stored-remote-conflict/);
  git(f.repositoryRoot, 'remote', 'set-url', 'origin', URL);
  git(f.repositoryRoot, 'config', 'remote.origin.pushurl', `${URL}-push`);
  await assert.rejects(bindRepository(input), /remote-mismatch/);
  git(f.repositoryRoot, 'config', '--unset', 'remote.origin.pushurl');
  git(f.repositoryRoot, 'config', '--add', 'remote.origin.url', URL);
  await assert.rejects(bindRepository(input), /remote-mismatch/);
  git(f.repositoryRoot, 'config', '--unset-all', 'remote.origin.url'); git(f.repositoryRoot, 'config', 'remote.origin.url', URL);
  git(f.repositoryRoot, 'checkout', '--orphan', 'unrelated');
  git(f.repositoryRoot, 'commit', '--allow-empty', '-m', 'unrelated history');
  await assert.rejects(bindRepository(input), /git-inspection-refused/);
});

test('disabled, other-profile, unauthorized tree and malformed input refuse', async t => {
  const f = await fixture(t), input = request(f);
  await bindRepository(input);
  await assert.rejects(bindRepository({ ...input, profile: { ...f.profile, enabled: false } }), /disabled/);
  await assert.rejects(bindRepository({ ...input, profile: { ...f.profile, id: 'other' } }), /binding refused/);
  await assert.rejects(bindRepository({ ...input, profile: { ...f.profile, allowedAssetTrees: [join(f.base, 'elsewhere')] } }), /outside-authorized/);
  await assert.rejects(bindRepository({ ...input, unexpected: true }), /invalid-binding-request/);
  await assert.rejects(bindRepository({ ...input, binding: null }), /binding refused/);
});

test('symlink roots, symlink state paths, writable parents and unsafe files refuse', async t => {
  const f = await fixture(t), input = request(f);
  const alias = join(f.trees, 'alias'); await symlink(f.repositoryRoot, alias);
  await assert.rejects(bindRepository({ ...input, repositoryRoot: alias }), /symlink/);
  const stateAlias = join(f.base, 'state-alias'); await symlink(f.stateDirectory, stateAlias);
  await assert.rejects(bindRepository({ ...input, stateDirectory: stateAlias }), /symlink/);
  await chmod(f.stateDirectory, 0o755);
  await assert.rejects(bindRepository(input), /private/);
  await chmod(f.stateDirectory, 0o700);
  await chmod(f.base, 0o777);
  await assert.rejects(bindRepository(input), /writable-directory/);
  await chmod(f.base, 0o700);
  const inside = join(f.repositoryRoot, 'private'); await mkdir(inside, { mode: 0o700 });
  await assert.rejects(bindRepository({ ...input, stateDirectory: inside }), /state-inside-repository/);
  const lock = join(f.stateDirectory, 'bindings.lock'); await rm(lock, { force: true }); await symlink(join(f.base, 'missing'), lock);
  await assert.rejects(bindRepository(input), /unsafe-state-file/); await rm(lock);
  await writeFile(lock, '', { mode: 0o644 });
  await assert.rejects(bindRepository(input), /unsafe-state-file/); await rm(lock);
  const state = join(f.stateDirectory, 'bindings.json'); await symlink(join(f.base, 'missing'), state);
  await assert.rejects(bindRepository(input), /unsafe-state-file/); await rm(state);
  await writeFile(state, '{}', { mode: 0o644 });
  await assert.rejects(bindRepository(input), /unsafe-state-file/);
  await chmod(state, 0o600);
  await assert.rejects(bindRepository(input), /malformed-binding-state/);
  await writeFile(state, 'x'.repeat(1024 * 1024 + 1));
  await assert.rejects(bindRepository(input), /oversized-binding-state/);
});

test('unborn, detached, nested root and symlink common directory refuse', async t => {
  const f = await fixture(t), input = request(f);
  const empty = join(f.trees, 'empty'); await mkdir(empty); git(empty, 'init', '-b', 'main');
  await assert.rejects(bindRepository({ ...input, repositoryRoot: empty }), /git-inspection-refused/);
  const nested = join(f.repositoryRoot, 'nested'); await mkdir(nested);
  await assert.rejects(bindRepository({ ...input, repositoryRoot: nested }), /ambiguous/);
  git(f.repositoryRoot, 'checkout', '--detach');
  await assert.rejects(bindRepository(input), /git-inspection-refused/);
  git(f.repositoryRoot, 'checkout', 'main');
  const moved = join(f.base, 'moved-git'); await rename(join(f.repositoryRoot, '.git'), moved);
  await symlink(moved, join(f.repositoryRoot, '.git'));
  await assert.rejects(bindRepository(input), /symlink/);
});

test('single state lock serializes simultaneous first enrollments without losing records', async t => {
  const f = await fixture(t), input = request(f);
  const other = join(f.trees, 'other'); await repo(other);
  const results = await Promise.all(Array.from({ length: 6 }, (_, i) => bindRepository({ ...input, repositoryRoot: i % 2 ? other : f.repositoryRoot })));
  assert.equal(results.filter(r => r.status === 'bound').length, 2);
  assert.equal(new Set(results.map(r => r.binding.repositoryId)).size, 2);
  const state = JSON.parse(await readFile(join(f.stateDirectory, 'bindings.json'), 'utf8'));
  assert.equal(state.records.length, 2);
});

test('inherited Git overrides cannot redirect inspected repository', async t => {
  const f = await fixture(t), input = request(f);
  const old = process.env.GIT_DIR;
  process.env.GIT_DIR = '/nonexistent/fixture-override';
  try { assert.equal((await bindRepository(input)).status, 'bound'); }
  finally { if (old === undefined) delete process.env.GIT_DIR; else process.env.GIT_DIR = old; }
});
