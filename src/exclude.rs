use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

use crate::policy::{debug_enabled, SyncPolicy};

pub(crate) fn normalized_dir_name(value: &str) -> String {
    value.trim_matches('/').to_ascii_lowercase()
}

pub(crate) fn excluded_dir_names_set(policy: &SyncPolicy) -> BTreeSet<String> {
    policy
        .exclude_dir_names
        .iter()
        .map(|d| normalized_dir_name(d))
        .filter(|d| !d.is_empty())
        .collect()
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_excluded_dir_name_exact() {
        let excluded: BTreeSet<String> = ["target", "node_modules", ".cache"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert!(is_excluded_dir_name("target", &excluded));
        assert!(is_excluded_dir_name("node_modules", &excluded));
        assert!(is_excluded_dir_name(".cache", &excluded));
        assert!(!is_excluded_dir_name("src", &excluded));
    }

    #[test]
    fn test_is_excluded_dir_name_pattern() {
        let excluded: BTreeSet<String> = [".tmp-".to_string()].into_iter().collect();

        assert!(is_excluded_dir_name(".tmp-abc", &excluded));
        assert!(is_excluded_dir_name(".tmp-123", &excluded));
    }

    #[test]
    fn test_is_excluded_dir_name_trailing_hyphen() {
        let excluded: BTreeSet<String> = [".tmp-".to_string()].into_iter().collect();
        // .tmp- matches .tmp-* (the hyphen is part of the prefix)
        assert!(is_excluded_dir_name(".tmp-file", &excluded));
        assert!(is_excluded_dir_name(".tmp-abc", &excluded));
        assert!(is_excluded_dir_name(".tmp-123", &excluded));
        // .tmpfile does NOT start with .tmp- (no hyphen), so NOT excluded
        assert!(!is_excluded_dir_name(".tmpfile", &excluded));
    }

    #[test]
    fn test_is_excluded_dir_name_empty_excluded_set() {
        let excluded: BTreeSet<String> = BTreeSet::new();
        assert!(!is_excluded_dir_name("target", &excluded));
        assert!(!is_excluded_dir_name("node_modules", &excluded));
    }

    #[test]
    fn test_is_excluded_dir_name_case_insensitive_matching() {
        let excluded: BTreeSet<String> = ["Target".to_string()].into_iter().collect();
        assert!(is_excluded_dir_name("target", &excluded));
        assert!(is_excluded_dir_name("Target", &excluded));
    }

    #[test]
    fn test_is_excluded_dir_name_star_prefix() {
        let excluded: BTreeSet<String> = ["build*".to_string()].into_iter().collect();
        assert!(is_excluded_dir_name("build", &excluded));
        assert!(is_excluded_dir_name("build-debug", &excluded));
        assert!(!is_excluded_dir_name("abuild", &excluded));
    }

    #[test]
    fn test_is_excluded_change_path_simple() {
        let excluded: BTreeSet<String> = ["target", "node_modules"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(is_excluded_change_path(
            Path::new("target/file.txt"),
            &excluded
        ));
        assert!(is_excluded_change_path(
            Path::new("target/deep/nested/file.txt"),
            &excluded
        ));
        assert!(is_excluded_change_path(
            Path::new("node_modules/package/index.js"),
            &excluded
        ));
        assert!(!is_excluded_change_path(
            Path::new("src/file.txt"),
            &excluded
        ));
        assert!(!is_excluded_change_path(
            Path::new("source/file.txt"),
            &excluded
        ));
    }

    #[test]
    fn test_matches_file_pattern_exact() {
        assert!(matches_file_pattern("test.txt", "test.txt"));
        assert!(!matches_file_pattern("test.txt", "Test.txt"));
    }

    #[test]
    fn test_matches_file_pattern_extension() {
        assert!(matches_file_pattern("test.txt", "*.txt"));
        assert!(matches_file_pattern("test.md", "*.md"));
        assert!(!matches_file_pattern("test.txt", "*.md"));
    }

    #[test]
    fn test_matches_file_pattern_prefix() {
        assert!(matches_file_pattern("test.output", "test.*"));
        assert!(matches_file_pattern("test.txt", "test.*"));
        assert!(!matches_file_pattern("other.output", "test.*"));
    }

    #[test]
    fn test_matches_file_pattern_glob() {
        assert!(matches_file_pattern("build-debug", "build*"));
        assert!(matches_file_pattern("build-release", "build*"));
        assert!(matches_file_pattern("build", "build*"));
        assert!(!matches_file_pattern("abuild", "build*"));
    }

    #[test]
    fn test_is_excluded_file_simple() {
        let patterns = vec!["*.log".to_string(), "*.tmp".to_string()];
        assert!(is_excluded_file(Path::new("error.log"), &patterns));
        assert!(is_excluded_file(Path::new("temp.tmp"), &patterns));
        assert!(!is_excluded_file(Path::new("file.txt"), &patterns));
        assert!(!is_excluded_file(Path::new("error.log.bak"), &patterns));
    }

    #[test]
    fn test_is_excluded_file_no_match() {
        let patterns: Vec<String> = vec![];
        assert!(!is_excluded_file(Path::new("file.txt"), &patterns));
    }

    #[test]
    fn test_is_excluded_file_empty_path() {
        let patterns = vec!["*.txt".to_string()];
        assert!(!is_excluded_file(Path::new(""), &patterns));
    }

    #[test]
    fn test_normalized_dir_name_various() {
        assert_eq!(normalized_dir_name("TARGET"), "target");
        assert_eq!(normalized_dir_name("//node_modules//"), "node_modules");
        assert_eq!(normalized_dir_name(".Git"), ".git");
    }

    #[test]
    fn test_can_restore_entry_modified() {
        use dracon_git::types::{DiffFile, FileStatus};
        let entry = DiffFile::new(PathBuf::from("src/main.rs"), FileStatus::Modified);
        assert!(can_restore_entry(Path::new("/repo"), &entry));
    }

    #[test]
    fn test_can_restore_entry_deleted() {
        use dracon_git::types::{DiffFile, FileStatus};
        let entry = DiffFile::new(PathBuf::from("src/main.rs"), FileStatus::Deleted);
        assert!(!can_restore_entry(Path::new("/repo"), &entry));
    }

    #[test]
    fn test_can_restore_entry_added() {
        use dracon_git::types::{DiffFile, FileStatus};
        let entry = DiffFile::new(PathBuf::from("newfile.txt"), FileStatus::Added);
        assert!(!can_restore_entry(Path::new("/repo"), &entry));
    }

    #[test]
    fn test_is_large_untracked_added_file() {
        use dracon_git::types::{DiffFile, FileStatus};
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let large_file = repo.join("large.bin");
        std::fs::write(&large_file, vec![0u8; 200]).unwrap();
        let entry = DiffFile::new(PathBuf::from("large.bin"), FileStatus::Added);
        // 200 bytes > 100 bytes threshold
        assert!(is_large_untracked(&entry, repo, 100));
    }

    #[test]
    fn test_is_large_untracked_modified_file() {
        use dracon_git::types::{DiffFile, FileStatus};
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let file = repo.join("small.txt");
        std::fs::write(&file, vec![0u8; 50]).unwrap();
        let entry = DiffFile::new(PathBuf::from("small.txt"), FileStatus::Modified);
        // 50 bytes < 100 bytes threshold
        assert!(!is_large_untracked(&entry, repo, 100));
    }

    #[test]
    fn test_is_large_untracked_nonexistent_file() {
        use dracon_git::types::{DiffFile, FileStatus};
        let entry = DiffFile::new(PathBuf::from("nonexistent.txt"), FileStatus::Added);
        assert!(!is_large_untracked(&entry, Path::new("/nonexistent"), 100));
    }

    #[test]
    fn test_has_sync_relevant_dirty_entries_modified() {
        use dracon_git::types::{DiffFile, FileStatus};
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::write(repo.join("test.txt"), "content").unwrap();
        let entries = vec![DiffFile::new(
            PathBuf::from("test.txt"),
            FileStatus::Modified,
        )];
        let excluded: BTreeSet<String> = BTreeSet::new();
        assert!(has_sync_relevant_dirty_entries(
            repo,
            &entries,
            &excluded,
            &[],
            100 * 1024 * 1024,
            &[],
        ));
    }

    #[test]
    fn test_has_sync_relevant_dirty_entries_excluded_dir_ignored() {
        use dracon_git::types::{DiffFile, FileStatus};
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join("target")).unwrap();
        std::fs::write(repo.join("target").join("file.txt"), "content").unwrap();
        let entries = vec![DiffFile::new(
            PathBuf::from("target/file.txt"),
            FileStatus::Added,
        )];
        let excluded: BTreeSet<String> = ["target".to_string()].into_iter().collect();
        assert!(
            !has_sync_relevant_dirty_entries(repo, &entries, &excluded, &[], 100 * 1024 * 1024, &[]),
            "untracked file in excluded dir should be ignored (not large, not restorable)"
        );
    }

    #[test]
    fn test_has_sync_relevant_dirty_entries_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let entries: Vec<dracon_git::types::DiffFile> = vec![];
        let excluded: BTreeSet<String> = BTreeSet::new();
        assert!(!has_sync_relevant_dirty_entries(
            repo,
            &entries,
            &excluded,
            &[],
            100 * 1024 * 1024,
            &[],
        ));
    }

    #[test]
    fn test_remove_tracked_excluded_paths_none_found() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("test.txt"), "content\n").unwrap();
        crate::git::git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .output()
            .unwrap();
        crate::git::git_cmd()
            .args(["commit", "-q", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        let excluded: BTreeSet<String> = ["nonexistent".to_string()].into_iter().collect();
        let result = remove_tracked_excluded_paths(repo, &excluded).unwrap();
        assert_eq!(
            result, None,
            "should return None when no tracked excluded paths found"
        );
    }

    #[test]
    fn test_append_to_gitignore_creates_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        assert!(!repo.join(".gitignore").exists());
        let patterns = vec!["target/".to_string(), "*.log".to_string()];
        let result = append_to_gitignore(repo, &patterns);
        assert!(result.is_ok());
        assert!(repo.join(".gitignore").exists());
        let content = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(content.contains("target/"));
        assert!(content.contains("*.log"));
    }

    #[test]
    fn test_append_to_gitignore_deduplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        let patterns = vec!["target/".to_string()];
        let result = append_to_gitignore(repo, &patterns);
        assert!(result.is_ok());
        let content = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        let count = content.lines().filter(|l| *l == "target/").count();
        assert_eq!(count, 1, "should not duplicate existing pattern");
    }

    #[test]
    fn test_append_to_gitignore_empty_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let patterns: Vec<String> = vec![];
        let result = append_to_gitignore(repo, &patterns);
        assert!(result.is_ok());
        assert!(
            !repo.join(".gitignore").exists(),
            "should not create .gitignore for empty patterns"
        );
    }

    #[test]
    fn test_matches_file_pattern_exact_match() {
        assert!(matches_file_pattern("Cargo.lock", "Cargo.lock"));
        assert!(!matches_file_pattern("Cargo.toml", "Cargo.lock"));
    }

    #[test]
    fn test_matches_file_pattern_extension_wildcard() {
        assert!(matches_file_pattern("test.rs", "*.rs"));
        assert!(matches_file_pattern("lib.rs", "*.rs"));
        assert!(!matches_file_pattern("test.txt", "*.rs"));
    }

    #[test]
    fn test_matches_file_pattern_prefix_wildcard() {
        assert!(matches_file_pattern("test.log", "test.*"));
        assert!(matches_file_pattern("test.log.bak", "test.*"));
        assert!(!matches_file_pattern("other.log", "test.*"));
    }

    #[test]
    fn test_matches_file_pattern_middle_wildcard() {
        assert!(matches_file_pattern("data.json.gz", "*.json.gz"));
        assert!(matches_file_pattern(
            "test.backup.json.gz",
            "*.backup.json.gz"
        ));
        assert!(!matches_file_pattern("data.json", "*.json.gz"));
    }

    #[test]
    fn test_is_excluded_file_pattern_matching() {
        let patterns = vec!["*.log".to_string(), "*.tmp".to_string()];
        let path = std::path::Path::new("debug.log");
        assert!(is_excluded_file(path, &patterns));
        let path2 = std::path::Path::new("data.tmp");
        assert!(is_excluded_file(path2, &patterns));
        let path3 = std::path::Path::new("data.rs");
        assert!(!is_excluded_file(path3, &patterns));
    }

    // ============================================================
    // should_stage_entry: auto_commit_exclude_patterns tests
    // ============================================================

    fn make_modified_entry(path: &str) -> dracon_git::types::DiffFile {
        use dracon_git::types::{DiffFile, FileStatus};
        DiffFile::new(PathBuf::from(path), FileStatus::Modified)
    }

    #[test]
    fn test_should_stage_entry_tracked_modified_excluded_by_pattern() {
        // A tracked, modified file matching the per-repo
        // auto_commit_exclude_patterns should NOT be staged.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join("web/test-results")).unwrap();
        std::fs::write(repo.join("web/test-results/slice13.png"), b"PNGDATA").unwrap();
        let entry = make_modified_entry("web/test-results/slice13.png");
        let excluded: BTreeSet<String> = BTreeSet::new();
        let patterns = vec!["**/test-results/**".to_string()];
        assert!(
            !should_stage_entry(
                repo,
                &entry,
                &excluded,
                &[],
                100 * 1024 * 1024,
                &patterns,
            ),
            "test-results PNG should be excluded by auto_commit_exclude_patterns"
        );
    }

    #[test]
    fn test_should_stage_entry_tracked_modified_no_match() {
        // A tracked, modified file that does NOT match the pattern
        // should still be staged.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/main.rs"), b"fn main(){}").unwrap();
        let entry = make_modified_entry("src/main.rs");
        let excluded: BTreeSet<String> = BTreeSet::new();
        let patterns = vec!["**/test-results/**".to_string()];
        assert!(should_stage_entry(
            repo,
            &entry,
            &excluded,
            &[],
            100 * 1024 * 1024,
            &patterns,
        ));
    }

    #[test]
    fn test_should_stage_entry_empty_patterns_list() {
        // Empty patterns list should behave the same as before the
        // change: files are staged unless excluded by other means.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/main.rs"), b"fn main(){}").unwrap();
        let entry = make_modified_entry("src/main.rs");
        let excluded: BTreeSet<String> = BTreeSet::new();
        assert!(should_stage_entry(
            repo,
            &entry,
            &excluded,
            &[],
            100 * 1024 * 1024,
            &[],
        ));
    }

    // -----------------------------------------------------------------
    // is_gitlink() — github-style tests under `#[cfg(test)]`.
    //
    // We avoid `tempfile` here and use a tiny shell-script-based approach
    // because the helper is a thin wrapper around `git ls-tree HEAD -- <p>`.
    // The interesting question is "does the helper detect a 160000 entry
    // correctly", which we verify by directly invoking `git` against a
    // hand-rolled repo. Using `tempfile::tempdir` would require pulling
    // in the `tempfile` dev-dep at this layer; since `submodule-sync`
    // tests already exercise the real flow end-to-end via
    // `stage_existing_files` (see sync.rs tests mod), these `is_gitlink`
    // tests just cover the explicit git-mode-prefix-detection corner
    // cases cheaply.
    // -----------------------------------------------------------------

    /// Runs `git -C <repo>` with the given args, returns stdout as a String
    /// (or empty string on failure).
    fn git_c(repo: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => String::new(),
        }
    }

    #[test]
    fn test_is_gitlink_returns_true_for_tracked_gitlink() {
        let td = tempfile::tempdir().unwrap();
        let repo = td.path().join("parent");
        std::fs::create_dir_all(&repo).unwrap();
        git_c(&repo, &["init", "-q", "-b", "main"]);
        git_c(&repo, &["config", "user.email", "t@t"]);
        git_c(&repo, &["config", "user.name", "t"]);

        // Build a real nested git repo at parent/submod/{.git,foo.txt}.
        let sub = repo.join("submod");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("foo.txt"), b"hi").unwrap();
        git_c(&sub, &["init", "-q", "-b", "main"]);
        git_c(&sub, &["config", "user.email", "t@t"]);
        git_c(&sub, &["config", "user.name", "t"]);
        git_c(&sub, &["add", "foo.txt"]);
        git_c(&sub, &["commit", "-q", "-m", "init"]);
        let sub_sha = git_c(&sub, &["rev-parse", "HEAD"]).trim().to_string();
        assert!(!sub_sha.is_empty());

        // Register the submodule in the parent via `git add <path>` of
        // the gitlink (the standard way — no need for `.gitmodules`).
        git_c(&repo, &["add", "submod"]);
        git_c(&repo, &["commit", "-q", "-m", "register submod"]);

        // Sanity: `git ls-tree HEAD -- submod` should report 160000.
        let ls = git_c(&repo, &["ls-tree", "HEAD", "--", "submod"]);
        assert!(
            ls.starts_with("160000 "),
            "expected `160000 commit ...\tsubmod` from ls-tree, got: {}",
            ls
        );

        assert!(is_gitlink(&repo, std::path::Path::new("submod")));
    }

    #[test]
    fn test_is_gitlink_returns_false_for_regular_file() {
        let td = tempfile::tempdir().unwrap();
        let repo = td.path().join("parent");
        std::fs::create_dir_all(&repo).unwrap();
        git_c(&repo, &["init", "-q", "-b", "main"]);
        git_c(&repo, &["config", "user.email", "t@t"]);
        git_c(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("regular.txt"), b"hi").unwrap();
        git_c(&repo, &["add", "regular.txt"]);
        git_c(&repo, &["commit", "-q", "-m", "init"]);

        assert!(!is_gitlink(&repo, std::path::Path::new("regular.txt")));
    }

    #[test]
    fn test_is_gitlink_returns_false_for_untracked_dir_with_dotgit() {
        // Untracked sibling subrepo (real `.git/` dir but no parent gitlink)
        // must NOT be classified as a gitlink. This is the case where the
        // daemon should fall through to the existing skip-and-recurse logic.
        let td = tempfile::tempdir().unwrap();
        let repo = td.path().join("parent");
        std::fs::create_dir_all(&repo).unwrap();
        git_c(&repo, &["init", "-q", "-b", "main"]);
        git_c(&repo, &["config", "user.email", "t@t"]);
        git_c(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("regular.txt"), b"hi").unwrap();
        git_c(&repo, &["add", "regular.txt"]);
        git_c(&repo, &["commit", "-q", "-m", "init"]);

        // Untracked subrepo on disk (no entry in parent index).
        let sub = repo.join("nested_subrepo");
        std::fs::create_dir_all(sub.join(".git")).unwrap();
        std::fs::write(sub.join("foo.txt"), b"hi").unwrap();

        assert!(!is_gitlink(&repo, std::path::Path::new("nested_subrepo")));
    }

    #[test]
    fn test_is_gitlink_returns_false_for_missing_path() {
        let td = tempfile::tempdir().unwrap();
        let repo = td.path().join("parent");
        std::fs::create_dir_all(&repo).unwrap();
        git_c(&repo, &["init", "-q", "-b", "main"]);
        git_c(&repo, &["config", "user.email", "t@t"]);
        git_c(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("regular.txt"), b"hi").unwrap();
        git_c(&repo, &["add", "regular.txt"]);
        git_c(&repo, &["commit", "-q", "-m", "init"]);

        // `git ls-tree HEAD -- does_not_exist` exits non-zero with empty stdout.
        // Our helper must return false in that case.
        assert!(!is_gitlink(&repo, std::path::Path::new("does_not_exist")));
    }

    // -----------------------------------------------------------------
    // is_gitlink_unchanged() — regression test for the 2026-07-01
    // parent-gitlink propagation bug. The original implementation
    // compared the parent's tracked gitlink against the nested
    // submodule's CHECKOUT HEAD. When the daemon also creates a
    // STANDALONE worktree of the same shared gitdir (via
    // `materialize_submodule`), the standalone commits advance the
    // shared gitdir's `refs/heads/main` while the nested checkout
    // HEAD stays at the OLD SHA. Result: the parent's gitlink
    // silently falls behind, and `is_gitlink_unchanged` returns
    // true (filtering the entry out of `to_stage`) so the gitlink
    // never updates.
    //
    // The fix reads the SHARED gitdir's `refs/heads/main` and
    // compares against the parent's tracked gitlink. When they
    // differ, return false (the entry is "changed" and must be
    // staged via `git add <path>`).
    //
    // This test simulates the structure produced by `materialize_submodule`:
    // - `<parent>/.git/modules/web-games-polis/` is the SHARED gitdir
    // - `<parent>/web/games/wip/polis/.git` is a file pointing to it
    // - `<parent>/web/games/wip/polis/HEAD` (the main worktree) is
    //   on `main` and lags behind the standalone
    // - `<parent>/.git/modules/web-games-polis/refs/heads/main` is
    //   advanced by the standalone's post-commit hook
    // -----------------------------------------------------------------

    /// Build a parent + standalone worktree pair in the style of
    /// `materialize_submodule`. Returns (parent_path, standalone_path,
    /// initial_sub_sha). The shared gitdir is at
    /// `<parent>/.git/modules/<sub_name>` and is laid out so the
    /// nested submodule at `<parent>/<sub_path>/.git` is a file
    /// pointing at it.
    fn build_parent_with_standalone_submodule() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        String,
    ) {
        let td = tempfile::tempdir().unwrap();
        let parent = td.path().join("parent");
        std::fs::create_dir_all(&parent).unwrap();

        git_c(&parent, &["init", "-q", "-b", "main"]);
        git_c(&parent, &["config", "user.email", "t@t"]);
        git_c(&parent, &["config", "user.name", "t"]);
        std::fs::write(parent.join("README.md"), b"# parent\n").unwrap();
        git_c(&parent, &["add", "README.md"]);
        git_c(&parent, &["commit", "-q", "-m", "init"]);

        // Build the subrepo's working tree + standalone path layout.
        let sub_name = "web-games-foo";
        let sub_path_rel = std::path::PathBuf::from("nested/foo");
        let nested_dir = parent.join(&sub_path_rel);
        let standalone_dir = td.path().join("standalone_foo");

        // The subrepo's own .git/ becomes the SHARED gitdir at
        // <parent>/.git/modules/<sub_name>.
        let shared_gitdir = parent.join(".git/modules").join(sub_name);
        std::fs::create_dir_all(&shared_gitdir).unwrap();
        // Move the subrepo's .git/ contents into the shared gitdir.
        let sub_dot_git = nested_dir.join(".git");
        std::fs::create_dir_all(&nested_dir).unwrap();
        std::fs::write(nested_dir.join("README.md"), b"# foo\n").unwrap();
        git_c(&nested_dir, &["init", "-q", "-b", "main"]);
        git_c(&nested_dir, &["config", "user.email", "t@t"]);
        git_c(&nested_dir, &["config", "user.name", "t"]);
        git_c(&nested_dir, &["add", "README.md"]);
        git_c(&nested_dir, &["commit", "-q", "-m", "init"]);

        // Capture sub_sha BEFORE we move the .git/ directory into
        // the shared location. After the move, nested_dir/.git is
        // a file (gitdir: pointer), and rev-parse still works
        // but reading it from the test is fragile.
        let sub_sha = git_c(&nested_dir, &["rev-parse", "HEAD"])
            .trim()
            .to_string();

        // Copy the subrepo's .git contents into the shared gitdir.
        fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
            std::fs::create_dir_all(dst).unwrap();
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let from = entry.path();
                let to = dst.join(entry.file_name());
                if from.is_dir() {
                    copy_dir(&from, &to);
                } else {
                    std::fs::copy(&from, &to).unwrap();
                }
            }
        }
        copy_dir(&sub_dot_git, &shared_gitdir);
        // Replace nested_dir/.git (a real directory) with a file
        // pointing to the shared gitdir. Use the relative path so
        // `git` resolves it correctly. Layout: nested_dir is at
        // `<tempdir>/parent/nested/foo/`, so 2 levels up (`../..`)
        // reaches `<tempdir>/parent/` where `.git/modules/<name>`
        // lives.
        std::fs::remove_dir_all(&sub_dot_git).unwrap();
        std::fs::write(
            &sub_dot_git,
            b"gitdir: ../../.git/modules/web-games-foo\n",
        )
        .unwrap();

        // Register the submodule as a gitlink in the parent.
        // sub_sha was captured earlier (before .git was replaced with
        // a file pointing to the shared gitdir).
        let cacheinfo = format!("160000,{},nested/foo", sub_sha);
        let update_args = vec![
            "update-index".to_string(),
            "--add".to_string(),
            "--cacheinfo".to_string(),
            cacheinfo,
        ];
        let update_refs: Vec<&str> = update_args.iter().map(String::as_str).collect();
        git_c(&parent, &update_refs);
        git_c(&parent, &["commit", "-q", "-m", "add submodule"]);

        (td, parent, standalone_dir, sub_sha)
    }

    #[test]
    fn test_is_gitlink_unchanged_false_when_shared_main_ahead_of_parent() {
        // Regression test for the 6/9 submodule propagation bug.
        //
        // Setup: parent tracks nested/foo as a gitlink at SHA_A.
        // The SHARED gitdir's refs/heads/main is advanced to SHA_B
        // (simulating a standalone worktree committing + the
        // fast_forward_daemon_standalone_to_main hook updating the
        // shared main ref). The nested submodule's checkout HEAD
        // stays at SHA_A.
        //
        // Before the fix: is_gitlink_unchanged returned true (nested
        // HEAD == parent gitlink), so the parent's gitlink was
        // never re-staged. After the fix: is_gitlink_unchanged
        // returns false (shared main != parent gitlink), allowing
        // the parent's gitlink to be updated.
        let (_td, parent, _standalone, _initial_sub_sha) =
            build_parent_with_standalone_submodule();

        // Advance the SHARED gitdir's refs/heads/main to a NEW commit.
        let shared_gitdir = parent.join(".git/modules/web-games-foo");
        let main_ref = shared_gitdir.join("refs/heads/main");
        let main_sha_before = std::fs::read_to_string(&main_ref)
            .unwrap()
            .trim()
            .to_string();
        let new_sha = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        std::fs::write(&main_ref, format!("{}\n", new_sha)).unwrap();

        // Parent's tracked gitlink should still be main_sha_before.
        let parent_link = git_c(&parent, &["ls-tree", "HEAD", "--", "nested/foo"])
            .trim()
            .to_string();
        assert!(
            parent_link.contains(&main_sha_before),
            "parent gitlink should still be {} before fix runs, got: {}",
            main_sha_before,
            parent_link
        );

        // CRITICAL: is_gitlink_unchanged must return FALSE because the
        // shared main ref is now ahead of the parent's tracked gitlink.
        // Before the 2026-07-01 fix, this returned TRUE (nested HEAD
        // matched parent gitlink), which silently dropped the entry
        // from `to_stage` and prevented the parent from seeing the
        // standalone's commits.
        assert!(
            !is_gitlink_unchanged(&parent, std::path::Path::new("nested/foo")),
            "is_gitlink_unchanged must return false when shared main ref ({} new_sha_before={}) is ahead of parent's tracked gitlink ({})",
            new_sha,
            main_sha_before,
            main_sha_before,
        );

        // Sanity: with the OLD behavior (shared main == parent gitlink),
        // is_gitlink_unchanged returns true.
        std::fs::write(&main_ref, format!("{}\n", main_sha_before)).unwrap();
        assert!(
            is_gitlink_unchanged(&parent, std::path::Path::new("nested/foo")),
            "is_gitlink_unchanged must return true when shared main == parent gitlink",
        );
    }
}

