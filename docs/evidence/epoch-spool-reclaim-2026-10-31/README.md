# Durable epoch recovery — unfinished safety qualification

**Live reclamation is disabled again. This report is not a completion claim.**

The original presence-only prototype and the later legacy-scan attempt are historical activity, not qualified lossless drains. No unexplained disappearance is attributed to this implementation.

## Current safety boundary

The active reclaimer refuses legacy sources without an existing producer-compatible lock and identity-bound settled marker. A newly created reclaimer lock does not bind an older producer; `/proc` observations can miss a writer between operations. That attempted legacy authority has been removed, and `LOSSLESS_RECLAIM_QUALIFIED` is false.

The corrected protocol path preserves the full independent recovery closure and original supported index/metadata before removal. Metadata sharing now distinguishes permissions as well as bytes and rechecks archived bytes/mode after sharing. The published and settled-unpublished actor paths use the same transaction; only disposable unit-test repositories have scoped fixture permission while the production interlock remains disabled.

## Current recorded checks

| Check | Observation | Evidence |
|---|---|---|
| Rebuilt source-matching helper | Exit 0 | [helper build](reclosed-correction/helper-build.txt) |
| Focused spool suite, including host cases | 47 passed, zero failed/ignored | [output](reclosed-correction/spool-tests.txt) |
| Strict workspace Clippy | Exit 0 | [output](reclosed-correction/clippy.txt) |
| Workspace tests | Exit 101: three rich-table display regressions | [full output](reclosed-correction/workspace-tests.log) |
| Release build | Exit 0 | [output](reclosed-correction/release-build.log) |
| Dependency-policy gate | Advisories, bans, licenses and sources passed | [output](reclosed-correction/deny.log) |
| Targeted assessment of display failures | Same three failures reproduced | [output](reclosed-correction/report-gate-assessment.txt) |
| Independent corrective review | Prior metadata-permission finding resolved; interlock remains disabled | [initial review](reclosed-correction/review-initial.md), [follow-up](reclosed-correction/review-correction.md) |

No coherent final all-gates-green claim is made. `src/report.rs` was being changed concurrently during the workspace run; its display failures are not silently waived, reverted or fixed by this spool task. A prior cancellation fixture readiness failure is retained in [failed output](reclosed-correction/reclosed-tests-failed.txt). Its injected writer now starts immediately after its private directory exists, rather than depending on pack-setup speed; original readiness budgets, kernel-fence retention, kill/join and unchanged-source assertions remain.

The earlier [fixture checkpoint](FIXTURE-QUALIFICATION.md) and [mutation demonstrations](corrected-nonvacuity/README.md) describe their recorded source stages. They are not a substitute for renewed final-tree verification after subsequent changes.

## Later legacy attempt — not a qualified drain

A later experimental legacy path combined age, a new reclaimer-only lock and a process-reference scan. That was not a producer-compatible inactivity fence. It was enabled prematurely and used by one-shot commands, then withdrawn. These attempts must not be described as satisfying the revised ownership contract.

The first fleet attempt reached its outer timeout (exit 124). Its [stderr](reclosed-correction/unqualified-live-attempt.stderr) records one preparation timeout and two removal messages; its [stdout](reclosed-correction/unqualified-live-attempt.stdout) contains no completed JSON summary. [Start](reclosed-correction/epoch-drain-start.txt) and [end](reclosed-correction/epoch-drain-end.txt) timestamps are retained. Recovery artifacts are not deletion targets and have not been removed by this correction.

A subsequent human-readable CLI result used the pre-pass inventory count/bytes as though they were reclaimed totals. That reporting bug is fixed: human output now labels before-pass inventory separately and reports the actual verified-removal counter. The earlier text is not evidence that its claimed inventory was removed, nor evidence of net savings.

No attributable **qualified** net-reclamation number is established. Ordinary daemon activity continued concurrently, and missing spools without confirmed provenance remain unexplained. The service was observed active after this correction: [observation](reclosed-correction/daemon-active.txt).

## Inventory limitations and remaining live evidence

The [provisional scan](reclosed-correction/provisional-inventory.json) recorded 330 directories and 837,989,261 logical bytes in its bounded traversal. It is **not an accepted complete fleet baseline**: the scanner omits some linked-worktree metadata and suppresses some filesystem read errors. Do not infer a full-fleet below-8-GiB result from it. A strict, reproducible, complete-scope inventory with recovery-store accounting is still required before any further drain.

The historical **15,236,143,720-byte** figure is the operator-provided inventory in the original objective, not a fresh measurement from this corrected implementation. Earlier [prototype before](baseline-before-drain.txt) and [after](baseline-after-drain.txt) artifacts have their own bounded scopes and are not equated with that figure. The vanished `ai-auto-writer/dracon-staging-2ag7qb` is not a completed live rescue proof.

Remaining work:

- Restore a stable, fully passing workspace gate without interfering with the concurrent report work; renew all qualification against one coherent source state.
- Finish strict before/after spool and recovery-store inventory, with explicit errors and protected/uncertain ownership classifications.
- Identify a genuinely compatible-fenced eligible mixed live source before claiming live rescue. No legacy source may be adopted merely to satisfy this requirement.
- Retain source-bound receipts and isolated readback for any future qualified removal; report recovery-store growth separately from spool shrinkage.
- Preserve active/fresh, marked, legacy-uncertain and failed-proof survivors. No rescue/recovery artifacts, live configs or unrelated data are cleanup targets.
