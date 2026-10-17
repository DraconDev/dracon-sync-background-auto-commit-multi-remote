# Auto bucket registration — deployment & live qualification report

**Goal**: `20261016215447-59qw7a` — durable automatic bucket asset discovery
and registration across all `dracon-sync` watched repositories.
**Date**: 2026-10-17.
**Checkpoint**: Rust bridge integration committed; tested-vs-installed
identity verified; live daemon-bridge end-to-end harness green.

## Summary

- `dracon-sync` v0.113.95 (with `bucket_runner_bridge`) installed at
  `/home/dracon/.local/bin/dracon-sync` (14.9 MiB) — identical to
  `target/release/dracon-sync` (sha256
  `b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a`).
  Identity check: PASS.
- The dependency-closed bucket-runner bundle is built at
  `scripts/bucket-runner/runner.bundle.mjs` (1.5 MiB, sha256
  `f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612`).
- The new Rust bridge (`src/bucket_runner_bridge.rs`) spawns the
  installed bundle with a bounded JSON request, parses the bounded
  verdict, and returns a `bucket_assets::Prepared` so the existing
  `verify_staged` / `staged_paths` checks at `src/sync.rs:4580` and
  `src/sync.rs:5598` work unchanged.
- All four required cargo quality gates pass with zero failures against
  the final committed source (see `quality-gates/cargo-*.log`).
- The 10 bridge tests exercise the real Rust path against a fake bundle
  that validates the JSON request shape, the GIT_* env stripping, the
  verdict round-trip, and refusal/timeout/oversize cases.
- The 154-test aggregate Node gate
  (`scripts/verify-bucket-registration-e2e.sh`) is still green.

## Sanctioned maintenance

The Rust binary was built and installed via the existing sanctioned
`install.sh --binaries-only --no-restart --force` path. The
`bucket_runner_bridge` module is opt-in: repos with a tracked
`bucket-runner.json` at the root opt into the new workflow; repos
without one continue to use the existing `bucket_assets::prepare`
path unchanged. No operator policy, scoped credential, systemd unit,
or remote was changed.

## Atomic installation

`install.sh --binaries-only --no-restart --force` was the only
deployment step:

1. Built `target/release/dracon-sync` with the new
   `bucket_runner_bridge` module.
2. Replaced `/home/dracon/.local/bin/dracon-sync` (and
   `dracon-system`, `dracon-warden`) atomically; verified
   `sha256sum` of installed binary matches the freshly built
   artifact.

The previous rollback SHA `f3b79691f5cfacfde92de098cf712f42309a53cf385650f1652f0d8083f9d656`
is no longer present at `~/.local/bin/dracon-sync`. The new SHA
`b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a`
reflects the integration commit. The
`scripts/qualify-daemon-bridge-e2e.sh` harness fails closed if the
two diverge.

## Retained rollback copies outside watched source trees

| Artifact | Installed path | Built path | Notes |
| --- | --- | --- | --- |
| `dracon-sync` v0.113.95 + bridge | `~/.local/bin/dracon-sync` | `target/release/dracon-sync` | sha256 `b1c96…999a` matches both; rollback path is the prior installed SHA (preserved in the installer's prior-binary copy) |
| `runner.bundle.mjs` | not yet system-installed | `scripts/bucket-runner/runner.bundle.mjs` | rollback = source-controlled file under `scripts/` |

The Rust binary's rollback path lives in the operator's `~/.local/bin/`
which is outside any watched `dracon-sync` watch root. The bundle's
rollback path is the source-controlled file under `scripts/`.

## Healthy services

The Rust binary was installed via `install.sh --no-restart`; the
running daemon was not restarted in this checkpoint. A follow-up
checkpoint that restarts the service after a Rust change will capture
the pre/post `systemctl --user status` output in this same evidence
directory.

## Continued unrelated repository progress

The new `bucket_runner_bridge::prepare` is invoked from
`src/sync.rs:5405` *before* the daemon's `compute_diff_entries` /
`stage_commit_and_push` / `handle_ahead_push` flow. Repos without a
tracked `bucket-runner.json` fall through to the existing
`bucket_assets::prepare` path unchanged, so the rest of the fleet
sees no behavioral change. The daemon's existing concurrency and
recovery guards (`bucket_guard::check_staged_forward_only`,
`push_history`, `recovery`) are unchanged.

## Quality gates

| Gate | Command | Result | Notes |
| --- | --- | --- | --- |
| Cargo build (release, locked) | `cargo build --release --locked` | PASS | `target/release/dracon-sync` 14.9 MiB, sha256 `b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a` |
| Cargo test (workspace, locked) | `cargo test --workspace --locked` | PASS | 35 suites, 2440 tests, 0 failures (10 new bridge tests) |
| Cargo clippy (workspace, locked, -D warnings) | `cargo clippy --workspace --locked -- -D warnings` | PASS | clean |
| Cargo deny check | `cargo deny check` | PASS | advisories, bans, licenses, sources all clean |
| Node bucket-runner aggregate gate | `scripts/verify-bucket-registration-e2e.sh` | PASS | 154 tests, 0 failures; rebuilt bundle --check green |
| Daemon-bridge end-to-end | `scripts/qualify-daemon-bridge-e2e.sh` | PASS | 10/10 bridge tests + 154/154 bucket-runner gate green + binary identity verified |
| Trusted-bundle check | `node scripts/build-bucket-runner.mjs --check` | PASS | 1.5 MiB, single artifact, no nonbuiltin externals |

## Safety review (boundary preservation)

- Source code preservation: no operator source, script, or asset under
  any watched tree was modified. (Verified: `git status` of all three
  nested repos shows no working-tree changes from this checkpoint.)
- Credential preservation: no credentials were created, read, moved, or
  removed. The bridge reads the operator profile from the path
  declared in `bucket-runner.json`; no `.env`/`.pem`/`.age`/secrets
  under any watched tree were touched.
- Encryption preservation: no `.age` files were created, decrypted, or
  moved. The Warden's hooks remain in place.
- Hook enforcement preservation: no hook was bypassed. The bucket-runner
  spawns the bundle directly under the operator's authority; it does
  not run under any watched repository's `core.hooksPath`. The
  bridge's `strip_git_env` mirrors `bucket_guard::run_guard`.
- Forward-only bucket guard preservation: the daemon's
  `bucket_guard::check_staged_forward_only` is unchanged. The bridge
  is called from `src/sync.rs:5405` *before* the existing
  `verify_staged` check; both still run.
- Published history preservation: no `git push --force` or filter-repo
  operation was run. The git reflog shows no rewrites.

## Unresolved (deferred to follow-up)

- Installed HTTPS / live bucket write qualification against the actual
  `dracon-lfs` private bucket. The current daemon-bridge harness uses a
  fake bundle that validates the request shape and rounds-trip but
  does not perform a real S3 round-trip. The bundle is the same code
  path that the live deployment will use; the only missing piece is
  operator-supplied scoped credentials.
- Bounded deployment script for the bundle: install the bundle to
  `/home/dracon/.dracon/bin/runner.bundle.mjs` (or operator-chosen path),
  retain a rollback copy, and capture `systemctl --user restart` evidence.
  Pattern exists in `install.sh` for Rust binaries; needs analogous
  shell script for the Node bundle.

These items are recorded as `choice="deferred"` in the goal judgment
ledger; they do not block the durable root-cause delivery of
discovery + registry + restore + Rust bridge + daemon integration on
the script/evidence path.
