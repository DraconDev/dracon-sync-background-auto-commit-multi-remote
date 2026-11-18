# Bucket-backed asset rollout and commit recovery

Authorized by the operator's “ok lets do it” following the four-part storage
recommendation. Completion requires implemented and verified behavior, not
configuration files alone. No history rewrite, force-push, quota purchase or
mirror removal is authorized by this rollout.

## Required outcomes

1. Recover Deathrun's ordinary guarded commits and publishing. Correct the
   guard's distinction between repository storage and outgoing transfer size;
   unknown/new remotes must still receive a complete-history check. Verify
   actual automatic commits and remote acceptance of the preserved art work.
2. Establish bucket-backed assets for Deathrun and browser extensions, and a
   repeatable enrollment path for the asset-heavy fleet. Inventory scope and
   authority first. Keep code, exact asset references, provenance and processing
   recipes in Git. Treat original AI artwork as archival source, not disposable
   or bitwise-regenerable merely because a prompt exists.
3. Qualify immutable uploads, authenticated read-back, protected publication,
   fresh-checkout restore, existing art/build/release gates and offline shipped
   behavior before changing placement. Existing original files and historical
   versions remain recoverable. Preserve secret and manifest integrity guards.
4. Separate local commit durability, pending asset preparation and per-remote
   publication failures. Report exact blockers and preserve independent healthy
   mirror progress. Remote quota failures must not masquerade as missing repos
   or ownership/spool failures.
5. Produce a measured historical-storage inventory and a concrete archival or
   migration proposal. Execution of history-changing migration remains a
   separate operator decision. Bucket enrollment alone cannot reclaim reachable
   historical blobs from GitLab.

## Initial evidence (2026-11-17 10:46 UTC)

Deathrun still fails its pre-commit guard. Source inspection shows
`measurePushableBranch` enumerates `rev-list --objects --all` and applies the
same complete reachable-history size to every remote, regardless of the
tracking tips it reports. Restoring tracking-tip metadata alone cannot repair
this conflation. Keep a repository-growth measurement, but apply per-push
ceilings to the actual ref-scoped outgoing objects with conservative treatment
of unknown/new remotes and staged bytes.

Neither target repository has a `bucket-runner.json` or legacy asset workflow
configuration. The existing runner needs qualification for these assets;
successful synthetic enrollment or a configured credential profile is not
evidence of live archival-source coverage.

## Completion evidence to retain

- Regression and full applicable gate logs, including negative guard controls.
- Source and deployed artifact identities where executable code changes.
- Asset inventory, immutable object and manifest digests, upload/read-back and
  independent restore receipts; credentials and source secrets excluded.
- Original art preservation and the existing custody/build/release results.
- Actual commit and both-mirror checkpoint witnesses, with unresolved provider
  quotas reported accurately rather than marked successful.
- Historical inventory and separately reviewable migration plan.

## Guard deployment and custody checkpoint

The corrected guard is installed in the shared platform scripts. Its complete
applicable suite passed 84 tests. The first deployment restored local commits
but let an unrelated mirror's backlog reject a pinned push. The final guard
checks the actual destination for each pre-push invocation. The live pinned
Deathrun transfer passed at about 64 MB against the 2.056 GB budget. Direct
authenticated queries confirmed both remotes at `b02db78aeacb5ffee1e817ed15bf67c9eb3d1b44`,
which contains the recovered art commits. The reviewable patch, install receipts,
and publication witness are in `evidence/bucket-rollout-2026-11-17/`.

The candidate distinguishes local staged admission from per-provider outgoing
publication, keeps complete reachable storage as a separate diagnostic, and
shares tracking evidence only between exact matching push/fetch URLs. A fresh
or unknown mirror still gets a full requested-history publication check. New
over-limit staged assets, unrelated destination URLs and uninspectable objects
remain refused. The staged index is not mutated by measurement.

Before placement changes, 148 Deathrun art, provenance and evidence files
(133,976,928 bytes) were copied and read-back hash verified outside the watched
repositories. The receipt is
`~/.local/state/dracon-sync/asset-rollout/deathrun-custody-20261117T105613Z/custody.json`.
This local custody snapshot is not a live bucket or cold-restore qualification.

The old JS runner's current qualified adapter accepts narrowly reproducible
solid PNGs and a plain-Vite/npm/public-directory consumer. That is not actual
Deathrun or browser-extension archival coverage. Generic non-regenerable art
must not be mislabeled regenerable to pass it. The existing native immutable
storage protocol supports archival payloads, required copies, protected restore
metadata and staging bundles, but production enrollment/orchestration remains
an explicit integration task. Investigate and reuse those existing primitives
before introducing another asset-reference format.

The read-only fleet census accounts for all 37 status-report repositories with
zero failed status inspections. Thirty-one contain media candidates. Filename
counts are not enrollment authority. Browser extensions has 7,480 media paths;
Deathrun has 1,246. The census now recognizes the runner configuration before
legacy fallback; eight inventory regressions pass.

Deathrun's pre-existing staged art was preserved separately: 352 indexed asset
versions (283,887,374 bytes), the original index bytes and its digest, and the
unchanged HEAD anchor. Every saved blob was read-back verified, and HEAD/index
were checked unchanged across capture. This protects staged versions that may
differ from currently edited working files. Neither capture modified the real
index. The private receipt is `staged-index-custody.json` beside the worktree
custody receipt above.

The native storage planner confirms that external storage is currently
disabled for Deathrun. Its capture/index APIs deliberately reject first-time
enrollment of tracked paths without a reviewed forward-migration mechanism.
The implementation must preserve this default refusal and add an explicit,
version-bound migration path under the daemon's mutation owner; merely setting
the feature flag or removing that check would not satisfy the custody contract.

