# Explicit excluded-worktree restoration: in-progress implementation

Status: NOT deployment-qualified. See the fleet-idle CURRENT checkpoint for raw
outcomes and unfinished work. This design is not a completion or safety verdict.

## Demonstrated failure and scope

The fresh workspace attempt reported 1446 passing sync tests and one failing
positive: `test_excluded_tracked_file_reverted_when_opted_in`. The epoch refactor
left `to_restore` unused, disabling an existing explicit per-repository policy.
Default exclusions must still preserve edits; restoring the old unconditional
post-commit Git reset/restore would erase later hook/producer work and possibly
unknown staging. Neither remedy is acceptable.

## Implemented boundary

Only a repository's explicit `revert_excluded_to_head = true` enables this path.
Dry-run, default preservation and captured staged intent do not enter it. It runs
BEFORE ordinary private staging, never after hooks or publication. Excluded-only
changes can enter the path without manufacturing a commit.

The operation acquires the actual index lock before running filters. It refuses
cached intent, nonregular tracked targets, unknown content and foreign locks.
Untracked exclusions are not deleted. A pinned private HEAD index generates the
restore targets through actual required smudge filters, using NUL-delimited exact
filenames. The actual index is never reset or replaced by this operation.

File fingerprints include inode/device, length/mode/ownership, modification time
and streamed SHA256. Directory FDs opened component-by-component without following
symlinks constrain the write locations. A private recovery directory in the Git
directory is persisted and fsynced before any atomic exchange; displaced working
bytes stay reachable there. Missing files use atomic no-replace, not overwrite.
Cross-filesystem/unsupported atomic operations fail visibly and retain bytes;
there is no unchecked copy/unlink fallback.

A dedicated source-identity-bound executable helper takes Linux read leases to
reject already-open writable FDs/mappings. This matters: merely moving a file to
a backup can redirect an existing writer's future bytes into that backup. Lease
signal disposition belongs ONLY to the dedicated child, never daemon threads.
Lease type and the kernel break interval are checked around mutation. Parent
cancellation owns and kills the helper process group; no detached blocking task
is allowed to perform later restores. Required filter processes are owned by the
same deadline infrastructure. New writer/lease-break interleavings are NOT yet
fully qualified: do not infer comprehensive race safety from the basic controls.

## Observed controls, not a final qualification

`excluded-restore-expanded-controls.log`: eight passing tests covering binary
restoration with exact displaced-byte recovery, absent/untracked exclusions,
foreign staged intent and byte-exact index, foreign lock preservation, an active
writable FD whose subsequent bytes stay in the canonical file, required-smudge
refusal and later filter edits, finite hanging-filter/descendant cleanup, dry-run,
excluded-only/no-extra-commit behavior and a running pre-commit producer whose
later output is not reset. `excluded-restore-original-behavior.log`: both original
tracked-exclusion regressions pass, including the formerly failing positive.

Those controls used the immutable external helper recorded in
`excluded-restore-pinned-helper-path.txt`; the old build-output path is NOT reused
as proof. The earlier first positive attempt failed executable/source binding;
that failed log remains intact. Strict dracon-sync all-target Clippy passes.
Strict WORKSPACE Clippy passes after the small guard-assembly fixes below.

## Concurrent workspace assembly repair

A separately arriving committed guard-test change introduced three invalid
`f64 * u64` expressions. Explicitly converting the exact GiB scale fixes typing
without changing numeric intent. Its burst fixture also appended OLD timestamps
AFTER the current sample, violating the append-ordered latest-30 sample history.
The corrected 30-sample fixture keeps the cleanup followed by 28 observations,
including the SAME two final bursts, preserves both numeric assertions, and adds
an explicit strictly chronological assertion. Both new guard tests pass. The
production median calculation changed only its even-length check to the Clippy-
requested equivalent; no disk-rate algorithm or assertion threshold was relaxed.
Original compiler, behavioral and Clippy failures remain in the evidence logs.

## Work still required before deployment

- Verify cancellation while the helper actually HOLDS leases, not only while its
  required smudge child hangs. Verify new-writer/lease-break races and cancellation
  around exchanges. Preserve all displaced bytes and never silently report idle.
- Implement/rehearse diagnosis and recovery for interrupted or multi-file partial
  exchanges. Retaining a recovery JSON/file tree alone is not proof that subsequent
  daemon/reporting will keep uncertain recovery state visible and nonhealthy.
- Check non-Linux/unsupported-filesystem behavior explicitly. The current guarded
  path refuses where its required atomic/lease primitives are unavailable; do not
  substitute destructive writes or call these refusals successful restoration.
- Require truthful follow-up outcomes when later writes arrive in an otherwise
  empty candidate. Rehearse original hooks and storage/security boundaries.
- Reconcile the other disabled policy integration (version bump/release, unmerged
  resolution, artifact cleanup and large-untracked hygiene) without weakening
  staging ownership or meaningful tests.
- Then run a fresh coherent complete workspace suite, release build, deny and
  strict Clippy, independently review, qualify actual installed/loaded execution,
  apply the authorized zero-batch migration only with the supporting daemon,
  and prove every watched repository actually raw-clean/idle and published.

No real repository commit/push, policy migration, service change, or deployment
was performed by this continuation. Synthetic fixture producers were not removed
to make the convergence assertions pass.
