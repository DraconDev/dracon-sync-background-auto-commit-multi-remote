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
- Releases its pause after three checks below the existing sync footprint floor,
  even when unrelated host PSI remains high. Existing healthy-host exit rules
  remain available as well.
- Preserves independent/manual markers, including the alternate freeze path,
  and retains ownership when `resume` fails so it can retry.

The operator Nix module raises `MemoryHigh` to 1280 MiB to accommodate the
observed working set and allow shutdown. `MemoryMax=1400M` and
`MemorySwapMax=128M` remain enforced. This is a measured headroom adjustment,
not an unlimited cgroup. The live Nix source is
`~/nixos/modules/dracon-sync-operator.nix`; applying it requires root.

## Installation and live verification

The guard source and helper are `scripts/dracon-memory-guard.sh` and
`scripts/recover-sync-memory.py`. They are installed atomically at the existing
user timer's script location, `~/.dracon/scripts/`. Backups and a digest receipt
for the initial installation are under
`~/.local/state/dracon-sync/deployments/memory-guard-20261116T030736Z/`.

A process already trapped in reclaim needs privileged temporary headroom:

```sh
sudo systemctl set-property --runtime dracon-sync-operator.service MemoryHigh=1280M
```

The pending graceful SIGTERM can then complete; inspect the same service's
MainPID and journal rather than repeatedly signalling it. Verify that the new
PID's memory falls, the guard removes its own pause, and actual new commit
messages appear in the system journal. A cleared marker alone does not prove
that commits resumed.

Apply the updated Nix module for persistence. After that, remove only the
temporary `MemoryHigh` control override created by `set-property` and reload
systemd; retain unrelated control properties. Verify the final effective
`MemoryHigh=1342177280` and `MemoryMax=1468006400` with `systemctl show`.
Do not restore 1024 MiB while the old ~1130 MiB process is still trapped.

## Validation

Regression coverage includes newline-free stderr, progress after truncation and
carriage returns, linked-worktree exclusion in both directions, unresolved
repository identities, failed sandbox reads followed by successful retry, and
incomplete sandbox output. `python3 scripts/test_memory_guard.py -v` tests
pressure recovery with fixture procfs/cgroups and mocked control commands, plus
the recovery helper's PID identity and signal constraints.

Live recovery remains unproven until a replacement PID and fresh automatic
commits are observed. The root-only adjustment above is necessary for the
currently throttled process; user-owned script installation cannot apply it.
