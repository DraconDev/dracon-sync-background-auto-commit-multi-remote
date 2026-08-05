## dracon-sync v0.113.41 — retain completed discovery grace

A repository that has completed the post-discovery 15-second safety grace is
now marked initialized and stays eligible for ordinary sync work. Successful
clean syncs no longer remove that marker and starve mirror pushes by restarting
the grace period on every daemon cycle.

1225 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
