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
- Test-only immutable local backend with bounded streaming, create-only
  publication, readback checks, cold reopen, corruption/missing-object failure,
  interrupted capture, and symlink object refusal.
- Isolated encrypted-byte and Git LFS/prepared-filter experiments; exact restore,
  single effective driver, stale preparation refusal, and cold reference clone.
- Pointer encoding decision recorded in `storage-representation-decision-2026-10-01.md`.

Commands checked on 2026-10-01:

```sh
cargo test --workspace --locked
cargo build --release --locked
cargo clippy --workspace --locked -- -D warnings
cargo deny check
cargo test -p dracon-sync encrypted_payload_round_trip --locked -- --ignored
nix shell nixpkgs#git-lfs --command python3 dracon-sync/scripts/storage-representation-prototype.py
```

The last command was run from the parent workspace. The prototype's ordinary
standalone invocation is documented in `docs/storage-planning.md`. The age test
is normally ignored because it requires external tools; it was explicitly run
and passed. Workspace checks include the normal storage tests. These checks
prove the current milestone, not absent S3/journal/daemon integration.

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
| C: storage/security | Prototype only | Production backend interface, S3 capability conformance, Warden streaming/trust adapter, protected restore metadata |
| D: durable journal | Not implemented | Versioned transactional journal, per-job leases, crashes/restarts, reconciliation and resource limits |
| E: Git bridge | Prototype only | Production pointer/manifest schema, required local filter composition, manual-index races, all staging entry points, outgoing-ref validation |
| F: restoration | Prototype only | Packaged hydrate/verify commands, safe destinations/cache, historical key recovery, independent copy failover |
| G: daemon/status | Not implemented | Fair retries, source/asset grouping, actual durability/concern JSON, unrelated-source syncing during outages |
| H: live pilots/release | Not started | Approved backend/recovery cost and exact pilot paths, real provider drills, clean-machine fixtures, final gates/release |
| I: legacy maintenance | Separate proposal pending | Measured exact paths, verified backup/rollback, explicit sanctioned authorization |

## Next implementation sequence

Specify the manifest and durable journal around the selected pointer encoding.
Implement journal/reconciliation tests and the Warden security adapter, then
production local/S3 backends. Wire the Git bridge only after exact-version and
security gates pass. Continue with hydration, daemon scheduling/status, pilots,
and packaged release validation. Keep the whole roadmap active; a green preview
build does not mean automatic preservation exists.

No production storage enrollment, bucket writes, independent-copy policy,
retention changes, or history rewrites were performed for this milestone.
