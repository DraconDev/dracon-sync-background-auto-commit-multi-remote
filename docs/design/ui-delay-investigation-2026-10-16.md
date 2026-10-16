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

## Pending evidence

Root-cause synthesis, fixes, deterministic regressions, quality gates,
deployment record and timestamped 30-minute live observation remain pending.
