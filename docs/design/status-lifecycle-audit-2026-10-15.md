# Inspection/discovery lifecycle audit — reliability queue item 1

## Scope and finding

Audited current status inspection ownership, watchdog reservation release,
process-group cancellation, and single-flight discovery. This is not the later
queue items for broader starvation recovery, hook isolation or production soak.
The existing implementation and deployment context is documented in
`status-inspection-lifecycle-2026-10-15.md`.

Remaining ordering gap: at a per-repo boundary, the age watchdog ran before
ready-result collection. An inspection could already be finished but still
appear reserved when a slow coordinator returned to it. Elapsed reservation
age alone does not prove that the owned job is still running. Collection now
precedes the watchdog. The regression
`daemon::tests::completed_status_is_collected_before_wedge_sweep` uses a finished
job with a 61-second-old reservation and proves it is collected successfully,
the reservation is released, and no wedge abort is reported. Existing stale
result checks remain intact: this change does not claim an old result is fresh.

## Ownership evidence

- `git::inspect_status` owns an asynchronous Git child and bounded captured
  output through the existing process-group guard; no blocking libgit2 job is
  abandoned by async cancellation.
- `git::inspection::tests::hung_filter_is_killed_and_next_inspection_recovers`
  exercises the inspection's internal timeout with a real clean filter.
- New `git::inspection::tests::owner_abort_kills_filter_and_next_inspection_recovers`
  starts that filter with a long internal budget, waits for its helper PID,
  explicitly aborts the owning Tokio task, verifies cancellation, verifies the
  helper has exited (or is a non-running zombie awaiting reaping), and then
  successfully reinspects the same repo after clearing the fault.
- `daemon::tests::wedged_status_task_aborted_and_slot_freed` verifies removal
  from pending/spawn-time/abort maps and typed cancellation. The cancelled join
  collector regression verifies that it inserts no fabricated result.
- Discovery retains at most one blocking walk and a complete same-scope
  snapshot; it deliberately does not timeout and respawn blocking walks.
  Its existing regression verifies a blocked refresh does not block polling
  or create duplicate workers. No history, filters, hooks, staging eligibility,
  identity, credentials or limits are altered by this item.

## Reproduction and results

From the utilities workspace, one bounded regression command:

```
timeout 180 bash -c 'cargo test -p dracon-sync --locked git::inspection && cargo test -p dracon-sync --locked status_'
```

Exit 0: five inspection tests pass; status-filtered suites pass (one library
and 35 binary tests). Clippy `timeout 180 cargo clippy --workspace --locked --
-D warnings` and `timeout 590 cargo build --release --locked` exit 0.

Full verification history is preserved, not flattened into a false green run:

1. `timeout 590 cargo test --workspace --locked` timed out in the warden unit
   suite after 30 successful suites / 2,165 passing tests and no assertion
   failures. The named warden test passes in isolation.
2. A retry with `--test-threads=2` was invalid: `.cargo/config.toml` requires
   serial testing because fixtures mutate process-global environment. It
   produced 198 failures from overlapping fake Git/PATH fixtures. No product
   change was made to accommodate that invalid invocation.
3. Correct serial reruns: `timeout 360 cargo test -p dracon-warden --locked`
   exits 0 (206 + 18 + 2 tests); `timeout 360 cargo test -p dracon-sync --locked`
   exits 0 (144 + 1,295 + 10 + 14 tests, plus doc tests). Together with the
   completed unaffected workspace suites, all workspace test suites have
   passing coverage. This is aggregate coverage, not a claim that the timed-out
   all-workspace invocation passed.
4. `timeout 180 cargo deny check` exits 1 on the four existing rustls advisories;
   bans, licenses and sources pass. Dependency remediation is a separate
   confirmed queue item; no advisory exceptions were added.

Raw output: `evidence/status-lifecycle-audit-2026-10-15/` includes regression,
Clippy/build/deny, initial workspace timeout, invalid parallel retry, and both
correct serial package reruns.

## Deployment boundary

This item builds but does not install the new binary. Combined deployment with
rollback and the 30-minute observation are separate confirmed queue items.
Broader scheduler recovery and fairness remain the next item; this audit does
not claim the entire fleet is permanently converged.
