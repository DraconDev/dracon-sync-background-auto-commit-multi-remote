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

- VidPro now uses two workers, retaining every test and assertion.
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
