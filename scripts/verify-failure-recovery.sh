#!/usr/bin/env bash
# One bounded integrated episode; never starts a daemon or uses live storage.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ $(uname -s) != Linux ]]; then
  echo 'This harness requires Linux (/proc identity/state checks); refusing a zero-test pass.' >&2
  exit 1
fi
for binary in cargo git node timeout; do
  command -v "$binary" >/dev/null || { echo "Missing prerequisite: $binary" >&2; exit 1; }
done
# Fixture commits inherit the existing configured identity, never manufacture it.
git var GIT_AUTHOR_IDENT >/dev/null
git var GIT_COMMITTER_IDENT >/dev/null
log=$(mktemp /tmp/dracon-recovery-harness-output.XXXXXX)
trap 'rm -f -- "$log"' EXIT
timeout --signal=TERM --kill-after=10s 300s cargo test --locked \
  -p dracon-sync --bin dracon-sync \
  daemon::discovery_scheduler::failure_recovery_tests::integrated_failure_recovery_harness \
  -- --exact --nocapture 2>&1 | tee "$log"
# Cargo succeeds when a misspelled test filter selects zero tests. Fail closed.
grep -Fq 'test result: ok. 1 passed; 0 failed;' "$log" || {
  echo 'Harness did not execute exactly one passing scenario.' >&2
  exit 1
}
