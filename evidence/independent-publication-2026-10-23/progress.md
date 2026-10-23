# Implementation checkpoint — independent publication

This is unfinished implementation, not a deployment or completion receipt.
The authoritative objective/contract is the active goal document, not the
unrelated bucket-platform text in the older global handoff brief.

## Current ownership and tested changes

Parent is the only source writer in dracon-sync, on the existing main branch.
No service actions, installation, live manual pushes, state repairs, trust or
identity changes, history rewrites, or checkout removals were performed.
The normal daemon may auto-commit these files independently.

* `src/git/push_resources.rs`: removed the process-global push semaphore.
  Owned command futures and push-child nice 19 remain. Added `local_phase` to
  reacquire the EXISTING local semaphore temporarily while an actor's outer
  publication scope remains yielded; nested admission reuses its permit.
* `src/git/sync_lease.rs`: canonical common-directory identity and an independent
  persistent `dracon-sync.publication-lease`, with the same regular-file,
  owner/link, no-symlink and nonblocking kernel-lock checks. Existing operation
  lease remains local mutation ownership. Never unlink either lease file.
* `src/git/push.rs`: validated immutable `PublicationTarget`; pinned origin
  retries/fallbacks use exact object+branch and cannot pull/merge on rejection.
  Ref-race verification can compare the pinned target rather than moving HEAD.
* `src/git/multi_remote.rs`: split `prepare_mirror_remotes` (configuration
  mutation) from uploads; pinned named-remote API preserves its refspec across
  retries/fallback and cannot enter force/reconciliation paths. Legacy wrappers
  still exist for current callers; the actual actor has NOT been wired yet.
* `src/sync.rs`: ordinary commits and bootstrap use a bounded, owned Git CLI
  command with hooks and configured identity, not GitService::commit's detached
  blocking mutation / no-verify fallback. Explicit bucket/storage checks remain.
* Public sync now acquires publication ownership nonblockingly before local
  ownership. If another publisher exists, it runs commit-only and returns the
  new `PublicationPending` outcome, not success or transport failure. Commit-only
  skips GC/pruning, pull, origin configuration and metadata publication.
* `SyncContext` now owns operation/publication File guards. They currently only
  keep ownership alive; their handoff to the actor is still unimplemented.
  The compiler warns that these fields are never read. Wire them; do not mask
  that warning or install this unfinished build.
* `src/main.rs` and `src/daemon.rs` distinguish publication pending. The daemon
  retains activity, blocked age, retry counters, cooldowns and stuck evidence.

## Passing bounded checks (not workspace qualification)

Receipts are copied into this directory where named below; original working
logs are under `/tmp/sync-convergence-20261023/checks/`.

1. `cargo test -p dracon-sync --bin dracon-sync --locked -j 1 git::`
   — 322 passed, zero failed, three existing ignored. Six Git input hashes were
   unchanged across this run (`primitive-inputs.json`, `primitive-tests.txt`).
   New primitive fixtures prove a second real commit while a pinned push hook
   blocks, identical old pinned origin/mirror refs after HEAD advances, later
   newest-tip uploads, rejected pinned uploads without merge/local edits loss,
   independent local/publication leases across processes and worktrees, unsafe
   lease rejection, and multiple push commands entering concurrently.
   These are upload primitives, NOT the integrated daemon actor.
2. `cargo test -p dracon-sync --bin dracon-sync --locked -j 1 push_resources`
   — four passed, including bounded local refresh and cancellation dropping
   its work before capacity release (`local-capacity-tests.txt`).
3. `cargo test -p dracon-sync --bin dracon-sync --locked -j 1 owned_commit_tests`
   — three passed in `pending-owner.log`: real commit with another publication
   owner, no push/no failure reset; rejecting pre-commit preserved staged and
   unstaged content; cancellation AND deadline killed hook descendants and a
   subsequent real commit succeeded. Earlier two-boundary receipt is copied as
   `owned-commit-boundary-tests.txt`.
   The first version of the new pending-owner fixture failed because derived
   `SyncPolicy::default()` has a zero staging size limit. Reverted that fixture
   choice to parsed production defaults; size guards/assertions are unchanged.
