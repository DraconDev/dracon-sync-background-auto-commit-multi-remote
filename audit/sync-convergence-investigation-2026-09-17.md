# Sync convergence investigation — in progress

## Scope and status

The requested 2–3 second dispatch target has **not** been demonstrated fleet-wide.
No release/install or 15-minute live verification has been performed for these
changes. The live daemon remains running on v0.113.58. No live-repository
collision was intentionally provoked and no manual push substitutes for daemon
convergence.

## Evidence and findings

- Baseline journal captured at `/tmp/sync-convergence-baseline.log` contains a
  junk-runner `index.lock` failure followed by later successful work. It proves
  a collision occurred, not the identity of its competing owner or that it
  explains other repositories' delays.
- A bounded 12-second `strace` of daemon PID 3658242 and descendants, filtered
  to the presumed junk-runner index-lock path, observed no matching lock
  operations. `/tmp/sync-junk-index-lock.trace` and
  `/tmp/sync-junk-index-lock.stderr` record the attempt. This is inconclusive;
  it is not a fleet-wide process/lock capture. The daemon remained active.
- `src/daemon.rs` awaited result collection for two pulse intervals followed
  by up to `trailing_drain_deadline_secs` (default 120 seconds), despite an
  existing persistent detached-job registry. Removing those waits does not
  cancel worker futures or shorten push/filter execution deadlines.
- Regression `pending_result_collection_does_not_block_next_repo` failed
  against an extracted blocking `tasks.next().await` helper (100ms timeout)
  and passed with single-poll collection. Logs:
  `/tmp/sync-nonblocking-before.log`, `/tmp/sync-nonblocking-after.log`.
  This is a collector regression, **not** a full pre-fix daemon timing test.
- Regression `test_index_lock_contention_preserves_work_and_recovers` uses a
  temporary repository and exclusive `index.lock` creation. Sync fails while
  the lock exists, preserves the file and lock, then commits the exact file
  content after release. A first test run uncovered fixture ownership
  misconfiguration; setting only the fixture's temporary watch root fixed
  the setup. Production ownership protections were not weakened.
- Regression `stalled_worker_retains_exclusive_dispatch_ownership_across_cycles`
  passes for four pending-collection cycles and allows a new reservation only
  after observed completion. Production now checks ownership before per-repo
  maintenance and reserves it at the dispatch boundary. This validates the
  helper invariant; it does **not** establish a historical duplicate push or
  cover the separate 15-minute wedge recovery path.

## Verification so far

`cargo test -p dracon-sync --locked`: 1031 unit tests passed, 3 ignored;
10 integration tests passed, zero failures (before the additional ownership
regression). `/tmp/sync-convergence-suite.log`.

Additional ownership regression: one passed, zero failed;
`/tmp/sync-exclusive-worker.log`. `cargo fmt --all` run afterwards.

## Remaining investigation and gates

1. The scan loop still does sequential discovery, status/filter-aware diffs,
   and some network maintenance. These can consume far more than one pulse;
   nonblocking result collection alone cannot prove the requested target.
2. The 15-minute wedge path clears ownership without demonstrating termination
   of the old worker. Generation-based result discard is not process
   cancellation. This needs a safe bounded recovery design and real coverage.
3. Establish measured event/quiet-window/queue/worker/remote timing, including
   continuous edits, failed/slow repos, missed events and future timestamps.
4. Audit status truthfulness and retry fairness, including external blocks.
5. Run all required package/workspace gates; release/install changed utilities
   via the sanctioned maintenance workflow; verify installed binaries.
6. Observe 15 minutes of live convergence and report each sampled repo's
   actual state. No current evidence supports claiming that work complete.
