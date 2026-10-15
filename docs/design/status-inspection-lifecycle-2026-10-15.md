# Status inspection lifecycle and retry starvation — 2026-10-15

## Observed failure

Fleet status showed multiple dirty repositories stalled for hours while PUSH
still displayed the last successful result. The service was active. Logs showed
four repositories repeatedly timing out every 60s while others retained
`status-pending` holds for more than 6,000s. After a restart the same signature
returned on a different group, including small utility repos.

The earlier increase of the filter-aware classification floor did not fix this
separate status-inspection lifecycle. Nor does one green fleet snapshot prove
that either problem cannot recur.

Related incident: parent `docs/design/sync-wedge-filter-hang-2026-10-04.md`.

## Mechanism and change

The daemon called dracon-git 94.7.2 `GitService::get_status`, which delegates to
`spawn_blocking`. Aborting its async caller does not cancel the blocking closure
or its synchronous subprocesses. The scheduler released the reservation but
could immediately retry the same early repository, creating more blocking work
and denying later repositories the four global status slots. The specific
warden-side trigger for a hung filter remains unproven.

`src/git/inspection.rs` replaces only the daemon's status-inspection boundary:

- Git porcelain v2 with branch/ahead/behind and all untracked files;
- NUL-delimited parsing, including rename source records and non-UTF8 paths;
- no optional index locks, no hook bypass or filter bypass;
- existing owned process-group cancellation/capped pipe capture in `git/ops.rs`;
- bounded runtime: cancellation drops and kills Git plus filter helpers rather
  than merely abandoning an uncancellable blocking job;
- parse/exit/timeout failures are errors, never fabricated clean snapshots.

`src/daemon.rs` now applies a 300s per-repo retry cooldown on both the watchdog
abort and an inspection error. Failed early repositories cannot immediately
reclaim all four slots. The concurrency cap remains four and the status timeout
remains 60s. No policy, identity, trust list, ownership, history, or bucket
eligibility setting is changed.

## Regression evidence

- A real configured clean filter starts a sleeping helper. Inspection times out;
  the helper is confirmed terminated; removing the simulated fault lets a fresh
  inspection succeed.
- Real unborn repository inspection counts files within an untracked directory.
- Parser fixtures cover clean/detached/unborn heads, ahead/behind, combined
  changes, rename-source skipping, conflicts, non-UTF8 paths, malformed output.
- Scheduler regression models four failed early repositories and verifies the
  next four gain admission, with retries admitted exactly at cooldown expiry.

Bounded validation: `timeout 180 cargo test -p dracon-sync --locked status_`,
`timeout 180 cargo test -p dracon-sync --locked git::inspection`,
`timeout 590 cargo test --workspace --locked`, and
`timeout 180 cargo clippy --workspace --locked -- -D warnings` pass.
`timeout 400 cargo deny check` still fails on existing dependency advisories;
bans, licenses and sources pass. No advisory exceptions were added.

## Follow-up: synchronous discovery was still starving result collection

The first deployment did not resolve sustained fleet convergence. A bounded
`strace` of the daemon coordinator at 14:49 showed `openat`/`getdents64` walking
`dracon-utilities/target/debug/deps` (19 GiB locally) while its status reservations
aged. At another observation the service had no live Git children despite status
holds continuing. The trace proves coordinator-side build-cache enumeration;
it does not prove the precise cause of every earlier filter timeout.

The global `exclude_dir_names` intentionally omits `target` because using it
for cleanup/staging had previously damaged vendored source named `target`.
Restoring that blanket exclusion would regress source safety. Instead:

- Discovery alone recognizes the standard 43-byte `CACHEDIR.TAG` signature and
  prunes those explicitly tagged cache trees. No file is deleted or unstaged;
  untagged source named `target` remains discoverable, as do nested standalone
  utility repos. Explicitly watched roots remain discoverable.
- `daemon/discovery_scheduler.rs` performs discovery in one background blocking
  job, refreshing at most every 30s. Slow discovery cannot block the coordinator.
  There is never more than one walk in flight, even when a filesystem stalls.
- Scheduling retains the last complete snapshot for the same watch configuration.
  Initial discovery and configuration changes do not supply partial/old-scope
  snapshots to retain/prune operations. Completed results from an obsolete
  configuration are discarded. Errors retain the last valid snapshot and retry
  after cooldown; a walk exceeding 60s emits a warning without spawning another.

Regression tests cover cache/source distinctions, explicit roots, nested repos,
blocked discovery retaining a usable snapshot without duplicate walks, and watch
configuration changes. All 22 discovery tests pass. Full workspace tests,
Clippy and the release build pass after these changes.

Release deployment and sustained live convergence must be checked independently;
unit tests and a successful restart do not prove every fleet gate passes.
