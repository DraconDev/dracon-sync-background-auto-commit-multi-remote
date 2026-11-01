# Stuck-push budget must be per-revision (2026-11-01)

## Demonstrated defect

`browser-extensions-shared` exhausted its 5-failure push budget on commit
`2cf8c70e` and stayed paused while the daemon added **36 more commits**.

The retained gate log showed the real cause was not a product gate:

```
error: Workspace not found "extensions/collections/new-tab/minimalist-new-tab"
      at .../snapshot-2cf8c70e.../package.json
→ snapshot dependency installation exited 1
```

At `2cf8c70e`, `git archive` contained **0** files for those workspace dirs,
while `git archive HEAD` contains **54** and all 12 declared workspaces are
present. The commit was a transient broken intermediate state — the daemon
commits every few seconds, so `package.json` can land before the directories it
references.

The ledger recorded no revision, so it could not tell that the failing tip was
gone. Auto-push only resumed after a manual `repair stuck-unstuck`.

## Fix

* `StuckRepoEntry` gains `failed_tip` (`serde(default)`, so existing ledgers
  load unchanged).
* `record_push_failure` records the tip being pushed and **restarts the series
  at 1 when the tip changed**.
* `stuck_decision` takes the live tip: an exhausted budget is only `Exhausted`
  while the recorded tip still matches. A superseded tip returns `Retry`
  immediately (no backoff earned by a revision that no longer exists), and the
  gate persists the reset so the next failure counts 1.
* A missing recorded tip (legacy entry, unreadable HEAD) is **not** treated as
  superseded — the budget stays in force.

Regression: `exhausted_budget_on_a_superseded_tip_does_not_pause_the_current_one`
covers same-tip (Exhausted), unknown tip (Exhausted), new tip (Retry), and the
legacy no-tip case. **Negative control**: reverting the supersession check made
it fail with `left: Exhausted, right: Retry`.

## Gate status — NOT green, deployment held

* strict Clippy: exit 0
* release build: exit 0
* `cargo deny`: exit 0
* workspace tests: **2 runs, each 1 failure, both different, both green in
  isolation** at load average 54 → 90:
  - `dracon-warden index_batch_descendant_has_no_live_process_after_cleanup`
  - `dracon-sync test_concurrent_staging_defers_then_captured_and_worktree_epochs_converge`

This is the documented load-flake class (retry, never narrow). Deployment is
withheld until one full run is green; no gate was skipped or loosened.
