## dracon-sync v0.113.14 — WARN flag reads the classified dirty counts

Hotfix on top of v0.113.13's exclusion-aware dirty semantics.

### Fixed

- **False WARN on excluded-only dirt, for real this time.** v0.113.13
  applied the exclusion-aware classification to the ACTIVITY label
  (`synced · 1 excl`) and the state flags, but the STATUS WARN flag in
  the main `repos` row pass still read the *raw* dirty counts — so a
  repo whose only dirt is policy-excluded (junk-runner's
  `.pi-glla/active.jsonl`) showed `synced` in ACTIVITY while STATUS
  stayed 🟡 WARN. One-line wiring fix: `real_is_dirty` now reads
  `effective_status`. Verified live against the original repro:
  junk-runner back to `✅ CLEAN synced · 1 excl`, fleet header WARN 0.

Upgrade: `cargo install dracon-sync --locked` or your usual update path.
