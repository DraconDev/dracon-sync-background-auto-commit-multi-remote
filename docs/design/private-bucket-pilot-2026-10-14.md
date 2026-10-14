# Private bucket workflow pilot — operator report

Durable references for implementation and reproduction. Committed paths are
relative to their own repositories: `dracon-sync/...` is the utilities
workspace, `web/...` and `bucket-assets.json` are dracon-platform.

## Boundaries and selected pilot

Pilot: shared games hub (`web/games`) in dracon-platform, using its existing
`web/config/bucket-manifests/games.json` brand-favicon declaration and generator.
No nested game is opted in. No Gitea/LFS endpoint or second pointer/reference
format. Preserve the existing public music executor and disabled legacy ops.

Private storage uses `dracon-lfs`, London UK. Existing schemaVersion=1
manifest fields remain authoritative: repository, namespace, sourcePath,
logicalPath, objectKey, sha256, bytes, regenerable, visibility, generator.
Only this bucket permits visibility=private. The public promotion planner
refuses it, rather than silently emitting inaccessible URLs.

The private build workflow restores assets into explicitly ignored source
locations and a content-addressed cache. Build outputs may contain the restored
public favicon; source bucket access stays private, with no credential-bearing
browser URLs. Source/config/docs are never silently declared eligible.

## Routing and publication invariants

Default large eligible asset threshold: 10 MiB. Pressure threshold: 80% of the
applicable measured projected-history budget, configurable. The GitHub 2 GB
per-push ceiling is a separate constraint from the existing operator-growth
budget; neither a table's .git size nor the 5.5 GiB growth budget proves fresh
remote recoverability. Use a documented provider ceiling where known and
explicit operator budgets for unmeasured providers; expose the basis in reports.
Unknown or failed measurement refuses routing rather than pretending zero.

Eligibility requires an explicit regenerable binary manifest entry with a
single-line generator declaration. Private/public visibility must match the
storage binding. Pressure never authorizes uploading source or credentials.
Large eligible assets route from first appearance; existing historical blobs
are retained. Once routed, every future version must remain bucket-backed,
even if the current byte size shrinks below the threshold.

Upload snapshots file bytes, uses full SHA256 immutable keys, conditional
create (never public ACLs), and verifies remote content by a fresh authenticated
read-back before publishing a reference. Source drift or concurrent manifest
changes refuse publication. Cache hits cannot stand in for remote read-back.
Bounded operations, retry budgets, private credentials and atomic temporary
files apply to uploads/restores. Failed publication may leave an unreferenced
object; no deletion is implemented. Every historical referenced object is kept.

Two lifecycle repairs were made after audit reproduction:

1. **Upload verdict is bound to manifest bytes.** `publishManifest` returns
   after registering a SHA-256 of the exact bytes it published, and the CLI
   reports that digest as `manifestSha256`. The daemon re-hashes the manifest it
   captures and refuses the cycle unless it matches the verified bytes, so an
   edit between publication and capture cannot be laundered into a commit
   (`post_upload_capture_tampering_is_refused`).
2. **Publication locking survives a killed publisher.** Exclusion is a kernel
   `flock` on a stable lock file held by a guardian process that exits on
   stdin EOF when the publishing parent dies; the inode is never unlinked while
   writers may race, and a SIGKILLed upload releases the lock instead of
   wedging publication forever
   (`SIGKILL during publication releases exclusion and next upload recovers`).

   P2 follow-up reviewed: the "survivor holds the guardian stdin write end"
   scenario the reviewer raised requires a *different* process to inherit the
   guardian's stdin fd. Node's `spawn` with a Writable stream as `stdio[0]`
   proxies through a fresh pipe rather than dup'ing the underlying fd, and no
   production code path constructs such a survivor. The `SIGKILL...recovers`
   test is the realistic recovery proof; the proposed ppid-change fallback
   was not added because it would be complexity guarding a path the runtime
   cannot reach.

The protected existing favicon's tracked baseline must remain in Git: deleting
it would violate the existing forward-only guard. Route future versions through
the manifest and exclude that worktree path from auto-staging without deleting
its committed baseline. New bucketed assets materialize into ignored paths.
Upload and reference publication must precede any commit of changed references.
Daemon integration must exclude materialized paths from re-staging,
verify the final staged manifest rather than trusting mutable worktree state,
and fail closed with no commit on storage failure. Existing files and prior
manifest remain usable on failure. Enable only after independent-checkout
restoration and the pilot build pass.

