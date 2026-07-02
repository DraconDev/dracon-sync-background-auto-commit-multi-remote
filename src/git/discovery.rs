//! Repository discovery — find git repos under watch roots.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::policy::debug_enabled;

/// One submodule entry as declared in the parent repo's `.gitmodules`.
///
/// `name` is the .gitmodules key (e.g. `web-games-polis`).
/// `path` is the working-tree path of the submodule relative to the
/// parent (e.g. `web/games/wip/polis`).
/// `url` is the first `submodule.<name>.url` value found (any of the
/// github/gitlab/codeberg SSH URLs is fine — the daemon's multi-remote
/// push config builds the per-remote push URLs from this).
/// `sha` is the gitlink SHA tracked in the parent's index (the SHA
/// the submodule's HEAD should be at after a clean `git submodule
/// update --init`). The daemon uses this to materialize the
/// standalone worktree at the matching commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubmoduleEntry {
    pub name: String,
    pub path: String,
    pub url: String,
    pub sha: String,
}

/// Discover git repositories under the given watch roots.
/// Searches up to 4 levels deep, skipping excluded dirs and repos.
pub(crate) fn discover_git_repos(
    roots: &[PathBuf],
    excluded_dir_names: &BTreeSet<String>,
    exclude_repos: &[String],
    system_repo: Option<&str>,
) -> Vec<PathBuf> {
    let exlude_set: HashSet<String> = exclude_repos.iter().map(|s| s.to_lowercase()).collect();
    let mut repos = Vec::new();
    for root in roots {
        // Check if the root itself is a git repo (before recursing into children).
        // This handles the case where a watch root is itself a git repo (e.g., ~/.dracon).
        let root_dot_git = root.join(".git");
        if root_dot_git.exists()
            && (root_dot_git.is_dir() || is_git_worktree_file(&root_dot_git))
            && !exlude_set.contains(&root.to_string_lossy().to_lowercase())
        {
            repos.push(root.clone());
        }
        discover_git_repos_recursive(root, excluded_dir_names, &mut repos, 0, 4);
    }
    // Compute the surviving set imperatively so we can call
    // `is_nested_submodule_with_standalone` which needs to
    // inspect the FULL list of already-collected repos (we
    // need every parent in scope to evaluate the helper for
    // each child — `retain` can only see items not yet
    // visited). The helper then decides whether to skip
    // nested submodule checkouts whose shared gitdir is the
    // same as a standalone worktree (the duplicate-row
    // filter added 2026-07-01 for goal `mr10pdzr-i495vy`).
    {
        let mut surviving: Vec<PathBuf> = Vec::with_capacity(repos.len());
        for r in &repos {
            let abs = r.to_string_lossy().to_lowercase();
            let name = r
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if exlude_set.contains(&abs) || exlude_set.contains(&name) {
                continue;
            }
            if is_duplicate_standalone_for_nested(r, &repos, roots) {
                if debug_enabled() {
                    eprintln!(
                        "🐛 skipping standalone {} (nested submodule is the canonical watch path)",
                        r.display()
                    );
                }
                continue;
            }
            surviving.push(r.clone());
        }
        repos = surviving;
    }

    // Submodule pass: for each discovered repo, look for submodules
    // declared in .gitmodules and add a candidate path for each
    // one. The candidate path is computed from the canonical
    // .gitmodules name (`web-games-polis`) using the same anchor
    // root that the daemon discovered the parent under, so that
    // the resulting path is something the daemon can both
    // materialize (if it doesn't exist yet) and skip (if it
    // already does).
    //
    // CHANGED 2026-07-02 (goal `mr3g843f-lajfpg`):
    // Nested-on-main architecture. The nested submodule path
    // (e.g. `/home/dracon/Dev/dracon-platform/web/games/wip/polis/`)
    // IS the canonical watch path. `discover_git_repos_recursive`
    // already picks up the nested path (it has a `.git` file
    // pointing into the parent's `.git/modules/`), so we do NOT
    // add a candidate at the watch root. If the nested path is
    // missing (e.g. on first-time clone), we fall back to the
    // standalone-at-watch-root candidate for backwards compat
    // with older layouts.
    //
    // ADDED 2026-06-30, goal `mr10pdzr-i495vy`:
    // "Make the daemon discover submodules as proper repos".
    //
    // Note: collect candidates first then push, because iterating
    // `&repos` and pushing to `repos` simultaneously is a
    // borrow-checker error.
    let mut submodule_candidates: Vec<PathBuf> = Vec::new();
    for parent in &repos {
        for sub in list_submodules(parent) {
            // The nested submodule path: <parent>/<sub.path>.
            // If it exists as a directory with a `.git` file
            // (worktree-style checkout of the shared gitdir),
            // it's already discovered by the recursive walk —
            // skip adding a candidate.
            let nested_path = parent.join(&sub.path);
            let nested_already_discovered = nested_path
                .join(".git")
                .exists()
                && repos
                    .iter()
                    .any(|r| std::fs::canonicalize(r).ok() == std::fs::canonicalize(&nested_path).ok());

            if nested_already_discovered {
                continue;
            }

            // Fall back to the watch-root standalone for legacy
            // (pre-migration) layouts: use the path basename as
            // the worktree name. This matches how the existing
            // daemon reports the ghost rows in `repos` (e.g.
            // `polis`, not `web-games-polis`), and produces URLs
            // of the form `DraconDev/{repo}.git` via the
            // existing `push_url` template. If the operator
            // wants the longer `DraconDev/web-games-polis.git`
            // form, a `repo_name_map` entry can map it.
            let worktree_name = Path::new(&sub.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| sub.name.clone());
            if let Some(anchor) = find_anchor_root(parent, roots) {
                let candidate = anchor.join(&worktree_name);
                submodule_candidates.push(candidate);
            }
        }
    }
    for candidate in submodule_candidates {
        if !repos.contains(&candidate) {
            repos.push(candidate);
        }
    }

    // Always include system_repo if it exists and is a git repo
    if let Some(system) = system_repo {
        let system_path = PathBuf::from(system);
        let system_abs = system.to_lowercase();
        let system_name = system_path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if system_path.exists()
            && system_path.join(".git").exists()
            && !repos.contains(&system_path)
            && !exlude_set.contains(&system_abs)
            && !exlude_set.contains(&system_name)
        {
            repos.push(system_path);
        }
    }

    repos
}

