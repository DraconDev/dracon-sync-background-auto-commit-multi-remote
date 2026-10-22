## Review

Reviewed supplied `/tmp/sync-fix-20261022/review.diff` (7c46a19 through observed HEAD 4ee4d08 plus source worktree delta), current named source seams, design report, and CLI regression script. No repository mutations or commands executed.

- **Correct — shared finite inspection budget:** `dracon-sync/src/git/inspection.rs:13–49`, `src/daemon.rs:281`, and `src/report.rs:4103` share sixty seconds while retaining owned Git cancellation, filters, untracked enumeration, and thread limits. Report concurrency is bounded at four.
- **Correct — targeted details and truthful errors:** `src/report.rs:4009–4050,4935–4947` select exact-basename candidates before inspection and propagate inspection failures rather than reporting absence. Duplicate candidates remain available for ambiguity detection.
- **Correct — reporting:** `src/report.rs:2853–2859,5040,5083` removes unsupported UNKNOWN convergence promises and distinguishes inspection failures from concerns. `src/report.rs:6450–6460,16494–16502` addresses two-digit FAIL-age column overflow.
- **Correct — lease contention:** `src/sync.rs:5240–5255` returns `OperationDeferred` before commit/push work. `src/daemon.rs:1034–1037,1124–1128` retains remote failure memory and applies a cooldown without consuming failure budget. `src/main.rs:1183–1185` makes no false commit claim.
- **Correct — scope:** the supplied diff does not bypass filters/hooks, reduce remote coverage, or repair repository state. The design report explicitly leaves aud-vis checkout wiring, doomtap policy refusal, SamAI lint refusal, and strategy transfer/retry recovery unresolved.
- **Fixed:** none; read-only review.

### Finding
**P2 — lease-backoff regression coverage is incomplete.**  
`dracon-sync/src/daemon.rs:2427–2443` claims to test preservation of remote backoff but neither seeds nonempty remote-failure maps nor asserts their preservation. Earlier calls pass empty maps. Consequently, removing the new preservation guard at lines 1034–1037 would not fail these added assertions.

**Smallest improvement:** seed `remote_failures` and `mirror_consecutive_fails` with a paused remote before applying `OperationDeferred`; assert both remain unchanged. Also seed a nonzero failure count to exercise failure-memory retention. Current implementation appears correct; this is a test gap, not a demonstrated runtime defect.

## Rehearsal evidence

- `/tmp/sync-fix-20261022/detail-regression.log` records both PASS checks: known inspection failures propagate; unrelated repositories are not inspected.
- Read the fixture script: mutations are confined to a temporary directory.
- `clippy.exit` is `0`; `deny.log` reports advisories, bans, licenses, and sources all OK.
- Full workspace/build completion and deployment were not independently verified. Parent owns final receipts; pending gates/deployment are not code blockers.
- No expensive suites were duplicated. No sync-now, repair, push, service restart, or agent spawning occurred.

## BLOCKERS

None found within the reviewed change. Existing repository/product-policy failures remain operational blockers to those repositories’ convergence, not defects fixed by this patch.

**Merge verdict: OK with notes.**