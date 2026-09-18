# Sync convergence — 0.113.64 consolidated report (2026-09-18)

Supersedes the timing claims in `sync-convergence-investigation-2026-09-17.md`
where they conflict. Every claim below names its artifact; anything without an
artifact is labeled as pending or withdrawn.

## 1. Release and deployment (verified this session)

- **Release commit** `703f91b` (`release: v0.113.64`), tag
  `dracon-sync-v0.113.64` verified at the same SHA on all three remotes
  (`git ls-remote {origin,github,gitlab} refs/tags/dracon-sync-v0.113.64`).
- **Gates**: `scripts/release.sh 0.113.64` ran the full AGENTS.md discipline
  (`cargo test --workspace --locked`, `cargo build --release --locked`,
  `cargo deny check`, `cargo clippy --workspace --locked -- -D warnings`) —
  all passed. Full log:
  `audit/sync-convergence-repair-evidence/release-0.113.64/release-0.113.64.log`.
  (One dry-run left the parent `Cargo.lock` at 0.113.64 while the nested
  manifest reverted to 0.113.63; the lock was re-synced offline to the
  manifests and the daemon committed the revert before the real run. The
  failed first real-run attempt died on `--locked` drift, not on a test.)
- **Install**: `~/.local/bin/dracon-sync --version` = `dracon-sync 0.113.64`;
  `scripts/verify-install.sh` passes (untracked=0, gitignore handling intact).
  Installed via rename (direct overwrite fails with ETXTBSY on the running
  binary).
- **Running**: daemon restarted as PID 55328; `/proc/55328/exe` md5 equals the
  installed binary md5 (`0ff2a52551bcb283bcdc5208375727b7`). Deployment record:
  `audit/sync-convergence-repair-evidence/release-0.113.64/deployment.json`.
- crates.io publish succeeded; GitHub release created. No history rewritten,
  no remotes deleted, no exclusions added, no protections weakened.

## 2. What 0.113.64 fixes (each with a fail-before/pass-after regression)

CHANGELOG `[Unreleased]` entry (shipped as 0.113.64) covers seven items;
the tests below exist in `src/` and pass in the release gates:

1. Three-tier scan order (`scan_priority`: due → ready-classification →
   unknown → established-clean). Scheduling hint only; eligibility,
   ownership, and filter gates unchanged.
   `daemon.rs:1268 ready_restart_work_precedes_unknown_inspection_with_fixed_pulse_budget`
   (failed at 3.2s vs the fixed 3s bound before, passes after).
2. Wedge recovery retains ownership until worker teardown (`sync_workers` +
   `request_worker_cancellation`; `in_flight` released only after join).
   `daemon.rs:1354 wedge_cancellation_waits_for_worker_teardown_before_redispatch`.
   Known residual, disclosed not claimed: the 15-min wedged path
   (`daemon.rs` trailing drain) now only calls `request_worker_cancellation`
   and never releases the reservation itself, but when the aborted join
   resolves cancelled, the apply arm (`let Ok(..) = joined else { continue; }`)
   skips the release too — so a wedged-then-aborted repo holds its
   reservation until daemon restart. Fail-safe (no duplicate dispatch can
   occur while synchronous work may still run) but not self-healing (no
   redispatch without a restart). Reachable only after a >15-min wedge;
   no fix is claimed for it in 0.113.64.
3. Typed push-cancellation aggregation (JoinError identity preserved through
   anyhow; `push_error_is_cancellation` via downcast; cancellation-only
   results never arm the 300s stuck backoff; mixed results still record real
   failures). `daemon.rs:2448` + `daemon.rs:2503` (incl. spoof-text negativas).
4. Parent-owned mirror pushes (`join_all` + `catch_unwind`, no detached
   spawn). `multi_remote.rs:1826 cancelled_mirror_parent_terminates_push_process_group`.
5. Process-group teardown + stderr-holder ordering (`GroupKillGuard`;
   drain-before-reap in `run_child_inner`).
   `ops.rs:747 cancellation_after_leader_exit_kills_stderr_holder`.
6. Quarantined repos stay excluded in every discovery form
   (submodule-fallback + canonical path checked before legacy-anchor
   conversion). `discovery.rs:1133
   discover_git_repos_exclude_repos_suppresses_submodule_fallback_candidate`.
7. Serialized forge-existence persistence (`PERSISTENT_EXISTS_WRITE`).
   `multi_remote.rs:1462 concurrent_forge_confirmations_preserve_every_pair`.

## 3. Isolated acceptance (fixed bounds, release-equivalent tree)