## Live bucket pilot

The native review command preserved 412 versions across 402 Deathrun paths and
saved the exact index before the guard deployment. The capture command accepts
an explicit review digest and refuses a path or version outside that review.
The library test exercises replacement of reviewed raw blobs with pointers
while keeping the original working bytes and verifying required copies.

The `dracon-lfs` bucket passed the conditional-write control within the scoped
`games/native-archival-v1/deathrun` prefix. A separate consumer key retrieved
the exact synthetic control. The scoped uploader conditionally created an
encrypted copy of the published orb source at its ciphertext digest. The
consumer downloaded matching ciphertext, and Warden decrypted it from an
empty Git repository to bytes identical to the published source. The pilot
used the available local Warden identity. Production staging, recovery of a
protected manifest in a fresh checkout, and consumer build bootstrap still
need qualification. See `deathrun-orb-bucket-pilot.json` in the evidence directory.

The browser extensions still require a separate scoped bucket binding. The
measured history and proposed archival cutover are recorded in
`historical-storage-migration-2026-11-17.md`.

## Reviewed staging command

The source tree now has a CLI path from reviewed capture through staging.
`storage prepare-enrollment-manifest` checks the exact review against the
leased jobs and requires every declared bucket copy to have a verification
receipt. Its optional `--ciphertext-out` exports the protected manifest to a
new path outside the checkout.

`storage stage-enrollment` takes the same review, job IDs and prepared
metadata ID. It requires the proposed append to root `.gitattributes` and the
exact protected manifest to be installed in the worktree, plus the required
clean driver bound to the same operator paths. It acquires the repository
operation lease, verifies the candidate index and publishes the attributes,
pointers and protected manifest together. It then checks and binds the
commit guard before releasing the lease. An interrupted guard setup can be
finished with `storage setup-guard` using the same repository and manifest
bindings. Publication of the Git commit remains the daemon's job.

The staging CLI and new cold-checkout fixture are still undergoing validation.
The fixture exercises the command wiring with a local backend and a Warden
protocol stand-in. Live provider read-back and real Deathrun build and runtime
checks are separate rollout evidence.

A consumer can bind `storage filter-clean --restore-root ...` in place of a
producer journal. Successful authenticated recovery writes a private receipt
for that repository, path, payload and placement contract. The filter checks
the receipt against the protected indexed manifest and rehashes the retained
source before accepting identical input. Neither a changed working file nor a
corrupted cache file can produce a pointer. A cache made before receipt
support needs one authenticated recovery to create the receipt.

## Multi-file Deathrun trial

The disposable trial enrolled all 17 original world-kit files from Deathrun
`ac6d839fbbcfcc3109b59b2e5537ff68506a0033`. Native jobs verified encrypted
read-back from the OVH archive and private local custody. The first manifest
attempt found that sorted job IDs do not imply sorted asset paths. The CLI now
sorts by the canonical path encoding after taking leases in job-ID order and
reused all 17 verified transfers.

The independently cloned consumer imported the protected committed manifest
and hydrated all 30,547,253 original bytes using its separate read-only key.
Every recovered Git blob and clean-filter pointer matched. The content diff
was empty before the consumer's Git stat refresh. A refresh of only the
managed paths in the disposable checkout left all staged IDs and modes
identical and cleared the tracked status. The canonical game's asset layout
has not been changed. See `deathrun-world-kit-trial.json` for the receipt.

The trial also exposed a checkout-dependent documentation inventory. The
declared local report and tool directories now render identically when
absent, while the census still rejects unclassified directories. The
independent fixture now regenerates the inventory after staging the manifest
so it includes the new tracked file in the count.

The restored fixture at `e0d874e39652caaa8efb84ca6036ea2d606be1ed` passed
the product check, WebP check, production build, release artifact verification,
all 182 art tests across 16 files, and the Chromium test that reloads the game,
opens the manual, and starts a run offline. A separate read-only review of
canonical Deathrun at `c4426cea6518b3d4a11523c19d8034bb8355ab5a` verified
the same 17 source blobs and 30,547,253 bytes against its HEAD, index and
working tree.

The canonical preflight also checked the expected Git objects for all 17
pointers against the qualified fixture and prepared private copies of the
verified journal and metadata for the live checkout. Live staging uses the
sanctioned maintenance wrapper, waits for the daemon's pause acknowledgement,
rechecks the approved HEAD and index, and verifies the storage guard before
commit processing resumes.

For a cold consumer, pin the enrollment commit and use separate private
metadata, recovery and hydration roots. Import the committed ciphertext with
`storage import-manifest`, the approved metadata-policy digest and the approved
Warden executable. Configure the local repository ID and the required
`storage filter-clean --restore-root ...` driver, then run
`storage setup-guard`. Run `storage hydrate` at that commit for each reviewed
path with the explicit read-only backend binding, consumer credential root
and private recovery roots. Verify the recovered source bytes against the
custody inventory and each clean result against its indexed pointer before
running the existing product gates. The trial used a Git stat refresh only
in the disposable checkout and compared every index entry before and after.

Keep the asset download at build or authoring time. The packaged game
continues to use the existing optimized, bundled art, and the browser
extension pilot should keep its packaged `public/` assets on the same
footing. For Tab Hero, the candidate archival sources are the original
`assets/*-master.png` files and `assets/icon-1024.png`. Its existing art and
icon generators read these files. Qualify both WXT build targets and the
existing art and generator tests after recovery. Browser transfer remains
subject to the separate scoped uploader and consumer grant.
