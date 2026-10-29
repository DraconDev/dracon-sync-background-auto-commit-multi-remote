# Active systematic fleet-idle repair — checkpoint

## 2026-10-29 — dracon-sync build and Clippy blocker corrected; full tests incomplete

On dracon-sync source `aba148619237`, the debug package build and strict
all-target package Clippy now pass. The build failure was a production call to
`is_build_output_dir_name` hidden behind `cfg(test)`, plus an unused
`auto_pull_merge` service parameter; both are corrected, and
`artifact_component_classifies_build_output_names_for_production_hygiene`
regresses the production path. The strict Clippy run then caught a single-element
test-fixture loop in `dracon-sync/src/git/push.rs`; direct setup calls fixed it.

Three targeted hygiene/excluded-recovery tests pass on the current source,
including the one-exchange recovery result and archive-change concern behavior.
The full serial `cargo test -p dracon-sync` attempt timed out at its 1800-second
limit without a final suite summary; the last emitted alert-threshold test passed
in isolation. This is incomplete evidence, not a passing package gate. Failure
and pass logs plus a machine-readable assessment are retained in
`checks/epoch-integration-partial/current-sync-controls-2026-10-29.json`.
No release build, full-workspace gate, deployment, or loaded-runtime/fleet result
is established by this update. A subsequent read-only `dracon-sync repos` query
also timed out at 30 seconds without output, so no fresh fleet status is claimed;
its bounded context is retained in
`checks/status-inspection-root-cause/current-repos-cli-timeout-2026-10-29.md`.
A subsequent partial journal shows the daemon active and this checkout's
checkpoint commit synced late, but also records the browser-extension backlog
rising 64→73 while publication remained owned by another worker, a late
`dracon-platform` sync failure without a later result in the captured interval,
and continuing trailing-drain work. This is not a clean-idle observation; see
`checks/status-inspection-root-cause/daemon-followup-assessment-2026-10-29.md`.
A direct remote check also found the documented Codeberg destination absent from
this checkout's configured remotes and 4,943 commits behind, while GitHub and
GitLab advertise the current tip. Because the quota design records Codeberg at
its grace limit and defers history cleanup, no reconnect or push was attempted;
Task 5.1 is reopened pending an operator-approved, quota-safe plan. Details are
in `checks/status-inspection-root-cause/dracon-sync-remote-scope-2026-10-29.md`
and its JSON receipt.
A 01:04 UTC follow-up still does not permit an idle claim: host load was
107/96/92 with 641 MiB available and 40 GiB swap used; the browser backlog had
reached 95 unpushed commits while its 23-minute pre-push gate was still active,
and dracon-platform had a 104-entry change-pileup alert. The full status query
was not retried under pressure. `repair stuck-list` showed only temporary test
fixtures, not live convergence. See
`checks/status-inspection-root-cause/task5-live-assessment-2026-10-29.md` and its
raw snapshot. The active goal/task list still names fleet task 5. The on-disk utilities and
platform handoffs describe a Dracon AI slice, not the bucket-strategy text in
the post-compaction message; no scope switch was made. At 01:08 UTC the
browser GitLab push and pre-push gate remained active, with the first two
workspaces' gates passed and the third workspace's tests running; the unpushed
count reached 98. This is evidence of gate progress but not publication. Host
pressure remained high, so no full-fleet query was attempted. See
`checks/status-inspection-root-cause/task5-browser-gate-progress-2026-10-29.md`
and its JSON snapshot. A 01:12 UTC read-only check then found the gate passed,
the daemon-owned GitLab push process had exited, and both unique configured
browser destinations advertised commit `5f35a18ed8fb`. The local browser branch
was 70 commits beyond that published commit, and the daemon reported 73 unpushed
commits by 01:13:50; this proves one publication epoch, not current-tip
convergence. See
`checks/status-inspection-root-cause/task5-browser-publication-receipt-2026-10-29.md`
and its JSON receipt.
A Doomtap follow-up found the repeated 15-minute refusals are attached to a
staged new `docs/screenshots/qa/probe-unapproved.png` addition, absent from the
exact visual-publication allowlist. The hook's exact refusal text is not
preserved; the 1.7 MB file and staged index entry remain untouched pending
operator classification. See
`checks/status-inspection-root-cause/doomtap-unapproved-qa-staging-2026-10-29.md`.

### Follow-up — 2026-10-29 09:10 UTC and later

The pending Doomtap classification is superseded: the daemon later committed the
exact `docs/screenshots/qa/probe-unapproved.png` pointer, and read-only remote
checks showed `github`, `gitlab`, and `origin` `main` refs all at local HEAD
`6e991b6e8a91f2e95e7e1f7b182ab919088587af`. The path-limited status for that
pointer, `.gitignore`, and the publication-policy test is clean. This verifies
that exact publication only; the whole Doomtap tree and fleet were not checked.
See `checks/status-inspection-root-cause/task5-live-assessment-2026-10-29.md`.

The Warden `.gitattributes` resmudge/competing-writer integration control now
passes five sequential targeted runs; read-only checks showed Warden `main`
`aa351b5f6cab65199c4f69c3177641adfa12a5e9` advertised by all three configured
remotes. This verifies source/test publication, not release deployment or loaded
runtime. The actual excluded-recovery helper's
`renameat2` schedule also passes five times with a cooperative Warden writer
queued at the fence; a same-helper raw-uncooperative negative still returns
success while its old writer lands on the displaced inode, so arbitrary-writer
preservation and that timing's parent settlement/refusal remain open. Receipts
are in `../../docs/design/canonical-writer-fence-2026-10-27/PLAN.md` and
`checks/epoch-integration-partial/`.

No full current dracon-sync suite, full-workspace gates, deployment, loaded
runtime, or fresh full-fleet clean-idle observation is established. The earlier
scheduler snapshot showed browser publication and platform inspection/retry
work active; do not treat the Doomtap remote check as fleet convergence. A later
09:31 read-only check found utilities `dracon-sync` main
`739c4492dfd9281d5c42d0dfff1380476a4768ce` advertised by all three configured
remotes, while Codeberg remains unconfigured. A subsequent 09:32 check showed
the evidence-only utilities commit `7c5c18f974da30d2b8b7937cff51694cfd704838`
on all three configured remotes. The 09:33 read-only snapshot showed utilities `main`
`39e4b1eac983b50885421787d021f5d8890a6950` on `gitlab` and `origin` (whose host
is GitHub). The separate `github` alias seen at 09:32 was gone by then; the
reason is unknown, but GitHub remains covered through `origin`. The daemon
journal still showed browser backlog 114 and trailing-drain activity; no
full-fleet query was run. Later, GitLab declined utilities evidence commit
`781ce70d` at 09:35:38 with a generic pre-receive refusal; the daemon scheduled
its own retry, and at 09:43:58 read-only checks showed `gitlab` and `origin`
(`github.com`) both at that commit. The exact server-hook cause remains unknown.
At the 09:43 scheduler snapshot, browser publication and platform retry/Polis
work were active, with 73 browser commits unpushed; this still is not a clean-
idle fleet observation. At 09:47, browser `main` was 85 commits ahead of the
matching `gitlab`/`origin` refs; a contemporaneous process snapshot showed the
pre-push/Node/Git gate tree actively executing, but no ref advance in that
snapshot. The 09:47:35 `origin` gate then failed on `clean-web TEST` because
snapshot `ac036eb` had a duplicate brace in `engine.test.ts`; later committed
fix `9df0bc` is an ancestor of current local tip `e3bc4fb`, but no gate result
on that corrected tip is available. By 09:54, the same old browser snapshot had failed `clean-web TEST` again;
`web-time-tracker-limiter` test/lint passed and `youtube-filter` typecheck was
running. Browser local main was `75809018ffcc1c308923884ea01336fb34377833`,
89 commits ahead of `gitlab`/`origin` at `64c46f5c9b7d0b6683eb7f13ed74271fd97bbf4a`;
committed syntax fix `9df0bc` is included in local main, but the corrected tip
has not yet passed a push gate. At 10:07:48, dracon-sync local `main`
`54675a5173d8ffb058de3d4b542b1259d643d524` matched both `gitlab` and `origin`
(GitHub); no separate `github` alias was configured, and Codeberg remains
unconfigured. The old browser snapshot failed its gate at 09:54:47. After the daemon's normal
retry, attempt `IPQglO` passed every workspace gate at 10:07:36 on corrected
snapshot `9c665ef1349198136f88ede60bfb6f49c56cb9bf`, including `clean-web` and
`youtube-filter`. The receipt tests accepted the real committed evidence; it
still states the latest 12-round run saw no targets and relies on an earlier
same-feed sighting/stamp for removal evidence. At 10:07:48, GitHub `origin` had
advanced to `9c665...`, while GitLab remained at
`64c46f5c9b7d0b6683eb7f13ed74271fd97bbf4a`; local browser main was
`8afc24db5e125acfdc2dcc0088412d63b819ecc5` and the daemon still reported a
push in flight. Newer local commits are beyond the qualified snapshot, so this
is partial publication rather than convergence. At 10:12:23, local browser main
was `184fc508fc92621fe175357845196273b60d0724` (95 commits ahead of GitLab),
`origin` remained at `9c665...`, and GitLab at `64c46...`. The second pre-push
attempt `E34dYX` is for the `9c665...` / `64c46...` GitLab tuple; its seven
dependency builds passed, provisioning finished, and `affiliate` CI was running.
The current tip is newer than that snapshot and will still need publication.
At 10:14:12, `affiliate` CI and all `ai-tts-pro` gates had passed; `clean-web`
typecheck/build passed and its test was running. Later workspace gates were not
yet completed in this attempt. At 09:58:21 the daemon also reported a
parent-platform `update-ref` failure on an existing `main.lock`; read-only checks
showed the zero-byte lock was over seven hours old with no holder. A guarded
`dracon-sync maintenance` repair removed only that stale lock and resumed sync.
The parent later reached five consecutive failures and a 15-minute backoff. At
10:06:18 its post-commit hook failed after staged-epoch publication with
`STALE: products, games, music, studio, tools, books, chat, jobs`; the daemon
then committed additional parent files, but no advertised parent ref result was
verified. At 10:07:48 the scheduler still showed parent retrying. At 10:12:23 it showed
dracon-platform in a push phase with a retry timer, Polis retrying, and
endless-td committing. These remain active/incomplete states, not a clean-idle snapshot. Browser gate
`E34dYX` passed on `9c665...` and GitLab advanced there; `dnIA3x` passed on
`70545...` and GitHub `origin` advanced there. The next `70545...` / `9c665...`
attempt `pv4dtY` was still running at 10:22:31. Those gate snapshots predate the
new local verdict fix. Review found that a missing switch-on sample could be
misread as a successful A/B return; daemon commit `db959118d2` now requires both
intact A/B feed samples and reports unreadable comparisons honestly. Its focused
verdict tests passed 12/12, and package typecheck/lint passed, but no full gate
covers the current tip yet. At 10:22:21 browser local main was `db959118d2`,
`origin` `70545...`, and GitLab `9c665...`; the current tip remains unpublished.
The latest live receipt was not regenerated. At 10:30:36, both browser remotes
were at `70545...`, while local `main` was `71b22...` (22 commits ahead of
both advertised refs; the local `gitlab/main` tracking ref was stale at
`9c665...`). Gate attempt `C4gRIL` was still running at 10:31:43 on older
snapshot `1526bb...`; it does not verify the current tip. The current verdict fix
requires same-tab/same-URL feed samples, and its 14 focused tests, package
typecheck/lint, and receipt-evidence check passed locally. The current tip has
no full pre-push result yet. The `dracon-sync` evidence updates are now published
at `487a31...` on both `origin` and `gitlab`. `dracon-platform` and `polis` were
still active/retrying in the scheduler. No full-fleet clean-idle observation is
established.

