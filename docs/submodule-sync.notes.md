# Submodule sync inspection notes (task doc-1)

Working notes for `dracon-sync/docs/submodule-sync.md`. These are raw
findings; the polished doc is the final deliverable.

## doc-1a — daemon submodule-aware code paths

All paths relative to `dracon-sync/src/`.

### `exclude::is_gitlink_unchanged` (exclude.rs:585-617)

```rust
pub(crate) fn is_gitlink_unchanged(repo: &Path, path: &Path) -> bool
```

Returns **true** when the path is a `160000` (gitlink) entry tracked by
parent AND `rev-parse HEAD` inside the submodule equals that tracked SHA.

Used to **filter out** gitlinks whose inner HEAD already matches the
parent pointer — i.e. the "dirty" state is just the submodule's own
working tree (not a pointer change that needs to be propagated to the
parent).

### `sync_repo` flow (sync.rs:2684+)

```rust
let DiffResult { status, entries, filter_only_cleared } =
    compute_diff_entries(&svc, repo).await?;

if !status.is_clean && policy.auto_commit {
    if ctx.backstop_active { return Ok(SyncOutcome::NothingToDo); }
    ...
    let (to_stage, to_restore): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .filter(|e| {
            if repo.join(&e.path).is_dir()
                && crate::exclude::is_gitlink_unchanged(repo, &e.path)
            {
                return false;          // <-- submodule HEAD == tracked; skip
            }
            ...
        })
        .partition(|e| should_stage_entry(...));
    if !to_stage.is_empty() {
        if let Some(outcome) = stage_commit_and_push(
            &svc, &mut ctx, &status, &to_stage, &to_restore).await?
        { return Ok(outcome); }
    }
}
```

So the flow is:

1. `compute_diff_entries` returns ALL dirty entries (modified tracked +
   untracked + gitlink-pointers whose SHA differs from the tracked
   commit).
2. The `is_gitlink_unchanged` filter drops entries where the submodule
   HEAD already matches. If `rev-parse` differs, the gitlink entry
   PASSES THROUGH to `should_stage_entry`.
3. `stage_existing_files` recurses INTO the entry, BUT — as of goal
   `mr0rim9u-lzzfv9` — the recursion SKIPS any subdir whose `.git`
   exists (file or directory). **That skip drops the gitlink update on
   the floor.**
4. The parent then shows `M <submodule>` forever; the daemon's
   `get_status` keeps reporting the gitlink delta as
   `modified_files +1` and AHEAD/BEHIND diverge.

### `stage_existing_files` submodule branch (sync.rs ~706)

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
if full_dot_git.exists() { continue; }    // <-- DROPS gitlink update
```

This is the gap: when the entry is a gitlink-pointer modification
(parent's tracked SHA ≠ submodule's current HEAD), the daemon SHOULD
run `git add <path>` (which on a gitlink just updates the index entry
to the new SHA — see verification in doc-1c below) and then commit. But
the current branch treats the entry as "skip — it's a submodule".

## doc-1b — `dracon-git::get_status` accounting

`dracon-git-94.7.0/src/lib.rs:100-138`:

```rust
pub async fn get_status(&self) -> Result<RepoStatus> {
    ...
    let statuses = repo.statuses(Some(&mut opts))?;
    for entry in statuses.iter() {
        let s = entry.status();
        if s.is_index_new() || s.is_index_modified() || s.is_index_deleted()
            || s.is_index_renamed() || s.is_index_typechange()
        { status.staged_files += 1; }
        if s.is_wt_new() { status.untracked_files += 1; }
        else if s.is_wt_modified() || s.is_wt_deleted()
              || s.is_wt_renamed() || s.is_wt_typechange()
        { status.modified_files += 1; }
    }
    ...
}
```

A gitlink-pointer change shows up as `WT_MODIFIED` in libgit2 (the
working-tree entry's SHA differs from the index's expected SHA).
Therefore:

- `modified_files` increments by 1 (correct: gitlink is modified).
- `is_clean` becomes false.
- The CLI fallback (`git status --porcelain`) produces ` M rust-ai-web-auto`
  which is what `dracon-sync repos` surfaces as "MOD=1".

So `dracon-git` DOES report gitlink changes correctly. The bug is purely
in the daemon's `stage_existing_files` (drops the entry).

### `dracon-git::get_diff_entries` (lib.rs:188)

Uses `git status --porcelain -z` — returns the raw 4-char status
prefix + NUL-terminated path. So `get_diff_entries` returns
`" M\0rust-ai-web-auto"` and `compute_diff_entries` puts that in
`entries` with status `FileStatus::Modified`. Good.

## doc-1c — manual reproduction on web-auto

```
$ cd /home/dracon/Dev/web-auto
$ git ls-tree HEAD rust-ai-web-auto
160000 commit 331a716ae7a327c866c435d9da91696f312c9bac   rust-ai-web-auto

$ cd rust-ai-web-auto && git rev-parse HEAD
552abf6efc822f72a2d809def4a659c501a91aed

$ cd ..   # back to web-auto
$ git status --short
 M rust-ai-web-auto           #   <-- parent sees gitlink as modified

# Manual fix: `git add` the submodule path (no recursion)
$ git add rust-ai-web-auto
ok 1 file changed, 1 insertion(+), 1 deletion(-)

$ git ls-files --stage rust-ai-web-auto
160000 552abf6efc822f72a2d809def4a659c501a91aed 0       rust-ai-web-auto
# ^ index now points to submodule's actual HEAD — no recursion into work tree

$ git status --short
M  rust-ai-web-auto           # staged
```

After committing and pushing this would update the parent gitlink to
552abf6 — the operation the daemon needs to automate.

The key API fact: `git add <gitlink_path>` is the canonical way to
update the parent's pointer without touching the submodule's
working tree. (`git add -A -- <gitlink_path>` is what the daemon's
`stage_existing_files` runs today, which DOES also work — but it
precedes a `.exists()` check that skips the entire subtree, so the
gitlink update never reaches git add.)

## Summary

The daemon does each subrepo correctly (its own commits land on its own
remotes). The gap is the **parent gitlink propagation**: when a
submodule's HEAD moves (via the subrepo's own sync loop), the parent
sees ` M <submodule>`, the daemon's compute_diff_entries picks it up,
`is_gitlink_unchanged` returns false (because SHA differs), the entry
passes into `stage_existing_files`, and then the submodule-skip branch
silently drops it — leaving the parent's gitlink stale forever.

Recovery commands (any of these work):

1. `cd <parent_repo> && git add <submodule_path> && git commit -m
   "chore(<submodule>): sync to <short_sha>"`
2. `cd <parent_repo> && git submodule update --remote
   <submodule_path>`  (fetches the submodule remote and updates the
   parent index)
3. `cd <parent_repo> && git -C <submodule_path> pull --ff-only` then
   `git add <submodule_path>` and commit.

Recovery is one command, but it's MANUAL until the daemon gains the
gitlink-aware stage path.
