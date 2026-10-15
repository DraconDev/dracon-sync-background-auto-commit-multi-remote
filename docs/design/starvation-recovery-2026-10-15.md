# Per-repository starvation recovery — 2026-10-15

## Contract

Automatically recover abandoned or hung scheduler reservations, keep healthy
repositories progressing, bound retries, and never overlap workers for the same
repository. Recovery must not kill unrelated processes, delete live locks, or
restart the service as its normal mechanism.

This follows `status-lifecycle-audit-2026-10-15.md`. The implementation is built
and tested here; combined deployment and live observation are separate queue
items. No installed binary or service was changed for this item.

## Root causes and implementation

### Probe ownership and recovery

`src/daemon.rs` now retains a `ProbeOwner` containing the actual abort handle,
Tokio task identity, and cancellation-requested state. Status and classification
watchdogs request cancellation once; they do **not** release a live reservation.
The ready-only collectors resolve `JoinError::id()` against that retained owner,
turn panic/cancellation into a repository-specific error result, and release
ownership only after the join proves task teardown. Unknown orphan joins cannot
release another repository. An ownerless reservation is immediately reclaimable.

The pulse boundary collects ready completions before sweeping **all** pending
probes, including repositories removed from discovery or bypassing downstream
eligibility gates. Such repositories can no longer retain global probe slots
indefinitely simply because their per-repo watchdog is unreachable. Per-repo
checks remain as a second boundary after a long scan.

Cancellation state is distinct from the original spawn timestamp. Completion
can race cancellation: aborting an already-finished task is a no-op, and its
successful late status must retain the original age rather than be accepted as
fresh. `probe_race_tests.rs` covers that precise interleaving.

### Fair bounded admission

`recovery::fair_probe_order` sorts by least-recent probe admission, never-admitted
repositories first. Existing deadline/ready-result hints are stable tie-breakers,
not permanent priority tiers that can monopolize a bounded pool. Both status
and classification admission stamp the production ledger. Existing global caps
remain four status jobs and two classification jobs.

Status failures retain their 300-second retry cooldown. Classifier failures
(including joined cancellation/panic errors) retain exponential 1, 2, 4, ...,
300-second capped backoff; failure increments now saturate. Successful classifier
results reset the streak. Eligibility, ownership, quiet-window, staging, hook,
size-limit, and forward-only bucket guards are not bypassed.

### Sync worker lifecycle

`recovery::SyncRecovery` separates queued from executing workers. A worker stamps
execution start **after acquiring** its shared FIFO semaphore permit. The
15-minute execution watchdog does not repeatedly cancel healthy waiters and
send them to the back of that queue. Cancellation is requested once per running
worker. Ownership and its semaphore permit survive until actual teardown.

The repository tag is now outside the fallible worker join: an unspawned,
coordinator-polled future awaits the actual worker. There is no second spawned
wrapper that can lose the repository identity on its own `JoinError`.

Both completion paths call the same `release_sync_owner` helper before any
activity-entry early exit. It clears ownership, abort handle, current-cycle
membership, and detached residence timestamp. This removes phantom detached
ages that previously cancelled a later healthy generation prematurely.

Joined sync errors, including panic/cancellation, receive exponential backoff
capped at 300 seconds; success clears it. An obsolete test endorsing the unsafe
practice of clearing still-running owners at a drain boundary was replaced by
the opposite production invariant.

## Prompt-to-artifact verification

| Requirement | Concrete evidence |
| --- | --- |
| Healthy peers progress beside a hung probe | `recovery_status_waits_for_teardown_and_healthy_peer_progresses`: actual healthy status collected while a controlled cancellation-resistant task remains owned |
| Healthy queued workers progress after hung execution recovery | `recovery_fifo_healthy_syncs_run_after_hung_holder_joins`: actual one-permit semaphore, explicitly registered FIFO waiters B/C, owned hung A cancellation, then B before C |
| Repeatedly failing early repos cannot monopolize admission | `fair_admission_serves_every_repo_despite_repeated_early_failures`: real production ordering helper admits every repo within three four-slot rounds despite adversarial fixed priority |
| Abandoned reservations recover | `recovery_abandoned_reservations_release_without_touching_live_owners`: both production watchdog helpers reclaim missing-owner/missing-metadata reservations |
| Panic/cancellation retains identity and releases only after join | `recovery_classification_panic_and_cancellation_release_known_owners`, status teardown test, and `recovery_tagged_sync_join_releases_all_bookkeeping_on_panic_in_quiet_cycle` |
| Retries remain bounded | `queued_workers_keep_fifo_and_execution_retries_are_bounded`: 100 error rounds, exact before/at-deadline checks, 300s cap and success reset; existing status/classifier cooldown boundary tests also pass |
| No overlapping workers | Status controlled teardown test, FIFO sync test, existing exclusivity tests, and replacement trailing-drain test; no force-clear of a live owner |
| Safe recovery mechanism | Reviewed diff changes only scheduler ownership/admission/cancellation and tests; no new PID/name killing, lock removal, or service restart path |

## Verification and review

All commands run from the utilities workspace with its configured serial test
execution. Raw output lives in `evidence/starvation-recovery-2026-10-15/`.

- `timeout 180 cargo test -p dracon-sync --locked recovery`: exit 0; eight
  scheduler recovery tests plus five matching library tests pass, one unrelated
  library fixture ignored. `recovery-final.log` and `reviewer-reproduction.log`.
- `timeout 360 cargo test --workspace --locked`: exit 0; 35 successful suite
  summaries, 2,399 passed, zero failed, 25 ignored. `workspace-final.log`.
- `timeout 180 cargo clippy --workspace --locked -- -D warnings`: exit 0.
  `clippy-final.log`.
- `timeout 590 cargo build --release --locked`: exit 0. `release-final.log`.
- `timeout 120 cargo deny check`: exit 1 on the four previously recorded rustls
  advisories RUSTSEC-2026-0346 through RUSTSEC-2026-0349. Bans, licenses and sources
  pass. No dependency versions or advisory exceptions were changed here;
  dependency remediation is a separate confirmed queue item. `deny-final.log`.

Verification history is not hidden: `package-tests.log` contains an early
failure in the new controlled multi-thread teardown test. The fixture assumed
one nonblocking drain must see `is_finished` immediately. A completed Tokio
handle and FuturesUnordered's ready notification can become observable at
different instants. The test now exercises repeated bounded nonblocking pulse
drains instead of asserting that unsupported single-poll guarantee.
`package-tests-final.log` passes after that fixture correction; the later
full-workspace pass covers the final owner/timestamp-race changes as well.

Independent fresh-context reviewer inspected the baseline `7fc6746` to current
source changes using `review.diff`, checked the reproduction log, and reported
no issues / merge verdict OK in `reviewer.md`. The reviewer had read-only tools:
command execution was performed by the parent and is explicitly attributed as
such in that report. Scout findings are preserved in `scout.md`.

## Safety and limits

This is scheduler recovery, not a claim that arbitrary synchronous or
uninterruptible downstream work can safely be force-terminated in process.
When cancellation has not completed, the system deliberately retains ownership
and does not launch an overlapping replacement. Tests include a bounded,
controlled cancellation-resistant poll and prove that boundary. Underlying
command/hook timeout improvements and production rollout remain separate queue
items. Recovery does not change the existing lock janitor or relax any lock
liveness check. The default response is never to restart the daemon or delete a
live lock to manufacture progress.
