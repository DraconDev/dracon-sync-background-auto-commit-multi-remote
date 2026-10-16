import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmod, chown, link, mkdir, mkdtemp, readFile, rm, symlink, writeFile, rename } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inspectConsumer, revalidateConsumer, PREBUILD, MANIFEST } from './consumer.mjs';

const canonical = value => `${JSON.stringify(value, null, 2)}\n`;
const hash = value => createHash('sha256').update(value).digest('hex');
async function fixture(t, { managed = true, version = '7.1.2' } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'bucket-consumer-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')));
  execFileSync('git', ['init', '-q', root], { env });
  const pkg = { name: 'consumer-fixture', private: true, custom: { preserve: ['all', 'keys'] }, scripts: {
    build: 'vite build', postbuild: 'node postbuild.mjs', dev: 'vite', preview: 'vite preview',
    ...(managed ? { prebuild: PREBUILD } : {}),
  }, devDependencies: { vite: version } };
  const lock = { name: pkg.name, lockfileVersion: 3, requires: true, packages: {
    '': { name: pkg.name, devDependencies: { vite: version } }, 'node_modules/vite': { version },
  } };
  await writeFile(join(root, 'package.json'), canonical(pkg));
  await writeFile(join(root, 'package-lock.json'), canonical(lock));
  await writeFile(join(root, 'index.html'), '<!doctype html><div id="app"></div>\n');
  await mkdir(join(root, 'public'));
  await writeFile(join(root, 'public/source.txt'), 'source bytes remain unchanged\n');
  await writeFile(join(root, 'postbuild.mjs'), 'throw new Error("qualification must never execute scripts");\n');
  return { root, pkg, lock, inspect: options => inspectConsumer({ root, ...options }),
    save: () => writeFile(join(root, 'package.json'), canonical(pkg)),
    saveLock: () => writeFile(join(root, 'package-lock.json'), canonical(lock)) };
}
async function unsupported(f, reason) {
  const result = await f.inspect();
  assert.equal(result.status, 'unsupported', JSON.stringify(result));
  assert.equal(result.reason, reason);
  assert.equal(result.consumer, null);
  assert.equal(result.plan, null);
  return result;
}

test('independent checkout beneath parent Git/package boundary remains eligible for text qualification', async t => {
  const f = await fixture(t);
  const parent = await mkdtemp(join(tmpdir(), 'consumer-parent-repo-'));
  t.after(() => rm(parent, { recursive: true, force: true }));
  execFileSync('git', ['init', '-q', parent]);
  await writeFile(join(parent, 'package.json'), canonical({ name: 'parent', private: true, workspaces: ['unrelated/*'] }));
  const nested = join(parent, 'independent');
  await rename(f.root, nested);
  const result = await inspectConsumer({ root: nested });
  assert.equal(result.status, 'supported', JSON.stringify(result));
  assert.equal((await revalidateConsumer(result.evidence)).status, 'supported');
});

test('managed plain Vite verifies text only, does not build or mutate; manifest is snapshot data only', async t => {
  const f = await fixture(t);
  await writeFile(join(f.root, MANIFEST), 'not adopted as manifest authority\n');
  const before = await readFile(join(f.root, 'package.json'));
  const result = await f.inspect();
  assert.equal(result.status, 'supported');
  assert.deepEqual(result.consumer, { id: 'filesystem-restore', version: 1, assetRoots: ['public'] });
  assert.equal(result.runtimeQualified, false);
  assert.equal(result.scope, 'plain-vite-bootstrap-text-v1');
  assert.equal(result.manifestPath, MANIFEST);
  assert.equal(result.plan, null);
  assert.equal((await revalidateConsumer(result.evidence)).status, 'supported');
  assert.deepEqual(await readFile(join(f.root, 'package.json')), before);
  assert.equal(await readFile(join(f.root, 'public/source.txt'), 'utf8'), 'source bytes remain unchanged\n');
  assert.equal((await revalidateConsumer({ ...result.evidence })).reason, 'unrecognized-consumer-evidence');
});

