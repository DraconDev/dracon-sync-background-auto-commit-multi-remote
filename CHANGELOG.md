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

### Changed (operator table feedback, 2026-07-29)

- **REM column shows ACTIVE push remotes only** — excluded remotes
  are no longer rendered dim (operator: "we are showing github gitlab
  and codeberg for all, that is almost certainly wrong"). The dim
  styling was invisible in pastes and read as "all repos have all
  three remotes". Now `🐙🦊` = github+gitlab only; exclusion detail
  lives in `repos <name>` / the JSON row.
- **CHANGES column split out of ACTIVITY** (operator: "the activity
  can just have the first part and anything excluded or modified or
  changed or waiting to commit can be its own column"). ACTIVITY now
  holds only the state label (`⏳ dirty 0m` / `🟢 synced 19m` /
  `⚪ idle 6h` / `⚫ cold 1d`); the counts (`1 mod`, `1 mod 1 excl`)
  render in their own column, `—` when clean. ACTIVITY_COL 23 → 16,
  CHANGES_COL 14, rich-tier floor still ≤ 165.
- **🔓 public marker** joins 🔒: the REPO cell now shows BOTH
  visibility states from the github visibility cache (operator: "we
  need to show public and private"). Unknown/unprobed repos still get
  no marker.

## [0.113.16] - 2026-07-29

### Fixed

- **REM column lied about codeberg for quota-postured repos**
  (operator live-table spot, 2026-07-29): the report's
  push-to/excluded computation applied only the codeberg-public-only
  visibility gate, missing the daemon's v0.112.28 quota-posture rule
  (`codeberg_push_excluded` — codeberg skipped at push time when the
  repo has no codeberg tracking ref AND effective auto-create is
  off). convos, dracon-libs, practice-form and DraconDev showed a
  BRIGHT 🗻 while the daemon deliberately skipped codeberg — a
  silent push-gap lie. New `report_effective_remotes` helper computes
  the FULL daemon-equivalent filter once and drives both
  `push_to_remotes` and `excluded_remotes`; `codeberg_skip_reason`
  gains the `"quota"` variant so the compact/text renderers can
  distinguish quota skips from visibility skips.

### Added

- **🔒 private-repo marker in the REPO cell** (operator request):
  repos whose github visibility cache entry says private render as
  `name 🔒` (3 cells carved from the truncate budget; unknown/unprobed
  repos get no marker — a false 🔓 would be worse than none). Legend
  gained the REPO line explaining both the ⚡branch fold and 🔒.

## [0.113.15] - 2026-07-29

### Added

- **REM column in the rich `repos` table** (operator request,
  2026-07-29): one icon per configured push remote — 🐙 github,
  🦊 gitlab, 🗻 codeberg — bright when the daemon pushes there,
  dimmed when excluded from auto-push (e.g. junk-runner's
  policy-excluded codeberg). Unknown remote names render as their
  first two letters rather than being dropped. Width funded by
  narrowing REPO 22→20, ACTIVITY 28→23, TOUCHED 16→15 (still 165-col
  budget). Requires comfy-table's `custom_styling` feature (now
  enabled) so the per-icon embedded ANSI is width-safe. Legend gained
  the REM explainer line. NOTE: codeberg is 🗻 (U+1F5FB) not ⛰/🏔 —
  those measure width-1 in unicode-width but render 2, which would
  break the table math.
- **Last-push age in the PUSH cell** (operator request): a successful
  push cell now reads `✅ OK 5m` / `✅ OK 3h` (git `%cr` relative time
  parsed + shortened via the ACTIVITY age pipeline). PENDING/FAIL
  cells unchanged.

## [0.113.14] - 2026-07-29

### Fixed

- **WARN flag still read the RAW dirty counts** (operator live report,
  2026-07-29): v0.113.13 applied exclusion-aware classification to the
  ACTIVITY label and status flags but the `warn` computation in the
  main `repos` row pass still read the unclassified `status` — a repo
  whose only dirt is policy-excluded (junk-runner's
  `.pi-glla/active.jsonl`) showed `synced · 1 excl` in ACTIVITY while
  STATUS stayed 🟡 WARN, re-creating the exact false-WARN class the
  release was shipped to kill. `real_is_dirty` now reads
  `effective_status`. Verified live: junk-runner back to
  `✅ CLEAN synced · 1 excl`, fleet header WARN 0.

## [0.113.13] - 2026-07-29

### Fixed

- **False WARN on excluded-only dirt** (goal-list item, 2026-07-29):
  the report's dirty counts came from raw dracon-git status, which
  includes files the daemon will never commit
  (`auto_commit_exclude_patterns` — e.g. junk-runner's 15 MiB
  append-only `.pi-glla/active.jsonl` — and submodule-worktree-only
  gitlink dirt). Both looked like permanent stalls: junk-runner showed
  `⏳ dirty 2h` + 🟡 WARN forever, and dracon-platform inherited a
  second false WARN through the junk-runner submodule entry. The report
  now re-derives dirty counts from `git status --porcelain -z
  --ignore-submodules=dirty` for tracked-dirty repos only, classified
  with the same patterns the sync loop stages by. Excluded entries show
  as a `· N excl` ACTIVITY marker ("dirty by policy, visible, not
  alarming") and CANNOT drive the dirty-clock or WARN; gitlink SHA
  drift still counts (the daemon advances gitlinks). Fast: no
  clean-filter pass, and clean repos pay nothing.

### Changed

- **`dracon-sync repos` table v2**: the USED column was dropped
  (operator feedback: it duplicated ACTIVITY's tiers) and the single
  `N/N/N` COMMITS cell was split into dedicated **1H / 6H / 24H**
  columns (bright = active window, grey = zero). `used_label` and
  `commits_window_label` removed; legend updated to match the shipped
  columns; header-fit / narrow-terminal / legend tests updated.

## [0.113.12] - 2026-07-29

### Changed

- **`dracon-sync repos` prints its legend under every table by
  default** (goal-list item, 2026-07-29). The legend text was rewritten
  to match the columns that actually ship in the v0.113.8 rich table
  (the 2026-07-08 text referenced removed columns — MOD, PUSH-TO,
  "Daemon =" — and the pointer line "run `repos --legend` when
  confused" didn't prevent operator confusion: an explanation you have
  to remember to ask for doesn't explain). Covers STATUS, ACTIVITY,
  A/B, PUSH, USED, COMMITS (1h/6h/24h), SIZE (white <1 GiB / 🟡 ≥1 GiB
  / 🔴 ≥2 GiB github limit), TOUCHED. Width-gated: suppressed on
  terminals < 120 cols (compact tier) rather than wrapping brokenly;
  `repos --legend` still prints it unconditionally on demand.

## [0.113.11] - 2026-07-29

### Added

- **Tip-keyed verdict cache for the push-path `github_pack_too_large`
  guard**. The verdict is fully determined by (pushed-branch tip, github
  tracking tips, limit) under the v0.113.10 delta semantics, so the
  cache key is resolved by reading ref files directly (loose refs,
  packed-refs, HEAD indirection, config-file remote scan) — a cache hit
  performs NO git subprocess and skips the `.git` dir walk. Previously
  every push cycle re-measured in full; an actively-committing repo
  with gitdir ≥ 2 GiB and an over-limit uncompressed delta would have
  paid a multi-second `pack-objects` run per cycle. Only clean
  determinations are cached (the conservative detached-HEAD/error
  fallback is never pinned behind an unmoved key), and caller-supplied
  precomputed sizes bypass the cache.

### Fixed

- **`release.sh` remote derivation + idempotency** (failed identically
  on v0.113.9 and v0.113.10). The github push remote is now derived
  from `remote.*.url` (new `scripts/resolve-github-remote.sh`) instead
  of the hardcoded name `github` — this repo names it `origin`. The
  CHANGELOG-close step is extracted to `scripts/close-changelog.py`
  and is idempotent (a re-run on an already-closed version leaves the
  file byte-identical; the v0.113.10 re-run duplicated the header).
  Steps 5/6 now tolerate the partial-failure re-run path: already-
  published crates.io version, existing tag, nothing-to-commit, and
  existing GitHub release are all "already done", not fatal errors.
  The script also prints the exact mirror-tag push commands
  (codeberg/gitlab) at the end — both prior releases forgot them.

## [0.113.10] - 2026-07-29

### Changed

- **`github_pack_too_large` measures the push delta, not the whole
  branch** (fixes the junk-runner-class false positive). The slow path
  now computes, per github-host remote, the objects the remote does not
  already have (`rev-list --objects <branch> --not <remote-tip>`), and
  when the uncompressed delta exceeds the 2 GiB limit it takes a
  compressed second chance: the same object set is streamed through
  `git pack-objects --stdout` and counted — github's limit applies to
  the compressed pack it receives. Compressible histories (junk-runner:
  3.79 GiB uncompressed whole-branch vs 14.77 MiB actual next-push pack)
  now clear correctly; incompressible over-limit deltas (CAG's PNGs)
  remain flagged. Safety degradations: missing/non-ancestor tracking
  tips and the no-github-remote case measure the whole branch
  (fresh-remote = whole branch ships); multiple github remotes take the
  worst case; measurement errors stay conservative. The `.git` < 2 GiB
  fast path is unchanged.

### Added

- **`auto_prune_stale_backup_branches`** (default `false`): opt-in
  janitor for stale daemon-created branches
  (`backup/pre-sync-largeblob-fix-*`, `daemon-standalone`) and orphaned
  remote-tracking refs (`refs/remotes/<removed-remote>/*`). A daily
  per-repo pass bundles all candidates into
  `<backup_dir>/auto-prune/`, verifies the bundle, deletes locally, and
  deletes remote copies whose tracking tips match the bundled local tip
  (never the remote's default-HEAD branch). The remote deletion injects
  `DRACON_ALLOW_REWRITE=1` into that single push command's environment —
  the sanctioned narrow exception to the no-auto-rewrite policy — and
  every deletion is `log_warn!`'d with repo, ref, tip, and bundle path
  so the journal remains the operator-review trail. Requires
  `backup_dir`.

## [0.113.9] - 2026-07-29

### v0.113.9 — 2026-07-29 — advisor-catch: SIZE color semantics + assert removal

Two follow-up fixes to v0.113.8 surfaced by the
post-release advisor review:

- **SIZE color threads `pack_too_large` explicitly** —
  `size_label(Option<u64>, bool)` now takes the same bool
  the daemon uses for PACK_SIZE_WARNING / CONCERN. Red iff
  `pack_too_large == true` (the actual github-rejection
  condition); yellow iff gitdir ≥ 1 GiB (capacity warning
  independent of push). **deathrun** (4.08 GiB gitdir but
  ✅ CLEAN) was the test case that exposed the bug: the
  original code colored it Red, contradicting its STATUS
  cell. Post-fix: Yellow. `RepoReportRow` gained a new
  `pack_too_large: bool` field.
- **Removed runtime `assert!` in `print_repos_rich_table`** —
  the column-set ≤ 165 cols invariant was enforced at
  runtime (panicking the process on forced-narrow layouts).
  The invariant is already pinned by the test
  `test_rich_table_fits_narrow_terminal`; runtime enforcement
  was the wrong layer. comfy-table's `Absolute(width)`
  degrades gracefully on narrow terminals.

`test_size_label_units_and_colors` rewritten to cover the
new signature including the deathrun-vs-junk-runner
color-distinction case. See `release-notes-v0.113.9.md`
for full source-change table, before/after fleet state,
and cross-references.

### v0.113.8 — 2026-07-29 — rich-table diagnostic columns

The `dracon-sync repos` rich-table default view dropped the
HINT prose column and gained 4 new diagnostic columns:

- **USED** — combined human + daemon activity tier
  (`🟢used` / `🟡mod` / `⚪idle` / `⚫cold`). Answers
  "which repos are used" at a glance.
- **COMMITS** — 1h/6h/24h commit split (`N/N/N` format).
  Reveals recent iteration cadence.
- **SIZE** — gitdir bytes in adaptive units (B → KiB → MiB
  → GiB), color-coded by github's 2 GiB pack-size threshold
  (red at ≥ 2 GiB, yellow at ≥ 1 GiB, white below).
- **TOUCHED** — last commit author + relative time
  (`DraconDev 14m`, `dracon 10 sec`).

The ACTIVITY column widened from 21 to 28 cols (now fits
`⏳ dirty 8m · 1 mod + 5 ut` without truncation). The rich
table grew from 7 to 10 columns; the minimum terminal width
bumped from 90 to 165 cols (operators on narrower terminals
route to the Compact tier automatically, unchanged from
v0.113.7).

Detail / "why is this row in this state?" moves to the
per-repo drill-down (`dracon-sync repos <name>` or
`--layout vertical`); the rich table surfaces *what* is
happening (use, growth, recency) and leaves the *why* for
follow-up.

6 new unit tests cover the 4 new helpers
(`used_label`, `commits_window_label`, `size_label`,
`touched_label`) + the rich-table layout invariants
(`test_rich_table_headers_fit_columns`,
`test_rich_table_fits_narrow_terminal`). Total daemon
test count: 854 (was 854 in v0.113.7; the new tests
added 6 net new).

See `release-notes-v0.113.8.md` for full source-change
table, before/after fleet state, and trade-off analysis.

## [0.113.5] - 2026-07-27

### v0.113.7 — 2026-07-28 — concern-retry-softening: auto-mirror eager-create fix

Addresses the concern-repair `create_private_remote` eagerness
gap: pre-fix, the daemon's auto-repair concern path forked an
offline mirror on the FIRST invocation of `handle_no_origin`
whenever `has_origin` was false — even on a transient SSH/DNS
hiccup or for a repo that was previously pushed. Post-fix:

- **3x retry with 5s delay** before declaring origin gone
  (`probe_any_remote_reachable` in `src/report.rs`, runs
  `git ls-remote <name> HEAD` per configured remote; definitive
  not-found answers count as "reachable but missing" so the
  probe does not hang on a configured-but-empty forge).
- **Gone-since ledger** (`<policy_dir>/origin-gone-ledger.tsv`):
  records the first observed unreachable failure; cleared when
  the next invocation succeeds. The mirror-create gate is open
  only when the elapsed window exceeds the
  `CREATE_MIRROR_GONE_THRESHOLD_SECS = 900` (15 min).
- **"Never pushed" guard** (`ever_pushed`): the gate stays
  closed even after 15 min if the checkout has any
  `refs/remotes/<name>/*` entry — if the operator ever pushed,
  the current missing-origin is transient by definition.
- **Pure decision helper** (`decide_create_mirror` +
  `CreateMirrorDecision` enum, both `pub(crate)`) extracted for
  regression-testing without network probes.
- **Two new regression tests** (`concerns_retry_softening`,
  `concerns_retry_softening_really_gone`) cover the 5+ boolean
  input combinations and the 900-sec threshold boundary.
- **Log distinction**: `transient ssh hiccup — will retry`
  (probe inconclusive, ever-pushed, or gone < 15 min) vs
  `origin gone > 15min AND never pushed — creating offline
  mirror`. Both also go through `log_incident` for the
  ledger audit trail.

**Honest framing**: the 73-minute browser-extensions-shared
stall from 2026-07-27 was the SYNC-M3 issue, closed in
v0.113.5 (the `should_push = ahead > 0 || upstream_ref_missing`
gate; `git log empty..tag-sha` for an unreachable origin no
longer pushes). This v0.113.7 release closes the distinct
`create_private_remote` eagerness gap — a real but separate
soft-spot, located in the `handle_no_origin` concern-repair
path rather than the daemon's per-cycle sync loop.

### v0.113.7 — 2026-07-28 — pack-size-concern: silent-skip → ❌ CONCERN

Surfaces the github-push-permanently-skipped class as a
visible CONCERN in the `repos` table (was a buried HINT with
a `🔄 ACTIVE` row). Pre-fix, a repo whose pushable branch
exceeded GitHub's 2 GiB pack limit emitted only a HINT like
`.git exceeds 2 GB (github limit) — may fail to push to
github` while the daemon's push path was silently skipping
GitHub — and the row's STATUS cell stayed at `🔄 ACTIVE`. The
operator had to read journalctl to learn the push was being
skipped. Post-fix:

- **CONCERN reclassification** (production call site at
  `src/report.rs:3157`, helper `pack_too_large_forces_concern`
  at `src/report.rs:1693`, both `pub(crate)`). When
  `github_pack_too_large.0` is true, the row is now classified
  as `❌ CONCERN` (not `🔄 ACTIVE`). The helper is a pure
  function so the regression test does not have to spin up a
  whole `RepoReportRow` (same pattern as M1/M2/M4 helper
  extraction: `daemon.rs:72`, `sync.rs:3652`, `daemon.rs:124`).
- **HINT text updated** (`src/report.rs:2100`): the misleading
  "may fail to push to github" phrasing became "github push is
  skipped; shrink history or migrate assets to OVH". The hint
  tells the operator (a) the push is permanently skipped, and
  (b) the two available remediations.
- **Auto-repair no-op** (`src/report.rs:6417`, the
  `run_repair_concerns` loop): when the `PACK_SIZE_WARNING`
  flag is present, the auto-repair short-circuits with
  `⏭️ skipping auto-repair: github push is permanently
  skipped (pushable branch > 2 GiB). Operator action
  required.` Without this guard, the new CONCERN would invoke
  every handler in the loop (`handle_no_origin`,
  `handle_no_upstream`, `handle_behind`, etc.) and silently
  fail every sync cycle, producing journalctl noise.
- **One new regression test** (`test_pack_too_large_forces_concern`):
  pins the helper's 4-case boolean matrix (true with size,
  true without size, false with size, false without size).

**Live evidence**: `capture-anime-girls` (CAG) — pushable
branch 2.37 GiB, was at `🔄 ACTIVE` with the HINT buried in
journalctl. Post-deploy, the row shows `❌ CONCERN` with the
updated HINT and the daemon's `auto_repair_concerns` cycle
no longer iterates past it.

**Investigation**: `docs/design/cag-github-push-block-2026-07-28.md`
(the github-side remediation is still operator's call: orphan
cutover vs OVH migration vs filter-repo).

**Design doc**: `docs/design/pack-size-concern-2026-07-28.md`.

**Test count**: 1158 → 1159 (1 new). Clippy + deny clean.

### v0.113.6 — 2026-07-28 — completes v0.113.5: M4 trailing-drain unification

The published `v0.113.5` tag was created on a doc-only commit
before the M4 helper (`apply_outcome`) was actually added to source.
This release re-tags the M4-complete state under a new tag (no tag
rewriting, AGENTS.md "no force-push anywhere" honored).

- **SYNC-M4 — main apply phase vs trailing-drain symmetry** (helper
  `apply_outcome` at `daemon.rs:124`, `ApplyOutcome` enum at
  `daemon.rs:95`, call sites at `daemon.rs:4261` and `daemon.rs:4424`).
  Pre-fix the main apply phase and the trailing-drain path each had
  their own `match sync_res { ... }` block — nearly identical, but
  with two divergence bugs the audit caught. First, trailing-drain
  `NothingToDo` did nothing (no activity.remove / failure_count
  reset, leaking entries across cycles). Second, trailing-drain
  `Synced` did not call `stuck_push_repos.remove + save` (ledger
  would stay stale until a main-phase success). Post-fix, both
  phases route through the single `apply_outcome` function; the
  `is_late: bool` parameter toggles the log suffix only — outcome
  classification and side effects are structurally identical.
  `RepoActivity` was promoted to `pub(crate)` so the helper can
  take `&mut RepoActivity` (still crate-private, not exposed to
  downstream crates). Regression test
  `test_m4_helper_structurally_unified` drives each `SyncOutcome`
  variant through the helper and pins the
  `ApplyOutcome::Success / Blocked / BackstopSkipped / Failure`
  classification matrix plus the side-effect contracts.

- **Release-notes correction**: `release-notes-v0.113.5.md` line
  refs and the test-count claim (`851 passed`) were written before
  the M4 fix landed in source. The corrected posture (matching
  this release) is `852 passed` (the M4 regression test was added
  during this session), with M1/M2/M3 line refs as published.

### v0.113.5 — 2026-07-27 — MEDIUM-finding remediation batch (M1-M4)

Closes the 4 still-open SYNC MEDIUMs from `AUDIT_FULL_2026-07-26.md`:

- **SYNC-M1 — `detached_discard` keyed per-task-generation, not
  per-repo** (`daemon.rs:4176-4196`, helper
  `should_discard_stale_detached_result` at line 65). Pre-fix the
  `HashSet<PathBuf>` discarded whichever future result arrived
  first, inverting outcome depending on completion order. Post-fix
  `HashMap<PathBuf, u64>` stores the wedged generation; only a
  result whose generation matches is dropped. `SyncTrioJoin` tuple
  extended 3 → 4 elements (added `u64` generation); per-repo
  `dispatch_gen` counter bumped on every dispatch.
- **SYNC-M2 — filter-only early return drops injected stale-gitlink
  entries** (`sync.rs:4216` plus helper
  `should_short_circuit_filter_only` at `sync.rs:3989`). The
  short-circuit now also requires `stale_gitlink_injected == false`;
  if the gitlink-injection step ran, the rest of the apply phase
  continues and the parent gitlinks converge.
- **SYNC-M3 — v0.113.1 FilterOnly `handle_ahead_push` flips benign
  repo to PushFailed / stuck-ledger exhaustion** (`sync.rs:4214`).
  Removed the `|| !branch_has_upstream` clause from `should_push`.
  Pre-fix, mirror-only repos with no upstream configured had
  `should_push=true` forever; every 300s stage cooldown cycle issued
  a real push attempt that could write to the stuck ledger.
  Observed live on browser-extensions-shared (73-minute stall on
  2026-07-27 after a transient ssh hiccup). Post-fix `should_push =
  ahead > 0 || upstream_ref_missing`: push only when there is
  positive evidence of unpushed work (the v0.112.30 bootstrap-push
  behavior is preserved via the `upstream_ref_missing` arm).
- **SYNC-M4 — main apply phase vs trailing-drain asymmetry**
  (`daemon.rs:2641-2799`, helper closure `apply_outcome` inside
  `run_daemon` returning the closure-local `ApplyOutcome` enum).
  Pre-fix, the two phases each had their own `match sync_res { ... }`
  block; trailing-drain `NothingToDo` did nothing (leaking activity
  entries across cycles) and `Synced` did not clear the
  stuck-ledger. Post-fix both phases route through the single
  `apply_outcome` closure; divergence is structurally impossible.

Also closed 14 unrelated pre-existing baseline clippy warnings
(`int_plus_one`, `bool_assert_comparison`, `cmp_owned`,
`useless_conversion`, `unused_variables`, `useless_vec`,
`unnecessary_get_then_check`, `cloned_ref_to_slice_refs`) so
`cargo clippy --workspace --locked --all-targets -- -D warnings`
is clean again at the workspace root.

See `release-notes-v0.113.5.md` for the audit cross-references and
operator notes. Regression tests: `test_m1_*`, `test_m2_*`,
`test_m3_*`, `test_m4_*` in `daemon.rs` and `sync.rs`.

## [0.113.4] - 2026-07-26

### v0.113.4 — 2026-07-26 — full-audit remediation batch 4 (visibility + standard_files)

- **SYNC-H4 — visibility cache-poison on transient gh failure**:
  `sync_mirror_visibility` used the bool `get_github_visibility`
  (safe-default `true` on ANY failure) and unconditionally wrote the
  visibility cache "even on partial failures" — violating the
  `get_github_visibility_opt` cache-poison invariant. A network
  hiccup / auth expiry / rate limit flipped PUBLIC mirrors to
  private (an uncommanded remote state change) and poisoned the
  cache for 24h, gating the codeberg-public-only push path off.
  Now uses `_opt` and skips BOTH the mirror flips AND the cache
  write when visibility is unknown. The test that encoded the buggy
  contract was rewritten to assert the new one.
- **SYNC-H5 — `standard_files` source path traversal**: the target
  got full component validation but the source only an `is_absolute`
  check on the RAW string — and tilde expansion happened AFTER
  validation, so `source = "~/.ssh/id_rsa"` or `../../etc/passwd`
  passed. Worse, the daemon's execution path never calls
  `validate_config` at all. A config typo or write to the policy
  file was a read-anywhere → publish-everywhere primitive under the
  daemon's UID (copied into every watched repo, auto-committed,
  auto-pushed to public forges). New shared
  `is_safe_standard_file_path` (no absolute, no `..`; `~/...` still
  allowed) enforced BOTH by `validate_config` AND at the point of
  use in `ensure_standard_files` (skip + warn). Regression tests
  added.

## [0.113.3] - 2026-07-26

### v0.113.3 — 2026-07-26 — full-audit remediation batch 3 (auto-repair path)

Remediation batch 3 of `AUDIT_FULL_2026-07-26.md`. Investigation first:
the SYNC-H6 repair path DID fire in the fleet (dracon-platform,
2026-07-15 02:02 — a `backup/pre-sync-largeblob-fix-*` branch exists
that post-dates the 2026-06-30 audit's "zero backup branches" claim),
but the feared self-undo did NOT materialize: no merges in the window,
no blobs >6 MiB in main's history today. The backup branch tip points
at a commit in main's first-parent history (unrewritten), so the
filter-repo either no-op'd or never ran; the code path is live though,
and its failure mode (verified by reproduction in the audit) is
unacceptable. Fixes:

- **SYNC-H6 — `rewrite_ahead_paths` destroyed its own backup, deleted
  `origin`, and reported real rewrites as no-ops** (empirically
  reproduced in the audit): `git filter-repo --invert-paths --force`
  rewrites ALL refs — the `backup/pre-sync-*` branch created before
  the rewrite preserved nothing; backup-tree vs HEAD-tree were
  therefore ALWAYS equal, so the F31 no-op check deleted the backup
  and returned `Ok(None)` for REAL rewrites → the caller never
  pushed; and filter-repo deletes the `origin` remote, so the next
  cycle's auto-pull-on-reject would merge the PRE-REWRITE history
  back in (the >100 MiB blob returns to local history and is pushed
  to all mirrors — the repair silently un-doing itself). Now:
  (1) the backup is a `git bundle` FILE in the gitdir — not a ref,
  so the rewrite cannot touch it; (2) filter-repo runs with
  `--refs HEAD` (only the current branch is rewritten); (3) the
  no-op check compares pre/post-rewrite HEAD SHAS; (4) the origin
  URL and upstream sha are captured BEFORE the rewrite, origin is
  re-added afterwards, and the caller force-pushes origin + mirrors
  with `--force-with-lease=<ref>:<pre-rewrite-upstream-sha>` via the
  new `force_push_after_rewrite` (a diverged remote fails the lease
  and is logged, never clobbered). New regression test:
  real-rewrite → Some(outcome), bundle contains pre-HEAD, origin
  preserved, lease captured, side branches untouched.
- **M7 — auto-pull-retry hazards** (folded in, same function
  cluster): `git pull --no-rebase origin HEAD` pulled the remote's
  DEFAULT branch (not necessarily the branch being pushed), could
  open `$EDITOR` on a tty (`dracon-sync once` hanging in vim), and
  left the repo in MERGING state on conflict. Now pulls the explicit
  `refs/heads/<branch>`, passes `--no-edit`, and runs
  `git merge --abort` on failure.

## [0.113.2] - 2026-07-26

### v0.113.2 — 2026-07-26 — full-audit remediation batch 1 (4 HIGH fixes)

Remediation batch 1 of `AUDIT_FULL_2026-07-26.md` (13 HIGH findings
fleet-audit; every HIGH independently verified against code, two
empirically reproduced). Fixes:

- **SYNC-H8 — conflict-state detection was a no-op for nested
  submodules** (all 10 nested game repos):
  `is_merge_in_progress` / `is_rebase_in_progress` /
  `is_cherry_pick_in_progress` probed `<repo>/.git/MERGE_HEAD` etc.,
  but for the canonical nested-on-`main` submodule layout `<repo>/.git`
  is a FILE (gitdir pointer) — ENOTDIR, always false. The daemon could
  `git add -A` through an operator's in-progress conflicted merge,
  silently "resolving" conflicts with markers and auto-pushing them to
  all forges. All three helpers now resolve the real gitdir via
  `path_gitdir()` (same fix class as the v0.112.33 IndexLock fix).
  Regression tests: gitfile + plain layouts.
- **SYNC-H2 — auto-commit backstop was self-defeating**: it returned
  `NothingToDo`, which the daemon apply phase treats as success —
  removing the activity entry and wiping `ahead_since`, the very signal
  `is_backstop_active` needs. The backstop disarmed after ONE skipped
  dispatch and the daemon re-committed the next cycle; additionally the
  early return suppressed the push that would have drained the backlog.
  The backstop now calls `handle_ahead_push` first (draining ahead<N is
  the fastest way out of the state) and returns the new
  `SyncOutcome::BackstopSkipped`, which retains the activity entry, is
  excluded from failure accounting, applies a 60s cooldown, and does
  not feed the sustained-blocked notification machinery.
- **SYNC-H3 — `maybe_auto_gc` blocked a tokio worker with no timeout
  and ran before the conflict check**: v0.113.0 used synchronous
  `std::process::Command::output()` for `git gc --prune=now` — a
  multi-GiB gc pinned a worker for minutes, the wedge valve could
  re-dispatch the repo while the old gc was still running, and
  `--prune=now` (no mtime grace) racing any concurrent writer is the
  classic prune race against in-flight object writes. Now: runs AFTER
  `check_conflict_state`, async bounded via `run_git_with_timeout`
  (600s, kill-on-timeout) using the shared git builder (honors
  `DRACON_SYNC_GIT_BIN`), plain `git gc` (2-week prune grace retained —
  stale tmp_pack_* files, the actual incident driver, are removed by gc
  regardless), plus a per-repo 1h attempt cooldown so a failing gc
  doesn't re-run every cycle.
- **SYNC-H1 — quiet-daemon permanent wedge**: the detached-task
  registry drain and the 15-minute wedged-task valve lived inside
  `if !to_sync.is_empty()`. A repo whose task outlived the trailing
  deadline stayed in `in_flight`; with no other repo dispatching
  (overnight, single active repo) the whole block was skipped every
  cycle — the finished/wedged task was never applied, the valve never
  fired, and `repos` reported the repo as actively-processing forever
  (false-healthy), re-opening the 2026-06-15 permanent-skip class. The
  gate now also opens when the detached registry is non-empty; in that
  quiet-maintenance mode the trailing drain runs with a ZERO deadline
  (poll-only: finished tasks applied, running ones not awaited) so
  cycle responsiveness is unchanged.
- **SYNC-H7 — `detect_large_blobs_ahead` pipe deadlock** (fixed ahead
  of its scheduled batch): cat-file's stdin pipe was filled BEFORE
  `wait_with_output()` started draining stdout — with ~4000 objects
  ahead the 64 KiB stdout pipe fills, cat-file stops reading stdin, and
  the parent's `write_all` blocks forever (uncancellable spawn_blocking
  thread + leaked child every repair cycle), leaving the 100 MiB blob
  guard silently disabled via `.unwrap_or_default()`. cat-file stdin is
  now fed from a temp FILE (std-only, Drop-guard cleanup) — no pipe, no
  deadlock. (Initial patch contributed by an audit subagent; repaired
  here to drop the dev-only `tempfile` dependency and a borrow error.)

## [0.113.1] - 2026-07-26

### v0.113.1 — 2026-07-26 — FilterOnly push starvation fix + stale upstream refresh

**Filter-only dirty no longer starves pending pushes.** The
`filter_only_cleared` early-return ran BEFORE the push phase: a repo
whose only dirty entries are filter noise (junk-runner's tracked +
gitignored `.pi-glla/active.jsonl` heartbeat, rewritten every ~15s by
its loop agent) returned `FilterOnly` every cycle, the 300s stage
cooldown silenced it, and the push phase was never reached —
already-committed work piled up unpushed indefinitely (junk-runner:
**19 commits, 10h of silent starvation** while the report showed
"pushing 240m"). Now the FilterOnly path runs `handle_ahead_push`
first (a cheap local no-op when there is genuinely nothing to push).

**Stale upstream tracking refs converge after a successful push.**
The daemon pushes to named mirror remotes; when `origin` shares a
URL with one of them (junk-runner: origin = gitlab), the push never
updates `refs/remotes/origin/main`, so libgit2 reported ahead>0
forever and the report lied ("pushing 240m" with the push long
done). `refresh_stale_upstream_ref` runs a bounded `git fetch
<upstream-remote>` after a successful push, but only when the
tracking ref actually disagrees with HEAD (zero network cost in the
common converged case).

### v0.113.0 — 2026-07-25 — auto-gc garbage knob + gitlab auto-protect on create

**`auto_gc_garbage_threshold_bytes`** (default 2 GiB, 0 disables): when a
repo's dangling-object garbage (`count-objects` `size-garbage` — the
tmp_pack_* debris of interrupted pushes) exceeds the threshold, the daemon
runs `git gc --prune=now` itself. Root-cause fix for the recurring `.git`
bloat incidents (hegemon 4.9 GiB, dracon-platform 37 GiB) that previously
needed manual gc and tripped the GitHub 2 GiB pack guard.

**gitlab branch protection on auto-create**: `create_repo_on_gitlab` now
immediately protects `main` (maintainers push, `allow_force_push=false`),
and re-ensures it on the already-exists path. Without this, the
2026-07-25 fleet protection sweep (19 branches) would silently regress
with every new auto-created repo.

**Note**: the no-history-rewrite hook enforcement designed for this
release moved to **dracon-warden 0.113.0** — warden owns the hook layer
fleet-wide via global `core.hooksPath` + `init.templateDir`; a second
installer in dracon-sync would have ping-ponged ownership every cycle.

### v0.112.42 — 2026-07-25 — `repos` cold-run perf (TTL 30s→1h) + KiB unit fix

**Perf**: `REPO_SIZE_CACHE_TTL_SECS` 30 → 3600. The 30s TTL meant every
`repos` run more than 30s after the last paid the full cold path
(count-objects + pack-check + missing-objects probe × 35 repos) —
measured 6.9–17.6s. Sizes and object corruption don't need 30s
freshness; the gitdir-mtime signature still invalidates post-TTL, and
the PUSH path measures fresh (`github_pack_too_large(repo, None)` in
sync.rs), so push-time 2 GiB accuracy is unaffected. Measured: every
run within the TTL is now ~0.98s (was 1s only within 30s).

**Correctness**: `measure_git_size_via_count_objects` treated
`git count-objects -v` output as **bytes** — git reports **KiB**. Every
repo looked 1024× smaller, which silently disabled
`github_pack_too_large`'s 2 GiB fast-path guard since v0.112.40
(dracon-platform's 11.4 GiB pack read as 0.011 GB). Fixed (×1024 on
parse). Verified no push-behavior flip: dracon-platform's pushable
bytes = 1.49 GiB < 2 GiB, all other repos far below.

### v0.112.41 — 2026-07-25 — daemon-mode GIT_SSH_COMMAND (systemd 258.7 userns ssh fix)

**Fix: every CLI fetch/pull from the daemon failed with `Bad owner or permissions on /nix/store/.../20-systemd-ssh-proxy.conf`.**

Root cause: the systemd 258.7 user-service sandbox (`ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateTmp`) now runs the daemon inside a **user namespace** where root-owned files appear as `nobody(65534)`. OpenSSH's `secure_filename()` rejects any config path component not owned by euid or uid 0, so the `/etc/ssh/ssh_config` system Include of systemd's nix-store `ssh_config.d` file failed the check — breaking plain-ssh `git fetch`/`git pull` from the daemon while pushes (which already set `GIT_SSH_COMMAND` with the `-F ~/.dracon/secrets/ssh/config` hardened config) kept working. Reproduced deterministically via `systemd-run --user -p ProtectHome=read-only ssh -G ...`.

Fix: daemon mode now sets `GIT_SSH_COMMAND` process-wide (only if not already set) to the same `git_ssh_hardening()` value pushes use, so dracon-git's CLI-first `fetch()`/`pull_merge()`/`pull_rebase()` subprocesses inherit it. See `docs/design/incident-amend-race-and-trust-2026-07-25.md`.

Ships together with **dracon-git v94.7.2** (`[patch.crates-io]` tag bump): git2 0.21 changed `default = []`, so the bare `git2 = "0.21"` built libgit2 with NO ssh/https transports — the libgit2 fetch/pull fallback always failed with `unsupported URL protocol; class=Net (12)`, masking the real CLI error above. v94.7.2 enables the `ssh`/`https` features and adds `ssh_cred()`: probe `SSH_AUTH_SOCK` eagerly (the agent failure is lazy — a naive `.or_else()` never fires) and fall back to `~/.ssh/id_ed25519|id_rsa|id_ecdsa`, mirroring CLI ssh `IdentityFile` behavior. Required because the daemon runs with no `SSH_AUTH_SOCK`.

### v0.112.40 — 2026-07-24 — `repos` perf fix (count-objects fast path + 30s cache TTL)

**Two-part fix for the `dracon-sync repos` 4-12s slowness during active daemon work.**

1. **Fast-path size probe** (`report.rs`): replaced `du -sb` (200ms+ per multi-GiB gitdir) with `git count-objects -v` parsed for `size + size-pack + size-garbage` (~10ms per gitdir — 17× speedup on 54 GiB gitdirs). `du -sb` retained as fallback for `count-objects` failures. Semantically tighter: the new number is **reachable object bytes** (what would actually ship to a remote, plus orphaned tmp_pack_* bloat) rather than total gitdir tree bytes (which included logs/refs/config). This means `github_pack_too_large`'s fast-path precondition (`if size < 2 GiB, skip slow path`) is now correct — previously a repo with 20 GiB of unreachable garbage could falsely trigger the slow path.
2. **30s cache TTL**: the size+pack cache (`repos-size-cache.json`) now records `cached_at_secs` alongside the gitdir mtime signature. The lookup honors entries within `REPO_SIZE_CACHE_TTL_SECS = 30` **regardless of gitdir mtime** — the daemon's constant commits/fetches bump the mtime but the cache is still valid for 30s. Old cache files (`cached_at_secs: None`) load successfully via `#[serde(default)]` and force one recompute, then start honoring the TTL.

**CRITICAL FIX (R2)**: the initial TTL implementation required `sig matches AND fresh` — which meant the TTL never fired because the daemon's gitdir mtime updates always invalidated the sig before the TTL could help. The corrected logic drops the sig check for fresh entries: if the cache was written within 30s, we serve it regardless of whether the daemon has since bumped the gitdir mtime. This was the difference between 6-7s per repo (`probe_missing_objects` running on every cache miss) and <1s per repo (cache hit).

**Measured impact** (live CLI, 34 repos, daemon actively committing):

| Scenario | v0.112.39 | v0.112.40 | Δ |
|---|---|---|---|
| Steady-state (no daemon activity) | 1.3s | 1.0s | 1.3× |
| Active daemon (back-to-back calls) | 1.3s–11.8s | 1.0s–4.1s | up to 3× |
| Cache miss (every gitdir changed) | ~10s | ~10s | flat — floor is libgit2 working-tree walk |

**Side effect (operator-visible):** the new `count-objects` measurement surfaces dangling tmp_pack_* files that were previously invisible to `du`. The fleet-wide scan in this release found ~50 GiB of orphaned tmp_pack_* files in `dracon-platform/.git/` (10 files, 30 GiB) and `hegemon` gitdir (9 files, 19 GiB). Running `git gc --prune=now` on those two repos freed 50 GiB. The repo sizes shown via `count-objects` are now ~3 orders of magnitude smaller for those repos (20.4 GiB → 19 MiB for hegemon; 54 GiB → 12 MiB for dracon-platform) because the orphaned objects are now `size-garbage` rather than masquerading as reachable.

**Tests:** 829 daemon tests pass (+4 new: cache round-trip with `cached_at_secs`, count-objects fast path, fallback chain, missing-repo returns None). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

See `docs/design/repos-perf-fix-v0.112.40-2026-07-24.md` for the full investigation, measurements, and design rationale.

### v0.112.39 — 2026-07-23 — deathrun size fix (orphan cutover) + BROKEN_HISTORY detection + frame-dump prevention

**deathrun fix + prevention, with an important diagnosis correction.**

1. **deathrun orphan cutover**: pushable branch was 2.85 GiB (audit-screenshot churn — `docs/audit-browser-v3` alone 1.5 GiB/4311 files, `.pi/chrome-screenshots` 585 MB), tripping github's 2 GiB pack limit. Orphan-cutovered to a clean root (`a77d795b rebirth`, 2388 files, 261 MB), force-pushed to gitlab (unprotect/push/re-protect) and github (`--force-with-lease=main`). **github accepted the push** — pushes resumed after days of the guard skipping it. All 3 forges + local at `036dedd8`, parent gitlink converged, `🟢 synced · healthy`.
2. **BROKEN_HISTORY detection** (`report.rs`): `probe_missing_objects` (with a path-strip fix) + a `BROKEN_HISTORY:N` state flag → CONCERN with hint "history damaged (N objects missing) — fresh clones fail; needs clone-from-forge or orphan cutover". Cached 24h alongside the size probe (`CachedRepoSize.missing_objects`).
3. **Frame-dump prevention** (`dracon-warden.toml`): `hygiene_patterns` now ignores `**/.pi/chrome-screenshots/` and `**/audit-*/screenshots/` fleet-wide (audit `.md` REPORTS still committed). AGENTS.md commit-all policy documents the exception (regeneratable frame dumps are valid to regenerate, wrong to keep forever). Same anti-rebloat class as hegemon's `**/.state-recon/**`.
4. **Auto-repair pre-flight** (`rewrite_ahead_paths`): refuses to rewrite a damaged gitdir (missing objects) with an alert instead of writing broken history.

**DIAGNOSIS CORRECTION**: the initial "2092 missing objects / broken history on both sides" was a **probe bug** — `git rev-list --objects` appends paths to blob/tree lines (`<sha> <path>`) and `cat-file` mis-parses them as "missing". The corrected probe (strips paths first) shows **0 missing objects everywhere**; deathrun was **fat, not broken**, and the auto-repair largeblob rewrites did NOT break history. The orphan cutover was still the right fix for the real size problem. See `docs/design/audit-screenshot-bloat-deathrun-2026-07-23.md`.

**Tests:** 825 daemon tests pass. `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.38 — 2026-07-22 — rich table default + per-repo detail

**Operator-requested UX reshape of `repos`:**

1. **New default view**: plain `dracon-sync repos` (at <242 cols) now shows a rich 7-column table (`# · STATUS · REPO · ACTIVITY · A/B · PUSH · HINT`) instead of the verbose per-repo block view. ACTIVITY includes dirty counts inline (`⏳ dirty 1d · 101 stg + 2 ut`); **A/B is the ahead/behind column** (`↑N` unpushed = data at risk, `↓N` upstream drift, `↑N ↓M` both, `—` in sync — the most important missing field); PUSH is the dedicated push-state cell (✅ OK / 🟣 PENDING / 🛑 STUCK / ❌ FAIL); branch is folded into REPO only when ≠ main (`darklord⚡master`). Sorted by severity. At ≥140 cols a PUBLISH column is added.
2. **Per-repo detail**: `dracon-sync repos <name>` (e.g. `repos darklord`) shows the full detailed block for ONE repo (branch, publish, changes, ahead/behind, push-to, push, last commit, pushed, activity, state, hint) — the "run details on a certain repo" path. Exit 2 on unknown basename or ambiguity.
3. The old block view remains available via `--layout vertical`; `-s/--summary` (3-col glance) and `--layout compact/full` (detailed tables at 242+/315+ cols) are unchanged.

New `LayoutTier::Rich` variant + `print_repos_rich_table`; `choose_layout_tier` returns Rich for <242.

**Tests:** 825 daemon tests pass (2 tier tests updated to the new default). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.37 — 2026-07-22 — desktop notifications for sustained problem states

**Operator-requested.** Two new sustained-state desktop notifications (`notify-rust`, Critical urgency, throttled to 30 min via the v0.112.31 expiring throttle) closing the gaps from the darklord and F0.2 incidents:

1. **Blocked >30 min**: a repo continuously blocked by a needs-human guard (merge/rebase in progress, commit-time ownership check) now fires a desktop notification after 30 continuous minutes. The darklord M10 block sat for ~a day with zero desktop notifications. New `blocked_since` field on `RepoActivity` (set on `SyncOutcome::Blocked`, cleared on any non-Blocked outcome).
2. **Unowned >15 min**: a repo continuously skipped by the ownership guard now notifies after 15 minutes with a pointer to `dracon-sync ownership --explain`. The F0.2 incident (daemon's own repo unowned for 25 minutes) had no operator signal beyond the journal. New `unowned_since` field (set in the ownership-skip branch, cleared when owned).

Also extracted `sustained_threshold_met` (unit-tested) shared by all four sustained checks (ahead/behind/blocked/unowned).

**Tests:** 825 daemon tests pass (+1). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.36 — 2026-07-22 — M10 guard honors ownership overrides + WARN width fix

1. **darklord WARN (operator-reported)**: the v0.112.33 M10 pre-commit identity guard used a raw trusted-list check and blocked darklord's deliberate per-repo identity (`darklord-dev <darklord@dracon.local`) despite its `owned = true` override — 101 staged files sat for a day, journal warning every ~50s. `commit_allowed_by_ownership` now accepts `owned = true` in `.dracon/dracon-sync.toml` (operator-blessed) OR an identity in the trusted lists (the F0.1 `test@test` case still blocks). It does NOT re-adjudicate origin trust (the loop's ownership gate already did). `Blocked` outcomes now cool the repo down 300s (was ~50s retry churn). 3 new tests; darklord's 101 files committed on deploy.
2. **darklord row visual drift (operator-reported)**: the WARN status cell used ⚠️ (U+26A0) — `unicode-width` counts it 1 but terminals render it 2 cells, so every WARN row's separators drifted one column right of the table frame. Replaced with 🟡 (yellow circle, width 2 = rendered 2, matching the 🟢⚪⚫🟣 activity-dot family) in the STATUS cell and the tally line.

**Tests:** 824 daemon tests pass (+3). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.35 — 2026-07-22 — activity-label date parser fix

- **Repos with commits older than ~2 weeks lost their activity indicator** in the `repos` WHAT cell (spotted live on `DraconDev`: last commit "4 weeks ago" rendered as a bare "healthy" with no `⚫ cold` prefix). `activity_label` used a unit-limited duplicate (`parse_relative_minutes_to_u64`) of the report's full `parse_relative_minutes` — it handled only seconds/minutes/hours/days. It now delegates to the complete, already-tested parser. Verified live: `DraconDev` shows `⚫ cold 28d · healthy`. Regression test: `test_parse_relative_minutes_to_u64_handles_weeks_months_years`.

**Tests:** 821 daemon tests pass (+1). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.34 — 2026-07-22 — excluded-path semantics preserve edits (F1.16)

**Operator-visible change (operator-approved, from `AUDIT_FULL_2026-07-21.md`):**

- **`auto_commit_exclude_patterns` no longer deletes your edits.** After each commit, the daemon UNSTAGES excluded files (so its own `git add -A` doesn't sweep them into your next manual commit) but **preserves their worktree content** — your edits to excluded files stay on disk as modified-unstaged. Previously `restore_excluded_paths` ran `git restore --staged --worktree`, silently deleting uncommitted edits to excluded files after every commit (audit F1.16). Operators who WANT hygiene enforcement ("these files must always equal HEAD") opt in per-repo with `revert_excluded_to_head = true` in `.dracon/dracon-sync.toml`. 2 regression tests (default preserves, opt-in reverts). Documented in AGENTS.md.

**Tests:** 820 daemon tests pass (+2). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.33 — 2026-07-21 — audit MEDIUM sweep (M2-M28 daemon/git/policy + M10)

**Operator-visible changes (from `AUDIT_FULL_2026-07-21.md`):**

1. **Daemon state machines (M2-M9)**: auto-create stops re-checking confirmed repos (forge_confirmed terminal state); empty-repo bootstrap cools down 60s on nothing-to-stage and sweeps operator-staged oversized/excluded files before the root commit; MAX_FAILURES no longer abandons repos forever (15-min backoff + re-probe + notification) and merge-in-progress (`Blocked`) no longer counts toward the budget; an origin push failure no longer starves github/gitlab/codeberg mirror pushes for the cycle; push-timeout scaling works for mirror-only and non-main repos (dynamic `count_ahead_commits`); SIGHUP clears all cooldown maps; the trailing-drain timeout no longer causes duplicate concurrent sync_repo (detached-task registry + 15-min wedged valve); filter-noisy repos get a real 300s stage cooldown (`SyncOutcome::FilterOnly`).
2. **Git layer (M11-M19)**: startup repair now fixes the CHECKED-OUT branch's gone upstream; the filter-branch fallback actually works (argv rebuilt); 7 git call sites now check exit codes (`std_git_checked`) — critically `consolidate_to_main` no longer deletes master on a failed checkout; `remote_repo_exists` distinguishes missing from network-down (tri-state + session cache); deleted forge repos fail fast (permanent push rejection class) instead of retrying forever; IndexLock works on submodules (real gitdir); non-ASCII filenames no longer dropped from diffs (`-z` parsing + exit propagation); `remove_stale_remotes` preserves operator-added remotes (`dracon.managed-*` marker scoping); `is_safe_git_path` rejects `..` at any depth.
3. **Policy/visibility (M20-M28)**: `config validate` now prints warnings and detects top-level fields silently absorbed into the last `[[table]]` entry (live-verified on the operator's own `standard_files_auto`); `expand_tilde("~/x")` resolves to `$HOME/x` (was filesystem root); the visibility cache is only written when the github leg succeeds (a failed `make-public` can't open the codeberg gate for a still-private repo); `make-public`/`make-private` detects basename ambiguity and prints the resolved path; `refresh-visibility` counts gh failures as errors instead of poisoning the cache to private; `parse_github_owner_repo` is host-verified (gitlab/codeberg origins no longer queried as github); `.env`-file secrets get the F52 control-char refusal (curl header injection); exclude patterns like `reports/kdp-live-*.md` work (were silently dead) and `**/tmp/**` no longer excludes `tmpl/` paths (segment-exact matching).
4. **M10 (F0.3)**: the daemon verifies the committer identity (user.email + user.name) is in the trusted lists BEFORE auto-committing — the F0.1 post-hoc lockout is now a pre-commit guard.

**Tests:** 818 daemon tests pass (+21). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.31 — 2026-07-21 — audit HIGH batch: failure-visibility + policy-enforcement

**Operator-visible changes (8 fixes from `AUDIT_FULL_2026-07-21.md`):**

1. **Push failure is no longer reported as `🔁 synced`** (H3/F1.3). New `SyncOutcome::PushFailed`; the apply phase counts the failure (no synced log, `failure_count` increments). A mirror-leg failure (origin succeeded) also returns PushFailed — sync isn't healthy until ALL forges are current.
2. **Throttled notifications actually expire** (H4/F1.1). Every notification previously fired exactly ONCE per daemon lifetime (the `Entry::Vacant` deadlines were never read). New `notify_throttled` helper at 7 sites + SIGHUP clear.
3. **Stuck-push ledger unified; `push_max_retries` enforced** (H5/F1.2). Per-cycle disk reload fixes the split-brain; retries stamp `last_retry_at` instead of deleting the entry (the budget reset every 5 minutes); `StuckDecision::Exhausted` stops auto-push when the budget is spent and tells the operator how to resume (`dracon-sync unstuck` / `repair-concerns --apply`). Startup logs repos entering already-stuck.
4. **Directory-expansion can no longer bypass the 100 MiB hard limit** (H6/F1.5). New `stage_existing_files_filtered` re-applies size + pattern policy to every file discovered by the untracked-dir recursion.
5. **Local-first ahead counting** (H7/F1.4). No more `git ls-remote` (SSH) every 1s cycle for broken-push repos — a missing tracking ref already implies all-commits-unpushed (local `count_all_head_commits` + new `any_mirror_tracking_ref_exists`); ls-remote fallback behind 300s cooldown.
6. **Codeberg API URL fixed** (H10/F3.1). The v0.112.29 GitLab two-placeholder `str::replace` bug had a codeberg twin — every codeberg visibility/metadata call 404'd. `make-public --include-codeberg` now works (live-verified: corrected URL → 200, old → 404).
7. **Ownership verdict re-detects on a 10-minute TTL** (H1/F0.2). Operator remediation (fixed user.email/origin) is picked up without a daemon restart; recovery gets a `✅ ownership restored` log + alert. The skip log carries the SIGHUP recovery hint.
8. **Mirror failures tracked and named** (M1/F1.7+F3.9). `mirror_consecutive_fails` is finally populated (Mirror-Degraded notification was dead code); the stuck-ledger `last_error` names the failing remotes (`remotes: bad-mirror`) — the `repos` HINT says WHICH forge is failing.

**Internal:**

- `sync.rs`: PushFailed outcome + propagation, `stage_existing_files_filtered` (+ per-file re-filter), `failing_remote_names`.
- `daemon.rs`: `notify_throttled` (+7 sites, SIGHUP clear), stuck-push reload/`last_retry_at`/`StuckDecision`, ahead-override local-first reorder + `ls_remote_cooldowns`, ownership TTL re-detect (`ownership_needs_redetect`, `OWNERSHIP_REDETECT_TTL = 600s`) + recovery alert, `mirror_consecutive_fails` wiring at both apply-phase sites.
- `git/status.rs`: `any_mirror_tracking_ref_exists`.
- `visibility.rs`: codeberg single-placeholder template.
- `main.rs`: sync-now PushFailed arm.

**Tests:** 797 daemon tests pass (+14). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean. Audit: `AUDIT_FULL_2026-07-21.md` (H1, H3, H4, H5, H6, H7, H10, M1 remediated).

### v0.112.30 — 2026-07-21 — empty-repo bootstrap + never-pushed detection + codeberg exclusion

**Operator-visible changes:**

1. **Brand-new `git init` repos are now fully bootstrapped by the daemon.** Previously the daemon loop bailed on `!is_repo_ready` before dispatching `sync_repo`, so an empty repo sat at "❌ CONCERN · no commits yet" until the operator committed manually. The new `sync::bootstrap_empty_repo_commit` creates the root commit from the operator's untracked files with the full staging policy (gitignore/warden secrets respected, size limits, exclude patterns, ownership gate). Gated on `git::is_stable_empty_repo` so mid-clone repos are never touched (lock-file + `tmp_pack_*` checks).
2. **Never-pushed repos no longer show a false "synced".** After `configure_publish_upstream_if_missing` wrote branch config, libgit2 computed ahead=0 (no remote-tracking ref) and the repo was skipped forever. New `upstream_tracking_ref_missing` + `count_all_head_commits` fallback: no tracking ref anywhere ⇒ every commit is unpushed. `handle_ahead_push` treats a missing tracking ref as push-needed.
3. **Codeberg is skipped for new repos under the quota posture.** Previously every push failed with `Forgejo: Push to create is not enabled` (guaranteed-failure spam). New `codeberg_push_excluded` skips codeberg at configure+push time when effective auto_create is off AND no codeberg tracking ref exists. Pre-v0.112.28 repos keep pushing; the dead remote is auto-removed from `.git/config` on first push.
4. **v0.112.29 auto-create throttled** to one attempt per 300s per repo (was 2 SSH `ls-remote`/sec per empty repo forever).

**Latent bug fixed:** the codeberg arms in `auto_create_all_remotes` (v0.112.28) and the new exclusion matched the raw `auth_type` field, which defaults to `GitHub` when unset in TOML — the per-repo `auto_create_on_codeberg` opt-in was silently ignored. Both now use `effective_auth_type()` (push_url auto-detect).

**Internal:**

- `git/status.rs`: `is_stable_empty_repo`, `upstream_tracking_ref_missing`, `count_all_head_commits`.
- `sync.rs`: `bootstrap_empty_repo_commit`; old bare `git add -A` bootstrap replaced; `handle_ahead_push` missing-ref fix.
- `git/multi_remote.rs`: `codeberg_push_excluded`, `has_codeberg_tracking_ref`, `push_mirror_remotes` exclusion, `auto_create_all_remotes` effective-auth fix.
- `daemon.rs`: bootstrap call at `is_repo_ready` site, ahead-override extension, `auto_create_cooldowns` + `empty_bootstrap_cooldowns`, configure-time codeberg skip.

**Tests:** 783 daemon tests pass (+25 new). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean. Design doc: `docs/design/empty-repo-auto-create-fix-2026-07-21.md`.

### v0.112.29 — 2026-07-21 — empty-repo auto-create + gitlab URL bug fix

**Operator-visible changes:**

1. **Empty local repos are now auto-created on github + gitlab as soon as the daemon discovers them.** Previously, a fresh `git init` repo with no commits would silently skip auto-create forever, leaving the operator staring at "❌ CONCERN · run repair-concerns --apply (set upstream)" until they made their first commit — at which point the daemon would finally try to push and fail because the forge-side repo didn't exist. Now `push_mirror_remotes_create_only` runs BEFORE the readiness check. Idempotent via `git ls-remote` pre-check.
2. **Empty repos show an accurate hint** ("no commits yet — make first commit to enable push") instead of the misleading "push: fail" label. New `EMPTY_REPO` flag drives the new `repo_hint` branch; `push_status` is now `EMPTY` not `FAIL`.
3. **`make-public` / `make-private` for GitLab is fixed.** Pre-existing URL bug in `set_gitlab_visibility`: the `GITLAB_API_PROJECTS` template had two `{}` placeholders, but `str::replace` replaced both, producing `projects/owner%2Frepo%2Fowner%2Frepo`. GitLab returned 404 for every visibility flip. Fixed to single-placeholder template.

**Internal:**

- `daemon.rs`: pre-`is_repo_ready` `push_mirror_remotes_create_only` call.
- `multi_remote.rs`: new `push_mirror_remotes_create_only` helper.
- `report.rs`: new `EMPTY_REPO` flag, updated `repo_hint`, updated `push_status` derivation.
- `visibility.rs`: fixed `GITLAB_API_PROJECTS` template (one `{}` placeholder).

**Tests:** 758 daemon tests pass (+3 new). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.28 — 2026-07-20 — visibility-flip CLI + codeberg quota opt-in

**Operator-visible changes:**

1. **New `dracon-sync make-public <repo>` and `make-private <repo>` subcommands.** Flip repo visibility across github + gitlab. Codeberg skipped by default (85 GiB grace quota); pass `--include-codeberg` to flip it too. Updates the local visibility cache on success so `repos` reflects the new state immediately.
2. **New repos skip codeberg by default.** Global config `codeberg.auto_create` changed from `true` to `false`. Per-repo opt-in: set `auto_create_on_codeberg = true` in `<repo>/.dracon/dracon-sync.toml`.
3. **GitHub noreply identities whitelisted.** `trusted_emails` in the global config now includes `dracon@users.noreply.github.com` and `DraconDev@users.noreply.github.com` — the GitHub web-UI default identity for known usernames. Fixes the `🚫 unowned` warning on repos authored via the GitHub web editor (e.g. `pi-goal-loop-audit`).

**Internal:**

- `multi_remote.rs:create_repo_on_github` no longer hardcodes `--private`. Honors the `private` parameter so `auto_create_repo(..., private=false)` actually creates a public repo.
- `auto_create_all_remotes` takes a new `codeberg_override: Option<bool>` parameter (per-repo opt-in for codeberg).
- `visibility.rs:flip_repo_visibility(...)` is the new helper for the CLI subcommands. Calls `set_github_visibility` (new), `set_gitlab_visibility` (existing), and `set_codeberg_visibility` (existing) per remote.
- `RepoPolicyOverride.auto_create_on_codeberg: Option<bool>` is the per-repo config field.

**Tests:** 755 daemon tests pass (+2 new). `cargo clippy --workspace --locked -- -D warnings` clean. `cargo deny check` clean.

### v0.112.27 — 2026-07-20 — operator UX: glance view for `repos` (3-column table)

The `repos` command had grown to 16 columns (ROLE, BRANCH, PUBLISH, M/S/U counts, AHEAD, BEHIND, PUSH, PUSH-TO, LAST COMMIT, STATE+ACT, HINT). For the common "is anything broken?" check, this is too noisy.

Fix:
- Added `--summary` / `-s` flag to `repos`: proper 3-column `comfy-table` (STATUS · REPO · WHAT) with UTF8_FULL_CONDENSED borders.
- WHAT = `activity + dirty-counts + push-status-if-stuck + hint` joined by ` · `, truncated to terminal width.
- `#` / `STATUS` / `REPO` columns use fixed `Absolute(N)` widths; `WHAT` uses `Dynamic` to absorb leftover terminal width.
- Works with `--only-concern` / `--only-warn` for "show me just the broken ones".
- Added `--summary-by-severity` to sort concerns first, clean last.
- New helpers: `severity_tier()` (0=concern, 1=warn, 2=active, 3=clean), `summary_what()` (builds WHAT), `print_repos_summary()` (3-col table renderer).
- Fixed R0 duplication bug: `🟣 pushing 0m (1 ahead)` no longer followed by a separate `1 ahead`.

**R1 fix**: operator feedback was "the summary needs to be a table." R0 used `println!` with manual spacing which broke alignment under ANSI color codes. R1 uses `comfy-table` for correct unicode width + ANSI handling.

**R2 fix**: operator feedback "the authors are wrong, we're freestyling some of it" — dropped the author from ALL three `repos` view variants. The author is `git log -1 --format=%an` (git commit author of the latest commit); for a solo operator who freestyles git identities (`DraconDev` / `dracon` / `darklord-dev`), this misleadingly implies multiple people. Removed from: (1) summary WHAT, (2) detailed Compact/Full HINT column suffix (the Compact tier had no dedicated author column — its 23-cell data row was truncated to the 16-col header, so author was only ever visible via the HINT suffix), (3) Vertical tier `author:` line. The `last_author` field is still computed but no longer displayed.

**+7 new regression tests** (`test_summary_what_clean_idle_repo`, `..._dirty_repo_includes_dirty_counts_and_hint`, `..._pending_push_drops_redundant_ahead_note`, `..._stuck_push_shows_status`, `test_severity_tier_ordering`, `test_print_repos_summary_renders_as_table`, `test_summary_what_handles_long_hint_with_word_boundary`). **935 total daemon tests** passing. `cargo build/test/clippy/deny` all green.

Verified:
- `repos -s` at 300 cols → clean 3-col table, headers + borders
- `repos -s` at 120 cols → still fits, REPO + WHAT show full
- `repos -s` at 80 cols → REPO truncates with `…`, WHAT truncates with `…`, no wrap
- `repos -s --only-concern` → filters to concern rows only
- `repos -s --summary-by-severity` → concerns first, clean last

### v0.112.26 — 2026-07-19 — UI polish follow-up (clean STATE+ACT truncation + wider HINT)

After v0.112.25, two cosmetic artifacts were still visible in the `repos` table:

1. **STATE+ACT mid-emoji truncation**: `🟠 dirty · ⏳ …` — the second emoji (⏳) was kept but the trailing text was clipped, leaving a dangling emoji + ellipsis.
2. **HINT column too narrow**: `daemon handles afte…` clipped the operator-friendly phrase mid-word.

Fix:
- New `state_plus_act_cell()` helper drops the activity part **cleanly** when the 15-col budget is tight. State always renders (`🟠 dirty`); activity only when there's room (`🟠 dirty · ⏳ dirty 1h`). State is preserved over activity because state is the actionable classification.
- Widened HINT column from `Absolute(22)` → `Absolute(26)` (budget 20 → 24 cols). Now fits `daemon handles after ch…` instead of `daemon handles afte…`.
- Bumped Compact tier threshold from `< 238` to `< 242` to match the new HINT width.

**+3 new regression tests** (`test_state_plus_act_cell_drops_activity_when_tight`, `..._keeps_activity_when_it_fits`, `..._handles_dash_activity`). **928 total daemon tests** passing. `cargo build/test/clippy/deny` all green.

Verified:
- 240 cols → Vertical (no wrap)
- 300 cols → Compact, all single-line, clean truncation (no `⏳ …` artifacts)
- 400 cols → Full, all single-line

### v0.112.25 — 2026-07-19 — UI render fix follow-up

v0.112.24's Compact-tier table used `LowerBoundary(N)` for REPO/ROLE/PUBLISH/STATE+ACT/HINT — meaning cells could GROW with content but not truncate. On terminals 220-237 cols (just below Compact threshold) the table rendered but variable-length cells wrapped to 2 lines.

Fix:
- Switched REPO/ROLE/PUBLISH/STATE+ACT/HINT (Compact) and REPO/PUBLISH/ACTIVITY/STATE/DAEMON/HINT (Full) to `Absolute(N)` widths
- Added `truncate_unicode_width(..., N-2)` to `role_cell()`, `publish_cell_label()`, REPO name in row loop
- Bumped Compact tier threshold from `< 220` to `< 238` (new column budget)
- Renamed parent label `parent (N submods)` → `parent·N` (9 chars, fits in 14-col ROLE column)

**+1 new regression test** (925 total daemon tests). `cargo build/test/clippy/deny` all green.

Verified:
- 230 cols → Vertical (no wrap)
- 240 cols → Compact, 32 single-line rows, 0 wraps
- 300 cols → Compact, all single-line
- 400 cols → Full, all single-line

**Stalled repos investigation** (neonbreak + endless-td showed `🔴 stalled`): root cause was the user's own `pi-loop` LLM agent repeatedly regenerating `tools/spec-audit.mjs` + `docs/spec-compliance.md` while hitting Anthropic 429 rate limits (160 iterations from 14:54 to 21:06 BST, 13 rate-limit errors, 1 operator_abort at 20:50). The loop is now stopped. Daemon was working correctly — no fix needed.

### v0.112.24 — 2026-07-19 — goal `4555eaf6` (unowned + codeberg-as-main + role layout)

Four operator-visible issues from `repos` table:

1. **hegemon was `🚫 unowned`** (HEAD author `Hegemon Audit <hegemon@local>`, F44 flags when either name OR email is untrusted):
   - Added `hegemon@local` to global `trusted_emails`
   - Amended the 2 audit-script commits on hegemon's main to use canonical `DraconDev` name (was `Hegemon Audit`)
   - Force-pushed to github + gitlab

2. **`opencode-plugins` (PRIVATE) showed `PUBLISH = codeberg/main`** because no `origin` remote existed:
   - New `ensure_origin_for_vscode()` in `multi_remote.rs` adds `origin = github URL` when mirrors exist but origin is missing
   - Never overwrites an existing origin (operator override wins)
   - One-time `git fetch origin` for existing repos

3. **ROLE column was 51 chars wide** for submods:
   - `RoleKind::Submod` now renders just `wip/<name>` (strips `web/games/` prefix)
   - Preserves `wip` vs `released` tier marker
   - Falls back to full sub_path for non-standard layouts

4. **Audit-script identity impersonation**: fixed by the hegemon amend

**+8 new regression tests** (924 total daemon tests). `cargo build/test/clippy/deny` all green.

### v0.112.23 — 2026-07-19 — UI rendering fix

`repos` table layout was broken — cells were wrapping to 2-5 lines per row. Root cause: `LowerBoundary` constraints allowed columns to GROW to fit the longest content (152-char auto-commit subjects), and `truncate_unicode_width()` wasn't being applied to cells.

Fix:
- LAST COMMIT, AUTHOR, STATUS, PUSH-TO: `LowerBoundary` → `Absolute`
  so columns are truly fixed at the listed width
- Every cell with variable-length content: `truncate_unicode_width(..., column - 2)` before passing to comfy-table
- STATUS 11 → 13 cols (so `🚫 unowned` = 11 cols + 2 padding fits)
- Full-tier threshold 300 → 315 cols (sum 287 + 24 borders = 311)
- New regression test: `test_long_commit_subject_truncated_to_last_commit_width`

Truncation budgets (column_width - 2 padding):
LAST COMMIT 17 → 15, PUSH-TO 32 → 30, HINT 15 → 13, ACTIVITY 11 → 9,
AUTHOR 11 → 9, STATE 15 → 13, DAEMON 15 → 13, STATE+ACT 17 → 15.

Test count: 916 (was 915, +1 new). `cargo build/test/clippy/deny` all green.
All 30 data rows now render on single lines.

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
<!-- cache test 1784643652 -->
