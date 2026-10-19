# Autonomous live retry qualification — RESULT (2026-10-19)

## Failure injection (local only; no OVH access-policy change)

- Method: runner profile `storage.endpoint` temporarily set to
  `https://127.0.0.1:9` (dead local port → connection refused transport
  failure). IAM policies, credentials, bucket and namespace untouched.
- Injected 09:05:25Z (profile hash 5c4b1673… → a5930a46…); byte-exact
  backup at ~/.dracon/state/bucket-runner/backups/profile-pre-retry-qual-20261019.json.
- New isolated fixture staged under injection: retry-input-20261019.json
  (public/qualification-retry-20261019/ovh-retry-81433a363012.png, 2200 B,
  SHA256 e842d3f0…fcc4), registered + committed 92c5d67 via
  `dracon-sync maintenance`.
- Incidental: a foreign session's forgotten daemon freeze (process gone)
  was resumed at 09:05:52Z via sanctioned `dracon-sync resume`; the 30m
  freeze watchdog would have auto-cleared it. Noted here for honesty.

## Failed attempt — safe (retry-failure-window-20261019.log)

- 09:05:58Z scheduled daemon: "bridge-fixture bucket runner bridge
  refused; no commit: bucket-runner status refused: refused".
- Local data preserved: fixture SHA256 still e842d3f0…fcc4; manifest
  unchanged (6d3ca2a5…), zero manifest references to the new fixture,
  HEAD unchanged. No unverified reference committed.
- Remote object absent: consumer HeadObject 09:07:48Z → 404
  (probe-retry-object-20261019.py exit 3).
- Unrelated repos committed+synced in the same window (.dracon,
  junk-runner, dracon-sync — see log).

## Autonomous retry — verified

- Profile restored byte-exact 09:07:52Z (SHA256 back to 5c4b1673…,
  endpoint s3.uk.io.cloud.ovh.net, no injection string remains).
- Scheduled daemon enrolled+uploaded with NO further human/agent repo
  action: manifest commit dfb16cf at 09:11:12Z; runner index
  updatedAt 09:11:12.290Z, digest 1357bbb4… == manifest file hash.
- Object LastModified 2026-10-19T09:11:12Z (retry cycle second);
  independent consumer GET exit 0 byte-identical
  (retry-consumer-20261019.json); fresh restoration exit 0 into empty
  root, 1 download, byte-identical, idempotent
  (retry-restoration-20261019.json).
- No duplicate/stale enrollment: exactly 1 manifest asset entry for the
  retry sourcePath (6 assets total), SHA/bytes match input.
- Unrelated repos synced throughout the retry window
  (retry-success-window-20261019.log: dracon-sync, junk-runner,
  db-gateway, .dracon).

## Close-out state

Profile hash == pre-injection original; daemon healthy (system service
pid 2018106, freeze off). No failure injection remains active anywhere:
the only change was the profile endpoint, restored byte-exact.
