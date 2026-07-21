//! Branch operations — current branch, main/master management, upstream tracking.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::{has_origin_remote, has_tracking_upstream, is_safe_branch_name};

/// Get the current branch name from HEAD ref or git CLI.
pub(crate) fn current_branch(repo: &Path) -> Option<String> {
    // CHANGED 2026-07-02 (goal `354fe3cb`):
    // Worktree-style checkouts have `.git` as a FILE pointing at
    // `<shared_gitdir>/worktrees/<X>`, so the HEAD ref lives at
    // `<shared_gitdir>/worktrees/<X>/HEAD`, not at `<repo>/.git/HEAD`.
    // The previous implementation only checked `<repo>/.git/HEAD`,
    // which doesn't exist for worktree-style checkouts. That caused
    // the function to fall through to `git rev-parse --abbrev-ref
    // HEAD`, which returns the literal string "HEAD" for detached
    // worktrees (instead of the expected `None`).
    //
    // Resolve the real gitdir first, then read its HEAD file.
    let head_path = resolve_head_path(repo);
    if let Some(head_path) = head_path {
        if let Ok(content) = std::fs::read_to_string(&head_path) {
            let trimmed = content.trim();
            if let Some(ref_name) = trimmed.strip_prefix("ref: refs/heads/") {
                return Some(ref_name.to_string());
            }
        }
    }
    crate::policy::std_git_command()
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty() && s != "HEAD")
}

/// Resolve the path to the HEAD file for a repo. Handles both
/// regular checkouts (where `<repo>/.git` is a directory) and
/// worktree-style checkouts (where `<repo>/.git` is a file
/// pointing at `<shared_gitdir>/worktrees/<X>`).
///
/// ADDED 2026-07-02 (goal `354fe3cb`).
fn resolve_head_path(repo: &Path) -> Option<std::path::PathBuf> {
    let dot_git = repo.join(".git");
    if dot_git.is_file() {
        // Worktree-style: read gitdir: line, canonicalize.
        let Ok(content) = std::fs::read_to_string(&dot_git) else {
            return None;
        };
        let rest = content.trim().strip_prefix("gitdir:")?;
        let gitdir_rel = rest.trim();
        let base_canon =
            std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
        let resolved = base_canon.join(gitdir_rel);
        let Ok(canon_resolved) = std::fs::canonicalize(&resolved) else {
            return None;
        };
        Some(canon_resolved.join("HEAD"))
    } else if dot_git.is_dir() {
        std::fs::canonicalize(&dot_git).ok().map(|p| p.join("HEAD"))
    } else {
        None
    }
}

