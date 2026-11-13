# Push unit isolation — a hook must not be able to kill the daemon (2026-11-13)

Related: `push-isolation-2026-10-15.md` (scoped concurrency + nice; this
document supersedes its "no memory caps / no external wrapper" boundary),
`spool-reclaim-throughput-2026-11-12.md`.

## Incident

From 00:28 to past 03:50 on 2026-11-13 `dracon-sync-operator.service` was
OOM-killed by the kernel every ~4 minutes: 43 kills (`Failed with result
'oom-kill'`), restart counter 26 → 31 and climbing. `dracon-sync health` was
green throughout — every restart produced a fresh, healthy-looking daemon.

- 00:25 `nixos-rebuild` deployed the 2026-11-11 memory-incident hardening:
  daemon unit `MemoryMax 2G → 1400M`, `MemorySwapMax 128M`.
- browser-extensions-shared had 186 unpublished commits. Its pre-push gate
  (`scripts/git-hooks/run-push-gates.mjs`, default 8 concurrent workspaces)
  runs as a child of `git push`, which ran as a child of the daemon — in the
  daemon's cgroup. Its own comments size it against a "2G cgroup".
- Every kill list contained the gate's `npm run typecheck` processes (43/43).
  The daemon itself held ~100 MB.
- Each restart resumed the same push 30 s later → same kill. The repository
  never published (last at 00:18) and every other repository's in-flight work
  died with each kill (dracon-platform showed ❔ UNKNOWN; its pushes still
  landed between kills).
- Measured afterwards under the mirrored sandbox: that gate holds ~2G steady
  and peaks at **4G**. The old 2G budget only held by luck of timing.

A second, independent defect made it worse: a hand-written runtime drop-in
from 2026-10-19 (`/run/systemd/system/dracon-sync-operator.service.d/10-resource-headroom.conf`,
`MemoryHigh=1536M`) overrides the NixOS module (runtime drop-ins outrank
`/etc`) and sat ABOVE the new `MemoryMax=1400M`. The soft limit therefore
never engaged; every overrun went straight to a hard kill.

## Why the 2026-10-15 design allowed this

`push-isolation-2026-10-15.md` bounded push concurrency and set nice 19, and
explicitly stated "not a memory cap or a promise that a single hook cannot
itself allocate excessive memory". That was safe while the daemon unit had
generous limits. Once the host capped the unit, the hook's memory became the
daemon's fate. Repository-supplied code must not share the daemon's cgroup.

## Design

`src/git/push_unit.rs`. Every ENROLLED daemon push (the `push_phase` /
`background_pushes` scopes — the same set that got nice 19) is spawned as

```
systemd-run --user --wait --pipe --collect --service-type=exec
  --expand-environment=no --unit=dracon-push-<96-bit random>.service
  --slice=dracon-sync-push.slice
  -p KillMode=control-group -p SendSIGKILL=yes -p TimeoutStopSec=25s
  -p RuntimeMaxSec=<4×timeout + grace + 60s> -p Nice=19
  -p <daemon sandbox, mirrored> --working-directory=<repo>
  --setenv=KEY… -- <absolute git> push …
```

- **Only a service can leave the daemon's cgroup.** The daemon is a SYSTEM
  service; the user manager cannot migrate one of its processes into a scope
  (cgroup migration needs write access at the common ancestor). A transient
  service is forked by the user manager itself — the pattern the epoch
  producer already uses in production.
- **No limits in the daemon.** The budget is the slice's, owned by the host
  (`systemd.user.slices.dracon-sync-push` in `scripts/dracon-sync-operator.nix`:
  MemoryHigh 4G, MemoryMax 6G, MemorySwapMax 512M, CPUWeight 20, CPUQuota
  800%, IOWeight 10, ManagedOOMMemoryPressure=kill). Without that config the
  slice is unlimited — the pre-2026-11-11 behavior minus the shared fate.
- **Same sandbox.** The daemon reads its own unit (from `/proc/self/cgroup`)
  back with `systemctl show -p ProtectSystem -p ProtectHome -p ReadWritePaths
  -p ReadOnlyPaths -p InaccessiblePaths -p PrivateTmp -p PrivateDevices -p
  NoNewPrivileges -p LockPersonality -p RestrictRealtime -p RestrictSUIDSGID`
  and applies the restricting values to every push unit (paths get the `-`
  ignore-if-missing prefix). Leaving the cgroup does not mean leaving the
  sandbox, and reading live values means module and push units cannot drift.
  Verified live: ssh `ls-remote` works, `$HOME` is read-only, the state dir
  is writable, and the real gate runs under it.
