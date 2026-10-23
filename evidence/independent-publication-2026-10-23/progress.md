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

## Integration landed and verified (still not deployed)

The structured per-repository actor is implemented and exercised end to end:

* `run_publication_actor` plans a round under local ownership, drops the
  operation lease, and runs the upload inside `push_phase` + `publish_phase`.
  While that upload is blocked it re-admits bounded local passes through
  `local_phase`, each taking its own short operation lease, then reacquires
  local ownership before post-publication bookkeeping. Guard drop order keeps
  child work before publication-lease release.
* `publication.rs` pins the exact object+branch, applies the pack/privacy/pause
  exclusions, prepares mirror configuration as LOCAL work, and owns the
  per-remote failure map. Upload performs no pull/merge/config/checkout
  mutation and every origin/mirror retry and HTTPS fallback names the same
  pinned object.
* The global push semaphore is gone. Different repositories' push commands
  enter concurrently; the only remaining limits are the existing local
  semaphore, per-repository publication ownership, operation deadlines and
  owned process groups.
* A caller that cannot obtain publication ownership commits through the
  commit-only path and reports `PublicationPending` — never success, never a
  transport failure, never a failure-counter reset.
* `git commit --no-verify` in an owned, bounded process is used for auto
  commits, preserving the daemon's long-standing commit-hook policy (in-process
  bucket/storage/exclusion/size gates; pre-push hooks still run).
* Truthful phases: `Committing` and `Pushing` stamps are set only at real
  operation boundaries, expire if abandoned, and `Current::Publishing`
  renders `PUBLISHING` instead of WORKING or CLEAN.
* Shutdown now requests cancellation for owned workers and JOINS them within
  the trailing-drain budget. If they do not join, it keeps the in-flight and
  scheduler evidence rather than claiming a clean stop.

### Passing bounded checks (receipts under `evidence/independent-publication-2026-10-23/`)

* `daemon-suite.txt`: `cargo test -p dracon-sync --bin dracon-sync --locked -j 1`
  — **1389 passed, 0 failed, 3 ignored** (255s), undisturbed run with no other
  cargo process touching the test binary.
* `git-suite.txt`: the whole `git::` subtree — 324 passed, 0 failed.
* `isolation-suite.txt`: the three pre-existing hook-isolation regressions now
  drive the production actor boundary (capacity, nice 19, rejection,
  cancellation) — 3 passed.
* New `sync/publication_actor_tests.rs` (2 tests, part of the 1389):
  * A's publication blocked in a real pre-push hook, B commits AND publishes
    concurrently, a SECOND real commit lands in A while A's upload is still
    blocked, releasing the hook publishes A's pinned tip, and bounded
    follow-up rounds converge A's bare remote on A's newest tip.
  * A held publication owner still lets a new edit commit and reports
    `PublicationPending` with no competing push.
* New `sync/owned_commit_tests.rs` (3 tests): daemon commit-hook policy pinned,
  publication-pending accounting conserved, and cancelled/expired publication
  kills the owned Git+hook+helper process group with nothing published.
* `scheduler_evidence` 4 tests including stamp expiry; `current_health` 4
  tests including the new PUBLISHING row.

### Regressions I caused and fixed at the root

1. The split first mutated a CLONED failure map in the plan, so a failed push
   could never reach the caller. Replaced by an owned plan map that the
   round hands back explicitly.
2. Routing auto-commits through plain `git commit` made daemon commits execute
   `.git/hooks/pre-commit`, failing a bucket fixture on warden config. Restored
   `--no-verify` (documented policy) and pinned it with a regression.
3. Five later failures (`sync_lease`, `storage`) were self-inflicted test
   interference: I ran a second `cargo test` while the long suite was still
   running, which relinked the binary and broke `current_exe()` mid-run. They
   pass in isolation and in the undisturbed full run above; never run cargo
   concurrently against one target dir.

## Remaining implementation slices

1. Release identity: `get_bump_info` still does unbounded `spawn_blocking` Git
   reads, and `run_release_pipeline_if_bumped` runs AFTER publication. Under
   concurrent local commits, that can tag whatever HEAD is at that moment
   rather than the version that was actually committed. Capture the exact
   version/commit obligation under local ownership and give the read a bounded
   owned call; do not let a version bump be released against a newer tree.
   Tag/Nix/janitor/default-branch paths already run inside `push_phase` and the
   operation lease, but they must be re-audited once publication is genuinely
   concurrent (they mutate local state).
2. CLI/one-shot parity: `sync-now`/`once` already go through the same
   `sync_repo_with_ahead_since`, so they inherit publication admission and the
   honest pending report, but there is no daemon-side rediscovery loop for a
   CLI owner that exits between rounds. Confirm the next daemon pulse picks up
   an unpublished tip committed by a CLI process (it should: dispatch is driven
   by actual ahead/dirty state, not by a queued request).
3. Reload identity: verify SIGHUP does not drop the publication lease or the
   per-repo failure memory while a worker is running.
4. Qualification: workspace tests, release build, `-D warnings` Clippy,
   `cargo deny`, then a safe install/restart and the isolated installed-binary
   receipt plus live autonomous observations.

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