/// Whether the repo has a master branch but NOT a main branch.
pub(crate) fn has_only_master_branch(repo: &Path) -> bool {
    use crate::policy::std_git_command;
    let has_master = std_git_command()
        .args(["rev-parse", "--verify", "refs/heads/master"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !has_master {
        return false;
    }
    let has_main = std_git_command()
        .args(["rev-parse", "--verify", "refs/heads/main"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    has_master && !has_main
}

/// Whether the repo has BOTH main and master branches.
pub(crate) fn has_both_main_and_master(repo: &Path) -> bool {
    use crate::policy::std_git_command;
    let config_path = repo.join(".git").join("config");
    let has_local_branches = if let Ok(config) = std::fs::read_to_string(&config_path) {
        config.lines().any(|l| l.trim() == "[branch \"main\"]")
            && config.lines().any(|l| l.trim() == "[branch \"master\"]")
    } else {
        false
    };
    if has_local_branches {
        return true;
    }
    let has_main = std_git_command()
        .args(["rev-parse", "--verify", "refs/heads/main"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let has_master = std_git_command()
        .args(["rev-parse", "--verify", "refs/heads/master"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    has_main && has_master
}

/// Consolidate to main: checkout main, delete local+remote master, push with upstream.
pub(crate) async fn consolidate_to_main(repo: &Path) -> Result<()> {
    use crate::policy::std_git_command;
    let branch = current_branch(repo).unwrap_or_else(|| "main".to_string());
    if branch != "main" {
        std_git_command()
            .args(["checkout", "main"])
            .current_dir(repo)
            .status()
            .with_context(|| format!("failed to checkout main in {}", repo.display()))?;
    }
    if let Err(e) = std_git_command()
        .args(["branch", "-D", "master"])
        .current_dir(repo)
        .status()
    {
        eprintln!("⚠️ failed to delete local master branch: {}", e);
    }
    if let Err(e) = std_git_command()
        .args(["push", "origin", "--delete", "master"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        eprintln!("⚠️ failed to delete remote master branch: {}", e);
    }
    if has_origin_remote(repo) && !has_tracking_upstream(repo) {
        if let Err(e) = super::push_with_retries(repo, 60, 3, "consolidate-to-main").await {
            eprintln!("⚠️ failed to push main with upstream: {}", e);
        }
    }
    Ok(())
}

/// Rename local master to main and update remote tracking.
pub(crate) async fn rename_master_to_main(repo: &Path) -> Result<()> {
    use crate::policy::std_git_command;
    let branch = current_branch(repo).unwrap_or_else(|| "main".to_string());
    if branch == "master" {
        std_git_command()
            .args(["branch", "-m", "master", "main"])
            .current_dir(repo)
            .status()
            .with_context(|| format!("failed to rename master to main in {}", repo.display()))?;
    }
    if has_origin_remote(repo) {
        if let Err(e) = super::push_with_retries(repo, 60, 3, "rename-master-to-main").await {
            eprintln!("⚠️ failed to push main to origin: {}", e);
        }
        if let Err(e) = std_git_command()
            .args(["push", "origin", "--delete", "master"])
            .current_dir(repo)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            eprintln!("⚠️ failed to delete remote master: {}", e);
        }
    }
    Ok(())
}

/// Delete the "other" default branch if it exists, preventing dual-branch drift.
/// If current branch is master → delete main. If current is main → delete master.
pub(crate) async fn prune_other_default_branch(repo: &Path) {
    use crate::policy::std_git_command;
    let branch = current_branch(repo);
    let other = match branch.as_deref() {
        Some("master") => "main",
        Some("main") => "master",
        _ => return,
    };
    let other_str = other.to_string();
    let repo_has_origin = has_origin_remote(repo);
    let repo_b = repo.to_path_buf();
    let repo_c = repo_b.clone();
    let other_b = other_str.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || {
        std_git_command()
            .args(["branch", "-D", &other_b])
            .current_dir(&repo_b)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    })
    .await
    {
        eprintln!("⚠️ failed to delete local {} branch: {}", other_str, e);
    }
    if repo_has_origin {
        let other_c = other_str.clone();
        if let Err(e) = tokio::task::spawn_blocking(move || {
            std_git_command()
                .args(["push", "origin", "--delete", &other_c])
                .current_dir(&repo_c)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
        })
        .await
        {
            eprintln!("⚠️ failed to delete remote {} branch: {}", other_str, e);
        }
    }
}

/// Check if a branch exists on the remote origin.
pub(crate) fn remote_branch_exists(repo: &Path, branch: &str) -> bool {
    use crate::policy::std_git_command;
    if !is_safe_branch_name(branch) {
        eprintln!("⚠️ branch name '{}' is unsafe, returning false", branch);
        return false;
    }
    std_git_command()
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/remotes/origin/{branch}"))
        .current_dir(repo)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Set the upstream tracking branch for a local branch on a named remote.
pub(crate) fn set_upstream_to_remote_branch(repo: &Path, remote: &str, branch: &str) -> Result<()> {
    use crate::policy::std_git_command;
    if !is_safe_branch_name(branch) {
        return Err(anyhow::anyhow!("branch name '{}' is unsafe", branch));
    }
    let target = format!("{remote}/{branch}");
    let status = std_git_command()
        .args(["branch", "--set-upstream-to"])
        .arg(&target)
        .arg(branch)
        .current_dir(repo)
        .status()
        .with_context(|| format!("failed to set upstream for {}", repo.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "set-upstream failed for {} -> {}",
            repo.display(),
            target
        ))
    }
}

/// Set the upstream tracking branch for a local branch on origin.
pub(crate) fn set_upstream_to_branch(repo: &Path, branch: &str) -> Result<()> {
    set_upstream_to_remote_branch(repo, "origin", branch)
}

fn old_tracking_from_status_line(line: &str) -> Option<String> {
    let start = line.find('[')?;
    let end = line[start..].find(']')? + start;
    let inside = line[start + 1..end].trim();
    let tracking = inside.split(':').next()?.trim();
    if tracking.is_empty() {
        None
    } else {
        Some(tracking.to_string())
    }
}

/// Detect and repair broken upstream tracking references (e.g. `origin/master: gone`).
/// Returns the number of repos repaired.
pub(crate) fn repair_broken_tracking(repos: &[PathBuf]) -> usize {
    let mut repaired = 0;
    for repo in repos {
        let output = match crate::git::git_cmd()
            .args(["branch", "-vv"])
            .current_dir(repo)
            .output()
        {
            Ok(o) => o,
            Err(_) => continue,
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            // Match lines like: `* main abc1234 [origin/master: gone] ...`
            let trimmed = line.trim();
            if !trimmed.starts_with('*') && !trimmed.starts_with(' ') {
                continue;
            }
            if !trimmed.contains(": gone]") {
                continue;
            }
            // Extract branch name. For the CHECKED-OUT branch the
            // `git branch -vv` line starts with `*` (`* main abc1234
            // [origin/main: gone]`), so the first whitespace token is
            // `*`, not the branch name — take the SECOND token in
            // that case. FIXED 2026-07-21 (v0.112.33, audit
            // M11/F2.1): the previous code took the first token and
            // `trim_start_matches('*')`, yielding `""` for the
            // current branch, which the `is_empty()` guard then
            // skipped — the one branch that matters (the one the
            // daemon pushes) was NEVER repaired.
            let mut tokens = trimmed.split_whitespace();
            let first = tokens.next().unwrap_or("");
            let branch = if first == "*" {
                tokens.next().unwrap_or("").to_string()
            } else {
                first.to_string()
            };
            if branch.is_empty() || !is_safe_branch_name(&branch) {
                continue;
            }
            // Extract the old remote tracking ref inside the `[...]` so the
            // log message shows the actual change, not a fake branch/branch.
            // Old line: `* main abc [origin/master: gone] ...` → old="origin/master"
            // Default to "origin/<branch>" if we can't parse for any reason.
            let old_tracking = old_tracking_from_status_line(trimmed)
                .unwrap_or_else(|| format!("origin/{branch}"));
            if set_upstream_to_branch(repo, &branch).is_ok() {
                eprintln!(
                    "🧹 startup: fixed broken tracking in {} ({} -> origin/{})",
                    repo.display(),
                    old_tracking,
                    branch
                );
                repaired += 1;
            }
        }
    }
    repaired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_old_tracking_from_status_line_parses_real_ref() {
        assert_eq!(
            old_tracking_from_status_line("* main abc123 [origin/master: gone] behind 1"),
            Some("origin/master".to_string())
        );
    }

    #[test]
    fn test_old_tracking_from_status_line_handles_missing_marker() {
        assert_eq!(
            old_tracking_from_status_line("* main abc123 [gone] behind 1"),
            Some("gone".to_string())
        );
    }

    #[test]
    fn test_old_tracking_from_status_line_rejects_empty_ref() {
        assert_eq!(
            old_tracking_from_status_line("* main abc123 [: gone] behind 1"),
            None
        );
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M11/F2.1): the checked-out
    /// branch must be repaired too — the pre-fix parser took the
    //  first whitespace token of the `git branch -vv` line, which is
    //  `*` for the current branch, so `trim_start_matches('*')`
    //  yielded "" and the is_empty guard skipped it. Only
    //  non-checked-out branches were ever repaired.
    #[test]
    fn test_repair_broken_tracking_repairs_checked_out_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        for (k, v) in [("user.email", "t@t"), ("user.name", "t")] {
            crate::git::git_cmd()
                .args(["config", k, v])
                .current_dir(&repo)
                .status()
                .unwrap();
        }
        std::fs::write(repo.join("a.txt"), "x\n").unwrap();
        crate::git::git_cmd()
            .args(["add", "a.txt"])
            .current_dir(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["commit", "--no-verify", "-q", "-m", "init"])
            .current_dir(&repo)
            .status()
            .unwrap();
        // Break the upstream: point branch.main.merge at a ref that
        // doesn't exist (origin/master) → `git branch -vv` reports
        // `[origin/master: gone]`.
        for (k, v) in [
            ("branch.main.remote", "origin"),
            ("branch.main.merge", "refs/heads/master"),
        ] {
            crate::git::git_cmd()
                .args(["config", k, v])
                .current_dir(&repo)
                .status()
                .unwrap();
        }
        // Sanity: the repo really is in the `: gone` state.
        let vv = crate::git::git_cmd()
            .args(["branch", "-vv"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let vv_text = String::from_utf8_lossy(&vv.stdout);
        assert!(
            vv_text.contains(": gone]"),
            "test setup must produce a gone upstream, got: {}",
            vv_text
        );

        let repaired = repair_broken_tracking(&[repo.clone()]);
        assert_eq!(
            repaired, 1,
            "the checked-out branch must be repaired (regression M11/F2.1)"
        );
        // After repair, the upstream points at origin/main.
        let merge = crate::git::git_cmd()
            .args(["config", "--get", "branch.main.merge"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&merge.stdout).trim(),
            "refs/heads/main"
        );
    }

    /// When a worktree is detached at a SHA, `git rev-parse
    /// --abbrev-ref HEAD` returns the literal string "HEAD". The
    /// previous `current_branch` implementation accepted this as
    /// a valid branch name, causing downstream push code to build
    /// refspecs like `HEAD:refs/heads/HEAD` (rejected by remotes).
    /// This test guards the regression by checking the filter logic.
    ///
    /// ADDED 2026-07-02 (goal `354fe3cb`).
    #[test]
    fn test_current_branch_rejects_git_cli_head_string() {
        // Filter from current_branch: filter(|s| !s.is_empty() && s != "HEAD")
        let detached: Option<String> = Some("HEAD".to_string());
        let filtered = detached.filter(|s| !s.is_empty() && s != "HEAD");
        assert_eq!(filtered, None);

        let on_main: Option<String> = Some("main".to_string());
        let filtered = on_main.filter(|s| !s.is_empty() && s != "HEAD");
        assert_eq!(filtered, Some("main".to_string()));
    }
}
