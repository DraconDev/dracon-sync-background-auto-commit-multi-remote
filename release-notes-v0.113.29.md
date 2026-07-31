## dracon-sync v0.113.29 — build_artifact_cleanup opt-out

The daemon hard-codes `output/` (and `gen`, `.output`, `*_output`, …)
as build-artifact dirs and untracked + gitignored tracked files under
them every cycle. For ai-auto-writer, `output/` holds the generated
books — the deliverable — and its loop deliberately commits chapters,
producing a ~30-commit/hour daemon↔loop ping-pong that starved
pushes. New per-repo knob: `build_artifact_cleanup = false` in
`.dracon/dracon-sync.toml`. Default unchanged (`true`).

1214 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