- **Environment.** The child's effective environment (daemon env + the
  command's set/remove edits, minus the daemon unit's `NOTIFY_SOCKET`,
  `INVOCATION_ID`, `LISTEN_*`, …) is passed with `--setenv=KEY` only, so
  values (tokens) never appear in any argv. A key that cannot be passed
  losslessly disables isolation for that push rather than being dropped.
- **Askpass.** The HTTPS fallback's askpass script lived in `std::env::temp_dir()`
  — the daemon's PrivateTmp, invisible to the unit (which has its own). It now
  lives in `~/.local/state/dracon/sync/askpass/` (0700, must be a real
  directory owned by us; temp dir otherwise), writable in both sandboxes.
- **Stopping.** Measured on systemd 258: killing the `systemd-run --wait`
  client (TERM or KILL) leaves its unit running. So the runner binds client
  PID → unit name. A timeout (`kill_process_group`) runs `systemctl --user
  stop` — systemd applies TERM → `TimeoutStopSec` (= `SIGTERM_GRACE`, 25 s) →
  KILL to the whole unit — then kills the client. A cancellation
  (`GroupKillGuard::drop`, cannot await) SIGKILLs the unit from a detached
  thread. The binding is dropped when the client is reaped (PID reuse).
- **Daemon death.** `RuntimeMaxSec` bounds a push whose daemon died. At daemon
  start (not `run_once`, which may run beside a live daemon) every surviving
  `dracon-push-*.service` is stopped before the first push. Test builds use
  the `dracon-push-test-` prefix and the two sweeps never claim each other's.
- **Budget kill classification.** `systemd-run --wait` reports `Finished with
  result: oom-kill`. `is_push_unit_resource_kill` makes that a LOCAL hook
  failure (checked before the success-banner rule: the gate may have passed
  and pack-objects been killed), so it fails fast instead of re-running the
  hook through retries, pull and HTTPS fallback, and the classifier names
  the slice.
- **Fallback.** No `/run/user/<uid>/bus`, no `systemd-run`/`systemctl` on PATH,
  a fenced command (its fd cannot cross the manager), or
  `DRACON_SYNC_PUSH_ISOLATION=0|off|false|no|direct` → spawn directly as
  before, nice 19 via `pre_exec`. Logged once.

## Observability

`health` gains a **Containment** row / JSON fields:

- daemon starts in the last hour from `~/.local/state/dracon/dracon-sync-daemon-starts`
  (written at daemon start; ≥4 is a loop → unhealthy; the daemon also logs 🚨);
- an effective `MemoryHigh ≥ MemoryMax` on the daemon unit → unhealthy, with
  the runtime drop-ins named;
- push isolation unavailable → warning.

Live on 2026-11-13 the row immediately reported the inverted limits and named
the `/run` drop-ins.

## Verification

- `push_unit_tests.rs`: argv shape (slice, properties, sandbox, no values in
  argv, no memory properties), effective-environment edits and manager-state
  stripping, orphan prefix ownership, kill switch, sandbox mirroring from the
  verbatim live `systemctl show`, own-unit detection.
- `slow_push_hook_leaves_capacity_for_inspection_and_commit_and_enforces_rejection`
  now asserts the real hook and its helper run in
  `/dracon-sync-push.slice/dracon-push-test-*` and the test process does not.
  Non-vacuity: with `DRACON_SYNC_PUSH_ISOLATION=0` it fails ("hook process …
  is not isolated").
- Existing real-hook contracts pass through real units: cancellation kills the
  hook and its TERM-resistant helper within 5 s; a 2 s-deadline publication
  settles within 30 s; hooks are honored on every daemon push path; HTTPS
  askpass tests.
- `isolated_push_oom_kill_is_a_local_fail_fast_resource_failure` uses the
  verbatim `systemd-run` OOM tail captured on this host.
- NixOS: the slice renders and `nix build --dry-run` of the host toplevel
  needs only the slice, user-units and etc derivations.

## Operator steps (host side)

1. `sudo rm /run/systemd/system/dracon-sync-operator.service.d/{10-resource-headroom,20-bridge-fixture,30-tasksmax}.conf`
   (all three are folded into the module) and `sudo systemctl daemon-reload`.
2. `sudo nixos-rebuild switch` (installs `dracon-sync-push.slice`).
3. Install the released daemon and restart it; confirm `dracon-sync health`
   shows Containment ✅ and, during a push,
   `systemctl --user list-units 'dracon-push-*'`.

## Boundaries / follow-ups

- Non-push daemon children (status, rev-list, bucket guard node) remain in the
  daemon's cgroup and its 1400M cap. Observed peaks are ~250 MB each.
- A restart-loop BREAKER (persist "push in flight" and back off a repository
  whose push was in flight across N consecutive daemon deaths) would contain
  future unknown causes; with hooks out of the cgroup the observed cause is
  gone, and the Containment row now makes any loop visible within minutes.
