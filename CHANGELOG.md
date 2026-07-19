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

### v0.112.22 — 2026-07-19 — MEDIUM-sweep follow-up

5 MEDIUM + 2 LOW deferred from v0.112.21, now remediated:

- **F31** `git/staging.rs`: `rewrite_ahead_paths` now compares
  `backup_branch^{tree}` vs `HEAD^{tree}` after the rewrite and
  deletes the empty backup branch on a no-op. Test:
  test_f31_noop_rewrite_deletes_backup_branch.
- **F33** `git/diff.rs`: `parse_name_status_line` now requires a
  digit suffix on rename (`R100`, not bare `R`). 7 new tests cover
  the matrix.
- **F34** `main.rs`: `dual-branch-repair` defaults to DRY-RUN; pass
  `--apply` to actually delete master locally + remotely.
- **F47** `git/ops.rs`: `kill_process_group` SIGTERM→SIGKILL gap
  extended from 200ms to 2s with `kill`-missing diagnostic.
- **F49** `git/ops.rs`: child-wait poll interval 250ms → 100ms (the
  `tokio::select!` was already event-driven via `progress_rx`).
- **F55** `role.rs`: `classify_roles` now prefers full-path equality
  over basename-only. Test: f55_full_path_distinguishes_same_basename_repos.
- **F60** `secrets.rs`: `check_secrets_dir_permissions` refuses
  group-writable (was world-writable only).
- **F61** `test_helpers.rs`: corrected doc-comment that falsely
  claimed `test_git_cmd()` serializes git invocations.

Test count: 915 (was 906, +9 new). `cargo build/test/clippy/deny` all green.

### v0.112.21 — 2026-07-19 — post-v0.112.20 audit remediation

8 daemon HIGH + 3 warden HIGH findings remediated from `AUDIT_FULL_2026-07-18-POSTFIX.md`. Critical changes:

- **F30 — Full table layout constraint sum 345 → 299 cols**: the v0.112.19 fix was incomplete because the test array had 22 entries but production had 23 (ROLE was added but never propagated to the test). At terminal width 300, the v0.112.19 table letter-wrapped ROLE and PUSH-TO columns. This release: (1) trims ROLE 35→18, PUSH-TO 32→22, LAST COMMIT 22→17, ACTIVITY 17→11, DAEMON 17→15, HINT 22→15; (2) updates the test array to match production (now 23 entries summing to 275+24=299); (3) replaces the stale "Sum: 268/Plus 23 borders: 291" comment with the actual values. New floor 299 cols fits any 300+ terminal.

- **F39 — ownership substring bypass** ([ownership.rs:267](src/ownership.rs)): `is_trusted_origin("https://github.com/DraconDev.evil.com/x.git", ...)` matched the trusted entry `"github.com/DraconDev"`. New `parse_origin()` extracts `(host, first_path_segment)` atomically and the matcher requires tuple equality, not substring. Also `redact_origin_credentials()` strips `user:password@` from URLs before logging.

- **F40 — `standard_files` path traversal** (policy.rs: validate_config): rejects absolute `target` paths, `..` components, Windows-prefix paths, and absolute `source` paths. A config typo `{target = "/etc/cron.daily/evil"}` is now an error rather than a write-anywhere primitive.

- **F41 — `git_askpass_script` token leak** ([git/ops.rs:263](src/git/ops.rs)): file is now created with `O_EXCL | O_NOFOLLOW` and mode `0o700` atomically (no world-readable race between write and chmod). New `AskpassScript` Drop guard for RAII cleanup. Tokens containing `'` (F59) are refused outright.

- **F42 — nix.rs comment clobber** ([nix.rs:65](src/nix.rs)): `update_version_in_flake_nix` now skips `version = "..."` lines that begin with `#`.

- **F43 — TOML trailing `;`** ([bump.rs:16](src/bump.rs)): `extract_version_from_cargo` strips a trailing `;` before the closing-`"` check.

- **F44 — classify step 3 OR-of-untrusted** ([ownership.rs:185](src/ownership.rs)): now flags Unowned if EITHER email OR name is untrusted. Previous logic was too lax: a single trusted value bypassed the check.

- **F45 — mem::forget TempDir leak** ([test_helpers.rs:67](src/test_helpers.rs)): temp dirs are now registered in a global `TEST_TEMPS` Vec and reaped at process exit, instead of being permanently stranded.

