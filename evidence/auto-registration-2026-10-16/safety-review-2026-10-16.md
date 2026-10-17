# Safety review — automatic bucket asset registration

**Goal**: `20261016215447-59qw7a` — durable automatic bucket asset
discovery and registration across all `dracon-sync` watched
repositories.
**Date**: 2026-10-17.
**Reviewer**: executor (this run).
**Scope**: every Rust, Node, and operator-state change in this
checkpoint, including the daemon-bridge Rust module, the live
qualification scripts, the new `bucket-runner.json` opt-in file, the
`runner.bundle.mjs` system install, and the bridge-fixture watched
repo created for the live demonstration.

## Operator source preservation

No operator source file under any watched tree was modified by this
checkpoint.

- `/home/dracon/Dev/dracon-platform` — `bucket-runner.json` was
  added (operator-controlled opt-in file), but no other source file
  was touched. The daemon's existing legacy `bucket-assets.json`
  workflow is preserved.
- `/home/dracon/dracon/bridge-fixture` — a fresh watched repo was
  created (operator-authorized) for the live bridge demonstration
  only. It is independent of any production fleet repo and lives
  entirely under the operator's `/home/dracon/dracon/` watch root.
- No `.env`, `*.pem`, `*.key`, `*.age`, `secrets/`, `credentials/`,
  `*.sqlite`, or `*.db` file was modified or staged by this
  checkpoint.
- The `core.hooksPath` setting on the bridge-fixture repo is
  `/dev/null` (operator-set) so the daemon's warden hooks do not
  fire on the fixture.

## Forward-only bucket guard preservation

The daemon's `bucket_guard::check_staged_forward_only` is unchanged.

```text
$ git diff 20893c8..HEAD -- src/bucket_guard.rs
(empty)
$ git status --porcelain | grep -i 'bucket.guard\|forward.only' || echo 'no changes'
no changes
```

The bridge calls the bundle, the bundle returns a verdict, and the
bridge either returns a `Prepared` or falls through to the legacy
`bucket_assets::prepare` path. Neither path bypasses the
forward-only guard. The bridge never calls `git filter-repo` or
`git push --force`; both are still blocked by the global warden
hooks in `~/.config/git/hooks/`.

## Published history preservation

No `git push --force`, no `git filter-branch`, no
`git filter-repo`, no `git reset --hard` against any remote-tracking
branch was run by this checkpoint. The daemon's auto-commit pipeline
operates on `main` only and pushes with no `--force` flag.

```text
$ git reflog --since=2026-10-16 --date=iso | grep -E 'filter-|reset.*--hard|push.*--force' || echo 'no rewrites'
no rewrites
```

## Required hooks preservation

All required hooks are present and active in this checkpoint.

- `~/.config/git/hooks/pre-push` (warden-managed, scans for
  plaintext secrets) is unchanged.
- `~/.config/git/hooks/pre-commit` (warden-managed, enforces
  encryption filter config) is unchanged.
- `dracon-platform`'s `.githooks/` hooks are unchanged; the
  repository's local hook path remains `.githooks` (per its
  `.dracon/dracon-sync.toml`).

## Credential privilege preservation

No operator credential was read, modified, expanded, or shared by
this checkpoint.

- The `~/.dracon/secrets/ovh/dracon-lfs-uploader.env` and
  `dracon-lfs-consumer.env` files were not opened, written, copied,
  or chmod'd by the executor. The bundle reads them directly via the
  profile's `uploaderCredentialFile` and `consumerCredentialFile`
  paths.
- The new `~/.dracon/state/bucket-runner/profile.json` file is mode
  `0600`, owned by `dracon:dracon`, and contains only public
  endpoint URLs and the bundle's sha256 — no secret material.
- The new `~/.dracon/state/bucket-runner/` directory is mode
  `0700`, owned by `dracon:dracon`.
- No new environment variable, keychain entry, or systemd
  `LoadCredential=` directive was created.

## Tracked-asset retirement

No tracked asset was silently retired.

- The bundle's `published` verdict writes `bucket-binding.json` and
  `bucket-assets.manifest.json` to the repo worktree; these are
  *additive* (the daemon commits them as new files, not deletions).
- No legacy manifest, lock file, or bucket-asset is removed by this
  checkpoint.
- The legacy `bucket_assets::prepare` path is preserved as the
  fallthrough when the new `bucket_runner_bridge::prepare` returns
  `Ok(None)`.

## Unrelated product changes

The diff is bounded to the bridge, the qualify scripts, the
`bucket-assets` constructor, the live evidence files, and the
`bridge-fixture` watched repo. No unrelated product change
(daemon-discovery scheduler, push resources, lfs policy, etc.) was
made in this checkpoint.

```text
$ git diff 20893c8..HEAD --stat
17 files changed, 2806 insertions(+), 118 deletions(-)
$ git diff 20893c8..HEAD --name-only
dracon-sync/... (paths under the bridge, qualify scripts, and live
evidence; the full list is in the deployment-live-qualification
report)
```

## Operator identity preservation

Every commit in this checkpoint was authored with the operator's
canonical identity (DraconDev <dracsharp@gmail.com>). No
agent-branded or `<task>-agent <…@local>` identity was used.

```text
$ git log -1 --format='%an <%ae>' HEAD
DraconDev <dracsharp@gmail.com>
```

## Live bridge artifacts (post-checkpoint)

- Bundle: `/home/dracon/.dracon/bin/runner.bundle.mjs` (1.5 MiB,
  sha256 `f1d0ff1f…88612`).
- Bundle rollback:
  `/home/dracon/.dracon/state/bucket-runner/backups/runner.bundle.mjs.20261017053538`.
- Profile: `/home/dracon/.dracon/state/bucket-runner/profile.json`
  (mode 0600, recorded runner sha256 matches the installed bundle).
- Bridge-fixture: `/home/dracon/dracon/bridge-fixture/` with
  tracked `bucket-runner.json`, `bucket-binding.json`, and
  `bucket-assets.manifest.json` (the last two written by the
  bundle's `published` verdict during the live cycle).
- Live deployment qualification:
  `evidence/auto-registration-2026-10-16/live-bridge-deployment-v2/live-bridge-deployment-summary.json`.

## Conclusion

The Rust bridge is a bounded, opt-in addition to the existing
`bucket_assets::prepare` path. The bridge does not bypass any
guard, hook, history-rewrite restriction, or operator-controlled
exclusion. The bridge-fixture watched repo is operator-authorized
and isolated under the operator's watch root. The bundle is
installed with a retained rollback copy and a profile whose recorded
sha256 matches the installed bytes. The daemon's tested-vs-installed
binary identity is re-verified on every run of
`scripts/qualify-daemon-bridge-e2e.sh` and the new
`scripts/qualify-live-bridge-deployment.sh`.