pub(crate) fn is_excluded_dir_name(name: &str, excluded_dir_names: &BTreeSet<String>) -> bool {
    let normalized = normalized_dir_name(name);
    for pattern in excluded_dir_names {
        let normalized_pattern = normalized_dir_name(pattern);
        if normalized_pattern == normalized {
            return true;
        }
        // .tmp- prefix pattern: matches .tmp-* only (e.g., .tmp-file, .tmp-abc)
        // NOT .tmpfile (no hyphen after .tmp) or .tmp (exact match handled above)
        if pattern.ends_with('-')
            && pattern.starts_with('.')
            && normalized.len() > normalized_pattern.len() - 1
            && normalized.as_bytes()[normalized_pattern.len() - 1] == b'-'
        {
            let prefix = &normalized[..normalized_pattern.len() - 1];
            if normalized.starts_with(prefix) {
                return true;
            }
        }
        // Glob-style * suffix: .build* matches .build-debug
        if pattern.ends_with('*') && normalized.starts_with(&pattern[..pattern.len() - 1]) {
            return true;
        }
    }
    false
}

pub(crate) fn is_excluded_change_path(path: &Path, excluded_dir_names: &BTreeSet<String>) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|name| is_excluded_dir_name(name, excluded_dir_names))
}

pub(crate) fn matches_file_pattern(file_name: &str, pattern: &str) -> bool {
    if pattern == file_name {
        return true;
    }
    if pattern.starts_with("*.") {
        let ext = &pattern[1..];
        if file_name.ends_with(ext) {
            return true;
        }
    }
    if pattern.ends_with(".*") {
        let prefix = &pattern[..pattern.len() - 1];
        if file_name.starts_with(prefix) {
            return true;
        }
    }
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let (prefix, suffix) = (parts[0], parts[1]);
            if file_name.starts_with(prefix) && file_name.ends_with(suffix) {
                return true;
            }
        }
    }
    false
}

