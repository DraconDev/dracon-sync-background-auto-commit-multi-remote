# Dracon Sync: general Git and object-storage implementation plan

Status: implementation underway; see the [delivery ledger](storage-delivery-ledger-2026-10-01.md).
Writing this document does not enable
uploads, change file placement, alter retention, or authorize history rewrites.
Date: 2026-10-01. Operator: DraconDev.

## Objective

Make external payload preservation a reusable, optional Dracon Sync feature
for any watched repository: software, games, research, documents, media, or
other workflows. Keep Git history useful without repeatedly adding large or
frequently changing payloads. Repository layout is independent of storage:
standalone repos, nested repos, and submodules remain supported.

The Dracon fleet supplies incident evidence and rollout candidates, not
hard-coded product rules. No platform directory, music namespace, personal
identity, or particular bucket is required by the feature.

Success means a repository version can be recovered with its exact assets,
Warden protection remains effective, and a failed bucket operation cannot
silently put external payloads back into Git or stall unrelated repositories.

## Local evidence motivating the general feature

- dracon-sync 0.113.92 fixes classification cooldowns and resolves TOUCHED
  aliases. It does not implement a fleet-wide automatic storage policy.
- GitHub's 2 GiB limit applies to one push, not total repository size.
  Local disk usage, reachable Git history, projected push bytes, and provider
  account quotas must be reported separately.
- The platform's current 5.5 GiB bucket-guard budget is an operator growth
  budget. It is not a provider guarantee; this plan does not raise it.
- The September catalog audit found 15.9 GB of historical blob content from
  repeated catalog revisions. The recent Strategy stall showed a separate
  64 MiB Warden filter bound. File-size policy alone misses both churn and
  whole-file processing costs.
- The shared bucket planner and reviewed publisher exist; the reviewed
  publisher is currently restricted to music. The shared resolver supports
  local caches and immutable asset references.
- Doomtap has a separate automatic clean/smudge-filter pilot. Its clean
  filter uploads during Git operations, changes the manifest separately,
  overrides Warden for selected extensions, and falls back to raw Git blobs
  when credentials/uploads fail. Do not extend these behaviors fleet-wide.
  Audit and consolidate the pilot before changing its live configuration.

References:
- `docs/design/parent-bloat-quarantine-analysis-2026-09-26.md`
- `docs/design/sanctioned-slimming-2026-09-26.md`
- `dracon-platform/web/docs/bucket-strategy.md`
- `dracon-platform/web/scripts/bucket-promote.mjs`
- `dracon-platform/web/games/wip/doomtap/scripts/assets/filter-clean.py`
- GitHub: https://docs.github.com/en/get-started/using-git/troubleshooting-the-2-gb-push-limit

The July storage/LFS documents contain historical assumptions, pricing, and
conflicting recommendations. In particular, deleting/untracking a file does
not shrink existing reachable history, and generation prompts alone do not
guarantee reproduction of the exact original media. This plan supersedes
those assumptions for new implementation; sanctioned maintenance remains a
separate procedure.

## Product scope and configuration

Ship the shared implementation and commands in the Dracon Sync repository.
Document the public configuration and reference format there. Existing platform
scripts are evidence or adapter candidates, not a runtime dependency; useful
components must be extracted with their licensing, tests, and contracts checked.

External storage is disabled unless configured. Existing commit-all behavior
and file limits remain unchanged for users who do not opt in. Support global
policy with explicit per-repository overrides, named backends, ordered path
rules, and an explanation command that reports the effective rule and reason.
A rule selects Git or external preservation, with optional size conditions,
privacy requirements, and retention/recovery policy. Detect conflicting or
invalid rules before processing files. Do not infer a workflow from repo names.

Use a backend interface for immutable put, verified get, access checks, and
capabilities. Start with a local test backend and an S3-compatible production
adapter; add other providers behind the same contract later. Backends advertise
which checks and durability guarantees they support. Credentials stay in the
existing secret/configuration facilities, never in committed references.
Separate configurable provider constraints from operator growth budgets.

References and restoration must work on another machine and after a repo move.
Commit portable backend identifiers and the versioned restore format, not local
absolute paths or credentials. Provide documented hydration/verification through
the packaged CLI without requiring a running daemon or platform scripts.

