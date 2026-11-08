#!/usr/bin/env bash
# Shipped by dracon-utilities (audit M8, 2026-10-02): previously live-only.
# install.sh copies this to ~/.dracon/sync-notify/ (see the watchdog .service).
# dracon-sync watchdog — restart the sync daemon if it was stopped.
#
# Rationale (2026-08-07): remediation procedures used to call
# `systemctl --user stop dracon-sync.service` around git surgery.
# A manual stop has NO systemd backstop (Restart=always only covers
# crashes), so a forgotten restart left the fleet unsynced. This
# watchdog mechanically guarantees the daemon can never stay stopped:
# if the service is inactive and no maintenance hold is present, it
# is restarted within ~2.5 minutes (timer period + jitter).
#
# The sanctioned quiesce path is `dracon-sync pause` / `resume` or
# `dracon-sync maintenance -- <cmd...>` — the daemon keeps RUNNING and
# skips cycles instead (self-healing via the 24h freeze TTL). This
# watchdog only fires when someone stopped the service outright.
#
# Escape hatch for genuine downtime (release installs, hardware work):
#   touch ~/.dracon/dracon-sync.maintenance-hold
# The watchdog then skips restart until the marker is removed.
#
# CORRECTED 2026-08-10 (audit LOW): the marker is a MANUAL escape
# hatch — no script touches it. (An earlier comment claimed
# dracon-sync/scripts/release.sh wraps the binary swap with the
# marker; it does not, and nothing else does either.) Operators who
# stop the daemon for a release install MUST touch the marker BEFORE
# stopping and remove it AFTERWARDS — otherwise the watchdog restarts
# the daemon mid-swap ~2.5 minutes later. The sanctioned quiesce path
# that needs no marker is `dracon-sync pause`/`maintenance` (freeze,
# daemon keeps running).
#
# CHANGED 2026-11-08 (watchdog-premise repair): this script used to watch
# ONLY the user-scope `dracon-sync.service`. On a host where that unit is
# MASKED — which is this host, since 2026-10-19, when the live daemon
# moved to the system-scope `dracon-sync-operator.service` — the script
# was a backstop for nothing: `is-active` reported "inactive" for a unit
# that can never run, the timer was left disabled, and the daemon that
# actually runs had no user-scope watcher at all.
#
# It now follows the unit that actually runs the daemon:
#
#   * Where the USER-scope daemon is the live one, the behaviour is
#     UNCHANGED — it is restarted.
#   * Where the user-scope unit is masked and the SYSTEM-scope daemon is
#     live, the system unit is checked. A user service CANNOT restart a
#     system unit — `systemctl start` returns "Access denied" and there is
#     no polkit rule granting it — so this watchdog cannot heal that
#     daemon. The system-scope `dracon-sync-operator-watchdog.timer` runs
#     as root and performs the restart. This one REPORTS it instead,
#     rate-limited, so a daemon that stays down is visible in the
#     operator's session instead of silently unbacked.
#
# Nothing is unmasked and no privilege is granted: the mask on
# `dracon-sync.service` stays, so there can never be a second daemon
# competing with the live one for the same repositories.

set -u

HOLD="$HOME/.dracon/dracon-sync.maintenance-hold"
USER_UNIT="dracon-sync.service"
SYSTEM_UNIT="dracon-sync-operator.service"
ALERT_COOLDOWN_SECS=600
NOTIFY="/etc/profiles/per-user/dracon/bin/notify-send"

if [ -f "$HOLD" ]; then
    echo "dracon-sync-watchdog: maintenance hold present ($HOLD) — skipping restart"
    exit 0
fi

# Is the user-scope unit the live daemon, or is it masked on a host where
# the system-scope unit runs the daemon?
if systemctl --user is-enabled --quiet "$USER_UNIT" 2>/dev/null; then
    if systemctl --user is-active --quiet "$USER_UNIT"; then
        exit 0
    fi
    echo "dracon-sync-watchdog: $USER_UNIT is inactive — restarting"
    systemctl --user start "$USER_UNIT"
    exit $?
fi

# The user-scope unit is masked or absent. If there is no system-scope
# daemon either, there is no live daemon to watch.
if ! systemctl list-unit-files "$SYSTEM_UNIT" --no-legend 2>/dev/null | grep -q .; then
    echo "dracon-sync-watchdog: no live sync daemon unit ($USER_UNIT masked, $SYSTEM_UNIT absent) — nothing to check"
    exit 0
fi

if systemctl is-active --quiet "$SYSTEM_UNIT"; then
    exit 0
fi

# The live daemon is down and this user-scope watchdog cannot restart it:
# starting a system unit needs privilege a user service does not have.
# The system-scope watchdog (root) does the restart; this one reports it,
# at most once per cooldown so a long outage does not become a popup storm.
STAMP_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
STAMP="$STAMP_DIR/dracon-sync-watchdog.last-alert"
now=$(date +%s)
last=0
if [ -f "$STAMP" ]; then
    read -r last < "$STAMP" 2>/dev/null || last=0
    case "$last" in
        '' | *[!0-9]*) last=0 ;;
    esac
fi
if [ "$((now - last))" -ge "$ALERT_COOLDOWN_SECS" ]; then
    if [ -d "$STAMP_DIR" ]; then
        echo "$now" > "$STAMP" 2>/dev/null || true
    fi
    echo "dracon-sync-watchdog: $SYSTEM_UNIT is inactive — reporting (this user-scope watchdog cannot restart a system unit; the system-scope watchdog does that)"
    if [ -x "$NOTIFY" ]; then
        "$NOTIFY" -u critical -t "Dracon sync watchdog" \
            "The sync daemon ($SYSTEM_UNIT) is not running. The system-scope watchdog restarts it; this session cannot." \
            >/dev/null 2>&1 || true
    fi
fi
exit 0
