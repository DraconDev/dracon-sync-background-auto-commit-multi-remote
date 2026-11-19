//! Superseded-upstream reconciliation (ADDED 2026-11-19).
//!
//! INCIDENT (ai-auto-writer, 2026-11-18): an agent loop committed
//! `c92fd27` at 23:39:28, the daemon published it at 23:39:44, and the loop
//! ran `git commit --amend` at 23:39:47. Local `main` moved to a sibling
//! commit with the same parent and message; 435 further commits were built
//! on it. Every push since was non-fast-forward, and the auto-pull conflicted
//! on the files the amend touched, so the repo sat in "needs operator
//! reconciliation" indefinitely.
//!
//! The remote-only commits in that shape carry no foreign work: the local
//! branch itself pointed at the upstream tip and then deliberately moved
//! off it. That fact is recorded in the branch reflog. When the upstream tip
//! is a former value of the local branch, everything upstream has was once
//! the local branch's own state, and the local branch is authoritative by
//! construction. The daemon records the superseded tip with
//! `git merge -s ours`: local content is kept byte-for-byte, the remote
//! commits stay in history (nothing is rewritten or lost), and every remote
//! that holds the superseded tip becomes fast-forwardable again.
//!
//! A tip the local branch never pointed at is genuine foreign work and is
//! never touched here; it still goes through the ordinary merge.

use std::path::Path;

