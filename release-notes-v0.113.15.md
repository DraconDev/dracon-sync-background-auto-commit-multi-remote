## dracon-sync v0.113.15 — REM icons + last-push age in the rich table

Operator-requested additions to the v2 rich `repos` table.

### Added

- **REM column** — one icon per push remote: 🐙 github · 🦊 gitlab ·
  🗻 codeberg. Bright = the daemon pushes there; dim = excluded from
  auto-push (e.g. junk-runner's policy-excluded codeberg). Unknown
  remote names render as their first two letters, never dropped.
  Column width funded by narrowing REPO/ACTIVITY/TOUCHED inside the
  same 165-col budget. Legend explains the icons.
- **Last-push age in the PUSH cell** — a healthy push now reads
  `✅ OK 5m` / `✅ OK 3h` instead of a bare `✅ OK`, so "when did this
  last actually go out" is visible without opening the detail view.

### Implementation notes

- comfy-table `custom_styling` feature enabled so per-icon ANSI
  dimming is width-safe.
- codeberg icon is 🗻 (U+1F5FB): ⛰/🏔 measure width-1 in
  unicode-width but render 2 cells — that mismatch would break the
  table math.
- Absolute column widths include the 2-cell padding — caught by
  live-render verification when the dim codeberg icon was truncated
  at REM_COL=6.

Upgrade: `cargo install dracon-sync --locked` or your usual path.
