## dracon-sync v0.113.18 — leading locks, icon CHANGES, audit batch

### Table polish (operator feedback)

- **Visibility markers moved to the FRONT** of the REPO cell —
  `🔒 name` / `🔓 name` / aligned pad for unknown, so the icons form
  a single vertical column.
- **CHANGES cell is now icon-form** — 📝 modified · 📦 staged ·
  🆕 untracked · 🚫 excluded-by-policy (`📝1🚫3`), `—` when clean.

### Independent audit of the v0.113.15–18 table work — findings fixed

- **M2**: the report now mirrors the daemon's over-2-GiB github skip
  (`pack_too_large` → github excluded from REM) — was latent (no repo
  currently over the limit) but would have shown 🐙 for a skipped
  remote. Helper called once per repo instead of 3×.
- **M3**: legend footer now prints under the rich tier only (it
  documents rich columns); `--legend` unchanged.
- **M1**: rich-table width-test arithmetic corrected (was passing by
  coincidence: omitted CHANGES_COL + double-counted padding; now
  asserts the exact 149-col total).
- **L4**: compact-tier PUSH-TO reason folded into the bracket —
  `github,gitlab [codeberg:quota]` fits the 30-col budget exactly.
- **L7**: A/B cell no-space form + truncation (a 4-digit double count
  could silently clip).
- Test gaps closed: `repo_cell_content` pure-fn tests, CHANGES
  2-digit-count truncation pinning, width-2 verification for all nine
  table icons.

1206 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