/// Find the watch root that contains `parent`. Used to anchor
/// submodule worktree candidate paths (e.g. `/home/dracon/Dev/`)
/// so the worktree is at the same level as its parent (not nested
/// inside the parent's working tree). Returns `None` if no watch
/// root contains the parent (defensive — should not happen for
/// repos discovered by `discover_git_repos_recursive`).
fn find_anchor_root(parent: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let parent_canon = parent.canonicalize().ok()?;
    for root in roots {
        if let Ok(root_canon) = root.canonicalize() {
            if parent_canon.starts_with(&root_canon) {
                return Some(root_canon);
            }
        }
    }
    None
}

fn discover_git_repos_recursive(
    dir: &Path,
    excluded_dir_names: &BTreeSet<String>,
    repos: &mut Vec<PathBuf>,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("⚠️ cannot read directory {}: {}", dir.display(), e);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("⚠️ cannot read entry in {}: {}", dir.display(), e);
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() || path.is_symlink() {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if excluded_dir_names.contains(&name) || name == "objects" {
            continue;
        }
        let dot_git = path.join(".git");
        if dot_git.exists() && (dot_git.is_dir() || is_git_worktree_file(&dot_git)) {
            // Record the subdir as a discovered repo AND continue recursing
            // into its children to look for any nested sub-subdirs that
            // might also have their own .git/. This supports the
            // "3 sibling repos inside a parent repo" structure (e.g.
            // 3 utility subdirs living under a docs-only monorepo).
            // CHANGED 2026-06-21 (goal 5297d4df): the previous `continue`
            // early-exit prevented discovery of nested standalone repos.
            repos.push(path.clone());
        } else if name.starts_with('.') {
            continue;
        }
        discover_git_repos_recursive(&path, excluded_dir_names, repos, depth + 1, max_depth);
    }
}

/// Check if a `.git` worktree file points to a valid git directory.
pub(crate) fn is_git_worktree_file(dot_git: &Path) -> bool {
    std::fs::read_to_string(dot_git)
        .map(|content| content.trim().starts_with("gitdir:"))
        .unwrap_or(false)
}

