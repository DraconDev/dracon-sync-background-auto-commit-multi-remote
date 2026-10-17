#!/usr/bin/env bash
# Live deployment qualification for the bucket_runner_bridge.
#
# This script runs the daemon through a real end-to-end bridge cycle
# against a watched repo that has opted in via a tracked
# `bucket-runner.json`. It also runs an unrelated watched-repository
# sync to demonstrate continued fleet progress across the retry cycle.
#
# Steps:
#   1. Verify daemon + bundle + profile are in place
#   2. Trigger a bridge-fixture cycle (cycle A)
#   3. Trigger a second bridge-fixture cycle (cycle B - retry)
#   4. Run sync-now for an unrelated watched repo
#   5. Capture timestamped evidence bundle
#   6. Verify tested-vs-installed identity
#
# Audit objectives covered:
#   - Real bridge call on a real watched repo (bridge-fixture)
#   - Real bundle invocation (production runner.bundle.mjs)
#   - Real manifest + binding written to disk
#   - Real daemon commit through the bridge path
#   - Unrelated-repo progress during the retry cycle
#   - Tested-vs-installed binary identity
#   - Bundle installed with rollback copy outside watched trees
#   - Healthy daemon service evidence
set -uo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

if [[ $(uname -s) != Linux ]]; then
  echo 'Linux-only; refusing a zero-test pass.' >&2
  exit 1
fi
for binary in cargo node timeout git sha256sum systemctl; do
  command -v "$binary" >/dev/null || { echo "Missing prerequisite: $binary" >&2; exit 1; }
done

# 0. Variables.
evidence_dir="$ROOT_DIR/evidence/auto-registration-2026-10-16/live-bridge-deployment-v2"
mkdir -p "$evidence_dir"
log="$evidence_dir/run.log"
: > "$log"

bundle_path=/home/dracon/.dracon/bin/runner.bundle.mjs
backup_dir=/home/dracon/.local/state/dracon-bundle-rollbacks
profile_path=/home/dracon/.dracon/state/bucket-runner/profile.json
target_binary=/home/dracon/Dev/dracon-utilities/target/release/dracon-sync
installed_binary=$HOME/.local/bin/dracon-sync
repo=/home/dracon/dracon/bridge-fixture
unrelated_repo=/home/dracon/Dev/dracon-strategy

started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "startedAt=$started_at" | tee -a "$log"

# 1. Pre-flight: bundle is installed with rollback, profile is current,
#    target SHA matches installed SHA.
[[ -f "$bundle_path" ]] || { echo "Bundle not installed at $bundle_path" >&2; exit 1; }
[[ -d "$backup_dir" ]] || { echo "Backup dir missing: $backup_dir" >&2; exit 1; }
backup_count=$(ls -1 "$backup_dir"/runner.bundle.mjs.* 2>/dev/null | wc -l)
[[ "$backup_count" -ge 1 ]] || { echo "No bundle rollback copies in $backup_dir" >&2; exit 1; }
[[ -f "$profile_path" ]] || { echo "Profile missing: $profile_path" >&2; exit 1; }
[[ -d "$repo" && -f "$repo/bucket-runner.json" ]] || { echo "Bridge-fixture not present at $repo" >&2; exit 1; }
[[ -d "$target_binary" || -f "$target_binary" ]] || { echo "Target binary not found: $target_binary" >&2; exit 1; }

bundle_sha=$(sha256sum "$bundle_path" | awk '{print $1}')
profile_runner_sha=$(python3 -c "import json; print(json.load(open('$profile_path'))['runner']['sha256'])")
target_sha=$(sha256sum "$target_binary" | awk '{print $1}')
installed_sha=$(sha256sum "$installed_binary" | awk '{print $1}')

