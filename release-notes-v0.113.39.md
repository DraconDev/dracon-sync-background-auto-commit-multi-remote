## dracon-sync v0.113.39 — retry public Codeberg eligibility

Public-only Codeberg provisioning is now kept eligible while a missing mirror's
visibility is private or unknown. A later successful visibility refresh can
authorize creation for a clean repository without requiring a synthetic file
change or manual commit.

1224 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
