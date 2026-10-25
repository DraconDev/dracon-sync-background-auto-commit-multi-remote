# Active systematic fleet-idle repair — checkpoint

Goal: .pi-glla/goals/20261025090441-ya0tp4.md. Strong final requirement is ALL
watched repositories clean AND idle in a fresh observation, not merely previously
published ancestors. Do not hide eligible work or stop editors/producers to obtain
it. No implementation, index recovery or deployment has occurred in this goal.

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

## Source leads, not yet conclusions

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

Owned read-only scoute8fd262b-7f11-4693-b70e-e435427785ad is researching ONLY the
staging/private-asset guard seam, <=30calls/120lines, no writes. Native completion
must be assessed before implementation or completion. Parent owns status inspection
research and every mutation. No branch/identity/security changes authorized.

Next: assess scout report, capture full intended-remote/object baseline for the
37-repository inventory, reproduce root causes in disposable fixtures, then make
small forward-only repairs with preservation/refusal tests before any live recovery.
Task1 is in progress; later tasks remain open. See confirmed goal for all8gates.
