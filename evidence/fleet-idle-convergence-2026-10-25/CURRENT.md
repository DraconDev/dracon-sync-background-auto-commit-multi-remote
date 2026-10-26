# Active systematic fleet-idle repair — checkpoint

## Hygiene v12 — CURRENT; coherent four-gate intermediate assembly, NOT deployed

Artifact cleanup and large-untracked ignore handling were genuinely disabled by
the staged-epoch refactor and are now restored through the private-candidate
model. See checks/epoch-integration-partial/hygiene-integration.md for the design,
the exact ordering constraint (index-side de-index in the candidate, canonical
ignore rules only AFTER the commit lands), and the retained failed/flaked runs.

CURRENT coherent gates, zero source drift, unchanged pinned helper:
- cargo test --workspace --locked: PASS; sync1484pass, system616, warden233,
  integration144 (security/stream suites in the raw log).
- cargo build --release --locked, strict all-target Clippy and cargo deny: PASS.
Proof: hygiene-order-v10-source-before.json and hygiene-order-v12-source-after.json.
One crash-lease control failed once under full-suite load and passed immediately
in isolation and in the re-run; that run is retained, not deleted or weakened.

This is still NOT final acceptance, deployment, loaded execution or fleet-idle
proof. No install/config migration, real-repo manual commit/push or loaded
execution was performed. Remaining prerequisites are unchanged: queued-open/
lease-break and exact-exchange qualification, unsupported-FS controls, actual
daemon restart/admission rehearsal, original product gates, independent review,
installed/loaded receipt, the authorized zero-batch migration, and a fresh
complete watched-fleet clean-idle observation.

## Unmerged-entry v6 — HISTORICAL four-gate intermediate assembly

The active fleet-convergence objective/contract in .pi-glla remains authoritative.
The pending task5 label does not waive the still-open implementation, original
product-gate, deployment, installed-execution and full-fleet prerequisites.

Restored policy-enabled stale-unmerged handling BEFORE ordinary entry capture
could reject that index. The production path no longer runs a late live-index
reset. It verifies the raw index, native identity/stage/flags observation,
explicit branch/HEAD and held no-follow regular-file descriptors. Only a complete
HEAD-equal selection is eligible. Active merge/rebase/cherry-pick, uncertain
bytes, unsupported flags/paths, foreign locks/index/ref changes and deadlines
preserve actual content rather than granting provenance to a later reread.
The returned epoch is derived from the private candidate, not the actual index.
Opt-out, disabled auto-commit and dry-run cannot enable this reconciliation.

Before conditional actual-index installation, an owned0700 recovery directory
contains the exact original index (including gitlink identity metadata) and a
fsynced independent pack of parent-repository conflict/changed cached blobs. Unchanged stage0 HEAD baseline blobs
are NOT duplicated. Metadata/object checks bound the raw index at64MiB,
retention input at16MiB and retained payloads at512MiB. Free-space preflight is
best-effort, NOT a global reservation against concurrent disk use. Recovery and
its parent directory are fsynced before replacement; the index parent is fsynced
afterward. This is ordering evidence, NOT an actual power-cut qualification.
Unknown worktree/flag refusals allocate no spool; repeated no-progress controls
preserve exact index bytes without growing recovery directories.

Executed v6 controls include real sync_repo accepting the verified staged B
without absorbing later C/untracked work, meaningful pre-commit executing once,
stale-slot-only reconciliation making NO empty/safety commit, cold recovery of
all three conflict versions into an unrelated empty object database, exact raw
names including non-UTF8/newlines, a live producer FD writing AFTER reconciliation
to the SAME canonical inode, and real index/ref race injection and held-FD hash
cancellation with actual descendant termination. Synthetic tests run only in
owned disposable repositories; they are NOT installed or loaded-fleet evidence.
Current positive qualification is Linux; non-Unix descriptor reconciliation
refuses conservatively, and no NFS/stalled-filesystem timing claim is made.

CURRENT complete intermediate gates, zero source drift, unchanged pinned helper:
- cargo test --workspace --locked: PASS; sync1475pass/0fail/3ignored,
  system616pass, warden233pass, plus security/integration suites in the raw log.
- cargo build --release --locked: PASS.
- cargo clippy --workspace --locked --all-targets -- -D warnings: PASS.
- cargo deny check: PASS, no dependency/security guard weakening.
- Focused unmerged controls9pass/0fail; original unmerged controls also remain
  in the passing full workspace suite. Counts are observations, not new pins.
Proof: checks/epoch-integration-partial/unmerged-entry-v6-source-before.json and
unmerged-entry-v6-source-after.json identify source/helper/release artifacts,
per-command exits and log hashes. v1 failed original controls and v2 Clippy
failure remain archived; no assertions were weakened. v4's complete four-gate
assembly and v5's targeted assembly are HISTORICAL, not mixed with v6 evidence.

