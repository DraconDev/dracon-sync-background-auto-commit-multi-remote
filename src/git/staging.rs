//! File staging and path management — unstage, restore, blob detection.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

/// Unstage paths that match excluded directory patterns.
/// Returns the count of unstaged files.
pub(crate) async fn unstage_excluded_paths(
    repo: &Path,
    excluded_dir_names: &BTreeSet<String>,
) -> Result<usize> {
    let staged = super::staged_paths(repo).await?;
    let mut to_unstage = Vec::new();
    for path in staged {
        if !super::is_safe_git_path(&path) {
            eprintln!(
                "⚠️ skipping unsafe path {} in {}",
                path.display(),
                repo.display()
            );
            continue;
        }
        if is_excluded_change_path(&path, excluded_dir_names) {
            to_unstage.push(path);
        }
    }
    if to_unstage.is_empty() {
        return Ok(0);
    }
    for chunk in to_unstage.chunks(50) {
        let mut cmd = crate::policy::tokio_git_command();
        cmd.args(["reset", "-q", "HEAD", "--"])
            .current_dir(repo)
            .kill_on_drop(true);
        for path in chunk {
            cmd.arg(path);
        }
        // CHANGED 2026-07-21 (v0.112.33, audit M13/F2.4): require
        // exit 0 — the previous `.status().await?` ignored non-zero
        // exits (index.lock contention, pathspec errors) and the
        // caller's count claimed the paths were unstaged anyway.
        let status = cmd.status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "git reset HEAD -- ({} paths) failed in {}: exit {}",
                chunk.len(),
                repo.display(),
                status
            ));
        }
    }
    Ok(to_unstage.len())
}

/// Unstage files that exceed the max file size threshold.
/// Returns the count of unstaged files.
pub(crate) async fn unstage_oversized_paths(repo: &Path, max_bytes: u64) -> Result<usize> {
    let staged = super::staged_paths(repo).await?;
    let mut to_unstage = Vec::new();
    for path in staged {
        if !super::is_safe_git_path(&path) {
            eprintln!(
                "⚠️ skipping unsafe path {} in {}",
                path.display(),
                repo.display()
            );
            continue;
        }
        let full = repo.join(&path);
        if let Ok(meta) = tokio::fs::metadata(&full).await {
            if meta.len() > max_bytes {
                to_unstage.push(path);
            }
        }
    }
    if to_unstage.is_empty() {
        return Ok(0);
    }
    for chunk in to_unstage.chunks(50) {
        let mut cmd = crate::policy::tokio_git_command();
        cmd.args(["reset", "-q", "HEAD", "--"])
            .current_dir(repo)
            .kill_on_drop(true);
        for path in chunk {
            cmd.arg(path);
        }
        // CHANGED 2026-07-21 (v0.112.33, audit M13/F2.4): require
        // exit 0 (same rationale as `unstage_excluded_paths`).
        let status = cmd.status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "git reset HEAD -- ({} oversized paths) failed in {}: exit {}",
                chunk.len(),
                repo.display(),
                status
            ));
        }
    }
    Ok(to_unstage.len())
}

