# dracon-sync v0.113.5 — 2026-07-27

MEDIUM-finding remediation batch from `AUDIT_FULL_2026-07-26.md`. All
4 still-open SYNC MEDIUMs closed in a single release.

## Highlights

- **M1 — `detached_discard` keyed per-task-generation, not per-repo**
  (`daemon.rs:4176-4196`, helper `should_discard_stale_detached_result`
  at line 65). The pre-fix `HashSet<PathBuf>` discarded whichever
  future result arrived first for the repo, inverting the outcome
  depending on completion order. The post-fix `HashMap<PathBuf, u64>`
  stores the wedged generation; only a result whose generation
  matches the wedged generation is stale enough to drop. A
  re-dispatched fresh task with a NEWER generation is NOT discarded;
  its result correctly applies the repo state. `SyncTrioJoin` tuple
  extended from 3 → 4 elements (added `u64` generation); per-repo
  `dispatch_gen` counter bumps on every dispatch. New regression test
  `test_m1_discard_matches_only_corresponding_generation` (5 cases)
  fails on pre-fix logic, passes post-fix.
- **M2 — filter-only early return no longer drops injected
  stale-gitlink entries** (`sync.rs:4216` plus helper
  `should_short_circuit_filter_only` at `sync.rs:3989`). Pre-fix, the
  filter-only early return checked `filter_only_cleared` alone and
  skipped the rest of the apply phase, dropping any parent-gitlink
  entries the gitlink-injection step had just queued. Post-fix, the
  short-circuit also requires `stale_gitlink_injected == false`; if
  the gitlink-injection step ran, the rest of the apply phase
  continues and the parent gitlinks converge. New regression test
  `test_m2_filter_only_bypass_decision` (4-case boolean matrix) pins
  the contract.
- **M3 — FilterOnly `handle_ahead_push` no longer flips benign repo
  to PushFailed / stuck-ledger exhaustion** (`sync.rs:4214`). Removed
  the `|| !branch_has_upstream` clause from `should_push`. Pre-fix,
  that clause made `should_push=true` forever for mirror-only repos
  with no upstream configured; every 300s stage cooldown cycle issued
  a real push attempt to a forge that might be flaky, and any
  transient remote failure was written to the stuck ledger by
  `record_push_failure`. With enough failures the repo flipped to
  `StuckDecision::Exhausted` and the desktop alarm fired for a repo
  that was completely benign. The fleet observed this concretely on
  2026-07-27 as a 73-minute browser-extensions-shared stall after a
  transient ssh hiccup. Post-fix, `should_push = ahead > 0 ||
  upstream_ref_missing`: push only when there is positive evidence
  of unpushed work (the v0.112.30 bootstrap-push behavior is
  preserved via the `upstream_ref_missing` arm). New regression test
  `test_m3_mirror_only_no_unwanted_push` fails pre-fix, passes
  post-fix. The 3 pre-existing mirror tests that relied on the
  removed clause now use a new `configure_branch_upstream` test
  helper to set `branch.master.remote=origin` +
  `branch.master.merge=refs/heads/master` (the same state `git push
  -u` would leave behind) so the bootstrap-push behavior is
  preserved exactly.
- **M4 — main apply phase vs trailing-drain symmetry**
  (`daemon.rs:95` for the `ApplyOutcome` enum, `daemon.rs:124`
  for the `pub(crate) fn apply_outcome` free function, call sites
  at `daemon.rs:4261` and `daemon.rs:4424`). Pre-fix, the main
  apply phase and the trailing-drain path each had their own
  `match sync_res { ... }` block — nearly identical, but with two
  divergence bugs the audit caught. First, trailing-drain
  `NothingToDo` did nothing (no activity.remove / failure_count
  reset, leaking entries across cycles). Second, trailing-drain
  `Synced` did not call `stuck_push_repos.remove + save` (ledger
  would stay stale until a main-phase success). Post-fix, both
  phases route through the single `apply_outcome` function; the
  `is_late: bool` parameter toggles the log suffix only — outcome
  classification and side effects are structurally identical.
  `RepoActivity` was promoted to `pub(crate)` so the helper can
  take `&mut RepoActivity` (still crate-private, not exposed to
  downstream crates). New regression test
  `test_m4_helper_structurally_unified` drives each `SyncOutcome`
  variant through the helper and pins the
  `ApplyOutcome::Success / Blocked / BackstopSkipped / Failure`
  classification matrix plus the side-effect contracts
  (`blocked_since` set on `Blocked`, cleared on `Failure` or
  `PushFailed`; `stage_cooldowns` entry inserted on `FilterOnly`,
  `Blocked`, and `BackstopSkipped`).

## Test / gate posture

- `cargo test --workspace --locked` — **852 passed**, 3 ignored (was 837
  before this batch — added 5 new regression tests: M1
  discard-matches-only-corresponding-generation, M2 filter-only-
  bypass-decision, M3 mirror-only-no-unwanted-push, M3 configured-
  upstream-missing-tracking-ref-still-pushes, M4 helper-structurally-
  unified).
- `cargo clippy --workspace --locked --all-targets -- -D warnings` —
  green. The pre-existing baseline clippy debt accumulated across
  the v0.112.20→v0.113.4 line was also closed: 14 unrelated
  `int_plus_one`, `bool_assert_comparison`, `cmp_owned`,
  `useless_conversion`, `unused_variables`, `useless_vec`,
  `unnecessary_get_then_check`, and `cloned_ref_to_slice_refs`
  warnings fixed at the same time. `cargo clippy ... -- -D warnings`
  is now clean at the workspace root again.
- `cargo build --release --locked` — clean.

## Audit cross-references

- `AUDIT_FULL_2026-07-26.md` § SYNC MEDIUMs (M1-M4)
- `.pi-tmp/audit-2026-07-26-part1-daemon-sync.md` (M1-M4 + L1-L5)
- `.pi-tmp/audit-2026-07-26-part3-report-policy.md` (M1-L8)
- `docs/design/filteronly-push-starvation-2026-07-26.md` (M3 design)

## Operator notes

- No new configuration fields. No daemon restart required beyond the
  binary swap. The daemon picks up the new behavior on the next
  dispatched task after `dracon-sync` is restarted.
- Fleet state pre-release: 36 repos · ✅ CLEAN 31 · 🔄 ACTIVE 5 ·
  🟡 WARN 0 · ❌ CONCERN 0. Post-release verification expected to
  show the same shape (M3 specifically was observed live on
  browser-extensions-shared; that repo should now stay stable
  across transient ssh hiccups).