# Publishing and forge-state follow-up, 2026-11-16

The deployed memory guard recovered a later pressure incident without operator
intervention: it requested recovery at 09:45 after a 09:43 pause, cancelled a
verified read-only probe, and released its pause at 09:47. Fresh automatic
commits and successful syncs continued afterward. Memory limits were unchanged.

## Frozen lockfile

The browser-extensions-shared publishing hook repeatedly failed its frozen Bun
install. Its pinned Bun 1.3.14 was already selected correctly. An immutable
snapshot of the committed workspace metadata reproduced the failure; regenerating
only lockfile metadata added the existing adblock-pro workspace and its link.
The resulting six-line addition changed no dependency versions. The frozen
check then passed with the same pinned Bun.

Before atomically replacing the canonical lockfile, all 70 manifests and the
original lockfile were checked against that snapshot. The daemon committed it
in 77d8d3bf5ae992c7afec391faa113dcf6c108c73. Hooks remained enabled. The
snapshot, before/after hashes, patch and command results are recorded in
`/tmp/dracon-browser-lock-audit-yhh_j8l4/audit-receipt.json`.

## False GitHub incident

The live forge-health file contained synthetic repository names and future
timestamps, with GitHub's incident flag latched. Its modification time predated
the earlier audit test run; this inspection does not establish which historical
writer created it. Pruning only checked the lower time bound, allowing future
hits to survive for weeks and extend GitHub retry pauses from 15 to 60 minutes.

At 13:38:32 UTC the original file was backed up and seven out-of-window hits
were removed. The now-empty GitHub incident was cleared. Backup and receipt:
`~/.local/state/dracon-sync/deployments/forge-state-repair-20261116T133832Z/`.
The repository's stuck retry ledger was then cleared through the supported
repair command after its lockfile cause was fixed.

The source now rejects future timestamps when pruning. It also serializes
load/update/save transactions among daemon workers and publishes JSON through
a temporary file and atomic rename. This prevents lost concurrent evidence and
partial JSON reads. Serialization is process-local, not a cross-process lock.
Three new regressions cover future corroboration, recovery preserving valid
evidence, and concurrent updates with simultaneous JSON readers. All 12 forge
tests pass.

## Qualification status

The release build, workspace Clippy with warnings denied, and `cargo deny check`
passed. Debug and release helpers reported the same compiled source identity:
`c169093474c075de63349483a0edeef615cd5718cc09565c994028903ff5b610`.
The release SHA-256 is
`e0dcd69d0bbb4296e753e0878a843b668d1151e1c81048eda0e98a47ee7df65e`.

The first full workspace run failed only
`integrated_failure_recovery_harness`: its healthy fixture exceeded a staging
deadline. The other 1,722 daemon tests passed, with five ignored. A focused
retry also exceeded the existing epoch deadline during severe host load. The
deadline has not been increased. A focused retry with `TOKIO_WORKER_THREADS=2`
also exceeded the staging deadline. The full rerun's daemon target finished
with 1,720 passing tests, three failures and five ignored tests. Its failures
were:

- `integrated_failure_recovery_harness`: healthy fixture epoch deadline expired.
- `shared_git_runner_deadline_and_drop_terminate_owned_descendant`: the fixture
  observed its PID marker before the marker contained an integer.
- `bootstrap_absolute_staging_deadline_cancels_hardener_descendants`: private
  staging initialization exceeded its deadline.

The host had load averages above 80 and less than 700 MiB available RAM during
focused checks. This is relevant context, not proof of causation or a waiver.
This candidate has not been installed because workspace validation failed.
The previously qualified daemon remains installed. Logs and command results are under
`/tmp/dracon-sync-followup-*.log` and
`/tmp/dracon-sync-followup-validation.json`.
The complete workspace rerun exited 101 at approximately 18:02 UTC with only
the daemon target failing; the other workspace targets, including Warden's
large-file streaming checks, passed.

The first browser hook after the lockfile repair reached 193 passing checks
before returning failure with two advisory diagnostics (de-addict typecheck and
tabmon tests). Its retained manifest is
`~/.local/state/dracon/sync/gates/browser-extensions-shared/dracon-pre-push.xyVMQP/manifest.json`.
Later manifests `dracon-pre-push.1f2bmm` and `dracon-pre-push.FnLfXJ` passed
at 15:21 and 15:23 respectively; subsequent daemon push attempts still failed.
A passing hook alone is not proof that either remote accepted the current tip.
The 17:23:57 GitLab attempt specifically reports Warden's possible-plaintext
secret refusal. No secret values were inspected or reproduced, no security gate
was bypassed, and no published history was rewritten during this follow-up.

## Evening publishing repair

