## dracon-sync v0.113.40 — repair lagging mirrors from clean repos

The daemon now detects a configured GitHub/GitLab/Codeberg tracking ref that is
behind local `main` even when the primary `origin` is already current. It
performs the normal mirror push path, while divergent or remote-ahead refs do
not trigger retry churn and remain protected by the no-history-rewrite policy.

1225 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
