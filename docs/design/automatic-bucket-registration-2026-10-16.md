# Automatic bucket registration — implementation design and evidence

Status: implementation in progress; NOT a rollout or completion claim.

## Post-rehearsal checkpoint (2026-10-16)

The task ledger was overoptimistic: component completion does not establish
production integration. Reopened integration and deployment after verifying
that only fixture/stub callers invoke `publishRepository`, no live capability
profile is installed, and the daemon still requires explicit per-repo config.
The parent evidence `evidence/auto-registration-2026-10-16/verification-correction.md`
records the full repeated cargo gates (2430 tests, zero failed) and a rebuilt
binary SHA different from the installed/running SHA. No new deploy occurred.

The publisher now waits for an explicit post-flock readiness message, refuses
symlink lock leaves and releases via parent-pipe EOF. Scratch upload snapshots
are private temporary directories outside watched source trees. Snapshot bytes
are bounded and provenance-verified before upload; every successful put is read
back and verified before publication. Current source bytes are revalidated
before manifest write. Concurrent creation/deletion of a manifest invalidates
the prior-state comparison; manifest paths cannot escape checkout ownership.
Seven new regressions cover lock exclusion/timeout/symlinks, successful-but-
corrupt PUTs, concurrent initial manifest creation, source changes before and
during upload, and manifest escape/symlink ancestors. The bounded gate passes
42/42 tests (`evidence/auto-registration-2026-10-16/publisher-safety-regressions.log`).

The next component checkpoint adds `bindings.mjs`: operator-selected exact
remote identity, common-Git-directory inode identity and a committed ancestry
anchor are stored privately outside the checkout. Directory renames preserve
the binding; a copied descriptor cannot grant it. The independent static
component review accepted the boundary and identified a temporary-file leak
on failed writes/fsync. The parent fixed cleanup and added injected write/fsync
regressions with successful retry.

`routing.mjs` requires an explicit measured budget; no absent-budget implicit
upload is possible. Eligible small assets enroll as local manifest entries
without remote calls. They route later when measured pressure rises; a
previously bucketed entry remains routed. Managed changed assets require a
fresh trusted probe and upload/read-back; stable IDs are retained and old
immutable objects remain. Arbitrary legacy entries and unsupported operator
edits are left intact. Fixture upload scenarios now explicitly provide synthetic
pressure rather than assuming every eligible asset should upload. All 60
component/E2E tests pass; this is still NOT production integration.

Remaining implementation order: dependency-closed installed runner/CLI and
scoped real transport; positively verified consumer bootstrap; bounded
clean/new/changed-repository scheduling under the existing repo mutation
owner; digest-bound pre-stage checks; then final gates,
sanctioned deploy, fresh inventory and actual live storage qualification.
No permissions or credentials were changed to manufacture a live success.

### Installed-runner prerequisites checkpoint

The scoped SDK transport is implemented with pinned dependencies. Its two
independent-review P1 findings were fixed: error-body collection is capped and
request-abort-aware before SDK deserialization, and credential ancestors must
be root/operator-owned. Tests cover oversized 403 bodies on GET/PUT, stalled
error-body cancellation and foreign-owner rejection. Credential fixtures now
live outside watched source trees.

Consumer text qualification and digest-bound revalidation are implemented for
plain Vite/npm only; no installer or runtime proof is implied. The independent
review's nested-standalone exclusion was fixed: a parent repo/package does not
invalidate a child's independently verified Git root. Directory enumeration
now streams with a cap before collecting/sorting, rather than limiting after
an unbounded readdir. The parent added create-only cache-first restoration:
matching bytes require no remote request, differing local edits are retained,
missing local-only entries fail explicitly, and corrupt/interrupted downloads
never appear at the final path.

The publisher now persists the private digest index before the manifest so
an index-write failure cannot leave a newly published manifest; retries
revalidate remote data. Exact authorized candidate sets from the future Rust
bridge prevent files appearing after policy selection from evading excludes.
An options-forwarding defect was corrected so lowered enumeration caps apply.
All 139 current component/E2E tests pass in the bounded script gate; evidence
is `evidence/auto-registration-2026-10-16/storage-consumer-restore-regressions.log`.
No permissions or credentials were changed to manufacture a live success.

### Trusted-bridge runner review remediation (commit 747f0ed)

The first independent review of the dependency-closed runner protocol
(`runner.mjs` + `publisher.mjs`) found three P1 issues; all fixed inline with
durable root-cause fixes and new tests:

1. **Stranded index-ahead transaction** (`runner.mjs:140-149`,
   `publisher.mjs:189-208`) — an interrupted transaction whose index rename
   succeeded but whose manifest rename did not was a permanent refusal. The
   publisher now detects a stranded index digest, re-issues the same record
   during a subsequent call, and exposes `recoveredFromStrandedIndex`. The
   runner rejects only foreign-identity indices; legitimate stranded
   transactions self-heal. Regression test:
   `runner recovers from a stranded index-ahead transaction and never imports a mismatched index`.
2. **Local-only assets returned as stage-suppression paths** — the
   `materializedPaths` list now filters to `bucketed: true` AND positively
   re-verified current outcomes. `outcomes[i].bucketed` and a `local-only`
   reason communicate the local-only path to the future Rust stage bridge.
   Regressions: `runner suppresses only positively re-verified bucketed entries
   and does not stage deleted ones` and `runner local-only entries stay
   unenrolled and are reported bucketed:false with local-only reason`.
3. **Retained missing/unsupported entry vetoes unrelated progress** — covered
   by the same two regressions: a deleted bucketed entry no longer returns as
   `eligible`/`already-registered`, and `commitEligible` remains true on
   idempotent retries of the remaining entries.

The bounded gate (`scripts/verify-bucket-registration-e2e.sh`) now runs 153
tests, all passing; the gate is the single source of truth for the runner's
harness. The installed protocol CLI, dependency-closed bundle, Rust
bridge/scheduler, fresh installed lifecycle qualification and real-storage
rollout remain open.




## Authorized outcome

All watched repositories are in discovery scope, including pre-existing assets
and newly discovered repositories. Enrollment requires demonstrable regeneration
provenance, a compatible consumer, safe ownership/path boundaries and authorized
private storage. Extension or size alone never grants eligibility. Existing
published history, tracked baselines, operator materializations and required
hooks remain intact. Unsupported cases must be visible, not silently skipped.

## Initial fleet baseline

`evidence/auto-registration-2026-10-16/fleet-initial.json` inventories 34
repositories and 10,797 Git-visible supported-media candidate paths. This is
**not** 10,797 eligible assets. Only dracon-platform has an explicit enabled
private workflow configuration at this baseline. The status report returned 33
rows and one inspection timeout for dracon-platform; the inventory includes
that failed repository explicitly. It did not treat an incomplete status report
as a complete discovery snapshot.

Reproduce the read-only inventory after saving `dracon-sync repos --json`:

```
python3 scripts/inventory-bucket-discovery.py STATUS.json \
  --failed-repo /home/dracon/Dev/dracon-platform \
  --output evidence/auto-registration-2026-10-16/fleet-initial.json
python3 scripts/test_inventory_bucket_discovery.py
```

The script reads filenames/configuration metadata, not asset contents or
credentials. Generator-like filenames are only hints, never trusted commands.
The final qualification must refresh this baseline against current discovery.

## Confirmed existing seams and gaps

- `bucket_assets::prepare` currently returns immediately without repo config;
  it runs a repo-selected Node workflow before computing/staging ordinary diffs.
- Publication serializes existing manifest entries under a kernel-owned flock,
  verifies immutable remote objects by read-back and returns a manifest digest.
  There is no candidate-discovery or enrollment operation in that loop.
- Existing generator command strings/empty input lists do not prove a candidate
  can be regenerated. Never promote them directly into automatic authority.
- Repo-local workflow resolution prevents using one installed shared workflow
  from independent new repositories. Copying sibling-checkout paths into config
  would not fix packaging or fresh-consumer portability.
- Quiet/new repositories need enrollment work even when their ordinary Git diff
  is clean. Reuse the daemon's complete repository discovery snapshots rather
  than introducing another independent recursive fleet walker.
- Existing restore used rename over an existing destination, which could discard
  operator edits. This prerequisite is now changed to create-only hard-link
  publication: matching existing bytes are left untouched; conflicts fail;
  concurrent consumers may succeed only if the destination also verifies.
  Seventeen transfer tests pass, including three new no-overwrite regressions
  (`restore-safety.log`). This is a bounded prerequisite, not full qualification.

## Architecture and implementation boundaries

### 1. Shared trusted workflow and explicit storage authority