4. `cargo test -p dracon-sync --bin dracon-sync --locked -j 1 test_m4_helper_structurally_unified`
   — one passed (`pending-accounting.log`), including pending-publication
   failure/backlog/blocked-age conservation assertions.

An earlier 600-second compile-only attempt expired (124). A subsequent bounded
1800-second run completed. No full workspace/build/Clippy/deny qualification
has been claimed for this revision.

## Baseline products and scope boundary

Initial JSON records browser-extensions-shared at
`ba2035f0b684e3a4a67eb7402bf07a8f05dd8d35`, with zero dirty files. Baseline
artifacts: `/tmp/sync-convergence-20261023/baseline-repos.json` and journal.
Read-only scout reports: `/tmp/sync-convergence-research.task-{1,2}.md`.

* Observed volume-and-video-pro refusal: journal 2026-10-22 23:57:55, TEST exit 1.
  Its original detailed log directory is missing; no original failing assertion
  or code root cause is established. Seven focused initial files passed 102.
* A disposable archive of the EXACT initial volume source passes all 178 tests
  in 14 files, first serially and then using the ORIGINAL unmodified TEST
  command, `vitest run --passWithNoTests` (11.81s). Installed dependencies were
  reused; shared package/dependency-source diff from initial commit is empty.
  Receipts: `volume-initial-source-tests.txt`, `volume-initial-original-test.txt`.
* Concurrent edits subsequently changed volume App.svelte, popup tests and
  additional files in the live product checkout. A run during those changes
  failed two new cases with Svelte effect_orphan. Preserve the receipt
  `volume-concurrent-change-tests.txt`; this is NOT proof about the missing
  original refusal and is outside baseline remediation unless our work causes
  a regression. Do not overwrite another writer's product changes.
* clean-web working source is unchanged from the initial commit. Its ORIGINAL
  TEST command, using workspace/root .bin PATH as the hook does, passes 924
  tests / 64 files (34.19s). Original log:
  `/tmp/sync-convergence-20261023/product-gates/clean-web-original-test.log`.
  WRONG HAPPY-DOM diagnostics occur in the passing output; do not infer an
  actual current runner mismatch from that text alone. Older timeout attempts
  are historical, not reproduced current code defects.
* Older retained auto-form-filler assertions/lint, youtube-filter type errors
  and SamAI failures belong to earlier attempts; the later journal gate swept
  all workspaces and named only volume (then a subsequent clean-web attempt).
  Do not repair currently passing source based only on old logs.
* No product source/tests were edited. Remaining applicable original lint,
  typecheck and build gates still need qualification. Do not weaken assertions,
  remove guards, increase ceilings blindly or invent a repair for passing code.
* aud-vis different-worktree metadata refusal is a real separate checkout
  blocker. Rewiring it and resetting live retry/state ledgers are NOT authorized.

## Next implementation slice — actual actor, not another architecture interview

The chosen durable design is a STRUCTURED PER-REPOSITORY ACTOR inside the
existing supervised worker, not a second global publication task registry.
One owner/failure map, independently polled local and publication futures,
canonical kernel leases, and no detached publication tasks. See the design
file `docs/design/independent-publication-2026-10-23.md` and recorded judgments.

1. Route both post-commit and ahead-only publication through an actor boundary
   receiving mutable SyncContext. Capture immutable object+branch and pack/
   destination guards under local ownership. Use the pinned transport APIs
   for EVERY origin/mirror retry/fallback. Keep each repository's actual
   publication commands serialized; different repos remain unconstrained.
2. ONLY AFTER upload is mutation-free, take/drop ctx.operation_lease and yield
   local capacity via push_phase. DO NOT release the lease around today's
   unchanged push_background_inner: it can still pull or edit configuration.
   Mirror setup must run under a short re-acquired local lease/admission,
   separately from actual upload; no push slot/global limiter.
