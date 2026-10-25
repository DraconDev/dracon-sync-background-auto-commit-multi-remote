# Active systematic fleet-idle repair — checkpoint

Goal: .pi-glla/goals/20261025090441-ya0tp4.md. Strong final requirement is ALL
watched repositories clean AND idle in a fresh observation, not merely previously
published ancestors. Do not hide eligible work or stop editors/producers to obtain
it. No daemon implementation, index recovery or deployment has occurred yet.
Only the read-only observer's inventory cardinality input was generalized, with
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

First scoute8fd262b settled. A retained follow-up is running as
3d460217-4a5c-4b89-a283-b7a4efcbe292, <=20additional calls/100lines, read-only,
for staged-identity/commit-sealing integration and batch-cap safety. Assess its
native result before implementation/completion. Parent owns status/hygiene
research and every mutation. No branch/identity/security changes authorized.

Next: assess follow-up design; full37remote/object baseline is now captured.
Inspect actual Doomtap hygiene refusal and reproduce staged-intent root gaps in
disposable fixtures, then make
small forward-only repairs with preservation/refusal tests before any live recovery.
Task1 is in progress; later tasks remain open. See confirmed goal for all8gates.
