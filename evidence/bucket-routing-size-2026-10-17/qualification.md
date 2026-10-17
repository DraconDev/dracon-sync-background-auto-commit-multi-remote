# Shared SIZE-routing policy qualification — 2026-10-17

Scope: second queued bucket item only. No production deployment, credential
changes, access expansion, hook exemptions or hook bypasses performed.

## Implementation

- `scripts/bucket-runner/routing-defaults.json`: one versioned policy,
  largeBytes=1024, pressureRatio=0.8, pressureLimitBytes=2147483648.
- Rust embeds this data and validates schema, ranges, safe integers and
  unknown fields before serializing the runner's exact four-field budget.
  Removed the 10 MiB literal and per-blob/aggregate denominator confusion.
- JS imports and validates the same data. Its existing inclusive size and
  pressure decisions remain downstream of asset qualification.
- Refreshed the checked-in dependency-closed bundle because the runner tests
  require it to match source. This is NOT installation or live qualification.
- Projected bytes remain zero pending the separate outgoing-accounting item.
  This item does not claim pressure measurement is implemented.

## Executed verification

Commands were executed from the parent workspace with bounded deadlines.
Logs in this directory preserve the actual output.

| Gate | Command | Result |
|---|---|---|
| Rust request/validation regression | timeout 180 env CARGO_BUILD_JOBS=1 cargo test -p dracon-sync --locked bucket_runner_bridge -- --test-threads=1 | 12 pass, 0 fail |
| Full runner Node regression | timeout 120 node --test dracon-sync/scripts/bucket-runner/*.test.mjs | 157 pass, 0 fail |
| Pinned bundle rebuild | timeout 30 node dracon-sync/scripts/build-bucket-runner.mjs | dependency closure verified, 1578883 bytes |
| Clippy | timeout 120 env CARGO_BUILD_JOBS=1 cargo clippy -p dracon-sync --locked -- -D warnings | pass |
| Whitespace | git -C dracon-sync diff --check | pass |
| Preserved safety source | git -C dracon-sync diff --quiet 10633cc -- scripts/bucket-runner/adapters.mjs scripts/bucket-runner/publisher.mjs src/bucket_guard.rs | exit 0 |

The first full Node run correctly failed `bundle differs` for the stale
checked-in bundle. After the normal pinned rebuild, all 157 tests passed.
No assertion or gate was weakened to obtain that result.

Coverage: 1023/1024/1025-byte size boundary; pressure equality boundary;
shared default identity; actual canonical 512-square PNG size eligibility;
small PNG local retention; unsupported bytes excluded even under pressure;
Rust request stub checks all four serialized policy fields; malformed,
unknown, zero/negative/out-of-safe-range policy inputs rejected.

A fresh read-only reviewer found no blockers for this SIZE item, confirmed
qualification and caps unchanged, and explicitly excluded outgoing-byte
measurement from its verdict. Reviewer lacked execution tools; parent
executed the logged gates rather than treating source review as test output.

## Remaining queued obligations

Outgoing accounting; final workspace gates/artifacts; atomic deployment;
new artifact non-secret match review if Warden refuses it; independent live
OVH object byte verification, fresh restore, autonomous retry and full audit.
A rebuilt bundle invalidates old content-bound Warden receipts by design.
Do not bypass the hook or claim a remote push of this new artifact without
that review. No such receipt was changed in this item.

Installed runner SHA-256 observed after checks remained
f1d0ff1fcb8b82d9bb3d835b07393f7cb94dca40c50dfba4136b847240b88612.
This records non-deployment, not evidence that the new source is running.
