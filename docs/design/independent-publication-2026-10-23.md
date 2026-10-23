# Independent local commits and publication — 2026-10-23

Status: implementation in progress; not a qualification or deployment receipt.

## Observed failure and scope

The daemon has a process-global push semaphore of one in
`src/git/push_resources.rs`. Separately, a worker reserves its repository and
holds `dracon-sync.operation-lease` throughout preparation, commit, hooks,
network publication and post-publication work. Yielding its local semaphore
permit permits OTHER repositories to commit, but cannot permit fresh commits
in the SAME repository. Increasing the global semaphore would fix neither the
second problem nor misleading whole-worker `Working` evidence.

The existing publication helper is not upload-only: rejection recovery may
pull/merge/abort; mirror setup edits Git configuration; release follow-up can
create tags or perform Nix maintenance. Releasing mutation ownership around
that unchanged helper is unsafe. Publishing moving `HEAD` is also unsafe once
local commits can overlap a publication round.

The solution is independent bounded local work and owned publication, not
fire-and-forget push tasks, extra global queues, or a timer resetting stuck
state. The existing content, ownership, privacy, pack, incident and hook gates
remain mandatory. Direct operator Git remains outside the cooperative protocol.

## Ownership and publication invariants

* The existing common-directory lease remains exclusive LOCAL mutation
  ownership. Preserve alias/worktree key resolution, checked persistent lock
  files, and nonblocking kernel locking. Never unlink a lease file.
* Add common-directory PUBLICATION ownership, held by one coordinator-owned
  round through joined cancellation/teardown. A local commit may hold local
  ownership while an upload holds publication ownership. Two upload owners
  cannot coexist for aliases, linked worktrees, CLI and daemon.
* A path requiring both acquires publication then local ownership, or uses
  nonblocking acquisition and defers. Never wait for publication ownership
  while holding local ownership. Destructive/history-sensitive maintenance
  requires both; no new live state repair is authorized by this design.
* Under local ownership, prepare a typed immutable publication plan: common
  directory, checkout, branch, exact object ID, validated destinations and
  policy, and any exact version/release follow-up identity. Configuration
  preparation is local work; uploads cannot pull, merge, abort, switch branches,
  rewrite, prune, or change configuration.
* Every destination, retry and HTTPS fallback publishes the SAME pinned object
  ID for a round. Rejection requiring local reconciliation returns a typed
  result, then retries only after separately owned local reconciliation and
  a newly validated plan. Do not silently disable an existing safety gate.
* Coalesce head-publication requests to the latest generation per repository.
  Preserve distinct versioned release obligations rather than dropping them
  with superseded head requests. Retain an immutable round tip while needed.
* Local completion must not await hooks or network publication. A new edit
  gets the same staging/content gates and local concurrency budget even while
  that repository's publisher waits/runs/retries. The old moving-HEAD backstop
  must not suppress these commits under pinned-tip publication.
* No global push-count admission exists. Per-product hook resource controls,
  daemon push-child nice 19, finite deadlines and owned process groups remain.
  Increased concurrent hook memory/task pressure is a real trade-off, not an
  excuse to add another hidden global limiter.

## Coordinator and accounting

The existing supervised worker becomes a structured per-repository actor with
independent local and publication futures. Its scheduler reservation identifies
the actor, not exclusive local execution: during upload it periodically obtains
local capacity and a SHORT mutation lease for fresh local passes. The actor
retains the publication lease across its owned round and teardown; the kernel
lease is keyed by resolved Git common directory, not raw checkout spelling.
This avoids duplicating task registries and remote-failure-map writers.

Actual HEAD under local ownership, rather than queued messages, is the durable
latest-tip request. Recheck it after a round and arrange bounded follow-up at
the newest validated tip. Startup and periodic reconciliation rediscover
unpublished tips and missing mirrors, including commits by another process.
No completion may discard newer local activity. A CLI encountering an existing
publisher may still commit, then reports publication pending rather than
starting a competing publisher.

An old-tip success records a real attempt, but does not mark newer HEAD
published, clear newer local activity, or erase another remote's failure.
Only the publication result owner updates remote counters/history; successful
local commits must neither increment nor clear them. Keep all-paused,
degraded, genuine-failure and interruption distinctions. No network attempt
means no transport failure. Local progress must not be suppressed by a
publication retry/exhaustion gate.

CLI/once must use the same leases and publication implementation, await its
owned work when admitted, and report deferral truthfully when another owner
exists. No detached CLI child or competing implementation.