Run pre-release on a release binary built from the same `src/` tree that
shipped (only version/changelog differ). Each verdict JSON ships with its
`daemon.log`, `git-events.jsonl`, and `policy.toml`:

- `.../gates-2026-09-18/fairness-default.json` — `passed: true`
  (continuous edits + slow push + pre-start change; `scan_overrun_ms: 3`,
  diagnostic only, never acceptance slack).
- `.../gates-2026-09-18/fairness-failing.json` — `passed: true`
  (failing remote first; healthy repo still converges; covers auditor TODO
  item 1a).
- `.../gates-2026-09-18/fairness-filter.json` — `passed: true`
  (slow required clean filter; covers auditor TODO item 1b).
- `.../gates-2026-09-18/forge-evicted.json` + `forge-uncached.json` —
  both `passed: true` (first dirty dispatch within the fixed 3.0s forge
  budget after restart with evicted/uncached forge cache; covers auditor
  TODO item 1c).
- Timing attribution is same-clock only (`scripts/sync_timing.py` pairs
  same-domain stamps; fixed 1000ms pulse bound; `scan_overrun_ms`/loadavg
  are diagnostics).

## 4. Live observation status (honest: repaired observer, run pending)

- The 0.113.63 observer is acknowledged broken: subject-matched
  (`Round-5`) timings (daemon subjects describe paths → `final.json` null),
  pre-read timestamps (cannot bound completion). Those numbers are
  WITHDRAWN as completion bounds.
- Repaired observer `scripts/observe-live.py` (committed in `324e21d`):
  content-based probe identity (commit whose TREE contains the unique
  marker) and post-read timestamps (valid upper bounds). Smoke-tested live:
  a trial marker committed by the daemon and detected via
  `git log -S` + `git show <rev>:<probe>`.
- The valid 15-minute installed-pipeline run (task 4) is PENDING, blocked by
  box contention, not by the pipeline: at observation time the box sat at
  load ~77 on 16 cores with 0 free RAM (operator chromium fleet + an
  unrelated `npm run release:check` at ~700% CPU), and daemon pulses
  stretched to 4–43s (journal `pulse_start` gaps 02:52:49–02:54:51). Under
  those conditions any live bound measures starvation, not scheduling, so
  the run was deferred rather than recorded as a failure. No live
  faster-than-X claim is made for 0.113.64.
- Self-caused incident recorded: my first release attempt was killed by its
  550s timeout while the maintenance freeze marker was held; the marker
  survived the kill and paused the daemon (~02:46–02:48) until
  `dracon-sync resume` cleared it. No data lost; the daemon resumed
  normally.

## 5. Hegemon / exclusion truthfulness (no concealment)

- No exclusions were added by this goal. The live operator config already
  quarantines the nested hegemon path in `exclude_repos`; that quarantine is
  operator-owned and out of scope to reconcile (history divergence).
- The daemon-side defect (excluded nested submodule reappearing as a phantom
  `healthy`/`EMPTY` row, incl. the stale `/home/dracon/Dev/hegemon` anchor)
  is fixed by item 6 above with regression; the fix suppresses a false row,
  it does not hide a real repo — the quarantine reason remains visible in
  the operator config, and `repair concerns` still surfaces genuinely
  vanished watch paths (e.g. the startup `VANISHED: /home/dracon/Dev/hegemon`
  notice refers to the legacy anchor, which indeed does not exist).
- Stuck-push reporting is truthful in this window: doomtap (5 failures,
  budget exhausted, auto-push paused with a `repair stuck-unstuck` pointer)
  and endless-td (retry scheduled with countdown) are surfaced, not
  displayed as healthy.

## 6. Withdrawn claims (do not cite)

- Any mixed-clock latency (daemon `daemon_ms` minus Unix `unix_ms`,
  e.g. the old "+79ms") — withdrawn, attribution fixed in `sync_timing.py`.
- 0.113.63 `final.json` null timings and pre-read observation stamps —
  withdrawn as bounds (kept only as raw poll logs).
- Strict-filter "passes" recorded under load without same-clock
  decomposition — not cited as acceptance; the fixed-bound isolated verdict
  in §3 is the acceptance signal.
- The old report's Round-5/6 window narrative is retained for history in
  `sync-convergence-investigation-2026-09-17.md` but is not the 0.113.64
  acceptance basis.

## 7. Remaining work

- Task 4: run `scripts/observe-live.py` (≈15 min) once box load permits;
  record `live-observation-0.113.64/live-report.json` + `final.json` with
  per-remote closure and the start/end inventory backlog state.
- Then re-call `complete_goal` against the auditor TODO (items 1a–1c are
  green above; observer repair + valid window close the remainder).
