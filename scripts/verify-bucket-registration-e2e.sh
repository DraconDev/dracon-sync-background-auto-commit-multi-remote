#!/usr/bin/env bash
# One bounded end-to-end bucket registration qualification; never starts a
# daemon or uses live storage. Runs the bucket-runner component suite (planner,
# publisher, adapters, capabilities) AND the new e2e harness that exercises
# full publish → fresh-consumer-restore plus the bounded fault/recovery
# scenarios required by the goal verification contract.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ $(uname -s) != Linux ]]; then
  echo 'This harness requires Linux (/proc identity/state checks); refusing a zero-test pass.' >&2
  exit 1
fi
for binary in git node timeout flock; do
  command -v "$binary" >/dev/null || { echo "Missing prerequisite: $binary" >&2; exit 1; }
done
log=$(mktemp /tmp/dracon-bucket-e2e-output.XXXXXX)
trap 'rm -f -- "$log"' EXIT
# The full bucket-runner suite covers discovery, capabilities, publisher, e2e
# registration, and bounded fault recovery (storage outage, partial upload,
# concurrent manifest change, daemon interruption, rename, idempotence).
timeout --signal=TERM --kill-after=10s 120s node --test \
  scripts/bucket-runner/adapters.test.mjs \
  scripts/bucket-runner/capabilities.test.mjs \
  scripts/bucket-runner/bindings.test.mjs \
  scripts/bucket-runner/routing.test.mjs \
  scripts/bucket-runner/storage.test.mjs \
  scripts/bucket-runner/consumer.test.mjs \
  scripts/bucket-runner/restore.test.mjs \
  scripts/bucket-runner/planner.test.mjs \
  scripts/bucket-runner/publisher.test.mjs \
  scripts/bucket-runner/runner.test.mjs \
  scripts/bucket-runner/e2e.test.mjs 2>&1 | tee "$log"
# The runner must execute every named suite. A misspelled filter or missing
# script would silently pass. Fail closed.
grep -Eq '^.*tests 153$' "$log" || {
  echo 'Bucket-runner suite did not execute 153 tests; refusing a zero-test pass.' >&2
  exit 1
}
grep -Eq '^.*pass 153$' "$log" || {
  echo 'Bucket-runner suite did not pass 153 tests; refusing a partial pass.' >&2
  exit 1
}
# Required e2e scenarios must have run (every goal verification contract bullet).
required=(
  'e2e: unregistered eligible asset enrolls, verifies and restores on a fresh consumer'
  'e2e: ineligible assets are never uploaded, the manifest is empty and no transport calls leak'
  'e2e: repeat enrollment is idempotent (no second upload, no second manifest write)'
  'recovery: storage outage during upload keeps local worktree and manifest absent; retry succeeds'
  'recovery: partial upload (storage lies about head()) is detected, manifest absent, retry succeeds'
  'recovery: concurrent manifest change aborts the second publisher; retry re-reads and succeeds'
  'recovery: simulated daemon interruption between storage upload and manifest write preserves invariant'
  'e2e: fresh consumer restores a renamed asset and refuses a deleted one'
  'publicationLock: readiness waits for kernel ownership, not a grace timer'
  'publicationLock: rejects symlink and timeout without leaking ownership'
  'publishRepository: successful PUT with corrupt stored bytes cannot publish'
  'publishRepository: concurrent creation of previously absent manifest is retained'
  'publishRepository: raced source is revalidated before any upload'
  'publishRepository: generator changes during upload prevent stale references'
  'publishRepository: rejects manifest escape and symlink ancestors before transport'
  'failed state write or fsync cleans temporary files and permits retry'
  'routing: small qualified assets remain local below measured pressure'
  'publishRepository: small assets register locally then route when measured pressure rises'
  'publishRepository: absent routing budget refuses before storage or manifest mutation'
  'publishRepository: changed managed asset renews proof and preserves old immutable object'
  'publishRepository: unsupported edits retain previous manifest and edited source'
  'SDK error bodies are bounded before deserialization for downloads and uploads'
  'credential ancestor authority refuses foreign owners even with mode 0755'
  'independent checkout beneath parent Git/package boundary remains eligible for text qualification'
  'consumer restore: corruption or interruption leaves final destination absent and retry succeeds'
  'publishRepository: failed index write leaves manifest absent and retry re-verifies'
  'publishRepository: sealed operator candidate set cannot be enlarged by new files'
  'runner recovers from a stranded index-ahead transaction and never imports a mismatched index'
  'runner suppresses only positively re-verified bucketed entries and does not stage deleted ones'
  'runner local-only entries stay unenrolled and are reported bucketed:false with local-only reason'
)
for scenario in "${required[@]}"; do
  grep -Fq "$scenario" "$log" || {
    echo "Required scenario did not run: $scenario" >&2
    exit 1
  }
done
echo 'bucket-runner qualification passed: 153/153 tests, all required e2e scenarios executed.'
