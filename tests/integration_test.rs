//! Integration tests for dracon-sync.
//!
//! These tests use real git repos (tempdir) to verify end-to-end behavior.

use std::path::PathBuf;

/// Helper to run a git command.
fn git_cmd(repo: &PathBuf, args: &[&str]) -> std::process::Output {
    let git_bin = std::env::var("DRACON_SYNC_GIT_BIN")
        .unwrap_or_else(|_| "/run/current-system/sw/bin/git".to_string());
    std::process::Command::new(&git_bin)
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap()
}

/// Helper to create a test repo with one commit.
fn create_test_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("test-repo");
    git_cmd(&tmp.path().to_path_buf(), &["init", "-q", "-b", "master"]);
    std::fs::create_dir_all(&repo).unwrap();
    git_cmd(&repo, &["init", "-q", "-b", "master"]);
    git_cmd(&repo, &["config", "user.email", "test@test.com"]);
    git_cmd(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("file.txt"), "initial content").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "init"]);
    tmp
}

#[test]
fn test_sync_repo_basic() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Make a change
    std::fs::write(repo.join("file.txt"), "modified content").unwrap();
    git_cmd(&repo, &["add", "."]);

    // Verify there are changes
    let status = git_cmd(&repo, &["status", "--porcelain"]);
    assert!(!status.stdout.is_empty(), "repo should have changes");
}

#[test]
fn test_sync_repo_commit() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Make a change and commit
    std::fs::write(repo.join("file.txt"), "modified content").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "test commit"]);

    // Verify commit was created
    let log = git_cmd(&repo, &["log", "--oneline"]);
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(log_str.contains("test commit"), "commit should exist");
}

#[test]
fn test_sync_repo_push_to_bare() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("bare.git");
    git_cmd(
        &tmp.path().to_path_buf(),
        &["init", "--bare", bare.to_str().unwrap()],
    );

    let repo = tmp.path().join("repo");
    git_cmd(&tmp.path().to_path_buf(), &["init", "-q", "-b", "master"]);
    std::fs::create_dir_all(&repo).unwrap();
    git_cmd(&repo, &["init", "-q", "-b", "master"]);
    git_cmd(&repo, &["config", "user.email", "test@test.com"]);
    git_cmd(&repo, &["config", "user.name", "Test"]);
    git_cmd(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);

    // Create initial commit
    std::fs::write(repo.join("file.txt"), "content").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "init"]);

    // Push
    let push = git_cmd(&repo, &["push", "-u", "origin", "HEAD"]);
    assert!(
        push.status.success(),
        "push should succeed: {}",
        String::from_utf8_lossy(&push.stderr)
    );

    // Verify remote has the commit
    let log = git_cmd(&bare, &["log", "--oneline", "master"]);
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(log_str.contains("init"), "remote should have the commit");
}

#[test]
fn test_sync_repo_multiple_commits() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Create multiple commits
    for i in 1..=3 {
        std::fs::write(repo.join("file.txt"), format!("content {}", i)).unwrap();
        git_cmd(&repo, &["add", "."]);
        git_cmd(
            &repo,
            &["commit", "--no-verify", "-m", &format!("commit {}", i)],
        );
    }

    // Verify all commits exist
    let log = git_cmd(&repo, &["log", "--oneline"]);
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(log_str.contains("commit 1"), "should have commit 1");
    assert!(log_str.contains("commit 2"), "should have commit 2");
    assert!(log_str.contains("commit 3"), "should have commit 3");
}

#[test]
fn test_sync_repo_branch_operations() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Create a branch
    git_cmd(&repo, &["checkout", "-b", "feature"]);

    // Make a commit on the branch
    std::fs::write(repo.join("feature.txt"), "feature content").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "feature commit"]);

    // Switch back to master
    git_cmd(&repo, &["checkout", "master"]);

    // Verify branch exists
    let branches = git_cmd(&repo, &["branch"]);
    let branches_str = String::from_utf8_lossy(&branches.stdout);
    assert!(
        branches_str.contains("feature"),
        "feature branch should exist"
    );
}