Warden is the first security integration, not a mandatory dependency for every
user. Plain public/non-sensitive assets can use the adapter directly under an
explicit policy. A rule requiring encryption must fail closed when the security
integration or keys are unavailable; never silently downgrade protection.
Preserve compatibility with existing Git filters and reject unsupported filter
compositions with an actionable diagnostic.

## Proposed operator profile, not universal defaults

Placement is deterministic per path and purpose. A repository crossing a
size threshold does not silently change placement or migrate history.

| Content | Default placement |
| --- | --- |
| Source, tests, authored docs, configs, generator inputs | Git, protected by Warden where required |
| Small final media changed infrequently | Git |
| Declared final media over 20 MiB | Object storage with a versioned reference |
| Declared render intermediates or frequently regenerated media | Object storage regardless of the 20 MiB threshold |
| Large generated catalogs | First assess canonical inputs and sharding; do not route arbitrary JSON by extension |
| Logs, sessions, database snapshots | Separate archive policy; preserve current behavior until that policy is implemented |

20 MiB is a proposed threshold for our pilot profile, not a provider limit or
a universal Dracon Sync default. Other users choose their own declared paths
and thresholds. Repos can set documented path-specific exceptions. Keep existing Git media as legacy
content by default. Initial opt-in applies to declared new paths; converting
an existing tracked path requires a reviewed migration that handles hooks,
consumers, and recovery. Existing versions remain available.

Private repository membership does not make an asset public, and a media
extension does not make its contents non-sensitive. Publication is explicit.
Unknown classification stays visible for review; never silently upload it.

Initially retain every recorded external version. No automatic expiry,
bucket deletion, cache eviction, raw-file deletion, or Git history rewrite
is included in this rollout. Later retention policies require an explicit
operator decision. Keeping versions forever moves growth to object storage;
it does not eliminate storage costs.

## Responsibilities and durability contract

The security integration owns classification, encryption, and sensitive
metadata protection; Warden supplies that integration for the Dracon fleet.
Sync owns scheduling, preservation status, and committing the exact version.
A shared object-storage adapter owns bounded upload/download and verification.
Git owns source history and immutable references. Object storage owns payloads.
The adapter is invoked by Sync; do not add an independently racing daemon.

References must record schema version, stable repository ID, logical path,
backend/object identity, payload byte count and digest, encryption format,
and the restore contract. Public digests may address public media. Private
references must avoid exposing plaintext digests or sensitive paths; protect
metadata with Warden where needed. Do not use a repo's mutable HEAD as its
storage namespace. Namespace prefixes are not access-control boundaries.

A completed preservation operation means:
1. Capture a stable source snapshot; detect edits during capture/upload.
2. The configured security integration prepares the approved representation,
   encrypting if required (Warden in the Dracon fleet).
3. Upload immutable bytes and verify length, digest, and intended access.
4. Record recoverable references and manifests in the same Git index snapshot.
5. Commit/push through existing hooks, then report the actual durability state.

An upload failure retains local data and a retry record. Never fall back to
raw Git storage for a path declared external. Never commit a reference to an
unverified object. A source edit during upload schedules a fresh snapshot;
manifest, pointer, and payload must agree on the committed version.

Uploads/downloads must not run inside Git clean/status/diff operations.
Filters may serialize an already-prepared local reference or refuse an
unprepared managed path. Resolve how the existing required Warden filter
composes with references before rollout: Git selects one effective filter,
so extension overrides cannot silently bypass Warden. Manual `git add` and
commits must obey the same rules as automatic staging.

Track pending-upload, verified-pending-commit, committed, and restore-failed
states durably. Retry across restarts with bounded concurrency/timeouts and
streaming I/O. Hold the Git index lock only for local staging/commit work.
A pending media file must not block unrelated source files or repositories;
Git commits and asset preservation each get truthful status.

## Phases and completion gates

### 1. Inventory and policy simulation

Define the general configuration and backend/reference contracts first.
Produce read-only inventories across representative repository shapes and
content classes; use platform, music, one game, and video output as local cases:
current files, tracked/ignored state, history growth by path/class, existing
manifests, filters, publishers, and consumers. Measure old raw blobs separately
from actual stored history. Review existing producer services for stale writer
code and large-file rewrite patterns.

