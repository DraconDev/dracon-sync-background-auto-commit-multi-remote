# Code Context

## Files Retrieved
All inspected source is `src/daemon.rs`; no source edits or execution tests.
1. Lines 48-69, 698-734 — tagged join type, reservation/cancellation helpers, ownership contract.
2. Lines 900-932, 1960-2044 — cancellation outcome handling and existing exclusivity tests.
3. Lines 4220-4383 — concurrency tests and obsolete force-clear regression.
4. Lines 6945-6990, 9180-9399, 9368-9644 — worker dispatch, registry, both collectors, cancellation sweep.
5. Lines 505-509 — nonblocking ready-only collector helper.

## Key Code
- `reserve_sync` inserts a repo into `in_flight`; `request_worker_cancellation` only calls the actual worker's `AbortHandle::abort()` (698-705).
- Worker acquires the shared permit before sync, holding it throughout sync (9267-9301).
- Wrapper preserves inner worker `JoinError` as a repo-tagged result (9336-9353).
- FIRST collector: `let Ok((repo, ...)) = joined else { continue; };` then removes ONLY ownership/abort-map entries (9372-9382).
- SECOND collector removes ownership, abort handle, cycle-set membership AND detached timestamp, but only on outer `Ok` (9462-9481).
- Handoff stamps every surviving cycle-set member, using `or_insert_with`, not every actual pending handle's identity (9548-9561).
- Sweep aborts after 900s and resets the timestamp; it never releases ownership (9564-9581).

## Architecture
One actual Tokio worker per reserved repo may be queued on the global semaphore. A second spawned task awaits that worker and returns repo/counters/outcome. Ready-only collectors apply wrapper results; remaining wrappers move into a persistent `FuturesUnordered`. `in_flight` survives pulses. Even quiet cycles enter collection when detached jobs exist (9329). The sweep requests cancellation of the actual worker, not its wrapper.

## Findings / Smallest Safe Corrections
### 1. Confirmed: completed first-drain jobs leave phantom detached ages
`dispatched_this_cycle` is seeded at 9335. The entire first collector (9372-9442) never removes its completed repo from that set or `detached_since`. Handoff (9548-9561) consequently stamps completed jobs even when no handles remain. Those timestamps persist because only the second collector removes them (9481).
Consequences: misleading pending counts/cancellation warnings; a later real dispatch of the same repo inherits an old timestamp (`or_insert_with`) and can be cancelled prematurely. Old ghost timestamps are also reconsidered whenever any detached/new work opens the collection block.
Smallest fix: identical bookkeeping in BOTH completion paths, before the activity-entry early exit: remove repo from `in_flight`, `sync_workers`, `dispatched_this_cycle`, and `detached_since`. Prefer one shared release helper to prevent divergence.

### 2. Confirmed structural hole: outer wrapper failure loses ownership identity forever
The wrapper's own `JoinHandle` can return `Err(JoinError)`. Both collectors drop this result without cleanup (9373-9375; 9470-9481). Its payload is the only repo tag (58-62). Once dropped, the registry may be empty while `in_flight` and worker/timestamp maps retain the repo. The quiet-cycle gate then stops running the sweep; if other work opens it, aborting the already-finished worker still cannot recreate the lost result.
Important distinction: a panic in the ACTUAL sync worker is already handled correctly by the wrapper (9341-9351); it becomes a tagged ordinary failure and ownership is released. The missing case is failure of the outer wrapper itself (no direct normal-path abort of that wrapper found).
Smallest robust correction: retain repo identity outside the fallible join boundary, e.g. an unspawned tagged future awaiting the actual worker handle in `FuturesUnordered`; preserve inner `JoinError` mapping. Do not merely clear ownership on an anonymous wrapper error: that does not prove the actual worker stopped.

### 3. Hung non-yielding work remains pinned; timeout is not teardown
The existing sweep is intentionally only an abort request (9564-9581). A worker stuck in synchronous/non-yielding work can keep its reservation AND semaphore permit forever. Repeating abort every 15 minutes does not change this. Force-clearing the reservation would introduce overlapping mutation/pushes, so it is not a safe recovery.
Within this file, safe improvements are explicit queued/running/cancelling state, original dispatch/start timestamps separate from warning timestamps, and visible unhealthy cancellation-pending state. Actual bounded termination requires cancellation-safe/bounded sync dependencies or process isolation; the delegated scope does not establish those dependencies' teardown behavior. Worker join proves Rust task teardown, not independently that every spawned subprocess/blocking child has stopped.

