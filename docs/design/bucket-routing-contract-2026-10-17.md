# Bucket routing contract — 2026-10-17

Status: implementation contract for the queued OVH bucket follow-up; NOT
an implementation, deployment or live-upload qualification claim.

## Scope and current evidence

The existing private target is `dracon-lfs` on OVH S3. Reuse the installed
trusted profile and scoped uploader/consumer credential files. Do not copy
credential values into reports. No new credentials, access-policy changes,
new adapter types, cap increases, tracked-asset retirement or fleet migration.

The current bridge (`src/bucket_runner_bridge.rs`, `prepare`) passes
projectedBytes=0, largeBytes=10 MiB and limitBytes=policy.max_push_blob_bytes.
`routing.mjs` independently defaults to 10 MiB and pressureRatio=0.8.
`publisher.mjs` calls validateBudget then shouldRouteEligible only after
qualification. The runner validates the exact four-field budget schema.

`adapters.mjs` accepts canonical, level-6-deflated solid RGBA PNGs only,
with maxDimension=512 and maxSourceBytes=maxRawBytes=2 MiB. Exact canonical
re-render equality rejects noise, padding and noncanonical compression.
Thus merely choosing a threshold below 2 MiB is insufficient evidence of
reachability. Measured with the actual renderer, RGBA [32,64,96,255]:

| Square dimension | Canonical encoded bytes |
|---|---:|
| 32 | 105 |
| 64 | 210 |
| 128 | 361 |
| 256 | 858 |
| 512 | 2201 |

These are observations of the current Node/zlib toolchain, not invariants
that tests should freeze across toolchain upgrades. Tests must generate
and independently measure real canonical assets.

## Shared policy and ownership

Introduce a versioned `scripts/bucket-runner/routing-defaults.json` as the
single checked-in default policy, consumed by both Rust and the bundled JS
runner. Proposed initial schema: schemaVersion=1, largeBytes=1024,
pressureRatio=0.8, pressureLimitBytes=2147483648 (2 GiB).

Rust may embed this JSON with include_str!; JS must bundle its imported data
into the dependency-closed runner. Invalid embedded data is an error, never
a silent fallback to unrelated defaults. Add a cross-language parity test.
Do not add a per-repository routing override or accept thresholds from an
untrusted bucket-runner.json in this change. If later implementation adds
operator-configurable knobs, both global/per-repo policy halves and merge
behavior need the repository's coverage tripwires and behavioral tests.

The 1 KiB threshold intentionally exercises the narrow current adapter:
small icons stay local at low pressure, while a real canonical 512-square
example routes by size. Small files are cheap in Git, so this is a limited
adapter routing policy, not a general recommendation to upload every tiny
file. No padding or relaxed provenance rules to make fixtures qualify.

The 2 GiB aggregate limit is a soft raw-payload routing budget, not a claim
of exact compressed pack size or a host acceptance guarantee. Never reuse
max_push_blob_bytes as the aggregate limit. Retain all existing per-blob,
pack, encryption and forward-only guards unchanged.

## Rust to runner handoff

`bucket_runner_bridge::prepare` must assemble the same four-field trusted
request budget: projectedBytes (measured as below), limitBytes (shared
pressureLimitBytes), largeBytes and pressureRatio. The JSON schema need not
expand. Validate positive limits, safe-integer range, positive largeBytes,
and a finite pressureRatio strictly between zero and one on both sides.

No config or profile is changed by this design item. Preserve the existing
unsupported/refused/retryable behavior. A budgeting failure must not turn a
managed repository into a legacy-path fallback that silently commits raw
managed assets. Dry runs stay read-only and perform no upload.

## Projected outgoing raw blob accounting

Meaning: a conservative, deduplicated estimate of raw Git blob bytes that
would be newly needed for the next guarded publish, including outstanding
history and prospective committable work. This is NOT total .git size,
whole-worktree size, apparent file count, or a compressed pack estimate.

1. Snapshot HEAD, the index identity/tree and the configured active publish
   remote/branch tips used for the cycle. Respect excluded and fetch-only
   remotes. Use the existing trusted remote resolution; do not invent a
   remote from repository-provided URLs. Snapshot values belong in internal
   accounting evidence, not credential-bearing logs.
2. For each active publish destination with a locally known remote tip,
   enumerate outgoing objects from HEAD minus that tip, using object IDs
   without pathnames (`rev-list --objects --no-object-names`). The union
   across destinations covers the laggiest mirror. Exclude trees/commits
   from byte accounting. Use cat-file batch size/type queries and sum each
   unique blob once. Do not exclude a blob merely because another mirror
   already has it. Stale tracking tips are conservative observations, not
   proof of live remote convergence.
