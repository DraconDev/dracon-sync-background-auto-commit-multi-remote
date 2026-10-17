# Auto bucket registration — deployment & live qualification report

**Goal**: `20261016215447-59qw7a` — durable automatic bucket asset discovery
and registration across all `dracon-sync` watched repositories.
**Date**: 2026-10-17.
**Auditor scope**: scripts/evidence-only deployment and gate qualification.
Rust bridge integration and live bucket write are deferred to a follow-up
goal (see `integration-seams-next.md`).

## Summary

- `dracon-sync` v0.113.95 installed at `/home/dracon/.local/bin/dracon-sync`
  (14.9 MiB) — identical to the freshly built `target/release/dracon-sync`
  (sha256 `f3b79691f5cfacfde92de098cf712f42309a53cf385650f1652f0d8083f9d656`).
  Identity check: PASS.
- The dependency-closed bucket-runner bundle is built at
  `scripts/bucket-runner/runner.bundle.mjs` (1.5 MiB, sha256
  `f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612`).
  It is not yet installed system-wide: the daemon integration lives in
  follow-up work.
- All four required cargo quality gates pass with zero failures against
  the final committed source (see `quality-gates/cargo-*.log`).
- The 154-test aggregate Node gate (`scripts/verify-bucket-registration-e2e.sh`)
  is green: the bucket-runner implements discovery, capabilities, consumer
  text qualification, digest-bound revalidation, measured-budget routing,
  trusted-bridge protocol CLI, dependency-closed bundling, plus the new
  P1 recovery for stranded index-ahead transactions and the local-only
  versus bucketed suppression list.
- Three independent review reports (storage, consumer, runner protocol)
  all concluded with no implementation blockers. Three P1 review findings
  on the trusted-bridge runner were fixed durably; three P2 cleanup items
  were also fixed durably. All fixes are covered by regression tests that
  run inside the bounded gate.

## Sanctioned maintenance

The cargo binary was built and verified inside the source checkout
(`target/release/`). It was NOT installed system-wide in this checkpoint
because:

1. The source committed at HEAD did not change any `*.rs` file relative to
   the previously installed v0.113.95 (verified with
   `git diff 23de743..HEAD -- '*.rs' Cargo.toml deny.toml` → empty).
2. The bucket-runner is a Node tool, not a Rust binary; it has no
   installation step in the existing `install.sh` and would require a
   separate sanctioned deployment script (out of scope for this script/
   evidence-only checkpoint).
3. No changes to the daemon, the systemd units, the operator policy, the
   scoped credentials, or the OVH/Codeberg/GitHub remotes are required
   for the script/evidence half of the verification contract.

The previously installed binary (`/home/dracon/.local/bin/dracon-sync`)
is byte-identical to the freshly built artifact, so the rolled-back path
is the existing install. A retained rollback copy is at
`/home/dracon/Dev/dracon-utilities/target/release/dracon-sync` (parent
target directory, outside any watched source tree).

## Atomic installation

The Rust binary builds deterministically into
`/home/dracon/Dev/dracon-utilities/target/release/dracon-sync`. The
existing `install.sh` flow is the sanctioned atomic installer; this
checkpoint did NOT invoke it because the source is unchanged. A future
deployment script for the bucket-runner bundle (out of scope here) will
follow the same atomic pattern: copy into a new directory, symlink, then
remove the old symlink in a single transaction, with a rollback copy
held in `~/dracon/rollback/` outside watched trees.

## Retained rollback copies outside watched source trees

| Artifact | Built path | Rollback path | Notes |
| --- | --- | --- | --- |
| `dracon-sync` v0.113.95 | `target/release/dracon-sync` | `/home/dracon/.local/bin/dracon-sync` | byte-identical; same sha256 |
| `runner.bundle.mjs` | `scripts/bucket-runner/runner.bundle.mjs` | not yet installed | rollback = source file under `scripts/` |

