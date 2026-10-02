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
- Shared Warden preparation adapter streams a verified captured source into a
  private bounded spool, enforces a subprocess deadline, refuses failed exits
  and plaintext output, and records approval before publishing the payload.
  Approved ciphertext survives restart without rerunning encryption; an
  unapproved artifact cannot create a prepared receipt.
- Actual 101 MiB Warden → journal → two local copies → cold recovery-store
  decryption passed with isolated fixture keys and exact original digest.
  These same-filesystem test copies do not certify independent failure domains.
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
| A: contracts/inventory | Partial | Private manifest schema specified; per-path churn attribution, producer/service review, full threat model and group contracts remain |
| B: policy | Partial | Versioned sticky enrollment, atomic group policy, recovery/retention settings, actual staging resolution |
| C: storage/security | Partial | Local streaming, Warden, retained protected metadata and isolated S3 conditional-write capability checks passed; actual provider/security certification remains |
| D: durable journal | Partial | Transactional records, leases, source snapshots, byte budgets and process-death tests checked; explicit captured-job preparation/copy execution checked; production reconciliation and operator resource policy remain |
| E: Git bridge | Partial | Strict pointer/private manifest codecs and retained Warden metadata preparation checked; Git metadata integration, required local filter composition, manual-index races, staging entry points and outgoing-ref validation remain |
| F: restoration | Prototype only | Packaged hydrate/verify commands, safe destinations/cache, historical key recovery, independent copy failover |
| G: daemon/status | Partial | Read-only redacted journal status checked; worker scheduling, live backend verification, fairness, grouping and outage isolation remain |
| H: live pilots/release | Not started | Approved backend/recovery cost and exact pilot paths, real provider drills, clean-machine fixtures, final gates/release |
| I: legacy maintenance | Separate proposal pending | Measured exact paths, verified backup/rollback, explicit sanctioned authorization |

## Next implementation sequence

Continue with the protected restore manifest, enrollment/operator binding
integration and production reconciliation around Warden and the copy executor. Add
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
workspace Clippy passed, followed by a final Sync all-target check after the
local failure-classification refinement. The locked release build passed. The
full workspace rerun passed 1969 tests (13 ignored), including the
28 shared-library tests and both actual Warden streaming integration tests.
Dependency policy remains green from the preceding milestone; this change
added no dependencies.

This remains library infrastructure: no new `storage prepare` command, production
CLI/operator enrollment integration, S3 adapter, manifest, filters or daemon worker
is present. No live storage paths/backends have been enrolled.

## Warden preparation composition milestone

The shared adapter now composes Warden with journal capture/approval/publication.
Synthetic subprocess tests cover failed exit, plaintext stdout, bounded output,
timeout/kill/retry, unapproved orphan handling and crashes on both sides of
security approval/publication. The real source-build Warden operational test
explicitly passed a 101 MiB fixture through preparation, both local copy receipts
and cold recovery-store decryption, without requiring local job state to restore.
No live keys, repo enrollment, filters or object endpoints were used.

Validation: all-target workspace Clippy, dependency checks and locked release
build passed. The full workspace run passed 1974 tests (15 ignored).
The final expanded core run passed all 35 tests (six ignored helpers/operational
checks), including the two added unapproved-output/recovery-revision regressions.
The real 101 MiB operational check was explicitly run and passed.
This milestone explicitly enables Tokio's existing `io-util` feature; no new
package or lockfile dependency was added.

Production bindings still must match owning repo ID, approved executable and
identity/security policy. The adapter does not classify non-sensitive uploads
or accept commands/recipients from an asset reference. CLI preparation,
protected restore manifests, historical-key recovery drills, S3 conformance,
Git staging and daemon scheduling remain release gates.

## Private runtime isolation and capacity wording

The default state base is inside the watched `.dracon` checkout; a read-only
`git check-ignore` confirmed the prospective storage-journal path was not
already excluded. No live journal/payload files were created. The framework
now installs exact private managed ignore protection in reserved journal
roots/namespaces and writable local stores before writing data. It refuses
project Git roots, already tracked runtime paths and modified ignore content.
Unmarked nonempty directories are also refused: unrelated operator notes or
source files cannot be hidden by installing a blanket ignore. Bootstrap uses
create-only private temporary files containing only the public ignore rule.
A real fresh-Git-repo test with a literal bracket/space path proves job records
and plaintext captures leave Git status empty; a tracked-path/tamper test proves
capture is refused without deleting existing bytes. Source commit-all policy
and existing project ignore rules are unchanged.

The current focused core run passed all 38 tests (six ignored). Local backend
failure assertions now distinguish private guard metadata from published
objects and still require interrupted upload spools to be removed. The latest
full workspace run passed 1979 tests (15 ignored), including the final
local-backend guard and unmarked-directory regression. All-target workspace
Clippy and the locked release rebuild passed. A read-only smoke check of the
built release's `repos --legend` confirmed the corrected capacity wording.

