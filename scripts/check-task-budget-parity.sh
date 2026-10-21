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
#   * the live system unit           — reported as DEPLOYMENT DRIFT: applying
#     a change to it needs root, so it is a warning with the exact command,
#     not a hard failure
#
# The live unit's budget is read the way systemd resolves it, NOT by reading
# the base unit file alone: a drop-in in <unit>.d/*.conf overrides the base
# value, and `systemctl show` reports the merged result. Reading only the base
# file reported drift for a service whose drop-in had already been applied
# (the base file still said 96 while the effective ceiling was 512) and kept
# advising a re-install of a drop-in that was already installed. When it is
# readable, the kernel's cgroup pids.max is reported as well: that is the
# limit the running service is actually held to, and it only changes when the
# service restarts — a merged unit saying 512 is not yet a running service
# held to 512.
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

# The effective budget of a systemd unit, resolved the way systemd resolves
# it: drop-ins in <unit>.d/ override the base unit file, and the last
# drop-in wins. Prints nothing when no budget is declared anywhere.
read_effective_tasks_max() {
    local unit_file="$1"
    local value v f dir
    value="$(read_tasks_max "$unit_file")"
    for dir in "$(dirname -- "$unit_file")/$(basename -- "$unit_file").d" \
               "/etc/systemd/system/$(basename -- "$unit_file").d"; do
        [ -d "$dir" ] || continue
        for f in "$dir"/*.conf; do
            [ -f "$f" ] || continue
            v="$(read_tasks_max "$f")"
            if [ -n "$v" ]; then value="$v"; fi
        done
    done
    printf '%s' "$value"
}

# The live unit's budget. `systemctl show` is authoritative — it reports the
# merged base unit plus drop-ins. When it cannot answer (systemd not
# reachable, or a fixture path that is not a real unit), the file resolution
# above is used instead.
read_live_tasks_max() {
    local unit_file="$1"
    local unit_name props id value
    unit_name="$(basename -- "$unit_file")"
    if command -v systemctl >/dev/null 2>&1; then
        props="$(systemctl show "$unit_name" -p Id -p TasksMax 2>/dev/null || true)"
        id="$(printf '%s\n' "$props" | sed -n -E 's/^Id=(.*)$/\1/p' | head -n1)"
        value="$(printf '%s\n' "$props" | sed -n -E 's/^TasksMax=(.*)$/\1/p' | head -n1)"
        # `systemctl show` on a name that is not a live unit falls back to the
        # caller's own scope and reports that scope's properties, so the Id
        # has to match the unit that was asked for before the value is used.
        if [ "$id" = "$unit_name" ] && [ -n "$value" ] && [ "$value" != "infinity" ]; then
            printf '%s' "$value"
            return 0
        fi
    fi
    read_effective_tasks_max "$unit_file"
}

# The ceiling the kernel actually enforces for the running service. It only
# changes when the service (re)starts, so it can lag the merged unit.
read_cgroup_tasks_max() {
    local unit_name="$1"
    local f v
    for f in "/sys/fs/cgroup/system.slice/$unit_name/pids.max" \
             "/sys/fs/cgroup/$unit_name/pids.max"; do
        [ -r "$f" ] || continue
        v="$(cat "$f" 2>/dev/null || true)"
        if [ -n "$v" ]; then
            printf '%s' "$v"
            return 0
        fi
    done
    return 0
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
    live="$(read_live_tasks_max "$LIVE_UNIT")"
    live_name="$(basename -- "$LIVE_UNIT")"
    if [ -z "$live" ]; then
        echo "• live unit ($LIVE_UNIT): no TasksMax declared (no ceiling)"
    elif [ "$live" != "$shipped" ]; then
        echo "⚠ DEPLOYMENT DRIFT: live unit ($LIVE_UNIT) enforces TasksMax=$live," >&2
        echo "  repository sources say $shipped. Applying it needs root:" >&2
        echo "    sudo install -m 644 scripts/30-tasksmax.conf \\" >&2
        echo "      /run/systemd/system/dracon-sync-operator.service.d/30-tasksmax.conf" >&2
        echo "    sudo systemctl daemon-reload" >&2
        echo "    kill -TERM \$(systemctl show $live_name -p MainPID --value)" >&2
    else
        echo "• live unit ($LIVE_UNIT): TasksMax=$live"
    fi
    cgroup="$(read_cgroup_tasks_max "$live_name")"
    if [ -z "$cgroup" ]; then
        :
    elif [ "$cgroup" != "$live" ]; then
        echo "⚠ the running service still holds TasksMax=$cgroup (cgroup pids.max) —" >&2
        echo "  the merged unit says $live, which only takes effect on restart:" >&2
        echo "    kill -TERM \$(systemctl show $live_name -p MainPID --value)" >&2
    else
        echo "• running service cgroup pids.max: $cgroup"
    fi
else
    echo "• live unit ($LIVE_UNIT) not present — deployment drift not checkable here"
fi

echo "✓ task budget is consistent across repository sources (TasksMax=$shipped)"
