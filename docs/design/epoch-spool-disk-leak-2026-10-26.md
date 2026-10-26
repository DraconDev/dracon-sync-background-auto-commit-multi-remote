# Epoch spool leak: 101 GiB in a git dir, and the fleet-wide disk wedge

Date: 2026-10-26
Status: three independent leaks repaired (`dracon-sync` 0.113.96), historic
litter reclaimed, deployed and observed at steady state.

## Symptom

The root filesystem (907 GiB) hit **100% full**. Every watched repository
started failing with:

```
initialize private staging refused: fatal: sha1 file
'/home/dracon/.dracon/.git/dracon-staging-96hLtK/index.lock' write error.
Out of diskspace
```
```
sync failed for /home/dracon/Dev/browser-extensions-shared: No space left on
device (os error 28) at path ".../.git/dracon-staging-rKiuuu"
```

`browser-extensions-shared/.git` had itself grown to **115 GiB**, of which
**101 GiB was 141 `dracon-staging-*` directories**, each ~875 MiB.

## Root cause: retention with no release and no sweep

`sync/staged_epoch.rs` allocates a private staging view per sync epoch:

```rust
let mut builder = tempfile::Builder::new();
builder.prefix("dracon-staging-");
let directory = builder.tempdir_in(directory)?.keep();   // <gitdir>/dracon-staging-XXXXXX
```

then `read-tree`s the **entire HEAD tree** into `directory/index` and, in
`Staging::finish`, writes an independent object pack of the candidate
payloads to `directory/retained-<sha>.pack`.

The retention is deliberate and correct for one epoch:

```rust
// Keep the independent pack after installation: a later actual-index
// replacement must not make the proven candidate collectible by prune.
retained: Option<PathBuf>,
```

The defect is that **nothing ever released it**:

- `Staging::finish()` returns the `Epoch`; the directory survives publication.
- `Epoch::commit()` consumes `self` and still never removes it.
- Neither `Epoch` nor `Staging` implements `Drop`, so every early return,
  every `EpochConflict` refusal, every deadline expiry and every successful
  commit left a full spool behind.
- There was **no sweep and no size bound**. The only bound on the leak was
  `git gc`, which never sees these directories (they are not objects).

So the leak rate was "one full-tree index + one full pack **per sync cycle**".
On a repository with a large history that is ~875 MiB, and
browser-extensions-shared runs many cycles per hour. At ~5-minute cadence that
is ~250 GiB/day — the disk was always going to fill.

A full disk then makes recovery **impossible**, because the next epoch cannot
even create its `index.lock`: this is a convergence defect, not merely
housekeeping. That is why 8 repositories were stuck (STALLED/RETRYING) rather
than failing one at a time.

### A second instance of the same class

`sync.rs:5031` (`run_sealed_commit_hooks`) does the same thing at a smaller
scale:

```rust
let directory = tempfile::Builder::new()
    .prefix("dracon-epoch-message-")
    .tempdir_in(git_dir)?;
...
if let Err(error) = hooks {
    let retained = directory.keep();   // retained on refusal, never released
```

65 orphans of this class were found. Only ~520 KiB total, so it was not the
disk driver, but it is the identical "retain on refusal, never release" shape
and is reaped by the same sweep.

## Repair

New module `src/git/epoch_spool.rs`.

1. **`SpoolLease`** — an RAII marker held by `Epoch` for its whole lifetime.
   While leased, the reaper skips the directory (consulted **twice** per
   candidate, before and after the age read, so a spool leased during the walk
   cannot be raced). The lease is dropped on **every** exit path — early
   return, refusal, deadline, success — which is exactly the release that was
   missing.
2. **`reap_abandoned_spools`** — the periodic janitor, wired into `sync.rs`
   beside `maybe_auto_gc` and `maybe_prune_stale_backup_branches` (i.e. post
   `check_conflict_state`, never in dry-run, never in `commit_only`). Per-repo
   hourly cooldown, so the steady state costs one directory scan per hour, not
   one per cycle.
3. **`sweep_epoch_spools`** + `dracon-sync repair spools [--apply]` — the
   operator-facing path for historic litter, dry-run by default.

Safety properties (all regression-tested in `epoch_spool::tests`):

| Property | Test |
| --- | --- |
| An aged, unleased spool is reclaimed | `abandoned_spool_is_reaped_and_fresh_one_is_kept` |
| A young spool survives | same |
| A **leased** spool is never deleted, even at 24h | `leased_spool_is_never_reaped_even_when_aged` |
| Releasing the lease makes an aged spool reapable | `released_spool_is_reapable_after_the_lease_drops` |
| A symlink named like a spool is never followed or removed | `symlinked_spool_like_entry_is_not_followed_or_removed` |
| Non-spool git dirs (`modules/`, `objects/`) are untouched even when ancient | `unrelated_directories_are_never_touched` |
| The sealed-message dir class is reaped too | `abandoned_sealed_message_dir_is_reaped` |
| `0` is NOT an opt-out (falls back to 3600s) | `zero_age_falls_back_to_the_safe_default_not_to_disabled` |
| The reaper's prefixes still match their producers | `spool_prefix_matches_the_staging_builder` |

`symlink_metadata` (not `metadata`) is used for every candidate, because
`remove_dir_all` through a symlink would delete outside the repository.

### Policy knob

`epoch_spool_max_age_secs` (default `3600`), both halves per the AGENTS.md
per-repo-knob rule (field + `default_epoch_spool_max_age_secs`) and declared
global-only in `OVERRIDE_COVERAGE_GLOBAL_ONLY`, matching
`auto_gc_garbage_threshold_bytes`. `0` is deliberately **not** an opt-out —
unlike the gc threshold — because an unbounded spool leak is a disk-exhaustion
defect rather than a tuning preference; a zeroed config still gets the 1h
floor.