- **F46 — EnvRestorer Drop UB** ([test_helpers.rs:222](src/test_helpers.rs)): documented the racy `set_var` during unwinding; relies on `--test-threads=1` discipline in `.cargo/config.toml`.

- **F32/F48/F50/F51/F52/F53/F54** MEDIUMs (selected): `restore_paths` now validates paths; `is_git_push_progress_line` switched to a regex (substring `delta`/`bytes` no longer extend the deadline on error messages); stderr-task `Err` is now surfaced instead of silently dropped; `extract_version_from_json` uses `serde_json` (handles escaped quotes); `load_secret` refuses env values with control characters; SSH `ssh://host:port` URLs now parse correctly; logged origins redact `user:password@`.

Test count: 906 (was 890, +16 new regression tests). `cargo build/test/clippy/deny` all green.

### v0.112.19 — 2026-07-18 — `repos` table fix for narrow terminals

The `dracon-sync repos` output renders a 22-column v1 Full table (~620 chars wide) at any terminal width where `terminal_size()` cannot determine the width (piped, scripted, agent-captured output). At 80-col wezterm ptys with redirected stdout (e.g. `script -q -c '...'`, piped logs, agent stdout capture), the result is 600+-char rows that wrap mid-cell, misaligning header/separator/data rows and producing visually broken tables. This was observed live by the operator on 2026-07-18 against 31 watched repos.

**Fix:** change the non-TTY fallback width from `Some(300)` (Full) to `Some(120)` (Compact-friendly) in `report.rs::terminal_width()`. Add `COLUMNS` env var support as a fallback after `DRACON_SYNC_TERM_WIDTH` (ncurses convention). Raise the Compact-tier threshold from `< 250` to `< 300` because the 15-column Compact layout's `LowerBoundary` constraints sum to ~215 cols minimum; comfy-table's `Dynamic` arrangement letter-wraps cell content (e.g. `PUSH` / `PENDING` on separate lines, `STATUS` header → `STA` / `TUS`) when the available width is below the sum of minimums. Routing 120–219 cols to Vertical instead avoids the letter-wrap artifact entirely.

**New CLI flag: `--layout <vertical|compact|full>`.** Bypasses terminal-width detection and forces the requested tier. Useful when piping to a file (where `terminal_size()` returns None and the fallback picks Compact) but the operator actually wants Vertical or Full. Emits a warning and falls back to auto-detection for unknown values; clap rejects invalid values up front.

**`comfy_table::Table::set_width(w)` applied to Compact and Full tables.** Forces the table to fit the actual terminal width; columns shrink to fit and cell content is truncated (with `…`) instead of letter-wrapped. Combined with the new tier thresholds, this means:

| Width | Tier | Max line length | Notes |
|---|---|---|---|
| 80 | Vertical | 86 | one repo per multi-line block |
| 120 | Vertical | 116 | (was 553, now readable) |
| 220 | Compact | 231 | (was 553, now readable) |
| 300 | Full | 346 | (was 616, now readable) |
| 400 | Full | 400 | (was 620, now readable) |

**3 new tests** (890 total, up from 887): `test_terminal_width_columns_env_var`, `test_terminal_width_fallback_is_compact`, `test_choose_layout_tier_fallback_no_env_no_tty_yields_compact_or_smaller`. Updated existing tier tests to match the new threshold (`< 220` → Vertical, `220-299` → Compact, `≥ 300` → Full). `cargo build --release --locked`, `cargo test --workspace --locked`, `cargo clippy --workspace --locked --all-targets -- -D warnings`, `cargo deny check` all clean.

**Design doc:** `docs/design/repos-table-fix-2026-07-18.md` — root cause, threshold rationale, before/after pty captures at 80/120/220/300/400 cols.

## [Unreleased]

### v0.112.20 — 2026-07-18 — `dracon-git` v94.7.1 patch (libgit2 ssh-agent fix)

The 2 CONCERNs surfaced by `dracon-sync repos` on 2026-07-18 (`endless-td` 53-ahead push-stuck with 35 consecutive failures, `neonbreak` 4-minute PENDING with 6 ahead / 4 behind) were caused by a libgit2 fetch bug in the external `dracon-git` crate v94.7.0. The daemon's `fetch()` function used `git2::Cred::ssh_key_from_agent`, which requires a running ssh-agent — the operator's wezterm/NixOS session has no ssh-agent (only a wezterm socket at `/run/user/1000/wezterm/agent.25368`), so every libgit2 fetch failed with `unsupported URL protocol; class=Net (12)`.

