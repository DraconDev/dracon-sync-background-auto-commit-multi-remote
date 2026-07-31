## dracon-sync v0.113.28 — 🚫 only counts true exclusions

The 🚫 column mixed POLICY exclusions (per-repo
`auto_commit_exclude_patterns` like junk-runner's active.jsonl)
with MECHANICS (submodule worktree dirt whose gitlink SHA didn't
move — nothing to commit at the parent; auto-advances when the sub
commits). "Just because they didn't commit why are they counting as
excluded" — right. The mechanics bucket is now separate: still
subtracted from committable counts, never displayed. 🚫 fires only
for true documented pattern exclusions.

1213 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