Ship one versioned workflow with the utility rather than executing arbitrary
new per-repo generator commands with uploader access. Resolve it through an
operator-controlled installed profile, bind the installed bundle to its tested
hash/version and preserve existing explicit opt-outs. Bundle its complete local
module/dependency closure; do not depend on dracon-platform being present as a
sibling checkout. A configuration/manifest remains portable repository data,
not new executable authority.

The profile selects already-authorized bucket/prefix and explicit scoped
uploader/consumer credential locations. Do not expand IAM or inherit ambient
credentials. Derive collision-resistant per-repository namespaces within that
prefix; preserve established namespaces and compatibility for existing pilot
entries. A repository without a valid storage/consumer capability is reported
unsupported until those capabilities actually exist.

### 2. Candidate discovery is not enrollment authority

Enumerate bounded Git-visible tracked/nonignored-untracked paths per discovered
repository. Never descend through gitlinks or nested Git ownership boundaries,
symlink ancestors, excluded/build/cache/secret paths. Preserve filenames safely
(NUL boundaries, no shell interpolation), and distinguish enumeration errors
from an empty candidate set. Revalidate paths and source identities before use.

Use a versioned adapter registry whose implementations are shipped/trusted
with the workflow. An adapter must positively verify regeneration evidence and
produce stable IDs, exact source/input digests, adapter version and expected
output identity. Do not execute a repository's arbitrary command string or
accept self-declared `regenerable:true` as proof. The first production adapter
should cover the existing deterministic brand favicon; additional adapters
must satisfy the same verification contract, not loosen it. Consider generic
bounded deterministic PNG recipes and recorded, pinned generator receipts,
but do not claim either is supported until implemented and tested.

For each repository/candidate, emit an explicit outcome such as eligible,
already registered, excluded, unsupported generator, unsupported consumer,
ambiguous ownership, source changed, or inspection failed. Initial inventory
hints do not become positive capability evidence.

### 3. One enrollment/publication transaction

Discover and validate registrations inside the same publication lock used by
upload. Assign stable collision-resistant IDs and unique content-addressed
object templates. Compare source, provenance inputs and prior manifest again
before atomic publication. Upload and independently verify before committing
any bucketed reference. Registration must not race ordinary staging of newly
selected raw bytes; the index verifier must cover newly enrolled paths too.

Keep existing small eligible assets unbucketed until normal routing selects
them. Preserve original tracked baselines and all local source bytes. New
registration does not delete an existing Git path to manufacture savings.
Changes to existing registered bytes require renewed eligibility evidence;
unsupported edits cannot inherit old proof or be silently discarded.

### 4. Lifecycle and consumer compatibility

A missing source is not proof of an intentional deletion and must not trigger
unconditional restoration during enrollment. Preserve existing references on
ambiguous rename/delete events, report them and use explicit lifecycle state;
never delete remote immutable objects as cleanup. Repeated observations are
idempotent. Partial uploads may leave harmless verified immutable objects,
never a published unverified reference.

A consumer must restore the original logical/source path from independent
cache/storage without replacing differing local bytes. Positively qualify the
supported application's build/restore entrypoint before enabling enrollment;
a filename extension is not a consumer adapter. Preserve operator-owned build
scripts and make any restore wiring reproducible in a fresh checkout.

### 5. Scheduling, visibility and rollout

Use existing discovery snapshots and bounded single-flight per-repository work
for initial/periodic clean-repo scans as well as changed/new repositories. A
slow or failed asset scan must not monopolize unrelated commit/push capacity.
Persist explicit unsupported outcomes and retryable failures distinctly.

Deploy only after end-to-end and fault qualification plus final workspace and
script gates. Use sanctioned maintenance, atomic artifact installation,
identity checks and rollback copies outside watched source repositories.
Demonstrate real autonomous discovery/registration, policy-selected routing,
consumer restoration, retry recovery and unrelated repository progress.

## Component ownership for implementation

This is multi-seam work. Keep one writer per checkout/worktree:

1. Utility integration: installed workflow/profile resolution, repository-scan
   dispatch, index binding, CLI/reporting, scheduling and Rust tests.
2. Transfer/discovery: adapter verification, manifest enrollment transaction,
   safe restoration, Node tests and fresh-consumer harness.
3. Integration/qualification only after those contracts are stable: packaging,
   installation, final gates, live observation and safety review.

### Architectural review disposition

