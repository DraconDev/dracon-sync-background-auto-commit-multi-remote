## dracon-sync v0.113.37 — configure newly authorized Codeberg mirrors

Public repositories that already had GitHub/GitLab remotes could be granted a
fresh public Codeberg authorization without having a local `codeberg` remote.
The create-only discovery path now configures permitted remotes before its
existence probe, allowing authenticated Codeberg auto-provisioning to run.

1224 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
