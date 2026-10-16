#!/usr/bin/env bash
# Fleet observation capture for the UI/lifecycle 2026-10-16 goal.
# Five samples 10 minutes apart keep the active state and retry behavior
# observable without overwhelming the daemon.
set -euo pipefail
out="$PWD/dracon-sync/evidence/ui-delay-2026-10-16/observation"
mkdir -p "$out"
sample="$out/sample-$(date -u +%H%M).json"
pid=$(systemctl --user show dracon-sync.service -p MainPID --value)
{
    echo "captured=$(date -u +%FT%TZ)"
    echo "main_pid=$pid"
    sha256sum "/proc/$pid/exe" /home/dracon/.local/bin/dracon-sync /home/dracon/.local/bin/dracon-warden
} > "$out/sample-$(date -u +%H%M)-identity.txt"
timeout 90 ~/.local/bin/dracon-sync repos --json > "$sample"
~/.local/bin/dracon-sync health > "$out/sample-$(date -u +%H%M)-health.txt"
timeout 20 journalctl --user -u dracon-sync.service --since '-3m' --no-pager \
    | tail -120 > "$out/sample-$(date -u +%H%M)-journal.txt"
