## dracon-sync v0.113.42 — stale-dirty pile-up alert

When a watched repo has committable changes whose OLDEST file mtime
exceeds `stale_dirty_alert_secs` (default 600s = 10 min, 0 disables),
the daemon emits a "Changes Piling Up" alert: a journal line plus an
entry in `~/.local/state/dracon/dracon-sync-alerts.jsonl`, throttled
to once per 30 min per repo. The age is mtime-based, so a frozen
daemon or a wedged cycle is surfaced on the first cycle after the
daemon resumes — even when it then commits immediately. Excluded
dirs/files, per-repo `auto_commit_exclude_patterns`, oversized files
(> 100 MiB), and unchanged gitlinks never trigger it. Per-repo
override: `stale_dirty_alert_secs` in `<repo>/.dracon/dracon-sync.toml`.

1228 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
