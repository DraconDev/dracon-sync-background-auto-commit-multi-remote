## Review
No issues found.

- Correct: Native budgets preserve stricter positive caller limits and propagate through existing child environments without changing lifecycle execution or Vitest overrides (`browser-extensions-shared/scripts/git-hooks/run-push-gates.mjs:14–24,92–99,307–319`).
- Correct: Catalogue repair is one import; regression uses mocked fetch, not provider calls (`packages/extension-core/src/byok/catalogue.ts:18`; `tests/byok/catalogue-convergence.test.ts:8–17`).
- Correct: Historical witness selection rejects failed Git queries while retaining ancestor, timestamp, object and PID/event checks (`dracon-sync/scripts/fleet_convergence_witness.py:11–61`). Destination advancement requirements remain intact (`scripts/fleet-convergence-ledger.py:27–44`).
- Correct: Push change modifies diagnostic wording only; regression guards against policy-weakening advice and network misclassification (`dracon-sync/src/git/push.rs:772,1772–1781`).
- Qualification: Retained logs report 19 hook tests and six witness tests passing; not independently rerun. Ledger still marks browser unproven. Future-artifact and whole-publication qualification remain separate—not defects in this slice.

Merge verdict: OK with notes