No installation/service/config migration, real-repo manual commit/push or fresh
loaded-execution/full-fleet observation was performed by this continuation.
The artifact-cleanup and large-untracked hygiene gaps recorded in that header are
now addressed by the hygiene v12 assembly above; its own remaining prerequisites
still forbid deployment. CORRECTION: version/release
bumping was ALREADY a no-op in known-green5cf3391 and is blamed to June16, not
introduced by epoch integration. Do not implement unrelated new release behavior
based on the earlier inaccurate TODO. Evidence: checks/epoch-integration-partial/
preexisting-version-noop-assessment.md. No committed scope is removed. Finish the
queued-open/lease-break/exact-exchange and unsupported-FS controls, actual daemon
restart/admission rehearsal, original product gates and independent review.
Then re-run all four gates on the coherent FINAL assembly, qualify installed and
loaded execution, safely apply the authorized zero-batch migration, and prove
actual daemon publication plus EVERY freshly enumerated watched repo clean/idle.
The earlier37-repository count is a BASELINE observation, never an inventory cap.

## Archive-safety v3f — HISTORICAL targeted assembly, NOT final/deployed

The active fleet-convergence objective/contract in .pi-glla remains authoritative;
ignore the stale bucket-strategy handoff. Task5 is NOT ready merely because it is
listed as next pending: tasks2/3/4 retain implementation/deployment prerequisites.
The recovered worker artifact b2a57e80 explicitly reported partial integration,
not acceptance; current code/evidence supersedes its old compile-only checkpoint.

Closed a real safety gap in the previous header's assumption: moving recovery
into an archive does NOT prove its inode will never receive late writer bytes.
Archived requests/targets now have immutable metadata observation seals including
ctime and link count. Concern probes stat them without rehashing history; changes,
missing/invalid seals or failed inspection never imply raw-clean idle. Same-size
late writes with restored mtime are detected. Source fingerprint capture also
binds hash/ctime/path identity across the read. Unknown sealed bytes are never
resealed. Known missing-seal legacy records can be classified read-only once;
existing identical seals survive retry after a crash-before-move. Raw canonical
and actual-index bytes are never changed by these classification/concern probes.
History scanning is O(retained targets), checked at2seconds with bounded seal reads;
no stalled-filesystem, NFS or cross-platform timing/qualification claim is made.

Initial v3 guard tests found tempfile creation used permissive default directory
modes, contrary to private-recovery intent. Actual recovery and private-staging
directories now request0700 AT CREATION, before materialization, not after exposure.
Worker-only legacy directory privacy repair requires an owned0700 archive root
and owned real children opened by pinned descriptor + O_NOFOLLOW, changes only
group/other directory access, and never touches payloads/foreign symlink targets.
The read-only report never performs this repair. Symlink/canary negative passes.

Executed parent-crash control: observe matching helper PID + actual source inode
in ACTIVE READ /proc/locks, verify real PPID, SIGKILL only its owned disposable
parent, observe helper death/zombie in finite time, then reopen/write canonical
inode nonblocking and preserve later bytes. This is separate from and in addition
to the earlier actual-lease cancellation proof. It does NOT establish the exact
instruction position of exchange; both sides must retain classifiable originals.
The archive FD control actually keeps a producer open across archival, writes
AFTER the move, restores mtime at same length, requires persistent concern and
real sync_repo Blocked, retains index/seal/canonical bytes and leaves producer on.

A final admission inspection found probe ERR could prevent its worker from fixing
legacy permissions. Current sync_repo admits diagnostics on errors and preserves
Blocked on unresolved settlement while draining already committed backlog. The
legacy positive now uses real sync_repo rather than just private settlement.
Its first extended fixture failed byte equality due to legitimate SDK stat-cache
refresh after the fixture's inode swaps (not changed cached object identities).
The fixture explicitly verifies cached HEAD equality and refreshes THAT synthetic
index before recording its byte-exact baseline; the equality assertion remains.
Failed v3 and v3e logs remain intact; no assertion/guard was weakened.

CURRENT v3f observed gates, zero source drift and unchanged pinned DEBUG helper:
- recovery10pass/0fail (includes actual-parent-crash, cancellation, late writer,
  known/unknown legacy and retry, symlink/privacy and worker-admission controls).
- restore9pass, original exclusion2pass, filter-only3pass, real JSON2pass, all0fail.
- cargo clippy --workspace --locked --all-targets -- -D warnings: PASS.
- current source/helper identities + actual per-command exits/log hashes:
  checks/epoch-integration-partial/excluded-archive-v3f-source-before.json and
  excluded-archive-v3f-source-after.json; raw logs/exit files use v3f-*.
The new JSON archive seal is a declared synthetic CONSUMER fixture, NOT provenance
that a helper produced it. Real settlement/FD and kernel-helper controls are
separate. Do NOT infer installation/loaded behavior from any DEBUG fixture.

PRECEDING v3d complete four-gate assembly: workspace1466 sync pass/0fail/3ignored,
616 system pass,233 warden pass plus security/integration suites; release build,
strict Clippy and cargo deny all PASS; zero source drift and unchanged helper.
Its source-before/source-after JSONs and four raw logs use excluded-archive-v3d-*.
Those four-gate results are HISTORICAL, not current v3f qualification: caller and
fixture changed afterward. Current f full-workspace/release/deny are NOT claimed.
Do not deploy target/release's older d assembly or mix these assembly proofs.
No install/service/config migration, real-repo manual commit/push or fresh loaded
execution/full-fleet observation was performed by this continuation.