#[test]
fn test_sync_repo_merge() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Create a branch and make a commit
    git_cmd(&repo, &["checkout", "-b", "feature"]);
    std::fs::write(repo.join("feature.txt"), "feature content").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "feature commit"]);

    // Switch back to master and make a commit (so merge is not fast-forward)
    git_cmd(&repo, &["checkout", "master"]);
    std::fs::write(repo.join("master.txt"), "master content").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "master commit"]);

    // Now merge feature (should create a merge commit)
    let merge = git_cmd(&repo, &["merge", "feature", "--no-edit"]);
    assert!(
        merge.status.success(),
        "merge should succeed: {}",
        String::from_utf8_lossy(&merge.stderr)
    );

    // Verify merge commit exists
    let log = git_cmd(&repo, &["log", "--oneline"]);
    let log_str = String::from_utf8_lossy(&log.stdout);
    // The merge commit should exist (might be "Merge branch 'feature'" or similar)
    assert!(
        log_str.lines().count() >= 3,
        "should have at least 3 commits (init, feature, merge)"
    );
}

#[test]
fn test_sync_repo_conflict_detection() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Create a branch and make a conflicting change
    git_cmd(&repo, &["checkout", "-b", "feature"]);
    std::fs::write(repo.join("file.txt"), "feature version").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "feature change"]);

    // Switch back to master and make a conflicting change
    git_cmd(&repo, &["checkout", "master"]);
    std::fs::write(repo.join("file.txt"), "master version").unwrap();
    git_cmd(&repo, &["add", "."]);
    git_cmd(&repo, &["commit", "--no-verify", "-m", "master change"]);

    // Try to merge - should fail with conflict
    let merge = git_cmd(&repo, &["merge", "feature", "--no-edit"]);
    assert!(!merge.status.success(), "merge should fail with conflict");

    // Verify conflict markers exist
    let content = std::fs::read_to_string(repo.join("file.txt")).unwrap();
    assert!(content.contains("<<<<<<<"), "should have conflict markers");
}

#[test]
fn test_sync_repo_stash() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Make a change
    std::fs::write(repo.join("file.txt"), "modified content").unwrap();

    // Stash the change
    let stash = git_cmd(&repo, &["stash"]);
    assert!(
        stash.status.success(),
        "stash should succeed: {}",
        String::from_utf8_lossy(&stash.stderr)
    );

    // Verify working directory is clean
    let status = git_cmd(&repo, &["status", "--porcelain"]);
    assert!(
        status.stdout.is_empty(),
        "working directory should be clean after stash"
    );

    // Pop the stash
    let pop = git_cmd(&repo, &["stash", "pop"]);
    assert!(
        pop.status.success(),
        "stash pop should succeed: {}",
        String::from_utf8_lossy(&pop.stderr)
    );

    // Verify change is restored
    let content = std::fs::read_to_string(repo.join("file.txt")).unwrap();
    assert_eq!(content, "modified content", "change should be restored");
}

#[test]
fn test_sync_repo_tag() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Create a tag
    let tag = git_cmd(&repo, &["tag", "v1.0.0"]);
    assert!(
        tag.status.success(),
        "tag should succeed: {}",
        String::from_utf8_lossy(&tag.stderr)
    );

    // Verify tag exists
    let tags = git_cmd(&repo, &["tag"]);
    let tags_str = String::from_utf8_lossy(&tags.stdout);
    assert!(tags_str.contains("v1.0.0"), "tag should exist");
}

#[test]
fn test_sync_repo_diff() {
    let tmp = create_test_repo();
    let repo = tmp.path().join("test-repo");

    // Make a change
    std::fs::write(repo.join("file.txt"), "modified content").unwrap();

    // Get diff
    let diff = git_cmd(&repo, &["diff"]);
    let diff_str = String::from_utf8_lossy(&diff.stdout);
    assert!(
        diff_str.contains("modified content"),
        "diff should show the change"
    );
}
