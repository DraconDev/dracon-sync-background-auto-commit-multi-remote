#!/usr/bin/env bash
# scripts/check-unit-deployment.sh — assert that the deployed systemd user units
# still match the units shipped in this repo.
#
# Mirrors dracon-system/scripts/check-unit-deployment.sh (same step numbering,
# same discovery rules, same exit contract), with sync-specific step-4 and
# step-6 checks. A unit change that lands in the repo but is never copied to
# ~/.config/systemd/user silently keeps the live service on the old unit. That
# has now shipped for sync too: MemoryDenyWriteExecute=true (2026-10-02)
# killed the process on its first regex JIT compile, and the removal sat in the
# repo while the live unit kept crashing — the reverse of the guard's F94
# direction (a fix deployed nowhere vs a breakage deployed everywhere), but the
# same class: file and manager disagreeing. A release cannot detect that on its
# own (deployment state belongs to the host), so this check makes the drift
# explicit instead of leaving it to be found by a crashing daemon.
#
# Usage: scripts/check-unit-deployment.sh [repo-unit] [deployed-unit]
# Exit codes: 0 = in sync, not deployed, or nothing checkable here;
#             1 = the deployed unit is stale, unverified, systemd has not
#                 picked the file up, or the running service cannot write a
#                 storage root its own unit names.
#
# Test seams (so the regression suite never depends on the live service):
#   SYNC_MOUNTINFO a mountinfo file to read instead of /proc/<pid>/mountinfo
#   SYNC_MAINPID   the pid whose namespace to inspect
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNIT_NAME="dracon-sync.service"
REPO_UNIT="${1:-$SCRIPT_DIR/../$UNIT_NAME}"
# systemd searches BOTH "$XDG_CONFIG_HOME/systemd/user" and
# "$HOME/.config/systemd/user" for user units, so this script searches both.
# Checking only one lets a drifted, deployed unit hide behind the other path and
# the script reports "nothing to compare" — a silent false pass, which is the
# exact failure this script exists to prevent. HOME may also be unset in a
# stripped container or CI environment, where `set -u` would turn "${HOME}/..."
# into a hard abort; an unset HOME is not drift either, it means there is no
# user unit directory to compare against. An explicit second argument always
# wins, which is what the regression suite passes.
if [ -n "${2:-}" ]; then
    DEPLOYED_UNIT="$2"
