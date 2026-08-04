## dracon-sync v0.113.36 — safe public Codeberg creation retry

A missing public Codeberg repository no longer falls through from an
inconclusive existence check to a guaranteed `Push to create is not enabled`
error. Forgejo's definitive response is classified as a missing repository so
the authenticated API creation path can run. If creation remains unavailable,
Codeberg is excluded for that cycle and retried later instead of generating a
push-failure loop.

1224 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
