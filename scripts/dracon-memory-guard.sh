#!/usr/bin/env bash
# Dracon memory-pressure guard (user-level, no root required).
#
# WHY THIS EXISTS
#   2026-11-11: the host collapsed to 182 MiB free RAM with ~1.6 GB/s of swap
#   traffic. The dracon-sync daemon was the load source (1.4 GiB RAM + 1.2 GiB
#   swap, 4.6 TB read I/O, a hung `git push` keeping repos in-flight). The
#   consequence was NOT a slow daemon -- it was a dead desktop: KWin's amdgpu
#   pageflip path timed out once per second for 10.5 minutes
#   ("Pageflip timed out! This is a bug in the amdgpu kernel driver"), the
#   screens blacked out, and every window stopped repainting while the apps
#   underneath stayed alive.
#
#   The existing dracon-system guard can only ADVISORY this (renice +
#   oom_score_adj), which a git-churning daemon simply ignores. This guard
#   does the one thing that actually stops the load: it freezes the sync
#   daemon via the daemon's own supported freeze marker, before the host
#   reaches the cliff.
#
# WHAT IT DOES
#   1. Every 30s: sample reclaimable headroom, PSI memory (full avg10) and
#      swap-in velocity from /proc.
#   2. FREEZE  -- when the host is heading into swap thrash AND the sync
#      daemon is a material part of it, pause the sync daemon
#      (dracon-sync pause -> ~/.dracon/dracon-sync.freeze). Requires the
#      condition on two consecutive checks so a transient spike cannot freeze
#      sync on its own, and requires the footprint gate below so host pressure
#      caused by OTHER tenants never brakes the fleet.
#
# FOOTPRINT GATE (2026-11-12, operator decision)
#   The original rule froze on host headroom alone. Measured 2026-11-12: the
#   daemon sat at 81 MiB RSS while tmux held 971 MiB and load was ~52; the
#   guard still parked all 38 repos, which is host pressure that braking the
#   sync cannot relieve. The 2026-11-11 incident it was written for looked
#   different: the daemon itself was 1.4 GiB RSS + 1.2 GiB swap with a HUNG
#   git push keeping repos in-flight.
#
#   So a freeze additionally requires the daemon to be holding at least
#   FREEZE_RSS_FLOOR_MB (default 512) of NON-RECLAIMABLE memory — the
#   cgroup's anon + unreclaimable slab + kernel_stack. That is the one
#   quantity that both (a) explains swap thrash and (b) the daemon must
#   actually be responsible for. Page cache and reclaimable slab are
#   excluded: they free themselves under pressure. A push merely RUNNING is
#   not a signal either — this fleet's healthy 4-gate runs take 12-26
#   minutes, so "busy" would freeze the fleet most of the day.
#   Neither condition present => the pressure is somebody else's and we do
#   not brake. Set MEMORY_GUARD_FREEZE_RSS_FLOOR_MB to tune, or to 0 to
#   restore the old host-only behaviour.
#   3. RECOVER -- after a bounded pause, request a graceful daemon restart
#      to release retained memory, at most once per cooldown.
#   4. UNFREEZE -- only the marker THIS script created is ever removed, after
#      recovery or when sync no longer materially contributes to pressure.
#      A freeze the operator set by hand is never touched.
#   5. UNSTICK THE COMPOSITOR -- if KWin logged new amdgpu pageflip timeouts
#      since the last run, ask KWin to rebuild its scenegraph
#      (org.kde.KWin /KWin reconfigure). This is the software equivalent of
#      unplugging and replugging a monitor, done automatically.
#
# NOTE ON "AVAILABLE" MEMORY
#   /proc/meminfo MemAvailable was observed reading exactly 0 on this host
#   during the incident, so it is NOT used as the primary signal. Headroom is
#   computed as MemFree + Buffers + Cached + SReclaimable, and PSI/swap-in
#   velocity carry the rest of the signal.
#
# STATE
#   ~/.dracon/memory/memory-guard.state
set -u