Add a policy simulator that explains placement and estimates future Git versus
external growth without uploads or index changes. Identify the initial 20 MiB
exceptions and intermediate-output paths. Document proposed settings and their
per-repository overrides; apply the policy coverage tripwire to new Sync knobs.

Gate: every pilot path has an explicit reason, privacy class, retention rule,
and restore requirement. No existing file silently changes storage.

### 2. Storage and security prototype

Implement the shared reference schema, security interface, and Warden adapter
inside Dracon Sync against a local
fake object backend. Reuse reviewed publisher/resolver components where their
contracts fit. Keep public publishing distinct from private preservation.
Specify and test filter ordering and behavior for both manual and daemon Git.
Decide whether existing manifests are sufficient or a pointer format is needed;
avoid building a second incompatible format without this comparison.

Gate: identical behavior without project-specific paths or services;
standalone, nested, and submodule repo coverage; same bytes/digest round-trip; encrypted restoration with authorized keys;
no plaintext or metadata leak; Warden is never bypassed; no network in Git
classification; retry never changes an existing immutable object.

### 3. Sync integration and recovery

Add the durable per-file state machine, selective staging, upload verification,
asset-aware status, and an explicit hydration/verification command. Ordinary
Git paths must retain current behavior. Cache hits permit offline development;
a cache miss reports missing assets clearly, without treating pointer text as
valid image/video data.

Gate: tests cover upload failure, lost credentials, source edits during upload,
process death at each phase, concurrent/manual commits, duplicate uploads,
corrupt/missing objects, files over 100 MiB, bounded memory, offline cache
hits/misses, and encrypted restore. A bucket outage cannot block unrelated
source commits. A cold checkout restores both the latest and an older version.

### 4. Controlled live pilot

Exercise the packaged CLI in a fresh generic repository with no platform
checkout, both with and without Warden. Then opt in a small declared set of
music assets as the first local live pilot. Check live bucket access
and billing terms without exposing credentials. Require one verified
independent recovery copy in addition to the primary object store before
calling irreplaceable asset preservation complete. Existing Git mirrors alone
do not back up external payloads. Report a missing recovery copy as degraded.

Run upload, consumer resolution, and cold restore checks; observe retries and
Git growth. Next pilot one game's new assets and temporary/final video outputs.
Consolidate Doomtap's existing filter path only after preserving compatibility
and verifying every existing pointer can still be restored.

Gate: preserved current/older versions, healthy source syncing during failure,
no unexpected public objects, bounded Git growth for external paths, and
explicitly accepted storage/recovery cost. No automatic old-data deletion.

### 5. Rollout and separate legacy cleanup

Roll out declared paths per repo. Record exceptions and teach generators the
same policy. Revisit catalog structure separately. Keep current repo/submodule
boundaries and daemon coverage. Publish generic setup, backend, security,
rule-precedence, manual-Git, and recovery documentation with examples for
multiple workflows. Update AGENTS, examples, operator docs, release
notes, packaged-install fixtures, and all required workspace checks.

For old history, prepare a separate measured slimming proposal with exact
paths, verified backups, preserved shipped content, and rollback evidence.
Execute only under explicit operator authorization and the sanctioned procedure.

Gate: all opted-in repositories pass restore drills and report both Git and
asset preservation health. Release the feature only after these gates pass.

## Monitoring and rollback

Report local Git bytes, reachable stored history, projected push bytes,
configured growth budgets, external retained bytes, pending bytes, recovery-copy
health, and backend/account quota where available. Begin with warnings at 70%
and 85% of an explicit repository growth budget; these are planning signals,
not automatic migration triggers. Evaluate the real push cap separately.

Rollback stops new external enrollment and preserves the adapter needed to
read existing references. Retain all payloads, manifests, caches, and retry
records. Never switch declared external paths back to raw Git as an outage
fallback. Immutable writes left orphaned by a crash are recorded for later
review rather than automatically deleted.

## Decisions before implementation

- Confirm the general opt-in configuration and initial production backend.
- Confirm the simulated local media profile and exceptions; the proposed 20 MiB
  default is adjustable before any paths are enrolled.
- Confirm the independent recovery destination and acceptable recurring costs.
- Approve the first live pilot's exact paths and public/private classification.
- Leave retention at preserve-all initially; choose expiry separately if wanted.

