#!/usr/bin/env bash
# Hermetic daemon sync-worker -> real runner/publisher -> injected storage I/O.
# This is a top-level driver, NEVER called from a collected test. No installation,
# live daemon, OVH credentials, network transport, or existing manifest required.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
[[ $(uname -s) == Linux ]] || { echo 'Linux required; refusing zero-test success.' >&2; exit 1; }
for binary in cargo node timeout flock git; do
  command -v "$binary" >/dev/null || { echo "Missing prerequisite: $binary" >&2; exit 1; }
done
log=$(mktemp /tmp/daemon-bridge-e2e.XXXXXX)
trap 'status=$?; if [[ $status -ne 0 ]]; then tail -60 "$log" >&2; fi; rm -f -- "$log"' EXIT
# Reuse normal debug artifacts. Installed/release identity is a separate live
# deployment gate; source harness success must not imply production execution.
timeout --signal=TERM --kill-after=10s 240s env CARGO_BUILD_JOBS=1 \
  cargo test -p dracon-sync --locked bucket_qualification_daemon -- --test-threads=1 \
  >"$log" 2>&1
required=(
  bucket_qualification_daemon_new_enrollment_retry_and_fresh_restore
  bucket_qualification_daemon_outage_partial_upload_and_recovery
  bucket_qualification_daemon_concurrent_manifest_preserves_independent_writer
)
for name in "${required[@]}"; do
  grep -Eq "^test sync::bucket_qualification_tests::${name} \\.\\.\\. ok$" "$log" || {
    echo "Required daemon-worker scenario missing or not passed: $name" >&2; exit 1;
  }
done
grep -Fq 'test result: ok. 3 passed; 0 failed; 0 ignored;' "$log" || {
  echo 'Expected three real daemon-worker scenarios; refusing partial/zero-test success.' >&2; exit 1;
}
timeout --signal=TERM --kill-after=10s 150s bash scripts/verify-bucket-registration-e2e.sh >>"$log" 2>&1
tail -4 "$log"
echo 'Hermetic daemon-worker qualification passed: fresh enrollment/commit, recovery, concurrency, retry and byte-identical fresh restore.'
echo 'Storage I/O injected only; no production installation or live OVH qualification claimed.'
