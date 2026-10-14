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

Before committing source removal, upload and reference publication must
succeed. Daemon integration must exclude materialized paths from re-staging,
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

- New private transfer module and routing tests: seven node:test cases pass.
- Existing public manifest/strategy tests: 23 Bun tests pass, zero failures.
- Legacy prototype is not production-safe: it publishes public ACLs and emits
  references after HEAD alone. It remains disabled; this goal does not enable it.
- Scoped user provisioning, live transfer, fresh build, daemon integration and
  Rust workspace gates are outstanding. No pilot automation is enabled yet.

Commands:
```
timeout 90 node --test web/scripts/bucket-asset-transfer.test.mjs
 timeout 90 bun test web/scripts/bucket-strategy-manifest.test.mjs web/scripts/bucket-strategy.test.mjs
```
Run these from dracon-platform. Live/provisioning and daemon commands will be
recorded only after actual execution; no test proof is inferred from plans.
