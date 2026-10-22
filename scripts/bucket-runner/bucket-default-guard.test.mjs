/**
 * Guard: test traffic must never target a production bucket.
 *
 * The May-2026 leftover prefixes in the shared bucket (e2e-*, crash-*, edge-*,
 * lifecycle-*, daemon-*, demo-*, writer-*, flock-*, m6-*, dummy-test,
 * s3-cloud-demo) were written by test runs that reached the live bucket. That
 * writer is gone, so this guard protects the *class* of accident instead:
 * no test file in this repository may pair a production bucket with a live
 * OVH endpoint, and the default bucket for test traffic must not be a
 * production bucket.
 */
import test from 'node:test';
import assert from 'node:assert/strict';
import { readdir, readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import {
  PRODUCTION_BUCKETS,
  TEST_BUCKET,
  isProductionBucket,
  isLiveOvhEndpoint,
  assertTestBucketTarget,
  defaultTestBucket
} from './bucket-policy.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const SCRIPTS_DIR = join(HERE, '..');
const REPO_DIR = join(SCRIPTS_DIR, '..', '..');

/** Every source file in the repository that could carry a (bucket, endpoint) pair. */
async function candidateFiles() {
  const out = [];
  const skip = new Set(['node_modules', 'target', '.git', 'evidence', '.pi-glla', 'vendor', 'dist']);
  async function walk(dir) {
    let entries;
    try { entries = await readdir(dir, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      if (skip.has(entry.name)) continue;
      const full = join(dir, entry.name);
      if (entry.isDirectory()) { await walk(full); continue; }
      if (!/\.(mjs|js|ts|rs|py|sh)$/u.test(entry.name)) continue;
      out.push(full);
    }
  }
  await walk(REPO_DIR);
  return out;
}

/** Pull (bucket, endpoint) pairs out of a source file. */
function pairsIn(text) {
  const pairs = [];
  const bucketPattern = /['"`]?bucket['"`]?\s*[:=]\s*['"`]([a-z0-9][a-z0-9-]*)['"`]/giu;
  const endpointPattern = /['"`]?endpoint(?:_url)?['"`]?\s*[:=]\s*['"`]([^'"`]+)['"`]/giu;
  const buckets = [...text.matchAll(bucketPattern)].map((m) => m[1]);
  const endpoints = [...text.matchAll(endpointPattern)].map((m) => m[1]);
  for (const bucket of buckets) {
    for (const endpoint of endpoints) pairs.push({ bucket, endpoint });
  }
  return pairs;
}

test('bucket_default_never_production', async () => {
  // 1. The default bucket for test traffic is not a production bucket.
  assert.equal(defaultTestBucket(), TEST_BUCKET);
  assert.equal(isProductionBucket(defaultTestBucket()), false, 'the test bucket must not be a production bucket');

  // 2. No file in the repository pairs a production bucket with a live OVH
  //    endpoint. Fixtures may name a production bucket as long as the
  //    endpoint is a non-routable one (example.invalid).
  const files = await candidateFiles();
  const violations = [];
  for (const file of files) {
    let text;
    try { text = await readFile(file, 'utf8'); } catch { continue; }
    for (const { bucket, endpoint } of pairsIn(text)) {
      if (!isProductionBucket(bucket)) continue;
      if (!isLiveOvhEndpoint(endpoint)) continue;
      violations.push(`${relative(REPO_DIR, file)}: ${bucket} @ ${endpoint}`);
    }
  }
  assert.deepEqual(violations, [], 'test traffic must not target a production bucket at a live endpoint');

  // 3. The guard actually bites: a production bucket at a live endpoint is
  //    refused, on a disposable fixture copy so nothing live is touched.
  assert.throws(
    () => assertTestBucketTarget('dracon-games', 'https://s3.uk.io.cloud.ovh.net'),
    /refusing to target production bucket dracon-games/u
  );
  assert.throws(
    () => assertTestBucketTarget('dracon-master', 'https://dracon-master.s3.uk.io.cloud.ovh.net'),
    /refusing to target production bucket dracon-master/u
  );

  // 4. ...and it lets the safe shapes through: the test bucket anywhere, and
  //    a production bucket name against a non-routable fixture endpoint.
  assert.doesNotThrow(() => assertTestBucketTarget(TEST_BUCKET, 'https://s3.uk.io.cloud.ovh.net'));
  assert.doesNotThrow(() => assertTestBucketTarget('dracon-lfs', 'https://s3.example.invalid'));

  // 5. The fixture copy proves the failure is about the bucket, not the file:
  //    the same pair in a disposable file is rejected the same way.
  const dir = await mkdtemp(join(tmpdir(), 'bucket-default-guard-'));
  try {
    const fixture = join(dir, 'production-default.mjs');
    await writeFile(fixture, "export const storage = { bucket: 'dracon-games', endpoint: 'https://s3.uk.io.cloud.ovh.net' };\n");
    const text = await readFile(fixture, 'utf8');
    const pairs = pairsIn(text);
    assert.equal(pairs.length, 1);
    assert.throws(() => assertTestBucketTarget(pairs[0].bucket, pairs[0].endpoint), /refusing to target production bucket/u);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('production bucket list covers every bucket the fleet owns', () => {
  // The list is the guard's knowledge; a bucket missing from it would be a
  // silent hole, so it is asserted against the known fleet inventory.
  for (const bucket of ['dracon-master', 'dracon-games', 'dracon-media', 'dracon-books', 'dracon-studio', 'dracon-platform', 'dracon-db', 'dracon-lfs']) {
    assert.ok(PRODUCTION_BUCKETS.includes(bucket), `${bucket} must be listed as a production bucket`);
  }
  assert.ok(!PRODUCTION_BUCKETS.includes(TEST_BUCKET));
});