`repos` capacity wording now describes the push-size guard and avoids implying
that total `.git` size triggers automatic bucket migration. The size-color and
five legend coverage/wrapping tests passed. GitHub's current
[repository limits](https://docs.github.com/en/repositories/creating-and-managing-repositories/repository-limits)
confirm the enforced push limit is separate from its total on-disk guidance.
The cap concern's action text is provider-neutral. No history or asset placement
was changed by this wording update.

## Portable manifest codec milestone

The [manifest contract](storage-manifest-contract-2026-10-01.md) specifies bounded
private decoded metadata, sticky enrollments/tombstones, portable approved-copy
identifiers and exact pointer agreement. The shared codec validates sorted unique
paths/copies, rejects unknown schema/security/retention fields, redacts parse
diagnostics, and bounds both input and encoding to 4 MiB/10,000 entries.
Five focused tests passed, including non-UTF-8 paths, changed pointer bytes,
tombstones, contract drift, malformed untrusted fields and bounded encoding.

This is not protected manifest publication: the codec returns private plaintext
for a future metadata-security transaction. It installs no files/filters and
performs no network operations. Production contract derivation, authorization,
metadata encryption/reuse, exact index transactions and packaged historical
recovery remain outstanding. The expanded full workspace run passed 1984 tests
(15 ignored); strict all-target Clippy, dependency policy and the locked release
build also passed for this codec milestone.

## Repository-bound security adapter

Warden preparation now requires an operator-bound stable repository ID in
addition to executable/repo paths. Every preparation and approved-output recovery
compares that ID with the leased job before source access or child execution.
A foreign binding cannot encrypt a captured job or adopt an already prepared
representation, and rejection does not modify the other repository's failure
state. The caller still must establish the trusted ID-to-checkout mapping;
manifest/config strings do not constitute authorization.

The copy executor now requires typed operator-resolved bindings carrying the
owning repo ID and each backend's allowed representation classes. It refuses
foreign, incomplete/extra or disallowed-security copy sets before backend I/O.
An encrypted-only convenience grant cannot publish a plaintext job. Explicit
non-sensitive approval is required for that representation. The types cannot
be deserialized from a committed manifest and do not resolve operator config,
certify credentials/provider access, or prove independent-copy durability.
The added refusal test uses panic-on-I/O adapters to verify this ordering.

The combined core run passed 45 tests (six ignored). Strict all-target workspace
Clippy, the locked release build and formatting checks passed. The real 101 MiB
Warden preparation/two-local-copy/cold-decryption test was explicitly rerun with
these repository/security grants and passed in 96.56 seconds. The copies are
still same-machine fixtures, not independent failure-domain certification.
Dependency policy remains green; these guards add no dependencies or policy
knobs. The final expanded workspace run passed 1986 tests (15 ignored), including
both actual Warden streaming integration checks. Unreleased changelog entries
now distinguish checked infrastructure from pending automatic preservation.

## Protected manifest preparation (2026-10-02)

The dedicated metadata store captures a bounded manifest, runs the repo-bound
Warden adapter, approves fsynced verified ciphertext and retains it for future
Git staging. Its records do not invent object destinations or Git receipts.
Unchanged versions reuse the exact ciphertext without invoking Warden or
rewriting the record. Capture/approval/publication recover across process death;
unapproved output cannot be adopted, approved corrupt bytes are not reencrypted,
and limits preserve earlier versions. Corrupt partial source proof is a permanent
integrity concern. Deadline failures kill/reap the child and respect backoff.

The actual source-build Warden check encrypted a non-UTF-8-path manifest, saved
its ciphertext, removed only the test-owned temporary metadata store and restored
the exact manifest with separately retained fixture keys. It passed in 2.99
seconds; no live keys, buckets, filters or source repos were used. The shared
Warden transform and private capture/atomic-write primitives are reused rather
than implementing a second subprocess or filesystem protocol. Snapshot budget
and digest failures now have typed capacity/integrity categories.

The final expanded core run passed 53 tests (eight ignored), including the
corrupt-prefix regression. The full workspace run passed 1994 tests (17 ignored).
Strict all-target Clippy, formatting and the final locked release rebuild passed
after replacing equivalent manual saturation arithmetic. Dependency policy
checks passed for advisories, bans, licenses and sources.
No dependencies or configuration knobs were added. Production policy derivation,
root separation/approval, manifest group consistency, atomic Git staging,
historical-key drills, packaged restoration and daemon wiring remain gates.

## Atomic Git index infrastructure (2026-10-02)

Exact-version staging bundles now bind retained protected metadata to verified
leased asset jobs. The repository-bound index transaction builds matched pointer
and ciphertext entries, preserves unrelated staging and compares the actual
index fingerprint under Git's lock. A private intent and complete immutable
candidate reconcile process death before/after atomic replacement. Recovery
recognizes its own lock by device/inode, preserves foreign locks and releases
its artifacts when a manual edit changes the baseline. Verification forces a
fresh index read; libgit2's cached index is not publication evidence.

Sticky contracts cannot disappear/change during ordinary updates. Changed
payloads require verified jobs; existing raw tracked paths require migration.
Prior encrypted metadata must resolve to a locally retained verified preparation.
Cold-checkout approved decryption/import remains an explicit integration gate.
Index staging does not write working files, install filters, commit or push.

The initial focused run passed eight tests (one ignored subprocess helper,
explicitly invoked by crash tests), covering manual edits/deletions, unrelated
staging, foreign locks, conflicting intents, lossless paths and tombstones.
A further regression checks changed references without jobs and unknown prior
metadata. Fixtures use synthetic subprocess output for transaction mechanics;
they do not replace the earlier actual Warden encryption/restore evidence.

Strict all-target workspace Clippy, dependency policy and the locked release
build passed. The direct git2 dependency uses the existing resolved 0.21 crate;
no new dependency version was introduced. The standalone package lock was
resolved from its own manifest to include the new direct dependency and earlier
storage additions. Workspace tests and the expanded focused run are recorded
below after completion. Working-file races, filter/setup integration, outgoing
commit validation, S3, packaged restoration and daemon wiring remain unfinished.

The expanded focused transaction run passed nine tests (one ignored helper),
including the changed-reference/unknown-prior-metadata regression. It took
111.18 seconds during measured host I/O pressure; process inspection confirmed
filesystem journal waits, and the existing run completed without restarting it.
The final all-target Sync Clippy check passed. Formatting is clean for Sync;
workspace-wide formatting also reported independent edits in dracon-system,
which this work did not change.

The full workspace run completed successfully: 2002 passed, zero failed and
18 ignored, including both actual Warden storage streaming integration tests.
This run compiled the eight-test transaction snapshot; the additional ninth
regression passed in the separately completed expanded focused run. The release
build and strict Clippy checks passed for the production implementation; the
expanded all-target Sync Clippy check also covered the added regression.
Logs: `/tmp/dracon-index-workspace.log`, `/tmp/dracon-index-final-focused.log`,
`/tmp/dracon-index-clippy.log`, `/tmp/dracon-index-final-clippy.log`,
`/tmp/dracon-index-release.log`, `/tmp/dracon-index-deny.log`.
No installed binary, production storage configuration or live bucket changed.

## Networkless clean driver infrastructure (2026-10-02)

A leased exact-version clean transformation now validates protected metadata,
source/payload snapshots, repo/path binding and sticky placement/security/copy
contract before accepting input. It hashes actual bytes in bounded chunks and
emits only the canonical prepared pointer after complete verification. Existing
output remains untouched on proof failure. Exact unhydrated pointers are reused;
foreign pointers, changed/same-size bytes, truncated or oversized streams and
read failures are refused. No encryption, backend call or working-file write
occurs during cleaning.

The explicit CLI binds existing local state and requires matching protected
metadata in the actual Git index, including an alternate `GIT_INDEX_FILE`.
It refuses unknown metadata and never falls back to raw content. Metadata/blob
headers and physical index size are checked before parsing/loading large data.
The metadata store can load its already verified private decoded manifest;
this is local correspondence evidence, not a new authorization or cold restore.

Five streaming unit tests passed. Three real-Git required-filter tests passed
for the initial indexed-metadata gate, covering unchanged edits, same-size
mutation, unhydrated pointer reuse, wrong path/repo, failed jobs, missing
metadata and alternate indexes. The expanded checks additionally exercise
mismatched indexed ciphertext and the pre-decode index limit. They use synthetic
Warden output for Git mechanics; actual cryptographic evidence remains the
separate Warden streaming/restore tests. Final gate results are recorded below.

No fleet filters/configuration, installed binary or live bucket changed.
Production attribute/setup composition, binding selection/reconciliation,
working-source races for direct index plumbing, outgoing-commit validation,
S3, cold import/hydration and daemon transfers remain unfinished.

The expanded core suite passed 68 tests (nine ignored). The three indexed-metadata
Git checks passed after adding mismatched ciphertext refusal. A further real-Git
non-UTF-8 path check passed; the final four-test integration run completed in
5.31 seconds and its subsequent strict all-target Sync Clippy check passed.
The full workspace snapshot contains the earlier three-test integration file;
the separately completed run verifies the additional path case.

The locked release build, strict all-target workspace Clippy, dependency policy,
Sync formatting and diff checks passed. The built release CLI's clean-driver
help also ran successfully with the documented explicit binding arguments.
No dependency or default policy change was needed for this milestone.
Final workspace results are recorded below when its existing run completes.

The full workspace run completed successfully: 2012 passed, zero failed and
18 ignored, including both actual Warden storage streaming checks. It covers
all production changes in this milestone and the three-test Git snapshot;
the final four-test run separately proves lossless CLI/Git path handling.
Evidence logs: `/tmp/dracon-clean-workspace.log`, `/tmp/dracon-clean-core.log`,
`/tmp/dracon-clean-verified-git.log`, `/tmp/dracon-clean-path-git.log`,
`/tmp/dracon-clean-final-clippy.log`, `/tmp/dracon-clean-path-clippy.log`,
`/tmp/dracon-clean-release.log`, `/tmp/dracon-clean-deny.log`.

Next Git integration work must replace per-version driver arguments with
repository-wide approved version selection, preserve the single effective
filter/attribute contract, and add outgoing-commit and working-source race
checks. The command's explicit version binding is an infrastructure interface,
not the final automatically configured fleet workflow. All remaining roadmap
packages retain their original scope; no production release is claimed.

## Repository-wide indexed selection and guard (2026-10-02)

One clean driver now handles all enrolled paths/versions without changing its
command for each prepared artifact. Indexed ciphertext selects a verified
local manifest; repo/path/contract/security/copies/payload select and lease an
eligible local job. Historical indexed versions resolve exactly even with newer
versions retained. Record budgets, ambiguous decoded/source identities,
corruption and busy leases fail closed. No recency fallback, upload, capture or
encryption occurs during selection.

An actual historical-manifest test exposed Git's unchanged-file stat cache:
ordinary add can omit the clean callback and leave a mismatched pointer group.
The independent `storage verify-index` primitive/CLI now checks actual reference
blobs and tombstones, plus staged attributes and the local required driver.
Raw/missing/mismatched entries, removed routes and unenrolled driver paths fail.
The guard uses the real/alternate index, suppresses hooks/fsmonitor and ambient
Git repo/config overrides, and does not change index or working files.

The focused core run passed 70 tests (nine ignored), including ambiguity and
catalog-limit refusal. Eight real-Git tests passed for generic multi-path and
historical selection, lease contention, byte/path fidelity, alternate indexes,
stat-cache-independent validation and staged required-driver attributes.
Synthetic metadata fixtures test mechanics; the real Warden integration checks
remain the separate crypto evidence. Full workspace/release/lint results follow.

The former per-version CLI arguments were unreleased infrastructure and are
replaced in current docs. Production hook chaining/setup, Warden composition,
working-source races and daemon commit/push integration are still unfinished.
S3, cold import/hydration, independent-copy drills and release/pilots retain the
full roadmap scope. No fleet filters, live buckets or installed binary changed.

The full workspace run completed successfully: 2019 passed, zero failed and
18 ignored. It includes all eight real-Git driver/guard checks and both actual
Warden storage streaming integration tests. Strict all-target workspace Clippy,
the locked release build, dependency policy, Sync formatting and diff checks
passed. Both updated release CLI help pages ran with the documented stable
repository arguments. No installed binary or production enrollment changed.
Evidence: `/tmp/dracon-selected-clean-workspace.log`,
`/tmp/dracon-selected-clean-core.log`, `/tmp/dracon-selected-clean-guard.log`,
`/tmp/dracon-selected-clean-clippy.log`, `/tmp/dracon-selected-clean-release.log`,
`/tmp/dracon-selected-clean-deny.log`.

## Direct commit protection and Warden hook binding (2026-10-02)

The source daemon now uses a direct storage guard for configured repositories,
including bootstrap commits: libgit2 bypasses Git pre-commit hooks, so a hook
alone cannot protect that path. The guard holds a process-backed owned index
lock, validates the canonical index/manifest/staged attributes, writes the
validated immutable tree to the explicit repository and commits that tree.
A real test caught and fixed the initially incorrect repository-less write-tree
call. Unconfigured repositories retain their ordinary commit path; configured
storage failures have no unguarded fallback.

`storage setup-guard` verifies the staged pair before publishing version-1 local
bindings and pins the invoked executable. Identical setup is idempotent; changed
bindings/unknown versions refuse. `verify-configured-index` honors alternate
indexes for manual Git use. No filter/hook installation, source rewrite, transfer
or production enrollment occurs during binding setup.

Warden's source pre-commit template runs the pinned guard after foreign/user
hook chains and retains its encryption gate. Missing, empty or unknown guard
versions and unavailable/non-absolute executables fail closed when configured.
A UUID by itself does not enroll an ordinary repository. Shell subprocess tests
prove quoted executable paths, failure propagation and retained encryption checks.

Focused evidence: 12 index tests passed (two subprocess helpers ignored),
including owned-lock recovery after process death and preserving foreign locks.
The native commit test passed for a valid root tree, rejected raw bytes and
preserved HEAD/index/worktree on rejection. Bootstrap passed valid and invalid
cases, proving the legacy --no-verify path cannot bypass the configured guard.
The real CLI binding test passed for incomplete setup refusal, idempotence and
alternate-index rejection. The actual source Warden installer + Sync executable
+ real Git commit test passed: user-hook chaining survives, a verified pair
commits and a raw staged payload blocks. Git itself refreshes its TREE cache
before pre-commit; staged content/modes/paths and worktree bytes remain unchanged
on rejection. Synthetic metadata fixtures prove Git mechanics, not cryptography.

Evidence logs: `/tmp/dracon-guard-lock-tests.log`,
`/tmp/dracon-native-guard-tests.log`, `/tmp/dracon-storage-bootstrap.log`,
`/tmp/dracon-guard-binding-git.log`, `/tmp/dracon-cross-utility-guard.log`.
The cross-utility test is explicitly invoked with DRACON_STORAGE_TEST_WARDEN;
it is not silently counted as ordinary workspace coverage. Final workspace,
release, lint and policy gate results are recorded after the running checks end.

No live buckets, installed binaries, fleet drivers or hooks changed. Production
attribute/filter composition, enrollment/worker reconciliation, working-source
race protection, outgoing-history/pre-push coverage, S3 and packaged cold
restoration remain unfinished. In particular, this is not complete outgoing
protection against manual --no-verify commits or removal of every local guard
marker. All remaining roadmap packages retain their full scope; no release or
production readiness is claimed.

The locked workspace run completed: **2027 passed, zero failed, 19 ignored**,
including all nine ordinary real-Git driver/binding tests, native/bootstrap
commit cases, both Warden hook cases and both actual storage crypto streaming
checks. Its integration-file snapshot predates the extra ignored cross-utility
test; that test passed separately with both built debug and release Warden
installers. The final empty-marker hook case also passed in the subsequent
focused Warden run. These supplementary results are explicit evidence rather
than additions to the workspace pass total.

Final strict all-target workspace Clippy, locked release build, dependency policy,
Sync/Warden formatting and diff checks passed. Both release guard CLI help pages
ran successfully. Logs: `/tmp/dracon-commit-guard-workspace.log`,
`/tmp/dracon-storage-final-hook-tests.log`,
`/tmp/dracon-cross-utility-release-guard.log`,
`/tmp/dracon-commit-guard-final-clippy.log`,
`/tmp/dracon-commit-guard-final-release.log`,
`/tmp/dracon-commit-guard-deny.log`. This completes verification of this source
milestone; the larger storage roadmap remains active and unreleased.

## Bounded attribute-query lifecycle (2026-10-02)

The storage inventory/index guard no longer collects unlimited stdout or waits
indefinitely for Git while holding a commit lock. Queries have a 100,000-path
and 16 MiB input budget, 32 MiB output budget, 1,024-byte individual filter-value
budget and shared 30-second I/O/process deadline. A dedicated runtime thread
supports both synchronous CLI callers inside Tokio and daemon blocking workers;
write/read/wait proceed concurrently under the same deadline. Error/timeout
cleanup terminates the query's owned Unix process group, including descendants
holding pipes after the original process exits, with bounded parent reaping.

Response parsing consumes the exact expected record count without allocating a
vector for every NUL field. Missing terminators, unexpected attributes,
unknown/duplicate paths, oversized values and extra records fail closed. Budget
exhaustion is a visible failure; it never verifies only a subset or changes
staging to raw content. Existing required-filter/index/working-file contracts
remain in force. No network or fleet configuration changes were made.

Five focused attribute tests passed, including the prior literal-path batch
case, 12,000 real-Git paths crossing pipe capacity, input/path refusal, malformed
responses, oversized output, blocked input and a descendant retaining stdout
after parent exit. The Linux descendant test verifies process death rather than
assuming SIGKILL delivery is synchronous. Evidence:
`/tmp/dracon-bounded-attrs-tests.log`. Full workspace/release/lint/policy results
follow after the running checks finish. The larger roadmap remains active;
this resource qualification does not complete S3, enrollment, hydration,
working-source races or outgoing-history/push protection.

The first full workspace attempt exposed a pre-existing timing dependence in
the historical-manifest integration fixture. With stderr diagnostics added,
the failure reproduced on attempt seven: Git refreshed an unrelated `second.bin`
index entry and correctly refused it because the restored older manifest did
not enroll that later-added path. The fixture had relied on the stat cache
skipping it. The historical index now removes that later-added entry before
restoring its earlier manifest, preserves the working bytes, and explicitly
proves a subsequent attempt to add the unenrolled sibling fails. Production
clean/guard checks were not weakened. That first failed run is retained at
`/tmp/dracon-bounded-attrs-workspace.log`; diagnostic evidence is at
`/tmp/dracon-historical-clean-repeat.log`. The fixed targeted case passed;
repeat qualification and a fresh full workspace run follow.

The corrected historical case passed ten consecutive targeted executions
(`/tmp/dracon-historical-clean-fixed-repeat.log`) and subsequently passed within
the fresh workspace run. Final strict all-target Clippy passed after the fixture
change (`/tmp/dracon-bounded-attrs-final-clippy.log`). The production release
build and dependency policy had already passed before that test-only correction
(`/tmp/dracon-bounded-attrs-release.log`, `/tmp/dracon-bounded-attrs-deny.log`);
no production code/dependency changed afterward. The actual source Warden hook
integration also passed with the new bounded query path
(`/tmp/dracon-bounded-attrs-cross-hook.log`). Final complete workspace totals
are recorded only when its remaining Warden integration/streaming checks end.

The fresh locked workspace run completed successfully: **2031 passed, zero
failed, 20 ignored**, including all new query checks, all nine ordinary real-Git
checks, the corrected historical-version fixture, Warden integration and both
actual crypto streaming tests. Evidence:
`/tmp/dracon-bounded-attrs-final-workspace.log`. The explicitly invoked
cross-utility hook check is recorded separately rather than counted among the
ignored workspace tests. Formatting/diff checks passed. These results validate
this bounded-query milestone; no production release/enrollment or larger-roadmap
completion is claimed.

## Cold protected-metadata import (2026-10-02)

`MetadataStore::import` and the unreleased `storage import-manifest` command now
rebuild private metadata correspondence from exact committed age ciphertext
without the original machine's preparation cache. The command pins a selected
commit, requires explicit local repository-ID/policy/Warden bindings and ignores
unstaged manifest changes. It does not install a filter/hook, activate a guard,
hydrate an asset, grant backend permissions, stage or push.

Ciphertext is length/digest/header checked in a bounded anonymous private spool,
decrypted with existing authorized keys into separate unpublished output, then
canonical-codec/repo checked before retaining source/ciphertext proof. One import
lease bounds transient files per namespace; existing retained/catalog budgets
apply. Corrupted sources/payloads and ambiguous caches fail without overwriting
previous bytes. Imported private specifications use version 2 with ciphertext in
their identity; version-1 encodings/IDs and the committed manifest codec remain
unchanged. Independently encrypted versions of the same decoded metadata coexist.

Crash points before/after import approval and after ciphertext publication
recover the exact retained version. Warden's adapter now creates an owned Unix
process group: timeout/failure/cancellation terminates descendants holding pipes
and releases the import lease. No new key-generation/network facility was added.
The already locked tempfile crate was promoted from test-only to runtime use for
anonymous private spools; no new crate version was selected.

Fifteen focused metadata tests passed (three operational/subprocess helpers
ignored), including cache limits/corruption, unchanged preparations, wrong-repo
and noncanonical/oversize/bad-exit refusal, process-death recovery, descendant
termination and cancellation/retry. The CLI cold-clone test passed; its final
workspace case additionally deletes the original test-owned cache and verifies
committed bytes are used while working manifest edits/index/pointer contents and
filter/guard settings remain unchanged. These synthetic fixtures prove mechanics.

The separately invoked actual Warden/age-key check passed after deleting the
original test store and moving the checkout. It imported both independently
randomized ciphertext versions, verified exact metadata/cache correspondence,
and refused corrupted age data. This is metadata/key recovery evidence, not an
asset hydration or independent-provider durability certificate. Logs:
`/tmp/dracon-cold-import-final-core.log`, `/tmp/dracon-cold-import-git.log`,
`/tmp/dracon-cold-import-final-real-crypto.log`.

Strict all-target Clippy, locked release build, dependency policy and formatting
passed; the release import-manifest help page ran. Full workspace totals follow
when its existing run finishes. No installed binary, live bucket, fleet filter,
operator key or production enrollment was changed. Asset hydration, provider
adapters, automatic enrollment/worker scheduling, Warden routing composition and
complete outgoing-history/push coverage retain the full original roadmap scope.

The full locked workspace run completed: **2038 passed, zero failed, 21 ignored**,
including the cold-clone CLI case with the original cache removed, all new
import/crash/cancellation cases and both actual large-payload crypto integration
checks. The real-key metadata import case is separately invoked and is not added
to that pass count. Logs: `/tmp/dracon-cold-import-workspace.log`,
`/tmp/dracon-cold-import-clippy.log`, `/tmp/dracon-cold-import-release.log`,
`/tmp/dracon-cold-import-deny.log`. Formatting/diff checks passed. This validates
the cold-metadata import milestone; packaged asset hydration and the wider
production storage feature remain unfinished and unreleased.

## Exact private asset recovery (2026-10-02)

`RestoreStore` and unreleased `storage restore-asset` recover an exact manifest
version into a dedicated private namespace. The CLI pins one committed manifest
and pointer, requires their correspondence and explicit local repository ID,
then selects a required copy from operator backend bindings. Repository overrides
cannot inject adapters. Local adapters are supported; S3 is still pending.
Encrypted assets require a matching Warden adapter and authorized existing keys;
non-sensitive assets require an explicit class grant without asset decryption.

Fetched ciphertext receives an independent bounded length/digest check even if a
backend incorrectly reports success. Decryption writes only anonymous private
output; failed authentication never publishes it. Successful output is retained
as a verified mode-0600 file under an opaque enrollment/version-derived name.
One namespace lease bounds concurrent spools. Payload, output, retained bytes and
version limits refuse overflow without evicting previous versions. Tombstones
cannot recover a current payload; an older selected manifest remains usable.
The shared snapshot primitive now publishes by create-only hard linking rather
than replacement rename. A race test verifies concurrently created operator
content survives and the verified capture is preserved on conflict.

Focused storage-core results: **83 passed, zero failed, 12 ignored** in
`/tmp/dracon-restore-core.log`, covering snapshot/index/import crash recovery and
five recovery tests, including version and retention limits. The cold-clone CLI
case passed with both the original metadata cache and original job journal
removed. It recovered bytes from the declared secondary copy while preserving
working manifest edits, pointer, index and filter/guard configuration. Evidence:
`/tmp/dracon-restore-cli-git.log`.

A separately invoked actual Warden/age test passed for **101 MiB** of encrypted
asset data. It deleted the original test-owned plaintext, ciphertext spool and
metadata cache, moved to a cold checkout, imported committed metadata with the
retained authorized keys and recovered byte-exact output from the reopened local
object store. Evidence: `/tmp/dracon-restore-real-crypto.log`. This proves private
exact-version recovery with keys, not independent-provider durability or safe
working-file hydration. No operator keys or live data were deleted or changed.

The full locked workspace passed: **2043 passed, zero failed, 22 ignored**.
Strict all-target Clippy, locked release build, dependency policy, formatting and
diff checks passed; the source-built release recovery help page ran. Evidence:
`/tmp/dracon-restore-workspace.log`, `/tmp/dracon-restore-clippy.log`,
`/tmp/dracon-restore-release.log`, `/tmp/dracon-restore-deny.log`.
The separately invoked real-key recovery test is not added to workspace totals.
Working-file hydration, S3, automatic worker/routing,
complete outgoing-history guards, reviewed pilots and production release retain
the full original scope. No installed binary or live enrollment changed.

## Portable guard requirement on cold clones (2026-10-02)

The native commit guard now detects `filter=dracon-storage` declarations in
staged and HEAD attribute blobs, independent of local driver settings. Cold
clones cannot silently take the ordinary commit path. Staging attribute removal
cannot erase HEAD's requirement. Nested attributes, macro definitions, quoted
and non-UTF-8 patterns and declarations without currently matching files are
covered. Comments and quoted filenames containing the assignment do not activate
storage. Driver configuration is also inspected through the effective Git
configuration so included/inherited settings cannot bypass a local binding.

Detection reads bounded ordinary attribute blobs and an owned ordinary index
without running filters. Blob/type/size errors, symlinked indexes and a HEAD
traversal timeout refuse commits. Existing configured-index/tree validation and
its commit lease remain authoritative after a binding is present. This closes
the missing-local-settings recognition gap; it does not certify backend copies
or supply complete outgoing-history/push protection.

Focused results: **28 passed, zero failed** in
`/tmp/dracon-portable-guard-tests.log`. Six additional tests cover a real cold
clone with all local settings absent, preservation after staged attribute/pointer
removal, a daemon bootstrap with hydrated bytes and no local driver, declarations
and benign comments, unreadable/oversized/symlinked inputs and included driver
settings. Refused commits preserve the HEAD/index/worktree where applicable and
do not leave an index lock. The final locked workspace passed:
**2049 passed, zero failed, 22 ignored**. Strict all-target Clippy, locked release
build, dependency policy, formatting and diff checks passed. Evidence:
`/tmp/dracon-portable-guard-workspace.log`,
`/tmp/dracon-portable-guard-clippy.log`,
`/tmp/dracon-portable-guard-release.log`, `/tmp/dracon-portable-guard-deny.log`.
The separately invoked actual source Warden hook test also passed and preserved
the existing user hook: `/tmp/dracon-portable-guard-warden-hook.log`. It is not
added to workspace totals.

No installed daemon, operator keys, live hooks or enrollment changed. Safe
working-file hydration, S3 adapters, automatic routing/worker scheduling,
complete outgoing-history coverage, pilots and release remain unfinished.

## Explicit Linux checkout hydration (2026-10-02)

`storage hydrate` now consumes exact verified recovery and publishes it to the
checked-out version's working path. The configured guard must match the supplied
repository ID, protected-manifest path and private metadata root. Inside the
owned Git index lease, hydration repeats placement verification and checks the
selected HEAD, exact staged pointer and protected metadata digest. Recovery
receipts now privately carry source/payload/manifest correspondence; their fields
cannot be constructed by external callers and source hashes are not printed.

A private intent binds version, path, fingerprints, Git mode and pinned directory
identities. Output is copied and reverified independently of the recovery cache;
working edits cannot mutate it through shared inodes. Linux directory-descriptor
operations refuse symlink traversal and use `renameat2(RENAME_NOREPLACE)` for
pointer capture/publication. Original pointers are retained privately. Capture
conflicts restore the original only into a still-missing working path; a new
operator file wins. Changed roots/parents, hard links, local edits, inconsistent
metadata, corrupt cache and exceeded version/byte limits refuse publication.
Output uses mode 0600, or 0700 for an executable Git entry. Index bytes and Git
history are unchanged.

Publication has a brief missing-path interval after pointer capture. Durable
intent/output/original files survive process death, and leases recover owned
stale Git locks. Explicit `--resume-local` selects only a matching private
transaction, verifies its identity and retained bytes, then resumes without
fetching/decrypting. It neither selects the newest unrelated record nor silently
falls back on provider failure. Corrupt or unprepared state fails while retaining
all files; even early intent-validation errors release recovered owned locks.

Nine focused hydration tests passed (one subprocess helper ignored), covering
publication, missing files, executable mode, reference/commit/placement binding,
version/retention limits, cache integrity, concurrent edits/creation/parent
replacement, four process-death phases and local resume without the original
recovery cache. Evidence: `/tmp/dracon-hydration-tests.log`. The final CLI suite
passed **11 tests, zero failed, one ignored** in
`/tmp/dracon-hydration-final-git.log`. The hydration case verifies required-clean
Git diff equivalence and unchanged index, edit refusal, historical-version
refusal, and explicit local resume with the backend offline and recovery cache
removed. These filesystem/CLI fixtures use synthetic protected metadata.

The actual-key operational test separately passed for **101 MiB** after deleting
the original source/ciphertext spool/metadata store: cold metadata import and
authenticated asset recovery were followed by checkout publication, byte-exact
verification, retained original pointer and unchanged index. The callback checked
manifest entries; this proves crypto/filesystem correspondence, not a production
filter deployment or independent-provider certificate. Evidence:
`/tmp/dracon-hydration-real-crypto.log`.

The final locked workspace passed **2059 tests, zero failed, 23 ignored**.
Shared recovery arguments were boxed to satisfy strict Clippy's enum-size lint
without changing flags; final all-target Clippy, release build and CLI regressions
passed. Dependency policy and formatting/diff checks
passed. Logs: `/tmp/dracon-hydration-workspace.log`,
`/tmp/dracon-hydration-clippy.log`, `/tmp/dracon-hydration-release.log`,
`/tmp/dracon-hydration-deny.log`. The source-built release help page ran. Final workspace evidence is recorded in
`/tmp/dracon-hydration-final-workspace.log`.

No installed daemon/hooks, fleet enrollment, operator keys or live buckets were
changed. Hydration is explicit and Linux-only; supported filesystems must provide
create-only rename and `/proc` access. Automatic startup reconciliation, S3,
worker/routing/enrollment, public metadata without Warden, complete outgoing
history/push coverage, growth benchmarks, reviewed pilots and release retain the
full original roadmap scope.

## S3 create-only protocol driver (2026-10-02)

`storage_core::s3` now implements `ImmutableBackend` over a minimal conditional
PUT/full GET transport boundary. Capture uses an anonymous private bounded spool
and a stable fingerprint. Both creation and existing-object responses require
independent complete readback before success. The driver exposes no unconditional
write, delete, listing or bucket mutation. Source failure/byte overflow prevents
provider contact; conflicting writes, failed readback and corrupt existing bytes
produce no successful receipt. Retrieval bounds actual bytes to the expected
identity, and callers still publish output only after successful verification.

Seven focused protocol tests passed, including a 101 MiB private-file streaming
round trip with 64 KiB capture requests, existing corrupt object preservation,
conflicts, source/provider/destination failures and size bounds. Evidence:
`/tmp/dracon-s3-protocol-tests.log`. Final strict workspace/all-target Clippy,
release build, dependency policy and formatting/diff checks passed. Logs:
`/tmp/dracon-s3-protocol-clippy.log`, `/tmp/dracon-s3-protocol-release.log`,
`/tmp/dracon-s3-protocol-deny.log`. The release build started before documentation
for three public items was added; the final strict Clippy verifies those additions.

This is a protocol driver tested with synthetic transports, not a signed HTTP
adapter or provider certification. The S3 planning binding still cannot perform
network recovery. Operator credential resolution, signed conditional HTTP,
endpoint capability checks, durable worker/routing/enrollment, independent cold
provider recovery and the original remaining release gates remain required. No
installed daemon, live bucket, keys or fleet configuration changed. See
[protocol requirements](storage-s3-protocol-2026-10-02.md).

The completed locked workspace run passed **2066 tests, zero failed,
23 ignored** across 35 suites. Evidence: `/tmp/dracon-s3-protocol-workspace.log`
(terminal exit 0), including the existing actual Warden large-stream test.

## Signed HTTPS and explicit S3 recovery (2026-10-02)

The S3 adapter now performs payload-signed, create-only HTTP PUT and complete GET
requests with explicit region/prefix/operator credentials. HTTPS origins are
validated; redirects, inherited proxies and decompression are disabled. Signed
headers include the overwrite condition, ciphertext hash, content length and
session token. Optional credential expiry is checked per request. Request/body
deadlines prevent trickled responses from extending a transfer indefinitely.
Provider bodies and arbitrary URL-bearing HTTP errors are never surfaced;
capacity/security/integrity/transient failures remain redacted and classifiable.

The read-only named credential resolver traverses directories through pinned
file descriptors, refuses links, non-private/unowned files, hard links, oversized
records, invalid/unknown JSON and expired credentials. Tracked or unignored
credential files in a checkout are refused without modifying index/ignores.
Credentials and signing keys use zeroizing owned buffers, with no credential
Debug/Serialize implementation. The caller explicitly supplies a private operator
directory; there is no environment/search fallback or key mutation.

Global S3 bindings add an optional signing region and prefix while old planning
bindings still deserialize. Runtime requires the region and operator credential
root. `restore-asset`/`hydrate` now select a required globally approved S3 copy and
execute the synchronous network adapter on a blocking worker. Protected manifest
selection, security grants, authenticated Warden decryption, private retention
and index-preserving publication remain the existing recovery boundaries.
`--resume-local` still does no provider/credential lookup.

Twenty focused tests passed: seven protocol, nine HTTP and four credential tests.
Two published AWS GET/PUT signing vectors match; wire tests verify actual signed
headers/body/path/readback, conditional conflicts, redaction, session credentials,
length/encoding/range refusal, redirect refusal and total body deadlines.
Evidence: `/tmp/dracon-s3-recovery-core-tests.log`.

The operational CLI suite passed **13 tests, zero failed/ignored**, including the
existing real Warden hook test and a new cold-clone signed HTTPS recovery. The
latter uses a temporary CA, published test credentials in a private JSON vault,
a local TLS provider and synthetic protected metadata. Original job/metadata
state is absent; recovered bytes and unchanged index/working pointer are checked.
It proves actual CLI binding/TLS/signature/fetch correspondence, not an actual-key
encrypted S3 or independent-provider durability certificate. Evidence:
`/tmp/dracon-s3-recovery-operational-cli.log` and `/tmp/dracon-s3-https-cli.log`.

Final strict all-target Clippy, release build, dependency policy and formatting/
diff checks passed. Logs: `/tmp/dracon-s3-recovery-clippy.log`,
`/tmp/dracon-s3-recovery-release.log`, `/tmp/dracon-s3-recovery-deny.log`.
Parent and standalone lockfiles resolve the new direct HMAC/clock/zeroize
requirements; standalone resolution was performed offline without updating
unrelated dependency versions. Chrono and zeroize declare Rust 1.62 and 1.85 respectively; the pinned HMAC
version was already in the workspace lock. No new full Rust 1.89 build is claimed.

No installed daemon/hooks, live provider, operator keys or fleet configuration
changed. Automatic capture/upload/routing/enrollment, live endpoint capability
approval, actual-key encrypted independent-provider cold recovery, complete
outgoing-history coverage, growth benchmarks, reviewed pilots and release remain
open within the original roadmap.

The completed final locked workspace run passed **2079 tests, zero failed,
24 ignored** across 35 suites (terminal exit 0). Evidence:
`/tmp/dracon-s3-recovery-workspace.log`; the two storage CLI operational cases
were separately executed successfully in the 13-test suite above.


## Capability checks and explicit durable job worker (2026-10-02)

Source builds now provide `storage probe-backend` and `storage advance-job`.
The probe retains a random 64-byte control under the reserved prefix, checks
competing creates (exactly one 200 and one 412), different-byte write refusal,
and complete readback before/after. The approved transport is confined to that
exact configuration, not externally constructible/rebindable, and expires after
one hour. No report or persisted receipt can replace a fresh check for explicit
S3 job advancement. Both-success, both-refused, ignored condition, false refusal,
partial/corrupt reads and stale/future approvals are tested.

The worker consumes an already captured durable job. It validates every required
global copy binding/security class, uses Warden SDK preparation or an explicitly
authorized non-sensitive representation, then verifies every required copy and
stops at ReadyToStage. Retained inputs are checked before capability requests;
selected payload length/SHA-256 is verified at EOF before a backend's spool
publishes/sends asset bytes. Same-length mutation, truncation, growth and a backend
claiming success without consuming its input cannot produce successful copies.
No working-file read, Git staging, commit, push, pruning or history change occurs.

Three core worker tests cover exact capture/preparation/retry, missing Warden or
forbidden class, and synthetic SDK composition/fail-closed plaintext output.
Synthetic outputs check composition only; actual cryptography remains covered
by the existing explicit operational Warden tests. A new local CLI case verifies
that both required stores receive the captured version despite newer working
edits, an existing foreign index lock survives, retries retain identity, and a
missing required binding refuses before opening an invalid first backend.

The TLS cold-clone fixture now also executes signed conditional asset PUT and
readbacks through `advance-job`, exercises new-object creation and existing-object
retry, and refuses an endpoint ignoring the overwrite condition. Five requests
per capability check retain a single 64-byte control; no delete/list/bucket-create
API is exposed. Fixture-only deletion forces the asset-creation branch and never
touches a live object. This test uses published fixture credentials, a temporary
CA and explicit non-sensitive content: it is not an actual-key encrypted S3 or
independent-provider durability certificate.

Focused evidence:
- `/tmp/dracon-s3-capability-tests.log`: 13 HTTP/signing/capability tests passed.
- `/tmp/dracon-s3-transfer-validation-tests.log`: 9 exact-input/copy tests passed.
- `/tmp/dracon-storage-advance-worker.log`: 3 core worker tests passed.
- `/tmp/dracon-storage-advance-operational.log`: all 14 Git CLI cases passed with
  ignored operational cases enabled and source-built Warden selected.

No installed daemon, bucket, live credential vault, operator policy or hook was
changed. The preview remains unreleased. Automatic capture/routing/enrollment,
startup reconciliation, all-version outgoing guards, resource qualification,
actual encrypted independent-provider drills, pilots/package/release and the
separate sanctioned legacy-maintenance proposal remain open. The roadmap scope
has not been reduced to the explicit job milestone.

Final locked workspace run completed successfully: **2,091 passed,
0 failed, 24 ignored across 35 suites**
(`/tmp/dracon-storage-advance-workspace.log`). The 14 operational Git CLI cases
were separately run with ignored cases enabled. Locked release build, strict
workspace/all-target Clippy, dependency deny, formatting and diff checks passed
(`/tmp/dracon-storage-advance-release.log`,
`/tmp/dracon-storage-advance-clippy.log`,
`/tmp/dracon-storage-advance-deny.log`). The release artifact's `storage advance-job
--help` also exposes the documented command; no installed binary was replaced.