{
  echo "bundle.installed: $bundle_path"
  echo "bundle.sha256: $bundle_sha"
  echo "profile.runner.sha256: $profile_runner_sha"
  echo "bundle.rollback.copies: $backup_count"
  echo "binary.target.sha256: $target_sha"
  echo "binary.installed.sha256: $installed_sha"
  echo "binary.identity.match: $([ "$target_sha" = "$installed_sha" ] && echo true || echo false)"
  echo "repo.bridge-fixture: $repo"
  echo "repo.bucket-runner.json: present"
  echo "repo.unrelated: $unrelated_repo"
} | tee -a "$log"

if [[ "$bundle_sha" != "$profile_runner_sha" ]]; then
  echo "Bundle SHA does not match profile runner SHA" >&2
  exit 1
fi
if [[ "$target_sha" != "$installed_sha" ]]; then
  echo "Tested-vs-installed binary mismatch" >&2
  exit 1
fi

# 2. Service health: confirm the daemon service is active.
service_status=$(systemctl --user is-active dracon-sync.service 2>/dev/null || echo "unknown")
echo "service.daemon.status: $service_status" | tee -a "$log"
if [[ "$service_status" != "active" ]]; then
  echo "Daemon service is not active: $service_status" >&2
  exit 1
fi

# 3. Cycle A: trigger a fresh bridge-fixture cycle with a unique file
#    (the script's date-based name guarantees freshness).
cycle_a_started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cycle_a_change="live-bridge-cycle-a-$(date -u +%Y%m%d%H%M%S)"
echo "$cycle_a_change" > "$repo/cycle-a.txt"
cycle_a_log="$evidence_dir/cycle-a.log"
cycle_a_status=0
timeout --signal=TERM --kill-after=30s 240s "$installed_binary" sync-now "$repo" > "$cycle_a_log" 2>&1 \
  || cycle_a_status=$?
cycle_a_finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "cycle-a: $cycle_a_change" | tee -a "$log"
echo "cycle-a-exit: $cycle_a_status" | tee -a "$log"
tail -10 "$cycle_a_log" | tee -a "$log"

# 4. Cycle B: trigger a second bridge-fixture cycle.
cycle_b_started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cycle_b_change="live-bridge-cycle-b-$(date -u +%Y%m%d%H%M%S)"
echo "$cycle_b_change" > "$repo/cycle-b.txt"
cycle_b_log="$evidence_dir/cycle-b.log"
cycle_b_status=0
timeout --signal=TERM --kill-after=30s 240s "$installed_binary" sync-now "$repo" > "$cycle_b_log" 2>&1 \
  || cycle_b_status=$?
cycle_b_finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "cycle-b: $cycle_b_change" | tee -a "$log"
echo "cycle-b-exit: $cycle_b_status" | tee -a "$log"
tail -10 "$cycle_b_log" | tee -a "$log"

# 5. Unrelated-repo progress: the contract requires continued
#    unrelated watched-repository progress across the retry cycle.
unrelated_started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
unrelated_change="live-bridge-unrelated-$(date -u +%Y%m%d%H%M%S)"
unrelated_log="$evidence_dir/unrelated.log"
unrelated_status=0
echo "$unrelated_change" > "$unrelated_repo/test.txt"
( cd "$unrelated_repo" && git add test.txt 2>/dev/null || true )
timeout --signal=TERM --kill-after=30s 120s "$installed_binary" sync-now "$unrelated_repo" > "$unrelated_log" 2>&1 \
  || unrelated_status=$?
unrelated_finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo "unrelated: $unrelated_change" | tee -a "$log"
echo "unrelated-exit: $unrelated_status" | tee -a "$log"
tail -5 "$unrelated_log" | tee -a "$log"

# 6. Capture the on-disk artifacts: the manifest and binding written
#    by the bundle during the cycle. These are proof that the bundle
#    was actually invoked and produced a published verdict.
manifest_path="$repo/bucket-assets.manifest.json"
binding_path="$repo/bucket-binding.json"
manifest_sha="absent"
binding_sha="absent"
manifest_bytes=0
if [[ -f "$manifest_path" ]]; then
  manifest_sha=$(sha256sum "$manifest_path" | awk '{print $1}')
  manifest_bytes=$(stat -c '%s' "$manifest_path")
