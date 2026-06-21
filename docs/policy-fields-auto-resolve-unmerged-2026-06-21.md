# Auto-Resolve Unmerged + Backlog Detection (v0.112.13)

This document describes the three new policy fields added in
`dracon-sync` v0.112.13 to prevent the daemon from stalling when a
watched repo has unmerged index entries or an untracked-file
backlog.

## Background

Prior to v0.112.13, the daemon's `stage_commit_and_push` flow had
no recovery path for unmerged index entries. If any watched repo had
paths in a 1/2/3 unmerged state (e.g. from a previous merge, a
pull+rebase, or a force-push), the daemon's `git add -A` would fail
with `cannot create a tree from a not fully merged index`, the
entire batch of 444+ untracked files would be discarded, and the
daemon would retry every 10s without making progress.

On `dracon-platform` (2026-06-21), this caused a 4+ hour stall where
the daemon made 0 commits and 0 pushes while the untracked-file
backlog grew from 0 to 293+ files.

## New policy fields

All three fields have sensible defaults that match the operator's
commit-all policy. Existing `dracon-sync.toml` files load unchanged
because the fields are marked `#[serde(default = ...)]`.

### `auto_resolve_unmerged: bool` (default `true`)

When the daemon's commit cycle is about to fail on an unmerged
index, it now:

1. Lists unmerged paths via `git ls-files --unmerged`
2. For each unmerged path, reads the working-tree file and compares
   it byte-for-byte to `git show HEAD:<path>`
3. **If the bytes match** (i.e. the user has the HEAD content
   already, just stale index bookkeeping): runs
   `git reset HEAD -- <path>` to clear the unmerge
4. **If the bytes differ** (i.e. the user has unmerged work in
   progress with conflict markers or `--theirs` content): leaves
   the path alone — the daemon must not touch user work

This step is purely a pre-flight check. It runs at the start of
`stage_commit_and_push`, before any `git add` operation.

**When `auto_resolve_unmerged=false`**: the daemon does not
auto-resolve anything. The user is responsible for clearing
unmerged entries themselves (e.g. via `git checkout --ours`).

### `push_debounce_secs: u64` (default `30s`)

Reduces push churn. The daemon still commits as soon as a batch
is ready, but it coalesces pushes within the debounce window so a
burst of small commits becomes one push per remote.

**When `push_debounce_secs=0`**: pushes happen as soon as a commit
completes (the pre-v0.112.13 behavior).

### `untracked_warn_threshold: usize` (default `500`)

Emits a `⚠️ untracked count exceeded threshold: <N>` log line when
the untracked-file count exceeds the threshold. This is a warning
that the backlog is growing faster than the daemon can drain —
useful for catching dev sessions (Playwright, vite, etc.) that are
producing files faster than the daemon can commit them.

**When `untracked_warn_threshold=0`**: the warning is disabled.

## Example: enabling all three with conservative defaults

```toml
# ~/.dracon/utilities/sync/dracon-sync.toml
[default]
auto_resolve_unmerged = true    # default; clear safe unmerged entries
push_debounce_secs = 30         # default; coalesce pushes within 30s
untracked_warn_threshold = 500  # default; warn when backlog > 500
```

## Example: disabling auto-resolve per-repo

```toml
# <repo>/.dracon/dracon-sync.toml
[default]
auto_resolve_unmerged = false   # leave unmerged entries for the user
```

## Safety: when the daemon does NOT auto-resolve

The auto-resolve is **only safe** when the working tree matches
HEAD. The check is byte-for-byte. The daemon does NOT auto-resolve
in any of these cases:

- An active merge conflict with conflict markers (`<<<<<<<`) in
  the working tree
- A user editing a file during a conflict (the working tree is the
  user's edits, not the HEAD content)
- A case where the user has chosen `--theirs` (the working tree is
  `--theirs`, not HEAD)

In all of these cases, the byte-level check fails, and the daemon
leaves the unmerged entry alone. The user resolves the conflict
manually and the daemon picks up on the next cycle.

## Performance

The `auto_resolve_unmerged_if_safe` function runs in O(N) where N
is the number of unmerged paths (typically 0-10). It does NOT scan
all untracked files; only the unmerged ones. The cost is one
`git ls-files --unmerged` per commit cycle (cheap: a few ms) and
one `git show HEAD:<path>` per unmerged path (also cheap: reads
the file's blob from the object store).

The `check_untracked_threshold` function runs `git ls-files
--others --exclude-standard` once per commit cycle to count
untracked files. This is also fast (a few hundred ms for a large
repo).

## See also

- `CHANGELOG.md` — v0.112.13 release entry
- `release-notes-v0.112.13.md` — release notes
- `docs/design/daemon-auto-resolve-unmerged-2026-06-21.md` (in the
  parent monorepo) — design rationale and live verification