/// Detect large blobs ahead of the current position.
pub(crate) async fn detect_large_blobs_ahead(
    repo: &Path,
    min_bytes: u64,
) -> Result<Vec<(u64, String)>> {
    let r = repo.to_path_buf();
    let display = r.display().to_string();
    tokio::time::timeout(
        Duration::from_secs(60),
        tokio::task::spawn_blocking(move || -> Result<Vec<(u64, String)>> {
            let rev_list = crate::policy::std_git_command()
                .args(["rev-list", "--objects", "@{u}..HEAD"])
                .current_dir(&r)
                .output()
                .with_context(|| format!("failed rev-list in {}", r.display()))?;
            if !rev_list.status.success() {
                return Ok(Vec::new());
            }
            let mut cat_file = crate::policy::std_git_command()
                .args([
                    "cat-file",
                    "--batch-check=%(objectname) %(objecttype) %(objectsize) %(rest)",
                ])
                .current_dir(&r)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .with_context(|| format!("failed cat-file in {}", r.display()))?;
            if let Some(mut stdin) = cat_file.stdin.take() {
                use std::io::Write;
                stdin.write_all(&rev_list.stdout)?;
            }
            let output = cat_file.wait_with_output()?;
            if !output.status.success() {
                return Ok(Vec::new());
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut out: Vec<(u64, String)> = stdout
                .lines()
                .filter_map(|line| {
                    let mut parts = line.split_whitespace();
                    let _oid = parts.next()?;
                    let obj_type = parts.next()?;
                    let size_str = parts.next()?;
                    let path = parts.next()?;
                    if obj_type == "blob" {
                        let size = size_str.parse::<u64>().ok()?;
                        if size > min_bytes {
                            return Some((size, path.to_string()));
                        }
                    }
                    None
                })
                .collect();
            out.sort_by_key(|a| a.0);
            Ok(out)
        }),
    )
    .await
    .with_context(|| format!("timed out in detect_large_blobs_ahead for {}", display))?
    .with_context(|| format!("detect_large_blobs_ahead timed out (>60s) for {}", display))?
}

/// Get the top-level directory name from a path.
pub(crate) fn top_level_dir(path: &str) -> Option<String> {
    path.split('/').next().map(|s| s.to_string())
}

/// Rewrite ahead paths using git filter-repo or filter-branch.
/// Returns Some(backup_branch_name) on success, None if no paths to rewrite.
///
/// F31 (2026-07-19): after a successful rewrite, check whether the
/// resulting HEAD actually differs from the backup branch. If the
/// rewrite was a no-op (e.g. the path glob didn't match anything
/// committed ahead of the remote), delete the backup branch to
/// avoid littering `git branch` output with empty `backup/pre-sync-*`
/// branches. The function signature is preserved: callers see
/// `Some(backup)` only when the rewrite actually changed history.
pub(crate) fn rewrite_ahead_paths(
    repo: &Path,
    paths_to_remove: &[String],
    backup_prefix: &str,
) -> Result<Option<String>> {
    if paths_to_remove.is_empty() {
        return Ok(None);
    }

    // ADDED 2026-07-23 (v0.112.39, prevention #56): object-
    // completeness pre-flight. A history rewrite (filter-repo /
    // filter-branch) must not run on a damaged gitdir — if objects
    // referenced by main's history are MISSING from the object
    // store, the rewrite would produce (or preserve) history
    // referencing objects that don't exist anywhere. NOTE: this is
    // a cheap guard for a hypothetical class — the deathrun
    // investigation (2026-07-23) initially suspected the auto-repair
    // had broken history, but the corrected probe showed 0 missing
    // objects (a probe artifact). The guard is kept as cheap
    // insurance: if a genuinely damaged gitdir ever appears, we
    // refuse to rewrite it and alert instead of making it worse.
    let missing = crate::report::probe_missing_objects(repo);
    if missing > 0 {
        return Err(anyhow::anyhow!(
            "refusing history rewrite in {}: {} objects referenced by main's history are missing from the object store (damaged gitdir) — restore from the forge or orphan-cutover first (backup not created)",
            repo.display(),
            missing
        ));
    }

    let backup_branch = format!("{backup_prefix}-{}", crate::policy::timestamp_secs());
    let create_backup = crate::policy::std_git_command()
        .args(["branch", &backup_branch])
        .current_dir(repo)
        .status()
        .with_context(|| format!("failed backup branch in {}", repo.display()))?;
    if !create_backup.success() {
        return Err(anyhow::anyhow!(
            "failed to create backup branch {} in {}",
            backup_branch,
            repo.display()
        ));
    }

    // Try git-filter-repo first (preferred, faster, actively maintained)
    let filter_repo_available = crate::policy::std_git_command()
        .args(["filter-repo", "--version"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if filter_repo_available {
        let mut args: Vec<String> = vec![
            "filter-repo".to_string(),
            "--invert-paths".to_string(),
            "--force".to_string(),
        ];
        for path in paths_to_remove {
            args.push("--path".to_string());
            args.push(path.clone());
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let rewrite = crate::policy::std_git_command()
            .args(&args_ref)
            .current_dir(repo)
            .status()
            .with_context(|| format!("failed filter-repo in {}", repo.display()))?;
        if !rewrite.success() {
            return Err(anyhow::anyhow!(
                "filter-repo failed in {} (backup: {})",
                repo.display(),
                backup_branch
            ));
        }
        return rewrite_was_noop_then_cleanup(repo, &backup_branch);
    }

    let filter_branch_available = crate::policy::std_git_command()
        .args(["filter-branch", "--version"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if filter_branch_available {
        let args = build_filter_branch_args(paths_to_remove);
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let rewrite = crate::policy::std_git_command()
            .args(&args_ref)
            .current_dir(repo)
            .status()
            .with_context(|| format!("failed filter-branch in {}", repo.display()))?;
        if !rewrite.success() {
            return Err(anyhow::anyhow!(
                "filter-branch failed in {} (backup: {})",
                repo.display(),
                backup_branch
            ));
        }
        return rewrite_was_noop_then_cleanup(repo, &backup_branch);
    }

    Err(anyhow::anyhow!(
        "Neither git-filter-repo nor git-filter-branch available in {}. Install git-filter-repo (pip install git-filter-repo) or git-filter-branch to rewrite history (backup branch: {})",
        repo.display(),
        backup_branch
    ))
}

/// Build the `git filter-branch` argv for the fallback rewrite path.
///
/// FIXED 2026-07-21 (v0.112.33, audit M12/F2.2): the previous argv
/// appended `paths_to_remove` as bare positional entries AFTER the
/// `--index-filter` string and before `--`. Two independent
/// breakages: (1) the index-filter command (`git rm -r --cached
/// --ignore-unmatch` with NO pathspec) dies with "fatal: No pathspec
/// was given" on every commit; (2) filter-branch forwards trailing
/// positionals to `git rev-list`, where `assets/big.mp4` is parsed
/// as a REVISION and dies with "bad revision". The fallback could
/// never succeed. The filter is now a single shell-quoted string
/// (paths inside the command), followed by `--` and an explicit
/// `--all` rev range (parity with the filter-repo arm, which also
/// rewrites all refs). Extracted as a pure function so the argv
/// shape is unit-testable without env shims.
fn build_filter_branch_args(paths_to_remove: &[String]) -> Vec<String> {
    let quoted: Vec<String> = paths_to_remove
        .iter()
        .map(|p| format!("'{}'", p.replace('\'', "'\\''")))
        .collect();
    let filter_expr = format!(
        "git rm -r --cached --ignore-unmatch -- {}",
        quoted.join(" ")
    );
    vec![
        "filter-branch".to_string(),
        "--force".to_string(),
        "--index-filter".to_string(),
        filter_expr,
        "--".to_string(),
        "--all".to_string(),
    ]
}

/// Compare backup_branch HEAD-tree to current HEAD. If equal, the
/// rewrite was a no-op — delete the backup branch so it doesn't
/// clutter `git branch` output. Otherwise return Some(backup_branch).
fn rewrite_was_noop_then_cleanup(repo: &Path, backup_branch: &str) -> Result<Option<String>> {
    // Use `git rev-parse <branch>^{tree}` so we compare trees, not
    // commit hashes — a no-op rewrite that touched the commit graph
    // but not the tree still produces the same content.
    let backup_tree = crate::policy::std_git_command()
        .args(["rev-parse", &format!("{}^{{tree}}", backup_branch)])
        .current_dir(repo)
        .output();
    let head_tree = crate::policy::std_git_command()
        .args(["rev-parse", "HEAD^{tree}"])
        .current_dir(repo)
        .output();
    match (backup_tree, head_tree) {
        (Ok(b), Ok(h)) if b.status.success() && h.status.success() => {
            let b_hash = String::from_utf8_lossy(&b.stdout).trim().to_string();
            let h_hash = String::from_utf8_lossy(&h.stdout).trim().to_string();
            if b_hash == h_hash {
                // No-op rewrite — delete the empty backup branch.
                let _ = crate::policy::std_git_command()
                    .args(["branch", "-D", backup_branch])
                    .current_dir(repo)
                    .status();
                return Ok(None);
            }
            Ok(Some(backup_branch.to_string()))
        }
        _ => {
            // Couldn't determine — assume rewrite happened (safer to
            // keep the backup than to silently drop it).
            Ok(Some(backup_branch.to_string()))
        }
    }
}

/// Restore paths from the index to the working tree.
pub(crate) async fn restore_paths(repo: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    // F32 (2026-07-18): each path must be a valid git path (no
    // `..`, no absolute path, no NUL) before we hand it to git. The
    // sibling `unstage_paths` function already gates on this helper;
    // restore_paths did not.
    for p in paths {
        if !super::is_safe_git_path(std::path::Path::new(p)) {
            anyhow::bail!("restore_paths: refusing unsafe path '{}'", p);
        }
    }
    let mut args = vec![
        "restore".to_string(),
        "--staged".to_string(),
        "--worktree".to_string(),
        "--".to_string(),
    ];
    args.extend(paths.iter().cloned());
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    if super::run_git_with_timeout(repo, &args_ref, 30, "restore")
        .await
        .is_ok()
    {
        return Ok(());
    }

    let mut reset: Vec<String> = Vec::new();
    reset.push("reset".to_string());
    reset.push("HEAD".to_string());
    reset.push("--".to_string());
    reset.extend(paths.iter().cloned());
    let reset_ref: Vec<&str> = reset.iter().map(|s| s.as_str()).collect();
    if let Err(e) = super::run_git_with_timeout(repo, &reset_ref, 30, "reset").await {
        eprintln!("⚠️ git reset fallback failed for {}: {}", repo.display(), e);
        return Err(anyhow::anyhow!(
            "restore failed: git restore failed and reset fallback also failed: {}",
            e
        ));
    }
    for path in paths {
        let checkout_args = ["checkout", "--", path];
        if let Err(e) = super::run_git_with_timeout(repo, &checkout_args, 30, "checkout").await {
            eprintln!(
                "⚠️ git checkout failed for {} in {}: {}",
                path,
                repo.display(),
                e
            );
        }
    }
    Ok(())
}

fn is_excluded_change_path(path: &Path, excluded_dir_names: &BTreeSet<String>) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|c| excluded_dir_names.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::create_test_repo;

    /// F31 (2026-07-19): `rewrite_ahead_paths` must delete the backup
    /// branch when the rewrite was a no-op (HEAD tree == backup tree).
    #[test]
    fn test_f31_noop_rewrite_deletes_backup_branch() {
        if !crate::git::ops::filter_repo_available_for_tests() {
            eprintln!("filter-repo not installed; skipping");
            return;
        }
        let repo = create_test_repo();
        let pre = crate::policy::std_git_command()
            .args(["rev-parse", "HEAD^{tree}"])
            .current_dir(repo.as_path())
            .output()
            .expect("rev-parse");
        let pre_hash = String::from_utf8_lossy(&pre.stdout).trim().to_string();

        // Empty paths_to_remove means rewrite_ahead_paths short-circuits to Ok(None).
        let r = rewrite_ahead_paths(repo.as_path(), &[], "test/backup");
        assert!(r.is_ok());
        assert_eq!(r.unwrap(), None);

        // Now test with a path that doesn't match anything in HEAD.
        // The commit tree won't change; backup should be deleted.
        let r2 = rewrite_ahead_paths(
            repo.as_path(),
            &["nonexistent/should/not/match.xyz".to_string()],
            "test/backup",
        );
        assert!(r2.is_ok());

        // Verify the backup branch was deleted (no-op rewrite cleanup).
        let branches = crate::policy::std_git_command()
            .args(["branch", "--list"])
            .current_dir(repo.as_path())
            .output()
            .expect("git branch");
        let stdout = String::from_utf8_lossy(&branches.stdout);
        assert!(
            !stdout.contains("test/backup-"),
            "expected no backup branches after no-op rewrite; got: {}",
            stdout
        );

        // HEAD tree unchanged.
        let post = crate::policy::std_git_command()
            .args(["rev-parse", "HEAD^{tree}"])
            .current_dir(repo.as_path())
            .output()
            .expect("rev-parse");
        let post_hash = String::from_utf8_lossy(&post.stdout).trim().to_string();
        assert_eq!(pre_hash, post_hash);
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M12/F2.2): pins the
    /// filter-branch fallback argv shape — paths must be INSIDE the
    /// single quoted `--index-filter` string (never bare positionals,
    /// which filter-branch forwards to `git rev-list` where a path
    /// like `assets/big.mp4` dies as a "bad revision"), followed by
    /// `--` and an explicit `--all` rev range.
    #[test]
    fn test_build_filter_branch_args_shape() {
        let args = build_filter_branch_args(&[
            "assets/big.mp4".to_string(),
            "docs/my file.pdf".to_string(),
        ]);
        assert_eq!(args[0], "filter-branch");
        assert_eq!(args[1], "--force");
        assert_eq!(args[2], "--index-filter");
        let filter = &args[3];
        assert!(
            filter.starts_with("git rm -r --cached --ignore-unmatch -- "),
            "index-filter must contain the pathspec inside the command: {}",
            filter
        );
        assert!(filter.contains("'assets/big.mp4'"));
        // Space-containing path is single-quoted so the shell keeps
        // it as ONE argument.
        assert!(filter.contains("'docs/my file.pdf'"));
        // No bare positional paths between the filter string and `--`.
        assert_eq!(args[4], "--");
        assert_eq!(args[5], "--all");
        assert_eq!(args.len(), 6);
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M12/F2.2): a path with an
    /// embedded single quote is escaped (`'\''`) so the shell can't
    /// break out of the quoted string.
    #[test]
    fn test_build_filter_branch_args_escapes_single_quotes() {
        let args = build_filter_branch_args(&["we'ird.bin".to_string()]);
        assert!(args[3].contains("'we'\\''ird.bin'"), "got: {}", args[3]);
    }
}