### Follow-up — 2026-10-29 10:37 UTC: stale-snapshot gate and live survey hunt

Gate `C4gRIL` failed at 10:34:40 on frozen snapshot `1526bb...`: YouTube
`TYPECHECK` reported eight missing-`abSameTab` declaration errors; its build,
full package tests and lint passed. The later local declaration commit `b3a313...`
adds `url` and `abSameTab`, and current local typecheck passes, but there is no
full gate result on the current tip. Two more manifests were still running on
the same stale `1526...` tuple at 10:37:29, with no workspace results recorded.
Direct remote checks still showed `origin` and GitLab at `70545...`; local main
`3bca32...` was 27 commits ahead of both advertised refs. The local GitLab
tracking ref was stale at `9c665...`, so its derived ahead count is not a remote
state.

The independent patient survey hunt logged ten intact Home-feed rounds
(53–76 videos, same tab, all targets absent) and began its next batch. This
confirms only that no survey was served in that sample, not that the extension
removed a rendered target. The separate real-feed shelves receipt was not
regenerated; the earlier same-feed target sighting remains the positive evidence.
The browser publisher and Polis inspection/retry remained active, so fleet-idle
acceptance is still open.

### Follow-up — 2026-10-29 10:41 UTC: parent ref check and current blockers

The parent `dracon-platform` ref-lock incident is resolved at the ref level:
local `main` and all three advertised `github`, `gitlab`, and `origin` `main`
refs were `f44b86...`; `.git/refs/heads/main.lock` was absent. The journal
contains the original 09:58 `update-ref` failure but no recurrence. This does
not requalify the separate 10:06 post-commit `STALE` hook result, and the parent
still had active work.

At 10:40:23, nested `polis` again refused a staged epoch and retained
`dracon-epoch-message-lEjYhM`. That directory contains only the generated
`COMMIT_EDITMSG`; the journal does not identify which commit hook refused or
preserve its stderr. `run_sealed_commit_hooks` invokes `pre-commit`,
`prepare-commit-msg`, and `commit-msg`, then logs only the retained-message
path. Keep the cause unresolved and do not treat the retry as healthy.

The browser tip had advanced to `80e0bd...` by 10:41:11 (31 commits ahead of
each advertised `70545...` ref). The stale-snapshot retries still failed
`TYPECHECK` for missing `abSameTab`; one had passed build/tests/lint and another
was still in tests. Current local typecheck passes, but no current-tip full gate
or publication is recorded. The survey watcher completed another ten-round
intact-feed batch with no survey target and began batch 173; this remains
absence, not removal evidence. Browser publication, Polis retry, and platform
work remained active, so no clean-idle fleet result is established.

### Follow-up — 2026-10-29 10:46 UTC: stale gate retries and continued no-target sampling

At 10:45:55, browser local `main` was `a3996a9380244d03815b3755e8148d87c70c2cd2`, 42 commits ahead of both directly advertised refs at `70545...`. The local `gitlab/main` tracking ref is stale and must not be used for advertised-remote counts. All three attempts on snapshot `1526bb...` (`C4gRIL`, `KGGFs6`, `JPCxS8`) failed the same YouTube typecheck because the snapshot omitted `abSameTab`; their build, full test and lint gates passed. The corrected local declaration typechecks, but no full gate or publication covers the current tip.

The patient survey watcher continues to emit tracked evidence. Completed batch 174 had ten intact Home-feed rounds with no survey target; batch 175 was in progress at the last check. These remain absence observations, not removal proof. The saved real-feed shelves receipt was not refreshed. Parent refs were last verified equal at 10:41:11 (`f44b86...` on local and all three remotes, no `main.lock`); Polis still had an unclassified hook refusal at 10:40:23. No current full-fleet clean-idle snapshot is established.

### Follow-up — 2026-10-29 10:50 UTC: corrected-code gate in progress

At 10:48:19, browser `main` was `5ca6d68c5ef32de269d88f8d57e87fc922f86669`, 51 commits ahead of both directly advertised refs at `70545...`. The new `V7InRp` gate started at 10:46:49 on snapshot `fbc42b18fdbb309b57183a7328db10c4f610cbf5`, seven commits behind that local tip; the verdict source, declaration and tests are unchanged between those revisions. This snapshot includes the corrected `abSameTab` declaration. At 10:50:31, its clean-web gates and YouTube typecheck had passed; YouTube build was still running, with no full gate result or publication yet.

The passive survey watcher remained alive. Batch 176 had five of ten rounds recorded, all absent on visible Home-feed grids; this does not show filtering. The local branch is still unpublished and continues accumulating evidence commits. The new gate is useful progress on the corrected code, but does not establish current-tip publication or fleet idleness.

### Follow-up — 2026-10-29 11:07 UTC: corrected code published; test harness repaired locally

Browser gates `V7InRp` and `0eJuTA` passed all eight workspace gates on `fbc42b18...`; direct `origin` and `gitlab` refs reached that snapshot. At 11:02:55 local `main` was `109a932b...`, 38 commits ahead of both refs. A later gate on snapshot `7e8eff49...` failed one test: `survey-hunt-recovery.test.ts` expected two fake-Node calls but got twelve because the test put a fake `node` on `PATH`, intercepting the watcher's real JSON helper invocations too. The test now injects a fake hunter script through `YFF_SURVEY_HUNT_SCRIPT` and uses a temporary sightings file instead. The daemon committed this test-only fix as `af5a9b1a...`; local package verification passed: 61 files / 1081 tests, typecheck, and lint (with a non-fatal Node module-type warning). At 11:06:41 the local tip was 39 commits ahead of both advertised refs at `fbc42...`; no gate yet qualifies the test fix. The old-snapshot attempts `B4CLx7` and `n3Jg5x` were still running then, and do not qualify `af5a`.

The operator chose to stop the passive watcher after batch 176. The stop was not applied promptly: batches 177 and 178 also completed, then batch 179 began. The watcher was terminated at 10:58:57 after three more absent observations (video-item counts 78, 58, 77; each showed a visible Home grid). The latest complete tracked `survey-hunt.json` is batch 178: ten absent observations from 10:55:17 to 10:57:48, all grids visible, 56–74 video items. The partial batch-179 log remains at `/tmp/yff-feedback-survey-watcher/batch-179-20261029T105803Z.log`; it is not a complete receipt. The watcher is stopped; no editor, agent loop, or daemon was stopped. No fresh real-feed shelves receipt or positive removal proof was obtained.

At 11:01:23, parent local `main` (`a33daf...`) was one commit ahead of directly verified `origin` and `gitlab` refs (`63d8e3...`), with no `main.lock`; the working tree still had active nested and outpost-eden changes. Polis's 10:55:30 epoch refusal (`0KLVtZ`) remained unclassified after five failures/backoff; a later filter-aware status query succeeded, but did not explain the refusing hook. Full-fleet clean-idle acceptance remains open.

### Follow-up — 2026-10-29 11:10 UTC: current daemon state and remaining stale gate

The latest `dracon-sync repos` view (about 11:09) reported 36 inspected repositories: 32 clean, 3 active, 1 blocked, and 0 inspection failures. It showed `dracon-platform` blocked, browser publishing, and Polis queued. `repair stuck-list` returned no stuck repositories. The dry-run `repair concerns` separately found one existing `terhub` concern (clean and equal to origin but lacking upstream); no repair was applied.

At 11:09:47, parent local `main` was `b1955355...`, two commits ahead of directly checked `origin` and `gitlab` refs at `7ee1bd29...`; `main.lock` was absent. The parent still had active nested and outpost-eden work. The browser local tip remained `af5a9b1a...`, 39 commits ahead of both remotes at `fbc42b18...`. `V7InRp`/`0eJuTA` passed the corrected declaration on `fbc42`; the later test failure on `7e8eff` was the fake-Node test-harness issue described above. The test fix is locally committed and package checks pass, but it has no qualifying gate or remote publication yet. `n3Jg5x` failed on the same pre-fix snapshot; `B4CLx7` was still running on that snapshot at the last status read.

## 2026-10-28 — current handoff after reviewer and evidence reconciliation

The original goal remains active and incomplete. Warden's cooperative canonical-
metadata controls now include a bounded `resmudge --apply` fence-wait regression
and an end-to-end encrypted tracked-`.gitignore` apply/cancellation control. Five
sequential targeted runs passed on dracon-warden `88d218482eeb`; the Warden/
security package suite reports 481 passed, 0 failed, 6 ignored, strict
package/all-target Clippy passes, and `cargo deny check` is clean. The first
fixture attempt did not match a scanner rule and is retained with its corrected
passing results. One earlier intentional two-thread probe failed in a test that
mutates process-global PATH; `.cargo/config.toml` requires serial Rust tests,
and the standalone plus complete serial runs passed. Full-workspace gates,
deployment and loaded-runtime proof remain outstanding; see
`../../docs/design/canonical-writer-fence-2026-10-27/PLAN.md` and
`../../docs/design/canonical-writer-fence-2026-10-27/warden-writer-controls-2026-10-28.json`
for scope and raw outcomes.

The temporary directory for a settled hygiene-review run was absent, but its
saved reviewer report was recovered and preserved under
`checks/final-assembly-2026-10-28/`. A later independent review artifact reports
a P1 on its inspected helper: second-exchange rollback moved a newly opened
writer's inode to private storage, and it returned BLOCK. That exact rollback
branch is absent from the current one-exchange helper. However, the separate
current raw-opener syscall receipt still returns `pass=false`: a queued old
writer remains on the displaced inode while the helper reports success. Five
cooperative Warden queue passes do not qualify that raw-opener schedule. Preserve
the raw-opener failure as an unresolved canonical-preservation gap; the proposed
in-place `.gitignore` rewrite remains unaccepted because interruption can expose
partial security rules. See the detailed historical evidence below.

