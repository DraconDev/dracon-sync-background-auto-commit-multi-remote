## dracon-sync v0.113.33 — per-repo build_artifact_cleanup opt-out actually works

v0.113.29 added the `build_artifact_cleanup` SyncPolicy field but
forgot the `RepoPolicyOverride` half — per-repo
`.dracon/dracon-sync.toml` files parse into a different struct that
lacked the field, so the opt-out was silently dropped and
ai-auto-writer's `output/` untrack/re-add ping-pong kept running
(~84s cycle), starving pushes ("pushing 78m" PENDING).

The effective value is now resolved like `auto_bump_versions`
(per-repo override → global default), carried on `SyncContext`, with
a regression test pinning the per-repo file → override → resolution
path.

1215 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