NEXT implementation remains: restore normal version/release bump (currently
version_bumped=false), unmerged-before-capture handling, artifact cleanup and
large-untracked hygiene WITHOUT replacing ownership/content safeguards; finish
actual queued-open/lease-break and exact-exchange qualification, unsupported-FS
controls, actual daemon restart/admission rehearsal and original product gates.
Then re-run ALL four gates on the coherent FINAL assembly, independently review,
qualify installed/loaded execution, safely apply authorized zero-batch migration,
and prove actual autonomous publication + every watched repo raw-clean/idle.
The captured baseline had37 repos; enumerate FRESH final inventory rather than
hard-coding37 or omitting newly watched repos. No outstanding concerns may remain.
Design: ../../docs/design/excluded-worktree-optin-2026-10-25.md.

## Recovery/follow-up v2 — HISTORICAL assembly evidence, not current proof

The active fleet-convergence goal remains authoritative, NOT the stale bucket
handoff. No installation, service change, real-repository manual commit/push or
live batch-policy migration was performed. The observed live service executable
was /home/dracon/.local/bin/dracon-sync; workspace release outputs were checked
not to alias installed utilities before building. This is NOT a loaded-v2 claim.

Persistent pending recovery now survives raw Git cleanliness and opt-in disable.
The daemon admits this metadata work independently of dirty classification.
Read-only diagnosis of every retained target recognizes exact untouched/rolled-
back desired bytes or exact displaced originals. Known partial/complete records
are fsynced and moved atomically without replacement to a private archive; no old
restore is replayed, no canonical file/index is changed. Unknown retained bytes
remain visibly blocked with a concrete recovery path. Foreign locks are retained.
Archives stay out of the pending dispatch scan to avoid rehashing history forever.

A real JSON test initially EXPOSED a false-healthy rendering path: the recovery
flag was present but current-health rendering overwrote concern=false/idle.
Current-health classification now treats that flag as an actual blocker and the
hint cannot be overridden by generic path-owner warnings. Passing real CLI proof
requires concern=true, health.current=blocked, non-idle cause and recovery hint,
with zero raw Git counts and byte-exact unmodified index/recovery. The RED report
artifact is retained as excluded-recovery-real-json-report.log.

Actual kernel-lease cancellation is now observed (ACTIVE READ lease in /proc/locks,
not an executor marker), followed by finite writer reopening of the canonical
inode and preserved later bytes. Private helper startup binds parent death to
SIGKILL before runtime threads; parent-crash edge interleavings are still NOT
fully qualified. Existing writable FDs and hardlinked sources are not silently
relocated. New empty-candidate follow-up detects unknown staged/ref updates and
later restored-path edits without resetting them. A real pre-push producer leaves
local bytes pending while the pre-existing backlog actually reaches its intended
disposable bare remote; no extra commit or stopped hook. The first fixture lacked
an upstream and correctly did not authorize bootstrap pushing: its failed log is
retained; the corrected fixture configures the EXISTING bootstrap gate, not a
weakened no-unwanted-push rule.

Identified v2 Rust assembly gates ALL passed:
- cargo test --workspace --locked: exit0; sync1461pass/0fail/3ignored,
  system616pass/0fail, warden233pass/0fail plus security/integration suites.
- cargo build --release --locked: exit0; NOT installed or executed-release proof.
- cargo clippy --workspace --locked --all-targets -- -D warnings: exit0.
- cargo deny check: exit0, no security/dependency waiver.
- Targeted controls: recovery5, restore9, original exclusions2, filter-only3,
  real JSON1, all0failed. These overlap the full suite; do NOT sum as extra tests.
The entire captured Rust workspace remained unchanged across tests and release
build, and the external pinned DEBUG helper SHA stayed unchanged. Assembly proof:
checks/epoch-integration-partial/excluded-recovery-v2-workspace-source-before.json,
*-workspace-source-after.json, *-release-source-after.json and
excluded-recovery-v2-checkpoint.json. Raw logs use excluded-recovery-v2-*.
Earlier failed builds/tests/report and original workspace failures remain intact.

DO NOT DEPLOY YET. Remaining source-known policy integration is still disabled:
normal version/release bump (version_bumped=false), unmerged resolution before
capture, build-artifact cleanup and large-untracked hygiene. Restore behavior
safely; green unit tests do NOT authorize silent feature removal. Further code
changes require a fresh coherent four-gate run; task4.1 remains in-progress for
the FINAL assembly, not waived by this intermediate green assembly.

Still require queued-open/lease-break and exact-exchange cancellation/crash
interleavings (including possible late displaced-writer activity after archival),
actual daemon restart/admission rehearsal, unsupported-platform/filesystem checks,
affected ORIGINAL product gates, independent review, qualified installed/loaded
execution, authorized zero-batch migration with its supporting daemon, and fresh
ALL37 autonomous-publication/raw-clean/idle proof with NO outstanding concerns.
Design: ../../docs/design/excluded-worktree-optin-2026-10-25.md.

## Explicit excluded-worktree opt-in — historical revision evidence

