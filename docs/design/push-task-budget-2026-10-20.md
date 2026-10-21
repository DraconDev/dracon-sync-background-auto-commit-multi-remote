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

## October 21 convergence follow-up

At 02:24, GitHub had advanced but GitLab still pointed at d1cce2ed0f.
The daemon repeatedly cancelled repository work after 900 seconds even
though its per-push timeout had scaled to 1800 seconds. The cancellation
occurred outside the push timeout; increasing only the Git timeout cannot
fix that watchdog mismatch. A separately launched 600-second CLI repair
was also repeating checks, so no third push was started during that run.

An automatic-resume maintenance wrapper let the existing daemon GitLab
attempt finish beyond the outer watchdog deadline. It still failed a real
gate at 02:32: TabMon's test log ended after two passing files, without a
suite summary or assertion failure. Clean Web, VidPro and YouTube Filter
passed. The live service reported pids.events max=8504 (historical aggregate)
and oom_kill=0; that does not identify the exact cause of this test exit.
Maintenance resumed normally. No gate failure was relabeled as success.

TabMon now uses one worker, retaining its complete suite and assertions.
A normal sync-now pass then committed the configuration and started from
a transient user scope with CPUQuota=200%, outside the system service's
96-task limit. Its full process lifetime is held by an automatic-resume
maintenance wrapper, avoiding competing daemon dispatch and preserving
ordinary hook, staging and push-history behavior. Final forge verification
must still be recorded after this pass exits.

## Verified recovery, 02:45–02:46 BST

GitHub main, GitLab main and local HEAD were all independently verified at
`8829dff91a26b88d658fdda20b31bbd9cfad49da`. The daemon wrote
`succeeded=true` at epoch 1792547132 (02:45:32 BST). GitLab's actual guarded
push passed all four workspace gates; TabMon passed all 947 tests with one
worker. VidPro and YouTube Filter gates also passed; the latter passed 721
tests. No history rewrite or gate bypass was used.

One worker had already been in flight when controlled maintenance began.
It delivered the same snapshot, so the controlled origin push received a
ref-update race: the remote already pointed at the desired 8829dff91a, but
the older advertisement had expected 0862adc8d9. After both remote refs
and the daemon's real success record were verified, only the redundant
controlled recovery scope was stopped. Its process/FD/scope ownership was
verified first. This avoided an unnecessary second GitLab ref-update race;
the remaining duplicate hook was not needed for publication. No push-history
record was edited manually. The maintenance wrapper resumed automatically,
the freeze marker was verified absent, and the system daemon stayed active.

Other agents continued editing after the verified snapshot; subsequent dirty
or WORKING rows are normal pending work. The live system-unit task cap is
still 96: the prepared increase to 512 requires the administrator command
above, which this session cannot authenticate.
