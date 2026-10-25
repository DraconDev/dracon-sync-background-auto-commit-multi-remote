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

## Recovery/follow-up v2 — observed, not installed

Five recovery controls now pass, including cancellation AFTER observing an actual
ACTIVE READ kernel lease in `/proc/locks`; an independent nonblocking writer can
then reopen the canonical inode and later bytes survive. The child also sets a
parent-death kill signal before Tokio initialization; this is implementation, not
an independent parent-crash execution proof.

Known untouched, rolled-back or partially applied transactions are classified by
exact retained inode/content identities and archived privately. This does NOT
replay old restores or change the actual index/canonical files. Unknown displaced
bytes remain pending; neither disabling the opt-in nor raw Git cleanliness waives
this state. Foreign locks remain exact and prevent settlement. Pending records
force daemon admission independent of raw Git dirt. Archived records stay retained
outside the pending dispatch set so history does not impose repeated full hashing.

The real JSON report test initially found a second false-healthy path: flags and
concern construction were correct, but current-health rendering reclassified the
raw-clean row as converged and cleared concern. Current-health now treats recovery
as a persistent blocker; the successful real CLI test requires concern=true,
health.current=blocked, a non-idle cause and a concrete recovery hint, while Git
counts stay truthfully zero and the report leaves index/recovery bytes untouched.

Empty candidates also inspect later cached/ref updates and restored-path changes.
A real pre-push producer leaves later canonical bytes pending while the original
backlog actually reaches a disposable bare remote, without manufacturing a commit
or disabling that producer. The first fixture lacked a configured upstream and
correctly did NOT authorize bootstrap publication; its RED log is retained. The
successful fixture configures the existing bootstrap gate rather than relaxing
no-unwanted-push behavior for upstream-less repositories.

Raw v2 results under checks/epoch-integration-partial: recovery5pass, restore9pass,
original exclusion2pass, filter-only controls pass, real JSON1pass. The FULL Rust
workspace now passes (sync1461pass/0fail/3ignored, system616pass/0fail,
warden233pass/0fail, plus their integration/security suites). Strict workspace
all-target Clippy, release build and cargo deny all pass. Source inventories before
and after the complete workspace and release runs show ZERO source drift; the
pinned external DEBUG helper is unchanged. These are Rust assembly gates, NOT
final behavioral/installed/deployment or fleet-idle evidence. No new ignores or
security waivers were introduced. Earlier failures remain in their original logs.

## Archive-safety v3f — current targeted gates; NOT deployment-ready

Archival is not proof that a displaced inode can never receive another write.
A pending open around exchange, or another retained descriptor, can write AFTER
parent classification/move. Archives now retain an immutable observation seal:
request identity, exact target inode/mode/owner/length/mtime AND ctime/link count.
Each read-only concern probe checks those metadata identities without rehashing
historical payloads. Same-length writes with restored mtime remain concerns.
Source hashing also checks ctime and path/descriptor consistency across the read;
classification cannot bind a hash of old bytes to a later metadata observation.
Changed sealed records are NEVER resealed to manufacture healthy status. Missing
older seals are admitted only by reclassifying every original recovery identity;
unknown bodies remain blocked. A pending seal from a crash before directory move
is accepted only if its complete existing observation is identical, not replaced.

This adds O(retained target count) metadata work. Probes have a two-second checked
budget and a 64-MiB seal read limit; expiration/inspection failure is not clean or
idle proof. This is not a timing claim for stalled/uninterruptible filesystem
syscalls or a cross-platform execution claim. Report/daemon admission sees changed
archives even with raw Git clean and the restoration opt-in now disabled. Hints
name both the pending Git-directory prefix and dracon-excluded-archive/.

The first v3 guard run EXPOSED permissive tempfile directory defaults (both actual
production creation and the old fixture). New recovery directories request0700
BEFORE checkout/filter materialization, not after exposing bytes. Private staging
also requests0700 at creation. Worker-only legacy permission repair pins an owned
private archive container and opens owned child directories with descriptor-based
O_NOFOLLOW; it only removes group/other directory access. Reports never chmod.
An actual symlink negative proves no chmod/content change escapes to an unrelated
owned directory. No trust guard or byte/content assertion was relaxed.

Parent crash is now executed, not merely implemented: a disposable parent starts
the matching pinned helper; an independent witness requires the helper's exact
PID and target inode in an ACTIVE READ /proc/locks entry, checks real PPID, kills
that owned parent, and observes helper death/zombie state within a finite bound.
A nonblocking writer can then reopen and retain later canonical bytes. The crash
may occur on either side of exchange; the test does not infer an exact instruction
position merely from a lease observation. Retained targets must still classify.
The existing actual-lease cancellation witness remains separate and passes.

A final caller inspection found that returning a probe error BEFORE admission
would strand legacy permission repair. Current v3f admits diagnostic worker work
on inspection errors, retains an unresolved Blocked result on failed settlement,
and still allows pre-existing backlog publication rather than burning a transport
failure budget for local metadata uncertainty. The legacy control now invokes
real sync_repo, requires successful repair, and keeps byte-exact index assertions.
Its first extended fixture exposed ordinary libgit2 stat-cache refresh after the
fixture's inode exchanges; that RED run is retained. The fixture now verifies
cached HEAD equality and explicitly refreshes ONLY its owned, HEAD-equal stat
cache BEFORE the byte-exact baseline. Neither the equality assertion nor any
production/native status semantics was weakened.

Current v3f targeted results: recovery10pass, restore9pass, original exclusion2pass,
filter-only3pass, real JSON2pass; workspace all-target strict Clippy passes. The
real archived-write JSON test uses a declared synthetic seal as a CONSUMER control,
not helper/settlement provenance; actual settlement/late-descriptor preservation
is exercised separately by the private worker controls. Source inventory and
pinned DEBUG helper stayed unchanged across these current gates.

The preceding v3d coherent assembly passed COMPLETE workspace tests (sync1466pass,
0fail,3ignored; system616pass; warden233pass plus security/integration suites),
release build, strict Clippy and cargo deny, with zero source drift. It is NOT the
current v3f assembly: caller admission and its fixture changed afterward. Do NOT
mix v3d full/release/deny with v3f targeted results as final four-gate proof.
Raw failures/successes remain excluded-archive-v3*, v3b*, v3c*, v3d*, v3e*, v3f*;
source-before/source-after JSONs bind the latter assembly gates and helper hashes.
No installation, live configuration migration or qualified loaded-runtime/fleet
observation was performed here.

## Work still required before deployment

- Qualify actual queued opens/new-writer lease breaks and precise exchange/crash
  interleavings, beyond post-archive FD writes and observed-lease parent death.
  The new archive concern probe must never be bypassed to claim idle.
- Check non-Linux/unsupported-filesystem behavior explicitly. The current guarded
  path refuses where its required atomic/lease primitives are unavailable; do not
  substitute destructive writes or call these refusals successful restoration.
- Rehearse actual daemon restart/admission and original hooks/storage/security
  boundaries, not only direct worker, kernel-lease and real report controls.
- Reconcile the other disabled policy integration (version bump/release, unmerged
  resolution, artifact cleanup and large-untracked hygiene) without weakening
  staging ownership or meaningful tests.
- After the remaining implementation edits, requalify a fresh coherent workspace
  suite, release build, deny and strict Clippy, independently review and qualify
  actual installed/loaded execution,
  apply the authorized zero-batch migration only with the supporting daemon,
  and prove every watched repository actually raw-clean/idle and published.

No real repository commit/push, policy migration, service change, or deployment
was performed by this continuation. Synthetic fixture producers were not removed
to make the convergence assertions pass.
