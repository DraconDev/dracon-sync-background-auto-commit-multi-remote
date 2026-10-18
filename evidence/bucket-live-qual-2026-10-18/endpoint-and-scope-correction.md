# Live OVH diagnosis correction — 2026-10-18

## Bucket is present: prior blocker diagnosis was incorrect

Operator supplied the signed-in OVH Public Cloud project Object Storage console.
Read-only bucket dashboard inspection at approximately 10:17 UTC establishes:

- Bucket `dracon-lfs`: London (UK), 1-AZ; 18 objects, 4.70 MiB.
- Endpoint `https://s3.uk.io.cloud.ovh.net/`.
- Virtual host `https://dracon-lfs.s3.uk.io.cloud.ovh.net/`.
- Root object prefixes displayed: `books`, `games`, `lfs`.
- The profile instead used `https://s3.sbg.perf.cloud.ovh.net`, region `sbg`.
  The earlier NoSuchBucket responses were from the wrong region. They never
  established bucket deletion, suspension or an account outage.

Independent boto3 read-only ListObjectsV2 using existing consumer and uploader
credential files, forcePathStyle, region `uk`, correct endpoint: **HTTP 200
for each role**, zero keys under `dracon-platform/`. No credentials printed.

## Local corrections executed

- Through sanctioned maintenance, atomically changed ONLY profile storage
  endpoint and region; all other parsed fields remain unchanged.
- Backup of original profile and sync config outside watched roots:
  `~/.local/state/dracon-sync/endpoint-correction/20261018T101823Z/`.
- Added exact watch root `~/dracon/bridge-fixture` (it was previously outside
  the two roots; prior sync-now runs did NOT demonstrate service autonomy).
- Service sandbox initially refused Git config writes there. Added only
  `ReadWritePaths=/home/dracon/dracon/bridge-fixture` via user-service drop-in
  `20-bridge-fixture.conf`. ProtectHome=read-only and ProtectSystem=strict and
  all other grants are unchanged. Restarted via maintenance; freeze cleared.
- New 2200-byte eligible source generated create-only at
  `public/qualification-20261018/ovh-live-2fc91627ddb9.png`, digest
  `215dc3a0b342d3d4855b07d7eb3555e69ce2ef520a4c0779b9b730e218116b9b`.
  No prior asset overwritten. `input.json` retains timestamp, recipe and digest.
- Candidate config committed normally as `506f114`, without filter bypass.
  Outgoing accounting had refused dirty filtered config (unknown-filter-bound);
  committing the non-secret config with normal Warden encryption preserves
  that guard. Neither source code nor filtering was weakened.
- Scheduled service PID 2960058 attempted fixture publication at 10:27:01 UTC;
  it refused. Manifest remained unchanged. No hand-edited manifest, manual S3
  upload, source retirement or synthetic success receipt was used.

## Remaining permission mismatch (conclusively read from OVH)

Read-only authenticated OVH API GETs on project users' `/policy` at ~10:29 UTC:

| Role | Object Allow actions | Resource |
| --- | --- | --- |
| games-uploader | s3:GetObject, s3:PutObject | arn:aws:s3:::dracon-lfs/games/* |
| games-consumer | s3:GetObject | arn:aws:s3:::dracon-lfs/games/* |

Both permit bucket-level ListBucket/GetBucketLocation. Destructive/admin
operations are explicitly denied. Existing repository binding requires
`dracon-platform/f5829561-d50d-4644-8e6e-ad7e293fa1db/` instead of `games/`.
Read-only production-transport HEADs of absent qualification objects return
false; that is not proof of GET/PUT authority. The published object Allow
resources categorically do not cover the current namespace.

**No IAM policies, keys, existing bindings, namespaces or credential contents
were changed.** Need explicit operator permission before extending object
Allow resources to the exact fixture prefix for each role. Preserve existing
`games/*` access and every Deny; no bucket-wide object grant. Consumer remains
GET-only; uploader GET+PUT only. Do not claim live upload/readback/restoration
until actual remote bytes and service enrollment are observed after permission
is resolved. Current fixture remains pending and service fails closed.

## Evidence limitations

Console and API values were read through the signed-in browser; local role
LISTs via boto3 are independent of daemon execution. Neither is an upload or
restoration receipt. Scheduled service refusal is in `scheduled-service.log`.
No IAM change or successful remote upload is claimed by this report.
