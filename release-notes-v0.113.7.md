# v0.113.7 — 2026-07-28 — pack-size-concern + auto-mirror retry softening

> **Two distinct fixes shipping together** (both v0.113.7 because
> the auto-mirror goal's code work completed in a previous session
> and the `### v0.113.7` entry in the per-utility CHANGELOG was
> already reserved for it). The packaging preserves both line
> refs and honest framing per the established convention.

## (1) pack-size-concern: silent-skip → ❌ CONCERN

Surfaces the github-push-permanently-skipped class as a visible
CONCERN in the `repos` table. Pre-fix, a repo whose pushable
branch exceeded GitHub's 2 GiB pack limit emitted only a HINT
like `.git exceeds 2 GB (github limit) — may fail to push to
github` while the daemon's push path was silently skipping
GitHub — and the row's STATUS cell stayed at `🔄 ACTIVE`. The
operator had to read journalctl to learn the push was being
skipped. Post-fix:

### Source changes

| Path | Change |
|---|---|
| `dracon-sync/src/report.rs:1693` | NEW helper `pub(crate) fn pack_too_large_forces_concern(pack_too_large: (bool, u64)) -> bool` (pure function) |
| `dracon-sync/src/report.rs:3157` | Production call site: when `pack_too_large.0`, set `concern = true` (routes decision through the helper) |
| `dracon-sync/src/report.rs:2100` | HINT text updated from "may fail to push to github" → "github push is skipped; shrink history or migrate assets to OVH" |
| `dracon-sync/src/report.rs:6417` | Auto-repair no-op: `if pack_too_large` (re-uses the size value computed at line 6391) short-circuits with `⏭️ skipping auto-repair: github push is permanently skipped` log line. **CHANGED 2026-07-28 (v0.113.7, follow-up)**: the initial version checked `flags.contains("PACK_SIZE_WARNING")` — but the `flags` vector at that point was built by `repo_state_flags_with_push_failure`, which doesn't add `PACK_SIZE_WARNING`. That flag is only added in `run_repos_report` at line 3157. The follow-up reuses the inline `pack_too_large` bool. |
| `dracon-sync/src/report.rs:1728` (helper) + `:6495` (call site) | Auto-repair guard extracted to testable helper `pack_too_large_skips_repair(pack_too_large: bool) -> bool`. The helper makes the "fires regardless of other concerns" property testable in isolation — no repo + concern list setup required. The reviewer's leftover observation #4 was: "for a hypothetical repo that ALSO has a CONCERN and ALSO has pack_too_large, the auto-repair would attempt handlers". The new helper's contract: pure bool predicate, fires unconditionally on `pack_too_large=true`. |
| `dracon-sync/src/report.rs:7971` (test module) | NEW test `test_pack_too_large_skips_repair` — 2-case boolean matrix verifying the helper's unconditional short-circuit property |
| `dracon-sync/src/report.rs:6222` | `verify_resolution` post-handler check now also considers `pack_too_large` — without this, a size-only concern would be reported as "resolved" after the auto-repair pass (the concern is actually unchanged). `concerns_resolved_now` now correctly reports `0` for CAG (was `1`). |
| `dracon-sync/src/report.rs:1706` (helper) + `:6277` (call site) | `verify_resolution_still_concern(ahead, behind, has_origin, has_upstream, pack_too_large) -> bool` helper extracted to make the post-check predicate testable in isolation. The helper includes `pack_too_large` in the predicate so a size-only concern stays "still concerned" until the operator actually shrinks the repo. |
| `dracon-sync/src/report.rs:6378` | `run_repair_concerns` now also recognizes pack-too-large as a concern (via `pack_too_large_forces_concern`) — without this, the `repos` table flagged a CONCERN that the `repair concerns` flow would have skipped entirely |
| `dracon-sync/src/report.rs:7882` (test module) | NEW test `test_pack_too_large_forces_concern` — 4-case boolean matrix |
| `dracon-sync/src/report.rs:7988` (test module) | NEW test `test_verify_resolution_still_concern` — 6-case matrix including the size-only case `assert!(verify_resolution_still_concern(0, 0, true, true, true))` |

### Live evidence

`capture-anime-girls` (CAG) — pushable branch 2.37 GiB:

