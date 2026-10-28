# Durable hygiene review follow-up — 2026-10-28

## Recovered review result

The settled run's temporary directory under `/tmp/pi-subagents-uid-1000/async-subagent-runs/05c0febe-e572-42bd-90f8-3a8934349efa` was absent when inspected. The saved reviewer output was present in Pi's session archive and is copied beside this follow-up as `durable-hygiene-review-2026-10-28.md`; this assessment uses that report only and does not treat the missing temporary bundle as evidence or relaunch the dependency.

The reviewer reported the two `.gitignore` retirement corrections (exact literal matching and exact tracked-regular-file validation) as fixed at the reviewed revision. Its residual question concerned the then-present second-exchange rollback path in `excluded_revert.rs`; the reviewer did not reproduce the queued-open interleaving or run gates, and explicitly limited its verdict to those two corrections.

## Comparison with current source

Current `dracon-sync` HEAD at assessment is `e38d3730226d`. The reviewed source revision `4be938222a65a9ce04b77add080586eebdc31d55` is an ancestor. Since that revision, the excluded-recovery helper was changed to perform one finalizing exchange for an existing path, verify both post-exchange fingerprints, and return unresolved on mismatch without exchanging back. The source comment now states that a second exchange could relocate a writer of the newly canonical inode. Tests include `excluded_recovery_finalizes_once_and_never_relocates_a_later_canonical_writer` and `excluded_recovery_late_displaced_writer_after_archival_stays_visible_and_blocked`.

This removes the specific second-exchange relocation branch identified by the reviewer. It does **not** establish kernel queued-open semantics or every simultaneous old/new writer interleaving: the finalize-once regression verifies the resulting inodes and retained bytes without an exact concurrent-open schedule, while the late-displaced-writer control verifies archive changes reopen a concern and block sync. Preserve the single-exchange/no-rollback invariant; do not cite this comparison as a general uncooperative-writer safety proof.

## Scope and status

The integration result is bounded to the review follow-up; it is not a full audit, workspace gate, deployment, loaded-runtime observation, or fleet-convergence result. The original goal remains active, and final clean-idle acceptance remains open.
