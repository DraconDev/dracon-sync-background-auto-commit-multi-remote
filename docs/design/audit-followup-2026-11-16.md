# Publishing and forge-state follow-up, 2026-11-16

The deployed memory guard recovered a later pressure incident without operator
intervention: it requested recovery at 09:45 after a 09:43 pause, cancelled a
verified read-only probe, and released its pause at 09:47. Fresh automatic
commits and successful syncs continued afterward. Memory limits were unchanged.

## Frozen lockfile

The browser-extensions-shared publishing hook repeatedly failed its frozen Bun
install. Its pinned Bun 1.3.14 was already selected correctly. An immutable
snapshot of the committed workspace metadata reproduced the failure; regenerating
only lockfile metadata added the existing adblock-pro workspace and its link.
The resulting six-line addition changed no dependency versions. The frozen
check then passed with the same pinned Bun.

Before atomically replacing the canonical lockfile, all 70 manifests and the
original lockfile were checked against that snapshot. The daemon committed it
in 77d8d3bf5ae992c7afec391faa113dcf6c108c73. Hooks remained enabled. The
snapshot, before/after hashes, patch and command results are recorded in
`/tmp/dracon-browser-lock-audit-yhh_j8l4/audit-receipt.json`.

## False GitHub incident

The live forge-health file contained synthetic repository names and future
timestamps, with GitHub's incident flag latched. Its modification time predated
the earlier audit test run; this inspection does not establish which historical
writer created it. Pruning only checked the lower time bound, allowing future
hits to survive for weeks and extend GitHub retry pauses from 15 to 60 minutes.

At 13:38:32 UTC the original file was backed up and seven out-of-window hits
were removed. The now-empty GitHub incident was cleared. Backup and receipt:
`~/.local/state/dracon-sync/deployments/forge-state-repair-20261116T133832Z/`.
The repository's stuck retry ledger was then cleared through the supported
repair command after its lockfile cause was fixed.

The source now rejects future timestamps when pruning. It also serializes
load/update/save transactions among daemon workers and publishes JSON through
a temporary file and atomic rename. This prevents lost concurrent evidence and
partial JSON reads. Serialization is process-local, not a cross-process lock.
Three new regressions cover future corroboration, recovery preserving valid
evidence, and concurrent updates with simultaneous JSON readers. All 12 forge
tests pass. Full workspace validation and deployment results are recorded below
when complete.
