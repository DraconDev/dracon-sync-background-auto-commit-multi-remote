//! Diff and status operations — parse git diff/status output and collect staged entries.

use anyhow::{Context, Result};
use dracon_git::{
    types::{DiffFile, FileStatus},
    GitService,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Get the list of files that actually differ from HEAD (filter-aware).
/// Unlike `git status`, `git diff HEAD` applies clean filters and correctly
/// ignores files that only differ due to smudge filter decryption.
pub(crate) async fn git_diff_head_files(repo: &Path) -> Result<HashSet<PathBuf>> {
    let r = repo.to_path_buf();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(move || -> anyhow::Result<HashSet<PathBuf>> {
            let output = crate::git::git_cmd()
                .current_dir(&r)
                .args(["diff", "HEAD", "--name-only", "-z"])
                .output()?;
            if !output.status.success() {
                anyhow::bail!("git diff HEAD exited with {}", output.status);
            }
            let files: HashSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect();
            Ok(files)
        }),
    )
    .await;
    let inner = match outcome {
        Ok(inner) => inner,
        Err(_) => return Err(anyhow::anyhow!("git diff HEAD timed out")),
    };
    match inner {
        Ok(Ok(files)) => Ok(files),
        Ok(Err(e)) => Err(anyhow::anyhow!("git diff HEAD task failed: {}", e)),
        Err(e) => Err(anyhow::anyhow!("git diff HEAD task failed: {}", e)),
    }
}

/// Parse a single line from `git status --porcelain` or `git diff --name-status`.
pub(crate) fn parse_name_status_line(line: &str) -> Option<(PathBuf, FileStatus)> {
    let mut parts = line.split('\t');
    let status_raw = parts.next()?.trim();
    if status_raw.is_empty() {
        return None;
    }
    let status_char = status_raw.chars().next()?;
    let (path, status) = match status_char {
        'M' => (parts.next()?, FileStatus::Modified),
        'A' => (parts.next()?, FileStatus::Added),
        'D' => (parts.next()?, FileStatus::Deleted),
        'T' => (parts.next()?, FileStatus::TypeChange),
        'R' => {
            let _old = parts.next()?;
            let new = parts.next()?;
            (new, FileStatus::Renamed)
        }
        _ => return None,
    };
    Some((PathBuf::from(path.trim()), status))
}

/// Get name-status entries via `git diff --name-status` with custom args.
pub(crate) async fn git_name_status_entries(
    repo: &Path,
    args: &[&str],
) -> Result<Vec<(PathBuf, FileStatus)>> {
    let output = crate::git::tokio_git_cmd()
        .args(args)
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to run git {:?} in {}", args, repo.display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(parse_name_status_line)
        .collect::<Vec<_>>())
}

/// Git status rank for sorting: higher = more relevant to sync.
#[cfg(test)]
pub(crate) fn fallback_status_rank(status: &FileStatus) -> u8 {
    match status {
        FileStatus::Deleted => 5,
        FileStatus::Renamed => 4,
        FileStatus::TypeChange => 3,
        FileStatus::Added => 2,
        FileStatus::Modified => 1,
        FileStatus::Unknown => 0,
        _ => 0,
    }
}

/// Get diff entries via `git diff` CLI (fallback when libgit2 fails).
pub(crate) async fn cli_diff_entries(repo: &Path) -> Result<Vec<DiffFile>> {
    let output = crate::git::tokio_git_cmd()
        .args(["diff", "--name-status", "HEAD"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if let Some((path, status)) = parse_name_status_line(line) {
            entries.push(DiffFile::new(path, status));
        }
    }
    Ok(entries)
}

/// Get untracked file entries via `git ls-files --others --exclude-standard`.
pub(crate) async fn untracked_entries(repo: &Path) -> Result<Vec<DiffFile>> {
    let output = crate::git::tokio_git_cmd()
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|p| DiffFile::new(PathBuf::from(p), FileStatus::Added))
        .collect())
}

/// Get diff entries from both repo status, diff, and untracked files.
/// This ensures untracked files are included in the diff entries so the
/// daemon can detect and commit them.
pub(crate) async fn repo_diff_entries(repo: &Path) -> Result<Vec<DiffFile>> {
    let svc = GitService::new(repo)?;
    let status = svc.get_status().await?;
    if status.is_clean {
        return Ok(Vec::new());
    }
    // Get diff entries between HEAD and working tree (includes both staged
    // and unstaged modifications, but NOT untracked files).
    let diff = cli_diff_entries(repo).await?;
    if !diff.is_empty() {
        // Only return diff entries if there are actual changes.
        // Also include any untracked files that may exist alongside mods.
        let untracked = untracked_entries(repo).await.unwrap_or_default();
        if untracked.is_empty() {
            return Ok(diff);
        }
        let mut combined = diff;
        combined.extend(untracked);
        return Ok(combined);
    }
    // cli_diff_entries returned empty. Check for untracked files or
    // staged-only changes.
    let untracked = untracked_entries(repo).await.unwrap_or_default();
    if !untracked.is_empty() {
        return Ok(untracked);
    }
    // Only staged files (git add'ed but no working tree differences yet)
    // or repos where diff parsing produced no results.
    Ok(Vec::new())
}

/// Get staged file paths from `git diff --cached --name-only`.
pub(crate) async fn staged_paths(repo: &Path) -> Result<HashSet<PathBuf>> {
    let output = crate::git::tokio_git_cmd()
        .args(["diff", "--cached", "--name-only", "-z"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Get the set of all currently-tracked file paths in the index.
/// Used to distinguish untracked (new) files from freshly-staged
/// tracked files when `auto_stage_untracked = false` is set.
pub(crate) async fn tracked_paths(repo: &Path) -> Result<HashSet<PathBuf>> {
    let output = crate::git::tokio_git_cmd()
        .args(["ls-files", "-z"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}