## Access and recovery

Control-plane provisioning needs an OVH Manager session or OVH API application
credentials; the broad S3 pair cannot create users/policies. Use fresh non-owner
users with supported OVH policies (not a generic AWS IAM assumption). Consumer
GET/HEAD allowed; PUT/DELETE refused. Uploader PUT/GET allowed; deletion and
bucket/ACL configuration refused. Verify with explicit isolated credential
clients against disposable canaries and a separate private bucket. Never
fall back to the broad shared credentials for these tests.

Encrypted recovery target: ~/.dracon/secrets/ovh/dracon-lfs-{uploader,consumer}.env
in the existing system configuration repository. File mode 0600, parent 0700.
Warden attribute routing alone is insufficient: verify encrypted Git blobs and
in-memory decrypt equality, plus cloud read using recovered credentials. Keys
must not appear in command arguments, logs, reports, public URLs or plaintext
tracked files. Off-box recipient custody must be reported accurately.

## Bounded verification commands

All commands are bounded and were executed. Paths are relative to their own
checkout; run Rust commands from the utilities workspace (`dracon-utilities`).

Tooling (from dracon-platform):

```
timeout 90 node --test web/scripts/bucket-asset-transfer.test.mjs
timeout 90 bun test web/scripts/bucket-strategy-manifest.test.mjs web/scripts/bucket-strategy.test.mjs
```

Live pilot upload (from dracon-platform). Emits a non-secret digest verdict;
keys never appear in stdout, arguments or the daemon log:

```
timeout 600 node web/scripts/bucket-private-workflow.mjs upload bucket-assets.json .
```

Live restore + isolated checkout and the documented hub build (from the
utilities workspace; copies only committed trees into an empty directory,
installs dependencies in an isolated cache, restores with the consumer
credential, then builds):

```
timeout 1200 python3 dracon-sync/scripts/verify-private-bucket-fresh.py \
  /home/dracon/Dev/dracon-platform dracon-sync/evidence/private-bucket/audit-repair/fresh
```

Scoped access qualification and encrypted recovery (from the utilities
workspace). Both must report `ok: true`:

```
timeout 240 node /home/dracon/Dev/dracon-platform/web/scripts/bucket-private-qualify.mjs
timeout 300 python3 dracon-sync/scripts/verify-private-bucket-recovery.py
```

Daemon-integration tests and all workspace gates (from the utilities
workspace). `private-bucket-verification.nix` pins the reproducible
environment: nixpkgs openssl, git, node, bun, procps (spawned by
`dracon-system` guard tests), openssh (spawned by mirror-push tests), and the
operator's rustup toolchain, which is required because the nixpkgs-channel
rustc is older than this crate's `rust-version` 1.89. `--pure` also removes an
ambient `OPENSSL_DIR` that otherwise points at an include-only path and breaks
the `openssl-sys` build script:

```
timeout 240 nix-shell --pure dracon-sync/scripts/private-bucket-verification.nix \
  --run 'timeout 180 cargo test -p dracon-sync --locked bucket_assets::tests -- --nocapture'
```

All five gates at once, with per-gate status files and captured logs:

```
timeout 2400 nix-shell --pure dracon-sync/scripts/private-bucket-verification.nix \
  --run 'bash dracon-sync/scripts/verify-private-bucket-gates.sh'
```

Evidence for this pass is under
`dracon-sync/evidence/private-bucket/audit-repair/`: `environment.log`,
`integration.{status,log}`, `workspace-tests.{status,log}`,
`clippy.{status,log}`, `deny.{status,log}`, `release-build.{status,log}`,
`transfer-tests.log`, `deployment.log`, `pilot-upload-bound.json`, and
`fresh/{fresh-build.json,games-build.log,restore.log,favicon-audit.log}`.
Each `.status` file records PASS or FAIL so a reader sees the gate outcome in
one line without trusting this prose.

## Result summary for this pass

- Transfer/routing tests: 13 node:test cases pass (the previously recorded 12
  predates the added process-kill recovery test). Public strategy/manifest
  suites: 23 bun tests pass.
- Rust integration tests: 5 pass in the pinned Nix environment.
- All five gates pass there (integration, workspace tests, clippy, deny,
  release build).
