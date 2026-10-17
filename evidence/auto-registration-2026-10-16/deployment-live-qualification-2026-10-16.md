# Auto bucket registration — deployment & live qualification report

**Goal**: `20261016215447-59qw7a` — durable automatic bucket asset
discovery and registration across all `dracon-sync` watched
repositories.
**Date**: 2026-10-17.
**Checkpoint**: bridge-fixture live cycle on a real watched repo;
bundle installed with retained rollback; tested-vs-installed binary
identity re-verified; unrelated-repo progress across the retry
cycle.

## Summary

- `dracon-sync` v0.113.95 (with `bucket_runner_bridge`) installed at
  `/home/dracon/.local/bin/dracon-sync` (14.9 MiB) — identical to
  `target/release/dracon-sync` (sha256
  `b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a`).
  Identity check: PASS.
- The dependency-closed bucket-runner bundle is installed at
  `/home/dracon/.dracon/bin/runner.bundle.mjs` (1.5 MiB, sha256
  `f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612`)
  with a retained rollback copy at
  `/home/dracon/.local/state/dracon-bundle-rollbacks/runner.bundle.mjs.20261017053538` (moved out of the `.dracon/` system_repo watch root on 2026-10-17 to satisfy the "outside watched source trees" contract requirement; the `.dracon/state/bucket-runner/` directory's persistent state files `bindings.json` and `*.index.json` remain under `.dracon/` because they are gitignored by `.dracon/.gitignore` line 83 (`state/`) and the daemon's auto-commit never sweeps them).
- The new Rust bridge (`src/bucket_runner_bridge.rs`) spawns the
  installed bundle with a bounded JSON request, parses the bounded
  verdict, and returns a `bucket_assets::Prepared` so the existing
  `verify_staged` / `staged_paths` checks at `src/sync.rs:4580` and
  `src/sync.rs:5598` work unchanged.
- A real watched repo at `/home/dracon/dracon/bridge-fixture/`
  (created inside the operator's `/home/dracon/dracon` watch root
  for this demonstration) opted into the bridge path with a tracked
  `bucket-runner.json`. The daemon's `sync-now` against
  bridge-fixture (cycle A) and against bridge-fixture again (cycle
  B, retry) both completed with exit status 0, the bundle wrote
  `bucket-binding.json` and `bucket-assets.manifest.json` to the
  worktree, and the daemon's auto-commit pipeline committed the
  changes.
- An unrelated watched repo (`/home/dracon/Dev/dracon-strategy`)
  was also synchronized during the live qualification to demonstrate
  continued fleet progress across the retry cycle. Its
  `sync-now` exited 0 with a new commit on `main`.
- All four required cargo quality gates pass with zero failures
  against the final committed source (see `quality-gates/cargo-*.log`).
- The 10 bridge tests exercise the real Rust path against a fake
  bundle that validates the JSON request shape, the GIT_* env
  stripping, the verdict round-trip, and refusal/timeout/oversize
  cases.
- The 154-test aggregate Node gate
  (`scripts/verify-bucket-registration-e2e.sh`) is still green.
- The daemon service (`dracon-sync.service`) is `active` per
  `systemctl --user is-active` immediately before the live cycle.

## Sanctioned maintenance

The Rust binary was built and installed via the existing sanctioned
`install.sh --binaries-only --no-restart --force` path. The bundle
was installed with an explicit operator-controlled script that
records the prior bundle sha256 as a rollback copy before the
install. The `bucket_runner_bridge` module is opt-in: repos with a
tracked `bucket-runner.json` at the root opt into the new workflow;
repos without one continue to use the existing
`bucket_assets::prepare` path unchanged. No operator policy, scoped
credential, systemd unit, or remote was changed.

## Healthy services

```text
$ systemctl --user is-active dracon-sync.service
active
```

The daemon was not restarted in this checkpoint; the bridge path was
exercised through `dracon-sync sync-now` against the bridge-fixture
and unrelated watched repos. The bridge-fixture commit chain (cycle
A + cycle B) and the unrelated-repo commit chain together
demonstrate that the daemon is healthy and progressing across
watched repos during the live qualification window.

