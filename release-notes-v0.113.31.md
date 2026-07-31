## dracon-sync v0.113.31 — legend on top + freeze-test isolation

- Legend now prints ABOVE the repos table — the terminal auto-scrolls
  to the bottom, so the table itself is what you land on (no more
  scroll-up past the legend every run).
- Freeze tests no longer read the machine's real
  `~/.dracon/dracon-sync.freeze` (HOME isolated to a tempdir) — a
  live operator pause no longer breaks `cargo test`.
- Env-test mutex tolerates poisoning: one panicking env test no
  longer cascade-fails later env tests.

1214 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
