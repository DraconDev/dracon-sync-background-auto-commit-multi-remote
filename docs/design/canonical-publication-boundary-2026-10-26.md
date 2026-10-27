# Canonical publication: unresolved writer-coordination boundary

## Current result

The current existing-file exchange protocol does **not** satisfy canonical writer preservation. No further deployment is justified by its earlier qualification. The goal remains unfinished; retaining bytes privately is not equivalent to keeping eligible writes on their canonical path.

The unchanged restored disposable syscall control was also re-run on the current
loaded/installed daemon `4ddef737a3fe0d42bfece133722bf48d36b2fa482c14ce01ffc15fc44fba2f9e`
on 2026-10-27. Baseline passes; the actual exchange schedule finishes without
harness errors, with old writer canonical=false, newer writer canonical=true,
and index preserved=true. Overall preservation FAILS. Receipt:
`evidence/fleet-idle-convergence-2026-10-25/checks/publication-resync-2026-10-27/current-loaded-canonical-writer-control.json`.
Static workspace gates pass independently; they do not override this result.

Earlier, the restored disposable syscall control failed against both:

- Installed executable `7176180470e4e81bd4f65fd5fb88a6662fc34af06497b2b3c41a38e2b87040d2`.
- Newly built release `7c39f954754338aa7728cf888aac237b27ea07bdd24c8e9f01afa49d0ee3e223`.

Both retain the index and newer writer's canonical identity but detach the queued old writer into private recovery. See `evidence/fleet-idle-convergence-2026-10-25/checks/epoch-integration-partial/exchange-restored-contract-{instrumented,final-stat-release}.json`. The older passing harness had inverted the original preservation condition; that acceptance was invalid. Original receipts and failed/cancelled attempts remain intact.

## Why another exchange variant is insufficient

An open queued before exchange has already resolved the original inode. An open after exchange resolves the replacement. Removing rollback protects the newer writer but cannot retarget the older one. Rolling back can instead detach the newer writer. Additional fingerprint checks and bilateral inode leases do not unify these identities or lock directory entries.

Ordinary in-place overwrite keeps the inode but is not an atomic content-and-size transaction. Cancellation, parent death, ENOSPC or partial writes can expose torn content; restoring a backup can overwrite subsequent legitimate edits. A backup, source identifier or later archive alarm is not proof of canonical preservation.

A bounded independent read-only assessment reached the same conclusion and did not approve the implementation. Its report is retained in the parent session's subagent artifacts under `protocol-preservation-assessment.md`.

## Required boundary and permission

Keep `.gitignore` publication and existing preservation requirements. Do not replace publication with private ignore rules, hide eligible files, stop producers or weaken the syscall control.

A complete implementation needs either a demonstrated transactional same-inode filesystem facility with the required crash/namespace guarantees, or cooperation from **all** affected producers through a shared publication boundary before their path lookup/open/rename. No such complete boundary has been demonstrated on this ext4 host. An advisory lock used only by the daemon is insufficient.

Continued bounded investigation checked the actual Linux 7.1.2 host using disposable files and owned processes. `statx(STATX_WRITE_ATOMIC)` reports minimum/maximum atomic write units of zero; `FICLONE` returns EOPNOTSUPP and leaves the original bytes unchanged. New file and directory delegations are supported, so their availability must not be dismissed generically. A real directory-delegation control nevertheless permits an existing-file O_RDWR open; it is not the required pre-lookup writer fence. A file read delegation rejects the nonblocking write-open, but this alone does not establish a same-inode atomic publication operation. Raw capability/control receipts are `publication-kernel-capabilities.json` and `publication-delegation-open-control.json` under the stat-repair check directory. No live producer or repository content was changed by these probes.

Adapting externally owned editors/producers is not an authorized local implementation detail. The operator must approve a concrete integration boundary and identify the affected producers/integration interfaces before that architecture can be implemented and qualified. Merely authorizing more lease/rename retries does not resolve this prerequisite. Fail-closed containment would remain containment, not successful `.gitignore` publication or goal completion.

## Independently repaired stat-cache path

The stat-cache merge now uses deadline-bound owned async Git children, explicit repository paths, path-keyed comparisons, SHA256-compatible deletion records, bounded index input and the original racy-clean mtime boundary. Clean/unborn repositories with no actual index retain their proven private candidate directly. Native controls cover exact staged trees and mode/add/delete/newline paths, unchanged worktree bytes, expired deadlines, large inventories, and a disposable bad-cache negative control that reproduces a hidden same-size edit.

All four required static gates passed on the repaired product inputs: full workspace tests, release build, strict workspace/all-target Clippy and cargo-deny. The machine manifest records exact logs, revisions, file inventories and hashes: `evidence/fleet-idle-convergence-2026-10-25/checks/status-inspection-root-cause/stat-repair-gate-manifest.json`. The only tracked input changing between gate snapshots was another task's cleanup evidence document; it is explicitly recorded, not discarded. These passes do not qualify the failed publication protocol. No new executable was installed by this work.

## Fleet prerequisites remain open

The fresh daemon report discovers 36 repositories; the original baseline contained 37. The user explicitly confirmed: “i deleted the wrden vid continue”. `/home/dracon/Dev/warden-vid` is therefore operator-retired, not an unresolved missing checkout. Do not restore it. Its historical baseline row is retained with the operator's confirmation and resolved retirement status; the current watched inventory remains separately counted. This is not a healthy-row substitution or an agent checkout deletion.

The accounted inventory retains all 37 baseline/current repository identities and records this missing checkout explicitly. Dirty/staged work and browser publication stuck state remain. The report's `ok=36`/`publish_state=Ok` summary is **not** a clean-idle proof: rows contain dirty flags, deferred deep checks and paused health. Advertised destination refs were not re-proven by that report.

At observation time the daemon was externally frozen. Its marker named maintenance in `/home/dracon/Dev/dracon-strategy/ai-auto-writer`; this work did not clear another owner's pause. A subsequent read-only check confirmed the enabled user freeze watchdog had fired and the marker was absent: this pause is no longer an active blocker. A transient ENOSPC report attempt failed and is retained separately; a subsequent fresh observation succeeded. No cleanup or checkout deletion was performed here.

`checks/status-inspection-root-cause/post-gates-accounted-fleet-inventory.json` contains the machine-readable accounted observation. It explicitly sets `allCleanIdleProven=false`. The original objective and verification contract are unchanged.
