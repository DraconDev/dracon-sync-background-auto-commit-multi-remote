#!/usr/bin/env bash
# Regression test for scripts/check-task-budget-parity.sh.
#
# The check must:
#   1. pass on the repository's own sources (they agree),
#   2. FAIL when one repository source drifts (the regression this script
#      exists for: the Nix module kept 96 while the shipped unit said 512),
#   3. tolerate a harmless change (a comment, a reordered key, an unrelated
#      key) without reporting drift, and
#   4. report live-unit drift as deployment drift, not as a source failure, and
#   5. NOT report drift for a live unit whose drop-in has already been applied
#      (the base file still says 96 while the effective ceiling is 512).
#
# All fixtures are disposable copies in a temp dir; the live unit is never
# touched.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
CHECK="$SCRIPT_DIR/check-task-budget-parity.sh"

[ -x "$CHECK" ] || { echo "check script is not executable" >&2; exit 1; }

work="$(mktemp -d "${TMPDIR:-/tmp}/dracon-task-budget-parity-XXXXXX")"
trap 'rm -rf "$work"' EXIT

# --- 1. the repository's own sources agree -------------------------------
out="$("$CHECK" 2>&1)" || { echo "repository sources failed the parity check:" >&2; echo "$out" >&2; exit 1; }
grep -q 'task budget is consistent' <<<"$out" || { echo "unexpected output for the repository sources: $out" >&2; exit 1; }

# --- 2. a drifted repository source is a hard failure --------------------
# The check resolves its sources relative to its own location, so the
# fixture needs its own copy of the script.
mkdir -p "$work/drift/scripts"
cp "$REPO/dracon-sync.service" "$work/drift/dracon-sync.service"
cp "$REPO/scripts/dracon-sync-operator.nix" "$work/drift/scripts/dracon-sync-operator.nix"
cp "$CHECK" "$work/drift/scripts/check-task-budget-parity.sh"
chmod +x "$work/drift/scripts/check-task-budget-parity.sh"
sed -i 's/^\([[:space:]]*\)TasksMax = 512;/\1TasksMax = 96;/' "$work/drift/scripts/dracon-sync-operator.nix"
grep -q 'TasksMax = 96;' "$work/drift/scripts/dracon-sync-operator.nix" \
    || { echo "fixture did not apply the drift" >&2; exit 1; }
if (cd "$work/drift" && ./scripts/check-task-budget-parity.sh >/dev/null 2>&1); then
    echo "check accepted a drifted task budget — the regression is unguarded" >&2
    exit 1
fi

# --- 3. a harmless change is tolerated -----------------------------------
mkdir -p "$work/harmless/scripts"
cp "$REPO/dracon-sync.service" "$work/harmless/dracon-sync.service"
cp "$REPO/scripts/dracon-sync-operator.nix" "$work/harmless/scripts/dracon-sync-operator.nix"
# harmless: an extra comment line in each source
printf '\n# an unrelated note that changes no limit\n' >> "$work/harmless/scripts/dracon-sync-operator.nix"
printf '\n# a plain comment in the shipped unit\n' >> "$work/harmless/dracon-sync.service"
cp "$CHECK" "$work/harmless/scripts/check-task-budget-parity.sh"
chmod +x "$work/harmless/scripts/check-task-budget-parity.sh"
out="$(cd "$work/harmless" && ./scripts/check-task-budget-parity.sh 2>&1)" \
    || { echo "check rejected a harmless change:" >&2; echo "$out" >&2; exit 1; }
grep -q 'task budget is consistent' <<<"$out" \
    || { echo "harmless change was not tolerated: $out" >&2; exit 1; }

# --- 4. live-unit drift is deployment drift, not a source failure ---------
mkdir -p "$work/live"
cat > "$work/live/unit" <<'EOF'
[Service]
TasksMax=96
EOF
out="$(LIVE_UNIT="$work/live/unit" "$CHECK" 2>&1)" \
    || { echo "check failed on deployment drift:" >&2; echo "$out" >&2; exit 1; }
grep -q 'DEPLOYMENT DRIFT' <<<"$out" \
    || { echo "deployment drift was not reported: $out" >&2; exit 1; }
grep -q 'task budget is consistent' <<<"$out" \
    || { echo "deployment drift must not read as a source failure: $out" >&2; exit 1; }

# --- 5. an applied drop-in is not drift ----------------------------------
# The live unit's budget resolves the way systemd resolves it: a drop-in in
# <unit>.d/ overrides the base unit file. Reading the base file alone
# reported drift for a drop-in that was already installed, and kept advising
# a re-install of it.
mkdir -p "$work/applied/unit.d"
cat > "$work/applied/unit" <<'EOF'
[Service]
TasksMax=96
EOF
cat > "$work/applied/unit.d/30-tasksmax.conf" <<'EOF'
[Service]
TasksMax=512
EOF
out="$(LIVE_UNIT="$work/applied/unit" "$CHECK" 2>&1)" \
    || { echo "check failed on an applied drop-in:" >&2; echo "$out" >&2; exit 1; }
if grep -q 'DEPLOYMENT DRIFT' <<<"$out"; then
    echo "check reported drift for a unit whose drop-in was already applied:" >&2
    echo "$out" >&2
    exit 1
fi
grep -q 'live unit' <<<"$out" \
    || { echo "live unit not reported: $out" >&2; exit 1; }

echo "✓ task budget parity check: drift detected, harmless change tolerated, deployment drift reported, applied drop-in not misread as drift"
