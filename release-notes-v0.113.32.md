## dracon-sync v0.113.32 — daemon-pause warning in `repos`

When the daemon is frozen (`dracon-sync pause` marker or
`DRACON_SYNC_FREEZE`), `repos` now prints a bold-yellow
`── ⏸️ DAEMON PAUSED (<reason>) — nothing is committing or pushing ·
resume: dracon-sync resume ──` line right under the banner, in every
layout tier. A paused daemon used to make every row silently stale —
PENDING pushes never completed, ↑N accumulated fleet-wide — with no
hint as to why. Invisible when not frozen.

1214 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