No production storage changes or live bucket writes were performed while
preparing this plan.


## Architecture decisions and alternatives

| Decision | Recommendation | Reason and remaining evaluation |
| --- | --- | --- |
| Repo structure | Keep independent project boundaries | Submodules partition history but do not bound a child's growth; storage routing must also work without them |
| Trigger | Declared rules evaluated on file versions | Repo-size overflow is too late and does not identify which content belongs elsewhere |
| Initial backend | S3-compatible plus local test backend | Reusable protocol; qualify actual provider capabilities rather than assuming all implementations behave alike |
| Security | Optional interface, Warden adapter first | Preserve fleet guarantees without requiring every installation to use Warden |
| Git representation | Compare standard LFS with a versioned custom reference before selecting | LFS has familiar tooling; a custom representation must justify security, backend, and scheduling differences |
| Transfers | Durable scheduler outside Git filters | Slow network work cannot hold ordinary Git classification or staging hostage |
| Retention | Preserve recorded versions initially | Expiry is a separate decision with consequences for checkout recovery |
| Legacy history | Separate sanctioned maintenance | Routing new writes cannot reclaim published reachable blobs |

Evaluate LFS in the first design milestone with a real encrypted round trip,
provider access, manual-Git behavior, installed-tool requirements, and recovery
from a fresh machine. Select one canonical representation before implementing
production reference/filter code. Do not silently convert existing LFS-managed
paths. If custom references win, document their interoperability costs and the
reason LFS cannot meet the selected contract. This is a decision gate, not a
commitment to build a second storage system alongside LFS.

## Policy semantics and illustrative configuration

The following is a proposed shape, not a configuration accepted by today's
binary. Field names and command syntax become stable only after schema review.
The example uses generic paths rather than platform conventions.

```toml
[storage]
enabled = true
schema_version = 1

[storage.backends.primary]
type = "s3"
endpoint = "https://objects.example.invalid"
bucket = "development-assets"
credential_ref = "operator-managed-primary"

[[storage.rules]]
paths = ["assets/final/**"]
placement = "external"
min_bytes = 20971520
backend = "primary"
security = "warden-encrypted"
retention = "preserve-all"

[[storage.rules]]
paths = ["renders/intermediate/**"]
placement = "external"
backend = "primary"
security = "warden-encrypted"
retention = "preserve-all"
```

Resolve inherited settings first; per-repo rules replace the inherited rule
list as a complete list rather than accidentally accumulating overlapping
rules. First matching rule whose path and conditions match wins. A Git rule
can provide an explicit exception before a broader external rule. Reject
obvious duplicate contradictory rules; explain other overlaps and effective
ordering in validation/simulation. No match preserves existing Git policy.
Disabled storage permits reading existing references but no new enrollment.
An enrolled external path stays external when its size later falls below the
threshold; otherwise routine edits would oscillate between representations.
Changing that enrollment requires explicit migration. Persist this decision in
versioned metadata, not only the local retry database.

Git ignores and ownership gates remain authoritative. This feature does not
start backing up ignored directories or foreign repos automatically. Path
matching excludes Git internals and applies within the owning repository;
nested repositories get their own rules. Symlink traversal outside the repo
is refused. An explicit Git placement rule does not bypass secret policy or
existing staging/provider limits. Oversized unmatched files remain visibly
blocked rather than acquiring an invented external destination.

Treat committed repository configuration as untrusted input. A clone must not
choose a new upload destination, credential binding, public ACL, external
command, or local file access merely by declaring it. Operator configuration
binds approved backend IDs and security capabilities; repo overrides can select
only those allowed bindings. Unknown bindings require explicit setup. Simulation
and ordinary read-only status never access credentials or upload payloads.
Public publication requires its own approved capability, not just a repo rule.

## Git representation and editing contract

Prototype the everyday edit/commit/checkout experience before choosing the
representation. The desired working tree contains usable payload bytes while
Git stores a compact reference. A clean filter is deterministic and local:
it looks up a prepared immutable reference for the exact current plaintext
version, or rejects an unprepared managed path. It must not return the last
prepared reference after the file has changed. Source identity comparisons
must be collision-resistant and streamed; timestamps alone are insufficient.
Sensitive source fingerprints belong in protected local state, not public
references or logs. Reuse prepared encrypted snapshots to avoid producing a
new ciphertext/object on every unchanged `git add`.

