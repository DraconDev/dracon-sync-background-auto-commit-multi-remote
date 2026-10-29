# Doomtap repeated epoch refusal — 2026-10-29

## Direct observations

- The daemon journal records repeated `max failures (5)` backoffs and late
  failures for Doomtap at 00:07–00:08, 00:23, 00:38–00:39, 00:53–00:54, and
  01:09–01:10 UTC. Filter-aware classification succeeded between retries, so
  this is repeated commit-epoch refusal, not a demonstrated classification
  failure.
- The 01:09:48 refusal retained
  `/home/dracon/Dev/dracon-platform/.git/modules/web-games-doomtap/dracon-epoch-message-qT5Q9f/COMMIT_EDITMSG`.
  The retained generated message identifies one new file:
  `docs/screenshots/qa/probe-unapproved.png`.
- A path-scoped, read-only check found the 1,697,315-byte PNG still present and
  staged as an addition (index blob `bec4a69763041e86453f17d774f80874ca3c2043`).
  No index, worktree, producer or hook state was changed.
- Doomtap's `.gitignore` has an exact allowlist for curated comparison inputs,
  selected regenerable QA caches and explicitly versioned asset pointers. It
  does not list `probe-unapproved.png`; the visual-publication design rejects
  broad screenshot exceptions. The staged path is therefore inconsistent with
  the documented exact allowlist. The hook's stderr was not retained, however,
  and `run_sealed_commit_hooks` retains the message directory for any
  pre-commit/prepare-commit-msg/commit-msg error, so the precise failing hook
  and refusal text remain unverified.
- A narrow source-text search found no `probe-unapproved` reference in the
  Doomtap source tree. That does not prove who created it or whether it is a
  disposable probe. The retained message directories are intentionally
  preserved on hook refusal; the epoch-spool design disables age-only deletion
  because it cannot prove ownership/publication.

## Assessment and safe next step

The evidence supports a path-specific staged blocker, not authorization to
widen the Warden allowlist or remove/unindex the file. The file is preserved
while its status is unresolved. Ask the operator to classify it as (a) a real
QA asset to approve by exact path after review, (b) disposable probe output for
a verified, documented exact-path cleanup, or (c) content to leave blocked.
No retry override, hook bypass, manual commit/push, deletion, or policy change
was made.
