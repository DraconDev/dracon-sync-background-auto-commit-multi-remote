## dracon-sync v0.113.38 — classify Forgejo missing repositories

Codeberg's SSH response `Cannot find repository` is now treated as a
definitive missing repository. Public mirror auto-provisioning can therefore
reach the authenticated Codeberg API creation path instead of remaining in
an inconclusive existence-check retry state.

1224 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