- Live scoped access: consumer read-only, uploader no-delete, anonymous refused.
- Encrypted recovery: local and master-recipient recovery byte-exact, no
  tracked plaintext; recovered keys still authenticate to OVH.
- Isolated fresh checkout restores byte-identically and the documented games
  build (`bun run --filter games build`) exits 0 with the restored asset absent
  from the machine cache.
- The tested binary is what the running daemon uses; the pilot repo is the only
  opted-in repo.

## Deployment and live state

- The release binary was installed through the sanctioned
  `dracon-sync maintenance` wrapper (pause → install → ALWAYS resume) with the
  previous binary hashed and retained under
  `~/.local/state/dracon-sync/pilot-backups/` for rollback. Evidence:
  `dracon-sync/evidence/private-bucket/audit-repair/deployment.log`.
- `ActiveState=active` and the installed binary sha256 equals the tested
  `target/release/dracon-sync` (see that log).
- The pilot repo has drift-tolerant history only; no history was rewritten and
  no existing blob was reclaimed anywhere in this goal.

## Residual limits (unchanged from the first pass)

- Off-box custody of the master key is still unverified
  (`offBoxCustodyVerified: false`).
- Live daemon behaviour in the watched fleet repo is proven by injected
  fixtures, not by observing a real fleet outage.
- Only one repo is opted in; nothing here is fleet-wide.

- New private transfer/routing/workflow/scoped-credential tests: thirteen node:test
  cases pass (routing including historical pressure, private schema, path/symlink
  rejection,
  independent-cache restore, size/hash mismatch, interrupted downloads/uploads,
  retries, concurrent publication/restores, source drift, immutable-object
  retention,
  unchanged-reference no-rewrite behavior, offline workflow/cache misses,
  disabled opt-in, credential isolation, SIGKILL publication recovery, and
  post-verification tampering).
- Operational CLI: `bucket-private-workflow.mjs upload|restore CONFIG REPO [--offline]`.
  Configuration selects existing manifest/asset root and absolute scoped credential
  locations, schemaVersion=1, enabled=false by default until qualification; it is
  not a new reference format. Upload measurement includes staged bytes and takes
  the minimum of configured budgets and documented provider ceilings.
- Unchanged bucketed references do not trigger upload/manifest rewrite again.
- Existing public manifest/strategy tests: 23 Bun tests pass, zero failures.
- Legacy prototype is not production-safe: it publishes public ACLs and emits
  references after HEAD alone. It remains disabled; this goal does not enable it.
- OVH supported control-plane provisioning is COMPLETE for the games pilot:
  project `8e0dd132ab65466dadaf643a1ae4392c`, non-owner scoped users
  `dracon-lfs-games-uploader` (858561) and `dracon-lfs-games-consumer` (858562).
  Both policies verified by GET export against the reviewed JSON in
  `scripts/ovh-private-policy.py`. No shared user, policy or broad key changed.
- Policy submission initially refused (HTTP 400): official guide documents
  `GetObjectVersionAttributes` but the live validator rejects it.
  `scripts/ovh-private-policy.py` now records the live supported action set
  (`DeleteIntelligentTieringConfiguration` and `RestoreObject` included,
  `GetObjectVersionAttributes` excluded) discovered from that rejection.
- Scoped S3 credentials minted through the supported
  `/user/{id}/s3Credentials` endpoints, RSA-OAEP encrypted in the browser to a
  one-time loopback bridge (`scripts/ovh-scoped-credential-capture.mjs`), and
  persisted warden-encrypted. No plaintext key ever reached tool output, a
  CLI argument, a report, or the daemon log.
- Encrypted recovery evidence: `scripts/verify-private-bucket-recovery.py`
  recovers both keys from committed HEAD blobs (dracon config commit
  `23729bac26a03ad3192be6ea6e652cf51ecfb0c5`), verifies local and dedicated
  master-recipient recovery byte-for-byte, then re-runs the full scoped cloud
  qualification with isolated clients. Result: `ok: true`, both blobs
  encrypted, no tracked plaintext. Evidence:
  `evidence/private-bucket/encrypted-recovery-2026-10-14.json`. Off-box custody
  is explicitly NOT verified (`offBoxCustodyVerified: false`).