At 22:00 the browser hook still refused publication. A read-only scan of all
319 commits ahead of the observed GitHub tip found one unreviewed match:
commit `5b3299db448ea2a52bd6795cb4af17524f7f4c32` introduced a deterministic
repeated-byte seed in Reddit Pro's `tests/comment-identity.test.ts`. The seed
is used by local author-alias tests and is not a service credential.

An exact review receipt was added to the existing hook mechanism, binding
added-content SHA-256
`a7dbcaf262f1ab03c427d06dd95b6424b323dd6abc21cb6909973a081b5e56be`
to match SHA-256
`9a8fdd7ec276b681d824ea6676faf7297eee66fa9127b185d93a17f9bf658f56`.
The historical content passes with a logged receipt; changing the match or
unrelated added content remains rejected. All 17 hook tests passed both before
and after atomic installation, with executable permissions preserved. No
detector, product gate or history rule changed. The daemon committed the hook
as `c0f540e6c3`. The supported stuck-unstuck command released its retry hold.
Backup and candidate-test receipt:
`~/.local/state/dracon-sync/deployments/browser-secret-review-20261116T220805Z/`.
The installed-hook test log is `/tmp/browser-hook-installed-tests.log`.

The `.dracon` report subsequently observed an actual publication phase instead
of UNKNOWN. Recent automatic commits and syncs establish continued activity,
not a claim that a constantly edited repository stays clean.

A separate transport stall affected multiple GitHub operations: the shared SSH
master accepted control requests but its Git sessions waited for minutes,
while a new connection with multiplexing disabled returned the advertised tip
immediately. At approximately 22:10, `ssh -O exit git@github.com` closed that
shared connection so normal Git retry handling could reconnect. The daemon
service stayed active and repository history was unchanged.
Fresh `.dracon` syncs were observed at 22:10:35, 22:11:45, 22:12:35 and
22:13:01. A read-only replay of the installed secret phase passed both remote
ranges for browser checkpoint `ee8f97ee3f5de158cfbcc117b7fb239983bb73ef`,
against GitHub tip `be47efe4ea99a3d089ef289448163f332b2e8fff` and GitLab tip
`84cc5542be9722a08a9df30459e2a822b4bdab86`. Replay evidence is in
`/tmp/browser-hook-review-5_r2yyen/phase1-both-remotes.json`.
The live product attempt `dracon-pre-push.yGI5eV` completed 45 passing checks
and three advisory test failures (Reddit Pro, Tab Hero and Tabmon), with no
gate-machinery failure. GitHub subsequently accepted checkpoint
`b81e6dbc18648f5c78df0852f88027d5e6b729a9`; an independent non-multiplexed
`ls-remote` verified the advertised ref at 22:22. The daemon's ahead backlog
fell from 324 to 32 while other agents continued editing. GitLab verification
remained pending at that observation; this is not an all-mirror convergence
claim.

## Final mirror result: GitLab storage quota

The next full-scope attempt, `dracon-pre-push.UOuiI5`, completed 193 passing
checks and three advisory failures (AI Web Scraper Pro and de-addict typechecks,
Tabmon tests). GitHub accepted the newer checkpoint
`9ee70a04049378568a64574c7708f466ab9b520e`, including the hook receipt.
Independent `ls-remote` verification is recorded in
`/tmp/browser-publish-remote-verification.json`.

The GitLab leg reused that revision's completed product result in
`dracon-pre-push.0M5MfA`, passed its secret phase with the exact receipt logged,
and uploaded its pack. At 22:48:07 GitLab rejected the checkpoint in its
server-side pre-receive check: the push would exceed the project's allocated
storage. Its advertised main ref remained
`84cc5542be9722a08a9df30459e2a822b4bdab86`.

This is a confirmed storage-capacity blocker, not a remaining local secret
refusal or product gate veto. No quota purchase, mirror removal, security
exception, repository deletion or history rewrite was performed. Clearing the
retry ledger again cannot fix this refusal. All-mirror convergence requires
additional GitLab capacity or an explicit change to the mirror policy.

The daemon also logged this rejection as a missing forge repository and evicted
its forge cache. The independently advertised existing main ref disproves that
diagnosis; this secondary error-classification defect remains to be corrected.
The classifier matches a bare `404` anywhere in its input; the pushed commit
ID `9ee70a04049378568a64574c7708f466ab9b520e` itself contains that sequence.
This is not evidence of an HTTP 404 response.

GitLab's project API independently confirms the repository exists, is not
marked for deletion and accounts for 10,161,969,648 bytes of repository storage
(approximately 10.16 GB). The project-specific `repository_size_limit` field is
null; no exact inherited quota was inferred from that field. The explicit
server refusal establishes the capacity blocker. Installed-hook tests, exact
scan replay, remote observations, storage statistics and the rejection line
are retained with the hook backup under
`~/.local/state/dracon-sync/deployments/browser-secret-review-20261116T220805Z/`.
