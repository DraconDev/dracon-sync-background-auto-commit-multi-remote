## Review
- **Fixed: Prior metadata P1 is resolved.** CAS identity now includes mode and content digest; reused candidates must match mode (`src/git/epoch_spool_cas.rs:165–193`). Original snapshot bytes, size, and mode are rechecked after sharing, before receipt publication (`src/git/epoch_spool_recovery.rs:774–777`; `src/git/epoch_spool_source.rs:124–138`).
- **Correct:** Regression fixture checks identical original index bytes with `0644`/`0600` in both insertion orders, preserved permissions, distinct inodes, and valid receipts (`src/git/epoch_spool_recovery.rs:889–919`).
- **Correct:** Relocated cancellation injection retains the original readiness budgets and assertions covering retained storage, held kernel fence, writer termination, eventual cleanup, and unchanged source (`src/git/epoch_spool_recovery.rs:621–636, 1123–1197`).
- **Correct:** Final deletion still follows receipt verification, ownership/inactivity checks, source-content verification, durable intent, and final no-await source checks (`src/git/epoch_spool_reclaim.rs:131–172`).
- **Evidence:** `/tmp/epoch-mode-fix-tests.log` records **47 passed, 0 failed, 0 ignored**, including the permission and cancellation fixtures. I inspected the log; I did not rerun tests.
- **Scope:** Read-only current-source follow-up, not a committed-range review. No builds or live operations performed.

No issues found.

**Merge verdict: OK with notes** — correction accepted; qualification remains disabled (`src/git/epoch_spool.rs:12`). This does not authorize a drain or qualify historical removals.

## BLOCKERS
None identified in this bounded corrective re-check.