### 4. Fairness weakness: queue wait and execution share the same wedge clock
Reservations and wrapper detach ages start before semaphore acquisition (9202; 9267-9285; 9557-9561). Every old queued worker is aborted along with genuinely hung running workers after 900s (9568-9579), even if never granted a permit. On redispatch it loses its previous semaphore queue position. This can repeatedly recycle healthy waiters behind long-held permits; no bounded execution/retry fairness follows from the current registry.
Smallest direction: track queued versus acquired/run start; preserve FIFO waiting ownership instead of treating waiting as a wedged execution. Bound/recover the actual permit holder first. Add bounded retry eligibility/backoff for repeatedly cancelled execution without releasing before join. Do not globally abort queued reservations to manufacture progress.

## Existing Safeguards / Tests
- Ready-only polling does not drop pending handles (505-509); persistent handoff retains ownership (9533-9561).
- Actual-worker cancellation retains ownership until observed join (6973-6975; 9564-9581); cancellation maps to PushPaused without failure/backoff or pause-memory reset (909-930).
- `stalled_worker_retains_exclusive_dispatch_ownership_across_cycles` (1981-1999) checks retained reservation across polls.
- `wedge_cancellation_waits_for_worker_teardown_before_redispatch` (2001-2043) checks cooperative future Drop before release; does not test blocking/non-yielding work or the production collectors.
- `test_trailing_drain_clears_stuck_in_flight` (4295-4351) explicitly endorses the OLD unsafe behavior of clearing still-running owners. Replace it; it currently provides false assurance and contradicts production.

## Deterministic Tests Suggested
1. Complete one worker before first ready-drain; shared collector must leave all four bookkeeping structures empty and no detach age. Redispatch at synthetic +901s must NOT cancel its fresh worker.
2. Fault-inject inner worker panic and outer wrapper failure separately; verify repo identity survives, actual teardown precedes release, registry is eventually empty, and unrelated repo progresses. Use channels, not sleeps.
3. One-permit semaphore: hold A via oneshot; queue B/C; advance injected time past wedge threshold. Assert B/C retain queue order and are not execution-timeout victims; after A teardown B runs before C, with max active count one.
4. Simulate cancellation-resistant poll with a controlled channel/barrier on a multi-thread runtime, release it explicitly, and assert ownership remains until join. Avoid an actual infinite loop or leaking thread.
5. Quiet-cycle test: no new dispatch, detached completion/cancellation still reaps and releases exactly once. Run the same collector used by production, not manual HashSet removals.

## Start Here
Open `src/daemon.rs:9372-9382` first: fixing first-drain bookkeeping is small, concrete, and prerequisite to trusting cancellation ages. Then unify the two collectors and preserve identity across all join outcomes.

BLOCKERS: none for the scoped corrections; safe forced recovery of genuinely non-yielding/subprocess work requires dependency teardown verification outside this investigation.

```acceptance-report
{"criteriaSatisfied":[{"id":"criterion-1","status":"satisfied","evidence":"Cited worker registry holes, existing safeguards, minimal corrections and deterministic tests above."}],"changedFiles":[],"testsAddedOrUpdated":[],"commandsRun":[],"validationOutput":["Manual read-only source inspection; no tests executed."],"residualRisks":["Sync dependency subprocess/blocking-task teardown not inspected; cannot attest cancellation prevents all external overlap.","Outer wrapper failure is structurally unhandled but no routine wrapper-abort path was found."],"diffSummary":"Source unchanged; findings artifact only.","reviewFindings":["First-drain completion leaves phantom detach ages.","Outer wrapper JoinError loses repo ownership identity.","Non-yielding workers can retain permits indefinitely; queued workers share wedge age."],"manualNotes":"Bounded investigation restricted to src/daemon.rs worker ownership/cancellation/reaping and related tests. No services, locks, Git state or child processes changed."}
```
