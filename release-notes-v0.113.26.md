## dracon-sync v0.113.26 — rich table at every width ≥165

Running `repos` on a maximized terminal (242+ cols) silently served
the OLD 16-column compact table — leftover auto-pick bands from the
pre-rich design. Auto-pick is now: `< 165 → Compact`, `≥ 165 → Rich`.
Compact / Full / Vertical remain available via `--layout`.

1213 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