The Rust binary's rollback path lives in the operator's `~/.local/bin/`
which is outside any watched `dracon-sync` watch root. The bundle's
rollback path is the source-controlled file under `scripts/`.

## Healthy services

`dracon-sync` was not restarted in this checkpoint (no source change to
restart for). The currently running service (if any) is the previously
installed v0.113.95. A follow-up checkpoint that restarts the service
after a Rust change will capture the pre/post `systemctl --user status`
output in this same evidence directory.

## Continued unrelated repository progress

This checkpoint did not run a fleet-wide daemon cycle because:

1. The Rust source is unchanged from the previously installed version;
   the daemon's behavior is identical to what is already running.
2. The bucket-runner is a script/evidence tool, not yet wired into the
   daemon, so its activation does not require a service restart.

A follow-up checkpoint after the Rust bridge integration will run
`dracon-sync health` and `dracon-sync repos` before/after a full
restart and capture the output in this evidence directory.

## Quality gates

| Gate | Command | Result | Notes |
| --- | --- | --- | --- |
| Cargo build (release, locked) | `cargo build --release --locked` | PASS | `target/release/dracon-sync` 14.9 MiB, sha256 `f3b79691f5cfacfde92de098cf712f42309a53cf385650f1652f0d8083f9d656` |
| Cargo test (workspace, locked) | `cargo test --workspace --locked` | PASS | 35 suites, 2430 tests, 0 failures |
| Cargo clippy (workspace, locked, -D warnings) | `cargo clippy --workspace --locked -- -D warnings` | PASS | clean |
| Cargo deny check | `cargo deny check` | PASS | advisories, bans, licenses, sources all clean |
| Node bucket-runner aggregate gate | `scripts/verify-bucket-registration-e2e.sh` | PASS | 154 tests, 0 failures; rebuilt bundle --check green |
| Trusted-bundle check | `node scripts/build-bucket-runner.mjs --check` | PASS | 1.5 MiB, single artifact, no nonbuiltin externals |

## Safety review (boundary preservation)

- Source code preservation: no operator source, script, or asset under
  any watched tree was modified. (Verified: `git status` of all three
  nested repos shows no working-tree changes from this checkpoint.)
- Credential preservation: no credentials were created, read, moved, or
  removed. The `restore.mjs` and `runner.mjs` create-only test fixtures
  use `/dev/null`/tmp paths only.
- Encryption preservation: no `.age` files were created, decrypted, or
  moved. The Warden's hooks remain in place.
- Hook enforcement preservation: no hook was bypassed. The bucket-runner
  spawns the bundle directly under the operator's authority; it does not
  run under any watched repository's `core.hooksPath`.
- Forward-only bucket guard preservation: the daemon's
  `bucket_guard::check_staged_forward_only` is unchanged. The new
  bucket-runner protocol CLI does not bypass it (the runner is invoked
  *by* the daemon's `bucket_assets::prepare` or its successor; the guard
  runs on the daemon side).
- Published history preservation: no `git push --force` or filter-repo
  operation was run. The git reflog shows no rewrites.

## Unresolved (deferred to follow-up)

- Rust bridge integration of the trusted-bridge runner protocol
  (`bucket_assets::prepare` and its successors must spawn `runner.bundle.mjs`
  with the bounded request schema and parse the bounded verdict JSON).
- Installed HTTPS / live bucket write qualification against the actual
  `dracon-lfs` private bucket (the current 154-test gate is in-process
  only; no live S3 round-trip has been executed).
- Bounded deployment script for the bundle: install the bundle to
  `/home/dracon/.dracon/bin/runner.bundle.mjs` (or operator-chosen path),
  retain a rollback copy, and capture `systemctl --user restart` evidence.

These items are recorded as `choice="deferred"` in the goal judgment
ledger; they do not block the durable root-cause delivery of
discovery + registry + restore on the script/evidence path.
