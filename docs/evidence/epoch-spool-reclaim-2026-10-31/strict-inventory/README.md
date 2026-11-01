# Fixed-scope read-only observations — no corrected drain

Scope: [the explicit manifest](repo-manifest.json) contains 36 checkouts reported by `target/debug/dracon-sync repos --json`. The strict observer resolves each actual gitdir and common directory, including nested module/worktree metadata, and deduplicates identities. Both schema-2 observations covered 59 gitdirs, completed without discovery/stat errors, and used that same manifest. This is an interval observation, not an atomic filesystem snapshot or deletion authority.

The observer has [four passing disposable-fixture tests](tests.txt): actual linked-worktree/common/module discovery, hardlink allocation accounting, explicit read/symlink failures, and protocol marker/identity/kernel-lock diagnostics. Source: [inventory-strict.py](../inventory-strict.py), [tests](../test_inventory_strict.py). No live configs, branches, daemon lifecycle or recovery payloads were changed.

## Before and later observation

The before interval began **2026-11-01T13:59:06Z**; the later observation ended **2026-11-01T14:00:47Z**. Raw records: [before](baseline-v2.json), [later](observation-after-v2.json). Derived counts, identity sets, missing-source details and sums: [comparison](observation-comparison.json).

| Gitdir | Before count | Before logical bytes | Later count | Later logical bytes |
|---|---:|---:|---:|---:|
| browser-extensions-shared/.git | 63 | 110,103,028 | 63 | 110,103,028 |
| dracon-platform/.git | 132 | 243,129,285 | 132 | 243,129,285 |
| dracon-platform/.git/modules/web-games-capture-anime-girls | 2 | 324,402 | 0 | 0 |
| dracon-platform/.git/modules/web-games-junk-runner | 3 | 807,725 | 1 | 403,701 |
| dracon-platform/.git/modules/web-games-polis | 126 | 414,269,065 | 126 | 414,269,065 |
| dracon-strategy/ai-auto-writer/.git | 7 | 83,285,881 | 7 | 97,200,667 |
| empire/.git | 1 | 96,311 | 1 | 96,311 |
| **Total** | **334** | **852,015,697** | **330** | **865,202,057** |

All paths above are under `/home/dracon/Dev`; all other scoped gitdirs had no observed spool. Schema-2 records include directory device/inode identity. The earlier [schema-1 initial observation](baseline.json) is retained as history, not substituted for this identity-bearing baseline.

- Before: **327 ownership-uncertain legacy**, **7 active/fresh**.
- Later: **327 ownership-uncertain legacy**, **3 active/fresh**.
- All **327** legacy directory identities remained present. Four fresh directory identities disappeared; no new identity appeared. Their removal provenance is not established by this observer. Do **not** claim zero live-owner removals or attribute those disappearances to the corrected reclaimer.
- No preservation markers were observed in either snapshot. This is not retrospective proof concerning earlier attempts.
- No source was observed as an ownership-eligible candidate. Legacy inputs cannot be classified as safely removable duplicates without a compatible fence. A largest eligible mixed source therefore has **not** been identified or rescued.

## Recovery-store cost and attribution

Both observations recorded **1,945,215,904 logical recovery-store bytes** and **1,933,897,728 allocated bytes after inode deduplication**. The `recovery_stores` entries are retained recovery payload, not cleanup candidates. Logical totals include repeated hardlinks; allocation includes data/directory blocks once per inode, excluding inode-table overhead and without measuring reflink sharing. Neither figure is a claim of global filesystem space freed.

The later scoped spool total **865,202,057** is below **8,589,934,592** bytes (8 GiB). This does not prove a qualified drain: no corrected drain was executed in this interval. Attributable qualified net reclamation is **not established**. The historical **15,236,143,720-byte** figure is operator-provided context from the original objective; these explicit-scope observations are not equated with it.

The service was observed active: [observation](daemon-active.txt). Existing uncertain/fresh sources remain protected by the corrected code. A valid live mixed-source proof and fully green workspace qualification are still required before enablement.

## Reproduction

From the parent workspace, choose a fresh output path and keep the same manifest for comparisons:

```sh
base=dracon-sync/docs/evidence/epoch-spool-reclaim-2026-10-31
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s "$base" -p test_inventory_strict.py -v
python3 "$base/inventory-strict.py" \
  --manifest "$base/strict-inventory/repo-manifest.json" --output /path/to/new-observation.json
```

Exit 2 means incomplete/changed observation, never a zero-byte pass. File-stat consistency checks detect changes during each measured tree; ongoing production can legitimately change a source after observation. Every future actual drain requires a fresh baseline and the real transaction's continuously held fence and full recovery proof, not this diagnostic lock sample.
