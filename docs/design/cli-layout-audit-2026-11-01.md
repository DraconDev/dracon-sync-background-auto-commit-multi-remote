# CLI table layout audit — 2026-11-01

## Why

`dracon-sync repos` showed `📤 PUBLISHING` wrapped onto a second line, which
reads as an extra table row. The cause was a hand-tuned `Fixed(13)` STATUS
column while that label is exactly 13 display columns. The question was whether
the same defect class existed elsewhere in the CLI.

## What the audit found

Four `repos` renderers (rich, compact, full, summary) plus four `main.rs`
tables. Existing layout tests only checked **header** widths and total width, so
a *cell* wider than its column was untested — that was the whole gap.

A new invariant test, `every_table_tier_renders_single_line_rows`, renders every
tier with worst-case vocabulary (long subjects/authors/hints/branches, 6-digit
counts, TB-scale sizes, `🟣 PENDING 59m`) and asserts no body line appears
without a row number.

Defects it found and the fixes:

| # | Defect | Fix |
|---|---|---|
| 1 | Rich STATUS `Fixed(13)` vs `📤 PUBLISHING` = 13 cols | width derived from `ALL_STATUS_LABELS` |
| 2 | Same hardcoded 13 in compact, full and summary (4 copies) | all use `status_column_width()` |
| 3 | Full tier's LAST COMMIT cell was untruncated although its constraint comment said "truncate cell content, not wrap" | `commit_summary_cell(..., 15)` |
| 4 | Full tier PUSHED `Fixed(11)` (9 content) held `shorten_when(last_push)`, which can be 14 cols | truncated to 9 |
| 5 | Full tier MOD/STG/UT/AHEAD/BEHIND/1h/6h/24h rendered raw integers; 6 digits overflowed UT's 5-col budget | `format_compact_count`, as the compact tier already did |
| 6 | A test mirrored the rich constants with a stale `STATUS_COL = 12` | mirror uses the derived value |

Structural changes that prevent recurrence:

- Status labels are named once (`current_health::LABEL_*` + `ALL_STATUS_LABELS`);
  `status()` and layout sizing both read them, so they cannot drift.
- `status_column_width()` derives the column from the widest label.
- `RICH_TIER_MIN_COLS` is a single constant used by `choose_layout_tier()` and by
  the floor tests. It moved **165 → 168** because the derived STATUS column needs
  2 more columns; terminals at 165–167 now fall back to Compact instead of
  rendering a malformed table.
- `print_repos_{compact,full,summary}` were split into
  `build_repos_{compact,full,summary} -> Table` plus a one-line print wrapper, so
  every tier is now assertable. Previously nothing could test them at all.
- `size_cell_text` truncates the combined `own+mods` label to `SIZE_CELL_BUDGET`
  (9), so a superproject with two huge packs (`9198G+9198G` = 11 cols) cannot
  wrap the SIZE cell.

## Not affected

The four `main.rs` tables (status, health, doctor, ownership) set **no** column
constraints and use `ContentArrangement::Dynamic`/`DynamicFullWidth`; they size
to content and wrap only to fit the terminal, which is intended for long
key/value cells. `ColumnConstraint` appears 0 times in `main.rs`. The defect
class here — a *fixed* width narrower than its content — cannot occur there.

## Verification

- `every_table_tier_renders_single_line_rows`: all four tiers clean.
- Negative controls (each reverted, then restored): restoring the index-only
  status width reproduced `│ 1  📤` + `│ PUBLISHING`; restoring the untruncated
  PUSHED/UT cells made the full-tier audit fail.
- Live CLI at compact/full/summary: 36 data rows, **0** wrapped lines each.
- Workspace: 44 suites, **2982 passed / 0 failed**; strict Clippy, release build
  and `cargo deny` all exit 0.
- Three pre-existing tests that pinned the old literal `165` now assert against
  `RICH_TIER_MIN_COLS`, so the floor can only change deliberately.