The operator's junk-runner tracking-policy choice was executed; do not treat the
old pending-decision and pre-decision idle snapshot below as current. Their
superseding notice points to `checks/final-assembly-2026-10-28/` evidence. The
latest structural assessment says junk-runner converged and dracon-platform
remains dirty through quarantined hegemon content. Task 5.2 is still open; no
fresh final full-fleet clean-idle observation is claimed. The separate plaintext
publication-safety repair remains queued, not completed by this checkpoint.

## 2026-10-27 — pre-push fail-closed repair was being silently reverted; fixed and deployed

While re-verifying the earlier fail-closed pre-push repair, the LIVE global hook
was found back in the fail-OPEN state, and it flipped between variants
(`d122d8c1…` fail-closed ↔ `1eaadcf3…` fail-open). Two writers, both real:

1. The installed `~/.local/bin/dracon-warden` (`39db331b…`, 09:58) predated the
   fail-closed template, and the daemon calls it on EVERY pass —
   `sweep_warden_autoharden` runs `dracon-warden once` (`dracon-sync/src/sync.rs:1107`),
   which reaches `refresh_global_hooks_if_stale()` (`dracon-warden/src/main.rs:2151`).
   Its own `setup-hooks`/`once` output reproduces `fail_open_reproduced: true`.
2. The workspace test suite wrote the live hook too: `integration_test` ran
   `once`/`repair` with the inherited HOME, and a 34-second green run changed the
   operator's `pre-push`. Raw pre-fix observation retained in
   `dracon-warden/docs/design/prepush-failclosed-deployment-2026-10-27/test-isolation-negative-control.md`.

Root cause is a missing representation, not a Git or network fault: the hook
marker was version-less (`-v1`), and refresh compared only "is this ours" +
"does the text differ", so the OLDER template looked like a legitimate drift
repair. Both variants are genuinely warden-managed.

Fixed: `WARDEN_HOOK_GENERATION` (2) is stamped into every rendered template via
`WARDEN_HOOK_MARKER_PREFIX`, and `refresh_warden_hook_if_stale` now REFUSES with
an error — never silently succeeds — when the hook on disk is newer than the
running binary, leaving its bytes intact. Generation is parsed by prefix
regex, so a future warden's hook is still recognized as warden-owned and still
compared, and a comment merely mentioning the marker is not a marker. The
in-hook chain check (which skipped running warden-local hooks twice) was
exact-match v1 and is now `^# dracon-warden-managed-hook-v[0-9]+$`; without that
change a v2 local hook would have been chained and executed twice per push.
Test isolation: every hook-refreshing CLI test now runs with a private HOME,
`test_warden_keygen` no longer mutates process env, and
`hook_refreshing_commands_never_touch_the_operator_home` asserts the live hook
is byte-identical across a real `once`+`repair` while proving the refresh landed
in the fixture HOME (so it cannot pass vacuously).

All four workspace gates pass on one committed assembly: tests 44 suites /
2888 passed / 0 failed, release build OK, strict all-target Clippy clean, deny
clean (`dracon-warden/docs/design/prepush-failclosed-deployment-2026-10-27/final-gates.json`
plus raw logs). Deployed via `dracon-sync maintenance` (never a bare stop);
installed binary now `b31f59ad…f9124` = the gated release build, previous
binary/policy/hook backed up under
`~/.local/state/dracon/rollback-binaries/warden-20261027T200325Z`.

Live verification: global `pre-push` is `cc7caa7b…` (marker v2, all 7
fail-closed checks). Running the daemon's exact auto-harden path leaves it
byte-identical, it stayed `cc7caa7b…` across live daemon cycles, and both
installed and qualified binaries generate that same hook. The discretionary
control against the LIVE hook refuses the synthetic secret, passes a harmless
change, refuses the simulated scan failure, `fail_open_reproduced: false`.

This is a security-regression repair, NOT convergence progress. Publication is
still contained by the intentional `auto_push = false`; task 2 staging /
inspection work is unfinished; task 5 (autonomous publication + all-clean idle)
remains unmet. No manual commit/push, producer stop, hook bypass or policy
weakening was used.

## 2026-10-27 — actual long-abort defect reproduced and repaired; polling stopped

Repeated fleet-only checkpoints were no progress: publication had deliberately
been held pending safeguards, and original tasks 2–4 were still unfinished.
Returned to those repairs rather than treating task 5 as a wait for idle.

CORRECTION to the earlier ordinary-abort paragraph below: that control released
I/O immediately, before escalation. The SAME full-content assertion with the
write held 6.5 seconds after abort FAILED with an EMPTY file. Exact old inputs
and retained failure: `../../docs/design/canonical-writer-fence-2026-10-27/ordinary-long-abort-negative.json`
and `.log`. The earlier short-abort success is historical, not complete safety.

Repaired lifetime, not grace duration: Pi registration now runs the entire host
SDK transaction IN PROCESS with the v1 flock held until pending I/O settles.
A small Linux N-API bridge provides nonblocking flock/CLOEXEC duplicate; owned
namespace/mode/link checks and abortable absolute acquisition deadline are in
JS. Ordinary writes are no longer in a force-killed child. Legacy bounded
worker now REFUSES ordinary mutation before truncation; metadata still uses
private atomic publication with directory-FD cleanup anchoring. No detached
future writer is introduced. Queue and fence release only after SDK settlement.

Actual-loader controls exercise short and >5-second abort, retaining the
original full-content/inode/mode assertions. Actual Warden stays excluded while
I/O is pending; another repo publishes in the SAME Pi process before that write
is released. Native peer/OFD reentrancy/cross-root refusal, bounded waits and
zero waiter-FD leakage, unsafe/replaced namespaces, native schemas/path/edit
compatibility, BOM/CRLF/Unicode/diff and ambiguous refusal all pass. Readonly
access cannot be bypassed by atomic rename. Cold umask comparison also first
FAILED (0644 vs native 0600), then passed after honoring native umask; its failed
log remains. Discovery/external-guard errors now increment refusal counters,
which explicitly do not imply unchanged content after ordinary cancellation.

29 controls pass with zero failures/skips on unchanged source/SDK/Node/binding/
header/native-peer inputs: `../../docs/design/canonical-writer-fence-2026-10-27/pi-inprocess-controls-v3.json`.
Native build log: `pi-inprocess-build.log` in that design directory. Binding was
built only in the qualification target. No live-session settings, installed
Warden/daemon binaries, publication policy or count-cap/index migration changed
by this phase; no manual commit/push or producer stop.

Remaining work is NOT waived: shell/user-bash/native-Git/editor interfaces,
parent/process-death and other I/O/deadline schedules, metadata ACL/xattr and
directory durability, linked-worktree/device and large-input qualification,
full coherent workspace/product gates, then actual installed/loaded runtime
and autonomous publication/full-fleet idle proof. Acquisition waits are bounded;
this does NOT claim a hard bound for kernel I/O that never returns. See current
prototype README and PLAN. Continue those prerequisites, not fleet-only polling.

## 2026-10-27 16:52 UTC — fresh fleet snapshot: not idle by design, gates deferred on load

Read-only `repos --json` saved to `checks/20261027-fleet-snapshot/repos.json`:
36 repos, 17 ok / 6 active / 13 warn / 0 concern / 0 inspection failures at
capture; recount minutes earlier showed 18/7/11 and 17/8/11 — fleet is actively
committing, not idle. 17 repos AHEAD with push PENDING (e.g. freeport 28,
.dracon 100, dracon-sync 23, pi-plugins 42, deathrun 50, doomtap 17) plus
browser-extensions-shared 1314→3031 unpushed in commit-only mode. Cause is the
intentional persistent containment `auto_push=false` in the global policy, not
a new push regression; daemon journal shows continuous `sync_commit` plus
`push paused (commit-only mode)` and trailing-drain deferrals. One
`junk-runner: staged intent exceeds strict batch limit` and one empty
classification on a dirty worktree were observed; both are diagnostic signals,
not cleared state. Full `cargo test --workspace --locked` was attempted and
timed out at 180s under load 66.75 / mem 27GB+21GB swap; no gate outcome is
claimed. Task 5 remains unfinished: no autonomous publication proof and no
all-clean idle observation while the hold is active and editing continues.

## 2026-10-27 — ordinary in-place abort parity qualified in disposable controls

A gated ordinary write (paused after open/truncate, before content bytes) plus
client abort now settles exactly like the native SDK: the held write finishes
inside the TERM->KILL grace, the call reports cancelled/aborted, the file holds
the FULL new bytes with the same inode/mode (not truncated partial, not silent
original), and the per-worktree fence is immediately reacquirable. No metadata
scratch is used. All nineteen controls pass; source/SDK/Warden identities agree
before/after: `../../docs/design/canonical-writer-fence-2026-10-27/pi-ordinary-cancel-controls.json`.
This closes the explicit ordinary-cancel deployment prerequisite for cooperative
aborts. SIGKILL mid-write, timeout kills, parent-death, large inputs, remaining
producer coverage, ACL/xattr, full assembly and live loaded proof remain open
blockers. No live session/settings/binary deployment, publication re-enablement
or manual recovery occurred.

## 2026-10-27 — typed worktree alias bypass closed in source controls

Replaced basename-only routing: EVERY typed edit/write in a discovered worktree
now acquires the shared fence before the worker resolves the target. Existing
same-repo aliases to metadata use atomic canonical publication while retaining
the alias. Cross-repo/unowned/shared-hardlink mutations refuse instead of using
another repository's fence or an ordinary unsafe fallback. Parent/Git-marker
discovery distinguishes no repository from broken/unreadable Git metadata; the
latter is always an error. External ordinary files retain native in-place
behavior with no-follow descriptor/ownership/link checks before truncation.

All eighteen disposable controls pass, zero failures/skips. Added actual-loader
checks cover same-repo alias write/edit, cross-repo refusal, ordinary inode/mode
fidelity, external plain files, outside link refusal and corrupt ownership
metadata. Existing cancellation/ENOSPC controls still pass. The cancellation
probe now pauses on real scratch readiness, verifies its file FD is closed,
and no longer depends on a brittle count of discovery subprocesses. Source,
compiled SDK and native Warden identities agree before/after execution:
`../../docs/design/canonical-writer-fence-2026-10-27/pi-alias-controls.json`.