Install the effective filter through an explicit setup operation. Required
filter failure prevents raw payload staging; validators/pre-commit checks
catch accidentally removed attributes and references that bypass enrollment.
Prototype Warden composition with real Git to verify its encryption/filter
bounds do not apply to raw external payloads before routing. Streaming encryption
must meet Warden's own contract; lifting its current bound is not an assumption.
No feature can prevent a determined user from disabling local hooks; document
that limit and the server-side checks available to installations that need it.

Hydration is explicit in the first release. Networkless smudge may use a verified
local cache; otherwise it leaves a recognizable reference and reports that
hydration is required. Missing/corrupt assets must produce a failing verification
result. Checkout never overwrites modified payloads to satisfy a reference.
An older checkout resolves the reference committed in that version, rather
than whichever object currently occupies the original logical path.

Git attribute installation/migration must preserve unrelated attributes and
existing filters. Unsupported combinations are refused, not overwritten. Parent
submodule advancement cannot advertise full asset preservation when the child
has unresolved payload durability; report child Git and asset states separately.
Define a pre-push validation path that checks the exact outgoing refs locally
against verified receipts and reference records. Remote push acceptance and
asset durability are separate facts.

## Durable transfer and commit state machine

| State | Durable evidence | Next action |
| --- | --- | --- |
| Discovered | Repo identity, path, source version, effective policy version | Capture a stable snapshot |
| Captured | Local snapshot, fingerprint, security decision | Prepare representation |
| Prepared | Immutable bytes, protected restore metadata | Queue backend upload |
| Uploading | Idempotency/object identity and attempt record | Resume or verify interrupted attempt |
| Primary verified | Verified primary receipt and payload identity | Obtain required recovery copy |
| Ready to stage | All required receipts and exact reference bytes | Recheck current source and index |
| Staged | Index contains the matching version/reference | Commit or reconcile manual commit |
| Committed | Commit/ref contains the reference | Push under existing Git policy |
| Preserved | Required Git destinations and object copies verified | Report success and retain versions |
| Retryable failure | Snapshot and typed failure retained | Backoff with jitter and bounded retries |
| Intervention required | Policy/security/conflict/missing-data reason | Keep local data; report actionable status |

Jobs identify a source version, not just a path. Source edits, renames, deletions,
and branch changes require reconciliation rather than reuse of stale work.
A deleted path can cancel unpublished work without deleting any payload already
referenced by history. Maintain a versioned journal/database with transactional
updates and restrictive permissions. Choice of database versus atomic records
must be supported by crash tests and dependency review in the prototype.
Recovery rechecks durable evidence; it never equates an interrupted request with
upload success. Corrupt local state is quarantined and rebuilt from references,
receipts, and preserved snapshots where possible without deleting source data.

Do not swap a hydrated working file for a pointer as an implementation shortcut.
Use Git filters/index plumbing with a short lock, revalidate source identity,
and defer on manual index conflicts. Do not overwrite the user's staged version
with a newer working-tree upload. Use a per-repository coordination primitive
for daemon/CLI staging and a separate per-job lease for transfers; test ownership
recovery after process death. A manual commit may finish a queued job: reconcile
by inspecting actual committed references rather than manufacturing a commit.

Allow unrelated paths to commit during a bucket outage, but preserve declared
atomic groups when source and asset versions must ship together. Support an
explicit group rule or defer the selected group with a clear status. Do not
assume all source edits are independent of pending assets. Policy explanation
must show this grouping and its effect on staging.

Bound transfer memory, temporary disk use, active jobs, network bandwidth, and
per-repo scheduling share. Preserve-all cannot capture infinitely many versions
under sustained churn during an outage. Use producer settling and coalesce
uncommitted versions by default, while retaining every successfully recorded
version. Workflows requiring every transient frame need an explicit capture
contract and capacity plan; Git debounce is not such a guarantee. Disk pressure
pauses new capture and reports unsaved risk without deleting live sources or
uncommitted snapshots. Respect the existing guard rather than inventing another
cleanup daemon.

## Backend, security, and recovery requirements

