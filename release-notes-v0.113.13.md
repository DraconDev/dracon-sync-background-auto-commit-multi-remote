# dracon-sync v0.113.13 — table v2 + exclusion-aware dirty semantics

> **Date**: 2026-07-29
> **Source**: operator feedback on the v0.113.8 table — "USED tells us
> nothing that ACTIVITY doesn't, the commits per time can have columns
> too, and we should look into the dirties".

## The false-WARN fix (the "dirties" investigation)

`dracon-sync repos` showed junk-runner as `⏳ dirty 2h · 1 mod` + 🟡
WARN and dracon-platform the same — forever. Investigation found **one
file** behind both: junk-runner's `.pi-glla/active.jsonl`, which is on
that repo's `auto_commit_exclude_patterns` (added 2026-07-28 after its
412 historical copies = 2.884 GiB nearly killed github sync). The
daemon was behaving perfectly — syncing every ~60s, correctly refusing
to commit the excluded file. The **report** was wrong: its dirty counts
came from raw dracon-git status, which knows nothing about exclusions,
so an intentional exclusion looked like a permanent stall. The parent's
WARN was the same file seen through the submodule gitlink (worktree
dirt, unchanged pointer).

The report now re-derives dirty counts from `git status --porcelain -z
--ignore-submodules=dirty` (tracked-dirty repos only; fast, no
clean-filter pass) classified with the same patterns the sync loop
stages by:

- **Excluded-only repos** show `🟢 synced 2m · 1 excl` — visible by
  policy, never alarming, and **cannot** escalate to WARN.
- **Gitlink SHA drift** still counts as parent dirt (the daemon
  advances gitlinks); only worktree-only submodule dirt is ignored.
- WARN requires **daemon-committable** dirt stalling — as it should.

Verified live: the fleet went from `🟡 WARN 2` (both false) to
`🟡 WARN 0`, with both repos showing `synced · 1 excl`.

## Table v2

- **USED column dropped** — it duplicated ACTIVITY's
  dirty/synced/idle/cold tiering.
- **COMMITS split into 1H | 6H | 24H columns** — bright = active
  window, grey = zero; no more `27/49/206` mental parsing.
- Legend updated to match (USED line removed, `N excl` + new columns
  documented).

## Tests

6 new classifier tests (excluded tracked pattern, committable
mod/staged, untracked semantics, **submodule worktree-dirt excluded vs
gitlink drift committable**, porcelain rename parsing, excl marker in
the activity label); header-fit / narrow-terminal / legend tests
updated to the new column set. 1192 workspace tests green, clippy
`-D warnings` clean. Released via the hardened `release.sh` — third
consecutive fully unattended release.