3. Concurrently poll publication and a local refresh loop. Each refresh uses
   local_phase and the private sync_repo_local_with_ownership commit-only
   path (boxed-return boundary if necessary to avoid recursive async types).
   It must revalidate current policy/ownership and preserve staging/content
   gates. Its remote-failures argument is None; only publication owns counters.
   Honor settling/cooldowns rather than hot-looping filter-only diffs.
4. Use a stop signal + structured join: completed upload stops NEW refresh
   admission but lets a current local commit finish. Do not cancel a commit
   merely because upload finished. Restore context/local mutation ownership
   before post-publication mutation. Keep guard drop order child work BEFORE
   publication lease. All work remains under finite existing actor budget,
   never reset by progress/follow-up; bounded rounds then pending rediscovery
   avoid an unbounded internal drain loop.
5. Recheck actual latest HEAD under local ownership after a round, coalesce
   newer work into bounded follow-up, and preserve pending/backlog evidence
   when only an older tip reached remotes. Startup/periodic inspection remains
   durable rediscovery, including another process's commits. Preserve all-paused,
   degraded, real failure, incident shielding and cancellation accounting.
6. Version/release/tag/Nix/default-branch/janitor paths need the same ownership
   protocol. Capture exact version/tip obligations before concurrent changes;
   do not release tags against whatever HEAD happens to be later. Do not lose
   configured release behavior or weaken immutable storage guards.
7. Add independently observed local/publication phases to fresh scheduler
   evidence and report consumers (command-start, waiting, committing, pushing,
   cancelling, blocked), not generic Working as proof. Historical PUSH remains
   separate. Preserve stale/future-clock fail-closed behavior.
8. Explicit daemon shutdown must cancel AND join owned workers/maintenance
   before clearing stopped evidence; current daemon tail lacks that join.
   Keep reload ownership identity and original finite watchdog semantics.

## Known local mutation lifecycle gap to resolve before qualification

`storage_guarded_commit` still invokes `storage::commit_configured_storage`
through spawn_blocking. Dropping that await does NOT stop mutation. The guarded
storage implementation seals a verified immutable tree under its own
storage_core::index::CommitLock; DO NOT replace it with unverified ordinary
Git commits or weaken the immutable-tree gate. Prefer running that exact
transaction in an owned, bounded helper process, preserving kernel mutation
ownership until child termination; inspect existing storage CLI before adding
an internal helper. CommitLock already has positive private-marker recovery
for its OWN index lock after process death; foreign locks are preserved.

The previous GitService::commit also had detached blocking mutation, synthetic
signature fallback and no-verify CLI fallback; ordinary path no longer uses it.
`get_bump_info` still has unbounded spawn_blocking Git reads; use bounded owned
capture when integrating frozen release identity. Do not pretend those old
helpers are cancellation-safe or merely wrap their await in timeout.

For strongest cancellation/lease ownership, consider scoped inheritance of
kernel lease FDs by the OWNED principal process: child holds ownership until
actual exit, not merely until the parent sends SIGKILL. Keep inheritance
controlled and child-before-lease teardown explicit; no broad process killing
or foreign-lock cleanup. Any helper must inherit configured identity unchanged.

## Outstanding work/verification

Task 1 is marked complete; task 2 in progress; tasks 3/4 pending. No completion
claim. No deployment, installed receipt, actual loaded revision or new autonomous
live convergence observation yet. Full workspace/release build/Clippy/deny still
required, as is one fresh-context reviewer rehearsal after integration.

A new read-only scout is researching exact installed-daemon fixture CLI/state/
transport isolation: run `40cce4e0-aeef-4287-9cd6-685d9a8cd97c`, output bound to
`/tmp/sync-convergence-installed-harness.md`. Native completion will notify;
do not poll or wait for that child. It has no source edit or execution authority.
The previous two research children are finished and their reports were read.