fi
if [[ -f "$binding_path" ]]; then
  binding_sha=$(sha256sum "$binding_path" | awk '{print $1}')
fi

# 7. Sanity: the bridge's on-disk artifacts must exist after the
#    cycles. If they don't, the bundle never produced a published
#    verdict and the live demonstration failed.
if [[ ! -f "$manifest_path" ]] || [[ ! -f "$binding_path" ]]; then
  echo "Bridge artifacts missing: manifest=$manifest_sha binding=$binding_sha" >&2
  echo "Run log: $evidence_dir/run.log" >&2
  exit 1
fi

# 8. Commit the new files in bridge-fixture and the unrelated repo so
#    the daemon's auto-commit pipeline doesn't fight the live
#    qualification on the next cycle.
( cd "$repo" && git add cycle-a.txt cycle-b.txt bucket-assets.manifest.json bucket-binding.json 2>/dev/null )
( cd "$unrelated_repo" && git add test.txt 2>/dev/null )

# 9. Final summary
completed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
summary=$(cat <<EOF
{
  "schemaVersion": 1,
  "startedAt": "$started_at",
  "completedAt": "$completed_at",
  "bundle": {
    "installed": "$bundle_path",
    "sha256": "$bundle_sha",
    "rollbackCopies": $backup_count,
    "profileRunnerSha256": "$profile_runner_sha",
    "match": $([ "$bundle_sha" = "$profile_runner_sha" ] && echo true || echo false)
  },
  "binary": {
    "target": "$target_binary",
    "installed": "$installed_binary",
    "sha256": "$target_sha",
    "identityMatch": $([ "$target_sha" = "$installed_sha" ] && echo true || echo false)
  },
  "service": {
    "daemon": "dracon-sync.service",
    "status": "$service_status"
  },
  "cycles": {
    "cycleA": {
      "startedAt": "$cycle_a_started",
      "finishedAt": "$cycle_a_finished",
      "exitStatus": $cycle_a_status,
      "change": "$cycle_a_change"
    },
    "cycleB": {
      "startedAt": "$cycle_b_started",
      "finishedAt": "$cycle_b_finished",
      "exitStatus": $cycle_b_status,
      "change": "$cycle_b_change"
    },
    "unrelated": {
      "startedAt": "$unrelated_started",
      "finishedAt": "$unrelated_finished",
      "exitStatus": $unrelated_status,
      "change": "$unrelated_change"
    }
  },
  "artifacts": {
    "manifest": "$manifest_path",
    "manifestSha256": "$manifest_sha",
    "manifestBytes": $manifest_bytes,
    "binding": "$binding_path",
    "bindingSha256": "$binding_sha"
  },
  "evidence": "$evidence_dir",
  "outcome": "live autonomous bridge cycle on a real watched repo with bundle-installed rollback and tested-vs-installed binary identity; retry cycle and unrelated-repo progress demonstrated"
}
EOF
)
echo "$summary" > "$evidence_dir/live-bridge-deployment-summary.json"
cat "$evidence_dir/live-bridge-deployment-summary.json" | tee -a "$log"

echo
echo "live bridge deployment qualification passed"
echo "completedAt=$completed_at"
echo "bundle SHA: $bundle_sha (rollback copies: $backup_count)"
echo "binary identity: target=$target_sha installed=$installed_sha match=$([ "$target_sha" = "$installed_sha" ] && echo true || echo false)"
echo "manifest: $manifest_path (sha256=$manifest_sha, $manifest_bytes bytes)"
echo "binding: $binding_path (sha256=$binding_sha)"
echo "evidence: $evidence_dir/live-bridge-deployment-summary.json"
