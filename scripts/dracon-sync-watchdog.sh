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

set -u

HOLD="$HOME/.dracon/dracon-sync.maintenance-hold"

if [ -f "$HOLD" ]; then
    echo "dracon-sync-watchdog: maintenance hold present ($HOLD) — skipping restart"
    exit 0
fi

if systemctl --user is-active --quiet dracon-sync.service; then
    exit 0
fi

echo "dracon-sync-watchdog: dracon-sync.service is inactive — restarting"
systemctl --user start dracon-sync.service
