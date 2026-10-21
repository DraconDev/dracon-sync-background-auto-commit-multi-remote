#!/usr/bin/env bash
# scripts/check-task-budget-parity.sh — assert that the daemon's task budget
# (TasksMax) is the same everywhere it is defined.
#
# Why this exists: the daemon's systemd unit was generated on 2026-10-19 with
# TasksMax=96, while the shipped unit in this repo was later raised to 512
# (Git filters and Node pre-push workers are the consumers that hit the
# ceiling) and the NixOS operator module kept 96. Nothing compared the three
# sources, so the live service kept enforcing the old limit for days and the
# drift was invisible until the daemon was inspected under load.
#
# Sources checked:
#   * dracon-sync.service            — the shipped user unit
#   * scripts/dracon-sync-operator.nix — the NixOS operator-unit module
#   * /run/systemd/system/dracon-sync-operator.service — the live system unit
#     (reported as DEPLOYMENT DRIFT: applying it needs root, so it is a
#     warning with the exact command, not a hard failure)
#
# Exit codes:
#   0 = every source agrees, or only the live unit differs (deployment drift)
#   1 = the repository sources disagree with each other (fixable in-repo)
#   2 = a source could not be read or declares no task budget
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

SHIPPED_UNIT="dracon-sync.service"
NIX_MODULE="scripts/dracon-sync-operator.nix"
LIVE_UNIT="${LIVE_UNIT:-/run/systemd/system/dracon-sync-operator.service}"

# Read TasksMax from a systemd unit file (Key=Value) or a Nix module
# (Key = "Value";). Prints nothing when the key is absent.
read_tasks_max() {
    local file="$1"
    [ -f "$file" ] || return 0
    sed -n -E \
        -e 's/^[[:space:]]*TasksMax[[:space:]]*=[[:space:]]*"?([0-9]+)"?[[:space:]]*;?[[:space:]]*$/\1/p' \
        "$file" | head -n1
}

shipped="$(read_tasks_max "$SHIPPED_UNIT")"
nix_module="$(read_tasks_max "$NIX_MODULE")"

fail=0
for pair in "shipped unit:$SHIPPED_UNIT:$shipped" "nix module:$NIX_MODULE:$nix_module"; do
    label="${pair%%:*}"
    rest="${pair#*:}"
    file="${rest%%:*}"
    value="${rest##*:}"
    if [ -z "$value" ]; then
        echo "✗ $label ($file) declares no TasksMax" >&2
        fail=1
    else
        echo "• $label ($file): TasksMax=$value"
    fi
done

if [ "$fail" -ne 0 ]; then
    echo "  fix: give every source the same task budget (the shipped unit and the" >&2
    echo "        parent flake's needSync \"TasksMax\" \"512\" check are the reference)" >&2
    exit 1
fi

if [ "$shipped" != "$nix_module" ]; then
    echo "✗ task budget drift between repository sources:" >&2
    echo "    $SHIPPED_UNIT: $shipped" >&2
    echo "    $NIX_MODULE: $nix_module" >&2
    exit 1
fi

if [ -f "$LIVE_UNIT" ]; then
    live="$(read_tasks_max "$LIVE_UNIT")"
    if [ -z "$live" ]; then
        echo "• live unit ($LIVE_UNIT): no TasksMax declared (no ceiling)"
    elif [ "$live" != "$shipped" ]; then
        echo "⚠ DEPLOYMENT DRIFT: live unit ($LIVE_UNIT) enforces TasksMax=$live," >&2
        echo "  repository sources say $shipped. Applying it needs root:" >&2
        echo "    sudo install -m 644 scripts/30-tasksmax.conf \\" >&2
        echo "      /run/systemd/system/dracon-sync-operator.service.d/30-tasksmax.conf" >&2
        echo "    sudo systemctl daemon-reload" >&2
    else
        echo "• live unit ($LIVE_UNIT): TasksMax=$live"
    fi
else
    echo "• live unit ($LIVE_UNIT) not present — deployment drift not checkable here"
fi

echo "✓ task budget is consistent across repository sources (TasksMax=$shipped)"