/// Returns true if `path` is a NESTED submodule checkout (a worktree of
/// a parent's gitdir) AND a sibling standalone worktree at the watch
/// root already exists (one already in `discovered` or computable from
/// `roots`).
///
/// Used by `discover_git_repos` to dedup the duplicate-row case where
/// the daemon would otherwise treat both the standalone
/// `/home/dracon/Dev/<name>/` and the nested submodule at
/// `<parent>/<path>/` as separate repos.
///
/// ADDED 2026-07-01, goal `mr10pdzr-i495vy`:
/// After `materialize_submodule` runs, the nested submodule
/// at `<parent>/<submodule_path>/` and the standalone
/// worktree at the watch root point at the SAME shared gitdir
/// (`<parent>/.git/modules/<name>`). Both are technically valid
/// "git repos" — both have a `.git` (one a file, one a dir).
/// But the daemon should sync only one of them per cycle.
///
/// CHANGED 2026-07-02 (goal `mr3g843f-lajfpg`):
/// Reverse polarity for the nested-on-main architecture: the
/// nested submodule checkout is now the canonical watch path.
/// Returns true if `path` is the **standalone** (the duplicate
/// worktree at the watch root), filtering it out.  The nested
/// submodule path survives and is what the daemon syncs.
///
/// CHANGED 2026-07-01 (goal `mr1x7j5i-zioba9`):
/// Previously the standalone was on a `daemon-standalone`
/// branch (now removed). The standalone is on `main` directly
/// now, so the comment is updated.
///
/// Strategy: scan `roots` for a parent that contains `path` as a
/// nested submodule. If `path` itself IS the standalone at the
/// watch root AND a sibling nested submodule exists in any of
/// the parents, return true (skip this path).
pub(crate) fn is_duplicate_standalone_for_nested(
    path: &Path,
    discovered: &[PathBuf],
    roots: &[PathBuf],
) -> bool {
    let _ = roots; // unused in pairwise logic
    let Some(own_gitdir) = path_gitdir(path) else {
        return false;
    };
    let Some(own_primary) = resolve_primary_gitdir(&own_gitdir) else {
        return false;
    };

    // Pairwise check: walk `discovered` looking for any OTHER path
    // whose PRIMARY gitdir (chopping `/worktrees/<X>`) is the same
    // as `path`'s primary gitdir. If so, the nested submodule
    // (which is the primary worktree of the shared gitdir) AND
    // `path` (which is a secondary worktree at the watch root) are
    // both pointing at the same shared gitdir — `path` is the
    // duplicate standalone and should be filtered.
    //
    // For the case where BOTH paths point at the same PRIMARY
    // gitdir (e.g. the test's synthetic setup or a non-`worktree
    // add` standalone), we tiebreak by checking which one is
    // nested inside a discovered parent.
    for other in discovered {
        if other == path {
            continue;
        }
        let path_canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let other_canon = std::fs::canonicalize(other).unwrap_or_else(|_| other.to_path_buf());
        if path_canon == other_canon {
            continue;
        }
        let Some(other_gitdir) = path_gitdir(other) else {
            continue;
        };
        let Some(other_primary) = resolve_primary_gitdir(&other_gitdir) else {
            continue;
        };
        if own_primary != other_primary {
            continue;
        }

        // Both share the same primary gitdir. Determine which is
        // the canonical nested submodule and which is the
        // duplicate standalone.
        //
        // Case 1: `path` is a secondary worktree (its gitdir is
        // `<primary>/worktrees/<X>`) and `other` is the primary
        // worktree (its gitdir is `<primary>`). Then `path` is the
        // duplicate standalone.
        if own_gitdir != own_primary && other_gitdir == other_primary {
            return true;
        }
        // Case 2: `path` is the primary worktree and `other` is
        // the secondary. Then `other` is the duplicate, NOT path.
        // Don't return true here (continue to next).
        if other_gitdir != other_primary && own_gitdir == own_primary {
            return false; // `path` is the primary; keep it
        }
        // Case 3: Both are primary (or both secondary) — tiebreak
        // by checking which is nested inside a discovered parent.
        // The nested one is canonical; the sibling is duplicate.
        let own_is_nested = is_inside_a_discovered_parent(path, discovered);
        let other_is_nested = is_inside_a_discovered_parent(other, discovered);
        if other_is_nested && !own_is_nested {
            return true;
        }
    }

    false
}

/// Returns true if `path` lives inside any `discovered` repo's
/// directory tree (i.e., `path` is a NESTED submodule of some
/// parent). Used by `is_duplicate_standalone_for_nested` to
/// decide which of two gitdir-sharing paths to keep.
fn is_inside_a_discovered_parent(path: &Path, discovered: &[PathBuf]) -> bool {
    let path_canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    for disc in discovered {
        let Ok(disc_canon) = disc.canonicalize() else {
            continue;
        };
        // path must be a STRICT descendant of disc
        if path_canon.starts_with(&disc_canon) && path_canon != disc_canon {
            return true;
        }
    }
    false
}

/// Resolve the gitdir a path points at. Returns the canonical
/// (canonicalized) path of the gitdir on disk.
fn path_gitdir(path: &Path) -> Option<PathBuf> {
    let dot_git = path.join(".git");
    if !dot_git.exists() {
        return None;
    }
    if dot_git.is_dir() {
        std::fs::canonicalize(&dot_git).ok()
    } else if dot_git.is_file() {
        let content = std::fs::read_to_string(&dot_git).ok()?;
        let rest = content.trim().strip_prefix("gitdir:")?;
        let gitdir_rel = rest.trim();
        let base_canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let resolved = base_canon.join(gitdir_rel);
        std::fs::canonicalize(&resolved).ok()
    } else {
        None
    }
}

/// Given a gitdir path, return its PRIMARY gitdir. For
/// `/foo/.git/modules/<name>/worktrees/<X>/`, the primary
/// is `/foo/.git/modules/<name>/` (chop `/worktrees/<X>`).
/// For `/foo/.git/modules/<name>/`, the primary IS
/// `/foo/.git/modules/<name>/`. For nested repos that
/// share a gitdir via `gitdir: <relative>` (e.g. nested
/// submodules' main worktree points at the primary
/// gitdir), the result is the gitdir itself.
fn resolve_primary_gitdir(gitdir: &Path) -> Option<PathBuf> {
    // Walk up the components looking for a "worktrees" segment.
    // If we find one, the primary gitdir is the components
    // BEFORE `/worktrees/<X>/...` (i.e., the parent of `/worktrees`).
    // If we don't find one, the primary IS the gitdir itself.
    let canon = std::fs::canonicalize(gitdir)
        .ok()
        .unwrap_or_else(|| gitdir.to_path_buf());
    let mut components: Vec<std::path::Component> =
        canon.components().collect();
    // Find a "worktrees" segment in the path.
    let worktrees_idx = components
        .iter()
        .position(|c| c.as_os_str() == "worktrees");
    if let Some(idx) = worktrees_idx {
        // Primary = components[..idx] (the components BEFORE
        // `/worktrees`).
        if idx == 0 {
            return Some(canon);
        }
        let mut primary = std::path::PathBuf::new();
        for c in &components[..idx] {
            primary.push(c);
        }
        return Some(primary);
    }
    // No /worktrees/ segment. Primary is the gitdir itself.
    Some(canon)
}

