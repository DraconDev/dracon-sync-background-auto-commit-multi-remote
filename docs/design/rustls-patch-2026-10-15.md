# rustls vulnerability patch — resolved versions and advisory results

## What was vulnerable

Four rustls advisories applied to rustls 0.23.45:

- RUSTSEC-2026-0346 — `Debug` impls for client session values expose TLS secrets
- RUSTSEC-2026-0347 — buffered application data sent to outer server after ECH rejection
- RUSTSEC-2026-0348 — Brotli certificate decompression can allocate up to 1 GB
- RUSTSEC-2026-0349 — unauthenticated ServerHello can downgrade connections

The fix shipped in rustls 0.23.46 (no RustSec advisory-suppression entries were added anywhere; `deny.toml` `[advisories] ignore = []` remains empty and no Cargo.toml manifest was edited).

## Resolution (parent workspace + both standalone lockfiles)

Parent `Cargo.lock` was already at 0.23.46 (prior health goal). Both standalone
locks still pinned 0.23.45 and are now patched:

| Lockfile | rustls | yoke-derive |
|---|---|---|
| `Cargo.lock` (parent workspace) | 0.23.45 → 0.23.46 | 0.8.3 → 0.8.4 |
| `dracon-sync/Cargo.lock` | 0.23.45 → 0.23.46 | 0.8.3 → 0.8.4 |
| `dracon-system/Cargo.lock` | 0.23.45 → 0.23.46 | 0.8.3 → 0.8.4 |

yoke-derive 0.8.3 is yanked on crates.io (unfetchable on fresh machines, which
breaks `--locked` installs) and the parent workspace already pinned 0.8.4, so
the un-yank is part of this patch rather than extra churn. A yanked warning is
a cargo-deny warning, not an error — tolerating it would have left the
supply-chain defect in place, so it is resolved, not suppressed.

Required lock-metadata repairs (no version changes, forced by Cargo to make
the stale standalone locks consistent with their current manifests):
dracon-sync `tempfile` getrandom dep edge 0.4.3 → 0.3.4 (same 3.27.0);
dracon-system own-package entry 0.112.42 → 0.112.45 plus the already-present
`libc` edge. `dracon-warden` has no rustls in its lockfile and is untouched.

Resolution is lock-only: `grep rustls|yoke|getrandom` across all `Cargo.toml`
matches nothing (all three are transitive), and no manifest or `deny.toml`
policy file changed.

## Advisory and gate results (evidence: `evidence/rustls-patch-2026-10-15/`)

Standalone qualification ran against a `CARGO_MANIFEST_DIR`-separate copy whose
manifests/locks are byte-for-byte identical to the canonical checkout
(verified before each gate run). Parent workspace qualification ran in the
canonical checkout.

| Gate | Result | Log |
|---|---|---|
| `cargo deny --locked check` (workspace) | advisories, bans, licenses, sources all ok; no RUSTSEC lines; yoked warning absent | `deny-workspace-qualified.log` |
| `cargo deny --locked check` (dracon-sync standalone, parent policy via `--config`) | all four categories ok | `deny-sync-standalone-qualified.log` |
| `cargo deny --locked check` (dracon-system standalone, parent policy via `--config`) | all four categories ok | `deny-system-standalone-qualified.log` |
| `cargo test -p dracon-sync --locked` (standalone) | 1,484 passed, 0 failed, 5 summaries | `sync-standalone-tests-qualified.log` |
| `cargo test -p dracon-system --locked` (standalone) | 502 passed, 0 failed, 3 summaries | `system-standalone-tests-qualified.log` |
| `cargo test --workspace --locked` | 2,412 passed, 0 failed, 25 existing ignored, 35 summaries | `workspace-tests-qualified.log` |
| `cargo build --release --locked` (sync + system standalone) | both passed | `*-release-qualified.log` |
| `cargo clippy --workspace --locked -- -D warnings` | passed | `workspace-clippy-qualified.log` |
| `cargo build --release --locked` (workspace) | passed | `workspace-release-qualified.log` |

Resolved versions are recorded in `lock-delta.json`; the full before/after
diff is `standalone-locks.diff`; final byte state is `final-hashes.txt`.
Initial qualified runs that referenced 0.8.3 locks were superseded by the
`*-qualified.log` set after the review caught the version/evidence mismatch;
all `-qualified` logs were produced against the exact byte-state in
`final-hashes.txt`.
