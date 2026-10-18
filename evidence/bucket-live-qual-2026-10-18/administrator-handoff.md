# Administrator action required — 2026-10-18

Focused independent v2 review cleared the candidate for operator execution (migration-review-v2.md). This is NOT live deployment or qualification evidence.

At the latest prerequisite check, the existing user daemon was active and `sudo -n true` returned password required. No privileged migration has executed. Tasks 2 and 3 remain unfinished.

Run in the operator's own terminal, entering the password only into sudo's local prompt (never into chat):

```sh
sudo /nix/store/vm6nxpp97fxgczw00bwkxdqdm2an3n95-python3-3.13.12/bin/python3.13 -I /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v2/migrate.py apply /home/dracon/.local/state/dracon-sync/service-migration/20261018-operator-v2
```

Use v2 only. Frozen script SHA256: 0425c65bc40de215aab5545f44c5be2e115d14efb07aaf8bb64f70a633f3aaa6. Frozen plan SHA256: ff48ede3a6fe1d51b27ada0b12bfbf829fe1c5430e00cc34b1da55a7711c06aa. Apply checks payload, live user-unit/drop-in and installed-binary hashes before the handover and refuses drift. Python -I avoids user Python environment injection; no optimization flags (validation assertions remain enabled).

Successful handover writes `migration-receipt.json` in the frozen v2 directory, masks the old user scheduler, enables system-manager dracon-sync-operator.service as UID1000 and its fixed-action watchdog timer, and verifies hold/freeze clearance. Incomplete rollback deliberately retains the maintenance hold and refuses to start another scheduler if system shutdown cannot be proved. Do not unmask/start a second scheduler on failure; inspect the recorded state and handover-backup first.

After administrator execution, return the result to this session. Independently recheck actual manager/process identity, namespace ownership and scheduler/watchdog states, then continue scheduled upload, independent consumer bytes/digest verification and fresh restoration. Migration alone does not satisfy the goal.
