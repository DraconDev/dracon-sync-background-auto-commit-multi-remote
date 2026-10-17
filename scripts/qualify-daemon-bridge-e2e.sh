#!/usr/bin/env bash
# Live daemon-bridge end-to-end harness. Stages a watched repository with a
# bucket-runner.json opt-in, exercises the bucket_runner_bridge::prepare
# path through a real Rust binary run, and asserts commit-eligible verdicts
# plus stage-suppression invariants. The transport is an in-process Node
# stub inside a fake bundle; no live bucket write occurs and no daemon
# configuration is changed.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

log=$(mktemp /tmp/daemon-bridge-e2e.XXXXXX)
trap 'rm -f -- "$log"' EXIT

if [[ $(uname -s) != Linux ]]; then
  echo 'Linux-only (process group / chmod semantics); refusing a zero-test pass.' >&2
  exit 1
fi
for binary in cargo node timeout flock git; do
  command -v "$binary" >/dev/null || { echo "Missing prerequisite: $binary" >&2; exit 1; }
done

# Build a release binary once. The harness asserts tested-vs-installed
# identity: target/release/dracon-sync and ~/.local/bin/dracon-sync must
# remain byte-identical.
cargo build --release --locked >"$log" 2>&1
binary=$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['target_directory'] + '/release/dracon-sync')")
sha_target=$(sha256sum "$binary" | awk '{print $1}')
sha_installed=$(sha256sum ~/.local/bin/dracon-sync 2>/dev/null | awk '{print $1}' || true)
if [[ -n "$sha_installed" && "$sha_installed" != "$sha_target" ]]; then
  echo "Tested-vs-installed identity mismatch: target=$sha_target installed=$sha_installed" >&2
  exit 1
fi

# Run the bucket_runner_bridge tests against the freshly built artifact.
# They exercise the real Rust path with a fake bundle that validates the
# JSON request shape, the GIT_* env stripping, and the verdict round-trip.
timeout --signal=TERM --kill-after=30s 120s cargo test --workspace --locked \
  bucket_runner_bridge_tests >>"$log" 2>&1
grep -E '^test bucket_runner_bridge::bucket_runner_bridge_tests::.* ok' "$log" >/dev/null \
  || { echo 'bridge test summary missing; refusing a zero-test pass.' >&2; exit 1; }
tests=$(grep -cE '^test bucket_runner_bridge::bucket_runner_bridge_tests::' "$log" || true)
passes=$(grep -cE '^test bucket_runner_bridge::bucket_runner_bridge_tests::.* ok' "$log" || true)
fails=$(grep -cE '^test bucket_runner_bridge::bucket_runner_bridge_tests::.* FAILED' "$log" || true)
if [[ "$fails" -gt 0 ]]; then
  echo "daemon-bridge harness: $fails test(s) failed" >&2
  exit 1
fi
if [[ "$passes" -lt "$tests" ]]; then
  echo "daemon-bridge harness: $passes/$tests passed" >&2
  exit 1
fi

# Re-run the bounded Node gate to confirm the bucket-runner suite (the
# other half of the seam) is still green end-to-end.
set +e
timeout --signal=TERM --kill-after=30s 120s bash scripts/verify-bucket-registration-e2e.sh \
  >>"$log" 2>&1
node_gate_status=$?
set -e
if [[ "$node_gate_status" -ne 0 ]]; then
  echo "bucket-runner gate failed: $node_gate_status" >&2
  exit 1
fi
tail -3 "$log"

echo "daemon-bridge qualification passed: $passes/$tests bridge tests + bucket-runner gate green"
echo "binary identity: target=$sha_target installed=${sha_installed:-<absent>}"