test('missing prebuild returns digest-bound proposed bytes, no authority; fixture-only application requalifies', async t => {
  const f = await fixture(t, { managed: false, version: '6.4.0' });
  await chmod(join(f.root, 'package.json'), 0o640);
  const before = await readFile(join(f.root, 'package.json'), 'utf8');
  const result = await f.inspect();
  assert.equal(result.status, 'plannable');
  assert.equal(result.consumer, null);
  assert.equal(result.plan.expectedBytes, before);
  assert.equal(result.plan.expectedDigest, hash(before));
  assert.equal(result.plan.mode, 0o640);
  assert.equal(result.plan.requiresExclusiveMutationLock, true);
  assert.equal(result.plan.requiresFreshInspection, true);
  assert.equal(await readFile(join(f.root, 'package.json'), 'utf8'), before);
  const proposed = JSON.parse(result.plan.proposedBytes);
  assert.equal(proposed.scripts.prebuild, PREBUILD);
  delete proposed.scripts.prebuild;
  assert.deepEqual(proposed, f.pkg);
  // This disposable fixture is exclusively owned by this test. Production apply
  // and its mutation lock deliberately belong to the parent, not this module.
  assert.equal(hash(await readFile(join(f.root, 'package.json'))), result.plan.expectedDigest);
  await writeFile(join(f.root, 'package.json'), result.plan.proposedBytes, { mode: result.plan.mode });
  assert.equal((await revalidateConsumer(result.evidence)).status, 'retryable');
  assert.equal((await f.inspect()).status, 'supported');
});

test('occupied prebuild including empty value remains byte-identical', async t => {
  for (const prebuild of ['', 'npm run sync-catalog', `${PREBUILD} && echo done`, null]) {
    await t.test(JSON.stringify(prebuild), async t => {
      const f = await fixture(t);
      f.pkg.scripts.prebuild = prebuild;
      await f.save();
      const before = await readFile(join(f.root, 'package.json'));
      await unsupported(f, 'occupied-prebuild');
      assert.deepEqual(await readFile(join(f.root, 'package.json')), before);
    });
  }
});

test('all Vite and Svelte config variants fail closed, never import config', async t => {
  for (const type of ['vite', 'svelte']) for (const extension of ['js', 'mjs', 'cjs', 'ts', 'mts', 'cts']) {
    await t.test(`${type}.${extension}`, async t => {
      const f = await fixture(t);
      const name = `${type}.config.${extension}`;
      await writeFile(join(f.root, name), 'throw new Error("never import me")');
      await unsupported(f, `unsupported-config:${name}`);
    });
  }
});

test('static assets are not default public; root index is mandatory', async t => {
  const f = await fixture(t);
  await rm(join(f.root, 'public'), { recursive: true });
  await mkdir(join(f.root, 'static'));
  await unsupported(f, 'missing-default-public-directory');
  await mkdir(join(f.root, 'public'));
  await rm(join(f.root, 'index.html'));
  await unsupported(f, 'missing-root-index');
});

test('canonical JSON requirement rejects malformed, duplicate, minified and alternate formatting without rewriting', async t => {
  for (const bytes of ['{', '{"scripts":{"build":"vite build"},"scripts":{}}\n', '{"name":"x"}\n', '{\n  "name": "x",\n  "name": "y"\n}\n']) {
    await t.test(bytes, async t => {
      const f = await fixture(t);
      await writeFile(join(f.root, 'package.json'), bytes);
      const result = await f.inspect();
      assert.equal(result.status, 'unsupported');
      assert.match(result.reason, /malformed-package-json|noncanonical-or-duplicate-package-json/);
      assert.equal(await readFile(join(f.root, 'package.json'), 'utf8'), bytes);
    });
  }
});

test('exact pinned Vite 6/7 and matching npm lock metadata are required', async t => {
  for (const version of ['^7.1.2', '~6.0.0', '8.0.0', '5.4.0', '7.0.0-beta.1', '07.1.0', 'npm:vite@7.0.0', 'file:../vite']) {
    await t.test(version, async t => {
      const f = await fixture(t, { version });
      await unsupported(f, 'unsupported-vite-version');
    });
  }
  const f = await fixture(t);
  f.lock.packages['node_modules/vite'].version = '7.1.3';
  await f.saveLock();
  await unsupported(f, 'vite-lock-mismatch');
  await rm(join(f.root, 'package-lock.json'));
  await unsupported(f, 'missing-package-lock.json');
});