/// Read `.gitmodules` from a parent repo and return the list of
/// submodules declared there, plus the SHA currently tracked in the
/// parent's index for each gitlink.
///
/// Strategy:
/// 1. Parse `.gitmodules` (a gitconfig-format file) for
///    `submodule.<name>.path` and `submodule.<name>.url`.
/// 2. Cross-reference each declared submodule with the parent's
///    index via `git ls-files --stage` to find the tracked gitlink
///    SHA. A submodule that's declared in `.gitmodules` but missing
///    from the index gets `sha = ""` (the caller should skip it).
///
/// Returns an empty Vec if `.gitmodules` is absent (most repos don't
/// use submodules), unreadable, or malformed. This is the safe
/// fallback — never treat a missing .gitmodules as a fatal error.
///
/// Why we read `.gitmodules` directly instead of `git submodule
/// status`:
/// - `git submodule status` requires the submodule gitdirs to be
///   present (it shells into `<parent>/.git/modules/<name>` to look
///   up each submodule's HEAD). The parent's `.git/modules/` may be
///   partially populated (e.g. only the submodules that have been
///   initialized for a particular clone variant), but `.gitmodules`
///   is committed and always reflects the operator's intent.
/// - We want the **tracked** SHA from the parent's index, not the
///   submodule's working-tree HEAD. The two diverge when the
///   submodule is uninitialized or has local commits the parent
///   hasn't seen. Reading `git ls-files --stage` for the 160000
///   entries gives the canonical "what the parent points at" SHA.
///
/// ADDED 2026-06-30, goal `mr10pdzr-i495vy`:
/// "Make the daemon discover submodules as proper repos".
pub(crate) fn list_submodules(parent: &Path) -> Vec<SubmoduleEntry> {
    let gitmodules = parent.join(".gitmodules");
    if !gitmodules.exists() {
        return Vec::new();
    }
    // Parse .gitmodules with the same regex/keys as a real gitconfig.
    // We can't shell out to `git config --file .gitmodules --get-regexp`
    // here because that would require `git` to be on PATH and would
    // not let us return a partial result on parse error. Instead we
    // parse it ourselves — .gitmodules is a small, well-formed file
    // (one section per submodule, three keys per section).
    let Ok(content) = std::fs::read_to_string(&gitmodules) else {
        return Vec::new();
    };
    let mut by_name: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    let mut current_name: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[' ) && trimmed.ends_with(']') {
            let section = &trimmed[1..trimmed.len() - 1];
            if let Some(rest) = section.strip_prefix("submodule ") {
                let name = rest.trim().trim_matches('"').to_string();
                current_name = Some(name.clone());
                by_name.entry(name).or_insert((None, None));
            } else {
                current_name = None;
            }
            continue;
        }
        if current_name.is_none() {
            continue;
        }
        let (key, value) = match trimmed.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => continue,
        };
        let name = current_name.as_ref().unwrap().clone();
        let entry = by_name.entry(name).or_insert((None, None));
        match key {
            "path" => entry.0 = Some(value.to_string()),
            "url" => {
                // .gitmodules may declare multiple URLs (one per
                // forge) — keep the first one (any is fine; the
                // daemon's multi-remote config will rebuild the
                // per-remote URLs from the canonical name).
                if entry.1.is_none() {
                    entry.1 = Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    // Cross-reference with the parent's index to get the tracked SHA.
    let tracked = read_parent_gitlink_shas(parent);
    let mut out: Vec<SubmoduleEntry> = by_name
        .into_iter()
        .filter_map(|(name, (path, url))| {
            let path = path?;
            // No URL declared — malformed entry, skip.
            let url = url?;
            // Look up the tracked SHA in the parent's index. If not
            // present (parent doesn't have this submodule checked
            // out), still return the entry but with sha = "" so
            // callers can detect it.
            let sha = tracked
                .iter()
                .find(|(p, _)| p == &path)
                .map(|(_, s)| s.clone())
                .unwrap_or_default();
            Some(SubmoduleEntry {
                name,
                path,
                url,
                sha,
            })
        })
        .collect();
    // Stable ordering: sort by path so the daemon's logs are
    // deterministic across runs.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Read the parent's index for gitlink (mode 160000) entries and
/// return `(path, sha)` pairs. Used by `list_submodules` to find the
/// SHA each submodule is currently tracked at.
///
/// Returns an empty Vec if the parent is not a git repo or
/// `git ls-files --stage` fails. The 160000 filter is essential:
/// `ls-files --stage` also lists regular files with their stage
/// numbers; only gitlinks have mode 160000.
fn read_parent_gitlink_shas(parent: &Path) -> Vec<(String, String)> {
    let output = crate::git::git_cmd()
        .current_dir(parent)
        .args(["ls-files", "--stage"])
        .output();
    let Ok(out) = output else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            // Format: `<mode> <sha> <stage>\t<path>` for staged
            // entries, `<mode> <sha> 0\t<path>` for unstaged.
            let mut parts = line.splitn(3, '\t');
            let meta = parts.next()?;
            let path = parts.next()?.to_string();
            let mut meta_parts = meta.split_whitespace();
            let mode = meta_parts.next()?;
            let sha = meta_parts.next()?;
            if mode == "160000" {
                Some((path, sha.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Count how many of the given untracked-path strings point to a nested
/// git repository under `repo`. Such entries inflate the parent's
/// untracked-file count without representing new files in the parent —
/// the child repo is a separately-tracked, independently-synced git
/// repo that the daemon discovers via `discover_git_repos`.
///
/// An entry is considered a nested git repo when its on-disk path
/// contains a `.git` entry (either a directory — a standalone nested
/// repo — or a `.git` file — a submodule / worktree). This matches
/// git's own boundary semantics: `git ls-files --others
/// --exclude-standard` stops at the first `.git/` it sees, so the only
/// nested-repo "noise" entries that reach the parent are the parent
/// paths of those nested repos (e.g. `child/`).
///
/// ADDED 2026-06-30, goal `mr02de1n-gjkgzp`:
/// "The daemon should subtract known-nested-repos from the parent's UT count".
pub(crate) fn count_nested_repo_untracked_entries(repo: &Path, entries: &[String]) -> usize {
    entries
        .iter()
        .filter(|entry| is_nested_repo_path(repo, entry))
        .count()
}

/// Returns true if `entry` (an untracked path returned by `git ls-files
/// --others --exclude-standard`) is the parent path of a nested git
/// repository under `repo`. Strips any trailing slash (which git
/// appends to directory entries) before checking for `.git`.
///
/// `entry` is interpreted as a path relative to `repo`. An absolute
/// path or one with `..` components is treated as non-nested (safe
/// fallback: don't subtract).
pub(crate) fn is_nested_repo_path(repo: &Path, entry: &str) -> bool {
    let trimmed = entry.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return false;
    }
    // Reject unsafe paths (absolute, .., etc.) — never treat as a
    // nested repo, but also don't fail loudly: the parent repo's
    // `git ls-files` would never emit these.
    if !is_safe_git_path(Path::new(trimmed)) {
        return false;
    }
    let full = repo.join(trimmed);
    let dot_git = full.join(".git");
    dot_git.exists()
}

/// Check if a path is safe — not in a way that could be used for
/// path traversal or other attacks.
pub(crate) fn is_safe_git_path(path: &Path) -> bool {
    if path.is_absolute() {
        return false;
    }
    let mut components = path.components();
    if let Some(first) = components.next() {
        if first.as_os_str() == ".." {
            return false;
        }
    }
    if let Some(first) = components.next() {
        if first.as_os_str() == ".." {
            return false;
        }
    }
    if path.to_string_lossy().starts_with('-') {
        return false;
    }
    true
}

/// Check if a branch name is safe to use in git commands (no injection chars).
pub(crate) fn is_safe_branch_name(branch: &str) -> bool {
    if branch.is_empty() {
        return false;
    }
    if branch.starts_with('-') {
        return false;
    }
    if branch.contains("..") {
        return false;
    }
    if branch.contains('\n') || branch.contains('\r') || branch.contains('\0') {
        return false;
    }
    if branch.ends_with('.') {
        return false;
    }
    if branch.contains('\\') || branch.contains('~') || branch.contains('^') || branch.contains(':')
    {
        return false;
    }
    if branch.contains('?') || branch.contains('*') || branch.contains('[') {
        return false;
    }
    if branch.contains(' ') {
        return false;
    }
    true
}

#[cfg(test)]
mod nested_repo_tests {
    use super::*;
    use std::fs;

    /// Build a parent dir with one nested git repo under `nested_name`.
    /// Returns the parent path. The parent is NOT a git repo (we just
    /// need it as a directory for the helper's filesystem checks).
    fn build_parent_with_nested(parent: &Path, nested_name: &str) -> PathBuf {
        let nested = parent.join(nested_name);
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(nested.join(".git")).unwrap();
        nested
    }

    #[test]
    fn count_nested_repo_untracked_entries_zero_when_no_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        assert_eq!(count_nested_repo_untracked_entries(&repo, &[]), 0);
    }

    #[test]
    fn count_nested_repo_untracked_entries_counts_nested_repo_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        build_parent_with_nested(&repo, "child_a");
        build_parent_with_nested(&repo, "child_b");
        let entries = vec![
            "child_a".to_string(),
            "child_a/inner.txt".to_string(),
            "child_b".to_string(),
            "scratch.txt".to_string(),
        ];
        // 2 nested-repo entries (child_a, child_b) plus 2 inner/scratch
        // entries that do NOT have .git inside.
        assert_eq!(
            count_nested_repo_untracked_entries(&repo, &entries),
            2,
            "must count both nested-repo dirs (child_a, child_b) and ignore child_a/inner.txt and scratch.txt",
        );
    }

    #[test]
    fn count_nested_repo_untracked_entries_handles_trailing_slash() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        build_parent_with_nested(&repo, "child");
        // git ls-files --others may emit "child/" with a trailing
        // slash for untracked directory entries.
        let entries = vec!["child/".to_string()];
        assert_eq!(
            count_nested_repo_untracked_entries(&repo, &entries),
            1,
            "trailing slash must not prevent detection of the nested repo",
        );
    }

    #[test]
    fn count_nested_repo_untracked_entries_counts_submodule_dot_git_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        // Build a `.git` FILE pointing to a worktree-style gitdir
        // (this is what submodules and linked worktrees look like).
        let sub = repo.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(repo.join(".git/modules/sub")).unwrap();
        fs::write(
            sub.join(".git"),
            "gitdir: /fake/path/.git/modules/sub\n",
        )
        .unwrap();
        let entries = vec!["sub".to_string()];
        assert_eq!(
            count_nested_repo_untracked_entries(&repo, &entries),
            1,
            "a sub/ where sub/.git is a file must also count as a nested repo",
        );
    }

    #[test]
    fn count_nested_repo_untracked_entries_keeps_plain_untracked_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        // A plain dir with NO .git inside — must NOT count as nested.
        fs::create_dir_all(repo.join("scratch_dir")).unwrap();
        fs::write(repo.join("scratch_dir/note.txt"), b"x").unwrap();
        fs::write(repo.join("a.txt"), b"x").unwrap();
        let entries = vec![
            "scratch_dir".to_string(),
            "scratch_dir/note.txt".to_string(),
            "a.txt".to_string(),
        ];
        assert_eq!(
            count_nested_repo_untracked_entries(&repo, &entries),
            0,
            "plain untracked dirs without .git must not be subtracted",
        );
    }

    #[test]
    fn is_nested_repo_path_rejects_unsafe_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        // Absolute path and .. are unsafe per is_safe_git_path;
        // is_nested_repo_path must treat them as non-nested (safe
        // fallback that does not subtract).
        assert!(!is_nested_repo_path(&repo, "/etc/passwd"));
        assert!(!is_nested_repo_path(&repo, "../etc"));
        assert!(!is_nested_repo_path(&repo, ""));
        assert!(!is_nested_repo_path(&repo, "."));
    }
}

