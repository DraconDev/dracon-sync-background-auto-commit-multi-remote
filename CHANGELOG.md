# Changelog

All notable changes to `dracon-sync` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Note**: prior to 0.112.12, `dracon-sync` was developed inside the
> [`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
> monorepo. Releases 0.0.0–0.112.11 are recorded in
> [`dracon-utilities/CHANGELOG.md`](https://github.com/DraconDev/dracon-utilities/blob/main/CHANGELOG.md)
> under the `dracon-sync` heading. From 0.112.12 onward, this CHANGELOG
> is the canonical record.

## [Unreleased]

### Added
- **Codeberg quota leak fix (`default_untracked_exclude_patterns`):**
  added 9 DIR-level patterns (`**/.pi/**`, `**/test-results/**`,
  `**/verify-screenshots/**`, `**/__screenshots__/**`,
  `**/.state-recon/**`, `**/chrome-screenshots/**`,
  `**/chrome-*/**`, `**/sign-in-flash-audit/**`, `**/~/**`) that
  catch the unambiguous collection directories identified by the
  2026-07-13 codeberg audit. Forward-compatible: any future agent
  tool using one of these names is auto-excluded from auto-stage.
  Empirical verification: 17 watched repos scanned, no false
  positives on intentional content like 1mg marketing screenshots,
  audit REPORTS (`docs/audit-*.md`), audit SCRIPTS
  (`scripts/audit-*.mjs`), or intentional game art. See
  `docs/design/codeberg-quota-leak-fix-2026-07-13.md`.

- **`scan-bloat` subcommand:** new `dracon-sync scan-bloat` that
  walks every watched repo, finds untracked collection directories
  not yet covered by `untracked_exclude_patterns`, aggregates
  them by leaf name across repos, and emits a sorted-by-size
  report with a suggested glob per bucket (e.g.
  `**/dracon-sync/**` for the per-crate build-artifact leak the
  audit found). The operator's manual review loop for forward
  compatibility — new tools using novel directory names will
  surface here instead of silently accumulating. Flags:
  `--min-size-mib <N>` (default 5) and `--min-repo-count <N>`
  (default 2), plus `--json` for machine-readable output. See
  `docs/design/codeberg-quota-leak-fix-2026-07-13.md`.

## [0.112.14] - 2026-06-22

### Fixed
- **`.pi/` recursion-skip bug**: the daemon's `stage_existing_files`
  recursion had a broad `name.starts_with('.')` skip that was meant
  to skip `.git/`, `.cache/`, `.venv/`, etc., but it ALSO skipped
  `.pi/` — silently blocking `*/.pi/goals/archived/*.md` from
  being auto-staged. These are operator docs (pi-goal tracking
  records) that the commit-all principle says MUST go up. The
  fix removes the dotfile-skip entirely; the dot-dirs we want to
  skip (`.cache`, `.direnv`, `.venv`) are already in the
  `excluded` BTreeSet, and `.git/` is handled by a separate
  `full_dot_git.is_file()` check. Adds regression test
  `test_stage_existing_files_recurses_into_pi_dir`.

## [0.112.13] - 2026-06-21

### Added
- **`auto_resolve_unmerged` policy field** (default `true`): when the
  daemon's commit cycle is about to fail on an unmerged index, it now
  lists unmerged paths via `git ls-files --unmerged`, compares each
  working-tree file byte-for-byte to `git show HEAD:<path>`, and runs
  `git reset HEAD -- <path>` to clear the unmerge when the bytes match
  (the user has the HEAD content already; we're just clearing git's
  bookkeeping). When the working tree differs from HEAD, the path is
  left alone (the user has unmerged work in progress that the daemon
  must not touch).
- **`push_debounce_secs` policy field** (default `30s`): reduces push
  churn. The daemon still commits as soon as a batch is ready, but it
  coalesces pushes within the debounce window so a burst of small
  commits becomes one push per remote.
- **`untracked_warn_threshold` policy field** (default `500`): emits a
  `⚠️ untracked count exceeded threshold: <N>` log line when the
  untracked count exceeds the threshold. Set to `0` to disable.

### Fixed
- **4+ hour daemon stall when a watched repo has unmerged index
  entries** (`web/ai-hub/audit-20260629/...` on `dracon-platform`).
  The daemon's `git add -A` would fail with `cannot create a tree from
  a not fully merged index`, the entire batch (444+ files) was
  discarded, and the loop retried every 10s without making progress.
  The new `auto_resolve_unmerged` step (above) prevents this by
  clearing safe unmerged entries before the staging step.

### Verified
- 597 unit tests pass (587 existing + 8 new + 2 modified)
- `cargo build --release --locked` succeeds
- `cargo deny check` is clean
- Live verification on `dracon-platform` (the worst case): unmerged
  cleared in 19s, 293+ untracked files drained in 90s, all 4 remotes
  at 0/0 within 3 min
- No regression in 11 other watched repos (auto-resolve is a no-op
  when the index is clean)

### Backwards compatibility
- All 3 new policy fields have `#[serde(default = ...)]`, so existing
  `dracon-sync.toml` policy files load unchanged
- The new defaults match the operator's commit-all policy:
  `auto_resolve_unmerged=true`, `push_debounce_secs=30`,
  `untracked_warn_threshold=500`

## [0.112.12] - 2026-06-21

### Changed
- **Standalone repo**: `dracon-sync` is now a first-class standalone git
  repository at
  [`DraconDev/dracon-sync-background-auto-commit-multi-remote`](https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote).
  Previously this code lived in
  [`DraconDev/dracon-utilities`](https://github.com/DraconDev/dracon-utilities)
  as a workspace member. Source-of-truth has moved to the standalone repo;
  future releases are cut from there via `scripts/release.sh`.
- **`scripts/release.sh`**: new per-repo release script. Same interface as
  the parent monorepo's `release.sh` (`<version> --yes [--dry-run] [--abort]`),
  scoped to the standalone repo's Cargo.toml, CHANGELOG, crates.io publish,
  and GitHub release. Each utility now releases independently on its own
  cadence.
- **Push-protected remotes**: the verbose repo name
  (`dracon-sync-background-auto-commit-multi-remote`) is the public-facing
  identity. Local directory is `dracon-sync/` for ergonomics. The 4-keyword
  description in the repo metadata ("background, auto-commit, multi-remote")
  is the canonical public description.

### Verified
- `cargo info dracon-sync` confirms version 0.112.12 on crates.io
- `gh release view v0.112.12` (verbose repo) shows the github release
- Daemon's `dracon-sync repos` continues to see this repo and pushes to
  the 3 remotes (github + gitlab + codeberg) on its own cycle

[Unreleased]: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/compare/v0.112.12...HEAD
[0.112.12]: https://github.com/DraconDev/dracon-sync-background-auto-commit-multi-remote/releases/tag/v0.112.12
