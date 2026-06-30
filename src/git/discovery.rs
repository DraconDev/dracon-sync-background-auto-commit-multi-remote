//! Repository discovery — find git repos under watch roots.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

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
    repos.retain(|r| {
        let abs = r.to_string_lossy().to_lowercase();
        let name = r
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        !exlude_set.contains(&abs) && !exlude_set.contains(&name)
    });

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
