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

Release deployment and sustained live convergence must be checked independently;
unit tests and a successful restart do not prove every fleet gate passes.