/// Read-only Git query; `None` on spawn failure or non-zero exit.
fn git_stdout(repo: &Path, args: &[&str]) -> Option<String> {
    let output = super::git_cmd()
        .args(args)
        .current_dir(repo)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> bool {
    super::git_cmd()
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(repo)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Return the upstream tip of `remote/branch` when it is a commit the local
/// branch once pointed at and has since moved away from without descending
/// from it (amend, reset, rebase of a published commit). `None` for every
/// other shape, including ordinary ahead/behind and any lookup failure:
/// the caller then falls back to the normal merge, so a false negative is
/// the old behavior, never a new risk.
pub(crate) fn superseded_upstream_tip(repo: &Path, remote: &str, branch: &str) -> Option<String> {
    if !super::is_safe_branch_name(branch) {
        return None;
    }
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let local = format!("refs/heads/{branch}");
    let tip = git_stdout(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{tracking}^{{commit}}"),
        ],
    )?
    .trim()
    .to_string();
    let head = git_stdout(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{local}^{{commit}}"),
        ],
    )?
    .trim()
    .to_string();
    if !super::is_valid_object_id(&tip) || !super::is_valid_object_id(&head) || tip == head {
        return None;
    }
    // Already merged, or plainly behind: the ordinary paths handle these.
    if is_ancestor(repo, &tip, &head) || is_ancestor(repo, &head, &tip) {
        return None;
    }
    let reflog = git_stdout(repo, &["reflog", "show", "--format=%H", &local])?;
    reflog.lines().any(|line| line.trim() == tip).then_some(tip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = crate::test_helpers::test_git_cmd()
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Local repo with a bare origin; `base` published on main.
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let bare = temp.path().join("origin.git");
        let hooks = temp.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        git(
            temp.path(),
            &["init", "--quiet", "--bare", bare.to_str().unwrap()],
        );
        git(
            temp.path(),
            &[
                "init",
                "--quiet",
                "--initial-branch=main",
                repo.to_str().unwrap(),
            ],
        );
        for (key, value) in [
            ("user.name", "Superseded Test"),
            ("user.email", "superseded@example.invalid"),
            ("core.hooksPath", hooks.to_str().unwrap()),
        ] {
            git(&repo, &["config", key, value]);
        }
        std::fs::write(repo.join("book.md"), "base\n").unwrap();
        git(&repo, &["add", "book.md"]);
        git(&repo, &["commit", "--quiet", "--no-verify", "-m", "base"]);
        git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(
            &repo,
            &["push", "--quiet", "--set-upstream", "origin", "main"],
        );
        (temp, repo, bare)
    }

    fn commit(repo: &Path, content: &str, message: &str) {
        std::fs::write(repo.join("book.md"), content).unwrap();
        git(repo, &["add", "book.md"]);
        git(repo, &["commit", "--quiet", "--no-verify", "-m", message]);
    }

    #[test]
    fn amend_after_publish_is_detected_as_superseded() {
        let (_temp, repo, _bare) = fixture();
        commit(&repo, "base\nchapter\n", "chapter");
        git(&repo, &["push", "--quiet", "origin", "main"]);
        let published = git(&repo, &["rev-parse", "HEAD"]);
        std::fs::write(repo.join("book.md"), "base\nchapter, longer\n").unwrap();
        git(
            &repo,
            &[
                "commit",
                "--quiet",
                "--no-verify",
                "--amend",
                "-a",
                "--no-edit",
            ],
        );
        commit(&repo, "base\nchapter, longer\nmore\n", "more work on top");

        assert_eq!(
            superseded_upstream_tip(&repo, "origin", "main"),
            Some(published)
        );
    }

    /// The incident shape end to end: the amend and later work conflict
    /// with the published commit, so the old plain pull failed forever.
    /// The fenced pull must now succeed, keep local content exactly, and
    /// leave a fast-forward push.
    #[tokio::test]
    async fn fenced_pull_absorbs_amend_race_and_push_fast_forwards() {
        let (_temp, repo, _bare) = fixture();
        commit(&repo, "base\nchapter v1\n", "chapter");
        git(&repo, &["push", "--quiet", "origin", "main"]);
        let published = git(&repo, &["rev-parse", "HEAD"]);
        std::fs::write(repo.join("book.md"), "base\nchapter v2\n").unwrap();
        git(
            &repo,
            &[
                "commit",
                "--quiet",
                "--no-verify",
                "--amend",
                "-a",
                "--no-edit",
            ],
        );
        commit(&repo, "base\nchapter v3 final\n", "revise chapter");
        let tree_before = git(&repo, &["rev-parse", "HEAD^{tree}"]);
        // Precondition: the plain merge really conflicts.
        let merge_tree = crate::test_helpers::test_git_cmd()
            .args(["merge-tree", "--write-tree", "HEAD", "origin/main"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!merge_tree.status.success(), "fixture must conflict");

        crate::git::pull_merge_origin(&repo, 30, "test-superseded")
            .await
            .unwrap();

        assert_eq!(git(&repo, &["rev-parse", "HEAD^{tree}"]), tree_before);
        assert_eq!(
            std::fs::read_to_string(repo.join("book.md")).unwrap(),
            "base\nchapter v3 final\n"
        );
        assert!(is_ancestor(&repo, &published, "HEAD"));
        git(&repo, &["push", "--quiet", "origin", "main"]);
        assert_eq!(superseded_upstream_tip(&repo, "origin", "main"), None);
    }

    /// Superseded tip plus genuine foreign work after it: the superseded
    /// commit is absorbed and the foreign commit still merges normally.
    #[tokio::test]
    async fn foreign_commit_after_superseded_tip_still_merges() {
        let (temp, repo, bare) = fixture();
        commit(&repo, "base\nchapter v1\n", "chapter");
        git(&repo, &["push", "--quiet", "origin", "main"]);
        std::fs::write(repo.join("book.md"), "base\nchapter v2\n").unwrap();
        git(
            &repo,
            &[
                "commit",
                "--quiet",
                "--no-verify",
                "--amend",
                "-a",
                "--no-edit",
            ],
        );
        git(&repo, &["fetch", "--quiet", "origin"]);
        let other = temp.path().join("other");
        git(
            temp.path(),
            &[
                "clone",
                "--quiet",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "user.name", "Other"]);
        git(&other, &["config", "user.email", "other@example.invalid"]);
        std::fs::write(other.join("notes.md"), "foreign\n").unwrap();
        git(&other, &["add", "notes.md"]);
        git(
            &other,
            &["commit", "--quiet", "--no-verify", "-m", "foreign"],
        );
        git(&other, &["push", "--quiet", "origin", "main"]);

        crate::git::pull_merge_origin(&repo, 30, "test-superseded-foreign")
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(repo.join("book.md")).unwrap(),
            "base\nchapter v2\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("notes.md")).unwrap(),
            "foreign\n"
        );
        git(&repo, &["push", "--quiet", "origin", "main"]);
    }

    #[test]
    fn foreign_upstream_work_is_never_superseded() {
        let (temp, repo, bare) = fixture();
        let other = temp.path().join("other");
        git(
            temp.path(),
            &[
                "clone",
                "--quiet",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "user.name", "Other"]);
        git(&other, &["config", "user.email", "other@example.invalid"]);
        git(&other, &["config", "core.hooksPath", "/dev/null"]);
        commit(&other, "base\nforeign\n", "foreign");
        git(&other, &["push", "--quiet", "origin", "main"]);
        commit(&repo, "base\nlocal\n", "local");
        git(&repo, &["fetch", "--quiet", "origin"]);

        assert_eq!(superseded_upstream_tip(&repo, "origin", "main"), None);
    }

    #[test]
    fn ordinary_ahead_or_behind_is_not_superseded() {
        let (_temp, repo, _bare) = fixture();
        // In sync.
        assert_eq!(superseded_upstream_tip(&repo, "origin", "main"), None);
        // Ahead only.
        commit(&repo, "base\nahead\n", "ahead");
        assert_eq!(superseded_upstream_tip(&repo, "origin", "main"), None);
        // Behind only: publish, then step back without rewriting the remote
        // (local was at the tip, and the tip descends from local).
        git(&repo, &["push", "--quiet", "origin", "main"]);
        git(&repo, &["reset", "--quiet", "--hard", "HEAD~1"]);
        assert_eq!(superseded_upstream_tip(&repo, "origin", "main"), None);
    }

    #[test]
    fn unsafe_or_missing_branches_are_ignored() {
        let (_temp, repo, _bare) = fixture();
        assert_eq!(superseded_upstream_tip(&repo, "origin", "no-such"), None);
        assert_eq!(superseded_upstream_tip(&repo, "origin", "-bad"), None);
    }
}
