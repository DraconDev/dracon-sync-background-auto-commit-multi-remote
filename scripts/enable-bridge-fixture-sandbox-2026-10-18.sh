#!/usr/bin/env bash
# Run through dracon-sync maintenance. Exact fixture write grant only.
set -euo pipefail
file="$HOME/.config/systemd/user/dracon-sync.service.d/20-bridge-fixture.conf"
[[ ! -e "$file" && ! -L "$file" ]]
mkdir -p "$(dirname "$file")"
tmp=$(mktemp "$(dirname "$file")/.fixture.XXXXXX")
printf '[Service]\n# Exact watched qualification checkout; preserve all existing sandbox restrictions.\nReadWritePaths=/home/dracon/dracon/bridge-fixture\n' > "$tmp"
chmod 644 "$tmp"
mv "$tmp" "$file"
timeout 15 systemctl --user daemon-reload
if ! timeout 90 systemctl --user restart dracon-sync.service; then
    rm -- "$file"
    timeout 15 systemctl --user daemon-reload
    timeout 90 systemctl --user restart dracon-sync.service
    exit 1
fi
timeout 15 systemctl --user is-active dracon-sync.service
systemctl --user show dracon-sync.service -p ReadWritePaths -p ProtectHome -p ProtectSystem
