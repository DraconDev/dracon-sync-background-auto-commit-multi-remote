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
        cmd.status().await?;
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
        cmd.status().await?;
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
pub(crate) fn rewrite_ahead_paths(
    repo: &Path,
    paths_to_remove: &[String],
    backup_prefix: &str,
) -> Result<Option<String>> {
    if paths_to_remove.is_empty() {
        return Ok(None);
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
        return Ok(Some(backup_branch));
    }

    let filter_branch_available = crate::policy::std_git_command()
        .args(["filter-branch", "--version"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if filter_branch_available {
        let mut args: Vec<String> = vec![
            "filter-branch".to_string(),
            "--force".to_string(),
            "--index-filter".to_string(),
        ];
        let filter_expr = "git rm -r --cached --ignore-unmatch".to_string();
        args.push(filter_expr);
        args.extend(paths_to_remove.iter().cloned());
        args.push("--".to_string());
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
        return Ok(Some(backup_branch));
    }

    Err(anyhow::anyhow!(
        "Neither git-filter-repo nor git-filter-branch available in {}. Install git-filter-repo (pip install git-filter-repo) or git-filter-branch to rewrite history (backup branch: {})",
        repo.display(),
        backup_branch
    ))
}

/// Restore paths from the index to the working tree.
pub(crate) async fn restore_paths(repo: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
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
