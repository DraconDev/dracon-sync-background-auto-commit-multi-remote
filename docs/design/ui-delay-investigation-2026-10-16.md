# Rich report and delay investigation — 2026-10-16

## Scope and baseline

Operator screenshot: `Screenshot_20261016_154250.png`, inspected natively.
Baseline captured 2026-10-16T15:01:25Z:
- sync HEAD `336e53527706be6924d0e2828da01a524e7ae473`
- warden HEAD `8452dee2269a60b6442d6cd099043ca30aa10a26`
- both working trees clean; sync service active, PID 3440303.

## Observed defects (not yet fixed)

- Rich ACTIVITY cell clips `WORKING (1 ahead)` while A/B already shows
  the same count. `current_health::label` appends the suffix and rich
  rendering truncates it rather than selecting a concise presentation.
- Screenshot header has vertical ticks at separator positions despite
  border-only rows. Inspect preset/intersection consistency and rendered
  terminal-cell widths, not just string lengths.
- Healthy hint link-out counts all repositories, hiding actionable
  retry/blocker details in the default rich output.
- 15:02:15Z journal capture includes dispatch-starved alerts around
  604–608 seconds for many repositories, with `no hold recorded`.
  This does not by itself prove scheduler unfairness: compare ownership,
  dispatch timestamps and actual commit/push progress before assigning cause.
- Platform status inspection timed out and scheduled retry in 300s.
  Earlier Git/filter waits suggest a filter-dependent inspection stall,
  but the exact internal Warden wait remains unproven.

## Investigation plan

Read-only scouts independently inspect renderer, scheduler/inspection,
and Warden packet/batch lifecycle. Parent owns all implementation changes.
Tests must distinguish timeout containment from root-cause repair and
exercise slow/fast concurrent work with explicit ownership cleanup.

## Safety

No operator product edits, asset retirement, guard bypass, history rewrite
or privilege expansion is authorized. Preserve encryption semantics. Deploy
only after passing gates, with atomic installs and retained rollback copies;
use sanctioned maintenance and normal service restart, never manual stop.

## Survey synthesis

Three read-only scouts completed (renderer, coordinator, Warden). Source
confirms the renderer overrides a border-only header intersection with a
cross, and rich activity clips the shared detailed label despite its comment.
The first fix keeps shared vertical labels intact and uses a concise rich
label; the footer exposes redacted row-associated reasons only for nonhealthy
states. Eight targeted rich renderer tests pass (see renderer-tests.log).
Further all-state and terminal-output capture coverage remains required.

Coordinator defects: status success is rejected after 30 seconds measured
from spawn, despite a 60-second command budget. This is a deterministic
liveness mismatch; a safe fix must retain snapshot-age accounting rather than
pretend old results are fresh. Discovery with no current-scope snapshot also
continues before ready-probe collection/watchdog sweeps. Lifecycle draining
must be independent of admission/discovery eligibility. Detail report status
still uses the legacy unbounded GitService call and needs owned cancellation.

Warden hazards are established in source, not established as the historical
incident cause: its five-second receive timeout excludes resolver and query
write time, teardown uses blocking wait, and the reader thread is unowned.
Fault-injection tests must establish these failure modes and ciphertext-safe
fallback before a hang remediation claim. Idle waiting between filter requests
is legitimate and must not receive a total driver lifetime timeout.

## Implementation checkpoint

- Rich-only activity label preserves detailed labels for other layouts;
  border-only header now has a continuous horizontal intersection. Footer
  filters by current classification, associates reasons with repo names,
  redacts credentials and strips terminal controls. Targeted renderer suite:
  9 pass, zero fail (`evidence/ui-delay-2026-10-16/renderer-tests.log`).
  All-state regression added that covers nine lifecycle states at four
  supported widths, long names, emoji, large counts, and asserted aligned
  borders plus complete ACTIVITY labels (`evidence/ui-delay-2026-10-16/
  rendered-states.log`).
- Ready probe collection/watchdog sweeps moved before discovery admission.
  Lifecycle draining (detached sync results and the 15-minute wedge valve)
  is also discovery-independent. Obsolete-scope repositories are still
  never admitted.
