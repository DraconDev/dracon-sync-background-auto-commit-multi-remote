# Push task budget and late report evidence — 2026-10-20

## Observed failures

The live system unit `dracon-sync-operator.service` reached TasksCurrent=96,
exactly TasksMax=96, while VidPro's pre-push tests ran. Node logged
`pthread_create: Resource temporarily unavailable`, followed by worker EPIPE.
Git status probes across unrelated repositories also failed to fork warden
or Git children. These are shared-cgroup admission failures, not evidence that
GitHub/GitLab remotes disappeared or that total repository size exceeded a
per-push pack limit.

VidPro had four Vitest workers; each Node process also owns V8/libuv threads.
Its test guard additionally moved the real `.wxt/` directory while other tests,
builds and pushes could use it. The claimed mutex was not implemented. On the
loaded host its real WXT regeneration hit the guard's 45-second timeout.

## Changes

- VidPro now uses one worker, retaining every test and assertion. Two workers
  were validated first, but parallel forge hooks plus probes still compete
  for the old 96-task ceiling; one worker leaves more immediate headroom.
- The real ensure-WXT guard runs in isolated temporary project fixtures with
  a local CLI dependency fixture. Tests check a valid existing config, missing
  and partial configs, correct prepare arguments, propagated preparation failure
  and successful CLI exit without output. No test moves the live `.wxt` tree.
  Real WXT still runs through the separate build/typecheck push gates.
- Packaged sync service TasksMax becomes 512. The parent Nix module and its
  service-parity check agree. CPU and memory ceilings are unchanged.
- The repo report samples scheduler evidence after expensive Git/filter probes,
  immediately before current-health classification. It retains the existing
  30-second freshness limit, rejects expired evidence, and updates status,
  activity, warning flags and counts together. Historical PUSH is independent.
  A regression covers evidence arriving during a slow scan and its later expiry.

## Validation and deployment status

VidPro: 103 files, 1,227 tests passed with the isolated fixtures and two workers.
The final single-worker setting is verified by the subsequent full push gates.
The report regression passes. The service validates under systemd-analyze;
Nix syntax and shell syntax checks pass. Further workspace/release checks and
forge verification are recorded below when complete.

The live task ceiling could not be changed from this session: noninteractive
sudo requested a password, and systemctl refused the operation because
administrator authentication was required. The concrete administrator step is:

```sh
sudo systemctl set-property dracon-sync-operator.service TasksMax=512
```

This applies immediately and persists as a systemd property override. It does
not stop the service. The prepared package/Nix defaults prevent reintroducing
the low ceiling on future installs. Until the administrator step is confirmed,
report the live ceiling as 96; do not claim it is deployed.

The full Nix flake check and generated service/watchdog parity check passed.
The new debug CLI was tested against live scheduler data: the browser repo
reports `health.current = working`, phase `working`, with a fresh observation,
while independently retaining the genuinely failed historical push result.
No stale evidence is relabeled healthy and no push gate was bypassed.

## Completed validation and October 21 follow-up

Workspace tests passed: 2,517 tests, zero failures, 25 operational ignores.
Workspace Clippy with warnings denied and the locked release build passed.
The installed CLI backup and manifest are in
`~/.local/state/dracon/deployments/20261020T212450Z-late-scheduler-evidence/`.
The running daemon was not restarted.

TabMon also retained a stale `music-2.x` fetch-tool assertion after the
fetcher moved to music-3.0. The assertion now requires an explicitly versioned
music model, including legacy plus variants; failures from the Python dry-run
are propagated, with skipping reserved for a missing Python executable.
The focused dry-run check and a complete 942-test push gate passed.

The subsequent controlled push did not fully converge: a later TabMon XP-cap
regression failed on GitLab. Maintenance automatically resumed after its
15-minute bound; the freeze marker was verified absent on October 21.
Current code passes all 13 XP tests. The daemon independently started another
normal guarded push at 01:29 BST; publication must be verified against both
forge refs before reporting recovery. Current live TasksMax remains 96.
At 01:33 BST the root filesystem had 135 GiB available (85% used); no cleanup
or deletion was needed for this follow-up.
