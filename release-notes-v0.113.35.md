## dracon-sync v0.113.35 — path ownership and public Codeberg mirroring

This release separates local synchronization scope from commit attribution:
repositories beneath configured watch roots are owned by path policy by
default. `owned = false` remains a hard opt-out, while untrusted identities
and foreign origins are surfaced as warnings rather than blocking an owned
path. Pushes still target only the configured operator namespaces.

Visibility handling now queries owned GitHub/GitLab forges as an aggregate:
any positive public result enables a public Codeberg mirror, private-everywhere
repos stop new Codeberg pushes while retaining existing mirrors, and unknown
or failed API results never publish. New GitHub/GitLab provisioning remains
private by default.

The `repos` report no longer labels invalid Git history as an empty repo, and
an ownership-skipped repo displays `BLOCKED` rather than a false active
`PENDING` push. Recovery preflight also refuses a failed history probe.

1224 workspace tests green; clippy and cargo deny clean.
Upgrade: `cargo install dracon-sync --locked` or your usual path.