This release **doesn't change any daemon source code**. Instead, it patches the workspace `Cargo.toml` to use a locally-built `dracon-git v94.7.1` (from `DraconDev/dracon-libs`) where `fetch()` is rewritten: **CLI primary path** (`std::process::Command("git fetch origin")` which respects `~/.ssh/config` and the `IdentitiesOnly yes` + `IdentityFile ~/.ssh/id_ed25519` pattern that std::process ssh reads) **+ libgit2 fallback** (the original `Cred::ssh_key_from_agent` code) for repos where the CLI path fails (binary blob edge cases).

The phantom MERGE_HEAD state (a side effect of the failed libgit2 fetch leaving `MERGE_HEAD` and `MERGE_MSG` files in the working tree's gitdir) was resolved automatically once `git fetch` started working and updated the remote tracking refs. No daemon-side handling needed.

**Operator's manual intervention for endless-td:** chose reset+replay strategy (per `ask_user_question`): saved 3 untracked files, `git merge --abort`, `git reset --hard origin/main`, `git cherry-pick` of the 57 local-only commits, resolved 2 conflicts on `TASKLIST_FIXES.md` by taking "theirs" (the cherry-picked version, which is the correct new state). Result: 0 ahead / 0 behind, all 3 remotes at HEAD `16720ca7`.

**Operator's manual intervention for neonbreak:** none — auto-recovered once `git fetch origin` updated the remote tracking ref.

**Endless-td CONCERN resolution** (Cherry-pick: 57 commits replayed, 2 TASKLIST_FIXES.md conflicts auto-resolved by taking theirs, push to github + gitlab + origin all succeeded, ~6 seconds each).

**1 new test** in `dracon-git` (33 total, up from 32): `test_fetch_uses_cli_path_successfully` — verifies `fetch()` succeeds against a local bare remote (no ssh involved), confirming the CLI primary path works end-to-end.

**Live verification**: 890 tests pass, clippy clean, deny clean. Tally: `📦 32 repos · ✅ CLEAN 28 · 🔄 ACTIVE 4 · ⚠️ WARN 0 · ❌ CONCERN 0`. Both endless-td and neonbreak ✅ CLEAN (0/0 ahead/behind, healthy daemon state). The 32nd repo is `dracon-libs` itself (auto-discovered after the clone).

**Workspace `Cargo.toml` patch**:
```toml
[patch.crates-io]
dracon-git = { path = "/home/dracon/Dev/dracon-libs/tools/sync/dracon-git" }
```

This patch should be removed once `dracon-git v94.7.1` is published to crates.io (requires operator's `CARGO_REGISTRY_TOKEN`).

**Design doc**: `docs/design/concerns-investigation-2026-07-18.md` (14.7 KiB). **Release notes**: `release-notes-v0.112.20.md`. **AUDIT update pending**: `AUDIT_FULL_2026-07-18.md` §F5.

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

### v0.112.16 — 2026-07-17 — Codeberg public-only policy

The structural problem: codeberg has an 85 GiB global quota across
ALL private repos in an account, while github and gitlab use
per-repo limits with no global cap. On 2026-07-17, all codeberg
pushes were failing with `remote: Forgejo: Quota exceeded` even
though github and gitlab pushes succeeded for every repo. This
release implements the operator's strategic decision: use codeberg
as a curated marketing surface for public repos only.

**New policy field: `codeberg_public_only` (default `true`).**
The daemon now reads the cached GitHub visibility state (populated
by the existing `sync_mirror_visibility` cycle, 24h interval by
default) and automatically excludes the codeberg remote when a
repo is private. Public repos are unaffected. The safe-default
path (skip codeberg) fires when no cache exists yet, so private
work is never accidentally pushed to codeberg before the first
visibility sync.

**Per-repo override:**
```toml
# <repo>/.dracon/dracon-sync.toml
codeberg_public_only = false   # force codeberg push for this private repo
```

**Visibility cache file format change** (backward-compatible):
old `timestamp-only` files still pass freshness checks but surface
as `None` (unknown) so the safe-default skip fires until the next
sync rewrites them in the new `visibility=<public|private>\n<ts>`
format.

**`repos` output change:** the PUSH-TO column annotates the
policy-driven exclusion with the visibility reason:
`github,gitlab [excl:codeberg] (private)` (yellow). Manual
`exclude_remotes = ["codeberg"]` overrides are unchanged.

**24 new tests** (701 total). Design doc:
`docs/design/codeberg-public-only-policy-2026-07-17.md` (13.6 KiB).

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
