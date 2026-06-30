# Submodule sync with `dracon-sync`

> Status (2026-06-30): the daemon syncs each subrepo correctly on its
> own (per-repo commit + push to its own remotes) but does **not** yet
> auto-propagate the resulting gitlink update back into the parent.
> This document explains what works, what doesn't, and how to recover
> manually when you see a stuck parent.

If you are seeing `MOD = 1` on a row in `dracon-sync repos` whose
`LAST COMMIT` references a `.pi/...` path and the parent path is
something like `rust-ai-web-auto/`, that single `MOD` is almost always
the stale submodule pointer — see [the stuck-state recipe](#recovering-when-the-parent-shows-m-submodule).

## TL;DR

1. The daemon runs `sync_repo()` on every watched repo independently.
   For a parent + submodule setup, the parent repo and the submodule
   repo are both watched; each one has its own remotes (github,
   gitlab, codeberg) and its own auto-commit loop.
2. The submodule's commits land on the submodule's remotes
   automatically. Good.
3. The parent's working-tree gitlink entry shows `M <submodule>`
   after the submodule HEAD moves. The daemon picks the entry up but
   **silently drops it** during `stage_existing_files` (because the
   entry's path is a directory with its own `.git/` — see the open
   bug below). The pointer stays stale on the parent.
4. Manual recovery is one command. See [recovery](#recovering-when-the-parent-shows-m-submodule).

## How a parent + submodule pair flows through the daemon

### Repo discovery

The daemon watches every directory under `~/Dev/` (per its policy) and
records each one in the report. Parents and submodules show up as
**separate rows** in `dracon-sync repos`. Examples:

| row | repo | role |
|---|---|---|
| 4 | `web-auto` | parent (`rust-ai-web-auto` is a sibling subrepo, no `.gitmodules`) |
| 6 | `rust-ai-web-auto` | child (own remotes: github, gitlab, codeberg) |
| 1 | `dracon-platform` | parent (10 submodules declared in `.gitmodules`) |
| 2..12 | `hegemon`, `polis`, … | each a child of `dracon-platform` |

### Per-repo sync loop (`sync_repo`)

For **every** watched repo, on each daemon cycle:

1. `compute_diff_entries` collects modified tracked files + untracked
   files + submodule-pointer modifications.
2. `is_gitlink_unchanged(repo, &path)` filters out gitlinks where the
   submodule's HEAD already matches the parent's tracked SHA — those
   are "submodule dirty internally, but no pointer change to commit".
3. `should_stage_entry` partitions the surviving entries into
   `to_stage` (will be staged + committed) and `to_restore` (clean/smudge
   filter caught them — restored, not staged).
4. `stage_commit_and_push` runs `git add -A -- <staged_paths>` and
   commits with the message format `N file(s) in <dirs>`. The push
   uses refspecs to the configured remotes.

### When does a submodule pointer change show up?

A submodule pointer changes from the parent's perspective when:

- The submodule's working tree has commits not yet pushed
  (`.git/refs/heads/main` differs from any remote `refs/heads/main`),
  and the daemon runs `sync_repo` on the submodule first. The
  submodule's auto-commit loop creates a new local commit and pushes
  it; afterwards, the **submodule's** `HEAD` SHA differs from what
  the **parent** still has cached.
- Or you edit the submodule locally (`git checkout --detach`,
  `git pull`, etc.) without committing in the parent.

In either case, the parent sees ` M <submodule>` in
`git status --porcelain` because its index entry disagrees with the
worktree gitlink.

### Where the daemon currently drops the gitlink

`stage_existing_files` (in `dracon-sync/src/sync.rs`) is responsible
for recursing into the staged paths and producing a `git add -A
-- <paths>` command line. It has a deliberate skip for any path
whose `.git` exists — that skip is what is dropping the gitlink
update:

```rust
// CHANGED 2026-06-21 (goal 29144c2c): skip submodules.
// A git submodule has a `.git` FILE (not a directory) containing
// a gitdir pointer like `gitdir: ../.git/modules/<name>`. ...
//
// CHANGED 2026-06-30 (goal `mr0rim9u-lzzfv9`):
// broadened `.git` check from `is_file()` to `exists()` so it
// ALSO catches the case where a sibling subrepo has its own
// real `.git/` directory.
let full_dot_git = cp.join(".git");
if full_dot_git.exists() { continue; }     // <-- DROPS gitlink update
```

That `continue` is correct when the entry is an untracked directory
that happens to contain a real `.git/` (don't descend into the
subrepo's working tree) but it's incorrect when the entry is itself
the submodule pointer that needs to be staged. There is no current
distinction: any path that is itself a directory with a `.git` is
treated as a skip-target.

### What about `should_stage_entry`?

`should_stage_entry` looks at the entry's path against
`exclude_dir_names`, `exclude_file_patterns`, and the size cap
(`max_stage_file_bytes`). A bare gitlink path (`rust-ai-web-auto`)
passes all of those because:

- It's not in `exclude_dir_names` (those are `target`,
  `node_modules`, `.cache`, `.direnv`, `.venv`, `dist`, `build`,
  `archives`, `.tmp-*`).
- It's not in `exclude_file_patterns`.
- A bare path is 0 bytes in size and is not even a regular file —
  the size-check branch of `should_stage_entry` only triggers for
  `is_file() == true`, and a gitlink path is a directory.

So the entry would pass through to `stage_existing_files` if the
submodule-skip guard weren't there.

## Status: parent gitlink propagation is manual today

As of goal `mr0rim9u-lzzfv9` (2026-06-30), the daemon:

- ✅ Syncs each subrepo to its own remotes (commits + push).
- ✅ Filters out gitlink entries whose SHA already matches the
  parent (so it doesn't generate noise commits for subrepos whose
  internal worktree is dirty).
- ❌ Does **not** auto-stage + commit a parent gitlink update.

The parent's `dracon-sync repos` row will show `MOD = 1` and
`STATE = 🟠 dirty` (or `🟡 committing`) until you run a manual
recovery command.

## Recovering when the parent shows `M <submodule>`

Pick one of these. They all converge to the same outcome — a parent
commit that updates the gitlink pointer — but they differ in how
much they touch the submodule first.

### Option A — manual one-line commit

The cleanest, most local fix. Use this when the submodule is already
synced to wherever you want it.

```bash
cd /home/dracon/Dev/<parent>
git add <submodule>
git commit -m "chore(<submodule>): sync to $(git -C <submodule> rev-parse --short HEAD)"
git push                  # or: daemon will sync on next cycle if origin configured
```

What this does:

- `git add <submodule>` updates the parent's index entry from the
  old SHA to the submodule's current HEAD. No recursion into the
  submodule work tree (git treats the path as a 160000 gitlink).
- `git commit` records the pointer update with whatever message
  format you prefer (the daemon's own format is `N file(s) in …` —
  you can mimic that or use your own; the daemon auto-stage would
  use the format if it were implemented).

This is enough to clear `MOD = 1` on the parent.

### Option B — `git submodule update --remote`

```bash
cd /home/dracon/Dev/<parent>
git submodule update --remote <submodule>
```

This fetches the submodule's configured remote, checks out the
upstream branch's tip, and updates the parent's index in one step.
Useful when the submodule has remote changes that you want to land
on the parent without first pushing the submodule from this machine.

When this works depends on the submodule's `branch = ...`
configuration in `.gitmodules` (or the global `submodule.<name>.branch`
config). For unconfigured submodules (the
`web-auto/rust-ai-web-auto` case — no `.gitmodules`, just a bare
gitlink in the parent), this command falls back to the
superproject's recorded SHA and is a no-op.

### Option C — pull then add

```bash
cd /home/dracon/Dev/<parent>
git -C <submodule> pull --ff-only
git add <submodule>
git commit -m "chore(<submodule>): sync to $(git -C <submodule> rev-parse --short HEAD)"
```

This is Option A but with an explicit ff-only pull inside the
submodule first. Use it when you suspect the local submodule is
behind the submodule's remote.

### When to use which

| situation | use |
|---|---|
| Submodule is committed + pushed, parent is just unaware | A |
| Submodule has unpushed commits that were not daemon-synced (e.g. manual `git commit`) | A after `git -C <submodule> push` or C |
| Parent has `.gitmodules` and you want the latest submodule `branch` tip | B |
| Submodule is missing from disk (clone issue) | B (auto-clones) |

## Divergent subrepo with `+N / -M`

Some parents show very large divergence — e.g. `dracon-platform` was
shown as `+251 / -6610` at one point. The +N is the local commits
the daemon has staged (often a single `regenerate_facade_repos.py`
run that touched `web/` once). The -M is the remote divergence: the
parent's `master`/`main` on github/gitlab/codeberg is ahead of the
local working copy.

This is NOT a submodule issue per se — it's a normal local/behind
remote divergence. Resolution:

1. `git fetch --all` inside the parent.
2. `dracon-sync sync-now /home/dracon/Dev/<parent>` and check
   `dracon-sync repos` again. The daemon will fast-forward or pull
   as appropriate (see `auto_pull_merge`).
3. If divergence is real (you have local commits the remote doesn't
   know about and the remote has commits you don't have), resolve
   manually:
   - `git status` — see what is locally modified.
   - `git log @{u}..HEAD` — see what's on local ahead.
   - `git log HEAD..@{u}` — see what's on remote ahead.
   - `git pull --rebase` if you trust the remote history, or
     `git pull --no-rebase` for a merge commit.

After divergence is resolved, the `+N / -M` should drop to `+0 / -0`
and any leftover `MOD = N` entries are now the gitlink updates
described above — apply Option A.

## Untracked entries under `web/games/...` in `dracon-platform`

`dracon-platform`'s submodules sit at `web/games/{wip,released}/<name>/`
. When you see `?` (untracked) entries for one of them in
`dracon-platform`'s `git status`, that is the parent's index entry
disagreeing with the on-disk subrepo:

```
 M web/games/wip/hegemon       # parent's gitlink SHA differs from submodule HEAD
?? web/games/wip/polis         # workspace missing (no checkout), but parent still tracks pointer
```

`?? web/games/wip/polis` is the parent's pointer being unable to
find a valid worktree (e.g. you removed it with `rm -rf`). To
restore:

```bash
cd /home/dracon/Dev/dracon-platform
git submodule update --init web/games/wip/polis
```

Or, more permissively, to fetch and checkout ALL declared
submodules:

```bash
cd /home/dracon/Dev/dracon-platform
git submodule update --init --recursive
```

## Open bug: daemon does not auto-update parent gitlink

**Symptom**: a parent's `dracon-sync repos` row shows `MOD = 1`
*indefinitely* after a successful submodule sync cycle. A
`dracon-sync sync-now <parent>` run produces no new commit. The
parent never propagates the new submodule SHA to its own remotes
unless you do it manually.

**Root cause**: `stage_existing_files` in
`dracon-sync/src/sync.rs` (and its inner recursion branch) skip any
path whose `.git` exists. That guard is correct for an UNTRACKED
directory that happens to be a subrepo on disk (don't descend into
`.git/`), but it incorrectly skips the parent's gitlink entry
itself when git treats that entry as a directory.

**Proposed fix (not yet implemented)**: in `stage_commit_and_push`,
when the entry is a directory whose `.git` exists, instead of
running `git add -A -- <path>` (which recursively descends), run
`git add -- <path>` (which on a gitlink path only updates the
index pointer, no recursion). That single-token switch in the
`git add` invocation is enough.

**Why this is a `goal` rather than a one-line patch**: the
`should_stage_entry` partition + `stage_existing_files` recursion
+ `to_restore` restore flow assume `to_stage` is a list of paths
that all need to expand into files. Adding "gitlink doesn't
expand" requires either a separate path in the daemon (so the
`git add` command line is split between recursing and
non-recursing invocations) or a tweak to `stage_existing_files` to
detect a gitlink entry and emit a different sub-command. Either
option benefits from a design doc of its own; this document lays
out the user-facing behaviour and leaves the implementation as a
follow-up.

## Quick reference: code locations

| What | File | Lines | Notes |
|---|---|---|---|
| Per-repo sync entry | `dracon-sync/src/sync.rs` | 2660 (`sync_repo`), 2676 (…with `_ahead_since`) | |
| Gitlink-filter helper | `dracon-sync/src/exclude.rs` | 585 (`is_gitlink_unchanged`) | |
| Daemon submodule-skip guard (the bug) | `dracon-sync/src/sync.rs` | top-level ~706, inner-recursion ~822 | Doc-comment refs goal `29144c2c` (orig) and `mr0rim9u-lzzfv9` (broadening) |
| Status accounting | `dracon-git/src/lib.rs` | 100 (`get_status`), 188 (`get_diff_entries`) | Correct for gitlinks today |
| Per-repo policy config | `dracon-utilities/dracon-sync/.dracon/dracon-sync.toml` | per-repo TOML | Optional `auto_commit_exclude_patterns` for per-repo overrides |

## Related issues / ongoing work

- **Open bug**: parent gitlink propagation. See [Open bug](#open-bug-daemon-does-not-auto-update-parent-gitlink).
- **Convergent recent work**:
  - Goal `29144c2c` (2026-06-21): added the original submodule-skip
    guard in `stage_existing_files` to stop the daemon from `git
    add`-ing into a `.git` submodule pointer file.
  - Goal `mr0rim9u-lzzfv9` (2026-06-30): broadened that guard from
    `is_file()` to `exists()` so it catches the nested-subrepo case
    (real `.git/` directory, not just submodule pointer file).
    This is what unblocked web-auto's `rust-ai-web-auto/` from
    spamming `Pathspec is in submodule` errors. It did NOT fix the
    gitlink propagation gap.
- **Where submodules live in the watch graph**: `dracon-sync` walks
  `~/Dev/` recursively and registers both parents and children as
  separate watched repos. Submodules without `.gitmodules` (i.e.
  "broken" submodules from the parent config's perspective) are
  still registered correctly because the daemon treats anything
  with a `.git/` as its own repo.
