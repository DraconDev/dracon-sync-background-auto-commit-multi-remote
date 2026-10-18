# Administrator handoff — v7 post-start diagnostics

## What actually happened in v6

Operator reported `Migration refused/failed: AssertionError` with no message.
Parent inspected system journal: new system service STARTED at 21:36:28 BST,
printed daemon-start and startup-cleanup, then was stopped by the migration
rollback that same second. User service restarted at 21:36:30 BST. This was
NOT another enable/start failure. See migration-v6-system-journal.log.

Live verification after failure: old user service active+enabled, MainPID3145249;
new service inactive/MainPID0, no remaining /run units, neither freeze marker
nor maintenance hold present. Installed daemon, live source unit/drop-ins,
and frozen v6 payload hashes match. Backup directory exists, proving the
attempt passed preflight. Exact failing post-start assertion cannot be
recovered from historical output. Do NOT claim a particular check failed.

## v7 change

Replaced four bare post-start assertions with named RuntimeErrors and a
public-metadata-only observation record: active state, MainPID, four UIDs,
executable SHA256 and old-user-unit enable state. Same acceptance criteria,
no sandbox/ownership relaxation or rollback changes. Report persists in
`post-start-checks.json` before evaluating those checks. No credentials,
environments, raw journal bodies or asset contents included. 16 offline
tests pass (migration-offline-tests-v7.log), including digest/mask error
messages and retained observations. Simulated receipts in test log are NOT
live receipts. Parent-inspected change; previous independent reviews cover
unchanged confinement and rollback, not a new v7 execution result.

## Next action

One diagnostic migration attempt using v7, not any older plan. It will either
complete all checks or report exactly which observed value failed and roll
back. This is not presented as a proven fix for the unidentified assertion.

```sh
d="$HOME/.local/state/dracon-sync/service-migration/20261018-operator-v7"
sudo /nix/store/vm6nxpp97fxgczw00bwkxdqdm2an3n95-python3-3.13.12/bin/python3.13 -I "$d/migrate.py" apply "$d"
```

Enter password only locally and return the `Post-start checks` JSON and any
error line. Agent can also inspect the persisted report afterward.

Script SHA256: ec85e9178ae965a3e612c3319935ca0e2cf804c8342560d643207ca0a8f4e0a7
Plan SHA256: 94a954a8b0b124615a91de28cef4b39cbee7e88a787d9315b041fcb877423f65

Runtime-install REBOOT LIMITATION still applies: /run units vanish but home
mask persists. Restore scheduling or configure NixOS persistence before any
reboot. Live scheduled upload/independent consumer/fresh restoration remain
unqualified. Do not start a second scheduler after incomplete rollback.
