# Object-storage delivery ledger

Status: active implementation. The full roadmap is not complete or released.
This ledger records actual evidence and preserves the remaining scope.

## Implemented and checked

- `storage plan` and `storage validate` run from a source-build CLI without
  requiring a daemon; an explicit operator policy works in generic repos.
- Global/per-repo storage types, strict storage schema validation, complete
  rule-list overrides, explicit false, inclusive size conditions, ordered
  matching, operator-only backend bindings, and permitted security classes.
- Repositories cannot inject backend endpoints or silently downgrade an
  operator backend's encryption requirement.
- Read-only Git inventory, batched effective-filter inspection, regular-file
  byte estimates, exclusions/limits/legacy migration concerns, lossless
  non-UTF8 path identities, config/payload symlink refusal, nested repo isolation.
- Explicit history inventory separates unique reachable raw blob bytes across
  all local refs from own object-database bytes and from push-size estimates.
- Shared-library immutable local backend with bounded streaming, create-only
  publication, readback checks, cold reopen, corruption/missing-object failure,
  interrupted capture, and symlink object refusal.
- Isolated encrypted-byte and Git LFS/prepared-filter experiments; exact restore,
  single effective driver, stale preparation refusal, and cold reference clone.
- Strict canonical Git LFS v1 pointer codec in the shared library; malformed,
  oversized, noncanonical, or unsupported extended pointers fail closed.
- Versioned transactional journal with exact-source job identities, private
  files, per-job OS leases, compare-and-swap revisions, typed retry failures,
  required object-copy and Git-push receipts, and redacted read-only inspection.
- Durable bounded source snapshots with digest checks, matching-prefix resume,
  complete-spool adoption after process death, record/count/byte budgets, and
  no automatic deletion of retained source versions. See the
  [journal contract](storage-journal-contract-2026-10-01.md).
- Durable approved-payload candidate identities and bounded private payload
  snapshots; same-representation prefix resume, exact-byte reuse, independent
  byte budgets, and recovery on both sides of payload and job publication.
- Shared-library copy executor uses retained bytes, verifies every required
  destination despite historical receipts, saves attempts/receipts, and retains
  snapshots on typed integrity/capacity/transient failures. Transient readback
  tests exercise backoff; apparent upload success alone cannot mark a copy saved.
- `storage status` inspects local journal evidence without creating state or
  claiming current backend availability. Uninitialized repos need no enrollment.
- Warden source-build `storage-encrypt`/`storage-decrypt` stream whole-payload
  age representations through existing authorized recipients and identity
  discovery, independently of Git filter limits. Isolated CLI tests cover
  101 MiB payloads, exact restoration, an untrusted recipient, corruption,
  and plaintext byte budgets. Caller publication still requires successful exit.
- Pointer encoding decision recorded in `storage-representation-decision-2026-10-01.md`.

Validation commands used on 2026-10-01 (latest results below):

```sh
cargo test --workspace --locked
cargo build --release --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo deny check
cargo test -p dracon-sync encrypted_payload_round_trip --locked -- --ignored
nix shell nixpkgs#git-lfs --command python3 dracon-sync/scripts/storage-representation-prototype.py
```

The last command was run from the parent workspace. The prototype's ordinary
standalone invocation is documented in `docs/storage-planning.md`. The age test
is normally ignored because it requires external tools; it was explicitly run
and passed. Workspace checks include the normal storage tests. These checks
prove the current milestone, not absent S3/worker/daemon integration.

## Journal/security milestone validation

Focused shared-library checks passed 21 tests; three external-tool/subprocess
helpers were ignored by the ordinary run. Crash helpers are invoked explicitly
by the recovery tests. The two isolated Warden CLI integration tests passed,
including the 101 MiB encryption/decryption round trip. The isolated local
backend age/cold-reopen check was explicitly rerun and passed. Strict all-target
workspace Clippy, `cargo deny check`, and the locked release build passed.

The built release CLI was also exercised in an isolated fresh Git repo:
unenrolled and explicit-ID inspection created no state, and a corrupt record
caused a nonzero result while preserving its bytes. No installed binary changed.

