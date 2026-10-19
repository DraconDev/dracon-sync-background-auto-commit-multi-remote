# Upload-provenance finding — first fixture NOT claimed (2026-10-19)

After the v8 migration succeeded (2026-10-19 01:25 local, independently
verified: system unit from /run, Type=exec, MainPID 2018106 all UIDs 1000,
exe SHA256 2b74bb7c… matches plan, user unit masked, watchdog timer
active, daemon healthy), the independent consumer check was run:

- `verify-live-ovh-consumer-2026-10-18.py` exit 0 (independent-consumer.json):
  object `dracon-platform/f5829561-…/215dc3a0….png` exists in dracon-lfs,
  2200 bytes, SHA256 matches source, byte-identical.

HOWEVER the object's LastModified is 2026-10-18T11:15:59Z — the same second
as manifest commit ef9e787 (12:15:59+01:00) and the runner index update.
That upload cannot be attributed to the autonomous scheduled daemon:

- Scheduled user daemon (pid 3718491) logged bridge REFUSALS at
  12:14:15 and 12:19:29 local, bracketing the commit, and logged no
  bridge-fixture commit at any point on 2026-10-18 (journal searched).
- Parent agent session made no sync/runner invocation in that window
  (transcript checked 11:12–11:19Z; it paused at 11:14:28Z).
- The full upload→manifest→index→commit in one second implies a
  host-context dracon-sync invocation (e.g. CLI), where the sandbox
  UID-mapping refusal under qualification does not apply. The exact
  invocation is unattributable from surviving records.

Per the verification contract ("the daemon autonomously enrolls and
uploads it … committed timestamped evidence correlates the input, daemon
cycle, object and manifest. bucketed:true alone is insufficient"), this
object proves consumer readability but NOT autonomous scheduled upload.
It is therefore NOT claimed as task-2 evidence.

## Decisive rerun (same method, new isolated fixture)

A second fixture is generated in a new isolated namespace
(public/qualification-20261019/), registered as a candidate, and left
untouched for the migrated scheduled system daemon to enroll, upload
and commit autonomously. Unique bytes/SHA make the remote object's
creation time unambiguously attributable to a post-migration scheduled
cycle, which the system journal records. No existing asset is
overwritten; no manifest is hand-written.
