# Qualified bucket artifacts and deployment boundary — 2026-10-17

## Prepared, NOT deployed

This item rebuilds and qualifies production artifacts. It does not install,
restart services, publish crates, edit the trusted profile, change credentials,
change IAM, or send OVH/S3 requests. Live deployment, upload/readback and
scheduled retry are separately queued. A --version probe is not a loaded
service or successful enrollment receipt.

Frozen artifacts (outside both configured watch roots, `~/.dracon` and `~/Dev`):

```
~/.local/state/dracon-sync/qualified-bucket/20261017-2b74bb7c84cb/
  dracon-sync
  runner.bundle.mjs
  artifacts.json
```

The daemon artifact is mode 0555; bundle/manifest 0444; directory 0700.
They are ordinary operator-owned files, not an immutable store: ALWAYS verify
against these committed digests rather than trusting their path or version.

| Artifact | SHA-256 |
| --- | --- |
| dracon-sync | `2b74bb7c84cb9572b35b0ee246bdd29a9e9e56d622fe70fbe46779678d4dfdba` |
| runner.bundle.mjs | `352e4de0a452991fa9ed99c9de9afecb633b8399478912a5169c043823893020` |

The daemon still reports 0.113.95; version alone does not distinguish this
build from the older installed one. Source release build uses the workspace's
normal optimized thin-LTO/single-codegen-unit profile, with no RUSTFLAGS/profile
override. The runner rebuild checks dependency closure, pinned esbuild and no
fixture/test inputs. No external node_modules is needed by the frozen runner.

## Final-code gates

Durable receipts: `evidence/bucket-production-prep-2026-10-17/`.
All long commands were bounded, Cargo jobs/test threads serial, compilation
lower-priority (`nice -n 10`). At baseline about 4.3 GiB was available; before
release build about 9.4 GiB was available; 69 GiB disk free initially. No other
processes were killed, memory-capped, caches deleted or daemon service stopped.

- `timeout 600 nice -n 10 env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test --workspace --locked`:
  **2459 passed, zero failed, 25 existing ignored**, across 35 result blocks.
- `timeout 900 nice -n 10 env CARGO_BUILD_JOBS=1 cargo build --release --locked`:
  passed for the whole workspace, normal production profile (4m33s).
- `timeout 240 nice -n 10 env CARGO_BUILD_JOBS=1 cargo clippy --workspace --locked -- -D warnings`:
  passed without warnings/errors.
- `timeout 180 nice -n 10 cargo deny check`: advisories, bans, licenses, sources OK.
- `timeout 30 node scripts/build-bucket-runner.mjs`: rebuilt dependency-closed
  1,578,883-byte runner; --check also verifies exact rebuilt bytes.
- `timeout 420 bash scripts/qualify-daemon-bridge-e2e.sh`: all three actual
  sync-worker scenarios and 157 Node tests pass; no suite recursion.
- `timeout 45 node --test scripts/check-bucket-credential-access.test.mjs`:
  three tests pass, covering positive parsing/no output leaks, invalid modes
  and format, missing/disabled authority and tampered runner digest.

The 25 ignored cases are NOT represented as passes or live-storage coverage.
They include subprocess crash helpers deliberately invoked by parent tests,
legacy security key-state tests, source-Warden/age/HTTPS operational checks and
three clean-PATH forge mocks. Exact names are retained in `ignored-tests.txt`.
Do not run all ignored helpers indiscriminately: some intentionally terminate
their process. No ignore annotations were added/removed for this qualification.

Rust/member manifests and source did not change from the baseline through
these gates. New local-access utility/tests do not affect Rust production code
or runner dependency closure. Source fingerprints, baseline revisions,
artifact manifest, versions, command logs and installed-identity checks are
retained with the receipts. Workspace artifacts other than dracon-sync were
built for the gate only; this runbook does not authorize their deployment.

## Existing credential access, not remote scope proof

```
timeout 20 node scripts/check-bucket-credential-access.mjs \
  --profile "$HOME/.dracon/state/bucket-runner/profile.json"
```

The new local-only preflight uses the real profile/installed-runner verifier
and production S3 credential parser. Both existing uploader and consumer role
files passed ownership/private-mode, safe ancestor, no-follow, bounded stable
read and exact field grammar checks. It constructs clients with a network
handler that refuses/counts every request; **zero requests occurred**. It does
not invoke HEAD/GET/PUT, execute profile-selected code, enumerate a bucket,
read IAM policy or create bindings. It reports role booleans, not key values,
credential hashes, file contents or raw parser/SDK exception bodies.

Existing profile id: `dracon-platform-private-bridge`; bucket namespace root:
`dracon-platform`. Existing credential paths are profile-selected under
`~/.dracon/secrets/ovh/`; no credentials were copied to evidence or artifacts.
Role file names do not prove least privilege: actual OVH read/write scope and
fresh remote byte verification must be checked in the subsequent live items.

## Safe next-item deployment procedure (NOT executed here)

1. Re-read this committed runbook and artifact manifest. Verify both frozen
   hashes above; refuse mismatches. Re-run the local credential preflight.
   Capture fresh installed daemon/bundle/profile hashes and service MainPID.
   Refuse symlink/unowned destinations and validate that state/rollback paths
   remain outside configured watch roots. Do not rebuild inside maintenance.
2. Prepare a pinned deployment script, then run it through
   `dracon-sync maintenance -- bash PINNED_DEPLOY_SCRIPT`. Never use
   `systemctl --user stop dracon-sync.service` or an indefinite hold marker.
   The wrapper must resume even on failure. Pause prevents new cycles; any
   in-flight work observing a transitional bundle/profile pair must fail
   closed, not be given a bypass or a synthetic success receipt.
3. Before replacement, create a new private rollback directory under
   `~/.local/state/dracon-sync/bucket-rollbacks/UTC_TIMESTAMP/`. Save installed
   daemon, bundle and trusted profile there, preserving modes, and record
   hashes. The profile contains pointers, not copied role-file contents.
   NEVER place rollback executables in watched source or ~/.dracon trees.
4. Stage the pinned executable/bundle into unique sibling temp files on each
   destination filesystem, verify hashes and ownership/mode, then atomic rename
   onto `~/.local/bin/dracon-sync` and the profile-selected runner path. Update
   ONLY profile.runner.sha256 to the qualified bundle hash with a private,
   fsynced atomic sibling replacement. Preserve all other profile/namespace/
   credential fields. Do not touch IAM or widen permissions. On any failure
   restore the complete rollback tuple before resuming.
5. Normally restart the user service (restart, never stop), require active
   status, then compare `/proc/MAINPID/exe` to the pinned daemon hash. Check
   installed bundle and trusted profile digest match the pinned pair. Retain
   timestamps/PID/hashes as actual loaded-identity receipts; then separately
   qualify a NEW isolated asset and real remote bytes. Old manifest existence
   or an exit-zero sync alone is not live enrollment evidence.
6. If Warden rejects an artifact, stop publication/deployment at that boundary.
   No hook bypass, blanket exemption, history rewrite, force-push or credential
   substitution. The next item permits only independently reviewed exact
   content-and-match-bound non-secret receipts when genuinely needed.

Rollback uses the same maintenance + sibling/atomic replacement discipline
for ALL THREE saved files, then normal restart and verification of the saved
running/installed hashes. A rollback never deletes immutable remote objects,
retires source assets, restores over local edits, resets Git history or rewrites
manifests to manufacture compatibility. If compatibility refuses, preserve data
and report it rather than bypassing the guards. Live success/rollback must be
qualified and recorded in their own subsequent items.
