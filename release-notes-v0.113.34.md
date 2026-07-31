## dracon-sync v0.113.34 — per-repo override coverage tripwire

The v0.113.29→v0.113.33 incident class (a SyncPolicy knob whose
RepoPolicyOverride half was forgotten, silently dropping per-repo
settings in production) is now structurally impossible:

`test_repo_override_field_coverage_tripwire` enumerates both
structs' serde field names and fails `cargo test` when a new field
appears in either struct without an explicit per-repo decision —
add the override half (with merge resolution at the point of use),
or declare the field global-only / override-only in the test's
allow-lists. Allow-list rot is also caught.

Convention documented in the meta-repo AGENTS.md: "per-repo knobs
need BOTH halves".

1217 workspace tests green; clippy/deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
