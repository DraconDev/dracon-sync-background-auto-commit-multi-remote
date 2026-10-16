/** Timestamped live enrollment qualification run (operational, not a unit test).
 *
 * Executes the qualified publisher end-to-end against a scratch git repo:
 * plan -> publish (pass 1) -> timestamped manifest + index -> retry pass
 * (pass 2, must be idempotent) -> fresh-consumer restore with byte verify.
 * Transport is an in-process stub: NO live bucket write occurs, no
 * credentials are read, no daemon config is changed. The point of this run
 * is timestamped evidence that the qualified code path enrolls, binds and
 * restores on a live filesystem with real time, while the live daemon keeps
 * doing unrelated fleet work (captured separately from the journal).
 *
 * Exit 0 only if every invariant holds; any failure exits non-zero.
 * Usage: node scripts/qualify-live-enrollment.mjs --out-dir <dir>
 */
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { renderSolidPng, CONSUMER_ID, CONSUMER_VERSION } from './bucket-runner/adapters.mjs';
import { planRepository, publishRepository } from './bucket-runner/publisher.mjs';
import { assertObjectKey, bindTransport } from './bucket-runner/capabilities.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const stamp = () => new Date().toISOString();
const fail = msg => { console.error(`QUALIFY-FAIL: ${msg}`); process.exit(1); };
const note = msg => console.log(`[${stamp()}] ${msg}`);

const outDir = process.argv[process.argv.indexOf('--out-dir') + 1];
if (!outDir) { console.error('usage: qualify-live-enrollment.mjs --out-dir <dir>'); process.exit(2); }

const profile = {
  schemaVersion: 1, id: 'games-scoped-v1', enabled: true,
  runner: { path: '/installed/runner.mjs', sha256: 'a'.repeat(64), version: 1 },
  storage: { bucket: 'dracon-lfs', namespaceRoot: 'games', endpoint: 'https://s3.example.invalid',
    region: 'sbg', uploaderCredentialFile: '/operator/uploader.env', consumerCredentialFile: '/operator/reader.env' },
  allowedAssetTrees: ['/tmp'], consumers: [{ id: CONSUMER_ID, version: CONSUMER_VERSION }]
};
const binding = { schemaVersion: 1, profileId: 'games-scoped-v1', repositoryId: 'repo-live-qual',
  namespace: 'games/auto/repo-live-qual' };
const consumer = { id: CONSUMER_ID, version: CONSUMER_VERSION, assetRoots: ['authorized'] };

function git(cwd, ...args) {
  return execFileSync('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-C', cwd, ...args],
    { env: { ...process.env, GIT_OPTIONAL_LOCKS: '0', GIT_CONFIG_NOSYSTEM: '1',
      GIT_CONFIG_GLOBAL: '/dev/null', LC_ALL: 'C' } });
}
function makeTransport() {
  const objects = new Map();
  const operations = [];
  return {
    objects, operations,
    async head(key) { operations.push(['head', key]); return objects.has(key); },
    async putIfAbsent(key, file) {
      operations.push(['put', key]);
      if (objects.has(key)) { const e = new Error('PreconditionFailed'); e.$metadata = { httpStatusCode: 412 }; throw e; }
      objects.set(key, await readFile(file));
    },
    async download(key, file) {
      operations.push(['download', key]);
      if (!objects.has(key)) { const e = new Error('NotFound'); e.$metadata = { httpStatusCode: 404 }; throw e; }
      await writeFile(file, objects.get(key));
    }
  };
}

await mkdir(outDir, { recursive: true });
const startedAt = stamp();
note(`live enrollment qualification started (stub transport; no live bucket write)`);

// --- Fixture: scratch git repo under /tmp (never inside a watched tree) ---
const root = await mkdtemp(join(tmpdir(), 'live-qual-'));
const restoreRoot = await mkdtemp(join(tmpdir(), 'live-qual-restore-'));
note(`scratch repo: ${root}`);
git(root, 'init', '-q');
git(root, 'config', 'user.email', 'qualify@example.invalid');
git(root, 'config', 'user.name', 'live-qual');
await mkdir(join(root, 'authorized'), { recursive: true });
const manifestPath = join(root, 'manifest.json');
const indexPath = join(root, 'index', 'binding.json');
const recipe = { width: 32, height: 32, rgba: [255, 46, 136, 255] };
const bytes = renderSolidPng(recipe);
await writeFile(join(root, 'authorized/icon.png'), bytes);
note(`source asset written: authorized/icon.png sha256=${sha(bytes)} bytes=${bytes.length}`);

