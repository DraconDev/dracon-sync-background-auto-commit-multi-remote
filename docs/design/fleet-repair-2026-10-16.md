# Fleet repair — 2026-10-16 (in progress)

## Baseline

Six named repositories captured in `evidence/fleet-repair-2026-10-16/baseline.json`.
Operator edits/staging preserved; no history rewrite or guard/hook bypass has
been performed. The 30-minute final observation has NOT started; .dracon is
still blocked and requires explicit permission for unpublished-history repair.

## Repairs and evidence so far

- Freeport: full guard returned `BUCKET_STRATEGY_HIGH_WATER`, not an asset
  deletion violation. Estimated stored reachable bytes + reserve 737,020,949
  exceeded unchanged 711,983,104 budget. There were 7,754 loose objects.
  `dracon-sync maintenance -- git ... repack -a -d` (no pruning, no history
  rewrite) reduced the guard estimate to 475,969,797; full guard exit 0.
  Required hooks then accepted pushes of baseline a919b8f52c0 to GitHub and
  GitLab. GitLab printed a storage warning but accepted the branch update.
- Deathrun: staged deletion of `static/images/edge-ai-screenline.png` was
  intentional (runtime uses WebP; generation moved to assets-source; the
  ownership test requires the served PNG absent). Preserved exact HEAD bytes
  in `assets-source/edge-ai-screenline-served-legacy-2026-10-16.png`, SHA256
  654729885a77f7da845f1cd1e022e129a1f8fded2096522b7cee5b90a87c52f6,
  310,835 bytes. Added only that exact served path to the existing supported
  inventory retiredRoots mechanism, not a blanket exemption. Staged guard
  now passes; 18 forward-only regression tests pass. Daemon committed and
  synced the pending work; repository reported CLEAN, push OK.
- Browser extensions: actual pre-push replay proved four-gate rejection,
  not merely normal generator backlog. Fixed clean-web prefer-const and
  no-empty lint errors. YouTube-filter's reason-label clamp was present in
  source but happy-dom discarded vendor CSS properties in cssText. Preserve
  declarations via setAttribute and test actual style declarations plus
  overflow behavior (no weakened three-line assertion). Concurrent product
  work corrected clean-web cache-confirm tests independently; our exploratory
  resolver change was removed. Full hook replay subsequently passed both
  affected workspaces' typecheck/build/test/lint. Hook-enforced GitHub and
  GitLab pushes succeeded (different fresh tips because product agents keep
  committing); baseline c64f764f4fe is preserved in the pushed history.
- Platform/doomtap: live commit/push churn observed; investigate remaining
  UNKNOWN/lifecycle evidence before final acceptance. No daemon Rust or unit
  changes made in this repair so far.

## .dracon: approval required, no rewrite performed

Both intended remote main refs were checked with ls-remote and still equal
353eb37350040765a837e4ebf06eb10d8e89092f. Local main is
1ca1c920287b8bc35a0ea6da87cf25e99c44ca59, exactly 21 commits ahead.
Commit 94eac1b914 introduces
`backups/dracon-sync.rollback-20261015-pre-covers-deploy`. Warden scans
added blobs in every pending commit. A forward-only deletion/relocation does
not remove that blob from the rejected range, so cannot resolve the refusal.
No secret-scan exemption or bypass is proposed.

Recovery artifacts outside watched roots:
`~/.local/state/dracon/recovery/fleet-20261016/dracon-before-repair.bundle`
(full --all bundle, verified complete) and `dracon-sync.rollback` (SHA256
bcedd8b63a402ba7126020f93148b963be1a92f6eef30057fc119edb26594244).
The original rollback file remains in place pending approval.

Proposed procedure, ONLY after separate explicit approval:
1. Sanctioned maintenance; recheck both remote tips unchanged and ancestry.
2. Preserve unstaged events.jsonl and repos-seen-ledger.json edits securely.
3. Rebuild only the unpublished 21-commit range above the common published
   ancestor, removing only the rollback binary path from each tree; preserve
   other tree content and commit metadata, record an old-to-new commit map.
4. Keep the rollback binary outside the watched root, update deployment
   rollback instructions, restore all operator edits; do not recreate the
   watched binary in the recovered worktree.
5. Verify all tree differences are restricted to that one path, published
   history unchanged, replay normal hooks, push FAST-FORWARD to both remotes
   (no force push needed because the common published ancestor stays intact).
6. Resume and run the full 30-minute convergence observation.

Verification-contract consequence: original unpublished .dracon commit IDs
cannot themselves reach remotes while also removing the rejected blob.
Approval must allow mapped replacement commits for that repository only;
all other baseline commits must reach intended remotes unchanged.