An initial full workspace run overlapped another process using Warden's old
counter-based temporary directory names. Ten Warden tests failed as those
shared fixtures were removed or changed. The helper now uses unique owned
`tempfile` directories, retaining its quote/space path cases. The independent
Warden rerun passed all 167 tests while the new workspace run was active. The
full workspace rerun passed 1962 tests (12 ignored), including both
Warden streaming integration tests. The final complete-spool fsync hardening
also passed all 21 shared-library tests and strict all-target workspace Clippy.
The initial failed run is not counted as a passing gate.

## Current inventory findings

See `storage-inventory-summary-2026-10-01.json` for recorded metadata counts.
The temporary simulation used current operator exclusions, declared intermediate
path patterns, and an illustrative 20 MiB media condition. It did not approve
production placement or access a backend. All five repository inventories ran
successfully without creating the configured local-backend directory.

The simulated media condition selected no current files. Platform had 15 nested
repos and 33 symlinks; it also had 92 paths selecting `dracon-assets`, confirming
compatibility review cannot be limited to Doomtap. Doomtap had 772 paths selecting
that filter. These counts include actual effective attributes, not guessed
extensions, and can change as the daemon/agents work.

Music had 246,085,113,327 raw reachable blob bytes across all local refs while its
own object database occupied 2,760,378,368 bytes. Platform had 22,452,993,937 raw
blob bytes and an 8,863,661,056-byte database. These do not identify the next push
size or prove which path dominates; per-path/churn attribution is still needed.
The result supports purpose/churn rules and generated-data analysis instead of
assuming a 20 MiB media threshold alone will solve history growth.

## Outstanding work packages

| Package | Status | Required next evidence |
| --- | --- | --- |
| A: contracts/inventory | Partial | Per-path churn attribution, producer/service review, threat model, manifest/security/group contracts |
| B: policy | Partial | Versioned sticky enrollment, atomic group policy, recovery/retention settings, actual staging resolution |
| C: storage/security | Partial | Local streaming adapter and Warden CLI checked; S3 capability conformance, worker composition, protected restore metadata remain |
| D: durable journal | Partial | Transactional records, leases, source snapshots, byte budgets and process-death tests checked; prepared payload retention/copy execution checked; production reconciliation and operator resource policy remain |
| E: Git bridge | Partial | Strict pointer codec checked; protected manifest, required local filter composition, manual-index races, staging entry points and outgoing-ref validation remain |
| F: restoration | Prototype only | Packaged hydrate/verify commands, safe destinations/cache, historical key recovery, independent copy failover |
| G: daemon/status | Partial | Read-only redacted journal status checked; worker scheduling, live backend verification, fairness, grouping and outage isolation remain |
| H: live pilots/release | Not started | Approved backend/recovery cost and exact pilot paths, real provider drills, clean-machine fixtures, final gates/release |
| I: legacy maintenance | Separate proposal pending | Measured exact paths, verified backup/rollback, explicit sanctioned authorization |

## Next implementation sequence

Continue with security subprocess composition, the protected restore manifest
and production reconciliation around the journal and copy executor. Add
S3 backend capability checks and independent-copy verification. Wire the Git bridge only after exact-version and
security gates pass. Continue with hydration, daemon scheduling/status, pilots,
and packaged release validation. Keep the whole roadmap active; a green preview
build does not mean automatic preservation exists.

No production storage enrollment, bucket writes, independent-copy policy,
retention changes, or history rewrites were performed for this milestone.

## Prepared-payload/copy executor milestone

Focused shared-library tests passed 28 tests (four ignored helpers/external-tool
checks), including candidate binding, restart identity reuse, four payload/job
publication crash points, independent payload budgets, required-copy readback,
corrupt old receipts, quota failure and transient readback retry. No source bytes
or new ciphertext can be substituted by the copy executor. Strict all-target
workspace Clippy passed before the final local failure-classification refinement.
The current full workspace/release reruns are pending and will be recorded here.

This remains library infrastructure: no new `storage prepare` command, production
Warden subprocess orchestration, S3 adapter, manifest, filters or daemon worker
is present. No live storage paths/backends have been enrolled.
