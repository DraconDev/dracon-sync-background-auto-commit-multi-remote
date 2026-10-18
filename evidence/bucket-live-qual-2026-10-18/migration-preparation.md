# Operator-UID system service migration preparation

User explicitly authorized this service-manager migration. No privileged deployment has been executed: `sudo -n true` returned password required. Existing user daemon remains active.

Prepared scripts: `scripts/migrate-sync-system-service.py` and `scripts/test_migrate_sync_system_service.py`. No Rust source/binary changes. The migration retains the live user unit's confinement, exact writable paths, resource-headroom drop-in and fixture exception, sets explicit User=dracon/Group=users and PrivateUsers=no, and fails startup unless the process sees root-owned `/` and `/home` and euid1000. The system watchdog has one fixed administrative action: start the operator service. The user freeze watchdog remains in place; the old restart watchdog is disabled and old scheduler is masked during handover.

## Review dispositions

Fresh review v1 found two valid P1 rollback gaps. Both accepted and patched:

1. Rollback now re-establishes the maintenance hold, synchronously stops the system watchdog timer AND service, and requires inactive/failed state plus zero MainPID/ControlPID for services before restoring the user scheduler. Nonzero stops or ambiguous metadata abort recovery without starting a duplicate scheduler; unit files and backups remain for inspection.
2. Successful migration and rollback require resume success and clearance of BOTH freeze-marker paths before removing the hold. Failed resume or lingering markers leave the hold and report incomplete rollback.

The user-side in-flight restart watchdog is now also synchronously stopped and verified before disabling the original scheduler. Failed migration with complete rollback still returns failure (never reports migration success).

## Verification so far (offline only)

`python3 -B -m unittest discover -s dracon-sync/scripts -p test_migrate_sync_system_service.py -v`: 11 passed, 0 failures. First rerun exposed a mock's general show response shadowing MainPID --value; fixed the mock dispatch and reran all tests. Tests use temporary directories and fake managers only; printed simulated receipt is NOT live migration evidence.

Tests include preserved confinement, successful ordered handover, failed-start rollback, tampered-plan refusal, failed system shutdown preventing user restart, in-flight watchdog shutdown, resume error, alternate lingering freeze marker, missing process metadata and transitional service state.

`systemd-analyze verify` on all three frozen candidate units: exit0. Existing unrelated cups.socket legacy-path warning remains in the log. No cargo gates run: this delivery slice changes only Python migration tooling and evidence, not Rust.

## Frozen artifacts

Original `.../service-migration/20261018-operator` is reviewed BLOCKED; DO NOT EXECUTE that version. Use v2 only after the focused safety review clears it:

`/home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v2`

v2 plan.json SHA256: `ff48ede3a6fe1d51b27ada0b12bfbf829fe1c5430e00cc34b1da55a7711c06aa`.

The plan includes hashes of every payload and migration script, exact live unit/drop-ins and expected installed daemon hash. Apply revalidates them, so operator drift refuses the handover. Installed daemon remains pinned to `2b74bb7c84cb9572b35b0ee246bdd29a9e9e56d622fe70fbe46779678d4dfdba`.

Administrator execution is still required after review. After actual handover, independently inspect namespace provenance, system/user scheduler/watchdog state, runtime identity/hash and freeze/hold clearance before resuming scheduled upload, independent consumer readback and fresh restoration. Future daemon service restarts must target the system-manager `dracon-sync-operator.service`, not unmask the old user scheduler. Never run both schedulers.
