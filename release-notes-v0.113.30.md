## dracon-sync v0.113.30 — full-width flex table + calmer signals

- Rich table now spans the FULL terminal width: REPO flex-grows to
  absorb all columns beyond the 159-col floor (names truncate less).
- TOUCHED shows only the last author — the age was duplicated in
  ACTIVITY.
- A/B ↑N dims while a push is in flight (it's the batch being
  pushed, not stranded work).

1214 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
