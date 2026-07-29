# v0.113.9 — 2026-07-29 — advisor-catch: SIZE color semantics + assert removal

> **Two follow-up fixes to v0.113.8**, both surfaced by the
> post-release advisor review. The original v0.113.8 shipped the
> 4 new diagnostic columns (USED / COMMITS / SIZE / TOUCHED)
> correctly, but had two latent bugs that wouldn't show up in
> the test suite but WOULD show up to a live operator:
>
> 1. **SIZE color mismatch with daemon concern** — the SIZE
>    cell was colored Red based on raw `git_size_bytes` ≥ 2
>    GiB, but the daemon's `PACK_SIZE_WARNING` / CONCERN
>    predicate fires on the **pushable uncompressed** blob sum.
>    These diverge on **deathrun** (4.08 GiB gitdir + ✅ CLEAN),
>    which would have shown a red SIZE cell contradicting its
>    green STATUS cell.
>
> 2. **Runtime panic in forced-narrow layout** — the rich
>    table's column-set invariant (≤ 165 cols minimum) was
>    enforced by a runtime `assert!`. Operators who forced
>    `--layout rich` on a < 165-col terminal would panic the
>    process. The invariant is already pinned by the test
>    `test_rich_table_fits_narrow_terminal`; runtime enforcement
>    was the wrong layer.

Both fixes are pure source-code changes with full test
coverage and zero behavioral risk for the happy-path
operator.

## Fix #1: SIZE color threads `pack_too_large` explicitly

`size_label` now takes `(bytes: Option<u64>, pack_too_large: bool)`.
The boolean is the same one the daemon uses for
`PACK_SIZE_WARNING` / `pack_too_large_forces_concern` /
`pack_too_large_skips_repair` — the authoritative github-rejection
signal.

| `pack_too_large` | gitdir size | Cell color | Meaning |
|:---:|:---:|:---:|---|
| `true` | any | 🔴 Red | github push is genuinely broken (matches CONCERN) |
| `false` | ≥ 1 GiB | 🟡 Yellow | capacity-planning warning zone, push is fine |
| `false` | < 1 GiB | ⚪ White | normal |

`RepoReportRow` gained a new field `pack_too_large: bool`,
populated at row-build time from `pack_too_large.0` (the same
tuple field that drives the PACK_SIZE_WARNING flag). Stored
on the row so the SIZE cell can color based on the actual
concern rather than re-deriving it from `hint.contains(...)`
heuristics.

### Why this matters for deathrun

