#!/usr/bin/env bash
# Shipped by dracon-utilities (audit M8, 2026-10-02): previously live-only.
# install.sh copies this to ~/.dracon/sync-notify/ (see the watchdog .service).
# dracon-freeze-watchdog — warn on forgotten `dracon-sync pause` and auto-clear.
#
# Rationale (2026-08-24): a manual `dracon-sync pause` at 17:26 left the fleet
# frozen for 3.5h (14 repos PENDING, 2 WARN). The daemon's hard TTL was 24h,
# so it would have stayed frozen for a day. This watchdog bounds forgotten
# pauses to minutes: warn at 10m, auto-clear at 30m, daemon hard-clears at 1h.
#
# The sanctioned quiesce path is `dracon-sync maintenance -- <cmd>` which
# pauses, runs the command, and ALWAYS resumes — no freeze leak. Manual
# `pause`/`resume` is for interactive multi-step work and must be resumed.
# This watchdog is the mechanical backstop for the "forgot to resume" case.
#
# Markers (see dracon-sync/src/policy.rs:freeze_marker_paths):
#   ~/.dracon/dracon-sync.freeze
#   ~/.dracon/freeze/dracon-sync
set -u

WARN_SECS=600    # 10 minutes
CLEAR_SECS=1800  # 30 minutes

for marker in "$HOME/.dracon/dracon-sync.freeze" "$HOME/.dracon/freeze/dracon-sync"; do
    [ -f "$marker" ] || continue
    # stat -c %Y = mtime epoch seconds (GNU coreutils); stat -f %m = BSD.
    # FIXED 2026-10-03 (audit R3-L29): when BOTH fail (the normal
    # `resume` racing this 2-min tick deletes the marker between the
    # `-f` and the `stat`), SKIP — the old `|| echo 0` produced
    # mtime=0 → huge age → a false "auto-clearing" line plus notify
    # and logger noise misattributed to operator action.
    mtime=$(stat -c %Y "$marker" 2>/dev/null || stat -f %m "$marker" 2>/dev/null || true)
    case "$mtime" in
        ""|*[!0-9]*) continue ;;
    esac
    now=$(date +%s)
    age=$((now - mtime))
    if [ "$age" -gt "$CLEAR_SECS" ]; then
        echo "⚠️ dracon-freeze-watchdog: freeze marker $marker stale ${age}s (>30m) — auto-clearing (forgotten pause at $(date -d "@$mtime" 2>/dev/null || date -r "$mtime" 2>/dev/null))"
        rm -f "$marker"
        # Also notify via systemd journal and desktop if available
        if command -v notify-send >/dev/null 2>&1; then
            notify-send -u critical "dracon-sync" "Freeze auto-cleared after ${age}s — was paused since $(date -d "@$mtime" +"%H:%M" 2>/dev/null || echo "$mtime")" 2>/dev/null || true
        fi
        logger -t dracon-freeze-watchdog "auto-cleared stale freeze $marker age=${age}s" 2>/dev/null || true
    elif [ "$age" -gt "$WARN_SECS" ]; then
        echo "⚠️ dracon-freeze-watchdog: freeze marker $marker active ${age}s (>10m) — forgotten pause? will auto-clear at 30m (created $(date -d "@$mtime" +"%H:%M" 2>/dev/null || echo "$mtime"))"
        # FIXED 2026-09-27: this warn branch was journal-only. The
        # notify-send call lived on the auto-clear branch ALONE, so the
        # operator was notified when the problem was already being fixed
        # and never notified when the notification could still help.
        # Live 2026-09-27: a pause at 15:14:59 froze 8 repos at PENDING
        # for 32 minutes; the operator found it by reading `repos`, not
        # from a notification. AGENTS.md already claims this branch
        # notifies ("logs to the journal and sends notify-send if
        # available") — the code did not, so this restores the
        # documented behavior.
        if command -v notify-send >/dev/null 2>&1; then
            mins=$((age / 60))
            notify-send -u critical "dracon-sync" "Sync frozen ${mins}m — forgotten pause? auto-clears at 30m. Run: dracon-sync resume" 2>/dev/null || true
        fi
        logger -t dracon-freeze-watchdog "freeze active age=${age}s $marker" 2>/dev/null || true
    fi
done
