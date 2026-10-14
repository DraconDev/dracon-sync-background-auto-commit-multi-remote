# Private bucket workflow pilot — implementation in progress

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

## Current evidence (not deployment acceptance)

- New private transfer/routing/workflow/scoped-credential tests: twelve node:test cases pass
  (routing including historical pressure, private schema, path/symlink rejection,
  independent-cache restore, size/hash mismatch, interrupted downloads/uploads,
  retries, concurrent publication/restores, source drift, immutable-object retention,
  unchanged-reference no-rewrite behavior, offline workflow/cache misses,
  disabled opt-in, and credential isolation).
- Operational CLI: `bucket-private-workflow.mjs upload|restore CONFIG REPO [--offline]`.
  Configuration selects existing manifest/asset root and absolute scoped credential
  locations, schemaVersion=1, enabled=false by default until qualification; it is
  not a new reference format. Upload measurement includes staged bytes and takes
  the minimum of configured budgets and documented provider ceilings.
- Unchanged bucketed references do not trigger upload/manifest rewrite again.
- Existing public manifest/strategy tests: 23 Bun tests pass, zero failures.
- Legacy prototype is not production-safe: it publishes public ACLs and emits
  references after HEAD alone. It remains disabled; this goal does not enable it.
- OVH supported control-plane user creation succeeded through the signed-in
  Manager session: project 8e0dd132ab65466dadaf643a1ae4392c, non-owner scoped
  users dracon-lfs-games-uploader (858561) and dracon-lfs-games-consumer (858562).
  No shared user, existing policy or broad key was changed.
- Policy submission initially refused (HTTP 400): official guide documents
  GetObjectVersionAttributes but the live validator rejects it. Policy generator
  `scripts/ovh-private-policy.py` records the live supported action set instead.
  Corrected submission timed out; acceptance is UNKNOWN, not success. GET export
  also remained pending amid Chrome CDP detach/tab disappearance. Before retrying
  any mutation, inspect those two users' current policies via GET.
- S3 credentials have NOT been minted/captured. No new encrypted recovery files
  exist yet. Synthetic warden clean/smudge succeeds and uses encrypted markers,
  but that is not actual credential recovery evidence.
- Secure one-time capture helper `scripts/ovh-scoped-credential-capture.mjs`
  uses browser RSA-OAEP ciphertext to loopback, then warden encryption verification,
  local + master decrypt comparison, explicit encrypted staging and 0600 runtime
  materialization. It never returns/logs credentials. It is not yet live-qualified.
  The owned one-time capture process was stopped after Chrome/API timeouts;
  neither credential file exists. Restart only when policy GET verification succeeds.
- Live transfer, fresh build, daemon integration and Rust workspace gates remain
  outstanding. No pilot manifest has been switched or automation enabled.

Commands:
```
timeout 90 node --test web/scripts/bucket-asset-transfer.test.mjs
timeout 90 bun test web/scripts/bucket-strategy-manifest.test.mjs web/scripts/bucket-strategy.test.mjs
```
Run these from dracon-platform. Live/provisioning and daemon commands will be
recorded only after actual execution; no test proof is inferred from plans.
