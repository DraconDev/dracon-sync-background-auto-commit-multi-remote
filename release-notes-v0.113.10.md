# dracon-sync v0.113.10 — github-push guard delta measurement + stale-branch janitor

> **Date**: 2026-07-29
> **Trigger**: the 2026-07-29 stale-backup-branch cleanup
> (`docs/design/stale-backup-branch-cleanup-2026-07-29.md`) exposed that
> junk-runner's ❌ CONCERN was a false positive: the guard measured the
> whole branch's *uncompressed* blob sum (3.79 GiB) while github would
> actually receive a **14.77 MiB** compressed delta. Operator approved
> both fixes as one release.

## 1. `github_pack_too_large` — delta-vs-remote with compressed second chance

The github-push guard's slow path (fires only when `.git` ≥ 2 GiB) no
longer measures the whole pushed branch's uncompressed blob sum. It now
measures what github would **actually receive** on the next push:

1. **Delta** — per github-host remote (from local `git config`; no
   network), the objects on the pushed branch the remote does not
   already have: `rev-list --objects <branch> --not <remote-tip>`.
2. **Compressed second chance** — when the uncompressed delta exceeds
   the 2 GiB limit, the same object set is streamed through
   `git pack-objects --stdout` and counted (stream-counted, never
   buffered; 600s ceiling). Github's limit applies to the compressed
   pack — highly compressible content clears here, incompressible
   content does not.

Safety degradations (all toward the conservative direction):

- **No github remote configured** → whole branch (the daemon
  auto-creates the github repo on first push; fresh remote = whole
  branch ships).
- **Missing or non-ancestor tracking tip** (rewound/recreated remote +
  stale local ref) → whole branch. Trusting a non-ancestor tip would
  under-estimate — the unsafe direction.
- **Multiple github remotes** → worst case (max).
- **Pack-generation timeout/error** → keep the uncompressed figure
  (already ≥ limit, so "too big").
- **Detached HEAD / git errors** → conservative whole-`.git` fallback
  (unchanged).

The `.git` < 2 GiB fast path is unchanged — sound under compressed
semantics (a subset pack never exceeds the whole store).

### Measured impact (fleet, 2026-07-29)

| Repo | Old measure | New measure | Verdict |
|---|---:|---:|---|
| junk-runner | 3.79 GiB (whole branch, uncompressed) | 14.77 MiB (delta, compressed) | ❌ CONCERN → ✅ clears |
| deathrun (July) | 2.85 GiB PNGs | would stay ~2.85 GiB (incompressible) | stays flagged |
| capture-anime-girls | ~2.5 GiB PNGs | incompressible | stays ❌ CONCERN |
| dracon-platform | whole-branch rev-list + cat-file every sync cycle | delta only | cheaper steady state |

## 2. `auto_prune_stale_backup_branches` — opt-in janitor

New policy field (default `false`). When enabled, a daily per-repo pass:

1. Collects stale **daemon-created** local branches
   (`backup/pre-sync-largeblob-fix-*`, `daemon-standalone` — never
   `preserve/*` or operator-named branches) and **orphaned
   remote-tracking refs** (`refs/remotes/<removed-remote>/*` — the
   deathrun `restore/*` case pinned 2 GiB for a week).
2. Bundles ALL candidates into `<backup_dir>/auto-prune/<repo>-<ts>.bundle`
   and verifies it. **Any bundle failure aborts the pass — nothing is
   deleted.**
3. Deletes the local refs, `log_warn!`-ing each with repo, ref, tip,
   and bundle path (the journal becomes the operator-review trail that
   AGENTS.md assigns to new `backup/*` branches).
4. Deletes the remote copy on any configured remote whose tracking tip
   equals the bundled local tip (mismatch → skip; remote's default-HEAD
   branch → skip), injecting `DRACON_ALLOW_REWRITE=1` into that one
   push command's environment — the sanctioned narrow exception to the
   no-auto-rewrite policy, itself gated behind this opt-in.

Requires `backup_dir` (already set fleet-wide to
`/home/dracon/dracon/backups`).

## Tests

- 7 new guard tests (fixture repos, limit-parameterized core):
  empty-delta clears, missing/non-ancestor/fresh-remote whole-branch,
  worst-of-multiple-remotes, compressible second-chance clears,
  incompressible stays flagged.
- 5 new janitor tests (fixture repo + bare origin): disabled no-op,
  full flow (bundle + local + remote deletion, `preserve/*` untouched),
  tip-mismatch remote skip, orphan-ref pruning, bundle-failure abort.
- **866 daemon tests total** (was 854 in v0.113.9); full workspace
  suite + `cargo clippy --workspace --locked -- -D warnings` clean.

## Upgrade notes

- The guard change is purely in measurement; repos correctly flagged
  before (incompressible over-limit) stay flagged.
- To enable the janitor: `auto_prune_stale_backup_branches = true` in
  the global `~/.dracon/utilities/sync/dracon-sync.toml` (enabled
  fleet-wide with this release).
- Design docs: `docs/design/stale-backup-branch-cleanup-2026-07-29.md`
  (both features), `docs/design/pack-size-concern-2026-07-28.md`
  (semantics-update header).
