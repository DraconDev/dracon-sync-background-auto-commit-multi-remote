#!/usr/bin/env bash
# Run from workspace root via: dracon-sync maintenance -- bash <this-script>
# Fixed artifact digests pin this qualified build; no rebuilding inside maintenance.
set -euo pipefail
out="$PWD/dracon-sync/evidence/ui-delay-2026-10-16"
expected_sync=8f7f45c9a82b41dfb5bf762fa9d3347837a8ffe534eee6c8115c5242e8db2030
expected_warden=d342d4a1b188dd76769b17b60006af53fdd042f38c7aecbf39d5ff0cc1d13574
[[ -d "$out" && -f target/release/dracon-sync && -f target/release/dracon-warden ]]
[[ $(sha256sum target/release/dracon-sync | cut -d' ' -f1) == $expected_sync ]] || { echo 'sync digest mismatch'; exit 2; }
[[ $(sha256sum target/release/dracon-warden | cut -d' ' -f1) == $expected_warden ]] || { echo 'warden digest mismatch'; exit 2; }
backup="$HOME/.dracon/backups/ui-delay-20261016-$(date -u +%H%M%S)"
mkdir -m 700 "$backup"
printf 'started=%s\nbackup=%s\n' "$(date -u +%FT%TZ)" "$backup"
for name in dracon-sync dracon-warden; do
    destination="$HOME/.local/bin/$name"
    [[ -f "$destination" && ! -L "$destination" ]]
    cp -p -- "$destination" "$backup/$name"
    sha256sum "$destination" "$backup/$name"
    install -m 755 "target/release/$name" "$HOME/.local/bin/.$name.ui-delay-new"
    mv -f -- "$HOME/.local/bin/.$name.ui-delay-new" "$destination"
    cmp -- "target/release/$name" "$destination"
done
# Normal restart: no manual stop, hold-marker changes or hook overrides.
timeout 120 systemctl --user restart dracon-sync.service
timeout 15 systemctl --user is-active dracon-sync.service dracon-system-guard.service
pid=$(systemctl --user show dracon-sync.service -p MainPID --value)
[[ "$pid" -gt 0 ]]
# MainPID may be published during systemd's exec transition. Require the
# tested identity to settle within ten seconds, never assume Active=ready.
matched=false
for attempt in {1..100}; do
    if cmp -s -- target/release/dracon-sync "/proc/$pid/exe"; then matched=true; break; fi
    sleep 0.1
done
[[ "$matched" == true ]]
printf 'running_pid=%s\nfinished=%s\n' "$pid" "$(date -u +%FT%TZ)"
sha256sum target/release/dracon-sync target/release/dracon-warden \
    "$HOME/.local/bin/dracon-sync" "$HOME/.local/bin/dracon-warden" "/proc/$pid/exe"
systemctl --user show dracon-sync.service -p ActiveState -p MainPID -p ExecMainStartTimestamp -p KillMode
printf 'Warden is a Git filter, not an installed daemon unit; new invocations use the atomically installed binary.\n'