3. A genuinely new remote branch with no known tip conservatively considers
   all reachable local HEAD blobs. An unavailable tracking ref that should
   exist is an explicit measurement error, not a zero estimate. Distinguish
   this from a verified empty/new destination. Do not fetch or mutate Git
   state solely to hide accounting failure.
4. Include the prospective commit candidates from the SAME committable-path
   selection/exclusion policy that staging will use: staged replacements
   and selected worktree/new files, with precedence matching that staging
   operation. Do not count arbitrary ignored/excluded files or traverse
   nested repositories. Deletions add no new blob, but cannot subtract
   objects still required by outgoing historical commits. Deduplicate
   selected Git blob IDs against outgoing IDs. Use Git object semantics
   for symlinks/gitlinks and never dereference symlinks as regular files.
5. Account for clean-filter transformations: where a selected file's final
   Git blob is not yet knowable without running a mutating filter, use a
   documented conservative size bound or fail measurement. Never hash raw
   plaintext into the object database to obtain an estimate, invoke upload
   filters speculatively, or equate apparent raw size with ciphertext size.
   An unavailable safe bound is a refusal, not guessed zero. Worktree byte
   hashing, if needed, must be read-only, bounded and race checked.
6. Use checked arithmetic and enforce JS safe-integer range. Bound Git
   subprocesses, object counts, captured output and file reads; use existing
   cancellation/process-group ownership helpers. No pack creation, network
   upload or unbounded history scan to compute a soft routing estimate.
7. Revalidate the snapshot and candidate file identities before accepting
   the estimate. Index/HEAD/tip/file changes invalidate it and produce a
   bounded retryable result, rather than publication with stale evidence.
   Revalidation after upload and before commit still uses existing manifest
   and staged-path checks. Other repositories remain schedulable.

The implementation may conservatively overcount uncertain deduplication;
it must never represent incomplete accounting as a measured zero. A zero
is valid only after successful enumeration of no outgoing/prospective blobs.
Record estimate semantics, snapshot identifiers, bytes and safe reason codes
in qualification evidence; never secret values or captured SDK diagnostics.

## Routing decision and failures

After eligibility/provenance/consumer checks and budget validation:

- Existing verified bucket references stay bucket-routed (no silent raw
  fallback or tracked-asset retirement).
- Otherwise route when encoded eligible bytes >=1024 OR projectedBytes >=
  floor(limitBytes * 0.8). Equality routes. Below both stays local.
- Unsupported encoding, unsafe path, untrusted binding or consumer failure
  never becomes eligible because size/pressure is high.
- Missing measurement, overflow, timeout, unreadable object, filter-bound
  uncertainty or concurrent changes produce explicit refusal/retryable
  outcomes before publication. Retain source/index/operator edits, commit
  no unverified reference and retry through the existing daemon scheduler.
- Transport failure never authorizes source deletion or manifest success.
  Retry is idempotent; isolated local failure injection is removed afterward.

## Verification handoff to subsequent items

Each command is bounded and logs its actual exit status (avoid pipelines
that hide failure). Do not invoke the project's entire suite from a test.

1. `timeout 120 node --test scripts/bucket-runner/routing.test.mjs`:
   below/equal/above both boundaries, invalid budgets, retained bucket refs,
   actual canonical PNG reachability and unsupported candidate exclusion.
2. `timeout 120 cargo test -p dracon-sync --locked bucket_runner_bridge`:
   shared-default parity, exact serialized request, actual Git outgoing and
   prospective-work accounting, dedup, deletes, missing tips/new branches,
   clean-filter bounds, races, resource bounds and no-zero-on-error behavior.
   Test filtering must be checked to ensure it executes the added tests.
3. The bounded hermetic qualification harness must prove outage/partial
   upload, concurrent manifest refusal, idempotent retry and fresh restore.
4. Before deployment, bounded final-code workspace tests, release build,
   Clippy, deny and changed-script tests must pass; log failures honestly.
5. Sanctioned atomic install requires matching artifact/trusted-profile
   hashes, retained rollback outside watched source trees and runtime smoke
   execution. New bundle bytes invalidate old Warden review receipts: perform
   explicit content-and-match review, never path exemptions or hook bypass.
6. Live evidence must connect a NEW canonical fixture to a daemon cycle,
   actual OVH object, independent scoped read/digest, fresh consumer restore,
   then a locally injected failed attempt followed by autonomous recovery
   and unrelated sync progress. bucketed:true alone is not upload proof.

## Scope checklist

This item delivers only the policy/accounting design and measured renderer
reachability. Implementation, regression execution, production deployment,
OVH writes and final full audit belong to the following separate list items.
No source, installed artifact, credentials, index or production config was
modified while drafting this contract.
