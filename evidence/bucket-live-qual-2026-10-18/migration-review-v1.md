## Review
Source-only review; no commands executed or changes made. Inspected requested frozen artifacts and frozen `migrate.py`.

- **Correct:** Frozen scheduler explicitly uses `User=dracon`, `Group=users`, `PrivateUsers=no`, retaining confinement and both drop-ins. Script checks runtime UID and executable hash (`dracon-sync/scripts/migrate-sync-system-service.py:130–135`).

- **Finding P1 — rollback can start both schedulers.** Rollback ignores failure stopping the system scheduler, removes its files, then starts the user scheduler (`:148–158`). A nonzero stop result does not establish that the system daemon exited. It also stops only the system watchdog **timer**, not its potentially running service.
  **Smallest fix:** Re-establish maintenance hold; stop timer and watchdog service; require confirmed system scheduler shutdown before unmasking/restarting the user scheduler. If shutdown cannot be established, retain hold and report explicit recovery instructions rather than starting another scheduler.

- **Finding P1 — rollback can report recovery while scheduling remains frozen.** Rollback ignores `resume` failure and removes maintenance hold after checking only service activity (`:160–162`). Resume can fail removing a marker (`dracon-sync/src/main.rs:1049–1052`); an active daemon is not proof scheduling resumed.
  **Smallest fix:** Check resume success and marker clearance before removing hold; retain hold and report degraded rollback if either fails.

- **Coverage:** Existing simulation always succeeds for `user(...)`, including pause/resume, and does not exercise failed rollback shutdown (`test_migrate_sync_system_service.py:62–79`). Add stop-failure, in-flight watchdog, and resume-failure cases before re-freezing the script and manifest.

- **Merge verdict: BLOCK** pending these bounded rollback fixes. Do not request privileged execution of the current frozen script.

BLOCKERS