The independent reviewer identified three valid P1 gaps: an executable
provenance verification protocol, independent storage/consumer authority, and
quiet-repo scheduling plus stable ownership. All three are accepted for inline
implementation; none is a reason to weaken eligibility or narrow discovery.

The adapter protocol is `probeCandidate(context) -> explicit outcome + Evidence`
and `verifyEvidence(evidence, uploadSnapshot, trustedBinding) -> expectedDigest`.
Evidence binds schema/adapter version, stable repository ID, relative source
path, exact source digest/size, deterministic recipe digest, toolchain identity,
input digests and consumer adapter identity. The upload snapshot must reproduce
exactly; changed bytes never inherit old proof. The first bounded implementation
is a trusted deterministic solid-RGBA PNG recipe (covering the actual existing
32x32 brand favicon), with strict size/dimension/format caps and exact canonical
byte comparison. Unsupported PNG encodings remain unsupported, not guessed.
This is real regeneration from a bounded numeric recipe, not embedding arbitrary
asset bytes and calling that provenance. No repository command is executed.

Shared runner installation must include its dependency closure. Repo descriptors
must be credential-free; a separate operator-owned fleet capability profile
supplies permitted namespaces and credential handles. A portable descriptor does
not authorize a namespace on its own. Build bootstrap must be qualified against
recognized entrypoints before enabling automatic routing; API availability alone
is not a supported consumer. Neither IAM expansion nor relabeling arbitrary
non-game assets into the pilot's games prefix is authorized.

The scheduler must inspect clean repos before the ordinary clean-sync dispatch
gate, maintain bounded single-flight work and retry deadlines, and serialize
publication with ordinary repo mutation ownership. Stable generated bindings are
automatic daemon state plus portable repository descriptors, not an operator's
manually maintained per-repo registry. Conflicting copied descriptors must fail
closed; path rename must not churn a valid established namespace.

### Concrete authority and binding protocol

`scripts/bucket-runner/capabilities.mjs` now validates an immutable version-1
operator profile: one hashed installed runner, the existing private bucket,
HTTPS endpoint/region, separate explicit uploader/consumer credential-file
handles, authorized asset subtrees, and supported consumer protocol versions.
The profile loader refuses nonprivate/group-writable files and symlink paths;
it does not open credentials. Authorized asset **trees** cover future nested
repositories automatically, not a manually maintained per-repo enrollment list.
A tree authorization is only a prefilter, never provenance or consumer proof.
The live profile is not installed/enabled by these component tests.

Bindings are separate trusted state (`schemaVersion`, `profileId`,
`repositoryId`, `namespace`). Transport checks every key against the bound
namespace plus a full-SHA immutable identity independently of manifest claims.
The read-only transport has no upload/delete/provisioning method. Tests cover
caller-mutation isolation, cross-namespace/key attacks and capability-file/
runner integrity (six tests; `capability-tests.log`).

The automatic binding store must persist a random stable ID, the canonical
Git-common-directory identity and an enrollment commit anchor. A directory
rename retains the same identity and namespace. A new checkout carrying an
existing descriptor must also establish the trusted remote identity and anchor
ancestry before reusing an uploader binding; copying a descriptor into unrelated
history is a conflict, not authority. Consumers may read already-authorized
portable references without acquiring an uploader binding. Empty/missing or
ambiguous repositories receive explicit outcomes instead of guessed ownership.

### Component review and validation

The adapter component was independently reviewed at `a806ae7`: no production
blocker found; the P2 scheduling-dependent concurrent-write test was valid and
is replaced in canonical integration by a coordinated same-size valid PNG
replacement inside the read phase. It asserts exactly `retryable/source-changed`
and separately verifies the stable replacement is eligible. No production
probe callback was introduced for testing. Twelve combined adapter/capability
test groups pass (`component-tests.log`). No broad live enrollment is enabled.

The worker's original adapter commit carried an `okDraconDev` author-name
anomaly (canonical email); the integration committer is canonical DraconDev.
Published metadata is retained, not rewritten; subsequent component commits
must specify the canonical name/email explicitly.

### Enrollment planner

`scripts/bucket-runner/planner.mjs` is a read-only enrollment planner: it
enumerates Git-visible tracked/nonignored-untracked media under a positively
qualified consumer scope, drops all inherited Git overrides, validates the
root, runs the provenance probe and either extends the prior manifest or
returns a new manifest for the publisher. It never uploads, never modifies
source bytes and never rewrites legacy entries. Outcomes explicitly cover
unsupported adapter/consumer/storage scope, missing source, retryable source
change, symlink/nested-git paths and git-ignored files. Stable collision-
resistant IDs derive from `(repositoryId, sourcePath)` so repeats are
idempotent and new files get added without touching existing ones. Seven
planner tests pass.

