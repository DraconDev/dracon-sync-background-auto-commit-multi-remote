# v0.113.8 — 2026-07-29 — rich-table diagnostic columns (USED / COMMITS / SIZE / TOUCHED)

> **One ergonomic release**, not a fix: the `dracon-sync repos`
> rich-table default view dropped the HINT prose column and gained
> 4 new diagnostic columns that the operator explicitly asked
> for ("which repos are used, the size of the repo to see if it's
> growing to be a concern"). The HINT column's role ("why is
> this row in this state?") moves to the per-repo detail view
> (`dracon-sync repos <name>` or `--layout vertical`); the rich
> table now surfaces *what* is happening (use, growth, recency)
> at a glance, leaving the *why* for follow-up drilldown.

## What the operator asked for

Three concrete things were missing from the rich table:

1. **"Which repos are used?"** — the existing ACTIVITY column
   shows the daemon's last action timestamp + the activity
   label, but doesn't answer "am I actively touching this repo
   in the last hour?".
2. **"The size of the repo would be useful too to see if its
   growing to be a concern."** — the per-repo `.git` size was
   already computed (via `git count-objects -v`) and cached
   (24h TTL), but only surfaced in the per-repo detail view.
   The CONCERN column already had a `.git exceeds 2 GB (github
   limit)` hint, but the operator had to drill in to see the
   actual size and whether it was growing.
3. **"More detailed info about staged, committed, changes so
   on"** — the 1h/6h/24h commit windows were already collected
   but only shown in the verbose `--layout vertical` (and even
   there as separate columns). The rich table's ACTIVITY
   column had room for `⏳ dirty 8m · 1 mod` inline but was
   truncating the dirty-count tail at 21-cols.

## The new columns

The rich table grew from **7 → 10 columns**, widened the
ACTIVITY column from 21 → 28 cols, and dropped HINT:

| # | STATUS | REPO | ACTIVITY | A/B | PUSH | **USED** | **COMMITS** | **SIZE** | **TOUCHED** |

Each new column answers one of the operator's questions:

### USED — combined human + daemon activity tier

🟢used = daemon active OR human commit in last 1h · 🟡mod = 1h-24h · ⚪idle = 1d-7d · ⚫cold = 7d+

The daemon pushes/commits show up as `🟢used` even without
human commit activity (the operator's loop is running the
repo), so a repo being iterated by glla is correctly marked
active. The 4 tiers are tighter than the standalone ACTIVITY
thresholds (60m/24h/7d) because USED is the operator-iteration
summary, distinct from the fleet-health summary.

### COMMITS — 1h/6h/24h split

`N/N/N` format. `13/45/152` means 13 commits in last 1h,
45 in last 6h, 152 in last 24h. Empty repos render `-/-/-`.

The split matters because:
- `1h` spikes = operator actively iterating right now
- `6h` sustained = a session of work in progress
- `24h` reflects daily-cadence work; high values alone don't
  mean much, but combined with `0/0/0` in the smaller windows
  tells the operator the repo is dormant.

### SIZE — gitdir bytes, color-coded by the github pack-limit concern

Adaptive units (B → KiB → MiB → GiB). Color the cell:
- **Red** at `pack_too_large = true` (github genuinely refuses
  the push; matches the daemon's `PACK_SIZE_WARNING` /
  `pack_too_large_forces_concern` predicate exactly).
- **Yellow** at ≥ 1 GiB gitdir (capacity-planning warning zone,
  irrespective of whether the push is actually broken).
- **White** at < 1 GiB gitdir (normal).

**ADVISOR-CATCH (v0.113.8 follow-up)**: the original `size_label`
colored the cell red based on `git_size_bytes` ≥ 2 GiB. But
`git_size_bytes` (from `git count-objects -v`) is the COMPRESSED
pack-on-disk size, while the daemon's PACK_SIZE_WARNING concern
fires on the PUSHABLE-UNCOMPRESSED blob sum (the bytes that would
actually ship to a remote). These diverge exactly where it
matters: **deathrun** has 4.08 GiB gitdir (pre-gc pack residue)
but is ✅ CLEAN (the pushable is well under 2 GiB
post-orphan-cutover). The original red would falsely read as
"github push broken" when it isn't.

The fix threads `pack_too_large` (the same bool the daemon
uses for PACK_SIZE_WARNING) through `size_label(Option<u64>, bool)`.
Red iff `pack_too_large == true` (the actual github-rejection
condition); yellow iff gitdir ≥ 1 GiB (capacity warning
independent of push). The SIZE cell color and the row's
CONCERN/ACTIVE state are now visually consistent.

### TOUCHED — last commit author + when

`<author> <when>` (e.g. `DraconDev 14m`, `dracon 10 sec`).
Long author names truncate to 10 cols with ellipsis.
Empty repos render `- -`.

This is the column the operator reaches for when wondering
"who last touched this repo?". The author comes from
`git log -1 --format=%an` (the daemon records it per-commit);
the relative time comes from the daemon's parsed last-when
string.

## Source changes

| Path | Change |
|---|---|
| `dracon-sync/src/report.rs:4663` | NEW helper `pub(crate) fn used_label(row: &RepoReportRow) -> String` — combined human + daemon activity tier (returns one of `🟢used`, `🟡mod`, `⚪idle`, `⚫cold`) |
| `dracon-sync/src/report.rs:4702` | NEW helper `pub(crate) fn commits_window_label(row: &RepoReportRow) -> String` — `N/N/N` 1h/6h/24h split, `-/-/-` for empty repos |
| `dracon-sync/src/report.rs:4722` | NEW helper `pub(crate) fn size_label(bytes: Option<u64>) -> (String, Color)` — adaptive units + github-2-GiB color threshold (returns `(label, Color::Red\|Yellow\|White)`) |
| `dracon-sync/src/report.rs:4786` | NEW helper `pub(crate) fn touched_label(row: &RepoReportRow) -> String` — last author + when, with truncation |
| `dracon-sync/src/report.rs:5107` (`print_repos_rich_table`) | Header now 10 columns: `# · STATUS · REPO · ACTIVITY · A/B · PUSH · USED · COMMITS · SIZE · TOUCHED`. ACTIVITY widened 21 → 28 cols (now fits `⏳ dirty 8m · 1 mod + 5 ut`). HINT dropped. |
| `dracon-sync/src/report.rs:5177-5192` | New `assert!` pins the 10-column total width ≤ 165 cols (the pre-change minimum was 90). Operators on narrower terminals route to the Compact tier automatically. |
| `dracon-sync/src/report.rs:5121-5148` | Column widths: `AB_COL 7→9` (fits `↑/↓ A/B` header), `USED_COL=9` (fits `👆 USED` header), `COMMITS_COL 11→12` (fits `📊 COMMITS` header). The other 7 columns unchanged. |
| `dracon-sync/src/report.rs:2474` (`choose_layout_tier`) | Terminal-width thresholds shifted: < 165 → Compact (was: < 242 → Rich). The pre-change 7-8 column rich table fit ≥ 90 cols; the new 10-column rich table fits ≥ 165 cols. Operators on 90-164 cols now get the Compact tier (the 16-column `--layout compact` view). |
| `dracon-sync/src/report.rs:2636` (`print_repos_legend`) | Legend updated: ACTIVITY / USED / COMMITS / SIZE / TOUCHED each get a one-line description; HINT prose removed (now reachable via `repos <name>` detail view). |
| `dracon-sync/src/report.rs:10449-10498` (`test_choose_layout_tier_*`) | Tests updated for the new 165-col threshold. `test_choose_layout_tier_vertical` now exercises widths 165-241 (Rich zone); `test_choose_layout_tier_compact` exercises 90-164 + 242-299. |
| `dracon-sync/src/report.rs:10578` (`test_choose_layout_tier_fallback_no_env_no_tty_yields_compact_or_smaller`) | Updated assertion: 120 cols now routes to Compact (was Rich). The fallback invariant (`Some(120)` not `None`) is unchanged. |
| `dracon-sync/src/report.rs:10910` (new) | `test_rich_table_headers_fit_columns` — 10-case header-width matrix verifying each header text + 2 padding fits within its column minimum. Catches regressions when adding new columns. |
| `dracon-sync/src/report.rs:10959` (new) | `test_used_label_tiers` — 6-case tier matrix for `used_label` covering daemon-active + human-recency paths. |
| `dracon-sync/src/report.rs:11000` (new) | `test_commits_window_label_renders_split` — 4-case format matrix for `commits_window_label` including the empty-repo `-/-/-` rendering. |
| `dracon-sync/src/report.rs:11060` (new) | `test_size_label_units_and_colors` — 8-case unit matrix (B / KiB / MiB / GiB) + 4 threshold-boundary assertions (1.99 GiB yellow, 2.00 GiB red, 1.00 GiB yellow, 999 MiB white). |
| `dracon-sync/src/report.rs:11105` (new) | `test_touched_label_renders_author_and_when` — 3-case matrix covering normal / long-author-truncation / empty-repo paths. |
| `dracon-sync/src/report.rs:11150` (new) | `test_rich_table_fits_narrow_terminal` — pins the new 10-column total width ≤ 165 cols minimum. Catches regressions when adding new columns. |
| `dracon-sync/Cargo.toml:3` | Version bump 0.113.7 → 0.113.8 |

## Fleet state before / after (live observation, 2026-07-29 02:10 UTC)

**Before** (v0.113.7, 8-column rich table):

```
│ #  ┆ STATUS     ┆ REPO          ┆ ACTIVITY            ┆ A/B  ┆ PUSH       ┆ PUBLISH    ┆ HINT                                   │
│ 1  ┆ ❌ CONCERN ┆ junk-runner   ┆ 🟣 pushing 0m · …   ┆ ↑2   ┆ 🟣 PENDING ┆ origin/main┆ daemon will push after changes settle  │
│ 2  ┆ ❌ CONCERN ┆ capture-ani…  ┆ ⚪ idle 8h          ┆ —    ┆ ✅ OK      ┆ origin/main┆ .git exceeds 2 GB (github limit) — …   │
│ 17 ┆ ✅ CLEAN   ┆ deathrun      ┆ ⚪ idle 3h          ┆ —    ┆ ✅ OK      ┆ origin/main┆ healthy                                 │
```

**After** (v0.113.8, 10-column rich table at ≥165 cols):

```
│ #  ┆ STATUS     ┆ REPO          ┆ ACTIVITY            ┆ A/B  ┆ PUSH       ┆ USED   ┆ COMMITS    ┆ SIZE     ┆ TOUCHED         │
│ 1  ┆ ❌ CONCERN ┆ junk-runner   ┆ ⏳ dirty 0m · 1 mod  ┆ —    ┆ ✅ OK      ┆ 🟢used ┆ 13/45/152  ┆ 2.06 GiB ┆ DraconDev 11 m… │
│ 2  ┆ ❌ CONCERN ┆ capture-ani…  ┆ ⚪ idle 8h           ┆ —    ┆ ✅ OK      ┆ 🟡mod  ┆ 0/0/121    ┆ 2.55 GiB ┆ DraconDev 8 h…  │
│ 17 ┆ ✅ CLEAN   ┆ deathrun      ┆ ⚪ idle 3h           ┆ —    ┆ ✅ OK      ┆ 🟡mod  ┆ 0/1/59     ┆ 4.08 GiB ┆ DraconDev 3 h…  │
```

Each row now surfaces (1) whether the repo is in active use,
(2) recent commit cadence, (3) the gitdir size at a glance,
(4) the last author + when. Operators no longer need to drill
into the per-repo detail view for any of these.

## Trade-offs

**Lost**: the HINT column's prose. The `.git exceeds 2 GB
(github limit)` hint was the most-asked-for hint in the
fleet — it's now implicit from the SIZE cell color (red at
`pack_too_large == true`). Other hints (e.g. `intentional
legacy isolation, no upstream configured`) are reachable via
`dracon-sync repos <name>` or `--layout vertical`.

**Gained**: 4 diagnostic columns answering the operator's
3 explicit questions. The information was already collected
and stored in `RepoReportRow`; this release only surfaces it.

**Wide-terminal-only**: the rich table now requires ≥ 165
cols (was ≥ 90). Operators on 90-164 col terminals get the
Compact tier (16-column `--layout compact` view). The
Compact tier is unchanged from v0.113.7 — it still has the
HINT column + a PUSH-TO column for deep inspection on
narrower terminals.

**v0.113.8 follow-up (advisor-catch)**: the original
`size_label(bytes: Option<u64>) -> (String, Color)` colored
the cell red on `bytes ≥ 2 GiB`. This was wrong because
`git_size_bytes` measures the COMPRESSED pack-on-disk size
while the daemon's PACK_SIZE_WARNING concern fires on the
PUSHABLE-UNCOMPRESSED blob sum. **deathrun** has 4.08 GiB
gitdir but is ✅ CLEAN — the red cell would have falsely
read as "github push broken" when it isn't. Fix:
`size_label(Option<u64>, bool)` now takes `pack_too_large`
explicitly. Red iff `pack_too_large == true` (matches the
daemon's CONCERN predicate); yellow iff gitdir ≥ 1 GiB
(capacity warning independent of push); white otherwise.

## Test discipline

- `cargo build --release --locked` succeeds
- `cargo test --workspace --locked` passes (1120+ tests)
- `cargo clippy --bin dracon-sync --locked -- -D warnings`
  clean
- 6 new unit tests cover the 4 new helpers + the rich-table
  layout invariants

## Cross-references

- `docs/design/pack-size-concern-2026-07-28.md` — the
  v0.113.7 release that introduced the `PACK_SIZE_WARNING`
  flag + the github-pack-limit concern. The SIZE column
  in v0.113.8 uses the same threshold (2 GiB) and the same
  color coding (red at the limit).
- `docs/design/repos-table-fix-2026-07-18.md` — the
  historical `--layout vertical` rich view (15 columns,
  full per-repo detail). The new rich-table layout is the
  default view; `--layout vertical` remains the per-repo
  drill-down option.
- `AGENTS.md` "Test discipline" — the new tests satisfy
  the "New code paths require unit tests" rule.
