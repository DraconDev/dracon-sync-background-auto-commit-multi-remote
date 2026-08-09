# dracon-sync v0.113.49 — 2026-08-09

PUSH legend documents all 8 cell labels — close the documentation gap between
the legend and `push_cell_label`.

## What changed

The `repos` legend's PUSH row used to list 5 markers:

```
✅ OK +age · 🟣 push in flight · ❌ FAIL · 🩹 broken history · 🔑 forge token missing
```

But `push_cell_label` (`src/report.rs:5337`) emits **8** distinct cell labels:

| `push_status` value | Cell label |
|---------------------|------------|
| `OK` | `✅ OK` |
| `INTENTIONAL` | `✅ INTENT` |
| `PENDING` | `🟣 PENDING` |
| `PUSH_STUCK` | `🛑 STUCK` |
| `STUCK` | `🛑 STUCK` |
| `FAIL` | `❌ FAIL` |
| `BROKEN` | `🩹 BROKEN` |
| `BLOCKED` | `🚫 BLOCKED` |

Plus the `🩹` and `🔑` markers appended by `push_cell_with_markers` for missing
objects and missing forge tokens respectively.

The legend omitted 4 of these (`🛑 STUCK`, `🩹 BROKEN`, `🚫 BLOCKED`, `✅ INTENT`),
so operators seeing `🛑 STUCK` in the PUSH column had no legend entry to look it
up. The new PUSH row:

```
✅ OK +age · ✅ INTENT · 🟣 PENDING · 🛑 STUCK · ❌ FAIL · 🩹 BROKEN · 🚫 BLOCKED (+🩹 +🔑 markers)
```

Lists every cell label the code emits, with `🩹` and `🔑` noted as appended
markers (so the cell-text-vs-marker distinction is explicit).

## Origin

The `pi-goal-list-loop-audit` cascade finding at 2026-08-09 10:33:47 promoted
the PUSH legend row to an active goal. Reviewing the cell-label list revealed
the gap.

## Tests

New regression test `test_repos_legend_covers_all_push_cell_labels`
(`src/report.rs`):

- Iterates every `push_status` value passed through `push_cell_label` (OK,
  INTENTIONAL, PENDING, PUSH_STUCK, STUCK, FAIL, BROKEN, BLOCKED)
- Asserts each rendered cell label appears in the PUSH legend row
- Asserts the `🩹` and `🔑` markers are documented

This pins the legend as the source of truth: if a new `push_cell_label` arm
is added without updating the legend, the test trips. The existing
`test_repos_legend_lines_fit_min_width` (≤ 120 cols display width) still passes.

**1241 passed, 9 ignored** (+1 regression test). Clippy `-D warnings` clean
(0 new warnings in the touched file). `cargo deny check` clean.

## Operator action

None required. The change is documentation-only — the rendered cell labels
were already correct; the legend was just out of sync. Verify by running:

```bash
dracon-sync repos --legend
```

The PUSH row should now list all 8 labels + the 🩹 and 🔑 markers.
