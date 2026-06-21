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