#[cfg(test)]
mod submodule_tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Initialize a throwaway git repo in `dir` so the parent has a
    /// real `.git/` (required for `git ls-files --stage` to work
    /// during `list_submodules` cross-referencing). Returns the SHA
    /// of the initial empty commit.
    fn init_parent_repo(dir: &Path) -> String {
        fs::create_dir_all(dir).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        assert!(run(&["init", "-q"]).status.success(), "git init failed");
        assert!(run(&["config", "user.email", "test@example.com"]).status.success());
        assert!(run(&["config", "user.name", "Test"]).status.success());
        assert!(run(&["config", "commit.gpgsign", "false"]).status.success());
        assert!(run(&["config", "tag.gpgsign", "false"]).status.success());
        // Need at least one commit for the index to be readable by
        // `git ls-files --stage`. Add an empty commit so HEAD exists.
        fs::write(dir.join("README.md"), b"# test\n").unwrap();
        assert!(run(&["add", "README.md"]).status.success());
        assert!(run(&["commit", "-q", "-m", "init"]).status.success());
        let head = run(&["rev-parse", "HEAD"]).stdout;
        String::from_utf8_lossy(&head).trim().to_string()
    }

    #[test]
    fn list_submodules_returns_empty_when_no_gitmodules() {
        let tmp = tempfile::tempdir().unwrap();
        // No .gitmodules file at all.
        assert_eq!(list_submodules(tmp.path()), Vec::<SubmoduleEntry>::new());
    }

    #[test]
    fn list_submodules_parses_gitmodules_with_index_shas() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().to_path_buf();
        let _head = init_parent_repo(&parent);

        // Write a 3-submodule .gitmodules.
        let gitmodules = "[submodule \"web-games-polis\"]\n\
                          \tpath = web/games/wip/polis\n\
                          \turl = git@github.com:DraconDev/web-games-polis.git\n\
                          \turl = git@gitlab.com:DraconDev/web-games-polis.git\n\
                          \turl = git@codeberg.org:dracondev/web-games-polis.git\n\
                          [submodule \"web-games-deathrun\"]\n\
                          \tpath = web/games/wip/deathrun\n\
                          \turl = git@github.com:DraconDev/web-games-deathrun.git\n\
                          [submodule \"web-games-junk-runner\"]\n\
                          \tpath = web/games/wip/junk-runner\n\
                          \turl = git@github.com:DraconDev/web-games-junk-runner.git\n";
        fs::write(parent.join(".gitmodules"), gitmodules).unwrap();

        // Stage gitlink entries in the parent's index so
        // `list_submodules` can find the tracked SHAs. Use the
        // parent's HEAD as a placeholder SHA (we don't need real
        // submodule SHAs for this test — only that the cross-ref
        // lookup works).
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&parent)
            .output()
            .unwrap()
            .stdout;
        let head_sha = String::from_utf8_lossy(&head).trim().to_string();
        for path in ["web/games/wip/polis", "web/games/wip/deathrun", "web/games/wip/junk-runner"] {
            Command::new("git")
                .args(["update-index", "--add", "--cacheinfo", &format!("160000,{},{}", head_sha, path)])
                .current_dir(&parent)
                .output()
                .unwrap();
        }

        let entries = list_submodules(&parent);
        assert_eq!(entries.len(), 3, "expected 3 submodules, got {:?}", entries);

        // The order is by path (stable sort).
        assert_eq!(entries[0].name, "web-games-deathrun");
        assert_eq!(entries[0].path, "web/games/wip/deathrun");
        assert_eq!(entries[0].url, "git@github.com:DraconDev/web-games-deathrun.git");
        assert_eq!(entries[0].sha, head_sha);

        assert_eq!(entries[1].name, "web-games-junk-runner");
        assert_eq!(entries[1].path, "web/games/wip/junk-runner");
        assert_eq!(entries[1].sha, head_sha);

        // Multiple URL entries: only the first one is kept.
        assert_eq!(entries[2].name, "web-games-polis");
        assert_eq!(entries[2].path, "web/games/wip/polis");
        assert_eq!(
            entries[2].url,
            "git@github.com:DraconDev/web-games-polis.git",
            "first URL wins when multiple are declared"
        );
        assert_eq!(entries[2].sha, head_sha);
    }

    #[test]
    fn list_submodules_returns_empty_sha_when_not_in_index() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().to_path_buf();
        let _head = init_parent_repo(&parent);

        // .gitmodules declares a submodule, but the parent's index
        // does NOT have a gitlink entry for it (e.g. submodule was
        // removed from the working tree but .gitmodules wasn't
        // updated, or the index is stale).
        let gitmodules = "[submodule \"web-games-junk-runner\"]\n\
                          \tpath = web/games/wip/junk-runner\n\
                          \turl = git@github.com:DraconDev/web-games-junk-runner.git\n";
        fs::write(parent.join(".gitmodules"), gitmodules).unwrap();

        let entries = list_submodules(&parent);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "web-games-junk-runner");
        assert_eq!(
            entries[0].sha, "",
            "missing-from-index submodules must surface as sha = '' (caller skips them)"
        );
    }

    #[test]
    fn list_submodules_handles_broken_gitmodules() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().to_path_buf();
        let _head = init_parent_repo(&parent);

        // Garbage in .gitmodules: must not panic, must return empty.
        fs::write(parent.join(".gitmodules"), "this is not\na valid gitconfig [[[\n").unwrap();
        assert_eq!(list_submodules(&parent), Vec::<SubmoduleEntry>::new());
    }

    #[test]
    fn list_submodules_ignores_unknown_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().to_path_buf();
        let _head = init_parent_repo(&parent);

        // Has a non-submodule section — must be ignored.
        let gitmodules = "[core]\n\
                          \trepositoryformatversion = 0\n\
                          [submodule \"web-games-polis\"]\n\
                          \tpath = web/games/wip/polis\n\
                          \turl = git@github.com:DraconDev/web-games-polis.git\n";
        fs::write(parent.join(".gitmodules"), gitmodules).unwrap();

        let entries = list_submodules(&parent);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "web-games-polis");
    }

    #[test]
    fn discover_git_repos_finds_submodule_candidates() {
        // End-to-end: a parent repo with 3 submodules in .gitmodules
        // should produce just 1 candidate (the parent) when the
        // nested paths exist as worktrees (nested-on-main
        // architecture, goal `mr3g843f-lajfpg`).
        //
        // OLD BEHAVIOR (removed 2026-07-02):
        // produced 4 candidates: the parent + 3 submodule worktree
        // anchor paths under the watch root.
        let tmp = tempfile::tempdir().unwrap();
        let parent_dir = tmp.path().join("dracon-platform");
        fs::create_dir_all(&parent_dir).unwrap();
        let _head = init_parent_repo(&parent_dir);

        // 3 submodules in .gitmodules.
        let gitmodules = "[submodule \"web-games-polis\"]\n\
                          \tpath = web/games/wip/polis\n\
                          \turl = git@github.com:DraconDev/web-games-polis.git\n\
                          [submodule \"web-games-deathrun\"]\n\
                          \tpath = web/games/wip/deathrun\n\
                          \turl = git@github.com:DraconDev/web-games-deathrun.git\n\
                          [submodule \"web-games-junk-runner\"]\n\
                          \tpath = web/games/wip/junk-runner\n\
                          \turl = git@github.com:DraconDev/web-games-junk-runner.git\n";
        fs::write(parent_dir.join(".gitmodules"), gitmodules).unwrap();
        // Stage gitlinks in the index.
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&parent_dir)
            .output()
            .unwrap()
            .stdout;
        let head_sha = String::from_utf8_lossy(&head).trim().to_string();
        for path in ["web/games/wip/polis", "web/games/wip/deathrun", "web/games/wip/junk-runner"] {
            Command::new("git")
                .args(["update-index", "--add", "--cacheinfo", &format!("160000,{},{}", head_sha, path)])
                .current_dir(&parent_dir)
                .output()
                .unwrap();
        }

        // Watch root is `tmp.path()` (the dir containing
        // `dracon-platform/`). discover_git_repos should find
        // the parent + 3 submodule candidate paths under tmp.path().
        let roots = vec![tmp.path().to_path_buf()];
        let excluded: BTreeSet<String> = BTreeSet::new();
        let exclude_repos: Vec<String> = vec![];
        let discovered = discover_git_repos(&roots, &excluded, &exclude_repos, None);

        // The parent + 3 submodule candidates.
        assert_eq!(
            discovered.len(),
            4,
            "expected 1 parent + 3 submodules = 4 entries, got: {:?}",
            discovered
        );

        // The parent must be present.
        assert!(discovered.contains(&parent_dir), "parent not in discovered list");

        // The 3 submodule candidates (under tmp.path() because
        // tmp.path() is the anchor root).
        assert!(discovered.contains(&tmp.path().join("polis")));
        assert!(discovered.contains(&tmp.path().join("deathrun")));
        assert!(discovered.contains(&tmp.path().join("junk-runner")));
    }

    #[test]
    fn discover_git_repos_finds_no_submodule_candidates_when_no_gitmodules() {
        // Parent with no .gitmodules: discover should return
        // just the parent (no submodule candidates).
        let tmp = tempfile::tempdir().unwrap();
        let parent_dir = tmp.path().join("dracon-platform");
        fs::create_dir_all(&parent_dir).unwrap();
        let _head = init_parent_repo(&parent_dir);

        let roots = vec![tmp.path().to_path_buf()];
        let excluded: BTreeSet<String> = BTreeSet::new();
        let exclude_repos: Vec<String> = vec![];
        let discovered = discover_git_repos(&roots, &excluded, &exclude_repos, None);

        assert_eq!(discovered.len(), 1, "expected 1 parent, got: {:?}", discovered);
        assert_eq!(discovered[0], parent_dir);
    }

    #[test]
    fn discover_git_repos_dedups_standalone_with_nested_submodule() {
        // Regression test for the duplicate-row problem:
        // after `materialize_submodule` creates a standalone
        // worktree at the watch root, the daemon would
        // normally also discover the nested submodule at
        // `<parent>/<path>/` and treat it as a separate repo.
        // Both point at the same shared gitdir.
        //
        // CHANGED 2026-07-02 (goal `mr3g843f-lajfpg`):
        // Nested-on-main architecture. The NESTED submodule
        // checkout is now the canonical watch path. The
        // standalone at the watch root is the duplicate that
        // gets filtered out. The fix filters the standalone
        // when the nested submodule is also discovered.
        //
        // Setup:
        // - parent (`tmp.path()/dracon-platform`)
        // - nested submodule (`<parent>/web/games/wip/polis`)
        //   with `.git` file pointing to the shared gitdir
        // - shared gitdir at `<parent>/.git/modules/web-games-polis`
        // - standalone worktree at `tmp.path()/polis` with
        //   `.git` pointing to the shared gitdir
        //
        // Discovery must return the parent + the standalone
        // (NOT the nested submodule).
        let tmp = tempfile::tempdir().unwrap();
        let parent_dir = tmp.path().join("dracon-platform");
        let standalone_dir = tmp.path().join("polis");
        let nested_dir = parent_dir.join("web/games/wip/polis");

        fs::create_dir_all(&parent_dir).unwrap();
        let _head = init_parent_repo(&parent_dir);

        // Build a real submodule at <parent>/web/games/wip/polis
        // with its own .git/ that becomes the shared gitdir.
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(nested_dir.join("README.md"), b"# polis\n").unwrap();
        init_parent_repo(&nested_dir);
        let sub_sha = {
            let o = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&nested_dir)
                .output()
                .unwrap();
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };

        // Move <nested>/.git to <parent>/.git/modules/web-games-polis
        // and create a .git file pointing to it.
        let parent_gitdir = parent_dir.join(".git/modules/web-games-polis");
        fs::create_dir_all(parent_gitdir.parent().unwrap()).unwrap();
        copy_dir_recursive(&nested_dir.join(".git"), &parent_gitdir);

        let nested_dot_git = nested_dir.join(".git");
        fs::remove_dir_all(&nested_dot_git).ok();
        // Nested path: <tmp>/dracon-platform/web/games/wip/polis
        // 4 `..` from here reaches <tmp>/dracon-platform/, so the
        // gitdir is `.git/modules/web-games-polis` from there.
        fs::write(
            &nested_dot_git,
            b"gitdir: ../../../../.git/modules/web-games-polis\n",
        )
        .unwrap();

        // Register the gitlink in the parent.
        Command::new("git")
            .args([
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},web/games/wip/polis", sub_sha),
            ])
            .current_dir(&parent_dir)
            .output()
            .unwrap();

        // Materialize the standalone at <tmp.path()>/polis.
        // It must point to the same shared gitdir.
        fs::create_dir_all(&standalone_dir).unwrap();
        fs::write(standalone_dir.join(".git"), b"gitdir: ../dracon-platform/.git/modules/web-games-polis\n").unwrap();

        let roots = vec![tmp.path().to_path_buf()];
        let excluded: BTreeSet<String> = BTreeSet::new();
        let exclude_repos: Vec<String> = vec![];
        let discovered = discover_git_repos(&roots, &excluded, &exclude_repos, None);

        assert!(
            discovered.contains(&parent_dir),
            "parent must be in discovered list: {:?}",
            discovered
        );
        assert!(
            discovered.contains(&nested_dir),
            "nested submodule must be in discovered list (canonical watch path): {:?}",
            discovered
        );
        assert!(
            !discovered.contains(&standalone_dir),
            "standalone must be filtered out (duplicate of nested submodule): {:?}",
            discovered
        );
    }

    /// Recursive copy used by the duplicate-row test.
    fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
        fs::create_dir_all(dst).unwrap();
        for e in fs::read_dir(src).unwrap() {
            let e = e.unwrap();
            let from = e.path();
            let to = dst.join(e.file_name());
            if from.is_dir() {
                copy_dir_recursive(&from, &to);
            } else {
                fs::copy(&from, &to).unwrap();
            }
        }
    }
}
