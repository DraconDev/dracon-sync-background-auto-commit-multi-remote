## Review

- **Correct:** Intent publication fsyncs manifest, record directory and registry before actual index installation (`dracon-sync/src/sync/hygiene_intent.rs:241–272`; `staged_epoch.rs:978–1009`). Recovery requires matching parent and exact candidate tree (`hygiene_intent.rs:299–305`). Retries use fresh canonical rendering through the existing lease/exchange helper, not stored whole-file bytes (`hygiene_intent.rs:474–478`; `excluded_revert.rs:178–283`). Production/restart and foreign-staging tests exercise these boundaries (`hygiene_tests.rs:81–219`). Pending and inspection-failure reporting are explicitly non-clean (`current_health.rs:42–58`).

- **Finding: P1 — Retirement accepts a symlink as committed ignore rules.**  
  `dracon-sync/src/sync/hygiene_intent.rs:421–439` checks only whether `ls-tree` returned something; it never verifies `.gitignore`’s mode. A tracked `.gitignore` symlink whose target string is `dist/` supplies blob text matching the required pattern, although Git does not follow symlinked `.gitignore` files. Once recorded artifacts disappear from HEAD, lines 461–472 acknowledge the intent permanently, bypassing the canonical helper’s symlink refusal.  
  **Smallest fix:** Require a regular-file tree entry before evaluating committed rules; otherwise retain the intent diagnostically.  
  **Required regression:** Commit that symlink and a tracked `dist/a`, perform artifact de-indexing/commit, then retry. Intent must remain pending, with symlink and target unchanged.

- **Finding: P1 — Whitespace-normalized text equality falsely proves effective rules.**  
  Retirement uses `render_positive_additions(...).is_none()` (`hygiene_intent.rs:435–439`), whose duplicate detection trims existing lines (`dracon-sync/src/exclude/hygiene.rs:24–32`). Thus committed `" dist/\n"` satisfies required `"dist/"`, despite Git treating the leading space as part of the directory name. After artifact deletion, retry acknowledges an intent without the required ignore rule. Fresh canonical rendering shares this false no-op.  
  **Smallest fix:** Preserve Git-significant leading whitespace when comparing existing rules; do not use generic `trim()` as rule equivalence.  
  **Required regression:** Start with committed `" dist/\n"` and tracked `dist/a`; verify cleanup adds the actual `dist/` rule before retirement and Git ignores a disposable `dist/new` probe.

- **Limitations/source bound:** Read-only source review against goal `20261025090441-ya0tp4.md`; nested `main` read as `e0308aab503de869b07dc7f36ef338cf8be9305b`. Primary source identity recorded in the supplied qualification manifest: `hygiene_intent.rs` SHA-256 `0851fb0310f0901dd65442857390ce21f897391a37e61df8ed60629cd5defa6c`. No supplied committed-range diff was available; this is current-implementation review, not historical commit verification. No tests/builds or reproductions were executed. Supplied gate logs and empty drift artifact establish intermediate qualification only—not installed/runtime or full-fleet acceptance. Broader openly unfinished controls remain unapproved.

- **Merge verdict: BLOCK** pending the two retirement-proof corrections.