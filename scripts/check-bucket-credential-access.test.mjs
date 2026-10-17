import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm, chmod, readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { checkCredentialAccess } from './check-bucket-credential-access.mjs';
const canonical = value => `${JSON.stringify(value, null, 2)}\n`;
const cli = fileURLToPath(new URL('./check-bucket-credential-access.mjs', import.meta.url));
async function fixture(t) {
  // Credential authority deliberately refuses writable /tmp ancestors.
  // Fake credentials live in a private disposable directory under HOME.
  const base = await mkdtemp(join(homedir(), '.bucket-local-access-test-'));
  t.after(() => rm(base, { recursive: true, force: true }));
  const runner = join(base, 'runner.bundle.mjs'), uploader = join(base, 'uploader.env'), consumer = join(base, 'consumer.env');
  const marker = 'FAKE_TEST_KEY_NEVER_PRINT';
  await writeFile(runner, 'throw new Error("access preflight must not execute profile code");\n');
  const credential = `OVH_ACCESS_KEY_ID=${marker}\nOVH_SECRET_ACCESS_KEY=${marker}_SECRET\n`;
  for (const p of [uploader, consumer]) await writeFile(p, credential, { mode: 0o600 });
  const profilePath = join(base, 'profile.json');
  const profile = { schemaVersion: 1, id: 'local-access-test', enabled: true,
    runner: { path: runner, sha256: createHash('sha256').update(await readFile(runner)).digest('hex'), version: 1 },
    storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://example.invalid', region: 'sbg',
      uploaderCredentialFile: uploader, consumerCredentialFile: consumer },
    allowedAssetTrees: [base], consumers: [{ id: 'filesystem-restore', version: 1 }] };
  await writeFile(profilePath, canonical(profile), { mode: 0o600 });
  return { base, profilePath, uploader, consumer, marker, credential };
}
test('local credential preflight checks both production parsers without network or credential output', async t => {
  const f = await fixture(t);
  const out = execFileSync('node', [cli, '--profile', f.profilePath], { timeout: 15000, encoding: 'utf8' });
  const result = JSON.parse(out);
  assert.deepEqual(result.roles, { uploader: true, consumer: true });
  assert.equal(result.ok, true); assert.equal(result.networkRequests, 0);
  assert.ok(!out.includes(f.marker)); assert.ok(!out.includes('OVH_SECRET_ACCESS_KEY'));
  for (const p of [f.uploader, f.consumer]) assert.equal(await readFile(p, 'utf8'), f.credential);
});
test('local credential preflight redacts malformed and unsafe files and exits nonzero', async t => {
  const f = await fixture(t);
  await chmod(f.consumer, 0o644);
  const mode = await checkCredentialAccess(f.profilePath);
  assert.equal(mode.ok, false); assert.equal(mode.roles.uploader, true); assert.equal(mode.roles.consumer, false);
  await chmod(f.consumer, 0o600);
  await writeFile(f.consumer, `invalid secret: ${f.marker}`);
  const out = spawnSync('node', [cli, '--profile', f.profilePath], { timeout: 15000, encoding: 'utf8' });
  assert.equal(out.status, 1); assert.equal(JSON.parse(out.stdout).ok, false);
  assert.ok(!(out.stdout + out.stderr).includes(f.marker));
  assert.equal(JSON.parse(out.stdout).networkRequests, 0);
});
test('local credential preflight requires explicit valid authority and never executes profile code', async t => {
  const f = await fixture(t);
  const missing = await checkCredentialAccess(join(f.base, 'missing.json'));
  assert.equal(missing.ok, false); assert.equal(missing.networkRequests, 0);
  const out = spawnSync('node', [cli], { timeout: 15000, encoding: 'utf8' });
  assert.equal(out.status, 2);
});