pub(crate) fn is_excluded_file(file_path: &Path, excluded_patterns: &[String]) -> bool {
    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    for pattern in excluded_patterns {
        if matches_file_pattern(file_name, pattern) {
            return true;
        }
    }
    false
}

/// Match an untracked file path against `untracked_exclude_patterns`.
/// Supports two pattern styles:
/// 1. Simple basename match (e.g. `note.md` matches `subdir/note.md`)
/// 2. Glob match against the full path relative to repo (e.g.
///    `**/scratch/**` matches `foo/scratch/bar.txt`).
/// Used by the `untracked_exclude_patterns` policy field to keep user
/// notes, scratch research, and audit evidence out of auto-stage.
pub(crate) fn matches_untracked_exclude(
    repo: &Path,
    file_path: &Path,
    patterns: &[String],
) -> bool {
    let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Path relative to repo, with forward slashes for cross-platform glob match
    let rel = file_path
        .strip_prefix(repo)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");

    for pattern in patterns {
        // Basename-only patterns (e.g. `note.md`, `*.png`)
        if !pattern.contains('/') {
            if matches_file_pattern(file_name, pattern) {
                return true;
            }
            // Single-segment patterns like `*.png` — match against
            // the basename only.
            if matches_file_pattern(file_name, pattern) {
                return true;
            }
        }
        // Path-glob patterns (e.g. `**/scratch/**`, `.demon/**`).
        // Use the existing starts-with/contains substring logic.
        if pattern.starts_with("**/") || pattern.contains("/**") {
            if rel == pattern.trim_start_matches("**/").trim_end_matches("/**")
                || rel.starts_with(pattern.trim_end_matches("/**"))
                || rel.contains(pattern.trim_start_matches("**/").trim_end_matches("/**"))
            {
                return true;
            }
            // Fall back to substring match for `**/<name>` style patterns
            if let Some(tail) = pattern.strip_prefix("**/") {
                let tail = tail.trim_end_matches("/**");
                if rel.split('/').any(|seg| matches_file_pattern(seg, tail)) {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a path is a tracked gitlink (mode 160000) in the parent.
/// Returns true iff `git ls-tree HEAD -- <path>` reports a `160000`
/// entry for `path`. This is the only signal that the parent
/// references the directory through a gitlink pointer — distinct
/// from "directory exists on disk with a `.git/` inside" (which can
/// also be an untracked sibling subrepo with no gitlink).
///
/// Used by `stage_commit_and_push` in `sync.rs` to partition
/// `to_stage` into gitlink-pointer updates (handled by
/// `stage_gitlink_updates` via `git add <path>`) vs regular
/// files (handled by `stage_existing_files` via `git add -A`).
pub(crate) fn is_gitlink(repo: &Path, path: &Path) -> bool {
    let output = crate::git::git_cmd()
        .current_dir(repo)
        .args(["ls-tree", "HEAD", "--"])
        .arg(path)
        .output();
    let Ok(out) = output else { return false };
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Format: "160000 commit <sha>\t<path>"
    stdout.starts_with("160000 ")
}

/// Resolve the SHARED gitdir path for a nested submodule at
/// `<repo>/<path>` (a gitlink entry in the parent).
///
/// Submodule checkouts are not standalone repos: their `.git`
/// is a file containing `gitdir: <relative-path-to-shared-gitdir>`,
/// where the shared gitdir is `<parent>/.git/modules/<name>`
/// (created by `git submodule update --init`). The actual
/// refs (`refs/heads/main`, etc.) live in the shared gitdir.
///
/// Returns the canonicalized shared gitdir path on success,
/// or `None` if the path is not a gitlink-style submodule
/// checkout (e.g., the `.git` file is missing, malformed,
/// or the resolved path doesn't exist).
///
/// ADDED 2026-07-01, goal `mr10pdzr-i495vy`:
/// The parent-gitlink propagation fix needs to compare the
/// parent's tracked SHA against the SHARED gitdir's
/// `refs/heads/main` (which is what the standalone
/// worktree's `fast_forward_daemon_standalone_to_main`
/// hook updates). Reading the nested submodule's checkout
/// HEAD is NOT enough, because the nested checkout is a
/// separate worktree of the shared gitdir and its HEAD
/// stays at the OLD SHA even after a standalone commit +
/// fast-forward. This helper lets the caller walk to the
/// shared gitdir's `main` ref to detect stale pointers.
fn shared_submodule_gitdir(repo: &Path, path: &Path) -> Option<std::path::PathBuf> {
    let dot_git = repo.join(path).join(".git");
    if !dot_git.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let rest = content.trim().strip_prefix("gitdir:")?;
    let gitdir_rel = rest.trim();
    // The .git file's gitdir is relative to its own directory
    // (the submodule's working tree root, i.e. `<repo>/<path>`).
    let base = repo.join(path);
    let resolved = base.join(gitdir_rel);
    // Canonicalize to resolve `..` segments and symlinks.
    let canonical = std::fs::canonicalize(&resolved).ok()?;
    if !canonical.is_dir() {
        return None;
    }
    Some(canonical)
}

/// Check if a path is a gitlink (mode 160000) with an unchanged pointer.
/// Returns true if the entry is a submodule-like directory whose HEAD commit
/// matches what the parent repo tracks, meaning the "dirty" state is just
/// the submodule's own working tree being dirty (not a pointer change).
///
/// ADDED 2026-07-01, goal `mr10pdzr-i495vy`:
/// The original implementation compared the parent's tracked gitlink SHA
/// against the nested submodule's CHECKOUT HEAD (`git -C <path> rev-parse
/// HEAD`). That works for the `materialize_submodule` use case where the
/// nested submodule IS the only worktree of the shared gitdir. But after
/// the daemon also creates a STANDALONE worktree (`/home/dracon/Dev/<name>/`)
/// of the same shared gitdir, the standalone commits advance the shared
/// gitdir's `refs/heads/main` (via the post-commit
/// `fast_forward_daemon_standalone_to_main` hook), while the nested
/// submodule's checkout HEAD stays at the OLD SHA because git worktrees
/// have independent HEAD refs. As a result, the parent's gitlink
/// (which tracks `main`) silently falls behind the standalone's commits,
/// and the daemon's partition filter (`is_gitlink_unchanged` returning
/// true → entry removed from `to_stage`) prevents the parent's gitlink
/// from being updated.
///
/// The fix: in addition to comparing against the nested checkout HEAD,
/// compare against the SHARED gitdir's `refs/heads/main`. If the shared
/// `main` ref differs from the parent's tracked gitlink, the pointer is
/// STALE and must be re-staged via `git add <path>`.
pub(crate) fn is_gitlink_unchanged(repo: &Path, path: &Path) -> bool {
    let output = crate::git::git_cmd()
        .current_dir(repo)
        .args(["ls-tree", "HEAD", "--"])
        .arg(path)
        .output();
    let Ok(out) = output else { return false };
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Format: "160000 commit <sha>\t<path>"
    if !stdout.starts_with("160000 ") {
        return false;
    }
    let Some(sha) = stdout.split_whitespace().nth(2) else {
        return false;
    };
    // Primary check: does the shared gitdir's `main` ref match the
    // parent's tracked gitlink? If they differ, the standalone
    // worktree has commits the parent hasn't seen yet — the pointer
    // is STALE and must be staged for update.
    if let Some(shared_gitdir) = shared_submodule_gitdir(repo, path) {
        let main_ref = shared_gitdir.join("refs/heads/main");
        if let Ok(main_content) = std::fs::read_to_string(&main_ref) {
            let shared_main = main_content.trim();
            if !shared_main.is_empty() && shared_main != sha {
                // Shared `main` ref is ahead of parent's tracked gitlink.
                // The pointer is stale — return false so the entry is NOT
                // filtered out, allowing `stage_gitlink_updates` to
                // run `git add <path>` and update the parent's gitlink.
                return false;
            }
        }
    }
    // Fallback (original behavior): check the nested submodule's
    // checkout HEAD. This is still correct for the case where there's
    // no standalone worktree (so the shared `main` ref == nested HEAD).
    let sub_output = crate::git::git_cmd()
        .current_dir(repo.join(path))
        .args(["rev-parse", "HEAD"])
        .output();
    let Ok(sub_out) = sub_output else {
        return false;
    };
    let sub_sha = String::from_utf8_lossy(&sub_out.stdout).trim().to_string();
    sub_sha == sha
}

pub(crate) fn should_stage_entry(
    repo: &Path,
    entry: &dracon_git::types::DiffFile,
    excluded_dir_names: &BTreeSet<String>,
    excluded_file_patterns: &[String],
    max_stage_file_bytes: u64,
    auto_commit_exclude_patterns: &[String],
) -> bool {
    if is_excluded_change_path(&entry.path, excluded_dir_names) {
        return false;
    }

    if is_excluded_file(&entry.path, excluded_file_patterns) {
        return false;
    }

    // Per-repo `auto_commit_exclude_patterns` lets the operator
    // opt out of auto-commit for specific TRACKED file patterns
    // (e.g. `web/test-results/*.png`). The matching is identical
    // to `untracked_exclude_patterns` (basename + glob); it
    // doesn't matter to the matcher whether the file is tracked
    // or untracked. The key difference is this list applies to
    // MODIFICATIONS too, not just newly-added files.
    if !auto_commit_exclude_patterns.is_empty()
        && matches_untracked_exclude(repo, &entry.path, auto_commit_exclude_patterns)
    {
        if debug_enabled() {
            eprintln!(
                "⏭️  {} skipping tracked {} (auto_commit_exclude_patterns)",
                repo.display(),
                entry.path.display()
            );
        }
        return false;
    }

    let full_path = repo.join(&entry.path);

    // Submodules and directory type changes
    if matches!(entry.status, dracon_git::types::FileStatus::TypeChange) {
        return true;
    }

    match std::fs::metadata(&full_path) {
        Ok(meta) if meta.is_file() => {
            if meta.len() > max_stage_file_bytes {
                eprintln!(
                    "ℹ️ skip large file {} ({} bytes > {} bytes)",
                    full_path.display(),
                    meta.len(),
                    max_stage_file_bytes
                );
                return false;
            }
            true
        }
        Ok(meta) if meta.is_dir() => {
            // Skip gitlink entries with unchanged pointers (dirty submodule
            // working trees that don't represent a pointer change)
            if is_gitlink_unchanged(repo, &entry.path) {
                return false;
            }
            true
        }
        Ok(_) => true,
        Err(_) => {
            // File doesn't exist on disk
            if matches!(entry.status, dracon_git::types::FileStatus::Deleted) {
                // Deleted files should be staged - they don't exist on disk by definition
                true
            } else {
                // File doesn't exist and isn't a deletion - don't stage
                // This handles partial checkouts or files that were never there
                false
            }
        }
    }
}

pub(crate) fn can_restore_entry(_repo: &Path, entry: &dracon_git::types::DiffFile) -> bool {
    use dracon_git::types::FileStatus;
    matches!(
        entry.status,
        FileStatus::Modified | FileStatus::TypeChange | FileStatus::Renamed
    )
}

pub(crate) fn is_large_untracked(
    entry: &dracon_git::types::DiffFile,
    repo: &Path,
    threshold: u64,
) -> bool {
    use dracon_git::types::FileStatus;
    if entry.status != FileStatus::Added {
        return false;
    }
    let full_path = repo.join(&entry.path);
    match std::fs::metadata(&full_path) {
        Ok(meta) if meta.is_file() => meta.len() > threshold,
        _ => false,
    }
}

pub(crate) fn append_to_gitignore(repo: &Path, patterns: &[String]) -> Result<()> {
    let gitignore = repo.join(".gitignore");
    let current = std::fs::read_to_string(&gitignore).unwrap_or_default();

    let mut lines: Vec<String> = current.lines().map(String::from).collect();
    let mut added = Vec::new();

    for pattern in patterns {
        let pattern_line = pattern.trim();
        if pattern_line.is_empty() || lines.iter().any(|l| l.trim() == pattern_line) {
            continue;
        }
        added.push(pattern_line.to_string());
    }

    if added.is_empty() {
        return Ok(());
    }

    // Check if there's a warden-managed block
    let block_begin_idx = lines
        .iter()
        .position(|l| l.contains("--- BEGIN DRACON MANAGED BLOCK ---"));
    let block_end_idx = lines
        .iter()
        .position(|l| l.contains("--- END DRACON MANAGED BLOCK ---"));

    if let (Some(begin_idx), Some(end_idx)) = (block_begin_idx, block_end_idx) {
        // Warden manages this .gitignore - insert patterns INSIDE the managed block
        // (before the END marker) so warden will preserve them
        let insert_at = end_idx;

        // Check if we already have a large files section inside the managed block
        let has_large_files_section = lines[begin_idx..end_idx]
            .iter()
            .any(|l| l.contains("# Large files (auto-added by dracon-sync)"));

        let mut to_insert = Vec::new();
        if !has_large_files_section {
            to_insert.push("# Large files (auto-added by dracon-sync)".to_string());
        }
        for pattern in &added {
            to_insert.push(pattern.clone());
        }

        // Insert before the END marker
        for (i, line) in to_insert.into_iter().enumerate() {
            lines.insert(insert_at + i, line);
        }

        let new_content = lines.join("\n");
        std::fs::write(&gitignore, new_content)?;

        eprintln!(
            "📝 added {} large file pattern(s) to .gitignore in {} (inside warden managed block)",
            added.len(),
            repo.display()
        );

        return Ok(());
    }

    // No warden block - we can safely append
    // Check if we already have a large files section
    let has_large_files_section = lines
        .iter()
        .any(|l| l.contains("# Large files (auto-added by dracon-sync)"));

    // Build the new lines to append
    let mut to_append = Vec::new();
    if !has_large_files_section {
        to_append.push(String::new()); // blank line
        to_append.push("# Large files (auto-added by dracon-sync)".to_string());
    }
    for pattern in added {
        to_append.push(pattern);
    }

    // Append to the end
    lines.extend(to_append);

    let new_content = lines.join("\n");
    std::fs::write(&gitignore, new_content)?;

    Ok(())
}

/// Handle large untracked files by adding them to .gitignore.
/// Returns true if .gitignore was updated.
pub(crate) fn handle_large_untracked(
    repo: &Path,
    to_restore: &[dracon_git::types::DiffFile],
    policy: &SyncPolicy,
) -> Result<bool> {
    let large_untracked: Vec<_> = to_restore
        .iter()
        .filter(|e| is_large_untracked(e, repo, policy.max_stage_file_bytes))
        .collect();

    if large_untracked.is_empty() {
        return Ok(false);
    }

    let patterns: Vec<String> = large_untracked
        .iter()
        .map(|e| e.path.to_string_lossy().to_string())
        .collect();
    eprintln!(
        "📝 {} has {} large untracked file(s) > {} bytes - adding to .gitignore",
        repo.display(),
        patterns.len(),
        policy.max_stage_file_bytes
    );
    append_to_gitignore(repo, &patterns)?;
    Ok(true)
}

/// Common build / generated output directory names that should never be tracked.
/// These are checked IN ADDITION to exclude_dir_names to catch directories
/// that aren't in the standard exclusion list but are clearly build artifacts.
fn is_build_output_dir_name(name: &str) -> bool {
    matches!(
        name,
        ".output" | ".out" | "output" | "generated" | "gen" | ".next" | "dist-new"
    ) || name.ends_with(".output")
        || name.ends_with("_output")
        || name.starts_with("output-")
}

/// Find tracked files that live inside excluded directories or common build
/// output directories. These should never have been committed (build artifacts,
/// generated files, etc.). Removes them from git tracking and adds the directory
/// patterns to .gitignore.
pub(crate) fn remove_tracked_excluded_paths(
    repo: &Path,
    excluded_dir_names: &BTreeSet<String>,
) -> Result<Option<Vec<String>>> {
    let output = crate::git::git_cmd()
        .current_dir(repo)
        .args(["ls-files", "-z"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let mut top_level_excluded: BTreeSet<String> = BTreeSet::new();
    let mut to_remove: Vec<String> = Vec::new();

    for file in &files {
        let path = Path::new(file);
        let mut found_excluded = false;

        // Check standard excluded dir names
        if is_excluded_change_path(path, excluded_dir_names) {
            for component in path.components() {
                let name = component.as_os_str().to_str().unwrap_or("");
                if is_excluded_dir_name(name, excluded_dir_names) {
                    top_level_excluded.insert(name.to_string());
                    found_excluded = true;
                    break;
                }
            }
        }

        // Also detect common build output directories
        if !found_excluded {
            for component in path.components() {
                let name = component.as_os_str().to_str().unwrap_or("");
                if is_build_output_dir_name(name) {
                    top_level_excluded.insert(name.to_string());
                    found_excluded = true;
                    break;
                }
            }
        }

        if found_excluded {
            to_remove.push(file.to_string());
        }
    }

    if to_remove.is_empty() {
        return Ok(None);
    }

    let patterns: Vec<String> = top_level_excluded
        .iter()
        .map(|d| format!("{}/", d))
        .collect();
    eprintln!(
        "📝 {} has {} tracked file(s) inside build-artifact dirs {:?} — removing from git and adding to .gitignore",
        repo.display(),
        to_remove.len(),
        patterns
    );

    append_to_gitignore(repo, &patterns)?;

    for chunk in to_remove.chunks(50) {
        let mut args = vec!["rm", "-q", "--cached", "--"];
        for f in chunk {
            args.push(f);
        }
        let status = crate::git::git_cmd()
            .current_dir(repo)
            .args(&args)
            .status()?;
        if !status.success() {
            eprintln!(
                "⚠️ git rm --cached failed for some files in {}",
                repo.display()
            );
        }
    }

    Ok(Some(top_level_excluded.into_iter().collect()))
}

pub(crate) fn has_sync_relevant_dirty_entries(
    repo: &Path,
    entries: &[dracon_git::types::DiffFile],
    excluded_dir_names: &BTreeSet<String>,
    excluded_file_patterns: &[String],
    max_stage_file_bytes: u64,
    auto_commit_exclude_patterns: &[String],
) -> bool {
    entries.iter().any(|entry| {
        let full_path = repo.join(&entry.path);

        // Skip gitlink entries with unchanged pointers entirely
        // Use repo.join() because entry.path is relative to repo, not CWD
        if full_path.is_dir() && is_gitlink_unchanged(repo, &entry.path) {
            return false;
        }
        should_stage_entry(
            repo,
            entry,
            excluded_dir_names,
            excluded_file_patterns,
            max_stage_file_bytes,
            auto_commit_exclude_patterns,
        ) || can_restore_entry(repo, entry)
            || is_large_untracked(entry, repo, max_stage_file_bytes)
    })
}