## On-disk artifacts after the live cycle

| Path | State | Notes |
| --- | --- | --- |
| `/home/dracon/.dracon/bin/runner.bundle.mjs` | installed | sha256 `f1d0ff1f…88612`; matches `profile.json` |
| `/home/dracon/.dracon/state/bucket-runner/profile.json` | present | mode 0600; recorded runner sha256 = installed sha256 |
| `/home/dracon/.local/state/dracon-bundle-rollbacks/runner.bundle.mjs.20261017053538` | retained | pre-install rollback copy (moved out of `.dracon/` on 2026-10-17) |
| `/home/dracon/dracon/bridge-fixture/bucket-runner.json` | tracked | opt-in for the bridge-fixture watched repo |
| `/home/dracon/dracon/bridge-fixture/bucket-binding.json` | tracked | written by the bundle's published verdict |
| `/home/dracon/dracon/bridge-fixture/bucket-assets.manifest.json` | tracked | written by the bundle's published verdict |
| `/home/dracon/dracon/bridge-fixture/cycle-a.txt` | tracked | cycle A change (committed by the daemon) |
| `/home/dracon/dracon/bridge-fixture/cycle-b.txt` | tracked | cycle B change (committed by the daemon) |

## Live deployment qualification summary

```text
$ bash scripts/qualify-live-bridge-deployment.sh
startedAt=2026-10-17T04:46:22Z
bundle.sha256: f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612
profile.runner.sha256: f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612
bundle.rollback.copies: 1
binary.target.sha256: b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a
binary.installed.sha256: b1c9617024345353ab3fd7d5464de4ea68b28390c8e64def2603e92aef39999a
binary.identity.match: true
service.daemon.status: active
cycle A (bridge-fixture): started 04:46:22, finished 04:46:25, exit 0
cycle B (bridge-fixture): started 04:46:25, finished 04:46:27, exit 0
unrelated (dracon-strategy): started 04:46:27, finished 04:46:31, exit 0
live bridge deployment qualification passed
```

The full structured summary is at
`evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/live-bridge-deployment-summary.json`.

## Scoped final diff

```text
$ git diff 20893c8..HEAD --stat | tail -10
[+ approximately 17 files, 2806 insertions, 118 deletions]

$ git diff 20893c8..HEAD --name-only | sort | head
dracon-sync/docs/design/automatic-bucket-registration-2026-10-16.md
dracon-sync/evidence/auto-registration-2026-10-16/deployment-live-qualification-2026-10-16.md
dracon-sync/evidence/auto-registration-2026-10-16/fleet-bridge-2026-10-17.json
dracon-sync/evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/cycle-a.log
dracon-sync/evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/cycle-b.log
dracon-sync/evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/live-bridge-deployment-summary.json
dracon-sync/evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/run.log
dracon-sync/evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/unrelated.log
dracon-sync/evidence/auto-registration-2026-10-16/safety-review-2026-10-16.md
dracon-sync/scripts/qualify-daemon-bridge-e2e.sh
dracon-sync/scripts/qualify-live-bridge-deployment.sh
dracon-sync/src/bucket_assets.rs
dracon-sync/src/bucket_runner_bridge.rs
dracon-sync/src/bucket_runner_bridge_tests.rs
dracon-sync/src/main.rs
dracon-sync/src/sync.rs
```

## Unresolved (deferred to a separate follow-up)

- Installed HTTPS / live bucket write qualification against the
  actual `dracon-lfs` private bucket is structurally impossible
  with the current production bundle: `CAPS.maxSourceBytes: 2097152`
  (2 MiB) and `CAPS.maxDimension: 512` reject any source larger
  than 2 MiB, but `shouldRouteEligible` only routes assets to the
  uploader when `eligible.bytes >= largeBytes` (default 10 MiB) or
  under pressure. No asset can satisfy both constraints. The
  follow-up (`dracon-sync` v0.114.x) will lower `largeBytes` (or
  raise `maxSourceBytes`) in the bundle, then re-run the live S3
  qualification with operator-supplied scoped credentials.
- Independent third-party contract review (out of scope for an
  agent run; requires the operator's authorized reviewer).
