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
  8 pass, zero fail (`evidence/ui-delay-2026-10-16/renderer-tests.log`).
- Ready probe collection/watchdog sweeps moved before discovery admission;
  obsolete-scope repositories are still never admitted. This does not yet
  establish that all lifecycle work, including sync joins, is discovery-independent.
- Advisory status maximum age remains measured from original spawn, now
  explicitly 60-second inspection budget plus 30-second collection window.
  No old snapshot is relabelled fresh; downstream classification/staging
  remains authoritative. Boundary tests cover 29/30/31/45/59/60/89/90/91s.
  The existing cancellation-race test was updated to remain older than that
  explicit maximum, preserving its stale-clock assertion.
- Stale and failed status outcomes retain a hold reason. Detail report status
  uses owned `inspect_status` with an eight-second deadline rather than the
  legacy uncancellable call. Other report operations remain to be assessed.
- Targeted status suite: 38 pass, zero fail after updating the stale fixture
  (`evidence/ui-delay-2026-10-16/status-tests.log`).

## Pending evidence

Warden bounded transport implementation and fault fixtures, discovery-integrated
regressions, expanded renderer/all-state/captured-output coverage, full quality
gates, deployment record and timestamped 30-minute live observation remain
pending. No installation or service restart has occurred in this goal yet.
