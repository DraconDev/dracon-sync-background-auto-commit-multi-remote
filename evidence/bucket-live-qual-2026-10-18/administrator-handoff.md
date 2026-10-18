# Administrator action — v6, after successful ownership probe

Operator's actual system-manager probe returned euid1000/root0/home0 and
status0/SUCCESS. No need to repeat that probe. This is not yet proof of daemon
startup or live bucket qualification.

A concrete script bug was independently reproduced with real systemctl and
a disposable sysroot: plain enable creates wants links under /etc even when
the unit is installed under /run. NixOS /etc is immutable. v6 fixes all four
system-manager enable/rollback-disable commands to use --runtime. All
user-manager operations, ownership checks, confinement and rollback ordering
are unchanged. Historical v3 stderr was not captured, so it is a likely,
not conclusively proven, explanation of that failure.

14 tests pass, including a real offline sysroot enable/disable regression and
a fake-manager runtime-flag tripwire. Independent v6 source review cleared
operator execution (migration-review-v6.md). Installed user daemon remains
active. Do not execute obsolete v2–v5 artifacts.

Paste this block into your own terminal. Enter the password only at sudo's
local prompt, then paste back the result:

```sh
d="$HOME/.local/state/dracon-sync/service-migration/20261018-operator-v6"
sudo /nix/store/vm6nxpp97fxgczw00bwkxdqdm2an3n95-python3-3.13.12/bin/python3.13 -I "$d/migrate.py" apply "$d"
```

Frozen script SHA256: fc8369c5e4e4c906e6cf57e3dad15a5d1ed06ea1dbc70d46425e177f93513b8b
Plan SHA256: 3422fa4d50dd8b545fd7abc971157a026f0d374958a07643bd10e35ef90c02fd
Apply checks payload, live source unit/drop-ins and pinned daemon before
handover. Startup failure now prints/captures systemctl's stderr before
rollback. Retained hold or incomplete rollback means stop and report; never
start a second scheduler.

REBOOT LIMITATION: this is a runtime qualification install. /run units vanish
on reboot while the old user scheduler mask persists. Restore scheduling or
install declarative NixOS persistence BEFORE any reboot. Migration is not
qualified for unattended reboot persistence.

After execution, parent must inspect actual migration receipt, runtime UID,
binary hash, namespace root provenance, old/new scheduler and watchdog states,
and freeze/hold clearance. Then complete actual scheduled upload, independent
scoped consumer bytes/digest verification and fresh restoration with timestamped
committed evidence. Migration alone does not satisfy the goal.
