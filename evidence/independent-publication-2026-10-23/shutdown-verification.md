# Shutdown cleanup verification (auditor objection 2)

The auditor objected that the isolated receipt could pass while the daemon's own
log said an owned task had not joined. That was a real bug, not a reporting
nit, and both halves are fixed.

## The bug

`shutdown` computed `outstanding = detached_syncs.len() + sync_workers.len()`.
A detached join entry IS a live `sync_workers` entry — the same task counted
twice — and each resolved join decremented exactly once. Two joined workers
therefore left the counter at 2, so the daemon printed

    ⚠️ shutdown: 1 owned sync task(s) did not join within 120s — leaving their
       evidence in place rather than claiming a clean stop

on a fleet that had ALREADY stopped, after sleeping out the entire 120s budget.
Arithmetic check (both workers joined): pre-fix outstanding 2 -> "did not join";
post-fix `workers.len()` = 0 -> clean.

## The fix

`drain_owned_workers()` watches `sync_workers` (the authoritative owned set) and
pulls join results from `detached_syncs`:

* a stopped fleet returns 0 immediately (clean drain, prompt exit);
* a worker with no pending join result breaks out at once instead of burning
  the deadline, and is still REPORTED (never silently swallowed) so evidence is
  kept when cleanup genuinely failed.

## Regressions

* `daemon::tests::shutdown_drain_settles_when_owned_workers_have_stopped` —
  clean drain, ownership released, finishes well inside the budget.
* `daemon::tests::shutdown_drain_does_not_burn_the_budget_without_join_results`
  — an unjoinable worker is counted, not hidden, and the drain does not sleep
  out the deadline.

Both pass; the pre-fix arithmetic provably fails the first.

## Receipt now asserts shutdown, not exit status

`scripts/isolated-publication-receipt.py` no longer treats process exit as
proof. It scans the daemon log for any `did not join` / `have no pending join
result` / `owned sync task` line, looks for orphan processes under the fixture
root, and fails if SIGTERM did not stop the daemon. Verified non-vacuous: fed
the exact pre-fix log line it reports `would_fail: True`; fed the clean log it
does not trip.

Result on the final artifact:

    ok: true
    shutdown: joined after SIGTERM
    shutdown_unclean_lines: []
    orphan_pids_after_shutdown: []

## Live check with the installed binary

A second, disposable daemon (fixture HOME/state/watch root, local bare remote)
was started on the installed binary, allowed to commit real work, then sent
SIGTERM:

    exited after 1s
    journal: no "shutdown", no "did not join", no "owned sync task" lines
    no processes left under the fixture root

That is a real bounded shutdown with no unjoined-work warning, on the deployed
binary, rather than an inference from test output.