The original failing opt-in positive now passes WITHOUT changing its expected
HEAD-content assertion. Default preservation also passes. Restoration runs only
for the explicit repo opt-in, BEFORE ordinary private staging; never after hooks
or publication, never during dry-run or captured-intent handling. Excluded-only
work is handled without manufacturing a commit. The new path preserves the actual
index and unknown staging, uses real required smudge filters and exact filenames,
refuses foreign locks/content, and retains displaced binary bytes in a private
fsynced Git-directory recovery tree before Linux atomic exchange/no-replace.

A dedicated source-identity-bound helper owns kernel read leases and SIGIO
handling, not daemon threads. Already-open writable FDs cause refusal instead of
moving their future writes into a hidden backup. Parent deadlines own the helper/
filter process groups. There is NO unchecked overwrite/copy/unlink fallback on
unsupported primitives or filesystems; non-Linux opt-in restoration currently
refuses accurately. These implementation assertions are not a final safety verdict.

Observed new controls:
- excluded-restore-expanded-controls.log:8pass,0fail (binary recovery, deletion,
  untracked preservation, byte-exact foreign index/lock, active writer FD whose
  future bytes remain canonical, required-smudge refusal/later edits, actual
  hanging-filter descendant cancellation, dry-run, no-extra-commit and later
  running pre-commit producer preservation).
- excluded-restore-original-behavior.log:2pass,0fail, original preservation/opt-in
  tests. Original first-positive failure from stale helper identity is retained.
- excluded-restore-sync-strict-clippy.log: package all-target strict pass.
- excluded-restore-workspace-strict-clippy-final.log: workspace all-target strict
  pass after correcting newly arriving guard assembly failures, not suppressing
  warnings or tests. The new guard tests had3f64*u64 type errors and an OLD-time
  append after now. Correct types and a truly chronological latest30 fixture keep
  both original numeric assertions AND both bursts; add a chronological assertion.
  Both guard tests pass. Production rate logic changed only an equivalent parity
  spelling required by Clippy. Prior compile/behavior/lint failures remain raw.

