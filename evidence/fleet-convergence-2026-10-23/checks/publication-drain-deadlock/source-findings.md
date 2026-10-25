# Code Context

## Files Retrieved
All citations use captured **`fa9728b7358372c1fc169ca776fee551b9057ae7`**, inspected with `git show`; not current HEAD. Source paths below are relative to `/home/dracon/Dev/dracon-utilities/dracon-sync/`.
1. `src/sync.rs` (2334–2489, 5435–5469, 5527–5548): refresh admission, publication actor, lease ownership.
2. `src/git/push_resources.rs` (17–40, 47–99): task-local capacity, upload permit release/restoration, nested refresh admission.
3. `src/daemon.rs` (7859–7863, 9726–9763, 9986–10012): exclusive scheduler reservation, worker admission, detached registry lifetime.
4. `src/daemon/recovery.rs` (17–57, 128–144): evidence derives Working/execution age from worker lifetime; execution budget can greatly exceed local timeout.
5. `src/daemon/scheduler_evidence.rs` (48–104, 163–178, 190–229): publication execution stamps, freshness, recovery-derived phase, eligibility evidence.
6. `src/git/push_resources_tests.rs` (59–151), `src/sync/publication_actor_tests.rs` (105–122, 193–208, 245–275): existing tests cover refresh capacity and same-repo commits, but inspected scenarios do not deterministically force the completion/drain race below.

## Key Code
**Source-proven circular-wait interleaving at upload completion, not an idle actor registry policy.**

`sync.rs:2395–2401` wraps the upload with `push_resources::push_phase`. That helper drops the outer local-work permit, runs the upload, then **reacquires and retains the outer permit before returning** (`push_resources.rs:63–75`).

`sync.rs:2406` breaks the actor select when this wrapped publication future returns. Actor then awaits an already-started refresh **without cancelling it** (2448–2466). Refresh enters `local_phase` (2347–2361), which may already be suspended acquiring the **same** local-work semaphore (push_resources.rs:84–99).

### Concrete valid interleaving (capacity = 1)
1. Actor drops its outer permit for upload. Another healthy local worker B takes it.
2. Upload finishes. Its `push_phase` restoration queues acquisition first; publication execution stamp clears as upload ends (`scheduler_evidence.rs:86–95`). Wrapped publication future is still pending on capacity.
3. Actor refresh timer fires. Refresh observes outer permit absent and queues its own `local_phase` acquisition behind restoration.
4. B finishes and releases capacity. FIFO grants the sole permit to upload restoration, which stores it in outer task-local `LocalWork`.
5. Publication branch wins; actor leaves select and awaits the pending refresh.
6. Refresh cannot acquire the permit held by its own awaiting outer actor. Outer permit is released only when that owner exits. **Permanent circular wait absent external cancellation.**

The pending refresh passed the `admitted` check before outer restoration; it does not recheck afterward. Multiple actors can likewise retain all capacity while draining refreshes. Capacity one suffices to prove the invariant defect; fleet occurrence is not established by leases alone.

## Architecture
- `Working`/`execution_secs` intentionally describe a reserved worker whose initial local admission started (`recovery.rs:37–53`), not active CPU/Git work. `admitted_secs` is time since scheduler admission. Neither is an intentional idle-daemon actor registration label.
- `eligible=false` is not an independent commit decision inside this actor. Scheduler skips repo maintenance/status/classification while `in_flight` owns it (`daemon.rs:7859–7863`). Detached workers deliberately retain that reservation until completion/apply (9986–10012).
- During a healthy upload the actor itself supplies fresh same-repo commit passes every 750ms/3s, rather than scheduler redispatch (`sync.rs:2403–2441`). Therefore suppressing duplicate outer dispatch is correct **only while this worker still guarantees local refresh progress or bounded completion**.
- Cross-process busy publication admission also deliberately permits commit-only work (`sync.rs:5435–5469`). A held publication lease alone is not busy-local-work evidence. Operation lease is released before upload (2386); publication lease belongs to the whole worker context.
- In the proven deadlock, no upload remains, no refresh reaches operation-lease acquisition, outer worker keeps publication lease and scheduler reservation, and aged worker/publication stamps disappear. Thus fresh evidence can read `Working`, old execution/admission ages, null publication phase, no operation lease, no child processes, while edits stay uncommitted. This matches the supplied **shape**, not a definitive runtime stack diagnosis.
- Execution recovery budgets include scaled push deadlines × retries × targets (`recovery.rs:128–144`); a long-lived reservation need not yet trigger cancellation. Do not relabel or infer publication activity from lease ownership.

## Start Here
Open loaded `src/sync.rs:2395–2466` beside `src/git/push_resources.rs:63–99`.

**Root invariant:** an actor draining admitted/pending refresh work must not retain capacity that the refresh needs to finish. Keep ownership protection, preserve non-cancellation of actual local mutation, and restore outer local capacity **after** refresh drainage, before post-upload mutation. A behavioral fix should restructure the upload/yield/restoration boundary accordingly; merely clearing `in_flight`, dropping publication lease, changing evidence labels, or cancelling mutation is unsafe.

**Smallest deterministic behavioral regression:** use a one-permit semaphore and controlled futures/barriers. Hold capacity with B; finish upload and poll restoration until queued; trigger/poll refresh until queued behind it; release B. Assert actor completion within bounded virtual/test time, refresh executes, outer local ownership is restored only after refresh completion, and all capacity/leases return on completion. Drive production orchestration via a narrow injected upload/refresh helper if necessary. Add a subsequent same-repo tracked edit assertion to the existing actor integration test. Existing success-before-hook-release coverage does not force this completion/drain ordering. No test was executed or added here.

BLOCKERS
- Runtime attribution still needs parent source/state correlation; observed leases/evidence do not prove semaphore wait queues or this exact interleaving. The source defect itself is proven by the ownership/order trace above.
- No production edits, control operations, tests/builds, or new agents; only required external findings artifact written.

```acceptance-report
{
  "criteriaSatisfied": [{"id":"criterion-1","status":"satisfied","evidence":"Loaded-source ownership/order trace proves upload-restoration versus final-refresh circular wait and gives deterministic regression direction."}],
  "changedFiles": [],
  "testsAddedOrUpdated": [],
  "commandsRun": [{"command":"Read-only git show fa9728b with selective source searches","result":"passed","summary":"Inspected actor, semaphore, scheduler and evidence seams; no executable tests."}],
  "validationOutput": ["A pending refresh can await the same sole permit restored to the outer actor that awaits it."],
  "residualRisks": ["Actual fleet deadlock interleaving remains unconfirmed without runtime correlation."],
  "diffSummary": "External findings artifact only.",
  "reviewFindings": ["blocker: src/sync.rs:2448–2466 and src/git/push_resources.rs:68–74 permit circular wait at publication completion."],
  "manualNotes": "Working is worker lifetime, not proof of active publication. BLOCKERS: runtime attribution pending."
}
```
