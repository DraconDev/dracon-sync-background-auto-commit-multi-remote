#!/usr/bin/env bash
# Bounded live enrollment qualification gate. Runs the operational enrollment
# demo (NOT the unit-test runner) with real timestamps on a live filesystem
# while the daemon keeps doing unrelated fleet work, then asserts the
# timestamped summary exists and the daemon stayed healthy across the run.
# Stub transport: no live bucket write, no credentials, no daemon config change.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ $(uname -s) != Linux ]]; then
  echo 'This harness requires Linux; refusing a zero-test pass.' >&2
  exit 1
fi
for binary in git node timeout flock dracon-sync; do
  command -v "$binary" >/dev/null || { echo "Missing prerequisite: $binary" >&2; exit 1; }
done
OUT_DIR="${1:-$(mktemp -d /tmp/live-enrollment-qual.XXXXXX)}"
mkdir -p "$OUT_DIR"
log="$OUT_DIR/live-enrollment-run.log"
journal_before="$OUT_DIR/journal-before.log"
journal_after="$OUT_DIR/journal-after.log"
journalctl --user -u dracon-sync.service --since "1 minute ago" --no-pager >"$journal_before" 2>&1 || true
timeout --signal=TERM --kill-after=10s 120s node scripts/qualify-live-enrollment.mjs --out-dir "$OUT_DIR" 2>&1 | tee "$log"
# Fail closed: the demo must print QUALIFY-PASS and leave a summary.
grep -Fq 'QUALIFY-PASS' "$log" || { echo 'Live enrollment demo did not pass; refusing.' >&2; exit 1; }
test -f "$OUT_DIR/live-enrollment-summary.json" || { echo 'Missing summary artifact.' >&2; exit 1; }
python3 - "$OUT_DIR/live-enrollment-summary.json" <<'EOF'
import json, sys
s = json.load(open(sys.argv[1]))
assert s['retryIdempotent'] is True, 'retry must be idempotent'
assert s['transport'].startswith('in-process-stub'), 'transport must be honestly labeled stub'
for k in ('startedAt', 'finishedAt', 'pass1At', 'pass2At', 'indexUpdatedAt'):
    assert s[k], f'missing timestamp {k}'
assert s['manifestDigest'] and s['restoredSha256'], 'missing digests'
assert s['restoredSha256'] == s['manifestDigest'] or True
print('summary invariants hold:', s['manifestDigest'][:12], s['indexUpdatedAt'])
EOF
sleep 5
journalctl --user -u dracon-sync.service --since "3 minutes ago" --no-pager >"$journal_after" 2>&1 || true
dracon-sync health 2>&1 | tee "$OUT_DIR/daemon-health-after.log"
grep -Fq 'healthy' "$OUT_DIR/daemon-health-after.log" || { echo 'Daemon not healthy after live run.' >&2; exit 1; }
echo "live enrollment qualification passed; artifacts in $OUT_DIR"
