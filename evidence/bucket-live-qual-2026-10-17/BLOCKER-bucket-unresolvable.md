# SUPERSEDED diagnosis: wrong endpoint, not missing bucket

Correction verified 2026-10-18 from the operator's signed-in OVH console:
`dracon-lfs` exists in London, endpoint `https://s3.uk.io.cloud.ovh.net/`.
The prior Strasbourg (`sbg`) profile endpoint was incorrect. HTTP 404s at
that endpoint did NOT establish deletion or a bucket/account outage. Do NOT
follow the earlier recreation suggestion below. Both existing scoped roles
LIST the UK bucket successfully (HTTP 200). See
`../bucket-live-qual-2026-10-18/endpoint-and-scope-correction.md`.

Historical report below (incorrect root-cause conclusion retained explicitly
as superseded, not as current operational guidance):

# Earlier live OVH qualification report (2026-10-18)

## What was achieved before the block
- New eligible fixture `public/qual-20261017.png` generated with the repo's own
  `tools/make-solid-png.mjs`, probes `eligible` via the qualified adapter path.
- Registered as a sealed candidate in `bucket-runner.json` (config only).
  Fixture commits `6fc36da` (candidate) and `5ba4172` (2200-byte sizing) by
  `DraconDev`, forward-only, no manifest hand-edits.
- First daemon cycle (sync-now, exit 0) autonomously planned and commit
  `61ef7d1` enrolled the candidate as `bucketed:false` with the correct source
  digest — because the 82-byte original was below the operator routing
  threshold (`largeBytes: 1024`). Policy worked as designed; no bypass.
- Fixture resized to 2200 bytes (96x96→512x512 solid, within adapter caps);
  planner renews managed registrations on byte change, so no stale entries
  and no policy edits were needed.

## The block (external, OVH-side)
Second daemon cycle refused fail-closed: `bucket runner bridge refused; no
commit: bucket-runner status refused: refused`. Worktree and prior manifest
left intact; nothing committed, nothing bypassed.

Read-only probes (no PUT/DELETE, no secret output) against the profile-pinned
endpoint `https://s3.sbg.perf.cloud.ovh.net`, bucket `dracon-lfs`:

| probe | result |
| --- | --- |
| boto3 HEAD/GET (virtual-hosted + path styles, both roles) | 404 / NoSuchBucket |
| boto3 ListObjectsV2 prefix `dracon-platform/f5829...` (both styles) | 404 NoSuchBucket |
| runner's own `@aws-sdk/client-s3` + `forcePathStyle` (consumer LIST, HEAD icon) | 404 NoSuchBucket / 404 NotFound |
| runner's own SDK (uploader LIST `dracon-platform/`) | 404 NoSuchBucket |
| endpoint TCP/TLS/DNS | healthy (`s3.sbg.perf.cloud.ovh.net` resolves; HTTP 405/400 on `/`, `/dracon-lfs`) |
| credential files | parse OK, mode 600, untouched since 2026-10-14 (last known-good use) |

On 2026-10-14 the same bucket/endpoint/credentials passed scoped-access
checks (uploader-put-consumer-head-get, prefix list, 8 denial cases) and a
byte-identical fresh restore. Nothing client-side changed since: same
endpoint, bucket, creds, runner bundle (`352e4de0`), daemon (`2b74bb7c`).

## Operator action needed (outside goal scope: no IAM/bucket-admin authorized)
1. In OVH console/API check bucket `dracon-lfs` (region sbg): exists?
   suspended? moved to another region/endpoint? Check project/account standing.
2. If the bucket was deleted, restore/recreate it (objects are immutable and
   content-addressed; the two legacy icon/cover objects would need re-upload
   through the daemon, never by hand-seeding the manifest).
3. If the endpoint changed, say so — the profile update is a separate
   reviewed item, not this goal.
4. Then `/goal resume`: the pending state is consistent and forward-only
   (source committed in `5ba4172`, manifest pending). The next daemon cycle
   will renew the registration and upload; this goal resumes at task 2.

## Files
- `sync-now.log`: first cycle (enroll local-only) transcript.
- `fixture-commits.log`, `fixture-status.log`, `fixture-sha256.log`: input lineage.
- `manifest-state.json`: daemon-written manifest (icon/cover `bucketed:true`
  legacy entries; qual entry `bucketed:false`, digest `1e45ffa1...`, 2200 bytes).
- `daemon-journal-excerpt.log`: journal around the cycles.
- `s3-head-probe.py`: read-only probe (boto3; prints statuses only).