else
    unit_dirs=()
    [ -n "${XDG_CONFIG_HOME:-}" ] && unit_dirs+=("$XDG_CONFIG_HOME/systemd/user")
    [ -n "${HOME:-}" ] && unit_dirs+=("$HOME/.config/systemd/user")
    if [ ${#unit_dirs[@]} -eq 0 ]; then
        echo "• neither XDG_CONFIG_HOME nor HOME is set — no user unit directory to compare"
        exit 0
    fi
    # First candidate that actually holds the unit wins; with both present the
    # one systemd reports is preferred by the loaded-unit check further down.
    DEPLOYED_UNIT=""
    for dir in "${unit_dirs[@]}"; do
        # `-e || -L`, not `-f`: `-f` follows symlinks, so a unit symlinked to a
        # GC'd nix store path (or any dangling link) tested false and the script
        # reported "nothing to compare" — a silent false pass over a unit that IS
        # deployed and IS broken. A dangling link must reach the comparison so
        # it is reported (audit 2026-10-01).
        if [ -e "$dir/$UNIT_NAME" ] || [ -L "$dir/$UNIT_NAME" ]; then
            DEPLOYED_UNIT="$dir/$UNIT_NAME"
            break
        fi
    done
    if [ -z "$DEPLOYED_UNIT" ]; then
        primary="${unit_dirs[0]}/$UNIT_NAME"
        echo "• no deployed unit at $primary — nothing to compare (install it with:"
        echo "    mkdir -p \"\$(dirname \"$primary\")\" && install -m 644 \"$REPO_UNIT\" \"$primary\" && systemctl --user daemon-reload)"
        exit 0
    fi
fi
REDEPLOY_CMD="install -m 644 \"$REPO_UNIT\" \"$DEPLOYED_UNIT\" && systemctl --user daemon-reload"
# Overridable so the regression suite can drive the systemd-dependent steps
# deterministically instead of depending on the host's user manager.
SYSTEMCTL="${SYSTEMCTL:-systemctl}"
SYSTEMD_ANALYZE="${SYSTEMD_ANALYZE:-systemd-analyze}"

stale() {
    echo "✗ deployed unit is STALE: $DEPLOYED_UNIT" >&2
    echo "  the shipped unit in this repo differs from the deployed copy" >&2
    diff -u "$DEPLOYED_UNIT" "$REPO_UNIT" 2>/dev/null | sed -e 's/^/  /' | head -40 >&2 || true
    echo "  redeploy with:" >&2
    echo "    $REDEPLOY_CMD" >&2
    exit 1
}

# 1. Repo copy must exist — without it there is nothing to compare against.
if [ ! -f "$REPO_UNIT" ]; then
    echo "✗ repo unit copy not found: $REPO_UNIT" >&2
    exit 1
fi

# 2. A host that never installed the unit has no drift to report (containers,
#    CI, fresh clones). Silence with a one-line note, not a failure.
# `-e || -L` again, for the same reason as the discovery loop: a DANGLING unit
# symlink (a link into a GC'd nix store path, say) is deployed AND broken, and
# must reach the comparison below rather than be waved through as "not
# installed" (audit 2026-10-01).
if [ ! -e "$DEPLOYED_UNIT" ] && [ ! -L "$DEPLOYED_UNIT" ]; then
    echo "• no deployed unit at $DEPLOYED_UNIT — nothing to compare (install it with:"
    echo "    mkdir -p \$(dirname \"$DEPLOYED_UNIT\") && $REDEPLOY_CMD)"
    exit 0
fi

# 3. The core invariant: the deployed file is the shipped file.
if ! cmp -s "$REPO_UNIT" "$DEPLOYED_UNIT"; then
    stale
fi

# 3b. Companion watchdog units travel with the main unit (shipped in-repo since
# audit M8, 2026-10-02) and drift the same silent way. Each one whose repo copy
# exists AND whose deployed copy exists must agree byte for byte; a watchdog
# that was never deployed is the "fresh install without backstops" gap, which
# install.sh and the flake module close — not a drift verdict here.
deployed_dir="$(dirname "$DEPLOYED_UNIT")"
for companion in dracon-sync-watchdog.service dracon-sync-watchdog.timer \
                 dracon-freeze-watchdog.service dracon-freeze-watchdog.timer; do
    repo_companion="$SCRIPT_DIR/../$companion"
    deployed_companion="$deployed_dir/$companion"
    [ -f "$repo_companion" ] || continue
    if [ ! -e "$deployed_companion" ] && [ ! -L "$deployed_companion" ]; then
        echo "• companion $companion is shipped but not deployed — install it with:"
        echo "    install -m 644 \"$repo_companion\" \"$deployed_companion\" && systemctl --user daemon-reload"
        continue
    fi
    if ! cmp -s "$repo_companion" "$deployed_companion"; then
        echo "✗ deployed companion unit is STALE: $deployed_companion" >&2
        echo "  the shipped unit in this repo differs from the deployed copy" >&2
        diff -u "$deployed_companion" "$repo_companion" 2>/dev/null | sed -e 's/^/  /' | head -40 >&2 || true
        echo "  redeploy with:" >&2
        echo "    install -m 644 \"$repo_companion\" \"$deployed_companion\" && systemctl --user daemon-reload" >&2
        exit 1
    fi
done

# 4. The deployed file is the one systemd is running. If it is not, the file was
#    copied without `daemon-reload` and the drift is still live even though the
#    two files agree. The sentinel is sync's own incident property: the shipped
#    unit must NOT set MemoryDenyWriteExecute=true (it kills the daemon on its
#    first regex JIT compile — 2026-10-02), so a manager still LOADED with it
#    enabled after the file was fixed is exactly the "copied but never reloaded"
#    state. (The guard's checker probes ExecReload for the same reason; sync has
#    no ExecReload, so the JIT-killer takes its place.)
#
#    Both this step and step 5 need a running user manager, and a host without
#    one (CI, a container, a bare ssh session) has no opinion about the loaded
#    unit. Probing first keeps "cannot ask systemd" from being reported as
#    "systemd says no" — with no bus reachable, `systemctl --user show` and
#    `systemd-analyze --user verify` both exit non-zero ("Failed to connect to
#    user scope bus", "Failed to initialize manager") and would otherwise turn
#    an in-sync unit into a false alarm with a wrong remediation.
if command -v "$SYSTEMCTL" >/dev/null 2>&1 &&
    "$SYSTEMCTL" --user show -p Version --value >/dev/null 2>&1; then
    if grep -qE '^MemoryDenyWriteExecute=true' "$REPO_UNIT"; then
        echo "✗ the shipped unit sets MemoryDenyWriteExecute=true, which kills the" >&2
        echo "  daemon on its first regex JIT compile (2026-10-02). Remove it." >&2
        exit 1
    fi
    loaded="$("$SYSTEMCTL" --user show "$UNIT_NAME" -p MemoryDenyWriteExecute --value 2>/dev/null || true)"
    if [ "$loaded" = "yes" ]; then
        echo "✗ systemd is running $UNIT_NAME with MemoryDenyWriteExecute=yes" >&2
        echo "  although the shipped unit removed it: the file was fixed but never" >&2
        echo "  reloaded, so the live daemon still crashes on regex JIT. Run:" >&2
        echo "    systemctl --user daemon-reload && systemctl --user restart $UNIT_NAME" >&2
        exit 1
    fi

    # 5. The deployed unit must be a valid unit file, so a redeploy that
    #    "succeeded" can never leave sync unstartable. Only meaningful when
    #    there is a user manager to initialise (see step 4).
    if command -v "$SYSTEMD_ANALYZE" >/dev/null 2>&1; then
        if ! verify_out="$("$SYSTEMD_ANALYZE" --user verify "$DEPLOYED_UNIT" 2>&1)"; then
            echo "✗ systemd-analyze --user verify rejected $DEPLOYED_UNIT" >&2
            printf '%s\n' "$verify_out" | sed -e 's/^/  /' | head -20 >&2
            exit 1
        fi
    fi
else
    echo "• no reachable user systemd — skipped the loaded-unit and verify checks"
fi

# 6. The RUNTIME contract: the storage roots the unit's own ReadWritePaths names
#    must be writable in the namespace the daemon is actually running in.
#
#    Steps 3-5 compare files. They cannot see the thing that broke the guard's
#    reclaim path for 4 days (2026-09-28..10-01): a ReadWritePaths entry that
#    was correct on disk and still granted nothing, because the parent was
#    remounted read-only by ProtectSystem=strict and only a sub-mount restores
#    write access. Sync's equivalent is a daemon that can read every repo but
#    cannot write its state under ~/.dracon, its repos under ~/Dev, or its logs
#    under ~/.local/state/dracon — every commit fails while the unit looks fine.
#
#    Joining the namespace to test this needs CAP_SYS_ADMIN, which an
#    unprivileged operator shell does not have. It is still performable: the
#    kernel publishes every mount's flags in /proc/<pid>/mountinfo, and reading
#    another same-uid process's mountinfo needs no privilege beyond ptrace-mode
#    read. The longest mount that is a prefix of a root is the one that decides
#    access, so that is what gets checked. "Cannot ask" stays a note, never a
#    failure — a non-dumpable service process, a pid that exited between the
#    query and the read, or a container without /proc all mean "no opinion",
#    exactly like the absent user manager in step 4.
#
#    The roots come from the UNIT, not from policy: sync's writable contract is
#    the unit's own ReadWritePaths line (%h expanded against HOME), so the check
#    follows the sandbox instead of duplicating a second list that can drift.
unit_storage_roots() {
    # $1 = unit file. Echoes one expanded absolute path per line.
    grep -E '^[[:space:]]*ReadWritePaths=' "$1" 2>/dev/null \
        | sed -e 's/^[^=]*=//' | tr ' ' '\n' | while IFS= read -r entry; do
        # A leading `-` marks the path optional for systemd; it is still a
        # root the daemon is meant to write.
        entry="${entry#-}"
        [ -n "$entry" ] || continue
        case "$entry" in
            "%h"/*) entry="${HOME:-}/${entry#%h/}" ;;
            "%h") entry="${HOME:-}" ;;
            /*) ;;
            *) continue ;; # specifiers other than %h: not resolvable here
        esac
        [ -n "$entry" ] || continue # %h with no HOME: nothing to check
        printf '%s\n' "$entry"
    done
}
# The mount a path resolves through: the longest mount point that prefixes it.
# $1 = mountinfo file, $2 = absolute path to resolve. Echoes "<mountpoint>|<flags>".
# Plain shell rather than awk: the rule is "longest prefix wins, and among equal
# prefixes the later line shadows the earlier one", which reads more clearly
# written out than encoded, and this runs for at most a handful of roots.
effective_mount_for() {
    local target="$2" best_point="" best_flags="" best_len=-1 line point flags
    while IFS= read -r line; do
        point="$(printf '%s\n' "$line" | awk '{print $5}')"
        flags="$(printf '%s\n' "$line" | awk '{print $6}')"
        # The root mount is a prefix of everything, but "/foo" does not match
        # the shell pattern "//*", so it has to be handled before the case.
        # Skipping it would report a perfectly ordinary root — one that lives
        # under / with no mount of its own — as "not covered by any mount".
        if [ "$point" != "/" ] && [ "$target" != "$point" ]; then
            case "$target" in
                "$point"/*) ;;
                *) continue ;;
            esac
        fi
        if [ "${#point}" -ge "$best_len" ]; then
            best_len=${#point}
            best_point="$point"
            best_flags="$flags"
        fi
    done < "$1"
    [ -n "$best_point" ] || return 1
    printf '%s|%s\n' "$best_point" "$best_flags"
}

check_storage_root_writable() {
    local root="$1" mountinfo="$2"
    [ -n "$root" ] || return 0
    # FIXED 2026-10-03 (audit R3-L23): match on the CANONICAL path —
    # the old literal match false-positived on symlinked roots (live:
    # ~/.ssh -> ~/.dracon/secrets/ssh reported read-only while the
    # daemon writes fine through the rw bind). Unresolvable roots
    # keep the literal (old behavior, no new failure mode).
    local match_root="$root" canon=""
    if command -v readlink >/dev/null 2>&1; then
        canon="$(readlink -f "$root" 2>/dev/null || true)"
        [ -n "$canon" ] && match_root="$canon"
    fi
    local hit
    if ! hit="$(effective_mount_for "$mountinfo" "$match_root")"; then
        echo "✗ ReadWritePaths root $root is not covered by any mount in the running namespace" >&2
        echo "  the daemon cannot resolve or write it, so every state write against" >&2
        echo "  this root fails. Check ReadWritePaths= in $UNIT_NAME." >&2
        return 1
    fi
    local point="${hit%%|*}" flags="${hit##*|}"
    case ",$flags," in
        *,ro,*)
            echo "✗ ReadWritePaths root $root resolves read-only inside the running namespace" >&2
            echo "  effective mount: $point ($flags)" >&2
            if [ "$match_root" != "$root" ]; then
                echo "  (matched on canonical path $match_root)" >&2
            fi
            echo "  ProtectSystem=strict remounts the parent read-only, so only a" >&2
            echo "  sub-mount restores write access — the entry has to name the path" >&2
            echo "  itself, not the disk it sits on. Fix ReadWritePaths=." >&2
            return 1
            ;;
    esac
    if [ "$match_root" != "$root" ]; then
    echo "  ✓ ReadWritePaths root $root is read-write in the running namespace (mount: $point; via $match_root)"
    else
    echo "  ✓ ReadWritePaths root $root is read-write in the running namespace (mount: $point)"
    fi
    return 0
}

if [ -n "${SYNC_MAINPID:-}" ]; then
    main_pid="$SYNC_MAINPID"
elif command -v "$SYSTEMCTL" >/dev/null 2>&1; then
    main_pid="$("$SYSTEMCTL" --user show "$UNIT_NAME" -p MainPID --value 2>/dev/null || true)"
else
    main_pid=""
fi

# An explicit SYNC_MOUNTINFO replaces the /proc read entirely, so the regression
# suite can drive this step from a fixture on a host with no running service.
if [ -n "${SYNC_MOUNTINFO:-}" ]; then
    if [ ! -r "$SYNC_MOUNTINFO" ]; then
        echo "• mountinfo source $SYNC_MOUNTINFO is unreadable — cannot run the" >&2
        echo "  runtime storage-root check" >&2
        exit 1
    fi
    mountinfo="$SYNC_MOUNTINFO"
elif [ -z "$main_pid" ] || [ "$main_pid" = "0" ] || [ "$main_pid" = "N/A" ]; then
    echo "• sync service is not running — skipped the runtime storage-root check"
    mountinfo=""
elif [ -r "/proc/$main_pid/mountinfo" ]; then
    mountinfo="/proc/$main_pid/mountinfo"
else
    echo "• cannot read /proc/$main_pid/mountinfo (process not dumpable, or the" >&2
    echo "  service exited) — skipped the runtime storage-root check" >&2
    mountinfo=""
fi

if [ -n "${mountinfo:-}" ]; then
    storage_rc=0
    while IFS= read -r root; do
        check_storage_root_writable "$root" "$mountinfo" || storage_rc=1
    done < <(unit_storage_roots "$REPO_UNIT")
    [ "$storage_rc" -eq 0 ] || exit 1
fi

echo "✓ OK: deployed unit matches the shipped unit ($DEPLOYED_UNIT)."
exit 0
