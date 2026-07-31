## dracon-sync v0.113.27 — blank-for-public, banner header

- **Public = blank**: only private repos carry 🔒 in the REPO cell;
  public/unknown rows pad so names still start at the same column
  on every row. Retires the 🌍 globe (glyph centering looked
  off-column). Legend updated.
- **Single banner header**: `── dracon-sync repos ── 📦 36 · ✅ 33
  clean · 🔄 3 active · 🟡 0 · ❌ 0 · ⛔ 0 ───…` replaces the
  two-line 📜+📦 top. The config-path line no longer prints by
  default.

1213 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
