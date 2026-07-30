## dracon-sync v0.113.25 — visibility sweep, 🌍 public icon, table-legend

- **Periodic visibility sweep**: the GitHub visibility probe only
  ran inside `sync_repo`, but the daemon fast-path skips clean+synced
  repos — so idle repos with a pruned/missing cache showed a blank
  REPO icon FOREVER (pi-length-continue, bookmarks-new-tab,
  folder-auto-banner-fab). A spawned sweep now refreshes stale caches
  for all watched repos on the policy interval.
- **Public icon 🔓 → 🌍**: the padlocks differed by a 2-pixel
  shackle gap; the globe reads "public to the world" instantly.
- **Legend is now a comfy-table** matching the main table's style:
  label + meaning columns, blank rows between groups, 120 cols.

1213 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
