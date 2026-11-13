# Spool reclaim: crash-abandoned producers and `git rm` reports (2026-11-13)

Related: `spool-reclaim-throughput-2026-11-12.md` (refusal memo, `-q` on the
epoch `git rm`), `push-unit-isolation-2026-11-13.md` (the OOM loop).

## Problem

After ~60 OOM kills on 2026-11-13, `health` reported 882 aged spools
(2.5 GiB). The live read-only preview proved only **34** removable across
dracon-platform, `~/.dracon` and browser-extensions-shared. Two refusal classes,
both permanent by construction, held almost all of the rest:

1. **No settled marker** (`source inactivity is not proven`). A producer
   killed before settlement can never write `reclaim-settled.json`, and the
   sweeper must never manufacture it. Every crash therefore leaked its spool
   forever.
2. **`producer endpoint holds payload that is not a name`**. 415 endpoints
   held only `rm '<path>'`: the stdout of the daemon's own un-quieted
   `git rm`, fixed at the source on 2026-11-12 (`-q`), but the existing
   spools stayed unprovable.

## Change

- `ReclaimOwner::proves_inactivity` = settled marker **or**
  `proves_abandonment`, which requires all of:
  1. the reclaimer holds the owner lock (the lease holds it from creation to
     settlement; `SpoolLease::acquire` uses `create_new`, so a spool can never
     be re-leased or resumed);
  2. no live producer scope for it in this process;
  3. no directory or entry modified for `ABANDONED_AFTER` = 24h (producer
     units are capped at `RuntimeMaxSec` ≤ 3600 s);
  4. `/proc` shows no process with a descriptor, cwd, root or Git environment
     inside it (`epoch_spool_liveness.rs`, which existed since 2026-11-01 but
     was never mounted).

  This establishes quiescence only. Every content proof is unchanged.
- `producer_names` accepts lines that are exactly `rm '<path>'`; the removal
  they report is covered by the entry-by-entry index proof. Failure
  diagnostics (`fatal:`, warden stderr) are still preserved, as evidence of
  why the epoch failed.
- Logging: refusals are counted by reason in the pass summary; per-spool lines
  only under `DRACON_SYNC_DEBUG=1` (was ~55,000 journal lines/day).

## Verification

- Tests: abandoned + redundant → removed; abandoned + unpublished endpoint →
  preserved; recently changed entry → preserved; live child process with cwd
  in the spool → preserved; held lease → preserved; `rm` report only →
  removed; `rm` mixed with payload / `fatal:` / malformed → preserved.
  Mutations: dropping the `/proc` check fails the live-process test; dropping
  the age check fails the recent-change test.
- Live preview (read-only): **34 → 324** provable (dracon-platform 11 → 65,
  `~/.dracon` 23 → 114, browser-extensions-shared 0 → 145).
- Independent re-proof (`git rev-list --objects --remotes` set vs. every blob
  in the spool index, every object in its retained pack indexes, every object
  id in its endpoints): **325 checked, 0 disagreements**. Negative control: the
  same script flags **55/55** spools the reclaimer refused for unpublished
  content, so it is not vacuous.
