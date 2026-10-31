# Handling over-large repos: the 6.53 GiB gap, closed (2026-10-31)

## What the platform already does (verified, four layers)

1. **Prevent** — binaries live in the OVH bucket, so history stops growing.
   Verified live: music audio `0` tracked, game builds `0` tracked, book
   covers out of git (only `chapters/*.md` text remains). Public GETs are
   byte-exact: `dracon-media` mp3 → 200 / 3,346,530 B; `dracon-books` cover →
   200 / 696,685 B (exactly the manifest's declared size); `dracon-games`
   builds → 200. The `music/` assetPrefix is enforced (same key without it →
   403).
2. **Block** — the fail-closed bucket guard with per-repo nested budgets.
   This is what stopped `polis` (over by 600,621 B).
3. **Degrade** — `GITHUB_PACK_LIMIT_BYTES = 2 GiB`, measured against **the
   delta GitHub would receive**, not total repo size. `ai-auto-writer` is
   7.23 GiB reachable yet correctly reports `pack_too_large: false`, because
   `all_github_remotes_converged()` short-circuits to basis 0 — nothing ships.
   The verdict cache is keyed on remote tips, so a "clear" verdict is never
   reused after the tip moves. (I initially suspected that cache was stale at
   37.7 h; that suspicion was wrong and is not claimed here.)
4. **Cut** — sanctioned slimming. Done twice: polis 2.18 → 0.88 GiB with its
   tip tree byte-identical, under a verified bundle.

## The gap: nothing reclaimed unreachable-in-pack bloat

`maybe_auto_gc` triggered only on `size-garbage` (LOOSE unreachable) or
`size` (loose objects). Unreachable objects held **inside a pack** are
neither, so a repo can carry GiB of dead weight and report

```
garbage: 0    size-garbage: 0
```

Live evidence — `dracon-platform`:

| | value |
|---|---|
| `size-pack` | 8,278,964 KiB = **7.94 GiB** |
| reachable (`rev-list --objects --all` → `pack-objects`) | **1.37 GiB** |
| stranded inside packs | **6.53 GiB** |
| `garbage` / `size-garbage` | 0 / 0 |
| auto-gc runs in 7 days | **0** |

The stranded content was the pre-slim book-cover history (26 of the top 40
unreachable blobs carried PNG magic). A verified complete-history bundle
already existed at `~/dracon/backups/dracon-platform-pre-slim-20260930.bundle`.

## Fix 1 — reclaim (operator action)

`dracon-sync maintenance -- git -C …/dracon-platform gc --prune=now`

| | before | after |
|---|---|---|
| `size-pack` | 7.94 GiB | **1.37 GiB** |
| reachable | 1.37 GiB | 1.37 GiB (unchanged) |
| gitdir | 31.28 GiB | 24.71 GiB |

Verified after: `rev-list --objects --all --missing=print` → **zero missing**;
all three refs converged at `ad65ba272a90`.

> A caveat about my own measurement: the first `git fsck` runs printed
> `missing commit <tip>`, which looked like corruption. It was fsck racing the
> daemon's per-second commits — the "missing" SHA was simply the tip being
> written at that instant. Quiesced, `fsck` is clean and the authoritative
> `--missing=print` check reports nothing.

Fleet-wide, platform was the **only** case: `browser-extensions-shared` is
5.69 GiB `size-pack` and 5.69 GiB reachable — 0 stranded.

## Fix 2 — make it self-healing (`auto_gc_pack_growth_bytes`)

New third trigger in `maybe_auto_gc`:

* reads `size-pack` (`parse_count_objects_pack_bytes`, KiB like the other
  size fields);
* compares against the `size-pack` recorded after the last **successful** gc;
* fires when growth ≥ `auto_gc_pack_growth_bytes` (default **1 GiB**, 0
  disables) and the 24 h cooldown has elapsed;
* runs plain `git gc`, so Git's **two-week prune grace is retained** — it
  cannot lose recoverable work, only stop dead weight accumulating.

Two deliberate safety properties:

* **No baseline ⇒ disarmed.** Comparing against an absent baseline would
  report "grew by the whole repository" on the first observation after every
  daemon restart and repack every large repo immediately.
* **First observation ARMS the baseline without gcing.** Without this, a
  quiet repo that never trips the garbage/loose triggers would have no
  baseline forever and the new trigger could never fire — exactly the case it
  exists to catch. Arming costs one `count-objects` call and never fabricates
  a baseline from a pre-gc reading.

Wiring: `auto_gc_pack_growth_bytes` on `SyncPolicy`, `default_auto_gc_pack_growth_bytes`
(1 GiB), listed in `OVERRIDE_COVERAGE_GLOBAL_ONLY` (both policy tripwires
pass), documented in `dracon-sync.example.toml`, threaded through the
`sync.rs` call site.

## Verification

```
cargo test --workspace --locked      44 suites, 2930 passed / 0 failed
cargo build --release --locked       exit 0
cargo clippy --workspace --all-targets -- -D warnings   exit 0
cargo deny check                     advisories/bans/licenses/sources ok
```

One earlier full-suite run failed
`integrated_failure_recovery_harness` under load; it passes in isolation and
the unchanged re-run is 0 failed (the documented load-flake class — retried,
not narrowed).

**Negative controls** (disposable, then reverted):

* Neutralising the pack-growth predicate → `test_pack_growth_trigger_fires_only_on_real_growth`
  FAILED with `left: None, right: Some(PackGrowth)`.
* Disabling the arming block → `test_maybe_auto_gc_arms_pack_growth_baseline_without_gcing`
  FAILED.

Deployed by the sanctioned shape (atomic replacement, pidfd SIGTERM,
`Restart=always`; `systemctl stop` never used). Loaded == installed == built
(`e9d39acefbe6858e`).

> The deploy helper itself had a bug worth recording: it read
> `/proc/<newpid>/exe` inside its verification loop and treated a
> `PermissionError` as failure, which triggered its rollback and silently
> reverted a good deploy. Fixed to treat unreadable `/proc` entries as "not
> ready yet".

## Still open (needs an idle window — deliberately NOT forced)

`ai-auto-writer` currently holds **41 spool dirs / 5.05 GiB**. I measured
content redundancy rather than trusting age:

* 95,622 unique objects across all spools
* **95,613 already in the main object store**
* **9 exist only in spools** — 8 trees + 2 blobs, **~92 KB total**

So ~5 GiB is provably duplicated and ~92 KB is the only irreplaceable content.
It was **not** touched: the repo is in-flight (`phase=working`, `admitted_secs=133`)
with live producers (`book_daemon.py --watch`, two `ai-auto-writer write …`).
A live epoch may be using a spool, which is why `reap_abandoned_spools` fails
closed by design.

History slimming for `ai-auto-writer` (7.23 GiB reachable) is a materially
different decision from polis: its large classes are **shipped product**
(`epub` 1.19 GiB, `cover.png` 0.77 GiB), so excising them changes the working
tree rather than only history. That needs an explicit product classification
before any rewrite.
