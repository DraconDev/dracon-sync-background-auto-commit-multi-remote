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

## 2026-09-17 follow-up: classification and dispatch boundary

### Live read-only recheck (approximately 09:04 BST)

`/tmp/sync-live-recheck-inventory.json` records the exact UTC observation time,
HEADs, upstream comparison, and dirty paths. No extra daemon or manual staging
was run against these repositories.

| Repo | Dirty paths | HEAD vs cached upstream | Observation |
| --- | ---: | --- | --- |
| freeport | 0 | 0 ahead / 0 behind | No current backlog |
| junk-runner | 0 | 0 ahead / 0 behind | No current backlog |
| polis | 0 | 0 ahead / 0 behind | Earlier leftover no longer present |
| ai-auto-writer | 437 | 0 ahead / 0 behind | Uncommitted backlog persists |

Cached upstream equality is **not** proof of equality with every remote.
The live journal at `/tmp/sync-live-recheck-journal.log` contains repeated
ai-auto-writer `git diff HEAD timed out` classification skips at 08:59:37 and
09:01:56 BST. Its per-repo override only disables build-artifact cleanup; it
does not exclude these changes. The live service remains active, PID 3658242,
executing `~/.local/bin/dracon-sync daemon`; it has not been replaced.

### Isolated before/after evidence

- `/tmp/sync-slow-filter-before.json` reproduced cross-repository delay from
  one fixture-local required slow filter: healthy staging took roughly
  10.7–10.9 seconds. This fixture never reads real secrets or modifies live
  filter configuration.
- The working source moves classification into retained per-repo jobs and
  propagates non-unborn classification failures rather than treating them as
  untracked-only results. Required filters still execute when staging.
- The initial cache refactor accidentally removed ready classification
  results **before** quiet-window eligibility. Moving removal to the actual
  `reserve_sync` dispatch boundary prevents that repeated traversal. Matching
  provisional and confirmed fingerprints preserves the first status-dirty
  observation while classification runs. New regression:
  `pending_classification_preserves_status_transition_clock`.
- Latest instrumented run: `/tmp/sync-instrumented-probe.json`, detailed trace
  `/tmp/sync-fairness-5rftcf9t/daemon.log`. All five repositories converged.
  B staging started 2.204 seconds after its edit; pre-start B2 staging started
  2.265 seconds after daemon launch. C staging started **3.074 seconds after
  its edit**, so the unchanged strict 3-second assertion still **fails**.
- C was observed clean just before the edit, dirty on the following pulse,
  then eligible after 2006ms of observed quiet. Its slow push starts after
  staging, so a post-push grace period cannot explain or repair this staging
  failure. Polling phase and worker preparation must be distinguished from
  queue delay; the current harness does not yet establish the full contract.
- CLI commit instrumentation captured **no** commit command boundaries:
  `sync_repo` calls `GitService::commit` (`src/sync.rs`). The new exact-commit
  assertion therefore fails for missing evidence, not measured push delay.
  Existing HEAD-observation timing is only an upper bound on commit time and
  is insufficient to prove the exact commit-to-push gate. Instrument the real
  commit completion path before accepting that gate; do not weaken it.

### Bounded validation of current working source

- `timeout 180 cargo test -p dracon-sync --locked`: PASS, 1038 unit tests,
  3 ignored, 10 integration tests, zero failures;
  `/tmp/sync-boundary-full-tests.log`.
- `timeout 180 cargo clippy -p dracon-sync --locked -- -D warnings`: PASS;
  `/tmp/sync-boundary-clippy.log`.
- `timeout 30 cargo fmt --all -- --check`: PASS;
  `/tmp/sync-boundary-fmt.log`.
- `timeout 10 python3 -m py_compile dracon-sync/scripts/verify-daemon-fairness.py`:
  PASS.
- Debug build passed (`/tmp/sync-instrumented-build.log`); the end-to-end
  fairness gate still FAILS for the reasons above. Release build, publishing,
  installation and 15-minute deployment verification remain outstanding.

### Additional timestamped diagnostic run (not an acceptance pass)

Added debug-only daemon-relative timestamps to status, eligibility and dispatch,
and bounded path-mtime logging at eligibility (up to eight entries; no contents).
Mtimes remain diagnostic evidence; scheduling has **not** switched to mtimes.

`/tmp/sync-timestamp-probe.json` and
`/tmp/sync-fairness-c70ips87/daemon.log` show d-filter classification execution
of 4117ms, first dirty status at daemon 264ms, and dispatch at 5339ms. Since
classification started after that status observation, ready-to-dispatch delay
is at most `5339 - 264 - 4117 = 958ms`. The focused one-pulse check passes;
`/tmp/sync-d-filter-focused-timing.json` records this conservative bound.
This is NOT an edit-to-dispatch subsecond claim: the fixture deliberately has
a 2s quiet window and a required 4s clean filter.

The full probe still fails. B and C timing passed in this run, but pre-start B2
missed its strict deadline, and exact CLI commit timing remains unavailable.
This variation is evidence of remaining timing/observation uncertainty, not
permission to relax the assertions. The debug logging itself adds measured
inspection work and no fleet-wide bound has been established.

After the timestamp instrumentation, bounded package tests, clippy and fmt all
passed again (`/tmp/sync-timestamp-tests.log`, `/tmp/sync-timestamp-clippy.log`,
`/tmp/sync-timestamp-fmt.log`). No publishing, installation or service restart
occurred. The next necessary work is a proper commit-completion instrument,
phase-sensitive scheduling tests, and remaining classification cancellation /
ownership review before deployment.

### Deterministic pre-start boundary regression

`pre_start_dirty_activity_reaches_exact_quiet_boundary_once` supplies logical
instants at 0/1000/1999/2000/3000ms, classification ready at 1000ms, and a
pre-existing dirty status. It verifies the activity clock stays anchored at
startup and eligibility plus ownership permit exactly one dispatch at 2000ms.
`timeout 180 cargo test -p dracon-sync --locked pre_start_dirty_activity_reaches_exact_quiet_boundary_once`
passed (`/tmp/sync-prestart-deterministic.log`); bounded clippy passed
(`/tmp/sync-prestart-clippy.log`). This is helper-level coverage, **not** a
replacement for the failing full-daemon timing probe.

A further unchanged probe (`/tmp/sync-selfcheck-probe.json`) converged all five
repos but failed C's staging bound (3.121s after edit) and the unmeasured CLI
commit boundary. No production fix for that remaining timing failure is
claimed. Inspection of dracon-git 94.7.2 confirms `GitService::commit` uses
libgit2 first, with CLI only as a fallback; the wrapper cannot observe ordinary
successful commit completion.

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
