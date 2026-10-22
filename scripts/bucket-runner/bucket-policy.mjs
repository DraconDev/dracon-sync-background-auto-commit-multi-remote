/**
 * Bucket policy for test traffic.
 *
 * 2026-10-21: the shared `dracon-master` bucket was split into one bucket per
 * program, and the historical leftover test prefixes in it (e2e-*, crash-*,
 * edge-*, lifecycle-*, daemon-*, demo-*, ...) were written by test runs that
 * reached a live bucket. The old writer is gone, but the *class* of accident
 * is not: any test that pairs a production bucket with a live OVH endpoint
 * writes into production data.
 *
 * This module is the single place that knows which buckets hold real data
 * and which bucket test traffic may use. It is imported by the guard test
 * (`bucket-default-guard.test.mjs`) and by any test helper that needs a
 * default bucket.
 *
 * The rule is deliberately narrow: a test may NAME a production bucket as
 * long as the endpoint is not a live OVH one (fixtures use example.invalid),
 * because the endpoint is what makes a request real. A test that points at a
 * live OVH endpoint must use the test bucket.
 */

/** Buckets that hold (or held) real data. Never a test target. */
export const PRODUCTION_BUCKETS = Object.freeze([
  'dracon-master', // retired, kept read-only until every consumer has migrated
  'dracon-games',
  'dracon-media',
  'dracon-books',
  'dracon-studio',
  'dracon-platform',
  'dracon-db',
  'dracon-lfs'
]);

/** The bucket test traffic may write to. */
export const TEST_BUCKET = 'dracon-test';

/** Live OVH Object Storage endpoints (region `uk` and the virtual-host form). */
const LIVE_ENDPOINT_HOSTS = Object.freeze(['s3.uk.io.cloud.ovh.net']);

export function isProductionBucket(bucket) {
  return PRODUCTION_BUCKETS.includes(bucket);
}

/** True when an endpoint would route a real request to OVH Object Storage. */
export function isLiveOvhEndpoint(endpoint) {
  if (typeof endpoint !== 'string' || !endpoint) return false;
  let url;
  try { url = new URL(endpoint); } catch { return false; }
  if (url.protocol !== 'https:') return false;
  return LIVE_ENDPOINT_HOSTS.includes(url.hostname) || url.hostname.endsWith('.s3.uk.io.cloud.ovh.net');
}

/**
 * Assert that a (bucket, endpoint) pair is safe for test traffic.
 *
 * Throws when a production bucket is paired with a live OVH endpoint — the
 * exact shape of the 2026-05 leak. A test bucket is always allowed; a
 * production bucket with a non-live endpoint is a fixture and is allowed.
 */
export function assertTestBucketTarget(bucket, endpoint) {
  if (!isProductionBucket(bucket)) return { bucket, endpoint, live: isLiveOvhEndpoint(endpoint) };
  if (isLiveOvhEndpoint(endpoint)) {
    throw new Error(
      `refusing to target production bucket ${bucket} at a live OVH endpoint: ` +
      'test traffic must use the test bucket (dracon-test)'
    );
  }
  return { bucket, endpoint, live: false };
}

/** The default bucket for test traffic. */
export function defaultTestBucket() {
  return TEST_BUCKET;
}