IMPORTANT NEXT CONTROL: ordinary (non-metadata) in-place writes now run in the
owned child too. Qualify a cancellation/forced-kill AFTER truncation/write has
begun; do not infer preservation from atomic metadata tests or enable this
integration until any failure is fixed. Settle-before-abort behavior must not
silently weaken. Remaining shell/user-bash/native Git/editor coverage, other
kill/timeout/error schedules, metadata fidelity/durability, final whole-workspace
qualification and actual loaded/live fleet proof still remain. Prototype limits
and unsupported cases are explicit in README and PLAN.

No live session/settings or binary deployment, publication re-enablement,
count-cap/index migration, manual recovery commit/push or producer stop occurred.
The original goal/task acceptance and queued security follow-up are unchanged.

## 2026-10-27 — real Pi registration and actual cancellation/I/O controls pass

Supersedes the prototype's earlier missing-registration status below.
`pi-canonical-write-fence/index.js` now registers the native edit/write definitions,
retains schemas and legacy edit preparation, uses the caller's dynamic cwd and
normalizes paths with the actual host resolver. Non-metadata tool behavior stays
native; direct named metadata runs the complete transaction in the fenced worker.
Its status command explicitly reports that shell/aliases/editors are NOT covered.
The actual Pi resource loader loaded and executed this registration only in a
disposable fixture. No live settings/session was changed or claimed loaded.

All thirteen controls pass with zero skipped/failed. An actual family schedule
pauses after scratch write/fsync/close but before canonical rename: a second repo
publishes while the first lock is held; cancellation preserves the first file's
inode/content, clears scratch, exits the blocked child and allows a fresh native
participant to reacquire. A scoped native interposer returns ENOSPC on actual
private scratch write/writev and fsync calls. Both failure stages refuse, preserve
canonical bytes/inode, clear owned scratch and release the fence. Host disk was
not filled; this does NOT establish the cause of the historical plaintext leak.

The fresh receipt hashes prototype JS/tests, the C interposer, the exact Warden
binary and compiled host SDK before/after; all remain unchanged during the run:
`../../docs/design/canonical-writer-fence-2026-10-27/pi-io-controls.json`.
Earlier failures and component receipts remain retained. No binaries were
installed, no count cap/index migration or real-repository manual publication
ran, and no publication hold was removed.

Remaining implementation still includes alias and bash/user-bash/native Git/
external editor coverage, other parent-death/kill/timeout/error schedules,
metadata fidelity/durability, final whole-workspace assembly and actual loaded
operator-session/runtime proof. Do NOT enable source integration or attempt
fleet acceptance merely because these scoped fixture controls pass. Current
scope/limitations are in the prototype README and updated PLAN; original task
acceptance and the queued security item are unchanged.

## 2026-10-27 — Pi metadata worker prototype qualified in disposable controls

Implemented `worker.js`, `atomic.js` and ten controls in the existing
`/home/dracon/Dev/pi-plugins/extensions/pi-canonical-write-fence/` prototype.
The worker validates the actual inherited locked descriptor against its repo
before target lookup or SDK mutation-queue registration, then reuses the host's
complete edit/write transaction. Real fenced SDK controls preserve BOM, CRLF,
Unicode, diff results and ambiguous-edit refusal. Atomic scratch is private to
Git's admin directory, bounded by bytes/inodes and never eligible worktree data.
A pre-publication cancellation preserves original canonical bytes and inode.

A new namespace-substitution regression caught unsafe pathname-based cleanup
in the first prototype. It initially FAILED by deleting a foreign replacement
entry. Fixed cleanup/publication anchoring to a held private-directory FD and
owned temporary identity. All ten controls now pass with zero skips; the failed
control remains failed in its original raw log. Source hashes before/after the
passing run match. The Warden used was built into the separate qualification
target, not installed. Exact native/source identities and raw outcomes:
`../../docs/design/canonical-writer-fence-2026-10-27/pi-worker-controls-v2.json`.

NOT registered or deployed: no `index.js` yet, no live session picks this up,
no all-writer guarantee and no full-workspace qualification. Next implement and
qualify the extension registration while preserving native schemas/path/context
semantics; close unsupported alias/shell/Git/editor coverage and actual process
family/I/O-fault gaps before enabling anything. ACL/xattr and directory-durability
limitations are explicit in the prototype README and updated PLAN. No policy
hold was removed, no count-cap migration or real-repo manual publication ran.
The active goal and tasks 2–5 remain unfinished; the security follow-up remains
queued. This is concrete implementation progress, not clean-idle acceptance.

## 2026-10-27 15:40 UTC — resumed original goal; daemon fence controls pass

Re-read the active GLLA objective and task list: fleet convergence remains the
objective, not the stale bucket handoff. Task 5 cannot yet be accepted; tasks
2–4 still have explicitly unfinished work. The separately requested plaintext
publication repair remains queued; it has not replaced or narrowed this goal.

Persistent containment `auto_push=false` is still configured and the latest
health observation reports the running daemon frozen. Do not restore publication
for a fleet proof before the publication safeguards are qualified. This is not
an external permission blocker or a reason to label the fleet converged.

The daemon generated-ignore/excluded-restore source already contains the shared
pre-open fence integration. Added two regressions proving outer cancellation
leaves no stranded waiter and an actual inherited-descriptor child retains the
fence after its parent's handle drops, until final close. All five daemon fence
controls and all four generated-ignore controls pass, including recovery after
an accepted writer finishes, original active-writer refusal, linked-target
refusal and stale-render refusal. Strict daemon package/all-target Clippy passes.
Source hashes before/after agree. The dedicated qualification target did NOT
overwrite the live `target/debug/dracon-sync` helper. Raw receipts and updated
inventory: `../../docs/design/canonical-writer-fence-2026-10-27/` relative to
this checkpoint's directory.

No deployment, count-cap migration, index reconciliation, manual commit/push or
producer stop occurred. Pi tool workers/registration, shell/native-Git/editor
writer coverage, adversarial all-writer controls, whole-workspace final assembly,
loaded-runtime and full-fleet publication/clean-idle acceptance remain open.
The old background dependency's `/tmp/.../05c0febe-e572-42bd-90f8-3a8934349efa`
artifact is missing; its completion notice is not accepted as fresh proof and
was not blindly relaunched. Current controls have independent durable receipts.

## Active: operator-authorized cooperative writer integration (not deployed)

User authorized external .gitignore-writer integrations with a bounded pre-open
fence held through final close, retaining all existing guarantees. The previous
permission blocker is cleared; implementation is active, not deferred.

Warden source now owns a versioned per-worktree flock protocol and a write-fence
CLI; native harden_repo holds it around the entire metadata read/render/write,
including once/repair. Five primitive and four real CLI controls pass, as do the
full Warden suite and strict package/all-target Clippy. FD-lifetime control proves
killing only the wrapper does not prematurely unlock a still-live writer. This
is NOT full crash-family supervision or all-writer coverage.

Nothing in this phase is deployed. Daemon helper integration, Pi edit/write
integration, shell/Git/editor coverage, coherent whole-workspace qualification
and loaded execution remain unfinished. Task 4.1 is reopened for the new source.
The old raw-writer control remains failed; no displaced-writer condition was
accepted, no count-cap migration applied, no producer stopped. Exact design,
coverage gaps, source inventory and raw tests:
`dracon-sync/docs/design/canonical-writer-fence-2026-10-27/PLAN.md` (utilities parent).
Continue that implementation rather than returning to the already-answered
permission question or treating the primitive as a complete protocol.

## Latest: four newly approved caches repaired; current loaded writer control fails

User approved only arena-ward.png, arena-wound.png, defeat-after.png and
defeat-before.png under Doomtap docs/screenshots/qa/. Added exact ignore rules
and expanded the original publication-policy test without changing its curated
source contract. All three tests pass. Index-only removal ran under the
sanctioned maintenance wrapper: all four staged variants differed from local
bytes, BOTH variants were preserved privately, local bytes were unchanged and
every unrelated index entry matched exactly. The daemon automatically resumed.
Doomtap's unchanged original pre-commit then passes on an otherwise unmodified
copied index in 3.73s. No manual commit/push or broader exclusion was applied.
Receipts: checks/publication-resync-2026-10-27/doomtap-four-approved-{deindex,original-precommit}.json.

Re-ran the ORIGINAL restored canonical-writer syscall control against the actual
installed daemon (4ddef737a3fe0d42bfece133722bf48d36b2fa482c14ce01ffc15fc44fba2f9e).
Baseline succeeds; the adversarial schedule reaches the actual exchange without
errors and FAILS preservation: old writer canonical=false, newer writer
canonical=true, index preserved=true. See current-loaded-canonical-writer-control.json.
This is a current-artifact failure, not an inference from older releases. No
acceptance condition was inverted. Producer coordination remains an external
permission prerequisite documented in canonical-publication-boundary-2026-10-26.md.

Bounded remaining-work assessment: the configured count cap is still 100;
junk-runner has 127 staged paths and ai-auto-writer has 205. Their index bytes
were not changed. The previously approved zero-count migration is NOT applied
because its preservation prerequisites are unmet. Browser's current real-feed
gate still fails with one recorded failure: the control feed never displayed
the targets. No gate/receipt/assertion was changed to make it pass.

Latest recorded complete inventory: 36 watched, 29 summary-OK, 5 active, 2 warned,
zero inspection failures. This is not clean-idle acceptance or all-remote proof.
All static workspace gates remain qualified; canonical-writer/runtime and
whole-fleet acceptance remain unfinished.

## Latest: platform encryption blocker repaired without changing inclusion

The current original-hook reassessment found three non-whole-file-encrypted
platform `.env.dev` staged blobs, despite filter=dracon. DECIDE 4 had removed
credential patterns from protected_patterns to stop ignore negations; the filter
then silently lost its encryption scope despite retained manual attributes.
Warden now supports `encryption_only_patterns`: identical encryption treatment,
NO ignore negations, empty default, explicit plaintext-conflict validation and
consistent size/repair gates. Only the exact auth-api/billing-api/email-api
`.env.dev` paths are opted in. No .gitignore/.gitattributes or plaintext-hatch
change was made. Managed .env header normalization also now reuses authenticated
ciphertext without hiding body/whitespace edits.

All four FULL workspace gates pass on unchanged source inventories after this
repair (`dracon-warden/docs/design/encryption-only-scope-2026-10-27/v2-workspace-gates.json`,
relative to the utilities parent). Qualified Warden 39db331b735b4c71a25e27e2d7c90cf171301406c3b1ca786f5ea207f0fed370
is installed with retained binary/policy backups. The daemon independently
re-staged all three paths as whole-file tags. The unchanged original pre-commit
now passes on an otherwise unaltered copied index in 7.8s. Staged environment
bodies are verified preserved; only the existing Warden version header changes.
No secret values were output, no manual staging/commit/push, no hook bypass.
Platform is publishing (three ahead in the last report), not proven converged.