- Advisory status maximum age remains measured from original spawn, now
  explicitly 60-second inspection budget plus 30-second collection window.
  No old snapshot is relabelled fresh; downstream classification/staging
  remains authoritative. Boundary tests cover 29/30/31/45/59/60/89/90/91s.
  The existing cancellation-race test was updated to remain older than that
  explicit maximum, preserving its stale-clock assertion.
- Stale and failed status outcomes retain a hold reason. Detail report status
  uses owned `inspect_status` with an eight-second deadline and now parses
  `branch.oid` so report EMPTY detection is consistent with daemon results
  (`dracon-sync/src/git/inspection.rs` regression: `head_presence_survives_
  owned_inspection_for_report_empty_detection`).
- Targeted status suite: 38 pass, zero fail after updating the stale fixture
  (`evidence/ui-delay-2026-10-16/status-tests.log`).
- Warden transport now resolves and writes/reads under a single deadline
  on a separate helper process group. Direct children are signalled before
  reaping to prevent PGID reuse. Linux adds a pre-exec guardian that owns
  group cleanup after abrupt owner death (`prctl(PR_SET_PDEATHSIG)` +
  raw-clone + `close_range` + `sigsuspend` notification); non-Linux retains
  ordinary Drop cleanup only. 14 focused tests, 20 repeated abrupt-death
  checks and the equivalence test pass (`dracon-warden/docs/evidence/
  index-batch-*`).

## Quality gates

- `cargo test --workspace --locked`: pass
- `cargo build --release --locked`: pass
- `cargo clippy --workspace --locked -- -D warnings`: pass
- `cargo deny check`: pass
- Log: `dracon-sync/evidence/ui-delay-2026-10-16/workspace-tests.log`,
  `release-build.log`, `clippy.log`, `deny.log`, `final-gates.txt`.

## Deployment

- `dracon-sync maintenance -- bash dracon-sync/scripts/deploy-ui-delay-2026-10-16.sh`
  (declarative digest guard). Two consecutive deploys: the first surfaced a
  pre-existing report regression caused by switching the detail report to
  the owned inspection path; the second deployed the branch.oid report
  fix. Both were captured in `evidence/ui-delay-2026-10-16/deployment.log`
  with the post-restart identity check.
- Backup retention: `~/.dracon/backups/ui-delay-20261016-155232` and
  `ui-delay-20261016-160202` retained until a later deployment supersedes.

## Fleet observation (30 minutes)

Window `2026-10-16T16:02:58Z` to `2026-10-16T16:33:29Z` (duration
`0:30:31`); four samples ten minutes apart in
`dracon-sync/evidence/ui-delay-2026-10-16/observation/`.

- T0 (16:02:54Z): 25 OK / 6 active / 3 warn / 0 concern
- T10 (16:13:02Z): 26 OK / 7 active / 1 warn / 0 concern
- T20 (16:23:14Z): 28 OK / 1 active / 5 warn / 0 concern
- T30 (16:33:24Z): 32 OK / 0 active / 2 warn / 0 concern (`.dracon` is
  push-stuck, evidence-stale events.jsonl line; the live hint suggests
  `dracon-sync repair concerns --apply`).
- Daemon MainPID 2674977 and the live executable digest
  `8f7f45c9a82b41dfb5bf762fa9d3347837a8ffe534eee6c8115c5242e8db2030` match
  the deployed binary and release artifact at every sample.
- Journal activity across the window: 69 commits and 19 syncs captured in
  the snapshot windows; 40 trailing-drain handoffs (no wedge); zero
  status-stale or status-failed messages. Concurrent slow/fast repositories
  progress (T0 had 6 active, T30 had 0 active) without starvation or
  orphan work. The only persistent nonhealthy signal is the pre-existing
  push-stuck `.dracon` case, evidenced by its hint and the unchanged
  `events.jsonl` line.

## Documented limits

- Userspace deadlines cannot bound kernel-blocked spawn/exec, filesystem
  operations, or uninterruptible child teardown.
- Group signalling also does not establish bounded descendant reaping for
  nonchildren and processes outside retained ownership.
- Non-Linux abrupt-owner cleanup is explicitly unsupported (the rest of
  the platform's behaviour is unchanged).
- `.dracon` push-stuck persists across the window: pre-existing
  warden/secret-scan refusal on a long-lived `events.jsonl` write; not
  caused by this change.