STATE_DIR="${MEMORY_GUARD_STATE_DIR:-$HOME/.dracon/memory}"
STATE_FILE="$STATE_DIR/memory-guard.state"
MARKER="${MEMORY_GUARD_MARKER:-$HOME/.dracon/dracon-sync.freeze}"
ALTERNATE_MARKER="$HOME/.dracon/freeze/dracon-sync"
SYNC_BIN="${MEMORY_GUARD_SYNC_BIN:-$HOME/.local/bin/dracon-sync}"
KWIN_UNIT="${MEMORY_GUARD_KWIN_UNIT:-plasma-kwin_wayland.service}"
# Explicit fixture roots also make the pressure decisions testable without
# changing host memory, signalling the real daemon or touching manual markers.
PROC_ROOT="${MEMORY_GUARD_PROC_ROOT:-/proc}"
CGROUP_ROOT="${MEMORY_GUARD_CGROUP_ROOT:-/sys/fs/cgroup}"
RECOVERY_HELPER="${MEMORY_GUARD_RECOVERY_HELPER:-$HOME/.dracon/scripts/recover-sync-memory.py}"
RECOVER_AFTER_SECS="${MEMORY_GUARD_RECOVER_AFTER_SECS:-120}"
RECOVERY_COOLDOWN_SECS="${MEMORY_GUARD_RECOVERY_COOLDOWN_SECS:-1800}"

# Thresholds (see rationale above). Overridable for tuning and for tests.
FREEZE_HEADROOM_PCT="${MEMORY_GUARD_FREEZE_HEADROOM_PCT:-8}"        # freeze below this much reclaimable headroom
FREEZE_PSI="${MEMORY_GUARD_FREEZE_PSI:-15.0}"                        # ... or this much full-stall pressure ...
FREEZE_PSI_HEADROOM_PCT="${MEMORY_GUARD_FREEZE_PSI_HEADROOM_PCT:-25}"  # ... combined with headroom this low
FREEZE_SWAPIN_PAGES="${MEMORY_GUARD_FREEZE_SWAPIN_PAGES:-150000}"    # ... or this swap-in velocity (pages/s)
FREEZE_STREAK="${MEMORY_GUARD_FREEZE_STREAK:-2}"                     # consecutive bad checks before freezing
UNFREEZE_HEADROOM_PCT="${MEMORY_GUARD_UNFREEZE_HEADROOM_PCT:-25}"    # release only above this
UNFREEZE_PSI="${MEMORY_GUARD_UNFREEZE_PSI:-5.0}"                      # ... and below this
UNFREEZE_STREAK="${MEMORY_GUARD_UNFREEZE_STREAK:-3}"                  # ... for this many consecutive checks
# CHANGED 2026-11-13: a second release path. This host's baseline headroom
# under a full agent fleet sits around 10-20%, so the >25% rule alone can keep
# sync frozen until the 30m watchdog even after the stall is long over. If PSI
# has stayed calm for this many consecutive checks (default 10 x 30s = 5 min)
# and headroom is above the freeze floor, release anyway.
UNFREEZE_CALM_STREAK="${MEMORY_GUARD_UNFREEZE_CALM_STREAK:-10}"
# Footprint gate: a freeze also needs the daemon (plus its git children) to
# hold at least this much non-reclaimable memory. 0 disables the
# gate entirely (pre-2026-11-12 host-only freezing).
FREEZE_RSS_FLOOR_MB="${MEMORY_GUARD_FREEZE_RSS_FLOOR_MB:-512}"
DAEMON_UNIT="${MEMORY_GUARD_DAEMON_UNIT:-dracon-sync-operator.service}"

mkdir -p "$STATE_DIR" 2>/dev/null || true
touch "$STATE_FILE" 2>/dev/null || true

read_state() {
    # shellcheck disable=SC1090
    . "$STATE_FILE" 2>/dev/null || true
    LAST_PSWPIN="${LAST_PSWPIN:-0}"
    LAST_CHECK_ISO="${LAST_CHECK_ISO:-1970-01-01T00:00:00}"
    HEALTHY_STREAK="${HEALTHY_STREAK:-0}"
    BAD_STREAK="${BAD_STREAK:-0}"
    MARKER_INODE="${MARKER_INODE:-0}"
    MARKER_MTIME="${MARKER_MTIME:-0}"
    FROZEN_BY_US="${FROZEN_BY_US:-0}"
    LAST_RECOVERY_EPOCH="${LAST_RECOVERY_EPOCH:-0}"
    UNATTRIBUTED_STREAK="${UNATTRIBUTED_STREAK:-0}"
}

write_state() {
    cat > "$STATE_FILE" <<EOF
LAST_PSWPIN=$LAST_PSWPIN
LAST_CHECK_ISO=$LAST_CHECK_ISO
HEALTHY_STREAK=$HEALTHY_STREAK
BAD_STREAK=$BAD_STREAK
MARKER_INODE=$MARKER_INODE
MARKER_MTIME=$MARKER_MTIME
FROZEN_BY_US=$FROZEN_BY_US
LAST_RECOVERY_EPOCH=$LAST_RECOVERY_EPOCH
UNATTRIBUTED_STREAK=$UNATTRIBUTED_STREAK
EOF
}

