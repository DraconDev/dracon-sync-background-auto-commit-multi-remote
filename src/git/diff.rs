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
///
/// F33 (2026-07-19): previously the rename branch naively split on
/// `\t` and read two paths, which works for `R100\told\tnew` but is
/// silently wrong for the corner case where the OLD path is empty
/// (e.g. `git status --porcelain=v1` R-prefix can be ambiguous). We
/// now require the score to be present after the `R` (1-3 digits) and
/// reject anything that doesn't conform to `R<score>\t<old>\t<new>`.
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
            // Validate the score suffix: `R<digits>` (1-3 digits).
            let score = &status_raw[1..];
            if score.is_empty() || !score.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let old = parts.next()?;
            let new = parts.next()?;
            if old.is_empty() || new.is_empty() {
                return None;
            }
            (new, FileStatus::Renamed)
        }
        _ => return None,
    };
    Some((PathBuf::from(path.trim()), status))
}

/// Parse NUL-delimited `--name-status -z` output.
///
/// ADDED 2026-07-21 (v0.112.33, audit M17/F2.8): the line-based
/// parsers previously split `git diff --name-status` output on
/// newlines with `path.trim()` — with default `core.quotePath=true`,
/// a file `café.rs` arrived C-quoted (`"caf\303\251.rs"`, quotes
/// included) and became a PathBuf matching nothing on disk, and
/// newline-in-filename broke parsing outright. With `-z`, paths are
/// raw bytes (no quoting, no trimming) and records are NUL-separated:
/// `<status>\0<path>\0` for most statuses,
/// `<status>\0<old>\0<new>\0` for renames.
fn parse_name_status_z(stdout: &[u8]) -> Vec<(PathBuf, FileStatus)> {
    let mut entries = Vec::new();
    let mut iter = stdout.split(|b| *b == 0).filter(|s| !s.is_empty());
    while let Some(status_raw) = iter.next() {
        let status_str = String::from_utf8_lossy(status_raw);
        let to_path = |p: &[u8]| PathBuf::from(String::from_utf8_lossy(p).into_owned());
        match status_str.chars().next().unwrap_or(' ') {
            'M' => {
                if let Some(p) = iter.next() {
                    entries.push((to_path(p), FileStatus::Modified));
                }
            }
            'A' => {
                if let Some(p) = iter.next() {
                    entries.push((to_path(p), FileStatus::Added));
                }
            }
            'D' => {
                if let Some(p) = iter.next() {
                    entries.push((to_path(p), FileStatus::Deleted));
                }
            }
            'T' => {
                if let Some(p) = iter.next() {
                    entries.push((to_path(p), FileStatus::TypeChange));
                }
            }
            'R' => {
                let old = iter.next();
                let new = iter.next();
                if let (Some(_old), Some(new)) = (old, new) {
                    entries.push((to_path(new), FileStatus::Renamed));
                }
            }
            _ => {}
        }
    }
    entries
}

/// Get name-status entries via `git diff --name-status` with custom args.
pub(crate) async fn git_name_status_entries(
    repo: &Path,
    args: &[&str],
) -> Result<Vec<(PathBuf, FileStatus)>> {
    // CHANGED 2026-07-21 (v0.112.33, audit M17/F2.8): `-z` (raw,
    // NUL-delimited paths — non-ASCII filenames no longer dropped)
    // and non-zero exits now propagate as Err instead of reading as
    // "zero changed files".
    let output = crate::git::tokio_git_cmd()
        .args(args)
        .arg("-z")
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to run git {:?} in {}", args, repo.display()))?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git {:?} failed in {}: exit {}",
            args,
            repo.display(),
            output.status
        ));
    }
    Ok(parse_name_status_z(&output.stdout))
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
    // CHANGED 2026-07-21 (v0.112.33, audit M17/F2.8): `-z` + exit
    // status checked (was: status unchecked — a failed diff read as
    // "no changes").
    let output = crate::git::tokio_git_cmd()
        .args(["diff", "--name-status", "-z", "HEAD"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git diff --name-status -z HEAD failed in {}: exit {}",
            repo.display(),
            output.status
        ));
    }
    let mut entries = Vec::new();
    for (path, status) in parse_name_status_z(&output.stdout) {
        entries.push(DiffFile::new(path, status));
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
    //
    // CHANGED 2026-07-21 (v0.112.33, audit M17/F2.8):
    // `cli_diff_entries` now propagates non-zero exits as Err —
    // including `git diff HEAD` on an UNBORN repo (no HEAD yet),
    // which is a legitimate state (the v0.112.30 empty-repo
    // bootstrap depends on the untracked fallback below). Treat the
    // failure as "diff unavailable" and fall through to the
    // untracked-only path with a debug note, rather than aborting
    // the whole dirty-detection pipeline.
    let diff = match cli_diff_entries(repo).await {
        Ok(d) => d,
        Err(e) => {
            if crate::policy::debug_enabled() {
                eprintln!(
                    "🐛 {} cli_diff_entries unavailable ({}); falling back to untracked-only",
                    repo.display(),
                    e
                );
            }
            Vec::new()
        }
    };
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


#[cfg(test)]
mod f33_tests {
    use super::parse_name_status_line;

    #[test]
    fn parse_name_status_basic_statuses() {
        assert_eq!(
            parse_name_status_line("M\tsrc/lib.rs").unwrap().1,
            dracon_git::types::FileStatus::Modified
        );
        assert_eq!(
            parse_name_status_line("A\tnew.rs").unwrap().1,
            dracon_git::types::FileStatus::Added
        );
        assert_eq!(
            parse_name_status_line("D\told.rs").unwrap().1,
            dracon_git::types::FileStatus::Deleted
        );
    }

    #[test]
    fn parse_name_status_rename_with_score() {
        // F33 (2026-07-19): `git diff --name-status -M` emits
        // `R<score>\t<old>\t<new>`. Verify the score is required
        // and the OLD path is NOT mistakenly read as the new.
        let (path, status) = parse_name_status_line("R100\told_name.txt\tnew_name.txt").unwrap();
        assert_eq!(status, dracon_git::types::FileStatus::Renamed);
        assert_eq!(path.to_str().unwrap(), "new_name.txt");
    }

    #[test]
    fn parse_name_status_rename_without_score_rejected() {
        // F33: a bare `R` without a digit suffix is ambiguous
        // (could be the first char of a filename); reject it.
        assert!(parse_name_status_line("R\told.txt\tnew.txt").is_none());
    }

    #[test]
    fn parse_name_status_rename_with_non_digit_suffix_rejected() {
        // F33: the score must be digits; `Rabc` is invalid.
        assert!(parse_name_status_line("Rabc\told.txt\tnew.txt").is_none());
    }

    #[test]
    fn parse_name_status_rename_with_empty_paths_rejected() {
        // F33: an empty old or new path is malformed.
        assert!(parse_name_status_line("R100\t\tnew.txt").is_none());
        assert!(parse_name_status_line("R100\told.txt\t").is_none());
    }

    #[test]
    fn parse_name_status_unknown_status_returns_none() {
        assert!(parse_name_status_line("X\tfile.txt").is_none());
    }

    #[test]
    fn parse_name_status_empty_line_returns_none() {
        assert!(parse_name_status_line("").is_none());
        assert!(parse_name_status_line("\tfoo").is_none());
    }
}
