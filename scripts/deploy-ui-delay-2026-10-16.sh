#!/usr/bin/env bash
# Run from workspace root via: dracon-sync maintenance -- bash <this-script>
# Fixed artifact digests pin this qualified build; no rebuilding inside maintenance.
set -euo pipefail
out="$PWD/dracon-sync/evidence/ui-delay-2026-10-16"
[[ -d "$out" && -f target/release/dracon-sync && -f target/release/dracon-warden ]]
[[ $(sha256sum target/release/dracon-sync | cut -d' ' -f1) == 8bbc559eb45378c98fa19918d9286b52dc67c138c49c25a255de9400e5949ac5 ]]
[[ $(sha256sum target/release/dracon-warden | cut -d' ' -f1) == d342d4a1b188dd76769b17b60006af53fdd042f38c7aecbf39d5ff0cc1d13574 ]]
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
cmp -- target/release/dracon-sync "/proc/$pid/exe"
printf 'running_pid=%s\nfinished=%s\n' "$pid" "$(date -u +%FT%TZ)"
sha256sum target/release/dracon-sync target/release/dracon-warden \
    "$HOME/.local/bin/dracon-sync" "$HOME/.local/bin/dracon-warden" "/proc/$pid/exe"
systemctl --user show dracon-sync.service -p ActiveState -p MainPID -p ExecMainStartTimestamp -p KillMode
printf 'Warden is a Git filter, not an installed daemon unit; new invocations use the atomically installed binary.\n'
