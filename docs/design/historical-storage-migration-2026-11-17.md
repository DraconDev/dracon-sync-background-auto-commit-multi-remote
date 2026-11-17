# Historical storage: measured inventory and proposed migration

This is a reviewable proposal. It does not authorize a history rewrite, remote
replacement, deletion, quota purchase or force-push. Forward asset enrollment
remains the current implementation task.

## Measurements

The read-only receipt is
`evidence/bucket-rollout-2026-11-17/historical-storage.json`, observed at
2026-11-17 18:20 UTC. HEAD and all inspected refs stayed unchanged during both
inspections. No fetch, GC, object deletion or history operation ran.

| Repository | Reachable objects, existing disk representation | Current HEAD, unique raw blobs |
| --- | ---: | ---: |
| Deathrun | 1,943,036,395 bytes | 987,215,074 bytes |
| browser-extensions-shared | 13,173,472,026 bytes | 4,934,973,263 bytes |

These columns use different representations. Subtracting them does **not**
calculate reclaimable storage. Reachability includes historical versions and
retained refs. Current HEAD bytes include all unique blobs, not only art.
Existing delta compression, provider accounting and outgoing transfer packs
also differ. Local tracking refs are cached observations, not fresh remote
acceptance witnesses.

The browser GitLab tracking tip remains `84cc5542be9722a08a9df30459e2a822b4bdab86`.
Its local reachable closure accounts for 10,176,697,528 bytes in the existing
object representation. Forward enrollment does not remove those objects or
resolve the provider's reported storage-allocation rejection.

## Current work: prevent further asset growth

Preserve exact HEAD, staged and working versions before enrolling existing
files. Store archival sources as immutable, verified objects; publish protected
restore metadata and exact references through the guarded index transaction.
Qualify an independent checkout and the existing product gates before changing
placement. Keep GitHub progress independent of the GitLab quota failure.

The existing `dracon-lfs` bucket has passed a native conditional-write and
authenticated read-back control under `games/native-archival-v1/deathrun`.
The synthetic control contains 64 random bytes. No original art was involved.
The same key is denied under `dracon-platform/`; a profile namespace alone does
not grant cloud permissions. Do not put browser assets under `games/` merely
to work around that distinction. Prepare a separate least-privilege browser
binding and qualify its uploader and consumer before enrolling browser files.

## Proposed separate archival and cutover operation

1. Finish and qualify forward enrollment first. Record the exact Git commits,
   object/manifest identities, original-source custody and cold restore results.
   Existing mirrors and historical refs remain intact.
2. At a concrete agreed cutover commit, acquire the shared publication and
   mutation leases in that order. Capture every retained ref and its OID,
   including tags and any refs that exist only on a mirror. Fetching and
   reconciling those observations is a separate prerequisite; today's local
   census cannot prove that remote-only history is absent.
3. Produce a complete bundle of the selected history into private custody.
   Verify it, recreate all refs in an independent repository, and compare their
   OIDs and object integrity. Preserve non-Git art/provenance custody separately.
   Encrypt the archive with the established recovery keys, upload create-only,
   and verify authenticated read-back and recovery with independent credentials.
   Record the archive digest, byte count, ref inventory, key recovery result and
   retrieval procedure. A local bundle alone is insufficient evidence.
4. Prepare a new lightweight repository from the qualified current source,
   exact asset references and protected restore metadata. Include a signed or
   protected mapping from the old cutover commit and ref inventory to the
   archive. Build and run the products from a fresh checkout and demonstrate
   offline installed behavior. Measure its actual initial transfer and both
   forges' acceptance without changing the current remotes.
5. Present those concrete receipts, proposed new remote names and rollback
   procedure for the operator's separate cutover decision. Prefer a new
   repository with the old history retained as an archive over rewriting every
   published commit. This changes history continuity and still requires that
   explicit decision. If preserving the existing repository URLs is required,
   prepare a separate rewrite/ref-mapping proposal instead; do not silently
   substitute a force-push.
6. After an approved cutover, update canonical remotes, daemon discovery,
   submodule/pin references and consumer bootstrap configuration together.
   Retain the old repository and archive until independent recovery and the
   rollback period are accepted. Provider storage reclamation is a later,
   explicitly approved operation; do not delete the current mirror to make a
   status row appear healthy.

Rollback keeps the original repositories and refs available throughout the
prepared cutover. Stop advancing the proposed new repository, restore the
recorded canonical bindings and recover any post-cutover work through a
reviewed forward transfer. No automated history surgery is part of rollback.