meminfo_kb() { awk -v k="$1:" '$1 == k {print $2; exit}' "$PROC_ROOT/meminfo"; }

# Read one field out of a cgroup v2 memory.stat file and return it in kB.
# cgroup v2 memory.stat values are BYTES (cgroup v1 memory.usage_in_bytes too);
# treating them as kB inflates the reading 1024x, which would make the freeze
# gate always pass. Caught 2026-11-12 by cross-checking against MemoryCurrent.
memstat_kb() {
    local bytes
    bytes=$(awk -v k="$2" '$1 == k {print $2; exit}' "$1" 2>/dev/null)
    case "${bytes:-}" in
        ""|*[!0-9]*) echo 0 ;;
        *) echo $((bytes / 1024)) ;;
    esac
}

read_state

TOTAL_KB=$(meminfo_kb MemTotal)
[ -n "${TOTAL_KB:-}" ] && [ "$TOTAL_KB" -gt 0 ] || { echo "guard: cannot read MemTotal"; exit 0; }
HEADROOM_KB=$(awk '
    $1=="MemFree:"{f=$2} $1=="Buffers:"{b=$2} $1=="Cached:"{c=$2}
    $1=="SReclaimable:"{r=$2} END{print f+b+c+r}' "$PROC_ROOT/meminfo")
HEADROOM_PCT=$((HEADROOM_KB * 100 / TOTAL_KB))

PSI_FULL=$(awk '/^full/ { for (i = 1; i <= NF; i++) if ($i ~ /^avg10=/) { split($i, a, "="); print a[2]; exit } }' "$PROC_ROOT/pressure/memory" 2>/dev/null)
[ -n "${PSI_FULL:-}" ] || PSI_FULL=0

PSWPIN=$(awk '$1 == "pswpin" {print $2; exit}' "$PROC_ROOT/vmstat")
[ -n "${PSWPIN:-}" ] || PSWPIN=0
DT=30
if [ "$LAST_PSWPIN" -gt 0 ] && [ "$PSWPIN" -ge "$LAST_PSWPIN" ]; then
    SWAPIN_PAGES=$(( (PSWPIN - LAST_PSWPIN) / DT ))
else
    SWAPIN_PAGES=0
fi

# ── 1. compositor recovery ───────────────────────────────────────────────────
# KWin's amdgpu pageflip path wedges the whole desktop when it stalls. If new
# timeouts appeared since the last check, ask KWin to rebuild the scenegraph.
PAGEFLIP_NEW=$(journalctl --user -u "$KWIN_UNIT" --since "$LAST_CHECK_ISO" --no-pager 2>/dev/null | grep -c 'Pageflip timed out' || true)
if [ "${PAGEFLIP_NEW:-0}" -gt 0 ]; then
    if command -v qdbus >/dev/null 2>&1; then
        if qdbus org.kde.KWin /KWin reconfigure >/dev/null 2>&1; then
            echo "guard: ${PAGEFLIP_NEW} new KWin pageflip timeout(s) -> compositor reconfigured"
        else
            echo "guard: ${PAGEFLIP_NEW} new KWin pageflip timeout(s); reconfigure failed"
        fi
    else
        echo "guard: ${PAGEFLIP_NEW} new KWin pageflip timeout(s); qdbus unavailable"
    fi
fi

# ── 2. freeze / unfreeze the sync daemon ────────────────────────────────────
REASON=""
if [ "$HEADROOM_PCT" -lt "$FREEZE_HEADROOM_PCT" ]; then
    REASON="headroom ${HEADROOM_PCT}% < ${FREEZE_HEADROOM_PCT}%"
elif awk -v p="$PSI_FULL" -v t="$FREEZE_PSI" 'BEGIN {exit !(p+0 >= t+0)}'; then
    if [ "$HEADROOM_PCT" -lt "$FREEZE_PSI_HEADROOM_PCT" ]; then
        REASON="psi full avg10 ${PSI_FULL} >= ${FREEZE_PSI} with headroom ${HEADROOM_PCT}%"
    fi
fi
if [ -z "$REASON" ] && [ "$SWAPIN_PAGES" -ge "$FREEZE_SWAPIN_PAGES" ]; then
    REASON="swap-in ${SWAPIN_PAGES} pages/s >= ${FREEZE_SWAPIN_PAGES}"
fi

# ── 2a. footprint gate ─────────────────────────────────────────────────────
# See "FOOTPRINT GATE" in the header. Returns 0 when the sync daemon is a
# material part of the pressure (freeze may proceed), 1 when it is not.
#
# Reads the real systemd unit and its non-reclaimable cgroup memory.
# The descendant RSS walk is only a fallback when cgroup accounting is absent.
daemon_footprint_kb() {
    # What we want is the memory the daemon is actually CONSUMING pressure with
    # — i.e. memory that cannot be reclaimed on demand. NOT MemoryCurrent,
    # which counts page cache and reclaimable slab too.
    #
    # Measured 2026-11-12: MemoryCurrent 817 MiB decomposed into
    #   anon 155 MiB | file 477 MiB | slab 181 MiB (unreclaimable only 0.2 MiB)
    # so the unit's true non-reclaimable footprint was ~155 MiB while a naive
    # MemoryCurrent read said 817 MiB — enough to pass a 512 MiB floor and
    # re-introduce the exact false positive this gate exists to remove.
    # (MemoryCurrent is bounded by MemoryHigh/Max, so with no cap set it also
    # tracks the host-wide free-memory trend and drifts upward under pressure.)
    #
    # So: read the cgroup's own memory.stat and use
    #     anon + slab_unreclaimable
    # which is precisely "memory we cannot get back without swapping or
    # killing". The 2026-11-11 incident (1.4 GiB anon + 1.2 GiB swap) still
    # clears any sane floor; idle page cache never does.
    local cg stat anon slab kern
    cg=$(systemctl show -p ControlGroup --value "$DAEMON_UNIT" 2>/dev/null | sed -n '1p')
    if [ -n "${cg:-}" ] && [ -r "${CGROUP_ROOT}${cg}/memory.stat" ]; then
        stat="${CGROUP_ROOT}${cg}/memory.stat"
    elif [ -n "${cg:-}" ] && [ -r "${CGROUP_ROOT}/system.slice/${cg##*/}/memory.stat" ]; then
        stat="${CGROUP_ROOT}/system.slice/${cg##*/}/memory.stat"
    fi
    if [ -n "${stat:-}" ] && [ -r "$stat" ]; then
        anon=$(memstat_kb "$stat" anon)
        slab=$(memstat_kb "$stat" slab_unreclaimable)
        kern=$(memstat_kb "$stat" kernel_stack)
        echo $(( ${anon:-0} + ${slab:-0} + ${kern:-0} ))
        return
    fi
    # Fallback: RSS of the daemon plus every descendant (a gate runner spawns
    # a tree, and the hung push of the 11-11 incident lived in a child).
    local pid rss=0 child queue cur kids
    pid=$(systemctl show -p MainPID --value "$DAEMON_UNIT" 2>/dev/null || echo 0)
    [ -n "${pid:-}" ] && [ "$pid" -gt 0 ] 2>/dev/null || { echo 0; return; }
    rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ')
    [ -n "${rss:-}" ] || rss=0
    queue="$pid"
    while [ -n "$queue" ]; do
        cur=${queue%% *}
        case "$queue" in *" "*) queue=${queue#* } ;; *) queue="" ;; esac
        kids=$(pgrep -P "$cur" 2>/dev/null | tr '\n' ' ')
        for child in $kids; do
            local crss
            crss=$(ps -o rss= -p "$child" 2>/dev/null | tr -d ' ')
            [ -n "${crss:-}" ] || crss=0
            rss=$((rss + crss))
            queue="$queue $child"
        done
    done
    echo "${rss:-0}"
}

footprint_gate_ok() {
    # 0 disables the gate: behave exactly as before 2026-11-12.
    if [ "${FREEZE_RSS_FLOOR_MB:-512}" -le 0 ]; then
        return 0
    fi
    local floor_kb rss
    floor_kb=$((FREEZE_RSS_FLOOR_MB * 1024))
    rss=$DAEMON_FOOTPRINT_KB
    # The whole decision: is the daemon holding enough NON-RECLAIMABLE memory
    # to be part of the pressure at all? A hung push shows up here as growing
    # anon; a failing pre-push hook does not, and must not brake the fleet.
    [ "${rss:-0}" -ge "$floor_kb" ]
}

# A marker deleted by the watchdog or replaced by an operator is no longer
# ours, even while pressure persists. Never resume somebody else's pause.
if [ "$FROZEN_BY_US" -eq 1 ]; then
    CUR_INODE=$(stat -c %i "$MARKER" 2>/dev/null || echo 0)
    CUR_MTIME=$(stat -c %Y "$MARKER" 2>/dev/null || echo 0)
    if [ "$CUR_INODE" != "$MARKER_INODE" ] || [ "$CUR_MTIME" != "$MARKER_MTIME" ]; then
        FROZEN_BY_US=0
        HEALTHY_STREAK=0
        UNATTRIBUTED_STREAK=0
        echo "guard: owned freeze marker was removed or replaced; relinquishing ownership"
    fi
fi

# A second supported marker represents an independent pause. The CLI's resume
# removes both, so neither automatic recovery nor resume may touch this state.
MANUAL_HOLD=0
if [ -e "$ALTERNATE_MARKER" ]; then
    MANUAL_HOLD=1
fi

DAEMON_FOOTPRINT_KB=$(daemon_footprint_kb)
ATTRIBUTED=1
if ! footprint_gate_ok; then
    ATTRIBUTED=0
    if [ -n "$REASON" ]; then
        echo "guard: host pressure not caused by sync -- ${REASON} (daemon holds ${DAEMON_FOOTPRINT_KB} kB of non-reclaimable memory < ${FREEZE_RSS_FLOOR_MB} MB); not freezing"
        REASON=""
    fi
fi

if [ -n "$REASON" ]; then
    BAD_STREAK=$((BAD_STREAK + 1))
    HEALTHY_STREAK=0
    UNATTRIBUTED_STREAK=0
    if [ "$BAD_STREAK" -ge "$FREEZE_STREAK" ]; then
        if [ "$FROZEN_BY_US" -eq 0 ]; then
            if [ ! -e "$MARKER" ] && [ "$MANUAL_HOLD" -eq 0 ]; then
                if "$SYNC_BIN" pause >/dev/null 2>&1; then
                    FROZEN_BY_US=1
                    MARKER_INODE=$(stat -c %i "$MARKER" 2>/dev/null || echo 0)
                    MARKER_MTIME=$(stat -c %Y "$MARKER" 2>/dev/null || echo 0)
                    echo "guard: FROZE sync daemon -- ${REASON}"
                else
                    echo "guard: freeze failed -- ${REASON}"
                fi
            else
                echo "guard: sync already frozen by another actor -- ${REASON}"
            fi
        else
            now=$(date +%s)
            if [ "$MANUAL_HOLD" -eq 0 ] &&
               [ $((now - MARKER_MTIME)) -ge "$RECOVER_AFTER_SECS" ] &&
               [ $((now - LAST_RECOVERY_EPOCH)) -ge "$RECOVERY_COOLDOWN_SECS" ]; then
                if "$RECOVERY_HELPER" --unit "$DAEMON_UNIT" --executable "$SYNC_BIN"; then
                    LAST_RECOVERY_EPOCH=$now
                    echo "guard: requested graceful memory recovery; owned freeze retained until footprint falls"
                else
                    echo "guard: graceful recovery unavailable; owned freeze remains protected by the watchdog"
                fi
            else
                echo "guard: sync stays frozen -- ${REASON} (waiting for recovery delay/cooldown)"
            fi
        fi
    fi
else
    BAD_STREAK=0
    if [ "$FROZEN_BY_US" -eq 1 ] && [ "$MANUAL_HOLD" -eq 0 ]; then
        HEALTHY_STREAK=$((HEALTHY_STREAK + 1))
        if [ "$ATTRIBUTED" -eq 0 ]; then
            UNATTRIBUTED_STREAK=$((UNATTRIBUTED_STREAK + 1))
        else
            UNATTRIBUTED_STREAK=0
        fi
        # Other tenants' PSI cannot hold a freshly restarted, small daemon
        # frozen. The same footprint floor that controls entry controls exit.
        if [ "$UNATTRIBUTED_STREAK" -ge "$UNFREEZE_STREAK" ] ||
           { awk -v p="$PSI_FULL" -v t="$UNFREEZE_PSI" 'BEGIN {exit !(p+0 < t+0)}' &&
             { { [ "$HEALTHY_STREAK" -ge "$UNFREEZE_STREAK" ] &&
                 [ "$HEADROOM_PCT" -gt "$UNFREEZE_HEADROOM_PCT" ]; } ||
               { [ "$HEALTHY_STREAK" -ge "$UNFREEZE_CALM_STREAK" ] &&
                 [ "$HEADROOM_PCT" -ge "$FREEZE_HEADROOM_PCT" ]; }; }; }; then
            if "$SYNC_BIN" resume >/dev/null 2>&1; then
                echo "guard: released sync freeze -- headroom ${HEADROOM_PCT}%, psi ${PSI_FULL}, sync footprint ${DAEMON_FOOTPRINT_KB} kB"
                FROZEN_BY_US=0
                HEALTHY_STREAK=0
                UNATTRIBUTED_STREAK=0
            else
                echo "guard: resume failed; retaining ownership for retry"
            fi
        fi
    fi
fi

LAST_PSWPIN=$PSWPIN
LAST_CHECK_ISO=$(date -u +%Y-%m-%dT%H:%M:%S)
write_state
exit 0
