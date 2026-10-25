# Active systematic fleet-idle repair — checkpoint

## Resync continuation — current source, NOT deployment qualification

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