## Lifecycle and reporting

Local passes and publication commands retain finite operation deadlines. The
supervised actor also has an overall finite budget that follow-up rounds must
not reset. Reload retains ownership and failure identity; subsequent local
passes and publication rounds revalidate current policy. Upload completion
stops future refresh admissions but joins a currently executing local pass
rather than cancelling it midway through a commit. Shutdown stops admission, cancels and joins owned jobs, then clears
running evidence. Cancellation cannot free publisher identity while old hooks
remain runnable. Keep runner teardown before lease release.

Expose independently observed local and publication phases/timers, including
simultaneous committing and pushing. A reservation is not command execution;
lease wait/retry/cancelling/blocked are not successful updating. Preserve
freshness expiry, future-clock fail-closed behavior and historical PUSH as
separate evidence. Verify newest-tip convergence from actual Git objects and
remote refs, not labels or installation metadata.

## Baseline evidence and remediation boundary

Initial read-only observations are in `/tmp/sync-convergence-20261023/` and the
bounded research reports `/tmp/sync-convergence-research.task-{1,2}.md`.
The initial repository observation had 36 rows, 29 OK, 6 active, one warning
and one status failure. Representative ahead backlogs included
browser-extensions-shared, ai-auto-writer, endless-td and dracon-platform.
These counts are observations, not permanent test requirements.

The journal establishes a volume-and-video-pro TEST refusal at
2026-10-22 23:57:55; the original referenced attempt directory is missing.
Seven focused files passed (102 tests) during initial diagnosis. This does
NOT establish a root cause or full gate success. Retained earlier attempts
also show specific auto-form-filler assertions/lint, youtube-filter call-signature
errors, SamAI lint-rule/keyboard-test problems and clean-web runner/timeouts;
they are historical snapshots, not automatically current defects.

A subsequent bounded full serial volume suite failed two popup-surface cases
with Svelte `effect_orphan` (178 passed). During that run App.svelte AND the
popup test file were independently edited in the live product checkout; the
new cases were absent from the earlier successful focused run. Preserve this
as subsequent-change evidence, not proof of the missing original refusal or
permission to overwrite another writer. The live suite log and concurrent
App diff are retained under `product-gates/` in the observation directory.
No product source or test was edited by this investigation.

The initial JSON row records browser-extensions-shared's exact initial commit
`ba2035f0b684e3a4a67eb7402bf07a8f05dd8d35`, with no dirty files. A disposable
archive of that commit's volume source passes all 178 tests across 14 files
with installed dependencies and serial worker flags. This separates the
passing initial source from subsequent concurrent edits; it does not reproduce
the original parallel gate or explain the missing failure log. Receipt:
`/tmp/sync-convergence-20261023/product-gates/volume-initial-snapshot.log`.

The upload and lease foundation passes 322 targeted Git tests (zero failures,
three existing ignored tests), with input hashes unchanged across execution.
New real-Git tests cover pinned origin/mirror objects despite a second commit
while a hook blocks, latest-tip upload primitives, no pull/merge on pinned
rejection, cross-process independent local/publication ownership, aliases,
symlink refusal and push-command overlap/cancellation. These do NOT yet prove
the integrated daemon actor or installed runtime. Receipt:
`/tmp/sync-convergence-20261023/checks/publication-primitives.log`.

The aud-vis different-worktree status refusal is an explicit external checkout
blocker. Rewiring it, resetting retry ledgers, manually pushing live repositories,
rewriting history, changing trust/secrets/ownership or deleting checkouts are
not authorized. Do not label those rows healthy.

## Qualification still required

1. Real blocked-hook fixtures: repository A's first pinned round blocks; B
   commits AND pushes concurrently; a second real commit in A is created while
   its first hook remains blocked; only one A publisher exists; eventual bare
   refs contain A's newest tip at every intended destination.
2. Same-repository/alias/cross-process exclusion, missed-wakeup/coalescing races,
   stale-result accounting, cancellation/watchdog/shutdown/reload ownership,
   mirror failure/degraded/all-paused semantics and truthful fresh/stale phases.
3. Existing workspace tests, release build, warning-denying Clippy and cargo-deny;
   relevant product tests and original applicable pre-push gates without weaker
   assertions or removed guards.
4. Safe qualified installation/restart, installed-binary isolated execution,
   actual loaded revision, and autonomous live commits plus remote advancement.
   No manual live push or repository-state repair is a substitute.