Doomtap's previously approved fifth cache is removed from the index with local
and original staged variants preserved. Its current original hook instead names
four NEW staged frames: arena-ward.png, arena-wound.png, defeat-after.png and
defeat-before.png under docs/screenshots/qa/. They have NOT been ignored or
removed: previous exact-file approvals do not classify them.

Fresh recorded fleet: 36 watched, 29 summary-OK, 6 active, 1 warn, zero inspection
failures. Browser's real-feed evidence gate remains failed; strict-count-cap
migration is still pending its preservation preconditions; canonical writer
preservation and clean-idle acceptance remain open. Task 4.1 requalified with the
new full-workspace gates; no completion or all-destinations claim.

## Latest: key-ciphertext churn repaired; fifth exact Doomtap cache approved

User redirect reported nonstop GitGuardian incidents for dracon-home. Sampled
alert commits 7d7abf15 and 55f21121 change the same 16 SSH keys; private in-memory
checks show valid age envelopes and identical decrypted contents in every path.
The reuse comparator discarded an inline encrypted PEM's exterior newline when
it mistook the representation for a whole-file tag, then generated fresh random
ciphertext on each sync pass. Warden now authenticates the old payload, requires
matching exterior bytes, and permits exact inline-text reconstruction. Real key
edits, whitespace edits, plaintext input and whole-file policy upgrades still
refuse stale reuse. The synthetic negative control reproduced the churn before
the fix. No security/notification rule was disabled or incident dismissed.

