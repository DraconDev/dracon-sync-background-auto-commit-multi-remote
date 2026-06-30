//! Repository discovery — find git repos under watch roots.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

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
}