Pinned actual DEBUG helper: excluded-restore-pinned-helper-path.txt (immutable
external copy; not Cargo's mutable output). Source/helper hashes and bounded
results: excluded-restore-source-checkpoint.json. Design and limitations:
../../docs/design/excluded-worktree-optin-2026-10-25.md.

NEXT INSIDE THIS MECHANISM: qualify cancellation while leases are ACTUALLY held,
new-writer/lease-break races, and interrupted/multi-file partial exchanges. Add
visible recovery/diagnosis so incomplete retained transactions cannot disappear
into a subsequent healthy/idle result. A recovery JSON/tree alone does not prove
that! Also verify truthful empty-candidate follow-up on later writes. Do NOT
claim this mechanism deployment-safe from the8current controls or Clippy alone.

Other disabled policy integration remains in scope: normal version/release bump,
unmerged resolution before capture, build-artifact cleanup and large-untracked
hygiene. Reconcile safely, not by disabling features or relaxing assertions.
Still require a fresh coherent COMPLETE workspace suite, release/deny, affected
original product gates, independent review, installed/loaded runtime proof,
authorized zero-batch migration with the supporting qualified daemon, and actual
ALL37 raw-clean/idle plus autonomous publication. No deployment, service change,
live policy migration or manual real-repository commit/push performed here.

## Full-workspace reassessment — historical revision evidence

Re-read the active goal after compaction: do NOT follow the unrelated handoff
brief. No implementation, policy migration, deployment or real-repository Git
mutation was performed in this reassessment. Qualification task4/4.1 is in progress,
NOT passed; unfinished implementation tasks remain unfinished.

Fresh `cargo test --workspace --locked` returned101. The sync binary now has
1446passed,1failed,3ignored: the SOLE failing test is
`sync::tests::test_excluded_tracked_file_reverted_when_opted_in`. The opted-in
excluded file is still edited instead of restored. Keep that positive assertion;
do not turn it into an expected refusal or permit a blanket post-hook restore.
Earlier workspace security suites passed, but later workspace members are NOT
fully qualified because Cargo stopped at this failure. Raw outcome:
checks/epoch-integration-partial/materialization-full-workspace-tests.log.

`cargo deny check` passed (advisories, bans, licenses and sources all ok), with
no configuration/security waivers. Raw outcome: materialization-cargo-deny.log.
All source files compared against materialization-source-checkpoint.json are
unchanged. HOWEVER target/debug/dracon-sync changed identity during the workspace
attempt; do not reuse the previous helper SHA as execution proof for this run.
materialization-full-workspace-attempt.json records this mismatch honestly. Before
FINAL qualification, pin a built helper in an immutable external artifact path,
verify source identity AND executable hash, and use that exact path throughout
behavioral qualification; a mutable Cargo output is not an installed receipt.
This failed attempt is not a release/runtime/installed qualification.

Next fix: restore explicit excluded-worktree opt-in safely, while keeping default
edits, unknown staging and later hook/producer edits intact. Merely re-enabling
`restore_paths_git` after publication is NOT safe: the existing candidate records
exclude paths but do not yet record ownership/content snapshots for resetting
their working files. A proper restore needs captured content/identity checks,
index/ref refusal controls, required-smudge correctness and durable recovery of
any bytes displaced during a race, with bounded cancellation. Do not implement
an unchecked overwrite or filename-prefix ownership waiver just to pass this test.
The other source-known disabled policy behavior below also remains in scope;
a green unit suite alone would not justify leaving it disabled or deploying.

## Storage accounting continuation — historical revision evidence

No deployment, canonical index recovery, manual real-repository commit/push,
service change or live zero-batch migration was performed in this continuation.

The two remaining SDK enrollment/outage gates now pass with the original
Synced assertions, plus STRONGER raw-visibility and changed-source controls.
Prepared now retains the configured asset root. Post-publication accounting
recognizes only its existing exact runtime-lock exclusion and materialized
sources whose current streamed SHA256 AND byte count match the upload-verified
manifest. Same-size edits, unknown children, missing inspection targets and
substituted symlinks cannot be called processed. Raw Git state is NEVER changed,
ignored, normalized clean or hidden: the enrollment test requires that native
Git STILL reports exactly the two raw sources and SDK lock. These are eligible-
work cycle outcomes, NOT evidence that a watched repo is raw-clean/all-idle.
The final fleet contract still requires its independent actual raw-clean proof.

A real producer pre-commit hook changes materialized source bytes AFTER upload.
The original manifest stays exact, raw producer bytes survive, and both that
commit cycle AND a subsequent NO-COMMIT cycle must remain StagedPending. The
SDK intentionally pins existing references until explicit eligible regeneration;
we did NOT force promotion or relax that guard. Returning the fixture producer's
input to already verified bytes clears pending WITHOUT removing/stopping its
hook. This is a positive/negative control, NOT a live producer modification.
Gitignored present sources are also inspected; absent materializations in sparse
consumers are not fabricated work. Failed inspections are not healthy/no-work.

Early RED artifacts are retained: materialization-real-sdk-cohort.log aborted
with a stack overflow from an inline64KiB async buffer; it was fixed by a bounded
heap-backed buffer, NOT a larger worker stack. The next streaming cohort passed
3original SDK gates but exposed false NothingToDo for changed private bytes in
a no-commit cycle; that RED result remains in
materialization-real-sdk-streaming-cohort.log. The no-commit/filter-only paths
now retain private-source pending state while still allowing backlog publication.

Latest matching-helper source cohort:
- materialization-pending-sync__bucket_qualification_tests.log:4pass,0fail,
  including original enrollment/retry/fresh-restore, outages/partial uploads,
  concurrent-manifest preservation and the new running-producer negative.
- materialization-pending-followup_accounts_for_exact_verified_bytes.log:1pass,
  both configured-root forms plus exact-byte/foreign-path/missing/symlink controls.
- materialization-compatible-bucket_assets__tests.log:7pass,0fail.
- materialization-compatible-integrated_failure_recovery_harness.log:1pass,0fail.
- materialization-pending-strict-clippy.log: strict workspace/all-target passes.
Source and DEBUG helper hashes: checks/epoch-integration-partial/
materialization-source-checkpoint.json. This is NOT release/installed qualification.

Next remains explicit excluded-worktree opt-in reconciliation and the other
unwired legacy policy behavior listed below (version bump/release, unmerged,
build-artifact cleanup, large-untracked hygiene). Keep default edits and uncertain
staging intact; no blanket post-hook reset or filename-prefix ownership waiver.
Then require a fresh coherent full workspace suite, release/deny and Warden gates,
independent review, qualified safe deployment plus authorized zero-value migration,
and actual ALL37 autonomous-publication/raw-clean/idle proof. No completion claim.

## Earlier resync continuation — historical revision evidence

The active goal file is authoritative; `.pi-glla/handoff-brief.md` describes a
different historical bucket-strategy goal and must not redirect this work.
This continuation made NO canonical index recovery, real-repository manual
commit/push, installation, service change or live batch-policy migration.

### Production repairs and observed controls

- Ordinary private candidates now prune size/policy-rejected delta entries back
  to their exact baseline BEFORE installation. Existing captured staged intent
  still refuses intact; it is never silently pruned or re-staged. Original
  candidate objects remain in the independent retained pack. The real
  filter-inflation regression now passes. The updated bootstrap oversized-cache
  regression proves refusal, byte-exact original index, tiny unstaged bytes and
  no partial/oversized root commit. New ordinary/captured negative controls pass.
- Both normal and unborn preparation are wrapped in the captured epoch's ONE
  absolute deadline, not another full budget per staging command. Warden probing
  is asynchronous and its invocation now uses the owned process-group runner.
  Two real 1s staging controls prove the hanging hardener and its descendant stop,
  actual index/HEAD stay unchanged and local bytes survive. See
  latest-absolute_staging_deadline_cancels_hardener_descendants.log.
- An empty private delta no longer acquires/installs the actual index: a real
  foreign staged+unstaged payload and foreign lock survive this boundary. For a
  genuinely nonempty candidate blocked by index admission, the daemon now still
  publishes already committed backlog via its original publication path, but
  returns truthful local Blocked, never healthy filter-only. An actual pre-push
  refusal remains PushFailed. Both success and meaningful-hook rejection controls
  preserve cached bytes, dirty worktree bytes and the exact foreign lock.
- Exact-path diagnostics no longer claim directory names authorize unknown
  children. The actual foreign-child/private-candidate preservation controls stay
  active, not replaced by a pathname or metadata-prefix waiver.
- Deletion, transient-lock and mirror controls retain first-pass byte/deletion
  assertions and require bounded real convergence afterward. The mirror test now
  checks BOTH actual remote refs equal final HEAD and raw Git is clean, rather
  than accepting a boolean result without inspecting either destination.
- Guarded bootstrap negatives now require visible guard errors rather than
  healthy Ok(false); valid protected reference bytes must commit unchanged,
  invalid staging must remain byte-exact and hydrated bytes stay local. Both
  storage-bootstrap controls pass. No storage/security guard was relaxed.

### Gate evidence and remaining work

`resync-workspace-tests.log` is an actual FAILED workspace run: the sync binary
reported1431passed/12failed/3ignored; preceding security suites passed but the
workspace run stopped, so later crates cannot be called tested. Later targeted
repairs cover most of those failures, across successive revisions, NOT a passing
fresh coherent workspace run. The integrated harness now uses a10s WHOLE-epoch
budget instead of the old3s-per-operation assumption; its original60s episode,
fault-entry and process-teardown assertions remain unchanged and it passes.
The dedicated1s staging cancellation controls remain stronger independent checks.

Targeted logs: backlog-filter_only.log (4pass),
backlog-sync__staged_epoch__tests.log (9pass), backlog-publication_actor_tests.log
(4pass), backlog-test_index_lock_contention_preserves_work_and_recovers.log
(1pass), backlog-meaningful-hook-refusal.log (1pass), followup-storage_bootstrap.log
(2pass), followup-integrated_failure_recovery_harness.log (1pass), deletion and
transient-lock followup logs (1each), and
 diagnostic-test_sync_repo_mirror_push_success_returns_true.log (1pass).
Latest strict workspace/all-target Clippy passes with zero warnings in
resync-latest-strict-clippy.log; temporary legacy dead_code allowances STILL need
removal/reconciliation before claiming final implementation readiness.

Still demonstrated: both SDK qualification cases return StagedPending after
verified enrollment. Diagnostic proof identifies exact native untracked paths:
bucket-assets.manifest.json.lock, public/a.png, public/b.png. These are existing
Prepared materializations/runtime-lock exclusions, not an arbitrary prefix waiver.
Resolve eligible-work versus retained materialization accounting WITHOUT hiding
raw Git state, ignoring later changed source bytes or weakening storage checks;
do not merely widen the expected outcomes. The enrollment diagnostic assertion
remains RED. Explicit `revert_excluded_to_head` is also still unwired and its
behavioral regression remains RED: preserve default edits and safely honor the
explicit opt-in, with content/ownership controls, not a blanket post-hook reset.

Additional source-known unfinished policy integration MUST NOT be hidden by
passing targeted tests: `version_bumped = false` currently disables normal bump/
release behavior; unmerged resolution is placed after capture that can reject
unmerged entries; build-artifact cleanup still explicitly refuses rather than
reconciling owned index/.gitignore deltas; large-untracked hygiene helpers remain
unused. Restore intended policy behavior safely or diagnose concrete conflicts;
no silent feature disabling or wholesale waiver is authorized.

Latest inventory: checks/epoch-integration-partial/resync-source-checkpoint.json
records current source hashes and matching DEBUG helper identity. Logs span
revisions; rebuild the real helper after each further source edit. Final release
build, deny, full workspace tests, Warden qualification, original repaired gates,
independent review, safe installed/loaded runtime and ALL37 clean-idle/publication
proofs are still required. Live zero-count migration remains authorized but MUST
wait for its qualified supporting daemon. Do not call the goal complete.

## Earlier executor continuation (historical revision evidence)

NOT final qualification; no new daemon deployment or canonical-index recovery.
The implementation worker's partial result was assessed, not accepted as done.
Ordinary staging now uses a private index, a retained/fsynced delta pack and
expected-state installation. Parent boundary regressions passed3tests. A real
matching debug-helper test committed exact captured staged bytes while retaining
both later unstaged bytes and unrelated untracked content. Native Git2.51 rejected
two attempted HEAD-symref+referent transaction forms; their RED logs are retained.
The repaired path CAS-updates the explicit captured branch with its expected old
OID, under the owned index lock and immediate HEAD/index checks; detached HEAD
uses no-deref. A post-publication branch change remains pending, not clean.

Known publication no longer becomes a failed commit when post-commit inspection
or its absolute deadline fails; the real slow-post-hook regression passed1test.
Remaining local work no longer returns before publishing the known immutable
commit. A four-case outcome regression passed1test. Expected index/branch/lock
conflicts are typed nonfailure deferrals; arbitrary inspection errors are NOT
classified by message text (1test passed). The original concurrent-add fixture now
explicitly unsets GIT_INDEX_FILE for its FOREIGN add: cycle1 retains the private
candidate and foreign staging, cycle2 commits only captured staging, cycle3 commits
remaining worktree intent. Exact intermediate/final data assertions passed1test;
this is intentionally not a blanket next-cycle whole-worktree absorption.
Logs: checks/epoch-integration-partial/parent-*.log. These runs span source revisions,
not a fresh single-source complete suite or a guarded SDK qualification cohort.

**Operator decision:** disable the 100-file count cap; make batching opt-in with
0 meaning unlimited. Source default/example and all three consumer paths now use
that convention. zero-batch-policy-tests.log passes2tests: omitted/explicit zero
and positive parsing, plus120 captured files and size/exclusion negative controls.
zero-batch-production-epochs.log passes2real matching-helper tests: normal worktree
and unborn bootstrap each commit all120related files together (one new commit). Do not raise/remove size, ownership, storage, hook or deadline
safeguards. The effective live global policy is STILL100 intentionally: the old
loaded binary interprets zero as an empty batch. Apply the authorized zero-value
migration only together with a qualified supporting daemon, never to the old
runtime first. See checks/epoch-integration-partial/pending-batch-policy-migration.json.

The read-only strict-subset design scout failed at its provider usage limit;
no result was accepted and no automatic retry or alternate CLI was used. The
operator's unlimited-count decision removes the need for artificial subset commits
on this fleet. Independent final review remains required after implementation.

Latest broad unit run: current-all-unit-tests.log exited101 with1419passed,
20failed,3ignored (496.08s). This is NOT qualification. Failures include sealed
hook message-path expectations, guarded storage bootstrap/decommission cases,
bucket materialization/pending accounting, publisher/staged-intent expectations,
explicit excluded-worktree reversion, staged-blob size sweeps, filter-only/ahead
publication, lock admission, retired prefix-guard expectations and ordinary
first-pass pending metadata. Diagnose them; do not simply accept Pending everywhere
or weaken storage assertions. Some old expectations restage partially staged data
or call newly generated untracked metadata clean; replacements must strengthen
exact-byte and eventual clean/idle proofs, not conceal regressions.

After that run, actual lock admission gained an I/O-kind-preserving API:
AlreadyExists is typed contention/defer; missing-parent/permission/inspection errors
remain genuine failures. typed-lock-admission-test.log passes1negative-control test.
Real full commit-path contention recovery is not yet rerun on this revision.
Two former failing assertions were repaired with STRONGER preservation checks:
owned publisher contention must commit the captured staged blob (not sweep the
unstaged bytes), while retaining the unstaged file, cached OID, untouched remote
and unchanged failure memory; its targeted run passes1test. The original four-hook
lifecycle test now verifies exact argument positions with the owned private message
path, hook-rewritten commit text, temporary cleanup AND preservation of an existing
operator COMMIT_EDITMSG draft; its targeted run passes1test. Logs:
preservation-publication-owner.log and preservation-private-message-lifecycle.log.
The cross-repo publication test now permits truthful StagedPending ONLY alongside
real newer local HEAD; its pinned-tip and bounded final remote convergence
assertions remain unchanged. That edited test has not yet been rerun.

Three ordinary fixtures now require bounded metadata follow-up AND raw Git
cleanliness, not a wider outcome list alone: auto-hardening generates eligible
metadata after original classification. The dirty-repo targeted case passes1test
(metadata-followup-raw-clean.log); the mixed/untracked cases still need rerun.
The supporting helper was rebuilt from current source for these functional checks.
parent-current-source-inventory.json records current source/file hashes and that
DEBUG helper, not a final qualification or installed receipt. Rebuild after any
further source changes; never use installed-old/fake verifier fallback.

Next: finish ordinary-path policy/metadata/cleanup authority reconciliation,
retain all old policy behavior or diagnose an explicit conflict, remove unused
legacy code only after its behavior is integrated, run final strict workspace and
actual guarded/ordinary/index-race/signing/hook/cancellation/deadline cohorts from
one frozen source with real matching helpers, qualify Warden runtime changes,
deploy with loaded/executed evidence plus the authorized policy migration, then
obtain fresh ALL-watched clean-and-idle/ref/publication receipts. No all-idle claim
is supported yet; do not stop producers, reset indices or weaken gates to get one.

## Historical baseline and research (retained context)

Goal: .pi-glla/goals/20261025090441-ya0tp4.md. Strong final requirement is ALL
watched repositories clean AND idle in a fresh observation, not merely previously
published ancestors. Do not hide eligible work or stop editors/producers to obtain
it. No index recovery or deployment has occurred yet. Source-only inspection
repair now skips redundant dirty-child recursion ONLY with complete watched
coverage of indexed gitlinks; uncovered children retain full Git status, child
HEAD/index gitlink changes stay visible, and coverage is revalidated under the
same absolute cancellation budget. Daemon and report pass their full observed
inventory. 11 inspection development tests passed (see checks/inspection-development.log),
including partial coverage, child visibility, gitlink advances and existing
cancellation/root-wiring tests. That run emitted an unused-import warning, now
fixed in source but not yet rerun; it is NOT final strict qualification.

The read-only observer's inventory cardinality input was generalized, with
regressions preserving legacy-ten checks and rejecting omitted/duplicate/alias
paths. scripts/test-fleet-convergence-observe.py passes6tests. Full fresh ref
receipt baseline-refs.json covers ALL37 with zero repository/query failures and
the same loaded executable before/after. Initial capture/setup errors (fixed-ten
assumption, missing baseline_head and temporary import path) were not successes.

## Fresh evidence

- baseline-repos.json: full dracon-sync repos --json output,37repositories,
  30OK/6active/1warn/0concerns/0inspection failures at capture. Row state can
  change legitimately while work continues; do not reuse the previous goal's
  ten-repository inventory as this goal's fleet.
- baseline-runtime.json and baseline-service.txt identify the actual loaded
  executable and service. Preserve qualified source/revision semantics later.
- baseline-failure-journal.log retains Doomtap's repeated staged-intent refusal
  and historical platform inspection timeouts.
- doomtap-index-entries.z snapshots all2050index entries; doomtap-problem-path.json
  records the tracked pre-staged M entry for docs/screenshots/states/minion-burst-die.png.
  Index blobbbb838bc04782b4fe46143895717ee5cec6f9e56; worktree file1,732,735bytes,
  not a size-limit violation. Explicit !*.png rule includes it. No mutation made.
- platform-process-snapshot.json observes a real parent-repository push, not a
  currently hung status process. Prior status timeouts do not establish that
  every later platform operation is stuck.

## Research findings — assessed, not yet implemented

The first scout report is saved in its retained artifact; archive it into this
evidence directory before final reporting. It rules OUT current Prepared/private
bucket enrollment for Doomtap. HEAD and index are213byte legacyasset POINTERS,
not raw PNG Git blobs; doomtap-pointer-evidence.json records their different
payload SHA256s. The raw worktree matches the index pointer payload. !*.png does
not disable dracon-assets clean filtering. Never invoke live clean/diff/status
under the assumption that custom filters cannot upload or update manifests.

More recent actual full journal context proves Doomtap now reaches precommit:
storage highwater reports OK, followed by machine-local screenshot/report hygiene
refusals. Index-only removal is explicitly not a durable fix for deliberately
included content. Investigate exact curated deliverables/non-security inclusion
semantics; do not widen a blanket waiver or erase staged bytes. Earlier repeated
extras messages remain historical, not the current sole observed blocker.

Source-proven independent gap: compute_diff_entries drops Modified entries absent
from net HEAD/worktree diff, even if their staged index blob differs; nonempty
unrelated entries prevent the all-empty fallback. Need real-repo regression,
exact captured staged identities and preservation of partially staged content,
not a pathname-only extras waiver. Actual Doomtap instance cause of earlier
extras is still unproven; don't mislabel the source gap as observed causality.

Platform bounded measurements: normal owned-equivalent Git status13.256s with16
nested child starts; --ignore-submodules=dirty2.804s withzerochildren. Difference
is dirty/untracked nested Doomtap and Hegemon worktrees; current child HEAD/index
pointers agree. This supports expensive redundant nested scans, NOT a claim that
this13srun timed out. Any optimized parent scan must retain HEAD/index gitlink
changes and separately visible child work; do not hide unobserved child content.
Measurements/raw records/traces are in platform-status-* artifacts.

## Original source leads

sync.rs1191 staged_extra_paths protects against foreign concurrent staged work;
~4443 comment promises next-cycle inclusion, but actual Doomtap refusal repeats.
~6000 classification explicitly removes private_assets.materialized_paths before
stage intent. Determine whether the screenshot is such an enrolled path before
assuming that is the cause. Never simply waive the guard, absorb uncertain staged
content, unstage an operator blob blindly or commit raw storage-managed assets.

Existing designs read: docs/design/fleet-repair-final-2026-10-16.md and
ui-delay-investigation-2026-10-16.md. Historical sanctioned rewrite in that earlier
report is NOT authority for this goal. Its assumptions/deadline/cancellation limits
and Warden filter investigation are relevant; do not claim a timeout root cause
from a generic error alone.

Both first scouts settled and were assessed; staging-scout.md and
staging-design-followup.md are archived here. No tiny pathname union can close
the native commit API's final index race. Follow-up also corrects a prior parent
assumption: default max_stage_batch_files is100000, not100; verify effective
policy rather than treating120staged as an actual current cap violation. A strict
cap, if exceeded, must refuse truthfully without unstaging user intent.

New read-only scout bb8ed530-4700-46c5-92e0-a0daf5eefa10 is researching installed
Git native commit/index-lock timing and hook forwarding/rollback in disposable
fixtures only. Assess its result before picking the atomic transaction design;
a hook wrapper alone cannot solve provenance of read-after-native-git-add.
Parent owns source mutations. No canonical branch/identity/security changes.

Next: assess follow-up design; full37remote/object baseline is now captured.
Inspect actual Doomtap hygiene refusal and reproduce staged-intent root gaps in
disposable fixtures, then make
small forward-only repairs with preservation/refusal tests before any live recovery.
baseline-staged-local-paths.json now losslessly records staged raw path/mode/OID
metadata and local changed/untracked path lists for all37, base64 for exact path
bytes; all74metadata queries succeeded. No content, index or policy was changed
by those raw/no-textconv metadata queries. Baseline capture task1 is complete;
staging task2/2.1 is in progress. See confirmed goal for all8gates.