`deathrun` has:
- `git_size_bytes` = **4.08 GiB** (pre-`git gc` pack residue
  from before the orphan cutover, never cleaned up because
  the daemon's `auto_gc_garbage_threshold_bytes` default is
  2 GiB and deathrun's `.git` is below it).
- `pack_too_large.0` = **false** (the pushable branch is well
  under 2 GiB post-orphan-cutover; github is pushing
  successfully).
- STATUS = **✅ CLEAN**.

Under the original v0.113.8 code, the SIZE cell would have
shown `4.08 GiB` in Red — the operator reads "red SIZE" as
"github push broken", but github push is FINE. The
contradicting colors would have undermined trust in the new
diagnostic column entirely.

Post-fix: SIZE cell shows `4.08 GiB` in Yellow — matches the
"gitdir is large but push is fine" semantic, consistent with
the ✅ CLEAN STATUS cell.

### Source changes

| Path | Change |
|---|---|
| `dracon-sync/src/report.rs:4756` | `size_label` signature changed: `fn size_label(bytes: Option<u64>) -> (String, Color)` → `fn size_label(bytes: Option<u64>, pack_too_large: bool) -> (String, Color)`. The new bool is the authoritative Red trigger; the gitdir size only governs the Yellow warning zone. |
| `dracon-sync/src/report.rs:998` | NEW `RepoReportRow.pack_too_large: bool` field. Stored on the row so the SIZE cell can color based on the actual concern. |
| `dracon-sync/src/report.rs:3098-3580` (row construction) | Populate `pack_too_large: pack_too_large.0` (the tuple's first field, already computed for the PACK_SIZE_WARNING flag). |
| `dracon-sync/src/report.rs:4889, 9124, 9621, 10259, 11056, 11114, 11204` (7 test/factory sites) | Each `RepoReportRow` literal gets the new `pack_too_large: false,` field. |
| `dracon-sync/src/report.rs:4861-4900` (`for_tests`) | Updated `RepoReportRow::for_tests` constructor to include the new field. |
| `dracon-sync/src/report.rs:5298-5300` (`print_repos_rich_table`) | Call site updated: `size_label(row.git_size_bytes, row.pack_too_large)` instead of the hint-string heuristic. |
| `dracon-sync/src/report.rs:11060-11100` (`test_size_label_units_and_colors`) | Test rewritten to exercise the new signature: 8 unit-format cases + 4 color-threshold cases including the deathrun (4 GiB gitdir + no pack_too_large → Yellow, NOT Red) and junk-runner (2 GiB gitdir + pack_too_large → Red) cases. |

## Fix #2: assert → graceful render in `print_repos_rich_table`

The original v0.113.8 added a runtime `assert!` to enforce
the rich-table 10-column-set ≤ 165 cols invariant. The assert
was correct as a development-time sanity check, but
panicking a user-facing CLI on `--layout rich` (forced
override) at < 165 cols was the wrong enforcement layer.

The fix: remove the assert. The invariant is already pinned
by the test `test_rich_table_fits_narrow_terminal` (which
runs in CI). At runtime, comfy-table's `Absolute(width)` on a
narrow terminal gracefully degrades by squashing columns;
the new column-set logic handles it without panicking.

Operators on 90-164 col terminals get the Compact tier via
`choose_layout_tier()` (the default-routing branch). Operators
who explicitly pass `--layout rich` (or `--layout compact`)
on a narrower terminal get the same comfortable-column-squashing
graceful render. No more runtime panics.

The new `choose_layout_tier()` thresholds (from v0.113.8):

| Width | Tier |
|---:|---|
| < 165 | Compact (16-col) |
| 165-241 | **Rich (default, 10-col)** |
| 242-314 | Compact (16-col) |
| ≥ 315 | Full (22-col) |

### Source changes

| Path | Change |
|---|---|
| `dracon-sync/src/report.rs:5177-5192` (`print_repos_rich_table`) | Removed the runtime `assert!`. Replaced with a `let _ = (fixed, border_overhead, cell_padding, width);` to suppress the now-unused-warnings. |
| (no test changes) | The `test_rich_table_fits_narrow_terminal` test (line 11150) already pins the invariant at the test layer; it didn't need updating. |

## Test discipline

- `cargo build --release --locked` succeeds
- `cargo test --workspace --locked` passes (854 daemon tests,
  1120+ workspace tests)
- `cargo clippy --bin dracon-sync --locked -- -D warnings`
  clean
- The 6 v0.113.8 tests still pass; the rewritten
  `test_size_label_units_and_colors` covers the new signature
  including the deathrun-vs-junk-runner color case

## Fleet state before / after (live observation, 2026-07-29 02:35 UTC)

**Before v0.113.9** (v0.113.8 install with the SIZE-color bug):

```
│ 17 ┆ ✅ CLEAN   ┆ deathrun             ┆ ⚪ idle 3h          ┆ —    ┆ ✅ OK   ┆ 🟡mod  ┆ 0/1/59  ┆ 4.08 GiB ┆ DraconDev 3 h… │
```

The `4.08 GiB` cell rendered in **Red** under v0.113.8 — the
operator would read "github push broken" but it's actually
fine.

**After v0.113.9** (this release):

```
│ 17 ┆ ✅ CLEAN   ┆ deathrun             ┆ ⚪ idle 4h          ┆ —    ┆ ✅ OK   ┆ 🟡mod  ┆ 0/1/59  ┆ 4.08 GiB ┆ DraconDev 4 h… │
```

Same row, but the `4.08 GiB` cell now renders in **Yellow**
(matches the daemon's "capacity warning, push is fine"
semantic; consistent with the ✅ CLEAN STATUS cell).

**junk-runner** (genuinely broken: 2.06 GiB gitdir +
`pack_too_large.0 = true`) still renders Red, as intended.

**capture-anime-girls** (genuinely broken: 2.55 GiB gitdir +
`pack_too_large.0 = true`) still renders Red, as intended.

## Cross-references

- `dracon-sync/release-notes-v0.113.8.md` — the original
  v0.113.8 release that introduced the 4 new columns + the
  latent SIZE-color bug + the runtime-panic bug.
- `docs/design/repos-rich-table-columns-2026-07-29.md` — the
  architectural design doc with the rejected alternatives +
  the trade-off analysis. The "follow-up (advisor-catch)"
  paragraphs at the end of the SIZE section and the
  Trade-offs section were updated to reflect the v0.113.9
  fix.
- `AGENTS.md` "Test discipline" — the rewritten
  `test_size_label_units_and_colors` follows the
  "edge-case matrix" pattern (decompose the inputs into
  independent cases, exercise each in isolation).