- **Before**: row at `🔄 ACTIVE`, HINT = `.git exceeds 2 GB (github limit) — may fail to push to github`, daemon silently skipping github pushes
- **After**: row at `❌ CONCERN`, HINT = `.git exceeds 2 GB (github limit) — github push is skipped; shrink history or migrate assets to OVH`, auto-repair cycle logs `⏭️ skipping auto-repair: github push is permanently skipped (pushable branch > 2 GiB). Operator action required.`
- **`dracon-sync repair concerns --apply` against CAG**: `concerns_found: 1`, `operations_planned: 0`, `concerns_resolved_now: 0` — the no-op guard short-circuits with the ⏭️ log line BEFORE any handler runs, and `verify_resolution` post-check (routed through `verify_resolution_still_concern`) correctly keeps the size-only concern in the "still concerned" state. Pre-fix the post-check would have reported `concerns_resolved_now: 1` (misleading: the size issue is unchanged and the next repos cycle still shows ❌ CONCERN — the "resolved" tally was per-cycle drift, not real resolution). Post-fix the tally is honest: a size concern cannot be resolved by auto-repair, so it is not counted as resolved.

### Cross-references

- Investigation: `docs/design/cag-github-push-block-2026-07-28.md` (the github-side remediation is still operator's call)
- Design: `docs/design/pack-size-concern-2026-07-28.md`
- Audit context: `AUDIT_FULL_2026-07-26.md` (the HINT-vs-CONCERN classification gap was implicit in the audit's "silent failures" theme)

### Honest framing

This change surfaces a problem; it does NOT fix the github-side
situation. CAG's github mirror is still empty. The fix gives the
operator a CONCERN to react to; the choice of remediation
(orphan github-main cutover / OVH asset migration / filter-repo
shrink) is the operator's call and was explicitly deferred per
the consultation. The auto-repair no-op is conservative — the
daemon cannot fix what it cannot reach, and silently attempting
the repair every cycle is worse than not trying.

## (2) auto-mirror retry softening (from previous session, code work done)

The previous goal's `### v0.113.7` entry in
`dracon-sync/CHANGELOG.md:19` documents this work; the line
refs and test count are reproduced here for completeness.

### Source changes

| Path | Change |
|---|---|
| `dracon-sync/src/report.rs:5119` (was 1e45deb) | `handle_no_origin` now gates `create_private_remote` through `decide_create_mirror` |
| `dracon-sync/src/report.rs:5207` (was e809562) | NEW `pub(crate) fn probe_any_remote_reachable` — 3× retry with 5s delay before declaring origin gone |
| `dracon-sync/src/report.rs:5258` (was 08be865) | NEW gone-since ledger at `<policy_dir>/origin-gone-ledger.tsv` |
| `dracon-sync/src/git/multi_remote.rs:ls_remote_indicates_missing` | Promoted from `fn` → `pub(crate) fn` |
| `dracon-sync/src/report.rs:5217` | NEW `pub(crate) const CREATE_MIRROR_GONE_THRESHOLD_SECS: u64 = 900` (15 min) |

### Tests

- `concerns_retry_softening` — 5+ boolean input combinations
- `concerns_retry_softening_really_gone` — 900-sec threshold boundary
- `concerns_ledger_insert_if_absent` — TSV ledger semantics

All 3 passing (the previous goal's code work is unchanged; this
release just packages the code with the new pack-size-concern
work under a single v0.113.7 tag).

## Test count

1158 → 1161 (3 new from this release):

1. `test_pack_too_large_forces_concern` at `dracon-sync/src/report.rs:7882` — 4-case boolean matrix
2. `test_verify_resolution_still_concern` at `dracon-sync/src/report.rs:7988` — 6-case matrix including the size-only case (`pack_too_large=true` for an otherwise clean/synced repo)
3. `test_pack_too_large_skips_repair` at `dracon-sync/src/report.rs:7971` — 2-case boolean matrix verifying the helper's unconditional short-circuit property

## Gate posture

- `cargo build --release --locked -p dracon-sync`: ✅ clean
- `cargo test --workspace --locked --lib --tests`: ✅ 1161 passed, 9 ignored
- `cargo clippy --workspace --locked -- -D warnings`: ✅ no issues
- `cargo deny check`: ✅ clean (no new dependencies)

## Cross-references

- AGENTS.md: "Commit-all principle (2026-06-16, goal `6205ad1f`)" — unchanged
- AGENTS.md: "History-rewrite ENFORCEMENT stack (v0.113.0, 2026-07-25)" — unchanged (no history rewrite in this release)
- Previous release: `release-notes-v0.113.6.md` (M4 trailing-drain unification)