// --- Pass 1: initial enrollment ---
const trans = makeTransport();
const t1 = stamp();
const first = await publishRepository({ root, profile, binding, consumer, assetRoot: '.',
  manifestPath, transport: trans, indexPath });
if (!first.manifestWritten) fail(`pass 1 did not write manifest: ${first.reason}`);
note(`pass 1 published at ${t1}: manifestWritten=true toUpload=${first.toUpload.length} indexRebound=${first.indexRebound}`);
const manifestRaw = await readFile(manifestPath, 'utf8');
const manifest = JSON.parse(manifestRaw);
const index = JSON.parse(await readFile(indexPath, 'utf8'));
if (manifest.assets.length !== 1 || manifest.assets[0].bucketed !== true) fail('pass 1 manifest must hold one bucketed asset');
if (manifest.assets[0].sha256 !== sha(bytes)) fail('pass 1 manifest digest must match source bytes');
if (index.manifestDigest !== sha(manifestRaw)) fail('index must bind the written manifest digest');
if (!index.updatedAt || Number.isNaN(Date.parse(index.updatedAt))) fail('index must carry a valid updatedAt timestamp');
if (trans.objects.size !== 1) fail('exactly one object must be stored');
note(`manifest digest: ${sha(manifestRaw)}`);
note(`index updatedAt: ${index.updatedAt} namespace=${index.namespace}`);

// --- Pass 2: retry cycle (must be idempotent) ---
const t2 = stamp();
const putsBefore = trans.operations.filter(([op]) => op === 'put').length;
const second = await publishRepository({ root, profile, binding, consumer, assetRoot: '.',
  manifestPath, transport: trans, indexPath });
if (second.manifestWritten) fail('pass 2 (retry) must not rewrite the manifest');
const putsAfter = trans.operations.filter(([op]) => op === 'put').length;
if (putsAfter !== putsBefore) fail('pass 2 (retry) must not upload again');
const manifestRaw2 = await readFile(manifestPath, 'utf8');
if (sha(manifestRaw2) !== sha(manifestRaw)) fail('manifest must be byte-identical across the retry cycle');
note(`pass 2 retry at ${t2}: manifestWritten=false uploads=0 manifest-digest-unchanged`);

// --- Fresh-consumer restore (policy-based routing: namespace-bound key) ---
const readOnly = bindTransport(trans, profile, binding, { readOnly: true });
const entry = manifest.assets[0];
const ext = entry.sourcePath.split('.').at(-1).toLowerCase();
const key = assertObjectKey(profile, binding, `${manifest.namespace}/${entry.sha256}.${ext}`);
if (!key.startsWith('games/auto/repo-live-qual/')) fail('object key must stay inside the bound namespace');
const target = join(restoreRoot, entry.sourcePath);
await mkdir(dirname(target), { recursive: true });
await readOnly.download(key, target);
const restored = await readFile(target);
if (sha(restored) !== entry.sha256 || restored.length !== entry.bytes) fail('restored bytes must match the registry digest');
note(`fresh-consumer restore: ${entry.sourcePath} sha256=${sha(restored)} bytes=${restored.length} (verified)`);

// --- Summary artifact ---
const summary = { startedAt, finishedAt: stamp(), pass1At: t1, pass2At: t2,
  manifestDigest: sha(manifestRaw), indexUpdatedAt: index.updatedAt,
  namespace: index.namespace, objectKey: key, restoredSha256: sha(restored),
  retryIdempotent: true, transport: 'in-process-stub (no live bucket write)' };
await writeFile(join(outDir, 'live-enrollment-summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
await writeFile(join(outDir, 'live-enrollment-manifest.json'), manifestRaw);
console.log(JSON.stringify(summary));
note('QUALIFY-PASS: timestamped enrollment, retry idempotence and fresh restore all hold');
await rm(root, { recursive: true, force: true });
await rm(restoreRoot, { recursive: true, force: true });
