# dracon-sync v0.113.12 — repos table legend, printed by default

> **Date**: 2026-07-29
> **Source**: pi-goal-list item 4 — "make the table more informative"
> (operator picked: built-in legend footer).

## What changed

`dracon-sync repos` now prints its **legend under every table by
default**. The operator was confused by the v0.113.8 columns (USED,
COMMITS, TOUCHED, A/B) even though a `--legend` flag existed since
2026-07-08 — an explanation you have to remember to ask for doesn't
explain.

The legend text itself was **rewritten for accuracy**: the 2026-07-08
original referenced columns that no longer ship (MOD, PUSH-TO,
"Daemon =") and predated the v0.113.8/9 redesign. The new text covers,
one line each:

- **STATUS** — ✅ CLEAN / 🔄 ACTIVE / 🟡 WARN / ❌ CONCERN
- **ACTIVITY** — ⏳ dirty Nm · k mod/stg/ut · 🟢 synced · ⚪ idle · ⚫ cold
- **A/B** — commits ahead/behind upstream (↑ = unpushed work)
- **PUSH** — ✅ OK / 🟣 PENDING / ❌ FAIL
- **USED** — 🟢used <1h · 🟡mod 1h-24h · ⚪idle 1d-7d · ⚫cold 7d+
- **COMMITS** — commits in last 1h/6h/24h
- **SIZE** — white <1 GiB · 🟡 ≥1 GiB watch zone · 🔴 ≥2 GiB = over
  github's pack limit (push skipped)
- **TOUCHED** — author + age of the most recent commit

## Behavior details

- **Width-gated**: suppressed on terminals < 120 columns (where the
  compact tier prints) rather than wrapping brokenly; verified at 100
  cols (suppressed, table intact) and 240 cols (legend under table).
- **`repos --legend`** still prints the key unconditionally on demand.
- Tests pin column coverage, the color semantics, the shipped COMMITS
  windows (1h/6h/24h), and that every line fits the width gate.

874 daemon tests + clippy `-D warnings` green. Released with the
hardened v0.113.11 `release.sh` (second consecutive fully unattended
run).
