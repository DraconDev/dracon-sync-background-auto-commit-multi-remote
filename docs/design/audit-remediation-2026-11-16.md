# Audit remediation and memory-pressure pause recovery, 2026-11-16

The audit found three process and repository coordination defects. The operator
also supplied a `repos` report showing every dirty repository paused for 26
minutes by `~/.dracon/scripts/dracon-memory-guard.sh`.

## Changes

Stderr is drained in 8 KiB chunks with a 1 MiB retained diagnostic prefix.
Progress recognition uses an independent 8 KiB frame, discards oversized frames
until the next newline/carriage return, and continues after diagnostic truncation.
A watch channel coalesces progress so chatty hooks cannot build a notification
queue in the daemon. The existing hard operation deadline remains in force.

The push/maintenance registry resolves the repository's canonical common Git
directory through the same identity helper used by operation/publication leases.
Linked worktrees now share exclusion. An unresolved identity refuses a push or
defers maintenance instead of inventing an independent registry entry.

Sandbox discovery caches only successful, complete policies. Failed discovery
retains the direct runner inside the existing daemon sandbox; the next command
retries discovery. Missing properties in otherwise successful `systemctl show`
output are also refused. Resource isolation may temporarily be unavailable on
this fallback, but a query failure never authorizes an unconfined transient unit.

The full test run also exposed a deadline-ordering race in the history probe.
An already-expired budget could still accept a child that exited before its
first poll. The existing deadline regression now checks the deadline at entry,
before spawn and before observing a child result, keeping expired probes
unmeasured rather than presenting a late result as timely evidence.

## Why commits remained paused

At 02:57 UTC the live system daemon (PID 3522953) held approximately 1.1 GiB of
anonymous memory despite being paused. Its effective `MemoryHigh` was 1 GiB and
`MemoryMax` was 1400 MiB. Both executor threads were subsequently observed in
`mem_cgroup_handle_over_high`; cgroup full PSI reached approximately 68%, with
over 51 million `memory.events high` events and no OOM kill. The process could
not finish a verified, pidfd-delivered SIGTERM. Removing a freeze marker alone
does not release this memory or guarantee that a throttled scheduler can run.

The old guard waited for host PSI to fall before releasing its pause. Other
tenants can keep that signal high even after sync's own memory has fallen.
It also did not relinquish stale marker ownership on its pressure branch, and
forgot ownership after a failed `resume`, preventing retry.

The shipped guard now:

- Revalidates marker inode/mtime on every check, including under pressure.
- Requests a verified graceful restart after an owned pause lasts two minutes.
  A successful request starts a 30-minute cooldown; it is not a claim that the
  old process has exited. It never sends KILL, stops/disables the service or
  changes memory limits.
- Requires `Restart=always`, a matching installed executable, matching UID and
  the `daemon` subcommand before sending SIGTERM through a pinned pidfd.
- If the pinned daemon has not exited after five seconds, and its cgroup is
  above `memory.high` with at least 50% local full PSI, cancels only verified
  read-only Git probes belonging to that daemon, through their own pidfds.
  The allowlist is the exact observed `diff --cached --unified=0` and remote
  reachability `rev-list --objects --no-object-names` forms. It verifies the
  Git executable, UID, PPID and same cgroup again before signalling. Fetches,
  index mutations, hooks, commits and other workloads are excluded.
- Releases its pause after three checks below the existing sync footprint floor,
  even when unrelated host PSI remains high. Existing healthy-host exit rules
  remain available as well.
- Preserves independent/manual markers, including the alternate freeze path,
  and retains ownership when `resume` fails so it can retry.

Memory limits remain `MemoryHigh=1024M`, `MemoryMax=1400M` and
`MemorySwapMax=128M`. Targeted cancellation released the stalled readers' memory
and enabled a clean shutdown within the existing limits. A temporary 1280 MiB
headroom increase was initially proposed, then withdrawn after this recovery
path was verified; no privileged limit adjustment was applied.

## Installation and live verification

The guard source and helper are `scripts/dracon-memory-guard.sh` and
`scripts/recover-sync-memory.py`. They are installed atomically at the existing
user timer's script location, `~/.dracon/scripts/`. Backups and a digest receipt
for the initial installation are under
`~/.local/state/dracon-sync/deployments/memory-guard-20261116T030736Z/`.

At 03:22 UTC two direct Git children of PID 3522953 were independently verified
as read-only probes holding approximately 151 and 169 MiB RSS. SIGTERM through
their pinned pidfds released enough headroom for the daemon's pending graceful
shutdown. The system journal records a clean exit at 03:22:42, systemd's
`Restart=always` replacement PID 929428 at 03:23:12, and the memory guard
releasing its owned pause at 03:23:44. At 03:25 the journal records fresh automatic
commits (including 37 files in dracon-platform) and successful syncs of this
project, junk-runner and capture-anime-girls.

A cleared marker alone is not a recovery witness: inspect a replacement PID,
its cgroup footprint and fresh automatic commit messages. The installed helper
now performs the same narrowly scoped cancellation if severe cgroup throttling
prevents its graceful TERM request from completing. It never sends SIGKILL or
removes repository lock files.

## Validation

Regression coverage includes newline-free stderr, progress after truncation and
carriage returns, linked-worktree exclusion in both directions, unresolved
repository identities, failed sandbox reads followed by successful retry, and
incomplete sandbox output. `python3 scripts/test_memory_guard.py -v` tests
pressure recovery with fixture procfs/cgroups and mocked control commands, plus
the recovery helper's PID identity and signal constraints. Twelve guard/helper
regressions pass, including the positive reader cancellation and negative fetch,
commit, index mutation, foreign executable, unrelated parent and other-cgroup
cases.

Live commit recovery is verified by the journal witnesses above. Deployment of
the Rust audit fixes is a separate step after final source-matching workspace
validation and release-build verification.