test('unsupported invocation, disabled lifecycle and alternative package manager', async t => {
  const f = await fixture(t);
  for (const options of [{ entrypoint: 'vite build' }, { entrypoint: 'bun run build' }, { entrypoint: 'npm run build --ignore-scripts' }, { lifecycleScripts: false }]) {
    assert.equal((await f.inspect(options)).reason, 'unsupported-lifecycle-entrypoint');
  }
  for (const script of ['vite build --config x.js', 'vite build && echo done', 'bun run build']) {
    f.pkg.scripts.build = script;
    await f.save();
    await unsupported(f, 'unsupported-build-script');
  }
  f.pkg.scripts.build = 'vite build';
  f.pkg.packageManager = 'bun@1.2.0';
  await f.save();
  await unsupported(f, 'unsupported-package-manager');
});

test('legacy manifests, restore scripts, npm configuration and restore workflow conflict', async t => {
  for (const path of ['bucket-assets.json', '.dracon/bucket-assets.json', '.npmrc', 'bun.lock']) {
    await t.test(path, async t => {
      const f = await fixture(t);
      if (path.startsWith('.dracon/')) await mkdir(join(f.root, '.dracon'));
      await writeFile(join(f.root, path), '{}\n');
      const result = await f.inspect();
      assert.equal(result.status, 'unsupported');
      assert.match(result.reason, /legacy-authority|unsupported-package-lifecycle/);
    });
  }
  const f = await fixture(t);
  f.pkg.scripts['assets:restore'] = '';
  await f.save();
  await unsupported(f, 'conflicting-restore-integration');
  delete f.pkg.scripts['assets:restore'];
  f.pkg.scripts.postbuild = 'node restore.mjs';
  await f.save();
  await unsupported(f, 'conflicting-restore-integration');
  f.pkg.scripts.postbuild = 'node postbuild.mjs';
  await f.save();
  await mkdir(join(f.root, '.github/workflows'), { recursive: true });
  await writeFile(join(f.root, '.github/workflows/assets.yml'), 'run: old-bucket restore\n');
  await unsupported(f, 'conflicting-restore-workflow');
});

test('package edit races invalidate plan and opaque consumer evidence', async t => {
  const f = await fixture(t, { managed: false });
  const result = await f.inspect();
  f.pkg.scripts.postbuild = 'node operator-edit.mjs';
  await f.save();
  const operatorBytes = await readFile(join(f.root, 'package.json'));
  assert.notEqual(hash(operatorBytes), result.plan.expectedDigest);
  assert.equal((await revalidateConsumer(result.evidence)).reason, 'source-changed');
  assert.deepEqual(await readFile(join(f.root, 'package.json')), operatorBytes);
});

test('config, index, public, lock and manifest changes invalidate evidence', async t => {
  for (const path of ['vite.config.cts', 'index.html', 'public/new.txt', 'package-lock.json', MANIFEST]) {
    await t.test(path, async t => {
      const f = await fixture(t);
      const result = await f.inspect();
      assert.equal(result.status, 'supported');
      await writeFile(join(f.root, path), 'operator change\n');
      const fresh = await revalidateConsumer(result.evidence);
      assert.equal(fresh.status, 'retryable');
      assert.equal(fresh.consumer, null);
    });
  }
});

test('symlink files/directories/ancestors and hardlinks are not proof', async t => {
  for (const path of ['package.json', 'index.html', 'public', 'public/extra']) {
    await t.test(path, async t => {
      const f = await fixture(t);
      await rm(join(f.root, path), { recursive: true, force: true });
      await symlink('/tmp', join(f.root, path));
      await unsupported(f, 'symlink-path');
    });
  }
  const f = await fixture(t);
  await link(join(f.root, 'index.html'), join(f.root, 'alias.html'));
  await unsupported(f, 'hardlink-ambiguity');
  const alias = `${f.root}-alias`;
  t.after(() => rm(alias, { force: true }));
  await symlink(f.root, alias);
  assert.equal((await inspectConsumer({ root: alias })).reason, 'symlink-or-invalid-ancestor');
});

