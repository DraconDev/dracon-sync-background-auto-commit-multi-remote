# Renewed non-vacuity after legacy refusal and metadata-mode correction

The disposable snapshot run completed all three discriminating safety mutations:

| Mutation | Baseline | Broken | Restored |
|---|---:|---:|---:|
| Duplicate classification | 0 | 101, named classification assertion failed | 0 |
| Post-write rescue verification bypass | 0 | 101, named preservation assertion failed | 0 |
| Kernel lock AND settlement exclusion bypass | 0 | 101, named old-live-owner assertion failed | 0 |

Exact commands, durations and output paths are in [results.json](results.json); patches and complete logs are alongside it. These were behavioral failures, not compilation failures or command timeouts. The final restored helper build exited 0, and [the restored complete spool suite](restored-epoch-spool.log) passed **47 tests, zero failed/ignored**, including the host cases and both-order metadata-permission regression. [PASS](PASS) is the driver's terminal marker, not a live-drain authorization.

Reproduce from the parent workspace:

```sh
python3 dracon-sync/docs/evidence/epoch-spool-reclaim-2026-10-31/run-corrected-nonvacuity.py \
  --workspace "$PWD" --command-timeout 900
```

All mutations were in the private workspace `/home/dracon/.cache/space-hygiene/epoch-corrected-nonvacuity-yxn55gg3/workspace`, on its own target directory. Live/published source was read-only. [Snapshot inputs](snapshot-inputs.json) bind the copied tree; [comparison](relevant-live-source-comparison.json) confirms all 16 compared spool/lifecycle/helper/lock inputs still matched when checked afterward. Concurrent unrelated report edits mean this is **not** an all-gates-green claim for the current live workspace. The destructive interlock remains false.
