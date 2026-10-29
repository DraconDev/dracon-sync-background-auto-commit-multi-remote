# Loose-object repack trigger — repair for the polis refusal class (2026-10-29)

## Symptom class this closes

`checks/polis-high-water-2026-10-29/REPORT.md` documented a **reachable**
object-database shape the daemon could not self-heal:

```
loose_bytes = 1,864,982,930   (1.86 GB across 4606 loose objects)
pack_bytes  =   911,024,780
git count-objects -v → count: 4606  size: 1834196 KiB
```

One un-ignored `pi-session-*.html` export was rewritten and re-committed 14
times in eleven minutes. Each ~75 MB revision landed as a **loose** object, so
Git never delta-compressed the near-identical copies. Reachable stored size
crossed GitHub's 2.18 GiB high-water guard and the bucket pre-commit hook
correctly refused every epoch; the daemon correctly backed off 15 minutes.

That report's own "Could it recur?" answer names the gap precisely:

> The daemon's existing auto-gc path triggers on *garbage* thresholds, not on
> loose-but-reachable bloat, so it does not self-heal this shape.

`git gc` was run once by hand (through `dracon-sync maintenance`) to clear
polis. Nothing prevented the next repo from re-entering the same shape.

## Why the existing trigger could not see it

`auto_gc_garbage_threshold_bytes` (default 2 GiB) is compared against
`count-objects -v`'s **`size-garbage`** field — unreachable debris. In polis
every one of the 4606 loose objects was reachable (`comm` of loose names
against `--batch-all-objects` returned 0 orphans), so `size-garbage` stayed at
approximately zero while `size` (loose, reachable) sat at 1.79 GiB. A
garbage-only trigger is structurally blind to this class, no matter how the
threshold is tuned.

## Repair (durable, in `dracon-sync`)

1. **New independent trigger**, `auto_gc_loose_threshold_bytes`
   (`src/policy.rs`, serde default `64 MiB`, `0` disables only this trigger):
   compared against `count-objects -v`'s `size:` field, i.e. loose bytes.
   64 MiB is below the 100 MiB per-file staging limit, so a single repeatedly
   rewritten-but-eligible export is repacked before loose copies eat the
   guard's headroom rather than after.
2. **Correct byte parsing.** The old helper inlined a `* 1024` (count-objects
   reports KiB). It is now `parse_count_objects_kib_field_bytes`, shared by
   `parse_count_objects_garbage_bytes` and the new
   `parse_count_objects_loose_bytes`, with a unit test pinning the KiB→byte
   conversion for both fields.
3. **Trigger selection is pure and tested.** `auto_gc_trigger(...)` decides
   between `Garbage`, `LooseObjects`, or nothing, from measured bytes,
   configured thresholds and the previous attempt's state. Behavioral tests:
   - less than one threshold of *new* loose bytes after a successful repack
     must not repack again;
   - the 2-minute loose cooldown suppresses a high-frequency gc loop;
   - the garbage trigger keeps its independent 1-hour cooldown.
4. **Real GC path exercised, not mocked.** A `tokio` test builds a real repo
   through the shared fixture helper, asserts it has loose objects, runs
   `maybe_auto_gc(&repo, 0, 1)` (garbage trigger off, loose trigger at 1 byte)
   and asserts `count-objects` loose objects are `0` afterwards — i.e. plain
   `git gc` actually packed the reachable loose objects.
5. **Bounded cost, unchanged safety.** The existing SYNC-H3 properties are
   preserved: `run_git_with_timeout(..., 600s)`, plain `git gc` (Git's
   ordinary two-week prune grace retained — no `--prune=now`), never a ref
   rewrite. New bounded-ness on top:
   - `count-objects` is only probed for a repo at most once per minute
     (`AUTO_GC_LOOSE_PROBE_INTERVAL_SECS = 60`), so the per-pulse hot path does
     not grow a git child per cycle;
   - after a successful repack, another loose-triggered gc requires at least
     2 minutes **and** one further threshold of new loose bytes measured from
     the post-gc baseline (`loose_bytes_after_gc`), so a repo that packs down
     to a large steady state cannot loop;
   - a failed loose-triggered gc is retried no sooner than 15 minutes
     (`AUTO_GC_LOOSE_FAILURE_COOLDOWN_SECS`), where the garbage trigger keeps
     its 1-hour cooldown;
   - the post-gc baseline is only trusted when `count-objects` actually
     succeeded; otherwise the attempt is recorded as unsuccessful and falls
     into the failure cooldown instead of fabricating a baseline.
6. **Documentation corrected, not just extended.** AGENTS.md, the example
   policy, and the `SyncPolicy` doc comment all previously described the
   automatic path as `git gc --prune=now`. That has been wrong since the
   v0.113.2 SYNC-H3 fix (the audit R3-L16 note records the same drift). They
   now describe plain `git gc` with the retained prune grace, plus the new
   loose trigger.
7. **Per-repo story decided.** `auto_gc_loose_threshold_bytes` is listed in
   `OVERRIDE_COVERAGE_GLOBAL_ONLY` alongside the garbage threshold, so the
   `test_repo_override_field_coverage_tripwire` contract records the decision
   rather than letting the field arrive half-wired.

## Live shape this addresses (read-only, 2026-10-29)

`git count-objects -v` on watched checkouts, before any repair deployment:

| repo | loose objects | loose size | pack size |
|---|---|---|---|
| `browser-extensions-shared` | 9181 | 740.6 MiB | 4269.9 MiB |
| `dracon-utilities/dracon-system` | 5775 | 89.1 MiB | 1.4 MiB |
| `dracon-utilities/dracon-warden` | 3286 | 42.7 MiB | 2.1 MiB |
| `dracon-utilities/dracon-sync` | 2135 | 19.2 MiB | 16.2 MiB |
| `dracon-platform` | 2801 | 19.5 MiB | 8084.9 MiB |

`browser-extensions-shared` is above the new 64 MiB default with 740.6 MiB of
reachable loose objects, and `dracon-system` is above it too (89.1 MiB). Under
the old configuration neither repo was eligible for automatic gc: their
`size-garbage` is small relative to 2 GiB, so the daemon would have kept
accumulating uncompressed reachable copies — the exact precondition that made
polis un-pushable.

## Scope honesty

- This report establishes the repair, its behavioral tests and the live object
  shapes. It does **not** by itself establish that the deployed daemon has
  repacked those repos; that requires the deployment and observation steps.
- No repository history was rewritten, no ref deleted, no remote reconfigured,
  and no manual `gc` was run for this repair. The single polis repack recorded
  in the sibling report predates it and went through `dracon-sync maintenance`.
- The garbage trigger is unchanged in intent and cooldown; only the shared
  parser and the state bookkeeping it now shares with the loose trigger are new.