### Atomic publisher, verify, and index binding

`scripts/bucket-runner/publisher.mjs` closes the enrollment transaction: it
acquires the existing publication lock (kernel `flock` via a tiny shell
helper that dupes the parent's lock fd), re-runs the planner against the
on-disk manifest, downloads the immutable object (or copies from the worktree
and `putIfAbsent`s it), then `verifyEvidence` against the actual bytes.
Successful uploads mark `bucketed: true` and the index record is rewritten
under the same lock, binding the trusted namespace to the new manifest's
digest. Refused uploads leave the worktree, prior manifest and index
unchanged. Eight publisher tests cover write+verify+bind, refusal on remote
corruption, namespace isolation, race-with-concurrent-create, concurrent
publishers and unsafe manifest paths. Combined adapter/capability/planner/
publisher tests: 27/27 pass (`all-component-tests.log`).

### End-to-end registration, restoration and fault-recovery qualification

`scripts/bucket-runner/e2e.test.mjs` is the single bounded harness that
satisfies the goal's third and fourth verification contract bullets
(end-to-end automated harness + bounded fault/recovery harness). It runs
entirely in-process against an in-memory transport and never starts a daemon
or uses live storage. The harness executes eight scenarios:

1. **Unregistered eligible asset** enrolls, verifies, writes manifest, binds
   the index and is then restored by a fresh consumer that has no shared
   state with the publisher. The restored bytes match the source digest.
2. **Ineligible assets** (non-media extensions) are never uploaded: the
   manifest stays empty and zero transport write/read calls leak.
3. **Repeat enrollment is idempotent**: the second pass writes no manifest
   and triggers no second upload, and the manifest digest is unchanged.
4. **Storage outage**: `putIfAbsent` always fails with a 503-style error;
   the manifest is absent, the source remains intact on disk and a retry
   after the outage recovers cleanly.
5. **Partial upload**: `head` lies and `download` returns garbage; the
   publisher detects the SHA mismatch, refuses to write the manifest, and a
   retry with a clean transport succeeds without leaking the corrupt object.
6. **Concurrent manifest change**: a stale manifest is rewritten between
   reads; the publisher's commit-time compare-and-set rejects, and the
   retry re-reads and re-verifies. After a successful recovery, no manifest
   entry is `bucketed: true` without a verified, addressable object.
7. **Simulated daemon interruption** between the storage upload and the
   manifest write: the manifest is absent, the storage object is pending
   retry, the worktree is intact, and a retry finishes the registration
   without duplicating the index entry.
8. **Renamed asset** (original deleted, new file added): the new file is
   enrolled, the legacy entry is preserved for any reader still holding its
   digest, and a fresh consumer can resolve both the new and the legacy
   entries by their respective SHA-keyed objects.

`scripts/verify-bucket-registration-e2e.sh` runs the full bucket-runner
suite (35 tests) and asserts every required scenario executed; a misspelled
filter or a missing test fails the gate. The latest run is captured at
`evidence/auto-registration-2026-10-16/e2e-registration-tests.log`.

No live profile is installed and no `bucket-assets.json` opt-in is enabled.
Live qualification remains task 6 and is gated on the fleet rollout decision.

### Active lane board

| Lane | Checkout / isolation | Exclusive authority | Gate / handoff |
|---|---|---|---|
| Adapter component | `~/.local/state/dracon-sync/dev-worktrees/bucket-adapters-hpFRn5` (detached worktree of dracon-sync at `20893c8`) | New `scripts/bucket-runner/adapters.mjs` and its test only; no cloud/config/integration changes | Node adapter tests; committed or durable file handoff |
| Parent utility integration | Canonical dracon-sync checkout | Inventory/design, capability/profile/runner integration, scheduling and CLI; no writes in adapter worktree | Consume tested adapter handoff before integration |
| Parent transfer prerequisite | dracon-platform | Create-only restore and focused regressions only | 17 Node tests passed; no other active platform writer |

The initial scout and architecture reviewer were read-only. The adapter lane is
one independently testable provenance seam, not an issue-wide writer assignment.
The parent remains the integration owner after the component handoff.
