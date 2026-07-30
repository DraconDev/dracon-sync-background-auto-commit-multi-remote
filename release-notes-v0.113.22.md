## dracon-sync v0.113.22 — submodule badge redesign + REM active-only

Operator feedback on v0.113.21:

- **Submodule badge: `🔒└ name`** — the `↳` name suffix is now the
  tree-child glyph `└` placed directly after the privacy lock, so
  all markers form one fixed leading column (same reason the lock
  leads). Fixed 4-cell REPO prefix keeps names aligned across
  nested/standalone/unknown rows, and the badge never truncates.
- **REM is active-only again** — the dim-excluded suffix rendered a
  🗻 on every row under the fleet-wide codeberg quota posture
  (noise, not signal). Excluded remotes are omitted again; codeberg
  now appears on a row only when the repo actually pushes to it.

1213 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
