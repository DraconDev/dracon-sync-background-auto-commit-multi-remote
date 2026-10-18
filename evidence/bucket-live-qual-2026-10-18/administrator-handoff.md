# Administrator action required — 2026-10-18 (v5: diagnose first, then retry)

v3 failed live at `systemctl enable --now dracon-sync-operator.service` (exit 1,
no captured output). Rollback restored the original user scheduler (verified
active+enabled, no hold/freeze, no leftover system units). The unit file plus
drop-ins verify clean (exit 0), so the cause is unknown: either the start
itself (e.g. the root-provenance ExecStartPre check) or an enable quirk.

v5 (reviewed CLEAR FOR OPERATOR EXECUTION, migration-review-v5.md) changes
nothing about handover/rollback/identity/confinement. It only (1) verifies the
exact installed composition (unit + drop-ins) before stopping anything, and
(2) captures systemctl's own stdout/stderr to `handover-failure.log` plus the
terminal if the start fails again. DO NOT EXECUTE v2/v3/v4.

## Step 1 — transient provenance probe (safe: installs nothing, stops nothing)

Run in the operator's own terminal (password only at sudo's prompt, never chat):

```sh
sudo systemd-run --unit=bridge-provenance-probe --wait --pipe --collect -p User=dracon -p Group=users -p PrivateUsers=no -p ProtectHome=read-only -p ProtectSystem=strict -p 'ReadWritePaths=/home/dracon/.dracon /home/dracon/dracon/bridge-fixture' /etc/profiles/per-user/dracon/bin/node -e "const s=require('node:fs');console.log(JSON.stringify({euid:process.geteuid(),root:s.statSync('/').uid,home:s.statSync('/home').uid}));if(process.geteuid()!==1000||s.statSync('/').uid!==0||s.statSync('/home').uid!==0)process.exit(78)"
echo probe-exit=$?
```

This runs the same ownership check the service would run, as a throwaway
system unit. Paste back the JSON line and probe-exit.

## Step 2 — migration retry ONLY if probe-exit=0

```sh
sudo /nix/store/vm6nxpp97fxgczw00bwkxdqdm2an3n95-python3-3.13.12/bin/python3.13 -I /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v5/migrate.py apply /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v5
```

Use v5 only. Frozen script SHA256: c569aa70f142583dae0f0907d6d3b2b9407cbb46b7d5ebc99fe55b6b4215863c. Frozen plan SHA256: 36ab08eeb463881ef31602c7a7f5f67b87e2219f4b01c05084c3911d08abfc31. If the start fails again, the real error is now captured — paste the FAILED STEP block plus the result.

If probe-exit is nonzero, STOP and report: the system manager also hides root
ownership, and retrying cannot succeed — the strategy must change.

REBOOT WARNING (unchanged): `/run` units vanish on reboot while the user-unit
mask persists, so after any reboot NO scheduler runs until re-apply or a
declarative NixOS `systemd.services` module lands. Do not reboot
mid-qualification.

After administrator execution, return the result to this session for
independent re-verification before live upload/consumer/restore qualification.
Migration alone does not satisfy the goal.