- Scoped access qualification (`web/scripts/bucket-private-qualify.mjs`,
  evidence `evidence/private-bucket/scoped-access-2026-10-14.json`): uploader
  PUT + consumer HEAD/GET byte-identical (sha256 479cd0f8…), consumer prefix
  list allowed, anonymous GET refused (403). Refused with AccessDenied for
  consumer PUT, consumer delete, uploader delete, both ListAllMyBuckets, both
  outside-prefix writes, and both writes to a different private bucket
  (`dracon-master`). The seeded canary survived every delete probe, and the
  refused consumer PUT left no object.
- Daemon integration (`src/bucket_assets.rs`, opt-in via a TRACKED
  `bucket-assets.json` at repo root, with `.dracon/bucket-assets.json` accepted
  as an ignored operator-local override; default disabled fleet-wide).
  Upload/read-back runs BEFORE diff and staging, so verified reference changes
  land in the same commit cycle. Materialized asset/cache paths are excluded
  from staging, the staged manifest is re-read from the index and must equal
  the uploaded bytes, mismatches or a staged materialized file block the commit
  with worktree and prior manifest intact, and retries re-attempt after storage
  outages. Fault-injection test
  `daemon_commits_verified_reference_not_raw_bytes_and_recovers_from_failure`
  plus `staged_reference_drift_and_raw_materialization_are_refused` cover
  ordering, recommit-loop refusal, failure, and recovery; six tests pass,
  including the post-verification-tamper refusal above.
- LIVE PILOT TRANSFER (evidence `evidence/private-bucket/pilot-upload-2026-10-14.json`):
  `bucket-private-workflow.mjs upload` published the pilot reference —
  `games/7884ac7225df090731d8a00b56b6de69cb34cc1fe8763b8aeb15132f810664d7.png`,
  103 bytes, `bucketed: true`. Routing note recorded honestly: the measured
  projected pack (1,057,999,192 B) is below the 80% pressure threshold of the
  effective 2,000,000,000 B limit, and the favicon is smaller than the 10 MiB
  large-asset rule, so the pilot sets `largeBytes: 1` for this explicitly
  regenerable 103-byte asset. Pressure routing remains the default path for
  repos that actually approach a provider ceiling; no threshold was weakened.
- FRESH-CHECKOUT RESTORE + DOCUMENTED BUILD (evidence
  `evidence/private-bucket/audit-repair/fresh/fresh-build.json`): a Python
  driver copies the parent's committed tree plus the committed gitlink
  metadata/vendor trees needed by the hub workspace into an empty directory
  (no worktree, no cache, no prior materialized asset), installs an
  independent dependency set in a private cache, restores with the consumer
  credential — byte-identical, sha256 7884ac72…, 103 bytes — then runs
  `bun run --filter games build`, exit code 0, recorded in
  `audit-repair/fresh/games-build.log` with `restore.log` and
  `favicon-audit.log` beside it. The favicon audit remains as a secondary
  cross-check (6/6 OK, 5 failures without the restored asset).

- Rust workspace gates, run AFTER the final code change: `cargo test
  --workspace --locked` (35 suites ok), `cargo clippy --workspace --locked --
  -D warnings` (clean), `cargo deny check` (advisories/bans/licenses/sources ok),
  `cargo build --release --locked` (finished).
- Durable references: pilot config `bucket-assets.json`, private manifest
  `web/config/bucket-manifests/games-private.json`, and the new
  `web/scripts/bucket-{private-workflow,private-s3,asset-transfer}.mjs` are
  committed in dracon-platform by the daemon's own commit-all policy (e.g.
  commit `60555125e0`); daemon-side implementation is `dracon-sync`
  `src/bucket_assets.rs`. Encrypted credentials live only in
  `~/.dracon/secrets/ovh/dracon-lfs-{uploader,consumer}.env` (config commit
  `23729bac26a03ad3192be6ea6e652cf51ecfb0c5`).
- NOT verified: off-box custody of the master key (`offBoxCustodyVerified:
  false`), live daemon behaviour in the watched fleet repo beyond the injected
  fixture, and no fleet-wide enablement — only this one pilot repo is opted in.
```
timeout 90 node --test web/scripts/bucket-asset-transfer.test.mjs
timeout 90 bun test web/scripts/bucket-strategy-manifest.test.mjs web/scripts/bucket-strategy.test.mjs
```
Run these from dracon-platform. Live/provisioning and daemon commands will be
recorded only after actual execution; no test proof is inferred from plans.