- Immutable writes use provider-enforced create-only semantics where available;
  compare existing object identity before reusing an existing key. Never use a
  mutable logical-path key as the only version identity.
- Compute a strong payload digest while streaming; do not treat multipart ETag
  as a content digest. Record length, digest algorithm, and format version.
  Define when readback is required; a generic successful PUT/HEAD alone is not
  evidence of arbitrary provider integrity guarantees.
- Interrupted multipart uploads need bounded resource accounting and an explicit
  provider-supported cleanup policy; payload/version deletion remains separate.
- Retry throttling/transient errors with backoff. Authentication, policy, quota,
  and immutable-key conflicts get typed diagnostics instead of an infinite loop.
- Verify private access configuration with least-privilege capabilities. Avoid
  tokens, signed URLs, plaintext names, and sensitive digests in terminal/JSON
  logs. Do not follow arbitrary destinations contained in fetched references.
- Encrypt before egress when required. Version encryption/key identifiers and
  document key backup, rotation, revoked access, and historical decryption.
  Provider-side encryption alone does not replace Warden's protection contract.
- Independent recovery storage has separate failure/credential boundaries as
  configured by the operator. Two prefixes in the same bucket do not establish
  independence. Git remotes and object replicas are both part of recovery status.
- Recovery requires Git history, reference/manifest metadata, payload copies,
  approved backend bindings, and decryption keys. Test restoration without the
  original local database/cache or a running daemon.
- A restore drill compares exact recovered bytes for both current and historical
  versions. Report last verified drill and unresolved missing copies, not only
  most recent upload success. State which failures the selected backend and
  recovery profile tolerate; do not promise durability merely from replica count.

## Proposed CLI and observability contract

Command names below are design proposals, not available commands:

| Command family | Behavior |
| --- | --- |
| `storage plan` | Read-only effective policy, reasons, byte estimates, and enrollment/migration preview |
| `storage validate` | Validate rule ordering, approved bindings, security/filter compatibility, and schemas |
| `storage setup` | Explicitly install compatible attributes/filters and local approved bindings |
| `storage status` | Jobs, retained/pending bytes, backend health, required copies, and typed concerns |
| `storage prepare` | Capture/upload/verify selected declared paths without requiring the daemon |
| `storage hydrate` | Restore selected references for a checked-out version, preserving local edits |
| `storage verify` | Verify references and selected object copies; return nonzero on missing/corrupt data |
| `storage migrate` | Separate dry-run/apply forward-only conversion of reviewed tracked paths |

Every mutating command lists its scope and supports a reviewable plan. Read-only
commands never upload. Define stable JSON schemas and exit codes for complete,
pending, degraded, invalid configuration, and failed verification. Integrate
summary status into `repos`/`health` without forcing every fast report to scan
history or contact all backends. Deep checks are explicit and cached with ages.
A green Git push alone must not make a repository green when required payload
copies are missing. Distinguish optional recovery targets from required ones.

## Implementation work packages and dependencies

| Work package | Deliverables | Depends on | Completion evidence |
| --- | --- | --- | --- |
| A: contracts | Representation/LFS decision, threat model, policy/CLI/schema spec, operator examples | Inventory | Reviewed decisions and generic workflow prototype |
| B: policy | Types, validation, overrides, rule explanation, enrollment semantics | A | Round-trip/precedence tests and override coverage tripwire |
| C: storage core | Local/S3 adapters, streaming verification, security interface | A | Backend conformance and private/public round trips |
| D: journal | Versioned durable states, leases, crash recovery, limits | A | Restart/fault tests at each transition |
| E: Git bridge | Required local filters, source/index checks, outgoing-ref validation | B, C, D | Real-Git manual/daemon race tests; no raw fallback |
| F: restoration | CLI/cache/hydration, historical recovery, key binding | C, E | Cold restore without original local state |
| G: daemon/status | Fair scheduling, staging/group integration, report/health JSON | B, D, E | Outage leaves unrelated Git syncing; truthful required-copy health |
| H: pilots/release | Generic clean-machine install, local fleet pilots, docs and release | F, G | All release gates and recorded restore drills |
| I: legacy maintenance | Path inventory, backups, separately authorized slimming | Measured pilot results | Sanctioned maintenance evidence; never a prerequisite disguised as feature work |