test('nested Git, workspaces, nested package, unsafe ownership and writable directories refused', async t => {
  const f = await fixture(t);
  await mkdir(join(f.root, 'public/nested/.git'), { recursive: true });
  await unsupported(f, 'nested-git');
  await rm(join(f.root, 'public/nested'), { recursive: true });
  f.pkg.workspaces = [];
  await f.save();
  await unsupported(f, 'workspace-or-nested-package');
  delete f.pkg.workspaces;
  await f.save();
  await writeFile(join(f.root, 'public/package.json'), '{}');
  await unsupported(f, 'workspace-or-nested-package');
  await rm(join(f.root, 'public/package.json'));
  await chmod(join(f.root, 'public'), 0o777);
  await unsupported(f, 'unsafe-ownership-or-permissions');
  await chmod(join(f.root, 'public'), 0o755);
  if (process.geteuid() === 0) {
    await chown(join(f.root, 'index.html'), 12345, 12345);
    await unsupported(f, 'unsafe-ownership-or-permissions');
  }
});

test('bounded reads and directory enumeration fail closed', async t => {
  const f = await fixture(t);
  await writeFile(join(f.root, 'index.html'), Buffer.alloc(256 * 1024 + 1));
  await unsupported(f, 'file-size-limit');
  await writeFile(join(f.root, 'index.html'), 'index');
  let directory = join(f.root, 'public');
  for (let i = 0; i < 17; i++) { directory = join(directory, 'd'); await mkdir(directory); }
  await unsupported(f, 'directory-depth-limit');
  await rm(join(f.root, 'public/d'), { recursive: true });
  await Promise.all(Array.from({ length: 4097 }, (_, i) => writeFile(join(f.root, 'public', `f${i}`), '')));
  await unsupported(f, 'directory-entry-limit');
});

test('all inherited GIT overrides are stripped and Git-root boundaries are verified', async t => {
  const f = await fixture(t);
  const keys = ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_CONFIG_COUNT', 'GIT_CONFIG_KEY_0', 'GIT_CONFIG_VALUE_0'];
  const saved = keys.map(k => process.env[k]);
  try {
    Object.assign(process.env, { GIT_DIR: '/not-a-repo', GIT_WORK_TREE: '/tmp', GIT_INDEX_FILE: '/not-an-index',
      GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'core.worktree', GIT_CONFIG_VALUE_0: '/tmp' });
    assert.equal((await f.inspect()).status, 'supported');
  } finally {
    keys.forEach((k, i) => saved[i] === undefined ? delete process.env[k] : process.env[k] = saved[i]);
  }
  await rm(join(f.root, '.git'), { recursive: true });
  await unsupported(f, 'missing-git-root');
});

test('Git metadata indirection, includes, symlinks and staged gitlinks fail closed', async t => {
  const f = await fixture(t);
  const config = await readFile(join(f.root, '.git/config'));
  await writeFile(join(f.root, '.git/config'), `${config}\n[include]\n path = /not-read\n`);
  await unsupported(f, 'unsupported-git-config-include');
  await writeFile(join(f.root, '.git/config'), config);
  await writeFile(join(f.root, '.git/commondir'), '/not-read');
  await unsupported(f, 'unsupported-git-indirection');
  await rm(join(f.root, '.git/commondir'));
  await rm(join(f.root, '.git/HEAD'));
  await symlink('/not-read', join(f.root, '.git/HEAD'));
  await unsupported(f, 'symlink-path');
  await rm(join(f.root, '.git/HEAD'));
  await writeFile(join(f.root, '.git/HEAD'), 'ref: refs/heads/main\n');
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')));
  execFileSync('git', ['-C', f.root, 'update-index', '--add', '--cacheinfo', '160000', '1111111111111111111111111111111111111111', 'public/nested'], { env });
  await unsupported(f, 'nested-gitlink');
});

test('framework plugins, duplicate Vite declarations and lifecycle ignore environment are unsupported', async t => {
  const f = await fixture(t);
  f.pkg.devDependencies['@sveltejs/kit'] = '2.0.0';
  await f.save();
  await unsupported(f, 'unsupported-framework-or-plugin');
  delete f.pkg.devDependencies['@sveltejs/kit'];
  f.pkg.dependencies = { vite: '7.1.2' };
  await f.save();
  await unsupported(f, 'unsupported-vite-version');
  const previous = process.env.npm_config_ignore_scripts;
  try {
    process.env.npm_config_ignore_scripts = 'true';
    assert.equal((await f.inspect()).reason, 'unsupported-lifecycle-entrypoint');
  } finally {
    if (previous === undefined) delete process.env.npm_config_ignore_scripts;
    else process.env.npm_config_ignore_scripts = previous;
  }
});
