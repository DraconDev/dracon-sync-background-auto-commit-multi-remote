# Live OVH bucket qualification — RESULT (2026-10-19)

## Contract chain, decisive fixture (qualification-20261019)

| Link | Evidence |
|---|---|
| Input generated, isolated namespace | `input-20261019.json`: public/qualification-20261019/ovh-live-b1c954d94b4b.png, 2200 B, SHA256 26da6100…acb67, recipe 512×512 rgba(122,63,156,255); manifest-before 9229a1bf… |
| Candidate registered | bridge-fixture commit 4bf9405 (bucket-runner.json only; config registration is the task-1 mechanism; no manifest hand-edited) |
| **Autonomous scheduled daemon enrollment+upload** | System-manager daemon `dracon-sync-operator.service` pid 2018106 journal `01:47:33 📝 committed 1 file(s) in …/bridge-fixture` then `01:47:38 🔁 synced` (scheduled-cycle-20261019.log); manifest commit c93c85e (+50/-0) |
| Object in dracon-lfs | Independent consumer GET (independent-consumer-20261019.json): HTTP 200, **LastModified 2026-10-19T00:47:33Z — the exact second of the daemon cycle**, 2200 B |
| Bytes/digest verified independently | Consumer SHA256 26da6100…acb67 == source == manifest entry; byteIdenticalToSource true (scoped consumer credential file only) |
| Fresh restoration | fresh-restoration-20261019.json: empty root, production consumer transport, 1 download, restored byte-identical, retry already-present, manifest unchanged |
| Manifest/index correlation | Manifest SHA256 6d3ca2a5… == runner index manifestDigest, index updatedAt 2026-10-19T00:47:33.431Z |

## Migration verification (v8, operator-executed 01:25:57 local)

Independently re-checked, not taken from the receipt: system unit active
from /run/systemd/system, Type=exec, User=dracon/Group=users, MainPID
2018106 all UIDs 1000, /proc exe SHA256 2b74bb7c… == plan-pinned binary,
user unit inactive+masked, watchdog timer active (enabled-runtime),
`dracon-sync health` healthy/freeze-off. 17/17 offline migration tests
passed pre-apply. This removed the user-manager sandbox UID-mapping
ownership refusal (namespace-blocker.md) with confinement preserved.

A transient `accounting-unknown-filter-bound` refusal (01:37/01:42) was
caused by the agent's own uncommitted bucket-runner.json edit
(`* filter=dracon`): worktree accounting correctly fails closed on
filter-carrying files. Committing the config registration (the
pre-existing task-1 pattern) cleared it; no guard bypassed.

## First fixture (qualification-20261018) — supporting evidence only

Consumer GET exit 0 byte-identical (independent-consumer.json) and fresh
restore exit 0 (fresh-restoration.json), both 2026-10-19. Its upload
(LastModified 2026-10-18T11:15:59Z) is NOT claimed as autonomous
scheduled enrollment — see upload-provenance-finding-2026-10-19.md.

## Caveats / operator follow-up (do not lose)

1. **Reboot hazard:** the system units are runtime-installed under /run
   (ephemeral) while the user scheduler mask persists. Before any reboot,
   either restore user scheduling or make the operator system service
   declarative (NixOS); otherwise no sync scheduler starts. Rollback:
   `handover-backup` in the v8 directory.
2. The 20261019 source PNG was committed by the daemon as a plain tracked
   file (11b7ac1) in the cycle before candidate registration landed;
   enrollment then proceeded on the tracked source (same pattern as
   icon/cover/qual-20261017). No asset was overwritten or retired.
3. dracon-platform root still refuses `accounting-unknown-filter-bound`
   (pre-existing code limitation, fleet-attention-triage.md) — outside
   this goal; unrelated repos kept syncing throughout (journal).