All four FULL workspace gates pass on unchanged before/after source inventories
with a separate persistent target directory. The qualified Warden release was
atomically installed (bcb1987811d5e930faeb60f3b95a13940a470dd5918ef7293110b3a9c797e2bb).
Four observations across 105 seconds show no key diffs and no key-changing
commits while unrelated work continues. Later fleet status reports `.dracon`
converged/OK. Historical published ciphertext and alert-email backlogs remain;
this does not classify every historical incident as harmless. Evidence and
all raw gates: `../../../dracon-warden/docs/design/ssh-key-ciphertext-churn-2026-10-27/`
(from this document's directory, use the repository sibling `dracon-warden/`).
The earlier /tmp frozen build directory disappeared before this repair; no
qualification was inferred from that missing artifact.

User separately approved `docs/screenshots/qa/ascension-owned.png` as a cache.
Added only its exact ignore rule, expanded the existing policy regression, and
removed only that index entry inside `dracon-sync maintenance`. Its staged and
worktree bytes differed: BOTH variants were preserved privately, local bytes
remain unchanged, and every other index entry was verified unchanged. All three
visual-publication tests pass with the original source/comparison contract
intact. Receipt: `checks/publication-resync-2026-10-27/doomtap-owned-approved-deindex.json`.
No manual commit or push; daemon resumed automatically. Doomtap remains retrying
in the fresh observation, so cache repair is not publication proof.

Fresh fleet: 36 watched, 30 summary-OK, 5 active, 1 warn, zero inspection failures.
Canonical-writer preservation, browser real-feed evidence, remaining autonomous
publication and entirely clean/idle acceptance remain open. Original goal active.

## Latest qualification: full workspace gates pass; loaded daemon matches

`checks/publication-resync-2026-10-27/frozen-workspace-v2-gates.json` records
all four unmodified workspace commands passing on one frozen assembly. The
snapshot uses its own target directory; live helper artifacts were untouched.
The first snapshot omitted Git metadata, the existing parent Clippy config and
parent installer required by integration tests. Those setup failures remain in
their original logs. The corrected assembly includes all three current crate
revisions (no sibling substitution). Four Clippy failures in system source were
repaired without runtime cleanup changes: remove unused duplicate wrapper and
fixture, initialize a report directly, exercise the audit-row helper in the
existing open-FD test. No assertions or guards were dropped.

`frozen-workspace-v2-loaded-identity.json` proves the loaded, installed and frozen
release dracon-sync executables all hash to 4ddef737a3fe0d42bfece133722bf48d36b2fa482c14ce01ffc15fc44fba2f9e.
This is NOT a pass for the still-failed canonical writer-preservation control.
Task 4.1 is qualified; task 4.2 and final fleet acceptance remain open.

The `.dracon` advertised origin/GitHub/GitLab tips now all equal
1c34781526b8e14e90791260b0698bd3ec593ef0, a descendant of the observed daemon
push snapshot 98918e70; the earlier GitHub ref-query failure was transient and
remains in its receipt. New edits remain pending. Fresh fleet: 36 watched,
29 summary-OK, 7 active, 0 warnings/inspection failures; dirty/retrying rows
remain. Doomtap still stages the new `docs/screenshots/qa/ascension-owned.png`
(index blob 19017b7f9a7915026426a52d29f8dfb121c072c6). The prior exact-four
cache approval does not silently cover this new file; no de-index was done.

## Workspace lifecycle fanout repaired; actual browser evidence remains red

Supersedes earlier pending-browser and sibling-compile status below, not their
historical failed outcomes. Bun 1.3.14 ignores `--concurrent-scripts=1` for
workspace lifecycle jobs: a four-workspace native probe overlaps all four.
The service hit its 512-task limit; failed-attempt WXT children accounted for
hundreds of tasks. They later exited; no cleanup signal was sent.

Browser helper now no-ops workspace lifecycle bodies only during Bun's trusted
dependency script pass, restores exact manifest bytes and executes all original
workspace phase bodies serially. Phase keys remain present because deleting
prepare changes Bun file/workspace resolution and fails the frozen lock guard.
The failed deletion-based attempt is retained. All 35 hook tests pass; the
real pinned-Bun overlap regression fails the previous runner with EEXIST.
The canonical lock was refreshed for two stale workspace versions, preserving
every previous dependency resolution/integrity tuple and guarding against
concurrent manifest/lock edits. No frozen guard was removed.

Correct configured original hook (`scripts/git-hooks/pre-push`) on 85fde1cfd9a3
completed installation and 155 product gates in 1003s: 154 passed, 1 failed.
The remaining YouTube Filter evidence test truthfully refuses its committed
control run because neither survey nor Watch-together target appeared across
12 real feed loads. Its own goal is paused on that external feed condition.
No assertion/evidence was changed, and browser retry was not cleared again.
An earlier explicitly invoked global wrapper chained the legacy local hook;
that failed diagnostic is NOT the configured-hook verdict.

Fresh inventory: 36 watched repos, 29 summary-OK, 6 active, 1 warn, zero
inspection failures; still NOT clean/idle acceptance. `.dracon` advertised origin
and GitLab refs advanced to 9aa57f58, but current HEAD is newer and the GitHub
advertisement failed: no all-destinations success claim. Workspace `cargo test
--workspace --locked --no-run` now compiles; this is not a full test-suite pass.
Canonical writer preservation qualification and all remaining contract gates
remain open. Detailed immutable outcomes and raw logs are under
`checks/publication-resync-2026-10-27/` (browser-serial-configured-original-manifest.json,
browser-configured-serial-corrected-full-hook.json, browser-lifecycle-hook-tests.log,
browser-lifecycle-negative-control.log, fleet-after-serial-hook.json).

## 2026-10-27 publication reassessment — no acceptance claim

Read `checks/publication-resync-2026-10-27/ASSESSMENT.md` for current evidence.
The active objective is fleet convergence; the bucket handoff remains stale.
Fresh inventory has all 36 watched repos, 30 summary-ok, four active, two warned,
zero inspection failures; it is not clean-idle acceptance. Original copied-index
commit hooks reproduce four NEW Doomtap ascension QA-frame hygiene refusals and
three platform `.env.dev` whole-file encryption refusals without index changes.
The new QA paths need a source/cache decision; prior exact-nine approval does
not authorize silently removing them. Credential-scope/header probes stayed
private: no live Warden policy, canonical metadata or staging was modified.

Browser acceptance-driver lint/typecheck suppression is corrected, and its
original workspace lint/typecheck/tests/build plus actual Bun driver/probe
compilation pass. The daemon committed this edit. An original full pre-push
check of frozen revision735065c5 is still executing after its Bash tool was
interrupted: observe the EXISTING runner2070402 and its real manifest
`~/.local/state/dracon/sync/gates/browser-extensions-shared/dracon-pre-push.lwsNM1/manifest.json`.
Do NOT start another copy. The owned original invocation has a3600-second bound.
Its earlier affiliate self-test exceeded the unchanged5-second test deadline;
that failure stays failed even though a bounded unchanged native isolation
passed in3.067s. Assess terminal receipt/manifest, not the interruption or stale
running marker, before further action. No retry ledger has been cleared.

Canonical-writer preservation remains failed/open. No global policy or zero-batch migration was applied during this reassessment.

## 2026-10-27 hook repair deployed; autonomous `.dracon` push in flight

The `.dracon` publication wedge was a Warden pre-push false positive: a Google-key-shaped base64 run inside the age envelope of `secrets/ssh/micro3.key` was extracted before ciphertext context was considered. `mask_warden_tags` now masks valid complete tags before extraction in all three scans. Warden (235+18+2) and sync (1511) tests, strict Clippy and deny pass; full-workspace tests stay broken on unrelated in-flight `dracon-system` errors. Release warden `9b632d46` installed, global hooks regenerated. The exact refusing commit passes in 0.57s. `.dracon` was unstuck and its daemon push is running (timeout scaled to 5400s). Doomtap published (ahead 0). Browser stays PUSH_STUCK on genuine gate failures; new `ascension-owned.png` stays blocked pending operator decision. Full evidence: `checks/publication-resync-2026-10-27/ASSESSMENT.md`. No acceptance claimed.

## 2026-10-27 disk-exhaustion priority update — NOT fleet acceptance

The operator prioritized Git staging disk growth. Candidate packing now excludes
`^{parent}^{tree}` rather than only `^{parent}`: a positive tree-root traversal
was copying unchanged baseline blobs into every recovery pack. A disposable
negative control reproduces the old behavior. A loaded daemon observation now
captures a 32,045-byte browser candidate pack (previously roughly 875 MiB per
cycle), alongside autonomous local commit activity. Successful ordinary and
truly empty original candidates release their owned directories; rejected
original bytes and conflict-index recovery remain preserved.

Age-only cleanup was initially executed and deployed in this session, then
found unsafe and disabled. Earlier deleted recovery content has not been
retrospectively verified; do NOT claim no loss. Automatic spool cleanup is now
a no-op, explicit age-only apply refuses deletion, and tmp_pack inspection does
not unlink files. Failed recovery retention remains unbounded pending a safe
content/ownership-qualified policy. Do not restore the earlier age-only sweeps.

All four workspace gates pass on the final tree-delta source. Raw logs,
qualified deployment receipt, reviewer notes and bounded runtime observations:
`../epoch-spool-safety-2026-10-26/` (sibling evidence directory). The earlier
1200-second workspace timeout remains recorded; the unchanged bounded
1800-second retry passed. The corrected release is now loaded, superseding
historical statements below that no new binary was installed. This narrow disk
repair does NOT establish canonical-writer preservation or full runtime
qualification. Those previous failed controls remain failures.

Latest full-fleet inventory in that sibling folder: 36 repositories, 29 ok,
4 active, 3 warned, zero inspection failures. This is NOT clean-idle acceptance.
Browser still has hook-related push-stuck state; no retry ledger was cleared or
hook bypassed by this repair. Continue publication and preservation work under
the original goal, not a completion claim for the disk symptom alone.

## Current: preservation qualification FAILED; fleet acceptance remains open

The earlier no-rollback qualification was invalid: its syscall harness had
changed acceptance from preserving the old writer's canonical inode to accepting
that writer's displacement into private recovery. That is not equivalent to
preserving canonical eligible work. The original requirement is restored.
`checks/epoch-integration-partial/exchange-restored-contract-instrumented.json`
executes installed artifact `7176180470e4e81bd4f65fd5fb88a6662fc34af06497b2b3c41a38e2b87040d2`
in disposable repositories and records **pass:false**: old writer canonical=false,
newer writer canonical=true, index preserved=true. The first restored-contract
attempt encountered ENOSPC and remains a separate failed receipt; ordinary temp
storage probes and an instrumented rerun succeeded in reaching the actual failure.
The retained reviewer could not be resumed (run not found); no approval is inferred.
No further deployment is justified by the prior qualification claims.

A separate stat-cache repair is now locally tested, not deployed or fully
qualified. `git/status.rs` no longer uses quadratic inventory comparisons or
blocking Git children on the async epoch path. It uses path-keyed maps, shared
owned-child cancellation under the epoch deadline, explicit repository paths
(including linked Git directories), SHA256-compatible deletion OIDs, and the
original index mtime so racy same-size edits cannot be hidden by cache transfer.
Native regression verifies expired-deadline refusal, exact candidate tree/mode/
add/delete/newline-path installation, unchanged local bytes, and racy-edit visibility;
the representative 100,000-entry SHA256 delta test passes in 0.31 seconds.
The repaired product inputs now pass all four required static gates: full
workspace tests, release build, strict full-workspace/all-target Clippy and
cargo-deny. Exact revisions, raw successful outcomes, artifact identities and
phase input inventories are in
`checks/status-inspection-root-cause/stat-repair-gate-manifest.json`.
The missing-index cold-bootstrap regression introduced during this repair was
corrected; its failed run remains recorded. A later 900-second full-suite
attempt timed out, and the measured 1800-second bounded rerun passed. Disposable
bad-cache negative control demonstrates the hidden same-size edit; the repaired
installation keeps it visible. Concurrent unrelated cleanup documentation edits
are explicitly distinguished from unchanged product inputs.

This is NOT runtime or deployment qualification: the newly built release
`7c39f954754338aa7728cf888aac237b27ea07bdd24c8e9f01afa49d0ee3e223`
ALSO fails the restored original writer-preservation syscall control
(`checks/epoch-integration-partial/exchange-restored-contract-final-stat-release.json`).
No new executable was installed by this work. An independent bounded read-only
assessment confirms that another lease/exchange variant cannot unify old/new
writer inode targets, and ordinary in-place writes do not supply the required
crash/cancellation/content transaction.

The fresh report discovers 36 repositories, not the baseline's 37:
`/home/dracon/Dev/warden-vid` is absent on disk and from discovery; the user
explicitly confirmed they deleted it. It is now an operator-retired historical
row (resolved, not restored or relabelled healthy) in
`checks/status-inspection-root-cause/post-gates-accounted-fleet-inventory.json`.
No repository is silently removed. Dirty/staged work, browser push-stuck state,
deferred deep checks and externally paused daemon health remain; summary
`ok=36` is not clean-idle evidence. An ENOSPC observation failure and its later
successful retry are separate artifacts. Another project's maintenance marker
was not cleared by this work; a later read-only check found it absent after the
enabled freeze watchdog fired.

Operator prerequisite and exact evidence:
`../../docs/design/canonical-publication-boundary-2026-10-26.md` (from this run
folder). The user confirmed the retired checkout and authorized continuation.
Bounded native probes now establish that atomic write units are unavailable and
reflink is unsupported on the actual host; modern delegations exist but a held
directory delegation still allows existing-file write opens. This is not a
publication fix or approval. Investigation continues without changing external
editors or deploying the failing candidate, including the independently blocked
browser publication pipeline. The existing guarantees and `.gitignore`
publication requirement are unchanged. Task and verification obligations remain
open.

## Historical no-rollback claim — INVALIDATED, not current qualification

Operator decision received: KEEP .gitignore publication and existing guarantees;
no separate metadata scheme. The reproduced rollback defect was therefore fixed
in the canonical exchange path rather than routed around.

Reproduced defect (legacy): checks/epoch-integration-partial/exchange-syscalls-v1.json.
Second reproduced fact: simply DELETING rollback was NOT sufficient — with one
exchange plus cancellation, a queued old-inode writer resolved onto the NEW
canonical file (exchange-syscalls-cancel-v1.json, oldWriterLandedOnCanonical).

Current protocol: ONE finalizing RENAME_EXCHANGE for an existing inode, then a
BILATERAL post-exchange fingerprint proof (private==before, canonical==desired).
A mismatch reports uncertainty; it never performs a second exchange. Absent-path
NOREPLACE creation is unchanged. Settlement now states
private-desired-payload-retained vs canonical-exchange-settled instead of
assuming the private copy proves "untouched". Later access to the displaced
inode still surfaces as a real concern through the archive stamp probe.

Evidence: norollback-qualified-v3-receipt.json. cargo test --workspace (616+4
+12+233+18+2 and bin suites), cargo build --release, strict all-target Clippy
and cargo deny all exit 0; 12 excluded-recovery tests pass including the new
regression; the RELEASE binary passes the actual ptrace syscall qualification
(pass:true, helperShA256 8e5fb4fd...).

NOT proven and NOT claimed: the running daemon (shA256 8b511489...) is a
DIFFERENT older binary, so the fix is not deployed; no installed/loaded
rehearsal, no zero-batch migration, no cancellation/ENOSPC/restart/unsupported-FS
matrix, no concurrent canonical-leaf replacement control. An independent review
run 4e855284 TIMED OUT with no verdict — that is not approval.

Fleet at live-fleet-observation-v2/observation.json: 37 inspected, 31 clean,
6 with evidenced causes (db-gateway QUEUED, browser-extensions-shared
PUBLISHING, dracon-platform WORKING, dracon-sync UNKNOWN transient,
doomtap BLOCKED, junk-runner BLOCKED). Final acceptance remains BLOCKED.

## Browser pre-push repairs (committed; real hook passes at HEAD)

Three gate failures had real root causes and are fixed in the repo:
1. affiliate verify-urls.sh self-test hardcoded port 8731; an unrelated
   long-lived local server answered 404 there, so a live probe classified DEAD
   and failed the whole affiliate CI gate. Now binds an ephemeral port via a
   helper that publishes the bound port. Verified over 3 runs + full ci.sh.
2. gemini-toolkit formatTimestamp documented "24 Oct, 14:32" but returned the
   date only, so two same-day answers rendered IDENTICAL badges. Fixed + regression.
3. vidpro releaseMetadata pinned version 1.16.0 and failed on the legitimate
   1.17.0 bump that release.json already listed as pending. Now asserts the
   invariant (pending contains pkg.version, uploaded never pending).

Gate evidence (checks/browser-prepush-repair-v1/): gemini 568 pass, vidpro 1439
pass, bookmarks-new-tab 322 pass, plus typecheck/lint/build for all three, and
the ACTUAL pre-push hook at HEAD 548a3a10a563 reported "4-gate passed for all
affected workspaces".

Still open for that repo: 1708 unpushed commits (GitHub leg permanently skipped
by the documented 2 GiB pack guard) and push retries failing with EAGAIN while
the host is memory-exhausted (27/30 GiB RAM, 22 GiB swap). That is resource
pressure, not a code defect; no guard was weakened.

## Historical: canonical-write preservation P1 / hygiene review

## Hygiene review green v1 — HISTORICAL targeted fixes, NOT deployed

Both retirement P1 corrections were independently CLOSED by recheck05c0febe;
that approval is explicitly limited to those corrections, not canonical-write
preservation or final acceptance. Current targeted hygiene24 and strict Clippy
pass; prior full four-gate assembly remains historical after corrections.

The separate queued-open rollback defect is now ACTUALLY REPRODUCED, not merely
theoretical. Script checks/epoch-integration-partial/qualify-exchange-syscalls.py
uses ptrace ONLY to pause the owned disposable helper at real renameat2 entry/
exit; no source/register modifications. Baseline succeeds first. Then a queued
old-inode open requests a real BREAKING lease after the final check, first
exchange exposes the desired inode, a newer canonical writable FD completes,
and rollback relocates that newer FD into private storage. Actual receipt:
exchange-syscalls-v1.json (exit1, pass:false), helperSHA b7b98b2b7251858261a1e3227e654bda5e1b84b9aeeb25f114453ecb354a0d49.
NewerFD36999145 vs canonical36999126 after rollback; newer bytes land private,
not canonical. Index remains unchanged, which does NOT establish preservation.
Only disposable fixtures/processes were affected; no live deployment or editor/
producer was stopped. Existing mutation helper must not be deployed as qualified.

Independent design follow-up5c59b1bf confirms this P1 and says BLOCK. Existing-
inode writes with exclusive lease/fatal SIGIO are only a proposal, not a fix:
complete original+desired bytes, metadata and provenance must be durable before
mutation; partial canonical metadata after interruption can expose missing
security negations. Backup-only preservation does not authorize that visibility
tradeoff. New versioned recovery/seals and fail-closed diagnosis are mandatory;
old exchange settlement must not label partial writes untouched/healthy.
No in-place replacement has been implemented or accepted. Do not simply remove
rollback or assume blocked opens restart pathname lookup.

A material publication-policy alternative needs operator input before adoption:
versioned daemon-owned rule metadata plus an owned Git-local application path,
leaving existing .gitignore bytes/inodes untouched. This must preserve current
higher-priority user/security rules and define ordinary-clone setup semantics;
it is NOT permission to hide eligible work, weaken hooks or skip qualification.
The alternative changes where NEW generated rules are published/applied, rather
than accepting partial user-visible .gitignore files. Keeping traditional
.gitignore publication requires a qualified replacement protocol; current
mutation stays blocked in either case. Goal scope/gates remain unchanged unless
an operator explicitly confirms an amendment.

Next: resolve the publication-policy design boundary, implement a qualified
FD-preserving writer/recovery protocol, extend actual syscall controls through
cancellation/timeout/ENOSPC/restart/unsupported FS, then fresh four gates, original
product gates, independent full rehearsal, installed/loaded proof, authorized
zero-batch migration and every-watched clean/idle/autonomous-publication proof.
No task/goal completion claim. All background review runs have settled and been
read; no owned wait remains.

## Hygiene review green v1 — HISTORICAL targeted fixes, NOT deployed

Independent reviewer run93b97751 completed with TWO P1 blockers, not approval:
committed .gitignore symlinks could falsely retire intent, and generic trim()
equality mistook leading-space rules for effective artifact rules. Raw review is
checks/epoch-integration-partial/hygiene-review-blocking-findings.md. Both were
reproduced FIRST against old code: hygiene-review-red-v1-tests.log = 0pass/2fail,
exactly at the symlink retirement and premature leading-space acknowledgment.

Corrections now implemented: require one exact regular-file committed tree entry
(mode100644/100755, blob, valid OID/path) before evaluating rules. Shared rendering
preserves significant requested/existing whitespace rather than trim-equating
it. Literal encoders escape leading whitespace to survive Warden preservation
without broadening to a different dirname. Native probes verify space/tab/NBSP
literal names, old leading-space rule corrected only by an actual dist/ rule,
canonical/target/index preservation for the committed symlink, and retirement
only after real regular metadata commits.

Current targeted checks: hygiene-review-green-v1-tests.log and strict all-target
workspace Clippy PASS with matching pinned helper. The red failures are retained.
V2's four-gate/zero-drift assembly below is now HISTORICAL, not current source
qualification after fixes. No deployment, install/config migration, real-repo
manual commit/push or producer stop. Final/full-fleet acceptance remains OPEN.

Owned independent READ-ONLY recheck is pending:
05c0febe-e572-42bd-90f8-3a8934349efa, resumed prior reviewer context. Also requested
bounded secondary analysis of an UNPROVEN queued-open/exchange concern: after
last lease check, an old-inode queued writer may force rollback while a newer
canonical desired-inode writable FD opened between exchange and rollback. The
helper currently exchanges back unconditionally on lease/fingerprint mismatch.
Do NOT assert Linux name lookup restarts or FD safety without actual evidence.
A deterministic syscall-boundary control (e.g. owned disposable-child ptrace at
renameat2 entry/exit) may be needed; none has yet been implemented/executed.
Main source stays stable during review; assess saved actual result on wake.

Broader queued-open/lease-break, precise exchange, unsupported FS, original
product gates, fresh coherent four-gate qualification, installed/loaded rehearsal,
zero-batch migration and every-watched actual clean-idle/publication proof remain
required. No goal/task completion verdict is claimed.

## Hygiene durable v2 — HISTORICAL coherent intermediate assembly, NOT deployed

Typed artifact-ignore intent is now implemented in sync/hygiene_intent.rs.
A private descriptor-bound dracon-hygiene-intents/ registry publishes complete
fsynced manifests before actual-index installation. Records bind repository,
immutable parent/path/OID/mode and exact selected candidate tree; only that exact
cached tree can recover an owned deletion grant after process exit. A foreign
cached delta gets no grant. Retry revalidates current policy/provenance and
re-renders CURRENT canonical ignore bytes through the leased writer rather than
replaying old complete files. Metadata intent does not enter the uncertain-
canonical-recovery gate or block its own staged commit. Retirement requires
actual committed HEAD ignore rules AND the artifact paths absent from that HEAD;
helper success/marker alone does not retire work. Scheduling, report flags and
current-health classification include pending intent and inspection failures.

Actual controls pass: production artifact commit plus process exit while an
existing writable ignore FD refuses reconciliation; fresh pulse merges newer
canonical operator content and commits metadata through the production sync
function in a disposable fixture (NOT running-daemon proof); uncommitted own
candidate resumes while foreign cached tree cannot inherit its authority;
cleanup opt-out, malformed JSON and symlink registry preserve index/canonical
bytes and refuse. Existing original controls remain unchanged.

All FOUR unmodified workspace gates PASS on coherent source with ZERO before/
after drift: workspace tests, release build, strict all-target Clippy, cargo deny.
Proof and exact raw summaries/artifact identities:
checks/epoch-integration-partial/hygiene-durable-v2-receipt.json and companion
source-before/source-after/source-drift and per-gate logs/exits. V1 targeted
hygiene19 and strict Clippy passed before malformed/current-health regressions
were added for the v2 full workspace run. The new intent module is explicitly
included in the source proof. No installation, live service/config migration,
zero-batch change, real-repo manual commit/push or producer stop occurred.

Independent READ-ONLY code review is PENDING, owned run
93b97751-a9c5-4116-b065-e073e39e48ee, artifact durable-hygiene-review.md.
Assess its actual result; do not infer approval from completion notifications.
Parent source stays stable during review. No final goal or deployment verdict.

Still open: broader queued-open/lease-break and exact-exchange/unsupported-FS
controls, original applicable product gates on coherent revisions, installed/
loaded identity and execution/concurrency/cancellation rehearsal, authorized
zero-batch migration, final independent full-contract rehearsal and a NEW
entire watched-fleet clean/idle observation backed by actual autonomous commits
and intended-remote advertised refs. The four-gate assembly does not waive any
of those prerequisites. Tasks2/3/4 remain in progress; task5 is not acceptance.

The earlier d484c42a research artifact was assessed: architecture proposal only,
not implementation or current-review proof. Its constraints informed integration.

## Hygiene admission v3 — HISTORICAL targeted repair; NOT final/deployed

Re-read the actual active goal, not the stale bucket-strategy handoff brief.
Tasks2/3/4 remain in progress even though the next unchecked top-level task is5.

Four additional gaps repaired:
- A real large-only production cycle is admitted even with no ordinary stage
  candidates. Its control warms up/commits all earlier work, proves exactly ONE
  untracked large file is the only raw change, then verifies unchanged HEAD and
  unchanged large-file bytes/inode while metadata follow-up is reported.
- Large ignore planning uses only admitted partition paths, not a new global
  untracked scan. Private materialized paths never enter this input; auto-stage,
  untracked-pattern and explicit auto-commit/file exclusions remain binding.
  Tracked entries and oversized managed ignore/attributes files are not hidden.
- Actual successful canonical ignore writes now report pending local follow-up;
  writing the metadata does not stage/publish it. The control verifies unchanged
  index/HEAD, an actual dirty ignore file and idempotent rendering without fake
  work. Caller diagnostics now describe local follow-up, not only failed writes.
- Legacy dracon-ignore-recovery names are recognized consistently by cheap
  concerns, archive scanning/privacy repair and settlement. A physical legacy-
  namespace exchange/late-open-FD control proves archive late writes reopen a
  concern without replaying/resetting canonical or staged work.

Current observed targeted gates: hygiene16pass/0fail, excluded49pass/0fail,
staged-epoch9pass/0fail, staged-blob bootstrap1pass/0fail and strict ALL-TARGET
workspace Clippy PASS. Raw results and pinned helper path use
checks/epoch-integration-partial/hygiene-admission-v3-*. The earlier v1 compile
failure (PathBuf vs DiffFile stage-gate argument) remains retained; it was fixed,
not retried blindly. V2 also passed before adding legacy-namespace coverage.

Still NOT a final coherent four-gate assembly, deployment, loaded execution or
fleet observation. No manual real-repo commits/pushes, service/config migration,
zero-batch change or stopped producers. Original product gates, installed/loaded
identity, independent full-contract rehearsal and every-watched clean/idle
acceptance remain mandatory.

Remaining demonstrated hygiene gap: artifact ignore retry intent is ephemeral
across an index-side commit, before/if canonical reconciliation fails. The
tracked paths then vanish from HEAD and cannot rediscover the rules. Implement a
DURABLE typed intent with policy/provenance validation and truthful scheduling;
do not relabel the current pending return as a durable repair. See
checks/epoch-integration-partial/hygiene-durable-retry-open.md for the inspected
dataflow, ownership/deadlock constraints and required controls. Broader queued-
open/lease-break, exact-exchange and unsupported-FS qualification remains open.

## Hygiene safety v2 — HISTORICAL targeted repair; NOT final/deployed

The newer safety review found gaps that the preceding passing suite did not
cover. Ignore application recaptured a fresh canonical baseline after rendering,
which could overwrite a newer closed-writer edit. It now observes/renders/applies
under one authority and carries the ORIGINAL fingerprint through lease/exchange.
Real isolated boundary injection replaces the file immediately before the helper:
the old render is refused and the foreign canonical bytes/index remain intact.
Existing writable-FD, symlink and hardlink controls likewise refuse safely.
Generated requests now use the recognized dracon-excluded-recovery namespace;
previous dracon-ignore-recovery records were invisible to concern/archive scans.
File0600/directory0700 protections apply at creation. Artifact de-index uses literal
pathspec input, never glob expansion, never a gitlink checkout or filename-only
artifact match. Eligibility no longer waives explicit file/per-repo opt-outs.

Observed current targeted gates: hygiene13pass/0fail, excluded48pass/0fail,
staged-epoch controls PASS and strict ALL-TARGET workspace Clippy PASS. Matching
frozen helper path, raw exits and post-check identities use
checks/epoch-integration-partial/hygiene-safety-v2-*. No current full-workspace,
release-build or cargo-deny qualification is claimed after these edits; v12 below
is HISTORICAL. No service/binary/config deployment or zero-batch migration.

Remaining demonstrated hygiene gaps: large-only admission still needs a true
post-harden production control (existing positive also edited note.txt);
large observation must respect auto-stage opt-outs and private-asset exclusions,
not enumerate every untracked file indiscriminately; applied canonical metadata
must force follow-up rather than report NothingToDo; and failed post-commit
artifact ignore writes need durable retry intent so the deleted HEAD paths do
not erase their plan. Do not call these repaired merely because focused tests
pass. Original gates, installed/loaded behavior and every-watched clean/idle
acceptance remain required. The repeated crash-lease EAGAIN exposed an immediate-
release inference: zombie state did not prove physical lease teardown. The test
now requires an ACTUAL successful nonblocking open within the SAME original
three-second deadline from parent death, then retains the inode, late-write and
no-late-restore assertions. This is a finite cleanup criterion, not an idle grace
sleep or reset budget; the earlier failed observations remain archived. Broader
queued-open and exact-exchange qualification remains open.

Operator-approved Doomtap scope change: only the exact nine QA frame names were
de-indexed under maintenance, with exact positive ignore rules OUTSIDE the
unchanged74-name curated comparison block. An initial glob selected13 files;
the daemon re-staged them before the corrected exact-nine operation. Receipt
checks/doomtap-exact-qa-deindex.json proves all nine canonical hashes unchanged
and eight other QA paths remain tracked. Original publication-contract test
passes. Later checks/doomtap-exact-qa-followup.json observes no approved paths
still tracked, identical canonical hashes and the same HEAD advertised by GitHub
and GitLab. No manual commit or push; loaded-runtime/publication witness still
needs journal correlation and this is NOT a complete fleet observation.

## Hygiene v12 — HISTORICAL coherent four-gate intermediate assembly

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

## Task-board state after this phase (honest)

- 4.1 **complete**: all four gates pass on one committed assembly (2888 tests, 0
  failed; release build, strict all-target Clippy, deny all exit 0).
- 2.1/2.2/2.3 and 3.1 remain open by their own evidence; staged-intent code is
  integrated and consumed by hygiene/excluded-revert/unmerged-epoch/staged-epoch,
  but the full behavioral-control set and original-hook revision manifest are not
  finished, so they are not claimed.
- 4.2 open: installed-execution receipt for the *daemon* behaviours (staged work
  preservation, concurrency, bounded cancellation) is separate from the Warden
  install receipt proven here.
- Task 5 open and currently unmet: 36 repos, 16 ok / 6 active / 14 warn, 18 AHEAD,
  3 DIRTY, containment `auto_push = false` still in force. Final acceptance needs
  autonomous publication plus an all-clean idle observation, which cannot hold
  while publication is contained and editing continues.

## 2026-10-27 (late) — publication containment LIFTED; 17 repos converged autonomously

`auto_push = false` was the 2026-10-27 security containment with its own written
release condition ("Restore only after fail-closed publication is qualified").
That condition was met and verified BEFORE the flip, read-only: all 18 ahead
repos were `behind = 0` (fast-forwardable, no rewrite needed), zero `.env`/
`.pem`/`.key` additions and zero provider-shaped secrets in every pending range
(including browser-extensions-shared's 1535-commit range), no pack above the
per-forge limit, and the deployed pre-push hook already verified fail-closed
with `fail_open_reproduced: false`.

    before : ok=16 active=5 warn=15 concern=0 failures=0
    after  : ok=30 active=5 warn=1  concern=0 failures=0

17 repos drained to ahead=0 through the daemon alone. Destination check via
read-only `ls-remote` against each repo's ACTUAL remotes: 32 of 33 clean repos
match on every intended remote; the one mismatch (polis) had both remotes in
agreement with local simply newer (active editing, not a publication failure).

`browser-extensions-shared` did NOT converge and is not claimed. Its push is not
wedged or disconnected: it completes the configured pre-push gate (165 tuples)
in ~78 min inside the scaled 90-min window and then CORRECTLY refuses, because
`youtube-filter/TEST exited 1` — the extension's committed real-feed receipt
records a genuine failure and `check-receipt-evidence.mjs` rejects it
(reproduced: 1 failed | 24 passed). No gate, assertion or receipt was weakened
or rewritten. GitHub for this repo additionally cannot succeed: pack 3.67 GiB
vs the 2 GiB limit, which needs asset migration out of history.

Two intermediate conclusions of mine were WRONG and are corrected in
`checks/publication-restore-2026-10-27/browser-gate-duration.md` + `RESULT.md`:
(1) an "idle SSH disconnect" — actually my own `timeout` wrapper; the daemon
supplies its own `ServerAliveInterval=5`, and the `~/.ssh/config` edit was
reverted byte-for-byte; (2) a "silent revert" of `push_op_timeout_secs` — the
900 value is the operator's documented later decision, left untouched.

Goal status (SUPERSEDED — see the youtube-filter repair section below): task 4.1
complete; task 5 was unmet at that point because one repository was ahead.

## 2026-10-28 — the blocking gate was a real harness defect; fleet converged

`browser-extensions-shared` never published because its own pre-push gate
CORRECTLY refused: `youtube-filter/TEST exited 1`, caused by
`check-receipt-evidence.mjs` rejecting the committed real-feed receipt. The
failure behind that was a genuine harness bug, reproduced live on the
operator's own feed: the hunt phase found the Watch-together shelf stamped and
hidden (82 video items intact — the FIRST genuine live sighting on record), and
the A/B comparison phase then read a feed tab the operator's window had closed
and reported `No Chrome tab with id …`. The receipt turned that into
"switching the rule off did not bring it back — the receipt cannot attribute
the removal to the extension", which accuses the PRODUCT of a comparison that
was never read.

Repaired in the browser-extensions-shared repo (no gate weakened):
- `currentFeedTabId()` / `readFeedSample()` re-target a lost feed tab and
  report `lostTab`, so a missing comparison is never scored as negative;
- adjudication extracted to `tests/live/real-feed-verdict.mjs` (pure, testable)
  with `abReadable` requiring both samples AND no lost tab;
- verdict text no longer claims the element came back when it was not read;
- carried-forward sightings emitted in the gate's shape, bounded to 72h so one
  old observation cannot keep the gate green forever.

Verification: vitest 59 files / **1064 passed, 0 failed** (was 1 failed);
`tsc --noEmit` clean; `lint --max-warnings 0` clean; receipt gate **OK** with
`stamped=2`. Falsifiability: reverting the module to exact pre-fix logic fails
2 of the 10 new tests.

Result — the gate now PASSES and the repository publishes:

    gates: GqCEEf PASSED 02:30:37, 3RTN12 PASSED, IESiCU PASSED
    ahead: 1609 -> 9 -> 0        github: 45c33097 -> 59775504 -> 3a7bcf91
    final: local == origin == gitlab == b7ca4a1b0f

The pre-fix stuck-ledger entry (3 consecutive failures) was cleared with the
sanctioned `dracon-sync repair stuck-unstuck` once the cause was verified fixed
AND a real published advance was observed.

Fleet: `ok=34 active=2 warn=0 concern=0 failures=0`, zero AHEAD, zero PENDING
(`checks/publication-restore-2026-10-27/fleet-converged.json`). Three repos are
DIRTY with push=OK — live editing by their own agents, which the objective
explicitly permits and requires not stopping.

## 2026-10-28 — staged-intent permanent deferral repaired (objective's named class)

`junk-runner` had 127 staged, small files and had not committed in three days;
the journal repeated `captured staged intent exceeds strict batch limit` and
`Dispatch Starved (>10 min) … 99252s` while `repos` showed
`state_cause: committing`.

Root cause: `max_stage_batch_files = 100` is the OPERATOR's reviewability cap on
what the DAEMON stages by itself (2026-06-20, lowered from 100000); the ordinary
path defers correctly by CHUNKING. `staged_epoch::eligible()` applied that same
cap to intent the user had already staged, where it is not a deferral at all:
`materialize_tree` commits the whole captured index, so the epoch either takes
all 127 or none, and no cycle can ever shrink an existing index. A permanent
refusal, restated forever. The files were fine (8.24 MiB total, largest 3.6 MiB).

Repair: new `SyncPolicy::max_captured_staged_files` (default 10_000, 0 =
unlimited) + `captured_stage_limit()`, used by `eligible()`. Per-file guards
(excluded paths, oversized blobs) still apply unchanged, so nothing previously
rejected for a real reason is now admitted. Field added to
`OVERRIDE_COVERAGE_GLOBAL_ONLY`. Full report:
`checks/staged-intent-batch-limit-2026-10-28/REPORT.md`.

Regression: new test reproduces the 127-file shape and passes; the pre-existing
test that asserted the OLD contract now pins the split instead. Negative
control: restoring `stage_batch_limit()` in `eligible()` fails the new test with
the exact production error.

Gates: workspace tests 44 suites / **2893 passed / 0 failed**; release build,
strict all-target Clippy and `cargo deny` all exit 0. Deployed via
`dracon-sync maintenance`; installed binary `6a0458d6…` == qualified release,
daemon restarted onto it and verified through `/proc/<pid>/exe`. Rollback:
`~/.local/state/dracon/rollback-binaries/sync-20261028T041044Z`.

After deployment the batch-limit refusal is GONE and the epoch builds the
127-file message. The NEXT refusal is a different layer and remains open: the
repository's own `.githooks/pre-commit` blocks three already-TRACKED
machine-local `.pi-glla/*.json` files and states that the durable fix is scoped
root rules — a repository policy decision, so it is left for the owner rather
than guessed, with no hook bypassed.

## 2026-10-28 — operator decision executed: junk-runner unblocked (3-day stall ended)

The remaining blocker was a POLICY choice, not a code defect, so it was
escalated and the operator chose option 1: untrack the machine-local Pi state
files and add scoped root `.gitignore` rules — the durable fix the Warden
pre-commit hook itself names.

Done: added `.pi-glla/{session-owner,update-check,pending-approval-renders}.json`
and `.pi-glla/compactor-jobs/` to `.gitignore` (after the `!.pi-glla/**`
negation, which is why they were being tracked), and untracked those five files.
`.pi-glla/audit-loop/findings.baseline.md` was deliberately NOT excluded — the
hook's durable-record category allows `.pi-glla/audit-loop/*.md`, and
`git check-ignore --no-index -v` confirms it stays committable.

`git rm --cached` first REFUSED ("staged content different from both the file
and HEAD") because the index held a third, older snapshot. That snapshot was
preserved to
`~/.local/state/dracon/untracked-index-snapshots-junk-runner-20261028T095444Z/`
before any force, and worktree md5s were verified unchanged afterwards — no
worktree content was destroyed anywhere.

Result: the repo's own hook returns rc=0, and the daemon committed
`49a84d143` (129 files) and kept committing (~670 commits in 20 minutes of live
agent work). Verified durable: the three files stay untracked in HEAD, exist on
disk, and are ignored. `junk-runner` and `polis` are now fully converged in the
parent (`parent-pin == nested HEAD`, 0 dirty).

Remaining: `dracon-platform` DIRTY traces to `hegemon`, which the daemon
DELIBERATELY quarantines in `exclude_repos` ("Remove only after its divergent
mirror histories are reconciled"), plus live edits from agents still working.
That is not convergence debt this goal may clear. Fleet stayed at zero warnings,
zero concerns, zero failures throughout, with only transient AHEAD/PENDING as
the daemon drained live edits.

Full record: `checks/final-assembly-2026-10-28/DECISION-EXECUTED.md`.
