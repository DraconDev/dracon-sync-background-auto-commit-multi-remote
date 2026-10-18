# Administrator action required — 2026-10-18 (v3, NixOS runtime path)

v2 failed live on this host: `/etc/systemd/system` is an immutable symlink into
`/nix/store`, so the install step returned EROFS (read-only file system). The
script's automatic rollback restored the original user scheduler (verified
active+enabled, no hold/freeze, no system units installed). A root-owned inert
`handover-backup` directory remains inside the v2 plan dir; it is not used
again. DO NOT EXECUTE v2.

Focused independent v3 review cleared the candidate for operator execution
(migration-review-v3.md). This is NOT live deployment or qualification evidence.

Run in the operator's own terminal, entering the password only into sudo's local
prompt (never into chat):

```sh
sudo /nix/store/vm6nxpp97fxgczw00bwkxdqdm2an3n95-python3-3.13.12/bin/python3.13 -I /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v3/migrate.py apply /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v3
```

Use v3 only. Frozen script SHA256: f40b43d5595d459fbafb950e1d9b3c514ed544215162b8a7c66f63eea8188cda. Frozen plan SHA256: 13e9b172068a7f7066f09c350bb8a97830d3365be5e0477c44c0571049f5e928. Apply revalidates payload, live user-unit/drop-in and installed-binary hashes before the handover and refuses drift. Python -I avoids user Python environment injection; no optimization flags (validation assertions remain enabled).

v3 installs system-manager units to root-writable runtime `/run/systemd/system`
(no `/etc` writes on any path). Successful handover writes
`migration-receipt.json` in the frozen v3 directory (with
`runtimeInstallEphemeral:true`), masks the old user scheduler, enables
system-manager dracon-sync-operator.service as UID1000 and its fixed-action
watchdog timer, and verifies hold/freeze clearance. Incomplete rollback
deliberately retains the maintenance hold and refuses to start another
scheduler if system shutdown cannot be proved. Do not unmask/start a second
scheduler on failure; inspect the recorded state and handover-backup first.

REBOOT WARNING: `/run` units vanish on reboot while the user-unit mask
persists, so after any reboot NO scheduler runs until re-apply or a
declarative NixOS `systemd.services` module lands (tracked follow-up). Do not
reboot mid-qualification; if a reboot happens, report it before doing anything
else. The idle-no-scheduler state is fail-safe (no duplicate scheduler, no
root runaway), but sync will be stopped.

After administrator execution, return the result to this session. Independently
recheck actual manager/process identity, namespace ownership and
scheduler/watchdog states, then continue scheduled upload, independent
consumer bytes/digest verification and fresh restoration. Migration alone does
not satisfy the goal.