## Historic litter reclaimed (2026-10-26)

`dracon-sync repair spools` (dry-run) reported **1818 directories / 83.30 GiB**
across the fleet, dominated by:

| Repository | Dirs | Bytes |
| --- | --- | --- |
| `~/.dracon` | 1286 | 32.25 GiB |
| `wip/deathrun` | 41 | 24.34 GiB |
| `wip/capture-anime-girls` | 42 | 17.27 GiB |
| `wip/freeport` | 42 | 4.20 GiB |
| `dracon-sync` | 230 | 1.79 GiB |
| `dracon-strategy` | 13 | 1.10 GiB |

`--apply` reclaimed **1819 dirs / 83.30 GiB**; root free space went from
**1.6 GiB (100% used)** to **183 GiB (79% used)**. 59 spools remained, all
younger than the 1h floor (i.e. plausibly live or just-created).

Verified on a disposable repo (`/tmp/spool-verify`) before the fleet run: a
3-hour-old `dracon-staging-OLD123` was reclaimed while a fresh
`dracon-staging-NEW456` was preserved.

## Two further leaks found while verifying the repair

Fixing the spool leak exposed that it was **not the only** unbounded disk
consumer. Measuring per-repo `.git` byte deltas after deploying the spool fix
still showed growth, and chasing it found two more causes.

### Leak 2: `tmp_pack_*` leftovers had no reclaimer at all

browser-extensions-shared held **23 `tmp_pack_*` files totalling 10.71 GiB**
in a repository whose real pack is only **3.32 GiB** — 76% of `.git` was
dangling. `git count-objects` reported all of it as `size-garbage`.

The existing `maybe_auto_gc` could never reclaim it, for two compounding
reasons:

1. **`git gc` does not delete recent `tmp_pack_*` files.** Verified
   empirically on a disposable repo: a fresh 1 MB `tmp_pack_*` survived both
   `git gc` and `git gc --quiet`, and was collected only by
   `git gc --prune=now`. (The 2-week prune grace is about unreachable
   *objects*; a tmp pack is an incomplete *packfile*.) So gc reported 10.71 GiB
   of garbage while being unable to remove it — the app was told there was
   garbage and given no way to act.
2. The codebase deliberately never uses `--prune=now` (SYNC-H3: it removes the
   2-week grace, risking the prune race and expiring the reflog safety net),
   and that decision is correct.

Fix: `git::sweep_stale_tmp_packs` removes only regular `tmp_pack_*` files in
the real `objects/pack` dir, only when untouched for the configured age floor,
never following symlinks. This is strictly safer than `--prune=now`: a tmp
pack holds no object any ref can reach, because a pack only becomes
authoritative when its rename to `pack-<sha>.pack` completes.

Result on the live fleet: browser-extensions-shared 23 files / **10.72 GiB
reclaimed**; dracon-strategy/ai-auto-writer **11.48 GiB** on top of a 47.29 GiB
gc. Both now sit at `size-garbage: 0 bytes`.

### Leak 3: two gates made the janitors skip exactly the affected repos

**3a — gc gated on "no staged intent".** The call site read

```rust
if !dry_run && !commit_only && !entry_epoch.as_ref().is_some_and(|e| e.has_intent())
```

so a repo with staged work skipped auto-gc. But a busy repo has staged intent on
*nearly every cycle* — the very condition that generates the garbage. The gate
was not a safety gate (`git gc` never reads or writes the index and never
touches the worktree), and the real guards — `!dry_run`, placement below
`check_conflict_state` so it never runs mid-merge/rebase/cherry-pick — are
preserved. Removed.

**3b — maintenance gated on `!commit_only`.** `commit_only` is what a repo
falls INTO after its push is declared stuck, and a push-stuck repo is precisely
the one leaking a tmp pack per failed push. browser-extensions-shared was being
skipped on every cycle because the stuck ledger had put it in commit-only mode,
so the sweeper could never heal the repo that needed it most. Reclaiming
dangling bytes creates no commits and touches no refs, so it is independent of
publication; the gate was removed and `!dry_run` remains.

After 3a+3b the same cycle logged:

```
🗑️ browser-extensions-shared has 10.72 GiB dangling garbage (> threshold 2.00 GiB) — running git gc
🗑️ gc done for browser-extensions-shared in 153.0s (reclaimed ~10.72 GiB garbage)
🗑️ browser-extensions-shared reclaimed 23 abandoned tmp_pack file(s) (10.72 GiB)
```

### Steady-state verification

Measured per-repo `.git` byte deltas over a 6-minute window after the final
deploy and backlog clear:

| Repository | Delta |
| --- | --- |
| `~/.dracon` | +29.1 MiB (legitimate commits) |
| every other watched repo | 0 (below the 1 MiB reporting floor) |

Spool count stayed at 2-3 while browser-extensions-shared committed roughly
once per minute, i.e. allocation and release are now balanced.
`browser-extensions-shared/.git` went from **115 GiB → 4.2 GiB**, and root free
space from **1.6 GiB (100%) → 158 GiB (82% used)**.

## Follow-up (not done here)

The leak is bounded now, but the **design** still allocates a full-tree index
plus an independent pack per cycle. Bounding the leak to one spool per hour is
correct and sufficient; removing the per-cycle full-tree copy (e.g. reusing one
spool per repo, or an incremental index) is a separate optimisation that should
not be bundled into a disk-exhaustion repair.