Use existing `src/policy.rs` for `SyncPolicy`, `RepoPolicyOverride`, validation,
and the override tripwire. `src/sync.rs` staging paths, including bootstrap and
`clean_staged_paths`, must route enrolled payloads before applying raw-file
limits; ordinary Git paths retain their existing safeguards. Audit all staging
entry points, not only the main happy path. Integrate commands through
`src/main.rs`, scheduling through `src/daemon.rs`, and concerns/status through
`src/report.rs`. Put new logic in focused modules rather than extending the
large sync/report files with another inseparable subsystem. Review changes
needed in the published `dracon-git` dependency before planning local-only APIs.

Keep work packages independently reviewable, with new production behavior
behind the disabled-by-default switch until the integration gates pass. Do not
assign calendar dates until the representation and Warden prototype exposes
actual work. Sequence A first; B/C/D can proceed independently after contracts,
then E, F/G, and H. Storage architecture does not require rewriting existing
project layouts or adopting a new release process.

## Verification and release matrix

| Area | Required scenarios |
| --- | --- |
| Compatibility | Disabled feature matches current behavior; old configs; unknown reference/schema versions; existing LFS and Warden filters |
| Repository shape | Standalone, nested standalone, submodule on main, repo relocation, fresh clone, linked worktree |
| Content | Empty and threshold-boundary files; huge streaming payload; unusual filenames; symlinks; sensitive media; generated high-churn data |
| Policy | Global/per-repo inheritance; explicit false; order/overlap; sticky enrollment; ignored paths; unapproved backend and publication |
| Git concurrency | Working-tree edits during capture/upload; manual staging/commit; branch switch; rename/delete; atomic groups; index lock contention |
| Failure | Credentials revoked; quota/full disk; timeout/throttle; interrupted multipart; corrupt/missing object; journal corruption; kill at every state boundary |
| Recovery | Offline cache; cache miss; latest/older version; private key absent/restored; primary unavailable; independent copy; no original local journal |
| Security | No raw fallback; no filter bypass; private metadata/log protection; untrusted repo config; path escape; malicious reference endpoint |
| Performance | Large-file bounded memory/disk; fairness across repos; no network in status/clean; short Git locks; bounded retries |
| Packaging | Packaged CLI works without platform scripts, sibling source repos, local developer cache, or daemon |

Run focused fault/conformance tests during each work package. Final gates are
`cargo test --workspace --locked`, `cargo build --release --locked`,
`cargo clippy --workspace --locked -- -D warnings`, and `cargo deny check`, plus
packaged-install fixtures and the documented live restore drills. Confirm
commands and examples match the actual released binary. Review dependency
licenses/MSRV and provider-specific capability claims before release.

Do not release until every required invariant has an explicit passing test or
recorded operational drill. Keep a gate ledger with command/output artifacts,
backend profile, fixture version, and unresolved limitations. Passing the local
fake backend is insufficient evidence for production S3/provider behavior.

## Non-goals and later decisions

This first roadmap does not promise universal backup of all ignored files,
perfect capture of every transient filesystem write, automatic history surgery,
auto-publication, arbitrary execution of repo-provided upload scripts, or
automatic retention deletion. It does not make total object-store growth free.
Future archive profiles, retention/garbage collection, cross-provider replicas,
additional security adapters, and migration tooling follow measured needs.

Decisions that require operator preferences are explicit rather than blocking
planning: approved production backend/recovery cost, exact local pilot paths,
privacy classification, and retention beyond preserve-all. Engineering decisions
still requiring prototype evidence are the canonical representation, Warden
composition, journal format, capability-based verification, and atomic groups.
The first implementation deliverable is work package A's read-only inventory,
policy simulator design, and representation/security prototype; no live
migration or bucket upload is implied by accepting this roadmap.


## Implementation evidence

Read-only rule validation and inventory commands now exist in source builds;
see [preview usage](../storage-planning.md). The [representation decision](storage-representation-decision-2026-10-01.md)
selects standard LFS pointer encoding based on isolated experiments, with
Warden/filter/manifest production gates still outstanding. The [delivery ledger](storage-delivery-ledger-2026-10-01.md)
tracks completed evidence separately from remaining work. No automatic storage
routing, live uploads, retention changes, or history rewrites are enabled.
