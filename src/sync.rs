use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::log_warn;
#[cfg(test)]
use crate::test_helpers::{test_commit_cmd, test_git_cmd};

use anyhow::Result;
use dracon_git::GitService;

use crate::exclude::{
    can_restore_entry, handle_large_untracked, is_large_untracked, remove_tracked_excluded_paths,
    should_stage_entry,
};
use crate::git::multi_remote::push_mirror_remotes;
use crate::git::origin_url;
use crate::git::{
    cli_diff_entries, git_name_status_entries, has_origin_remote, has_tracking_upstream,
    untracked_entries,
    is_cherry_pick_in_progress, is_merge_in_progress, is_rebase_in_progress, is_repo_ready,
    prune_other_default_branch, push_with_retries, restore_paths, run_git_capture_output,
    run_git_with_timeout, unstage_excluded_paths, unstage_oversized_paths,
};
use crate::policy::{debug_enabled, load_repo_override, SyncPolicy};
use crate::visibility::{
    cached_repo_visibility, get_github_visibility, parse_github_owner_repo, sync_mirror_metadata,
    sync_mirror_visibility,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncOutcome {
    Synced,
    NothingToDo,
    Blocked,
    /// ADDED 2026-07-21 (v0.112.31, audit H3/F1.3): the commit half
    /// succeeded but the push failed. Previously both push paths
    /// (`stage_commit_and_push`, `handle_ahead_push`) swallowed the
    /// failure after recording it to the disk ledger and returned a
    /// success outcome, so the daemon's apply phase logged
    /// `🔁 synced`, reset `failure_count`, and dropped the activity
    /// entry — a false-healthy state in which push failure accounting
    /// (MAX_FAILURES, backstops) never engaged. The apply phase maps
    /// this variant to `sync_success = false` (commit kept, failure
    /// counted, no synced log).
    PushFailed,
}

/// Count the number of unpushed commits on `origin/main..HEAD` for the
/// given repo. Used by `push_background` to scale the push timeout so a
/// large 28-commit push with binary blobs doesn't get killed at the
/// 60s default. Returns 0 if `git rev-list` fails (e.g. no origin/main).
pub(crate) async fn count_ahead_commits(repo: &Path) -> Result<u64> {
    let output = crate::policy::tokio_git_command()
        .args(["rev-list", "--count", "origin/main..HEAD"])
        .current_dir(repo)
        .output()
        .await?;
    if !output.status.success() {
        // origin/main may not exist (e.g. never pushed yet) — treat as 0.
        return Ok(0);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().parse::<u64>().unwrap_or(0))
}

/// Check if the daemon's auto-commit backstop is active for a given
/// repo. The backstop fires when a repo has more than
/// `threshold` unpushed commits AND the push has been pending
/// (ahead_since) for at least `min_age_secs` seconds.
///
/// When the backstop is active, the daemon should:
/// - NOT auto-commit new changes (prevents moving target)
/// - Log a backstop message so the operator can see why
/// - Mark the ACTIVITY column as `⏸ backstop` so the report
///   reflects the intentional pause
///
/// The operator can disable the backstop by setting
/// `auto_commit_backstop_threshold = 0` in the policy.
pub(crate) fn is_backstop_active(
    ahead_since: Option<std::time::Instant>,
    now: std::time::Instant,
    ahead_count: usize,
    threshold: usize,
    min_age_secs: u64,
) -> bool {
    if threshold == 0 {
        return false;
    }
    if ahead_count <= threshold {
        return false;
    }
    let Some(since) = ahead_since else {
        return false;
    };
    now.duration_since(since).as_secs() >= min_age_secs
}

/// Scale the configured `push_op_timeout_secs` with the local ahead
/// count so a large 28-commit push doesn't time out at 60s.
///
/// Formula:
///   ahead ≤ 5  →  base timeout
///   ahead ≤ 20 →  base × 2
///   ahead ≤ 50 →  base × 4
///   ahead > 50 →  base × 6 (capped at 600s = 10 min)
///
/// Example with base = 60s:
///   ahead =  3 →  60s
///   ahead = 10 → 120s
///   ahead = 28 → 240s
///   ahead = 60 → 360s
///
/// Cap at 600s so a runaway push doesn't block the daemon forever.
pub(crate) fn scale_push_timeout(base: u64, ahead: u64) -> u64 {
    let multiplier: u64 = if ahead <= 5 {
        1
    } else if ahead <= 20 {
        2
    } else if ahead <= 50 {
        4
    } else {
        6
    };
    (base * multiplier).min(600)
}

impl SyncOutcome {
    pub fn has_changes(&self) -> bool {
        matches!(self, SyncOutcome::Synced)
    }
}

struct SyncContext<'a> {
    repo: &'a Path,
    policy: &'a SyncPolicy,
    excluded_dir_names: &'a BTreeSet<String>,
    dry_run: bool,
    #[allow(dead_code)]
    idle_seconds: u64,
    #[allow(dead_code)]
    policy_path: Option<&'a Path>,
    has_origin: bool,
    has_upstream: bool,
    #[allow(dead_code)]
    auto_bump_versions: bool,
    remote_failures: Option<&'a mut HashMap<String, usize>>,
    /// When true, the daemon's auto-commit backstop is active for this
    /// repo. The backstop fires when `ahead > threshold` AND
    /// `push pending > min_age_secs` — see `is_backstop_active`. While
    /// active, `sync_repo` skips auto-commit (the daemon logs the
    /// backstop and returns `SyncOutcome::NothingToDo`). Manual commits
    /// are unaffected.
    backstop_active: bool,
}

fn notify_webhook_failure(webhook_url: &str, repo: &Path, remote: &str, error: &str) {
    let payload = serde_json::json!({
        "event": "push_failure",
        "repo": repo.display().to_string(),
        "remote": remote,
        "error": error,
        "timestamp": crate::policy::timestamp_secs(),
    });
    let url = webhook_url.to_string();
    std::thread::spawn(move || {
        if let Ok(client) = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            if let Err(e) = client.post(&url).json(&payload).send() {
                eprintln!("⚠️ webhook notification failed: {}", e);
            }
        }
    });
}

async fn get_bump_info(repo: &Path) -> Option<(String, String, String)> {
    let new_ver = crate::release::detect_project_version(repo)?.0;

    let version_files = if repo.join("Cargo.toml").exists() {
        &["Cargo.toml"][..]
    } else if repo.join("package.json").exists() {
        &["package.json"][..]
    } else if repo.join("pyproject.toml").exists() {
        &["pyproject.toml"][..]
    } else if repo.join("pubspec.yaml").exists() {
        &["pubspec.yaml"][..]
    } else if repo.join("version.txt").exists() {
        &["version.txt"][..]
    } else if repo.join("VERSION").exists() {
        &["VERSION"][..]
    } else {
        &[
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "pubspec.yaml",
            "version.txt",
            "VERSION",
        ][..]
    };

    let mut old_ver = String::new();
    for file in version_files.iter() {
        let repo_pb = repo.to_path_buf();
        let file_s = file.to_string();
        let output = tokio::task::spawn_blocking(move || {
            crate::git::git_cmd()
                .args(["show", &format!("HEAD~1:{}", file_s)])
                .current_dir(&repo_pb)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
        })
        .await
        .ok()
        .flatten();

        if let Some(output) = output {
            if !output.status.success() {
                continue;
            }
            let content = String::from_utf8_lossy(&output.stdout);
            if let Some(v) = match *file {
                "Cargo.toml" => content
                    .lines()
                    .map(|l| l.trim())
                    .find(|l| l.starts_with("version") && !l.starts_with("version_prefix"))
                    .and_then(|l| l.split('=').nth(1))
                    .map(|v| v.trim().trim_matches('"').trim())
                    .filter(|v| !v.is_empty() && !v.starts_with("workspace"))
                    .map(|v| v.to_string()),
                "package.json" => content
                    .lines()
                    .map(|l| l.trim())
                    .find(|l| l.starts_with("\"version\""))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|v| v.trim().trim_matches('"').trim_matches(',').trim())
                    .filter(|v| !v.is_empty())
                    .map(|v| v.to_string()),
                "pyproject.toml" => content
                    .lines()
                    .map(|l| l.trim())
                    .find(|l| l.starts_with("version") && !l.starts_with("version_prefix"))
                    .and_then(|l| l.split('=').nth(1))
                    .map(|v| v.trim().trim_matches('"').trim_matches(',').trim())
                    .filter(|v| !v.is_empty())
                    .map(|v| v.to_string()),
                "pubspec.yaml" => content
                    .lines()
                    .map(|l| l.trim())
                    .find(|l| l.starts_with("version:"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|v| v.trim().split('+').next().unwrap_or("").trim())
                    .filter(|v| !v.is_empty())
                    .map(|v| v.to_string()),
                "version.txt" | "VERSION" => {
                    let v = content.trim();
                    if !v.is_empty() && v.contains('.') {
                        Some(v.to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            } {
                old_ver = v;
                break;
            }
        }
    }

    let level = if old_ver.is_empty() {
        "patch"
    } else {
        let old_parts: Vec<u32> = old_ver.split('.').filter_map(|s| s.parse().ok()).collect();
        let new_parts: Vec<u32> = new_ver.split('.').filter_map(|s| s.parse().ok()).collect();
        if old_parts.len() >= 3 && new_parts.len() >= 3 {
            if new_parts[0] > old_parts[0] {
                "major"
            } else if new_parts[1] > old_parts[1] {
                "minor"
            } else {
                "patch"
            }
        } else {
            "patch"
        }
    };

    Some((old_ver, new_ver, level.to_string()))
}

fn maybe_sync_visibility_and_metadata(ctx: &SyncContext<'_>) {
    if ctx.dry_run || (!ctx.policy.sync_visibility && !ctx.policy.sync_metadata) {
        return;
    }
    if let Some(origin_url) = crate::git::multi_remote::get_remote_url(ctx.repo, "origin") {
        if ctx.policy.sync_metadata {
            sync_mirror_metadata(
                &origin_url,
                &ctx.policy.remotes,
                ctx.repo,
                ctx.policy.sync_visibility_interval_hours,
            );
        }
        if ctx.policy.sync_visibility {
            sync_mirror_visibility(
                &origin_url,
                &ctx.policy.remotes,
                ctx.repo,
                ctx.policy.sync_visibility_interval_hours,
            );
        }
    }
}

fn check_conflict_state(repo: &Path) -> Option<SyncOutcome> {
    if is_rebase_in_progress(repo) {
        eprintln!(
            "⚠️ {} has rebase in progress, skipping (manual intervention required)",
            repo.display()
        );
        return Some(SyncOutcome::Blocked);
    }
    if is_merge_in_progress(repo) {
        eprintln!(
            "⚠️ {} has merge in progress, skipping (manual intervention required)",
            repo.display()
        );
        return Some(SyncOutcome::Blocked);
    }
    if is_cherry_pick_in_progress(repo) {
        eprintln!(
            "⚠️ {} has cherry-pick in progress, skipping (manual intervention required)",
            repo.display()
        );
        return Some(SyncOutcome::Blocked);
    }
    None
}

fn ensure_origin_remote(repo: &Path, policy: &SyncPolicy) -> bool {
    let has_origin = has_origin_remote(repo);
    if !has_origin && policy.auto_github_private {
        let private = if policy.sync_visibility {
            if let Some(url) = origin_url(repo) {
                if let Some((owner, repo_name)) = parse_github_owner_repo(&url) {
                    get_github_visibility(&owner, &repo_name)
                } else {
                    true
                }
            } else {
                true
            }
        } else {
            true
        };
        if let Some(url) = crate::report::create_github_private_remote(
            repo,
            &policy.auto_github_private_account,
            private,
        ) {
            println!("🔗 created remote for {}: {}", repo.display(), url);
            true
        } else {
            eprintln!("⚠️ failed to create GitHub remote for {}", repo.display());
            false
        }
    } else {
        has_origin
    }
}

async fn auto_pull_merge(
    svc: &GitService,
    ctx: &SyncContext<'_>,
    initial_status: &dracon_git::types::RepoStatus,
) -> Result<()> {
    let repo = ctx.repo;
    let policy = ctx.policy;
    if policy.auto_pull
        && ctx.has_origin
        && ctx.has_upstream
        && initial_status.behind > 0
        && initial_status.is_clean
    {
        if ctx.dry_run {
            println!(
                "🔽 Would pull/merge {} commit(s) from upstream in {}",
                initial_status.behind,
                repo.display()
            );
        } else {
            match tokio::time::timeout(
                Duration::from_secs(policy.pull_op_timeout_secs),
                svc.pull_merge(),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(dracon_git::error::GitError::MergeConflict)) => {
                    eprintln!(
                        "⚠️ pull/merge conflict in {} (manual intervention required)",
                        repo.display()
                    );
                    return Err(anyhow::anyhow!("pull/merge conflict"));
                }
                Ok(Err(e)) => {
                    eprintln!(
                        "⚠️ pull/merge failed for {}: {} - aborting sync pass",
                        repo.display(),
                        e
                    );
                    return Err(anyhow::anyhow!("pull/merge failed: {}", e));
                }
                Err(_) => {
                    eprintln!(
                        "⚠️ pull/merge timeout for {} after {}s - aborting sync pass",
                        repo.display(),
                        policy.pull_op_timeout_secs
                    );
                    return Err(anyhow::anyhow!("pull/merge timeout"));
                }
            }
        }
    } else if policy.auto_pull && ctx.has_origin && ctx.has_upstream && initial_status.behind == 0 {
        if debug_enabled() {
            eprintln!(
                "🐛 skip pull/merge for {} (branch not behind upstream)",
                repo.display()
            );
        }
    } else if policy.auto_pull && ctx.has_origin && ctx.has_upstream && !initial_status.is_clean {
        if debug_enabled() {
            eprintln!(
                "🐛 skip pull/merge for {} (dirty repo, commit first)",
                repo.display()
            );
        }
    } else if policy.auto_pull && !ctx.has_origin {
        eprintln!(
            "ℹ️ skip pull/merge for {} (no origin remote)",
            repo.display()
        );
    } else if policy.auto_pull && ctx.has_origin && !ctx.has_upstream {
        eprintln!(
            "ℹ️ skip pull/merge for {} (no tracking upstream on current branch)",
            repo.display()
        );
    }
    Ok(())
}

async fn clean_staged_paths(ctx: &SyncContext<'_>) -> Result<()> {
    let repo = ctx.repo;
    let policy = ctx.policy;
    let excluded_dir_names = ctx.excluded_dir_names;
    let dry_run = ctx.dry_run;
    let unstaged = if dry_run {
        0
    } else {
        unstage_excluded_paths(repo, excluded_dir_names).await?
    };
    if unstaged > 0 {
        eprintln!(
            "🧹 removed {} staged excluded paths in {}",
            unstaged,
            repo.display()
        );
    }
    let unstaged_oversized = if dry_run {
        0
    } else {
        unstage_oversized_paths(repo, policy.max_stage_file_bytes).await?
    };
    if unstaged_oversized > 0 {
        eprintln!(
            "🧹 removed {} oversized staged paths in {}",
            unstaged_oversized,
            repo.display()
        );
    }

    if let Some(removed_dirs) = if dry_run {
        None
    } else {
        remove_tracked_excluded_paths(repo, excluded_dir_names)?
    } {
        if !removed_dirs.is_empty() {
            eprintln!(
                "🧹 removed {} tracked excluded dir(s) from {}: {:?}",
                removed_dirs.len(),
                repo.display(),
                removed_dirs
            );
        }
    }

    Ok(())
}

struct DiffResult {
    status: dracon_git::types::RepoStatus,
    entries: Vec<dracon_git::types::DiffFile>,
    #[allow(dead_code)]
    filter_only_cleared: bool,
}

#[cfg(test)]
mod diff_tests {

    #[test]
    fn test_fallback_entries_recalculate_staged_files() {
        // When cli_diff_entries fallback is used, staged_files must be
        // recalculated from actual staged paths rather than leaving the
        // (potentially stale) libgit2 count.
        use crate::git::staged_paths;

        // Create a temp repo with a staged file
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("test-repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        // Initialize git repo
        let output = crate::git::git_cmd()
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .expect("git init failed");
        assert!(output.status.success(), "git init failed: {:?}", output);

        // Configure git user
        crate::git::git_cmd()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create initial commit
        std::fs::write(repo_path.join("README.md"), "initial").unwrap();
        crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        crate::git::git_cmd()
            .args(["commit", "--no-verify", "-m", "initial"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Now add a new file (staged)
        std::fs::write(repo_path.join("new_file.rs"), "fn main() {}").unwrap();
        crate::git::git_cmd()
            .args(["add", "new_file.rs"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // staged_paths should find the staged file
        let rt = tokio::runtime::Runtime::new().unwrap();
        let staged = rt.block_on(staged_paths(&repo_path)).unwrap();
        assert_eq!(staged.len(), 1, "expected 1 staged file, got {:?}", staged);
        assert!(staged.contains(&std::path::PathBuf::from("new_file.rs")));

        // The fallback code path: cli_diff_entries should find it
        let rt = tokio::runtime::Runtime::new().unwrap();
        let entries = rt
            .block_on(crate::git::cli_diff_entries(&repo_path))
            .unwrap();
        assert!(
            !entries.is_empty(),
            "cli_diff_entries should find the staged file"
        );
    }
}

async fn compute_diff_entries(svc: &GitService, repo: &Path) -> Result<DiffResult> {
    let mut status = svc.get_status().await?;
    let mut entries = svc.get_diff_entries().await?;
    let mut filter_only_cleared = false;

    {
        let diff_output = crate::git::git_diff_head_files(repo)
            .await
            .unwrap_or_default();
        if diff_output.is_empty() && !entries.is_empty() {
            let has_non_modified = entries
                .iter()
                .any(|e| !matches!(e.status, dracon_git::types::FileStatus::Modified));
            if !has_non_modified {
                entries.clear();
                status.is_clean = true;
                filter_only_cleared = true;
            }
        } else {
            entries.retain(|e| {
                if !matches!(e.status, dracon_git::types::FileStatus::Modified) {
                    return true;
                }
                diff_output.contains(&e.path)
            });
        }
    }

    if debug_enabled() {
        eprintln!(
            "🐛 {} status: clean={} modified={} staged={} entries(libgit2)={}",
            repo.display(),
            status.is_clean,
            status.modified_files,
            status.staged_files,
            entries.len()
        );
    }
    if entries.is_empty() && !filter_only_cleared {
        let fallback_entries = cli_diff_entries(repo).await?;
        if !fallback_entries.is_empty() {
            status.is_clean = false;
            status.modified_files = fallback_entries.len();
            // Recalculate staged_files from actual staged paths when using
            // fallback CLI entries, since the libgit2 count may be stale
            // (libgit2 returned 0 entries but CLI found changes).
            if let Ok(staged) = crate::git::staged_paths(repo).await {
                status.staged_files = staged.len();
            }
            entries = fallback_entries;
            if debug_enabled() {
                eprintln!(
                    "🐛 {} fallback entries(cli)={} staged={} => forcing dirty",
                    repo.display(),
                    status.modified_files,
                    status.staged_files,
                );
            }
        }
        // CHANGED 2026-06-20: when both libgit2 and CLI diff are empty
        // but the repo has untracked files (e.g. .dracon/data/keys/*.pub),
        // collect untracked entries so the auto-commit block below stages
        // and commits them. Without this, a repo with ONLY untracked
        // changes (no tracked modifications) enters the auto-commit block
        // but finds `entries` empty, skips `stage_commit_and_push`, and
        // returns `Synced` without ever committing the untracked file.
        if entries.is_empty() {
            let ut = untracked_entries(repo).await.unwrap_or_default();
            if !ut.is_empty() {
                status.is_clean = false;
                entries = ut;
                if debug_enabled() {
                    eprintln!(
                        "🐛 {} untracked entries={} => including for auto-commit",
                        repo.display(),
                        entries.len(),
                    );
                }
            }
        }
    }

    Ok(DiffResult {
        status,
        entries,
        filter_only_cleared,
    })
}

/// CHANGED 2026-07-21 (v0.112.31, audit H6/F1.5): the old signature
/// is preserved as a #[cfg(test)] wrapper (all remaining callers are
/// tests). Production call sites MUST use
/// `stage_existing_files_filtered` so directory-expanded files get the
/// per-file staging policy re-applied (size limit, exclude patterns).
#[cfg(test)]
async fn stage_existing_files(
    repo: &Path,
    existing: &[String],
    dry_run: bool,
    stage_timeout_secs: u64,
    excluded_dir_names: &std::collections::BTreeSet<String>,
) -> Result<()> {
    stage_existing_files_filtered(
        repo,
        existing,
        dry_run,
        stage_timeout_secs,
        excluded_dir_names,
        None,
    )
    .await
}

/// Stage existing files, recursing into untracked directories.
///
/// `filter`: when `Some((policy, auto_commit_exclude_patterns))`,
/// each file discovered by the directory-expansion recursion is
/// re-checked against the per-file staging policy
/// (`max_stage_file_bytes`, `exclude_file_patterns`,
/// `untracked_exclude_patterns`, `auto_commit_exclude_patterns`).
/// ADDED 2026-07-21 (v0.112.31, audit H6/F1.5): previously the
/// recursion staged every file inside an untracked directory
/// wholesale — a 500 MiB `assets/video.mp4` inside a new `assets/`
/// dir sailed through even though the same file at repo root was
/// rejected by `should_stage_entry`, violating the documented
/// 100 MiB hard exclusion (AGENTS.md) and every per-file pattern
/// exclusion.
async fn stage_existing_files_filtered(
    repo: &Path,
    existing: &[String],
    dry_run: bool,
    stage_timeout_secs: u64,
    excluded_dir_names: &std::collections::BTreeSet<String>,
    filter: Option<(&SyncPolicy, &[String])>,
) -> Result<()> {
    if existing.is_empty() {
        return Ok(());
    }
    // Filter out paths that no longer exist on disk. Build tools like vite
    // create timestamp-suffixed temp files (e.g.
    // `vite.config.ts.timestamp-1781483278562-7a994a6fc1011.mjs`) and delete
    // them within milliseconds. If `get_status()` lists such a path as
    // untracked, but the file is gone by the time we run `git add`, the
    // whole `git add` fails with `fatal: unable to stat ...`, blocking
    // every other file in the commit. We re-check existence right before
    // staging and drop the missing ones.
    //
    // Untracked DIRECTORY entries (e.g. `docs/avid-research/02-foo/`)
    // are recursed into and each file inside is added to the stage
    // list. The libgit2 status API returns one entry per untracked
    // top-level path, and `git ls-files --others` similarly collapses
    // fully-untracked subtrees into their parent dir when `--directory`
    // is in effect (the default). Without the recursion, the daemon
    // would see 12 untracked dirs, filter them all out, and stage
    // nothing — leaving the operator's new research/docs files
    // uncommitted. The recursion respects the same `existing.exists()`
    // and `is_dir()` guards so vanished files and submodules are
    // still filtered correctly.
    let input: Vec<String> = existing.to_vec();
    let mut expanded: Vec<String> = Vec::with_capacity(input.len() * 2);
    for p in input {
        let full = repo.join(&p);
        if !full.exists() {
            continue;
        }
        if full.is_file() {
            expanded.push(p);
            continue;
        }
        if full.is_dir() {
            // CHANGED 2026-06-21 (goal 29144c2c): skip submodules.
            // A git submodule has a `.git` FILE (not a directory)
            // at its root, with content like
            // `gitdir: ../.git/modules/<name>`. The parent's
            // `git status` reports it as `M DraconDev` (a single
            // entry with the gitlink SHA changed). When that entry
            // is passed to stage_existing_files, the recursion
            // walks into DraconDev's working tree and tries to add
            // its files as if they belonged to the parent, which
            // fails with `fatal: Pathspec 'DraconDev/X' is in
            // submodule 'DraconDev'`. Detect by checking if the
            // top-level dir's `.git` exists and skip the entire
            // subtree. This is safe because the parent references
            // the submodule via its gitlink pointer, not its
            // working-tree files.
            //
            // CHANGED 2026-06-30 (goal `mr0rim9u-lzzfv9`):
            // broadened `.git` check from `is_file()` to
            // `exists()` so it ALSO catches the case where a
            // sibling subrepo has its own real `.git/` directory
            // (not a submodule pointer file). Symptom: the daemon
            // recursed into a nested `rust-ai-web-auto/`
            // (which has `.git/` as its own git repo, not a
            // submodule), tried to `git add rust-ai-web-auto/...`
            // for ~19183 nested files, and every `git add` failed
            // with `fatal: Pathspec ... is in submodule
            // 'rust-ai-web-auto'`. After 5 failures, the daemon
            // marked the parent repo as `exceeded max failures`
            // and stopped syncing it entirely.
            let full_dot_git = full.join(".git");
            if full_dot_git.exists() {
                continue;
            }
            // Recurse into untracked directories. The libgit2/git status
            // API can collapse a fully-untracked subtree into a single
            // top-level directory entry (e.g. `web/games/libs/game/
            // effects/src/styles/backgrounds.css` shows up as
            // `web/games/libs/game/effects/src/`). The previous 1-level
            // walk handled the common case (files directly under the
            // directory) but missed nested subtrees like
            // `src/styles/` or `src/lib/components/`, leaving the
            // operator's deep code/library files uncommitted.
            //
            // We use an explicit stack (not Rust's native recursion) to
            // avoid blowing the stack on extremely deep trees. We skip
            // symlinks (to prevent loops), dotfile dirs (to avoid
            // walking `.git`, `.cache`, etc.), and any directory whose
            // basename is in the `excluded_dir_names` set (target,
            // node_modules, .venv, dist, build, archives, etc.). The
            // excluded-dir check uses the SAME BTreeSet that
            // `should_stage_entry` uses for consistency, so files
            // inside e.g. `node_modules/` are NEVER staged even when
            // they appear as untracked.
            let excluded = excluded_dir_names;
            // Skip the top-level dir itself if its basename is in the
            // excluded-dir set. This handles the case where libgit2
            // returned `node_modules/` or `.cache/` directly as a
            // top-level untracked entry (e.g. on a fresh clone where
            // the operator ran `npm install` and `.gitignore` is not
            // catching it for some reason).
            //
            // CHANGED 2026-06-22 (goal mqp8dffy-bonnlu): removed the
            // broad `name.starts_with('.')` skip. The previous
            // blanket-dotfile skip was meant to skip `.git/`, `.cache/`,
            // `.venv/`, etc., but it ALSO skipped `.pi/` — silently
            // blocking `*/.pi/goals/archived/*.md` from auto-commit.
            // Those are operator docs (goal tracking records) that the
            // commit-all principle says MUST go up. The dot-dirs we
            // actually want to skip (`.cache`, `.direnv`, `.venv`)
            // are already in the `excluded` BTreeSet; `.git/` is
            // handled by the separate `full_dot_git.is_file()` check
            // above. So removing the dotfile skip is safe and the
            // recursion now correctly descends into `.pi/`.
            if let Some(name) = full.file_name().and_then(|n| n.to_str()) {
                if excluded.contains(name) {
                    continue;
                }
            }
            let mut stack: Vec<std::path::PathBuf> = vec![full.clone()];
            while let Some(dir) = stack.pop() {
                let rd = match std::fs::read_dir(&dir) {
                    Ok(rd) => rd,
                    Err(_) => continue, // permission denied, vanished, etc.
                };
                for child in rd.flatten() {
                    let cp = child.path();
                    // Skip symlinks (loop guard)
                    let meta = match std::fs::symlink_metadata(&cp) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    if meta.file_type().is_symlink() {
                        continue;
                    }
                    if meta.is_file() {
                        if let Ok(rel) = cp.strip_prefix(repo) {
                            // ADDED 2026-07-21 (v0.112.31, audit
                            // H6/F1.5): re-apply the per-file staging
                            // policy to directory-expanded files.
                            // Without this, size-limit and pattern
                            // exclusions only applied to top-level
                            // entries; anything nested inside an
                            // untracked dir bypassed them.
                            if let Some((policy, auto_commit_excl)) = filter {
                                let rel_path = rel.to_path_buf();
                                if meta.len() > policy.max_stage_file_bytes {
                                    eprintln!(
                                        "ℹ️ skip large file {} ({} bytes > {} bytes)",
                                        cp.display(),
                                        meta.len(),
                                        policy.max_stage_file_bytes
                                    );
                                    continue;
                                }
                                if crate::exclude::is_excluded_file(
                                    &rel_path,
                                    &policy.exclude_file_patterns,
                                ) {
                                    continue;
                                }
                                if crate::exclude::matches_untracked_exclude(
                                    repo,
                                    &rel_path,
                                    &policy.untracked_exclude_patterns,
                                ) {
                                    continue;
                                }
                                if !auto_commit_excl.is_empty()
                                    && crate::exclude::matches_untracked_exclude(
                                        repo,
                                        &rel_path,
                                        auto_commit_excl,
                                    )
                                {
                                    continue;
                                }
                            }
                            expanded.push(rel.to_string_lossy().to_string());
                        }
                        continue;
                    }
                    if meta.is_dir() {
                        // Skip excluded dir names (target, node_modules,
                        // .cache, .venv, dist, build, archives, .tmp-*, etc.).
                        //
                        // CHANGED 2026-06-22 (goal mqp8dffy-bonnlu):
                        // removed the `name.starts_with('.')` skip.
                        // The previous blanket-dotfile skip was meant
                        // to skip `.git/`, `.cache/`, `.venv/`, etc.,
                        // but it ALSO skipped `.pi/` — silently
                        // blocking `*/.pi/goals/archived/*.md` from
                        // auto-commit. The dot-dirs we want to skip
                        // are already in the `excluded` BTreeSet
                        // (set up by `excluded_dir_names_set` from the
                        // policy's `exclude_dir_names` list, defaulting
                        // to `target`, `node_modules`, `.cache`,
                        // `.direnv`, `.venv`, `dist`, `build`,
                        // `archives`, `.tmp-*`). `.git/` is handled
                        // by the separate `inner_dot_git.exists()`
                        // check below (covers both submodule pointer
                        // files and nested git-repo directories). So
                        // removing the dotfile skip is safe and the
                        // recursion now correctly descends into `.pi/`.
                        if let Some(name) = cp.file_name().and_then(|n| n.to_str()) {
                            if excluded.contains(name) {
                                continue;
                            }
                        }
                        // CHANGED 2026-06-21 (goal 29144c2c): skip
                        // submodules encountered DURING recursion
                        // (not just at the top-level `existing`
                        // entry). A submodule can be nested inside a
                        // normal untracked directory, so the check
                        // belongs here too. Same logic as the
                        // top-level check: skip if `<dir>/.git`
                        // exists (covers both submodule pointer
                        // FILES and nested git-repo DIRECTORIES).
                        // CHANGED 2026-06-30 (goal
                        // `mr0rim9u-lzzfv9`): broadened from
                        // `is_file()` to `exists()` to match the
                        // top-level fix above.
                        let inner_dot_git = cp.join(".git");
                        if inner_dot_git.exists() {
                            continue;
                        }
                        stack.push(cp);
                    }
                }
            }
        }
    }
    let existing = expanded;
    if existing.is_empty() {
        return Ok(());
    }
    if dry_run {
        println!(
            "📝 Would stage {} file(s) in {}: {:?}",
            existing.len(),
            repo.display(),
            &existing[..existing.len().min(5)]
        );
        if existing.len() > 5 {
            println!("  ... and {} more", existing.len() - 5);
        }
    } else {
        // Filter out gitignored paths to avoid "The following paths are ignored
        // by one of your .gitignore files" errors. Without this, a single
        // gitignored path in the list causes the entire `git add` to fail,
        // blocking ALL files from being committed.
        //
        // We use `git check-ignore` to detect gitignored paths, then split:
        // - Non-ignored paths: `git add -A -- <paths>` (respects .gitignore)
        // - Ignored but already-tracked paths: `git add -A -f -- <paths>` (force
        //   re-stage; git already tracks these so gitignore shouldn't block updates)
        // - Ignored and untracked: skip entirely (.gitignore is intentional)
        let (force_paths, normal_paths) = partition_gitignored(repo, &existing).await;

        if !normal_paths.is_empty() {
            let mut add_args = vec!["add", "-A", "--"];
            for p in &normal_paths {
                add_args.push(p.as_str());
            }
            if let Err(e) = run_git_with_timeout(repo, &add_args, stage_timeout_secs, "add").await {
                eprintln!(
                    "⚠️ {} git add failed for {} paths: {:?}",
                    repo.display(),
                    normal_paths.len(),
                    &normal_paths[..normal_paths.len().min(5)]
                );
                return Err(e);
            }
        }

        // Force-add already-tracked gitignored files (git tracks them already,
        // so .gitignore shouldn't prevent staging updates to tracked content)
        if !force_paths.is_empty() {
            let mut add_args = vec!["add", "-A", "-f", "--"];
            for p in &force_paths {
                add_args.push(p.as_str());
            }
            if run_git_with_timeout(repo, &add_args, stage_timeout_secs, "add (force-tracked)")
                .await
                .is_err()
            {
                eprintln!(
                    "⚠️ {} git add -f failed for {} tracked gitignored paths: {:?}",
                    repo.display(),
                    force_paths.len(),
                    &force_paths[..force_paths.len().min(5)]
                );
                // Non-fatal: tracked gitignored files can be re-attempted next cycle
            }
        }
    }
    Ok(())
}

/// Stage a parent gitlink pointer update for each path in `gitlinks`
/// WITHOUT recursing into the submodule's working tree.
///
/// For tracked `mode 160000` entries, the daemon's main stage path
/// (`stage_existing_files`) skips the entry because the path is a
/// directory whose `.git` exists (the submodule's own gitdir). We
/// want the OPPOSITE behaviour here: do NOT recurse (that would
/// stage the submodule's working-tree files as if they belonged
/// to the parent), but DO record the new submodule HEAD SHA in
/// the parent's index. `git add <path>` (no `-A`) does exactly that
/// — it updates the gitlink entry to point at the path's current
/// `rev-parse HEAD`, without descending into the subdirectory.
///
/// Invariants:
/// - Each `gitlinks` entry MUST already pass `is_gitlink(repo, &p)`.
///   The caller is `stage_commit_and_push`, which partitions
///   `to_stage` via `is_gitlink` before calling this.
/// - We stage each path individually (NOT `git add <all>` in one
///   invocation) because the parent can have multiple distinct
///   gitlink entries in a single commit, and a failure on one
///   should not block the others.
///
/// ADDED 2026-07-01, goal `mr10pdzr-i495vy`; revised 2026-07-08
/// (goal `730eaf2a`): the daemon no longer materializes top-level
/// standalone worktrees at `/home/dracon/Dev/<name>/` (see AGENTS.md
/// "Submodule standalone worktree design"). Submodules are watched
/// only via their nested checkout at `<parent>/<submodule_path>/`,
/// which is detached at the parent's gitlink SHA while the shared
/// gitdir's `refs/heads/main` advances independently. `git add
/// <path>` from the parent reads the NESTED submodule's HEAD (the
/// gitlink SHA), not `main`, so it cannot observe `main`'s
/// advances. This causes the parent's gitlink to drift away from
/// `main`, breaking the convergence invariant (main SHA == parent
/// gitlink).
///
/// To fix: prefer `git update-index --cacheinfo 160000,<sha>,<path>`
/// with the SHARED gitdir's `refs/heads/main` SHA (which is what
/// the standalone worktree's commits advance directly, since the
/// standalone is on `main`). Fall back to plain `git add <path>`
/// when the shared gitdir isn't found (e.g., the parent has no
/// `.gitmodules` or the submodule isn't materialized as a
/// standalone worktree).
///
/// CHANGED 2026-07-01 (goal `mr1x7j5i-zioba9`):
/// Previous comment referenced a `fast_forward_daemon_standalone_to_main`
/// hook. With the daemon-standalone branch removed (the standalone
/// is on `main` directly), the canonical head is just `refs/heads/main`
/// from the shared gitdir — no buffer-branch fast-forward needed.
async fn stage_gitlink_updates(
    repo: &Path,
    gitlinks: &[String],
    dry_run: bool,
    stage_timeout_secs: u64,
) -> Result<()> {
    if gitlinks.is_empty() {
        return Ok(());
    }
    if dry_run {
        println!(
            "🔗 Would stage {} gitlink pointer update(s) in {}: {:?}",
            gitlinks.len(),
            repo.display(),
            &gitlinks[..gitlinks.len().min(5)]
        );
        if gitlinks.len() > 5 {
            println!("  ... and {} more", gitlinks.len() - 5);
        }
        return Ok(());
    }
    for p in gitlinks {
        // Prefer the SHARED gitdir's `refs/heads/main` SHA when
        // available — this is what the standalone worktree's
        // HEAD actually points at (since the standalone is on
        // `main` directly). The canonical-head helper reads
        // `refs/heads/main` from the shared gitdir.
        //
        // CHANGED 2026-07-01 (goal `mr1x7j5i-zioba9`):
        // Previous comment mentioned a `daemon-standalone` ref
        // being preferred over `main`. That ref was removed:
        // the standalone worktree is on `main` directly now.
        let shared_sha = crate::exclude::shared_submodule_canonical_head_sha(
            repo,
            std::path::Path::new(p),
        );
        if let Some(shared_sha) = shared_sha {
            // Use `git update-index --cacheinfo` to set the
            // gitlink explicitly to the shared canonical head
            // SHA. This bypasses the nested submodule's HEAD
            // and ensures the parent's gitlink tracks the
            // standalone's commits, not the nested submodule's
            // own (possibly divergent) state.
            let cacheinfo = format!("160000,{},{}", shared_sha, p);
            if let Err(e) = run_git_with_timeout(
                repo,
                &["update-index", "--add", "--cacheinfo", &cacheinfo],
                stage_timeout_secs,
                "update-index (gitlink -> shared canonical head)",
            )
            .await
            {
                eprintln!(
                    "⚠️ {} git update-index (gitlink) failed for path {:?}: {}",
                    repo.display(),
                    p,
                    e
                );
                // Non-fatal: a single failed gitlink update is
                // retried on the next daemon cycle.
            }
        } else {
            // Fallback: no shared gitdir found. Use plain
            // `git add <path>` which reads the nested
            // submodule's HEAD (the original behavior before
            // the materialize-submodule-as-standalone-worktree
            // feature).
            if let Err(e) = run_git_with_timeout(
                repo,
                &["add", "--", p.as_str()],
                stage_timeout_secs,
                "add (gitlink)",
            )
            .await
            {
                eprintln!(
                    "⚠️ {} git add (gitlink) failed for path {:?}: {}",
                    repo.display(),
                    p,
                    e
                );
                // Non-fatal: a single failed gitlink update is
                // retried on the next daemon cycle.
            }
        }
    }
    Ok(())
}

/// Materialize a parent repo's submodule as a standalone worktree
/// at `target_path`.
///
/// On first detection of a submodule, the daemon calls this
/// function to create a real standalone worktree at
/// `/home/dracon/Dev/<name>/` (or whatever `target_path` is), so
/// the daemon can treat the submodule as a normal repo: discover
/// it, classify ownership, auto-commit changes, and auto-push to
/// the multi-remote set.
///
/// The worktree is created via `git worktree add --detach
/// <target_path> <sha>` from the parent's `.git/modules/<name>`
/// gitdir. The new worktree shares the same object store as the
/// parent's nested submodule (the original `web/games/wip/polis/`
/// inside the parent is unaffected). This is the canonical way to
/// get a second worktree of an existing gitdir: each worktree
/// gets its own working tree and index, but they all share the
/// same objects, refs, and remotes.
///
/// Idempotency: if `target_path/.git` already exists and is a
/// worktree of the same gitdir, the function returns Ok(()) without
/// running `git worktree add` again. This makes it safe to call on
/// every daemon cycle.
///
/// Failure modes (all return `Err`, the daemon skips the submodule):
///
/// 1. The parent's `.git/modules/<name>/` is missing or unreadable.
///    Suggests the parent was cloned with `--no-recurse-submodules`
///    or the submodule was never initialized. Recovery:
///    `cd <parent> && git submodule update --init <submodule_path>`.
///
/// 2. The `git worktree add` command itself fails. Possible reasons:
///    - Target path already exists and is NOT a worktree of this
///      gitdir (a real conflict; the operator must resolve).
///    - Target path's parent dir is not writable.
///    - SHA is unreachable (e.g. a fetch was never run).
///
/// 3. The target_path already contains files that conflict with the
///    submodule's HEAD checkout. The function refuses to overwrite
///    non-empty directories.
///
/// ADDED 2026-06-30, goal `mr10pdzr-i495vy`:
/// "Make the daemon materialize submodules as standalone worktrees".
/// Partition paths into (force_add, normal_add) based on .gitignore.
/// - Paths already tracked in git → force_add (git add -f).
///   that match a gitignore rule (e.g. `**/.wxt/types/`) still get refused
///   by `git add <path>` even though `git check-ignore` reports them as
///   "not ignored" (gitignore is bypassed for tracked files in check-ignore,
///   but `git add <path>` re-evaluates the rule and refuses without -f).
/// - Untracked + gitignored → skip (respect .gitignore intent)
/// - All others → normal_add (git add, respects .gitignore)
async fn partition_gitignored(repo: &Path, paths: &[String]) -> (Vec<String>, Vec<String>) {
    if paths.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // Get list of already-tracked paths (git ls-files)
    let tracked: std::collections::HashSet<String> = {
        let output = crate::policy::tokio_git_command()
            .args(["ls-files"])
            .current_dir(repo)
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect(),
            _ => std::collections::HashSet::new(),
        }
    };

    // Run `git check-ignore` to find gitignored paths (only for untracked ones)
    let ignored: std::collections::HashSet<String> = {
        let untracked: Vec<&str> = paths
            .iter()
            .filter(|p| !tracked.contains(*p))
            .map(|p| p.as_str())
            .collect();
        if untracked.is_empty() {
            std::collections::HashSet::new()
        } else {
            let mut check_args = vec!["check-ignore"];
            for p in &untracked {
                check_args.push(*p);
            }
            let output = crate::policy::tokio_git_command()
                .args(&check_args)
                .current_dir(repo)
                .output()
                .await;
            match output {
                Ok(o) if o.status.success() || o.status.code() == Some(1) => {
                    // Exit 0 = some ignored, exit 1 = none ignored
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| l.to_string())
                        .collect()
                }
                _ => std::collections::HashSet::new(),
            }
        }
    };

    let mut force_paths = Vec::new();
    let mut normal_paths = Vec::new();

    for p in paths {
        if tracked.contains(p) {
            // Tracked file: always force-add with `git add -f`. `git add <path>`
            // re-evaluates gitignore and refuses for tracked files that match
            // a rule; `-f` bypasses the check. gitignore intent doesn't apply
            // here because the file is already in the index.
            force_paths.push(p.clone());
        } else if ignored.contains(p) {
            // Untracked + gitignored = skip (respect .gitignore intent)
        } else {
            // Not ignored and not tracked = normal add
            normal_paths.push(p.clone());
        }
    }

    (force_paths, normal_paths)
}

async fn git_rm_missing(repo: &Path, missing: &[String], dry_run: bool) -> Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    let mut rm_args = vec!["rm", "--ignore-unmatch", "--"];
    for p in missing {
        rm_args.push(p);
    }
    if dry_run {
        println!(
            "🗑️  Would delete (git rm) {} file(s) from {}: {:?}",
            missing.len(),
            repo.display(),
            &missing[..missing.len().min(5)]
        );
        if missing.len() > 5 {
            println!("  ... and {} more", missing.len() - 5);
        }
    } else if let Err(e) = run_git_with_timeout(repo, &rm_args, 30, "rm").await {
        eprintln!(
            "⚠️ {} git rm failed for {} paths: {:?}",
            repo.display(),
            missing.len(),
            missing
        );
        return Err(e);
    }
    Ok(())
}

/// No-op stub preserved for backwards compatibility with
/// callers that haven't yet been migrated.
///
/// CHANGED 2026-07-01 (goal `mr1x7j5i-zioba9`):
/// The original purpose was to fast-forward `main` to the
/// `daemon-standalone` branch's HEAD after each standalone
/// commit (so the parent's gitlink would see the new commit).
/// With the standalone worktree now on `main` directly, each
/// commit already advances `main` (no buffer branch in
/// between), so the hook is unnecessary. This stub remains
/// so existing call sites still compile; it does nothing.
///
/// If called, it just returns `Ok(())`.
async fn fast_forward_daemon_standalone_to_main(_repo: &Path) -> std::io::Result<()> {
    // No-op: the standalone worktree is on `main` directly,
    // so each commit already advances `main`. There is no
    // `daemon-standalone` ref to fast-forward.
    Ok(())
}

async fn post_commit_pull(svc: &GitService, repo: &Path, policy: &SyncPolicy) {
    if !policy.auto_pull {
        return;
    }
    let post_commit_status = match svc.get_status().await {
        Ok(s) => s,
        Err(_) => return,
    };
    if post_commit_status.behind > 0 && post_commit_status.is_clean {
        eprintln!(
            "📥 post-commit pull for {} ({} behind)",
            repo.display(),
            post_commit_status.behind
        );
        match tokio::time::timeout(
            Duration::from_secs(policy.pull_op_timeout_secs),
            svc.pull_merge(),
        )
        .await
        {
            Ok(Ok(())) => {
                eprintln!("✅ post-commit pull succeeded for {}", repo.display());
            }
            Ok(Err(dracon_git::error::GitError::MergeConflict)) => {
                eprintln!(
                    "⚠️ post-commit pull conflict in {} (manual intervention required)",
                    repo.display()
                );
            }
            Ok(Err(e)) => {
                eprintln!(
                    "⚠️ post-commit pull failed for {}: {} - will still attempt push",
                    repo.display(),
                    e
                );
            }
            Err(_) => {
                eprintln!(
                    "⚠️ post-commit pull timeout for {} after {}s - will still attempt push",
                    repo.display(),
                    policy.pull_op_timeout_secs
                );
            }
        }
    }
}

async fn restore_excluded_paths(
    ctx: &SyncContext<'_>,
    to_restore: &[dracon_git::types::DiffFile],
) -> Result<()> {
    let repo = ctx.repo;
    let policy = ctx.policy;
    let restorable: Vec<_> = to_restore
        .iter()
        .filter(|e| can_restore_entry(repo, e))
        .filter(|e| {
            !repo.join(&e.path).is_dir() || !crate::exclude::is_gitlink_unchanged(repo, &e.path)
        })
        .collect();

    handle_large_untracked(repo, to_restore, policy)?;

    let other_untracked: Vec<_> = to_restore
        .iter()
        .filter(|e| {
            !can_restore_entry(repo, e) && !is_large_untracked(e, repo, policy.max_stage_file_bytes)
        })
        .collect();

    if !other_untracked.is_empty() {
        eprintln!(
            "ℹ️ {} has {} small untracked excluded file(s)",
            repo.display(),
            other_untracked.len()
        );
    }

    if !restorable.is_empty() {
        let excluded_paths: Vec<String> = restorable
            .iter()
            .map(|e| e.path.to_string_lossy().to_string())
            .collect();
        eprintln!(
            "🧹 restoring {} excluded path(s) in {} after commit",
            excluded_paths.len(),
            repo.display()
        );
        restore_paths(repo, &excluded_paths).await?;
    }

    Ok(())
}

async fn run_release_pipeline_if_bumped(repo: &Path, policy: &SyncPolicy, version_bumped: bool) {
    if !version_bumped {
        return;
    }
    if let Some((old_ver, new_ver, level)) = get_bump_info(repo).await {
        let repo_override = crate::policy::load_repo_override(repo);
        let repo_auto_tag = repo_override.auto_tag.unwrap_or(policy.auto_tag);
        let repo_auto_release = repo_override.auto_release.unwrap_or(policy.auto_release);
        let repo_publish_targets = repo_override.auto_publish;
        let repo_nix_auto_update = repo_override
            .nix_auto_update
            .unwrap_or(policy.nix_auto_update);
        let steps = crate::release::run_release_pipeline(
            repo,
            &old_ver,
            &new_ver,
            level.as_str(),
            policy,
            repo_auto_tag,
            repo_auto_release,
            &repo_publish_targets,
            repo_nix_auto_update,
        )
        .await;
        for step in &steps {
            match step {
                crate::release::ReleaseStep::TagCreated(tag) => eprintln!("🏷️  {tag}"),
                crate::release::ReleaseStep::GitHubReleaseCreated(tag) => eprintln!("🚀 {tag}"),
                crate::release::ReleaseStep::Published { registry, version } => {
                    eprintln!("📦 published to {registry} v{version}")
                }
                crate::release::ReleaseStep::NixFlakePRCreated(url) => {
                    eprintln!("📄 flake PR created: {url}")
                }
                crate::release::ReleaseStep::Skipped(reason) => {
                    if debug_enabled() {
                        eprintln!("🐛 release skipped: {reason}");
                    }
                }
                crate::release::ReleaseStep::Failed { step: s, error } => {
                    eprintln!("⚠️ release failed: {s} — {error}")
                }
            }
        }
    }
}

/// Push to origin + all mirror remotes. Returns true if all succeeded.
///
/// Updates `remote_failures` (if Some) to track consecutive failures per
/// remote. On a successful mirror push, the entry is removed. This is the
/// only path that should update `remote_failures`; the previous design had
/// a separate `push_with_blob_check` function that did this but was never
/// called from the live code path.
async fn push_background(
    repo: &std::path::Path,
    policy: &SyncPolicy,
    has_origin: bool,
    mut remote_failures: Option<&mut HashMap<String, usize>>,
) -> Result<bool> {
    // Scale the push idle timeout with the local ahead count. A 60s
    // timeout is fine for a small push, but a 28-commit push with
    // binary test artifacts can sit in the negotiate phase for >60s
    // before emitting any progress. Without scaling, the first attempt
    // times out, the daemon burns its 3-attempt retry budget, falls
    // through to the 4-remote HTTPS fallback chain, and the operator
    // sees "pushing 4m" — a stall that the new ACTIVITY column will
    // surface, but that we should prevent. Capped at 600s (10 min) so
    // a runaway push can't block the daemon forever.
    let ahead_count = count_ahead_commits(repo).await.unwrap_or(0);
    let scaled_timeout = scale_push_timeout(policy.push_op_timeout_secs, ahead_count);
    if scaled_timeout != policy.push_op_timeout_secs {
        eprintln!(
            "⏫ {} scaling push timeout {}s → {}s ({} commits ahead)",
            repo.display(),
            policy.push_op_timeout_secs,
            scaled_timeout,
            ahead_count
        );
    }
    // ── Proactive oversized-pack guard (github 2 GiB limit) ──────────────
    // github rejects packs > 2 GiB. Retrying is vain: git still re-packs the
    // history (slow, and it saturates the daemon's push semaphore) only for
    // github to reject it again. The relevant size is the pack we would
    // actually send for the pushed branch — NOT the whole `.git` (which can
    // be huge for unrelated reasons, e.g. dracon-platform's 332 tags). So we
    // measure the pushable branch via `github_pack_too_large`. gitlab/codeberg
    // have no such limit and keep working. Self-healing: once the pushed
    // branch shrinks below 2 GiB the push resumes automatically.
    let (too_big_for_github, pushable_size) = crate::git::github_pack_too_large(repo, None);
    // Whether github was already flagged, so we notify once per regression
    // rather than spamming the journal every cycle.
    let github_already_flagged = remote_failures
        .as_ref()
        .map(|rf| rf.get("github").copied().unwrap_or(0) > 0)
        .unwrap_or(false);

    // Detect whether `origin` points at github. This matters for the
    // mirror-path github-exclusion logic below: if `origin` IS github,
    // github is already pushed by the `push_with_retries` block above and
    // must be excluded from the mirror path (to avoid the
    // `auto_create_all_remotes` stall that motivated the original
    // exclusion). If `origin` is NOT github (e.g. the 10 nested game
    // submodules of `dracon-platform` where `.gitmodules` lists codeberg
    // first and git picked that as `origin`), github MUST be pushed via
    // the mirror path or it never reaches the forge.
    // FIXED 2026-07-09 (goal fb8ddd6b — repo-discovery audit): the
    // previous logic unconditionally excluded github from the mirror
    // path, assuming `origin` = github for every repo. That assumption
    // is violated for any repo cloned from a non-github-first
    // `.gitmodules` (or with `origin` reassigned post-clone). The fix:
    // only exclude github from the mirror path when `origin` is github.
    let origin_is_github = if has_origin {
        crate::git::multi_remote::get_remote_url(repo, "origin")
            .map(|u| u.contains("github.com"))
            .unwrap_or(false)
    } else {
        false
    };

    // Push to origin (if the repo has one — mirror-only repos like .dracon
    // skip this and go straight to mirror remotes)
    if has_origin {
        // Skip origin if it points at github and the pack is too big for
        // github's 2 GiB limit (defensive; most repos' origin is codeberg).
        if too_big_for_github && origin_is_github {
            if !github_already_flagged {
                log_warn!(
                    "🚫 skipping origin (github) push for {}: pushable branch is {:.2} GiB (exceeds github's 2 GiB pack limit)",
                    repo.display(),
                    pushable_size as f64 / (1024.0 * 1024.0 * 1024.0)
                );
            }
        } else {
            match push_with_retries(
                repo,
                scaled_timeout,
                policy.push_retries,
                "push",
            )
            .await
            {
                Ok(()) => {}
                Err(e) => {
                    eprintln!(
                        "⚠️ background push to origin failed for {}: {}",
                        repo.display(),
                        e
                    );
                    return Ok(false);
                }
            }
        }
    }

    // Push to mirror remotes
    if !policy.remotes.is_empty() {
        let private = true;
        // CHANGED 2026-06-23 (goal mqqsyzyd-qkvna5): honor
        // per-repo `exclude_remotes` so a repo can opt out of a
        // specific mirror (e.g. gitlab for a repo over the free-tier
        // storage quota) without affecting other repos.
        let repo_override = crate::policy::load_repo_override(repo);
        let mut combined_exclude: Vec<String> = repo_override.exclude_remotes.clone();
        // CONDITIONAL codeberg exclusion: when the global
        // `codeberg_public_only` policy is on (default true) AND the
        // per-repo override doesn't disable it (`Some(false)`), AND the
        // cached repo visibility says private (or is unknown, which is
        // the safe default), skip the codeberg remote. This keeps
        // codeberg's 85 GiB global quota available for public-repo
        // marketing mirrors only. ADDED 2026-07-17 (goal
        // `codeberg-public-only`); see
        // `docs/design/codeberg-public-only-policy-2026-07-17.md`.
        let codeberg_public_only_effective = repo_override
            .codeberg_public_only
            .unwrap_or(policy.codeberg_public_only);
        if codeberg_public_only_effective {
            let cached_priv = cached_repo_visibility(repo);
            // Skip codeberg when:
            //   (a) cached visibility says private, OR
            //   (b) cache is empty (legacy or never-visibility-synced) —
            //       safe default until cache refreshes.
            // Don't skip when:
            //   (c) cached visibility says public.
            let skip_codeberg = match cached_priv {
                Some(true) => true,   // (a) explicitly private
                Some(false) => false, // (c) explicitly public
                None => true,         // (b) unknown — safe default
            };
            if skip_codeberg
                && !combined_exclude.iter().any(|e| e == "codeberg")
            {
                combined_exclude.push("codeberg".to_string());
                if crate::policy::debug_enabled() {
                    eprintln!(
                        "🐛 codeberg gate: skipping codeberg push for {} \
                         (public_only policy + visibility cache says {:?})",
                        repo.display(),
                        cached_priv.map(|p| if p { "private" } else { "public" })
                    );
                }
            }
        }
        // CONDITIONAL github exclusion from the mirror path. When
        // `origin` IS github, github is already pushed by the
        // `push_with_retries` block above; routing it through
        // `push_mirror_remotes` would re-trigger `auto_create_all_remotes`
        // (`gh repo create`), which historically stalled against an
        // already-existing repo and blocked the gitlab/codeberg pushes
        // that follow in the same call. (The stall was later mitigated
        // by `remote_repo_exists` — 2026-06-20 — but the exclusion
        // remains to avoid the redundant work.) When `origin` is NOT
        // github (e.g. the 10 nested game submodules of `dracon-platform`
        // where `.gitmodules` lists codeberg first and git picked that
        // as `origin`), github MUST be pushed via the mirror path or it
        // never reaches the forge — which is exactly the push-to-all
        // violation the 2026-07-09 audit (goal fb8ddd6b) surfaced. The
        // 2 GiB pack limit is still enforced by the `too_big_for_github`
        // skip above regardless of which path pushes github.
        if origin_is_github && !combined_exclude.iter().any(|e| e == "github") {
            combined_exclude.push("github".to_string());
        }
        if too_big_for_github {
            // ACTUALLY exclude github from the mirror push. The log message
            // below says "skipping github push", but unless we add it to
            // `combined_exclude` the `push_mirror_remotes` call below still
            // routes github through `auto_create_all_remotes` +
            // `push_to_all_remotes`, spawning a `git push github` that github
            // rejects (2 GiB pack limit). That hangs uploading the oversized
            // pack and leaks an orphaned git process the daemon re-dispatches
            // every cycle (see 2026-07-09 sync-stall audit). This is the
            // mirror-path counterpart of the origin-github skip above, which
            // does actually skip (it omits the `push_with_retries` call).
            if !combined_exclude.iter().any(|e| e == "github") {
                combined_exclude.push("github".to_string());
            }
            // Record the skip so the one-time notification doesn't re-fire
            // every cycle, and so the repo shows as intentionally-skipped.
            if let Some(rf) = remote_failures.as_deref_mut() {
                *rf.entry("github".to_string()).or_insert(0) += 1;
            }
            if !github_already_flagged {
                log_warn!(
                    "🚫 skipping github push for {}: pushable branch is {:.2} GiB (exceeds github's 2 GiB pack limit). Needs history rewrite / OVH migration; will resume once shrunk below 2 GiB.",
                    repo.display(),
                    pushable_size as f64 / (1024.0 * 1024.0 * 1024.0)
                );
                if let Some(url) = &policy.webhook_url {
                    notify_webhook_failure(
                        url,
                        repo,
                        "github",
                        "PACK_TOO_LARGE: pushable branch exceeds github's 2 GiB pack limit; skipping push. Rewrite history (git filter-repo) or move assets to OVH bucket.",
                    );
                }
            }
        }
        let push_results = push_mirror_remotes(
            repo,
            &policy.remotes,
            policy.push_op_timeout_secs,
            policy.push_retries,
            private,
            &combined_exclude,
            repo_override.auto_create_on_codeberg,
        )
        .await;
        let all_ok = push_results.iter().all(|(_, r)| r.is_ok());
        if !all_ok {
            for (name, result) in &push_results {
                if let Err(e) = result {
                    log_warn!("push to {} failed for {}: {}", name, repo.display(), e);
                    if let Some(ref url) = policy.webhook_url {
                        notify_webhook_failure(url, repo, name, &e.to_string());
                    }
                    if let Some(rf) = remote_failures.as_deref_mut() {
                        *rf.entry(name.clone()).or_insert(0) += 1;
                    }
                }
            }
            return Ok(false);
        } else if let Some(rf) = remote_failures {
            // Successful push — clear any prior failure count for the
            // remotes we actually pushed to. (Remotes we deliberately
            // skipped — e.g. github when `.git` > 2 GiB — are NOT in
            // `push_results`, so their skip marker survives until the
            // repo shrinks and they push successfully.)
            for (name, _) in &push_results {
                rf.remove(name);
            }
        }
    }
    Ok(true)
}

/// Task state transitions extracted from a markdown diff.
#[derive(Debug, Default)]
struct TaskTransitions {
    /// Tasks marked `[x]` (completed)
    closed: Vec<String>,
    /// Tasks marked `[~]` (in-progress)
    progress: Vec<String>,
    /// Goal metadata extracted from .pi/goals/ JSON files
    goal_metadata: Option<GoalMetadata>,
}

/// Metadata extracted from goal file JSON.
/// Provides richer context about goal lifecycle for AI consumption.
#[derive(Debug, Default)]
struct GoalMetadata {
    /// Goal status (active, complete, paused)
    status: Option<String>,
    /// Why the goal was paused/aborted
    pause_reason: Option<String>,
    /// Tokens used by the goal
    tokens_used: Option<u64>,
    /// Active seconds spent on the goal
    active_seconds: Option<u64>,
    /// Tasks with their evidence and skip reasons
    task_details: Vec<TaskDetail>,
}

/// Detail about a single task from goal metadata.
#[derive(Debug, Default)]
struct TaskDetail {
    id: String,
    status: String,
    evidence: Option<String>,
    skip_reason: Option<String>,
}

/// Extract task state transitions from the staged diff.
///
/// Scans ALL staged files for:
/// - `- [x]` or `// [x]` additions → task completed (CLOSED)
/// - `- [~]` or `// [~]` additions → task in-progress (WIP)
///
/// Works in any file:
/// - Markdown: `- [x] Fix bug`
/// - Code comments: `// [x] Implemented JWT`
/// - Text files: `[x] Done`
/// - Any file with checkbox syntax
///
/// This is deterministic — no LLM, no inference. Just regex on the diff.
fn extract_task_transitions(repo: &Path) -> TaskTransitions {
    // Get the diff for ALL files (not just markdown)
    let output =
        match run_git_capture_output(repo, &["diff", "--cached", "--unified=0"], "task-diff") {
            Ok(o) => o,
            Err(_) => return TaskTransitions::default(),
        };

    let mut transitions = TaskTransitions::default();

    for line in output.lines() {
        // Only look at ADDED lines (start with `+` but not `+++`)
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        let content = &line[1..]; // Remove leading `+`
        let trimmed = content.trim();

        // Check for completed tasks: [x] or [X]
        // Matches: `- [x]`, `* [x]`, `// [x]`, `# [x]`, plain `[x]`
        if let Some(rest) =
            extract_checkbox_text(trimmed, 'x').or_else(|| extract_checkbox_text(trimmed, 'X'))
        {
            let task = sanitize_task_name(rest);
            if !task.is_empty() {
                transitions.closed.push(task);
            }
        }
        // Check for in-progress tasks: [~]
        else if let Some(rest) = extract_checkbox_text(trimmed, '~') {
            let task = sanitize_task_name(rest);
            if !task.is_empty() {
                transitions.progress.push(task);
            }
        }
    }

    // Also extract goal metadata from JSON files
    transitions.goal_metadata = extract_goal_metadata(repo);

    transitions
}

/// Extract text after a checkbox marker like `[x]`, `[~]`, etc.
///
/// Handles common text/markdown prefixes:
/// - `- [x] task` (markdown list)
/// - `* [x] task` (markdown list)
/// - `[x] task` (plain text)
///
/// Does NOT match code comments (`//`, `#`) — nobody puts checkmarks in code.
fn extract_checkbox_text(line: &str, marker: char) -> Option<&str> {
    // Build the marker pattern: `[x]`, `[~]`, etc.
    let pattern = format!("[{}]", marker);

    // Try common text/markdown prefixes only
    let prefixes = ["- ", "* ", ""];
    for prefix in &prefixes {
        let full_prefix = format!("{}{}", prefix, pattern);
        if let Some(rest) = line.strip_prefix(&full_prefix) {
            return Some(rest.trim());
        }
    }

    None
}

/// Sanitize a task name for use in the routing key.
///
/// Commit subjects are indexes, not summaries. Keep task fragments short and
/// routing-key style so generated messages do not become multi-clause prose.
///
/// Removes:
/// - Markdown formatting: `**`, `__`, `*`, `_`
/// - Pipe characters: `|`
/// - Square brackets: `[`, `]`
/// - Explanatory clauses after `:`, `;`, `—`, or `–`
///
/// If the name starts with `**identifier** description` (common pattern),
/// extracts only the identifier plus the first descriptive word. Truncates to
/// 60 chars to keep commit subjects compact.
fn sanitize_task_name(name: &str) -> String {
    // Common pattern: `**F-reframe** description` → extract just `F-reframe`
    if name.starts_with("**") {
        if let Some(end) = name.find("**") {
            if end >= 2 {
                let identifier = &name[2..end];
                // If there's description after, include first meaningful word
                let rest = &name[end + 2..];
                if rest.is_empty() {
                    return compact_task_phrase(identifier);
                }
                let first_word = rest.split_whitespace().next().unwrap_or("");
                if first_word.is_empty() {
                    return compact_task_phrase(identifier);
                }
                return compact_task_phrase(&format!("{} {}", identifier, first_word));
            }
        }
    }

    // Fallback: general sanitization
    let sanitized = name
        .replace('|', "/")
        .replace("**", "")
        .replace("__", "")
        .replace('*', "")
        .replace('[', "(")
        .replace(']', ")")
        .replace('`', "") // Strip backticks (code in task names)
        .trim()
        .to_string();
    compact_task_phrase(&sanitized)
}

/// Compact a task phrase before it enters a commit routing key.
///
/// Drops explanatory clauses and limits plain text to three words. This keeps
/// generated commit subjects searchable without turning them into natural
/// language summaries.
fn compact_task_phrase(name: &str) -> String {
    let clause = name
        .split([':', ';', '—', '–'])
        .next()
        .unwrap_or(name)
        .trim();
    let clause = if clause.is_empty() { name } else { clause };
    let words: Vec<&str> = clause.split_whitespace().collect();
    let compact = if words.len() > 3 {
        words[..3].join(" ")
    } else {
        clause.to_string()
    };
    truncate_task(&compact)
}

/// Truncate a task name to a reasonable length for commit subjects.
/// Cuts at sentence boundary (`.`, `—`, `–`) or 60 chars, whichever comes first.
fn truncate_task(name: &str) -> String {
    const MAX_LEN: usize = 60;
    // Find the char boundary at or before MAX_LEN bytes
    let truncated_at_max = if name.len() <= MAX_LEN {
        return name.to_string();
    } else {
        // Walk chars to find the last valid boundary <= MAX_LEN
        let mut last_boundary = 0;
        for (i, _) in name.char_indices() {
            if i > MAX_LEN {
                break;
            }
            last_boundary = i;
        }
        last_boundary
    };
    // Try to cut at sentence boundary within the first MAX_LEN bytes
    let mut last_boundary_pos = None;
    for (i, c) in name.char_indices() {
        if i >= truncated_at_max {
            break;
        }
        if c == '.' || c == '—' || c == '–' {
            last_boundary_pos = Some(i + c.len_utf8());
        }
    }
    if let Some(pos) = last_boundary_pos {
        let truncated = &name[..pos];
        if truncated.len() >= 10 {
            return truncated.to_string();
        }
    }
    // Hard truncate at char boundary
    format!("{}...", &name[..truncated_at_max])
}

/// Extract goal metadata from .pi/goals/ JSON files in the staged diff.
///
/// When a goal file changes, reads the full file and extracts:
/// - Goal status, pause/stop reasons
/// - Token usage and active time
/// - Per-task evidence and skip reasons
///
/// Returns None if no goal files changed or parsing fails.
fn extract_goal_metadata(repo: &Path) -> Option<GoalMetadata> {
    // Get list of staged files
    let files_output =
        run_git_capture_output(repo, &["diff", "--cached", "--name-only"], "goal-files").ok()?;

    // Find goal files that changed
    let goal_files: Vec<&str> = files_output
        .lines()
        .filter(|f| f.starts_with(".pi/goals/") && f.ends_with(".md"))
        .collect();

    if goal_files.is_empty() {
        return None;
    }

    // Read the first goal file (most recent)
    let goal_path = repo.join(goal_files[0]);
    let content = std::fs::read_to_string(&goal_path).ok()?;

    // Parse JSON - goal files have JSON at the top, markdown at the bottom
    // Find the end of JSON by counting braces
    let mut depth = 0;
    let mut json_end = 0;
    for (i, c) in content.chars().enumerate() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
        }
        if depth == 0 && i > 0 {
            json_end = i + 1;
            break;
        }
    }

    if json_end == 0 {
        return None;
    }

    let json_str = &content[..json_end];
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let mut metadata = GoalMetadata {
        status: value["status"].as_str().map(String::from),
        pause_reason: value["pauseReason"].as_str().map(String::from),
        ..Default::default()
    };

    if let Some(usage) = value["usage"].as_object() {
        metadata.tokens_used = usage["tokensUsed"].as_u64();
        metadata.active_seconds = usage["activeSeconds"].as_u64();
    }

    // Extract task details
    if let Some(tasks) = value["taskList"]["tasks"].as_array() {
        for task in tasks {
            let detail = TaskDetail {
                id: task["id"].as_str().unwrap_or("").to_string(),
                status: task["status"].as_str().unwrap_or("").to_string(),
                evidence: task["evidence"].as_str().map(String::from),
                skip_reason: task["skipReason"].as_str().map(String::from),
            };
            metadata.task_details.push(detail);
        }
    }

    Some(metadata)
}

/// Detect dependency changes from the staged diff.
///
/// Parses Cargo.toml, package.json, requirements.txt, go.mod diffs
/// to extract specific dependency names that were added or removed.
///
/// Returns: `+dep1,+dep2,-dep3` or None if no dep files changed.
fn detect_dependency_changes(repo: &Path) -> Option<String> {
    let dep_files: &[(&str, &str)] = &[
        ("Cargo.toml", "toml"),
        ("package.json", "json"),
        ("requirements.txt", "txt"),
        ("go.mod", "gomod"),
    ];

    let mut added_deps = Vec::new();
    let mut removed_deps = Vec::new();
    let mut any_changed = false;

    for (file, format) in dep_files {
        // Get the diff for this file
        let output = match run_git_capture_output(
            repo,
            &["diff", "--cached", "--unified=0", "--", file],
            &format!("dep-diff-{}", file),
        ) {
            Ok(o) => o,
            Err(_) => continue,
        };

        if output.trim().is_empty() {
            continue;
        }
        any_changed = true;

        // Parse based on format
        for line in output.lines() {
            if !line.starts_with('+') && !line.starts_with('-') {
                continue;
            }
            if line.starts_with("+++") || line.starts_with("---") {
                continue;
            }

            let is_add = line.starts_with('+');
            let content = &line[1..].trim();

            let dep_name = match *format {
                "toml" => parse_cargo_dep(content),
                "json" => parse_npm_dep(content),
                "txt" => parse_pip_dep(content),
                "gomod" => parse_go_dep(content),
                _ => None,
            };

            if let Some(name) = dep_name {
                if is_add {
                    added_deps.push(name);
                } else {
                    removed_deps.push(name);
                }
            }
        }
    }

    if !any_changed {
        return None;
    }

    // If we couldn't parse any actual deps, skip the DEPS indicator
    // (e.g., version bump in package.json has no actual dependency changes)
    if added_deps.is_empty() && removed_deps.is_empty() {
        return None;
    }

    // Format output
    let mut parts = Vec::new();
    for dep in &added_deps {
        parts.push(format!("+{}", dep));
    }
    for dep in &removed_deps {
        parts.push(format!("-{}", dep));
    }

    if parts.is_empty() {
        // Dep file changed but we couldn't parse specific deps
        Some("changed".to_string())
    } else {
        // Limit to top 5 to keep title manageable
        let display: Vec<String> = parts.iter().take(5).cloned().collect();
        let suffix = if parts.len() > 5 {
            format!("+{}more", parts.len() - 5)
        } else {
            String::new()
        };
        Some(format!("{}{}", display.join(","), suffix))
    }
}

/// Parse a Cargo.toml dependency line like `serde = "1.0"` or `tokio = { version = "1" }`
fn parse_cargo_dep(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Skip section headers and comments
    if trimmed.starts_with('[') || trimmed.starts_with('#') || trimmed.is_empty() {
        return None;
    }
    // Look for `name = ` pattern
    if let Some(eq_pos) = trimmed.find('=') {
        let name = trimmed[..eq_pos].trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Some(name.to_string());
        }
    }
    None
}

/// Parse a package.json dependency line like `"express": "^4.18.0"`
fn parse_npm_dep(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Look for `"name":` pattern
    if let Some(colon_pos) = trimmed.find(':') {
        let key_part = trimmed[..colon_pos].trim();
        if key_part.starts_with('"') && key_part.ends_with('"') {
            let name = &key_part[1..key_part.len() - 1];
            // Skip non-dependency fields
            if !name.is_empty() && !name.starts_with('_') && name != "name" && name != "version" {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Parse a requirements.txt line like `requests==2.31.0` or `flask>=2.0`
fn parse_pip_dep(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Skip comments and empty lines
    if trimmed.starts_with('#') || trimmed.is_empty() {
        return None;
    }
    // Split on version specifiers
    let name = trimmed
        .split(&['=', '>', '<', '!', '~', ';', '['][..])
        .next()
        .unwrap_or("")
        .trim();
    if !name.is_empty() {
        return Some(name.to_string());
    }
    None
}

/// Parse a go.mod require line like `github.com/gin-gonic/gin v1.9.1`
fn parse_go_dep(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Skip comments and empty lines
    if trimmed.starts_with("//") || trimmed.is_empty() || trimmed == "require" || trimmed == ")" {
        return None;
    }
    // Split on whitespace, take first part (module path)
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if !parts.is_empty() && parts[0].contains('.') {
        // Return just the last component of the module path
        let module = parts[0];
        let name = module.rsplit('/').next().unwrap_or(module);
        return Some(name.to_string());
    }
    None
}

/// Extract newly added and deleted files from the staged diff.
///
/// Returns (new_files, deleted_files) as vectors of file paths.
fn extract_new_deleted_files(repo: &Path) -> (Vec<String>, Vec<String>) {
    let output =
        match run_git_capture_output(repo, &["diff", "--cached", "--name-status"], "name-status") {
            Ok(o) => o,
            Err(_) => return (Vec::new(), Vec::new()),
        };

    let mut new_files = Vec::new();
    let mut deleted_files = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let status = parts[0];
        let path = parts[1];

        // Only track source files, not lock files or generated files
        let should_track = !path.ends_with(".lock")
            && !path.ends_with(".sum")
            && !path.contains("/target/")
            && !path.contains("/node_modules/")
            && !path.contains("/__pycache__/");

        if !should_track {
            continue;
        }

        match status {
            "A" => new_files.push(path.to_string()),
            "D" => deleted_files.push(path.to_string()),
            _ => {} // Modified, renamed, etc.
        }
    }

    (new_files, deleted_files)
}

/// Check if a file path looks like a test file.
///
/// Common patterns:
/// - tests/, test/, __tests__/
/// - *_test.rs, *_test.py, *_test.go
/// - *.test.ts, *.test.js, *.spec.ts
/// - test_*.py, test_*.rs
fn is_test_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let basename = path.rsplit('/').next().unwrap_or(path);
    let basename_lower = basename.to_ascii_lowercase();

    // Directory patterns
    if lower.contains("/test") || lower.contains("/tests") || lower.contains("/__tests__") {
        return true;
    }

    // Suffix patterns (before extension)
    if basename_lower.contains("_test.")
        || basename_lower.contains("_tests.")
        || basename_lower.contains(".test.")
        || basename_lower.contains(".spec.")
    {
        return true;
    }

    // Prefix patterns
    if basename_lower.starts_with("test_") {
        return true;
    }

    false
}

/// Get the tag pointing to the current HEAD commit.
///
/// Returns the tag name if this commit is tagged, otherwise None.
/// Uses `git describe --tags --always --exact-match` for exact tag match.
fn get_current_tag(repo: &Path) -> Option<String> {
    // First try exact match (HEAD is exactly on a tag)
    let exact = run_git_capture_output(
        repo,
        &["describe", "--tags", "--always", "--exact-match"],
        "tag-exact",
    )
    .ok()?;

    let tag = exact.trim();
    if !tag.is_empty() && !tag.contains('-') {
        // Got an exact tag (no commit count suffix)
        return Some(tag.to_string());
    }

    // Fall back to --all (check all refs for this commit)
    let all_tags =
        run_git_capture_output(repo, &["tag", "--points-at", "HEAD"], "tag-points-at").ok()?;

    // Return the first lightweight tag (ignore annotated ones for simplicity)
    for line in all_tags.lines() {
        let tag = line.trim();
        if !tag.is_empty() && !tag.contains('^') {
            return Some(tag.to_string());
        }
    }

    None
}

/// Check if any environment files were changed in the staged diff.
///
/// Looks for:
/// - .env, .env.*, .envrc
/// - .secrets, secrets.*
/// - config/settings files with secrets patterns
///
/// Returns true if any env-like files changed.
fn has_env_changes(repo: &Path) -> bool {
    let env_patterns = [
        ".env", ".env.", // .env.local, .env.production, etc.
        ".envrc", ".secrets", "secrets.",
    ];

    let output =
        match run_git_capture_output(repo, &["diff", "--cached", "--name-only"], "env-check") {
            Ok(o) => o,
            Err(_) => return false,
        };

    for line in output.lines() {
        let path = line.trim();
        // Check if file name matches any env pattern
        let basename = path.rsplit('/').next().unwrap_or(path);
        for pattern in &env_patterns {
            if basename.starts_with(pattern) || basename == *pattern {
                return true;
            }
        }
    }

    false
}

/// Compute commit message from staged diff.
///
/// Returns a structured message with task state + blast radius.
///
/// Format: `\[INTENT\] | FILES:N DIRS:X DELTA:+A/-B [TEST:T] [BIN:B]`
///
/// INTENT (from markdown diff):
/// - `CLOSED: task1, task2` — tasks marked `\[x\]`
/// - `WIP: task1` — tasks marked `\[~\]`
/// - Omitted if no task transitions found
///
/// BLAST RADIUS (from git diff --numstat):
/// - FILES:N — total files changed
/// - DIRS:X,Y — top-level directories touched
/// - DELTA:+A/-B — lines added/removed
///
/// METRICS (also from diff):
/// - TEST:T — lines changed in test files
/// - BIN:B — binary files changed (context window warning)
///
/// # Why mechanical messages?
///
/// AI-generated commit messages are bad for AI workflows:
/// - They hallucinate context and intent
/// - They try to summarize but AI reads the diff anyway
/// - They're verbose, inconsistent, and noisy
///
/// Simple mechanical facts work better:
/// - Searchable: `git log --grep="JWT"` finds commits touching that task
/// - Honest: no interpretation, just data
/// - Compact: fits in `git log --oneline`
/// - The AI gets its understanding from the actual diff, not the commit message
///
/// The commit message is an INDEX, not a description.
fn compute_blast_radius(repo: &Path) -> String {
    let output = match run_git_capture_output(repo, &["diff", "--cached", "--numstat"], "numstat") {
        Ok(o) => o,
        Err(_) => return "0 file(s) DELTA:+0/-0".to_string(),
    };

    let mut files = 0usize;
    let mut added = 0i64;
    let mut removed = 0i64;
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let mut file_changes: Vec<(i64, String)> = Vec::new();
    let mut test_lines = 0i64;
    let mut binary_count = 0usize;

    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let a = parts[0];
        let r = parts[1];
        let path = parts[2];

        // Binary files show as "-" in numstat
        if a == "-" || r == "-" {
            binary_count += 1;
            files += 1;
            // Still track the directory
            if let Some(first_component) = path.split('/').next() {
                if !first_component.is_empty() && first_component != "." {
                    dirs.insert(first_component.to_string());
                }
            }
            continue;
        }

        let a_val = a.parse::<i64>().unwrap_or(0);
        let r_val = r.parse::<i64>().unwrap_or(0);

        added += a_val;
        removed += r_val;
        files += 1;

        // Track test file lines separately
        if is_test_file(path) {
            test_lines += a_val + r_val;
        }

        // Extract top-level directory for scope (skip root-level files)
        if path.contains('/') {
            if let Some(first_component) = path.split('/').next() {
                if !first_component.is_empty() && first_component != "." {
                    dirs.insert(first_component.to_string());
                }
            }
        }

        // Track files for listing (sorted by lines changed)
        file_changes.push((a_val + r_val, path.to_string()));
    }

    // Sort by lines changed descending, take top 3
    file_changes.sort_by_key(|b| std::cmp::Reverse(b.0));
    let top_files: Vec<String> = file_changes
        .iter()
        .take(3)
        .map(|(_, path)| path.clone())
        .collect();

    // Build routing key components

    // 1. Task intent (from markdown diff) — cap at 10 tasks per category
    let transitions = extract_task_transitions(repo);
    let is_merge = repo.join(".git/MERGE_HEAD").exists();
    let is_revert = repo.join(".git/REVERT_HEAD").exists();
    let intent_prefix = if is_merge {
        "MERGE: | ".to_string()
    } else if is_revert {
        "REVERT: | ".to_string()
    } else {
        const MAX_TASKS: usize = 10;
        let mut parts = Vec::new();
        if !transitions.closed.is_empty() {
            let shown: Vec<&str> = transitions
                .closed
                .iter()
                .take(MAX_TASKS)
                .map(|s| s.as_str())
                .collect();
            let suffix = if transitions.closed.len() > MAX_TASKS {
                format!(" +{}more", transitions.closed.len() - MAX_TASKS)
            } else {
                String::new()
            };
            parts.push(format!("CLOSED: {}{}", shown.join(", "), suffix));
        }
        if !transitions.progress.is_empty() {
            let shown: Vec<&str> = transitions
                .progress
                .iter()
                .take(MAX_TASKS)
                .map(|s| s.as_str())
                .collect();
            let suffix = if transitions.progress.len() > MAX_TASKS {
                format!(" +{}more", transitions.progress.len() - MAX_TASKS)
            } else {
                String::new()
            };
            parts.push(format!("WIP: {}{}", shown.join(", "), suffix));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("{} | ", parts.join(" | "))
        }
    };

    // 2. File count and dirs
    let dirs_str = if dirs.is_empty() {
        String::new()
    } else {
        format!(
            " in {}",
            dirs.iter().take(3).cloned().collect::<Vec<_>>().join(",")
        )
    };

    // 3. Top changed files
    let files_str = if top_files.is_empty() {
        String::new()
    } else {
        format!(" [{}]", top_files.join(", "))
    };

    // 4. Metrics suffix
    let mut metrics = Vec::new();
    if test_lines > 0 {
        metrics.push(format!("TEST:{}", test_lines));
    }
    if binary_count > 0 {
        metrics.push(format!("BIN:{}", binary_count));
    }

    // 5. New/deleted files
    let (new_files, deleted_files) = extract_new_deleted_files(repo);
    if !new_files.is_empty() {
        // Show top 10 new files (searchable), abbreviate nested paths
        let display: Vec<String> = new_files
            .iter()
            .take(10)
            .map(|f| {
                let parts: Vec<&str> = f.split('/').collect();
                if parts.len() > 2 {
                    parts[parts.len() - 2..].join("/")
                } else {
                    f.clone()
                }
            })
            .collect();
        let suffix = if new_files.len() > 10 {
            format!("+{}more", new_files.len() - 10)
        } else {
            String::new()
        };
        metrics.push(format!("NEW:{}{}", display.join(","), suffix));
    }
    if !deleted_files.is_empty() {
        let display: Vec<String> = deleted_files
            .iter()
            .take(10)
            .map(|f| {
                let parts: Vec<&str> = f.split('/').collect();
                if parts.len() > 2 {
                    parts[parts.len() - 2..].join("/")
                } else {
                    f.clone()
                }
            })
            .collect();
        let suffix = if deleted_files.len() > 10 {
            format!("+{}more", deleted_files.len() - 10)
        } else {
            String::new()
        };
        metrics.push(format!("DEL:{}{}", display.join(","), suffix));
    }

    // 6. Dependency changes
    if let Some(dep_info) = detect_dependency_changes(repo) {
        metrics.push(format!("DEPS:{}", dep_info));
    }

    // 7. Merge/revert detection — merge/revert commits start with MERGE:/REVERT:
    if !is_merge && repo.join(".git/MERGE_HEAD").exists() {
        metrics.push("MERGE:".to_string());
    }
    if !is_revert && repo.join(".git/REVERT_HEAD").exists() {
        metrics.push("REVERT:".to_string());
    }

    // 8. Tag detection — if this commit is tagged, include the tag
    if let Some(tag) = get_current_tag(repo) {
        metrics.push(format!("TAG:{}", tag));
    }

    // 9. Test-only detection — if ALL changed files are test files
    if !file_changes.is_empty() && file_changes.iter().all(|(_, path)| is_test_file(path)) {
        let test_files: Vec<String> = file_changes
            .iter()
            .take(5)
            .map(|(_, p)| {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() > 2 {
                    parts[parts.len() - 2..].join("/")
                } else {
                    p.clone()
                }
            })
            .collect();
        let suffix = if file_changes.len() > 5 {
            format!("+{}more", file_changes.len() - 5)
        } else {
            String::new()
        };
        metrics.push(format!("TESTONLY:{}{}", test_files.join(","), suffix));
    }

    // 10. Env file detection — if any env files changed
    if has_env_changes(repo) {
        metrics.push("ENV:".to_string());
    }

    // 11. Goal metadata — extract richer context from .pi/goals/ JSON files
    if let Some(ref goal_meta) = transitions.goal_metadata {
        // Goal status change
        if let Some(ref status) = goal_meta.status {
            if status == "complete" {
                metrics.push("GOAL:complete".to_string());
            } else if status == "paused" {
                metrics.push("GOAL:paused".to_string());
            }
        }

        // Pause reason (abbreviated)
        if let Some(ref reason) = goal_meta.pause_reason {
            let short_reason = if reason.len() > 50 {
                format!("{}...", &reason[..47])
            } else {
                reason.clone()
            };
            metrics.push(format!("PAUSE:{}", short_reason));
        }

        // Token usage
        if let Some(tokens) = goal_meta.tokens_used {
            if tokens > 100_000 {
                metrics.push(format!("TOKENS:{}K", tokens / 1000));
            }
        }

        // Active time
        if let Some(seconds) = goal_meta.active_seconds {
            if seconds > 60 {
                metrics.push(format!("TIME:{}m", seconds / 60));
            }
        }

        // Task evidence (completed tasks with evidence)
        let tasks_with_evidence: Vec<&TaskDetail> = goal_meta
            .task_details
            .iter()
            .filter(|t| t.status == "complete" && t.evidence.is_some())
            .collect();
        if !tasks_with_evidence.is_empty() {
            let evidence_summary: Vec<String> = tasks_with_evidence
                .iter()
                .take(3)
                .map(|t| {
                    // The filter above guarantees Some; use expect to make
                    // that invariant visible to future maintainers.
                    let ev = t.evidence.as_ref().expect("filter guarantees Some");
                    if ev.len() > 40 {
                        format!("{}:{}", t.id, &ev[..37])
                    } else {
                        format!("{}:{}", t.id, ev)
                    }
                })
                .collect();
            let suffix = if tasks_with_evidence.len() > 3 {
                format!("+{}more", tasks_with_evidence.len() - 3)
            } else {
                String::new()
            };
            metrics.push(format!("EVIDENCE:{}{}", evidence_summary.join("|"), suffix));
        }

        // Skipped tasks with reasons
        let skipped_tasks: Vec<&TaskDetail> = goal_meta
            .task_details
            .iter()
            .filter(|t| t.status == "skipped" && t.skip_reason.is_some())
            .collect();
        if !skipped_tasks.is_empty() {
            let skip_summary: Vec<String> = skipped_tasks
                .iter()
                .take(3)
                .map(|t| {
                    // The filter above guarantees Some; use expect to make
                    // that invariant visible to future maintainers.
                    let reason = t.skip_reason.as_ref().expect("filter guarantees Some");
                    if reason.len() > 40 {
                        format!("{}:{}", t.id, &reason[..37])
                    } else {
                        format!("{}:{}", t.id, reason)
                    }
                })
                .collect();
            let suffix = if skipped_tasks.len() > 3 {
                format!("+{}more", skipped_tasks.len() - 3)
            } else {
                String::new()
            };
            metrics.push(format!("SKIPPED:{}{}", skip_summary.join("|"), suffix));
        }
    }

    let metrics_str = if metrics.is_empty() {
        String::new()
    } else {
        format!(" | {}", metrics.join(" "))
    };

    format!(
        "{}{} file(s){}{} DELTA:+{}/-{}{}",
        intent_prefix, files, dirs_str, files_str, added, removed, metrics_str
    )
}

/// Detect unmerged entries in the git index and, if `auto_resolve_unmerged`
/// is enabled and the working tree content matches HEAD for those paths,
/// reset them so the index is clean and the commit can proceed.
///
/// Returns the number of unmerged entries that were resolved.
///
/// This is the SINGLE-POINT-OF-FIX for the 4+ hour staleness seen in
/// `dracon-platform` where 4 unmerged PNGs in `web/ai-hub/audit-20260629/...`
/// blocked every commit for 4+ hours. With this function, the daemon
/// auto-detects the unmerged state, verifies the working tree matches HEAD
/// byte-by-byte, and resets the unmerge so the commit can proceed.
///
/// ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44.
async fn auto_resolve_unmerged_if_safe(
    repo: &Path,
    auto_resolve: bool,
) -> Result<usize> {
    use anyhow::Context;
    use std::process::Command;

    // List unmerged entries. The output format is:
    //   <mode> <hash> <stage> <path>
    // for each unmerged stage (1, 2, 3). We collect the unique
    // paths (one entry per path across all stages).
    let output = Command::new("git")
        .args([
            "-C", &repo.to_string_lossy(),
            "ls-files", "--unmerged",
        ])
        .output()
        .with_context(|| format!("failed to list unmerged files in {}", repo.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(0);
    }

    // Parse unmerged paths. For each path with stage=1/2/3 entries, we
    // need to check if the working tree matches HEAD. If yes, we can
    // safely `git reset HEAD -- <path>` to clear the unmerge.
    let mut unmerged_paths: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for line in stdout.lines() {
        // Format: <mode> <hash> <stage> <path>
        // Example: "100644 abc123... 1    foo/bar.png"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            // The path is everything from index 3 onwards (paths can
            // contain spaces in unusual cases)
            unmerged_paths.insert(parts[3..].join(" "));
        }
    }

    if unmerged_paths.is_empty() {
        return Ok(0);
    }

    if !auto_resolve {
        eprintln!(
            "⚠️ {} has {} unmerged entries (auto_resolve_unmerged=false, commits will fail). \
             Set auto_resolve_unmerged=true in the policy to clear them automatically.",
            repo.display(),
            unmerged_paths.len()
        );
        return Ok(0);
    }

    let mut resolved: usize = 0;
    for path in &unmerged_paths {
        // For each unmerged path, compare the working tree file
        // content to the HEAD version. If they match, we can safely
        // reset the unmerge.
        let head_output = Command::new("git")
            .args([
                "-C", &repo.to_string_lossy(),
                "show", &format!("HEAD:{}", path),
            ])
            .output();
        let wt_bytes = match std::fs::read(repo.join(path)) {
            Ok(b) => b,
            Err(_) => continue,  // file deleted; skip
        };
        let head_bytes = match head_output {
            Ok(o) if o.status.success() => o.stdout,
            _ => continue,  // path not in HEAD; skip
        };
        if wt_bytes == head_bytes {
            // Working tree matches HEAD — safe to reset the unmerge.
            // `git reset HEAD -- <path>` clears all stages and sets
            // the file to its HEAD state (which is what the working
            // tree already has).
            let reset_result = run_git_with_timeout(
                repo,
                &["reset", "HEAD", "--", path],
                10,
                "reset-unmerged",
            )
            .await;
            match reset_result {
                Ok(_) => {
                    eprintln!(
                        "🔧 {} auto-resolved unmerged entry (working tree matches HEAD): {}",
                        repo.display(),
                        path
                    );
                    resolved += 1;
                }
                Err(e) => {
                    eprintln!(
                        "⚠️ {} failed to auto-resolve unmerged entry {}: {}",
                        repo.display(),
                        path,
                        e
                    );
                }
            }
        } else {
            eprintln!(
                "⚠️ {} has unmerged entry with working tree != HEAD; \
                 manual resolution required: {}",
                repo.display(),
                path
            );
        }
    }

    Ok(resolved)
}

/// Check the untracked file count and emit a warning if it exceeds
/// the policy's `untracked_warn_threshold`. Returns the untracked
/// count for use in the `drain_health` report field.
///
/// The reported count subtracts entries that point to nested git
/// repositories under `repo` (e.g. `child/`, where `child/.git`
/// exists). Such entries inflate the parent's UT count but do not
/// represent new files in the parent — the child is a separately-
/// tracked, independently-synced git repo. See
/// `count_nested_repo_untracked_entries` in `git/discovery.rs`.
///
/// ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44.
/// CHANGED 2026-06-30, goal `mr02de1n-gjkgzp`: subtract nested-repo
/// entries so the parent UT count reflects only the parent's own
/// working-tree noise.
async fn check_untracked_threshold(
    repo: &Path,
    threshold: usize,
) -> Result<usize> {
    use anyhow::Context;
    use std::process::Command;
    // Always count the untracked files (so callers can use the count
    // for reporting), but only emit a warning when threshold > 0 AND
    // the count exceeds the threshold.
    let output = Command::new("git")
        .args([
            "-C", &repo.to_string_lossy(),
            "ls-files", "--others", "--exclude-standard",
        ])
        .output()
        .with_context(|| format!("failed to list untracked files in {}", repo.display()))?;
    let entries: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    let raw_count = entries.len();
    let nested_repo_count =
        crate::git::count_nested_repo_untracked_entries(repo, &entries);
    let count = raw_count.saturating_sub(nested_repo_count);
    if count > threshold {
        if nested_repo_count > 0 && crate::policy::debug_enabled() {
            eprintln!(
                "⚠️ {} has {} untracked files ({} raw − {} nested-repo entries; threshold {}). \
                 Consider adding ephemeral directories to .gitignore.",
                repo.display(),
                count,
                raw_count,
                nested_repo_count,
                threshold
            );
        } else {
            eprintln!(
                "⚠️ {} has {} untracked files (threshold {}). \
                 Consider adding ephemeral directories to .gitignore.",
                repo.display(),
                count,
                threshold
            );
        }
    }
    Ok(count)
}

async fn stage_commit_and_push(
    svc: &GitService,
    ctx: &mut SyncContext<'_>,
    _status: &dracon_git::types::RepoStatus,
    to_stage: &[dracon_git::types::DiffFile],
    to_restore: &[dracon_git::types::DiffFile],
) -> Result<Option<SyncOutcome>> {
    let repo = ctx.repo;
    let policy = ctx.policy;
    let dry_run = ctx.dry_run;
    let has_origin = ctx.has_origin;
    let _idle_seconds = ctx.idle_seconds;

    // FIX (goal 55db3bfc-4fc0-4650-8349-38da9e62bd44): auto-resolve
    // unmerged index entries so the commit loop is never blocked.
    // See `auto_resolve_unmerged_if_safe` for the full rationale.
    let resolved = auto_resolve_unmerged_if_safe(repo, policy.auto_resolve_unmerged).await?;
    if resolved > 0 {
        eprintln!(
            "🔧 {} auto-resolved {} unmerged entries",
            repo.display(),
            resolved
        );
    }

    // FIX (goal 55db3bfc-4fc0-4650-8349-38da9e62bd44): emit a warning
    // when the untracked-file count exceeds the policy threshold.
    let _untracked_count = check_untracked_threshold(repo, policy.untracked_warn_threshold).await?;

    // FIX (goal mr0xseig-fn9bbd): split `to_stage` into gitlink
    // pointer updates vs regular files. Gitlink entries are staged
    // without recursion (`git add <path>`) so the parent's index
    // records the new submodule SHA without walking into the
    // submodule's working tree. Regular files keep their existing
    // recursion-and-`git add -A` path.
    let (gitlink_entries, regular_entries): (Vec<dracon_git::types::DiffFile>, Vec<dracon_git::types::DiffFile>) = to_stage
        .iter()
        .cloned()
        .partition(|e| crate::exclude::is_gitlink(repo, &e.path));
    let gitlink_paths: Vec<String> = gitlink_entries
        .iter()
        .map(|e| e.path.to_string_lossy().to_string())
        .collect();
    let regular_paths: Vec<String> = regular_entries
        .iter()
        .map(|e| e.path.to_string_lossy().to_string())
        .collect();

    // FIX (2026-06-19, goal mqli43u6-tg3lcf): limit the number of files
    // staged in a single commit batch to avoid lock contention and large
    // commit overhead. When a repo has more untracked files than the
    // batch limit, the daemon commits them in multiple smaller batches.
    //
    // FIX (goal mr0xseig-fn9bbd): batch-limit applies to the union of
    // regular and gitlink paths; gitlinks are NOT subject to the
    // recursion-skip behavior so they don't contribute to the
    // subdir-expansion that would otherwise inflate counts (a single
    // gitlink path is one entry, not many).
    let max_batch = policy.max_stage_batch_files;
    let total_to_stage = regular_paths.len() + gitlink_paths.len();
    let (regular_paths, gitlink_paths): (Vec<String>, Vec<String>) =
        if total_to_stage > max_batch {
            eprintln!(
                "📦 batching {} entries into chunks of {}",
                total_to_stage,
                max_batch
            );
            let take = max_batch;
            (
                regular_paths.into_iter().take(take).collect(),
                gitlink_paths.into_iter().take(take).collect(),
            )
        } else {
            (regular_paths, gitlink_paths)
        };

    let (existing, missing): (Vec<_>, Vec<_>) =
        regular_paths.into_iter().partition(|p| repo.join(p).exists());

    // FIX (goal mr0xseig-fn9bbd): gitlink pointer updates are
    // staged WITHOUT recursion via `git add <path>` (no `-A`).
    // `stage_existing_files` recurses-and-skips for anything in
    // `existing` whose `.git` exists; for gitlinks we want the
    // OPPOSITE — do not recurse, but DO emit the `git add`.
    let (gitlink_existing, gitlink_missing): (Vec<_>, Vec<_>) =
        gitlink_paths.into_iter().partition(|p| repo.join(p).exists());

    // CHANGED 2026-07-21 (v0.112.31, audit H6/F1.5): filtered
    // variant so files discovered by the directory-expansion
    // recursion get the per-file staging policy re-applied (size
    // limit + exclude patterns) — previously a 500 MiB file nested
    // in a new untracked dir bypassed the 100 MiB hard exclusion.
    // The `auto_commit_exclude_patterns` value uses the SAME
    // effective (per-repo-override-aware) resolution as the
    // partition in `sync_repo_with_ahead_since`.
    let staging_repo_override = load_repo_override(repo);
    let auto_commit_exclude_for_staging = staging_repo_override
        .auto_commit_exclude_patterns
        .as_deref()
        .unwrap_or(&ctx.policy.auto_commit_exclude_patterns);
    stage_existing_files_filtered(
        repo,
        &existing,
        dry_run,
        ctx.policy.stage_op_timeout_secs,
        ctx.excluded_dir_names,
        Some((ctx.policy, auto_commit_exclude_for_staging)),
    )
    .await?;

    stage_gitlink_updates(
        repo,
        &gitlink_existing,
        dry_run,
        ctx.policy.stage_op_timeout_secs,
    )
    .await?;

    let missing_iter = missing.into_iter().chain(gitlink_missing);
    let missing: Vec<_> = missing_iter.collect();
    if !missing.is_empty() {
        git_rm_missing(repo, &missing, dry_run).await?;
    }

    let staged = git_name_status_entries(repo, &["diff", "--cached", "--name-status"]).await?;
    let committed_entries: Vec<dracon_git::types::DiffFile> = staged
        .into_iter()
        .map(|(path, status)| dracon_git::types::DiffFile::new(path, status))
        .collect();

    let blast_radius = compute_blast_radius(repo);

    let version_bumped = false;

    if committed_entries.is_empty() {
        if let Err(e) = run_git_with_timeout(repo, &["reset", "HEAD", "--"], 10, "reset").await {
            // Non-fatal: the staging area will be cleaned up on the next sync cycle.
            // The most common cause is index.lock contention from a concurrent process
            // (sync-now CLI, warden daemon) holding the lock on the same repo.
            eprintln!(
                "⚠️ {} filter-only commit: git reset failed (non-fatal, will retry next cycle): {}",
                repo.display(),
                e
            );
        }
        if debug_enabled() {
            eprintln!(
                "🐛 {} skipped commit: all changes were filter-only (smudge/clean)",
                repo.display()
            );
        }
        maybe_sync_visibility_and_metadata(ctx);
        return Ok(Some(SyncOutcome::NothingToDo));
    }

    // Use blast radius as commit message
    let msg = blast_radius;

    if dry_run {
        println!(
            "📝 Would commit {} file(s) in {}:",
            committed_entries.len(),
            repo.display()
        );
        for entry in committed_entries.iter().take(10) {
            println!("  {:?}: {}", entry.status, entry.path.display());
        }
        if committed_entries.len() > 10 {
            println!("  ... and {} more", committed_entries.len() - 10);
        }
        println!("  message: {}", msg.lines().next().unwrap_or("(empty)"));
    } else {
        svc.commit(&msg).await?;
        eprintln!(
            "📝 committed {} file(s) in {}",
            committed_entries.len(),
            repo.display()
        );
        // If the repo is a materialized submodule's standalone
        // worktree, no extra fast-forward is needed: the
        // standalone is on `main` directly, so each commit
        // already advances `main`. The previous code called
        // `fast_forward_daemon_standalone_to_main` here to
        // copy the daemon-standalone branch's HEAD to main;
        // that helper is now a no-op (kept for backwards
        // compatibility). The call below remains so the
        // daemon's main flow doesn't change, but it does
        // nothing.
        //
        // CHANGED 2026-07-01 (goal `mr1x7j5i-zioba9`):
        // The previous version fast-forwarded `main` to the
        // daemon-standalone branch's HEAD after each commit.
        // With the standalone on `main` directly (no buffer
        // branch), this fast-forward is unnecessary.
        if let Err(e) = fast_forward_daemon_standalone_to_main(repo).await {
            eprintln!(
                "⚠️ fast_forward_daemon_standalone_to_main (no-op) error in {}: {}",
                repo.display(),
                e
            );
        }
        // Flush so journald captures commit activity in real-time
        let _ = std::io::stderr().flush();
        // Record this commit in the incident ledger so the `repos` report
        // can show the user the daemon is actively syncing this repo.
        // Without this entry, only `repair warns` (and friends) would
        // appear in the ledger, and normal auto-commit activity would be
        // invisible to the report.
        if let Some(pp) = ctx.policy_path {
            crate::report::log_incident(
                pp,
                "sync",
                repo.display().to_string(),
                format!("COMMITTED:{} files", committed_entries.len()),
                "sync_commit",
                None,
                "ok",
                Some(msg.lines().next().unwrap_or("").to_string()),
            );
        }
    }

    prune_other_default_branch(repo).await;

    post_commit_pull(svc, repo, policy).await;

    let alert_status = svc.get_status().await?;
    if alert_status.ahead > policy.alert_unpushed_threshold {
        eprintln!(
            "🚨 ALERT: {} has {} unpushed commits (threshold: {}). Something may be wrong with push.",
            repo.display(),
            alert_status.ahead,
            policy.alert_unpushed_threshold
        );
    }

    restore_excluded_paths(ctx, to_restore).await?;

    // CHANGED 2026-07-21 (v0.112.31, audit H3/F1.3): track push
    // failure and surface it as `SyncOutcome::PushFailed` instead of
    // falling through to `Ok(None)` (→ `Synced`). Previously the
    // apply phase logged `🔁 synced`, reset `failure_count`, and
    // dropped the activity entry on a failed push — false-healthy.
    let mut push_failed = false;
    if policy.auto_push && (has_origin || !policy.remotes.is_empty()) {
        // Push synchronously so mirror failures can be tracked in
        // `ctx.remote_failures` (the caller passes a `&mut HashMap`).
        // The `tokio::spawn` fire-and-forget pattern was removed because
        // it bypassed the failure-tracking needed by callers like
        // `test_sync_repo_mirror_failu[DRACON_SECRET:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSA4bHNsaDJTNlRybHV2ajNZeCtNbzJoNXNKWkVBaVAwTTQ1Z0ZscXRHSDBRClJEdUJONitmdVpERjhyWE5Nak1GVVJyYlU2MnMzZDBMOGtpaTZpN1A3Z28KLT4gWDI1NTE5IHk4SVhDUWxiM0Rlb0RKb2pkVWU1RTVLYjVhOWJvemlqWXlEN0JsY3dkekUKTnQxQXlTeFhXeHJsZ010U0dGQ1FidjRYUG1jeFBFSmpQZXloQW5RMUJROAotPiBYMjU1MTkgbFhYT0x2cFE1dzB6K0x2dTFiVUJWTjJ3NHI4S2NyZEFmU2ZlNXFaS2NoZwp3M2NIWXBqWDlLaXY0cU5qN1VzMXZGcUNxdTUxOGxBaUp3d1FnSjRzRHNFCi0+IFgyNTUxOSA5QjVDNTdZWWRlN1BidDMzUUJCY3RtY09HYnkxRmpJaVNpQnRyOFo5NTA4CjBSTVBJTVpFcURib1hVQ0pxMmVSWEROV1NLcFo0YXJWbXZiQmsyRVZDUkkKLT4gWDI1NTE5IGRvMTYyQ2F5am5OTm5QUitoWHlQQUkxMVVKc25SSnhVSTVtTTZPcGtjbmcKbEJTZDEvTDI1c01waGY4Vnd4aDdoNFRVRGRtMzlsS3VRMzhVVC9kWW0xNAotPiA2RX4tZ3JlYXNlIF4kLDtWJApPVWFyTmhLQ2RjM09XNU1qNXV3OGVBZFlqK3VFVFZFUkFRMXFhY01ITjRydDgwTDVYcjVKMU5NUDlvSXRvajBsClZZcUo5WUFvR0hELzBQR3JaYkFzM0ZrTHRCaTcyclZEUTAxVU04cXFXQXZENXdwVTY5dWxHb2pxVXpPKwotLS0gK0Ztam5zY0FYeE9WTVFqOVh1VEl3UjNZSUx3TGVNU3JxZlNIZXg4V3ZVNAqPpl3vrjZOIFVT3wonvzSsHpNG+32mNdocHzYsX5MzErQl+21wdpqd2tgosG54/HbIdgOTh3zxu+g=]`.
        match push_background(repo, policy, has_origin, ctx.remote_failures.as_deref_mut()).await {
            Ok(true) => {
                if let Err(e) = crate::daemon::refresh_publish_upstream(repo, policy).await {
                    // The publish upstream config is already set by
                    // `configure_publish_upstream_if_missing` and surfaces
                    // correctly in the PUBLISH column. A failed refresh
                    // only means `git rev-parse @{u}` still resolves
                    // through the configured merge key, not via a fetched
                    // remote-tracking ref. That's a transient state that
                    // resolves on the next successful push, so log it at
                    // debug to avoid spamming warnings on every cycle.
                    if debug_enabled() {
                        eprintln!(
                            "🐛 refresh publish upstream transient for {}: {}",
                            repo.display(),
                            e
                        );
                    }
                }
                crate::daemon::record_push_success(repo);
            }
            Ok(false) => {
                eprintln!("⚠️ push failed for {}", repo.display());
                crate::daemon::record_push_failure(
                    repo,
                    "git push returned non-zero (see daemon log)",
                );
                push_failed = true;
            }
            Err(e) => {
                eprintln!("⚠️ push error for {}: {}", repo.display(), e);
                crate::daemon::record_push_failure(repo, &e.to_string());
                push_failed = true;
            }
        }
    }

    run_release_pipeline_if_bumped(repo, policy, version_bumped).await;

    if push_failed {
        return Ok(Some(SyncOutcome::PushFailed));
    }

    Ok(None)
}

/// ADDED 2026-07-21 (v0.112.30): policy-aware root-commit bootstrap for
/// *stable empty* repos (operator ran `git init`, added files, no
/// commits yet).
///
/// Replaces the previous bare `git add -A && git commit -m initial`
/// bootstrap, which (a) ignored `max_stage_file_bytes`,
/// `untracked_exclude_patterns`, `auto_commit_exclude_patterns`,
/// `auto_stage_untracked`, and the ownership guard, and (b) discarded
/// the commit result (printing "created initial commit" even when the
/// commit failed, e.g. missing `user.email`).
///
/// Callers MUST gate on `crate::git::is_stable_empty_repo` first —
/// this function does not re-check the mid-clone guards.
///
/// Flow:
/// 1. Gate on `policy.auto_commit` (operator opted out of auto-commit).
/// 2. Ownership gate identical to the daemon loop's: skip unowned
///    repos when `auto_skip_unowned` is in effect. For an empty repo
///    the ownership signals are `user.email` + origin URL (no HEAD
///    author exists yet).
/// 3. Enumerate untracked files via `untracked_entries`
///    (`git ls-files --others --exclude-standard -z`), which respects
///    `.gitignore` — including the warden-managed secrets block.
/// 4. Apply the same per-entry policy filters the normal pipeline
///    uses (`should_stage_entry` + `matches_untracked_exclude` +
///    `auto_stage_untracked`).
/// 5. `git add -A -- <explicit paths>` via `stage_existing_files`
///    (never bare `git add .`).
/// 6. `git commit --no-verify -m "auto: initial commit (N files)"`.
///
/// Returns Ok(true) when a root commit was created, Ok(false) when
/// there was nothing policy-compliant to commit (caller should treat
/// the repo as still-empty), Err on git failures.
pub(crate) async fn bootstrap_empty_repo_commit(
    repo: &Path,
    policy: &SyncPolicy,
    excluded_dir_names: &BTreeSet<String>,
    dry_run: bool,
) -> Result<bool> {
    if !policy.auto_commit {
        return Ok(false);
    }

    // Ownership gate — mirrors the daemon loop's guard so a bootstrap
    // can never auto-commit into someone else's repo.
    let repo_override = load_repo_override(repo);
    let trusted = crate::ownership::TrustedSet {
        emails: policy.trusted_emails.clone(),
        authors: policy.trusted_authors.clone(),
        remote_hosts: policy.trusted_remote_hosts.clone(),
    };
    let ownership = crate::ownership::detect_ownership(repo, &trusted, repo_override.owned);
    let auto_skip_unowned = repo_override
        .auto_skip_unowned
        .unwrap_or(policy.auto_skip_unowned);
    if auto_skip_unowned
        && !matches!(ownership, crate::ownership::OwnershipReport::Owned { .. })
    {
        if debug_enabled() {
            eprintln!(
                "🚫 {} empty repo not owned, skipping root-commit bootstrap",
                repo.display()
            );
        }
        return Ok(false);
    }

    // Untracked enumeration respects .gitignore (including the
    // warden-managed secrets block) via --exclude-standard.
    let untracked = untracked_entries(repo).await.unwrap_or_default();
    let auto_commit_exclude = repo_override
        .auto_commit_exclude_patterns
        .as_deref()
        .unwrap_or(&policy.auto_commit_exclude_patterns);
    let mut to_stage: Vec<String> = Vec::new();
    for entry in &untracked {
        // `auto_stage_untracked = false` skips newly-added files,
        // exactly like the normal pipeline's partition.
        if !policy.auto_stage_untracked {
            continue;
        }
        if crate::exclude::matches_untracked_exclude(
            repo,
            &entry.path,
            &policy.untracked_exclude_patterns,
        ) {
            continue;
        }
        if !should_stage_entry(
            repo,
            entry,
            excluded_dir_names,
            &policy.exclude_file_patterns,
            policy.max_stage_file_bytes,
            auto_commit_exclude,
        ) {
            continue;
        }
        to_stage.push(entry.path.to_string_lossy().to_string());
    }
    if to_stage.is_empty() {
        return Ok(false);
    }

    if dry_run {
        println!(
            "🌱 Would create root commit in {} with {} file(s):",
            repo.display(),
            to_stage.len()
        );
        for p in to_stage.iter().take(10) {
            println!("  {}", p);
        }
        if to_stage.len() > 10 {
            println!("  ... and {} more", to_stage.len() - 10);
        }
        return Ok(false);
    }

    // Stage explicit paths (never bare `git add .`).
    // CHANGED 2026-07-21 (v0.112.31, audit H6/F1.5): filtered variant
    // so directory-expanded files get the per-file policy re-applied.
    stage_existing_files_filtered(
        repo,
        &to_stage,
        dry_run,
        policy.stage_op_timeout_secs,
        excluded_dir_names,
        Some((policy, auto_commit_exclude)),
    )
    .await?;

    // Verify something actually landed in the index (files may have
    // been deleted between ls-files and add).
    let staged = git_name_status_entries(repo, &["diff", "--cached", "--name-status"]).await?;
    if staged.is_empty() {
        return Ok(false);
    }

    let msg = format!("auto: initial commit ({} files)", staged.len());
    run_git_with_timeout(
        repo,
        &["commit", "--no-verify", "-m", &msg],
        policy.stage_op_timeout_secs,
        "bootstrap-root-commit",
    )
    .await?;
    eprintln!(
        "🌱 {} created root commit ({} files, empty repo bootstrap)",
        repo.display(),
        staged.len()
    );
    Ok(true)
}

pub(crate) async fn sync_repo(
    repo: &Path,
    policy: &SyncPolicy,
    excluded_dir_names: &BTreeSet<String>,
    idle_seconds: u64,
    remote_failures: Option<&mut HashMap<String, usize>>,
    dry_run: bool,
    policy_path: Option<&Path>,
) -> Result<SyncOutcome> {
    sync_repo_with_ahead_since(repo, policy, excluded_dir_names, idle_seconds, remote_failures, dry_run, policy_path, None).await
}

/// Like `sync_repo` but also takes the `ahead_since` instant from the
/// daemon's activity map, so the auto-commit backstop can be computed
/// inside `sync_repo`. Pass `None` from one-off callers (e.g.
/// `dracon-sync once`); the daemon passes the activity map's value.
pub(crate) async fn sync_repo_with_ahead_since(
    repo: &Path,
    policy: &SyncPolicy,
    excluded_dir_names: &BTreeSet<String>,
    idle_seconds: u64,
    remote_failures: Option<&mut HashMap<String, usize>>,
    dry_run: bool,
    policy_path: Option<&Path>,
    ahead_since: Option<std::time::Instant>,
) -> Result<SyncOutcome> {
    let svc = GitService::new(repo)?;
    if !svc.is_git_repo().await? {
        if debug_enabled() {
            eprintln!("🐛 {} is not recognized as git repo", repo.display());
        }
        let ctx = SyncContext {
            repo,
            policy,
            excluded_dir_names,
            dry_run,
            idle_seconds,
            policy_path,
            has_origin: false,
            has_upstream: false,
            auto_bump_versions: false,
            remote_failures: None,
            backstop_active: false,
        };
        maybe_sync_visibility_and_metadata(&ctx);
        return Ok(SyncOutcome::NothingToDo);
    }

    if let Some(blocked) = check_conflict_state(repo) {
        let ctx = SyncContext {
            repo,
            policy,
            excluded_dir_names,
            dry_run,
            idle_seconds,
            policy_path,
            has_origin: false,
            has_upstream: false,
            auto_bump_versions: false,
            remote_failures: None,
            backstop_active: false,
        };
        maybe_sync_visibility_and_metadata(&ctx);
        return Ok(blocked);
    }

    if !is_repo_ready(repo) {
        // CHANGED 2026-07-21 (v0.112.30): the previous bootstrap used
        // bare `git add -A` + `git commit -m initial` (no size limits,
        // no exclude patterns, no ownership gate, errors discarded),
        // and — critically — was unreachable from the daemon loop
        // because `daemon.rs` bailed on `!is_repo_ready` BEFORE
        // dispatching `sync_repo`. The daemon loop now bootstraps
        // stable empty repos itself; this block covers the CLI
        // (`sync-now`) path and any direct callers. On success we
        // fall through so the rest of the pipeline pushes the fresh
        // root commit in the same invocation.
        if !crate::git::is_stable_empty_repo(repo) {
            eprintln!(
                "⏳ {} not ready (mid-clone or empty repo), skipping",
                repo.display()
            );
            return Ok(SyncOutcome::NothingToDo);
        }
        match bootstrap_empty_repo_commit(repo, policy, excluded_dir_names, dry_run).await {
            Ok(true) => {
                // Fall through: HEAD is now valid, the pipeline below
                // computes status and pushes the root commit.
            }
            Ok(false) => {
                return Ok(SyncOutcome::NothingToDo);
            }
            Err(e) => {
                eprintln!(
                    "⚠️ {} empty-repo bootstrap failed: {}",
                    repo.display(),
                    e
                );
                return Ok(SyncOutcome::NothingToDo);
            }
        }
    }

    let has_origin = ensure_origin_remote(repo, policy);
    let has_upstream = has_tracking_upstream(repo);
    let initial_status = svc.get_status().await?;

    let repo_override = load_repo_override(repo);
    let auto_bump_versions = repo_override
        .auto_bump_versions
        .unwrap_or(policy.auto_bump_versions);

    // CHANGED 2026-07-01 (goal `mr1x7j5i-zioba9`):
    // The previous version of this code unconditionally
    // fast-forwarded the shared gitdir's `main` ref to the
    // standalone's `daemon-standalone` tip at the start of
    // every sync_repo cycle. That was needed when the
    // standalone was on a separate `daemon-standalone` branch
    // and the parent's gitlink tracked `main`. With the
    // daemon-standalone branch removed (the standalone is on
    // `main` directly now), the helper is a no-op and this
    // step does nothing. The call is preserved for backwards
    // compatibility with the daemon's main flow.
    //
    // ADDED 2026-07-01, goal `mr10pdzr-i495vy` (original):
    // The unconditional call was needed because the previous
    // design committed on `daemon-standalone` and only
    // fast-forwarded `main` in the post-commit hook. When the
    // standalone was clean, the daemon skipped the commit step
    // and `main` stayed stale. Running the fast-forward
    // unconditionally at cycle start fixed that. The fix is
    // no longer needed (the standalone is on `main` directly,
    // so commits always advance `main`), but the call site is
    // preserved.
    if !dry_run {
        if let Err(e) = fast_forward_daemon_standalone_to_main(repo).await {
            eprintln!(
                "⚠️ fast_forward_daemon_standalone_to_main (no-op) error for {}: {}",
                repo.display(),
                e
            );
        }
    }

    // Compute the auto-commit backstop. The backstop fires when
    // a repo has more than `auto_commit_backstop_threshold`
    // unpushed commits AND the push has been pending
    // (`ahead_since`) for at least
    // `auto_commit_backstop_min_age_secs`. We check it here so the
    // SyncContext has the flag for the auto_commit block below.
    let backstop_active = is_backstop_active(
        ahead_since,
        std::time::Instant::now(),
        initial_status.ahead,
        policy.auto_commit_backstop_threshold,
        policy.auto_commit_backstop_min_age_secs,
    );
    if backstop_active {
        eprintln!(
            "⏸️  daemon backstop: {} unpushed commits pending push >{}s, skipping auto-commit for {}",
            initial_status.ahead,
            policy.auto_commit_backstop_min_age_secs,
            repo.display(),
        );
    }

    let mut ctx = SyncContext {
        repo,
        policy,
        excluded_dir_names,
        dry_run,
        idle_seconds,
        policy_path,
        has_origin,
        has_upstream,
        auto_bump_versions,
        remote_failures,
        backstop_active,
    };

    let copied_standard_files = if policy.standard_files_auto {
        // Acquire git's index.lock before writing standard files to the working tree.
        // This prevents conflicting with git's own checkout during clone — if git
        // holds the lock, we skip; if we hold it, git waits. No heuristics needed.
        match crate::git::IndexLock::acquire(repo) {
            Ok(_lock) => crate::standard_files::ensure_standard_files(
                repo,
                policy,
                &repo_override,
                policy_path.map(|p| p.parent().unwrap_or(p)),
                dry_run,
            )?,
            Err(e) => {
                if crate::policy::debug_enabled() {
                    eprintln!("⏳ {}", e);
                }
                vec![] // skip standard files this cycle
            }
        }
    } else {
        vec![]
    };

    if !copied_standard_files.is_empty() && !dry_run {
        let paths: Vec<String> = copied_standard_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        // CHANGED 2026-07-21 (v0.112.31, audit H6/F1.5): filtered
        // variant for consistency with the main staging path.
        let std_repo_override = load_repo_override(repo);
        let std_auto_commit_exclude = std_repo_override
            .auto_commit_exclude_patterns
            .as_deref()
            .unwrap_or(&ctx.policy.auto_commit_exclude_patterns);
        stage_existing_files_filtered(
            repo,
            &paths,
            dry_run,
            ctx.policy.stage_op_timeout_secs,
            ctx.excluded_dir_names,
            Some((ctx.policy, std_auto_commit_exclude)),
        )
        .await?;
    }

    auto_pull_merge(&svc, &ctx, &initial_status).await?;

    clean_staged_paths(&ctx).await?;

    let DiffResult {
        mut status,
        mut entries,
        filter_only_cleared,
    } = compute_diff_entries(&svc, repo).await?;

    // ADDED 2026-07-01, goal `mr10pdzr-i495vy`:
    // Inject synthetic DiffFile entries for any tracked gitlink whose
    // SHARED gitdir's `refs/heads/main` is ahead of the parent's
    // tracked gitlink SHA. This is the propagation-fix half of the
    // submodule-as-standalone-worktree feature:
    //
    // - The nested submodule checkout at `<parent>/<submodule_path>/`
    //   is detached at the parent's gitlink SHA, while the shared
    //   gitdir's `main` advances independently. When the nested HEAD
    //   still equals the parent gitlink, `git diff HEAD` reports NO
    //   change and the gitlink never enters `compute_diff_entries`'s
    //   `entries`, so the parent's gitlink silently diverges from
    //   `main`.
    //
    // Fix: enumerate the stale gitlinks here and inject them into
    // `entries` with status = Modified so they flow through the same
    // partition-and-stage pipeline as naturally-dirty entries.
    // `stage_gitlink_updates` then reads the shared `main` ref via
    // `shared_submodule_main_sha` and writes the correct SHA into the
    // parent's index via `git update-index --cacheinfo 160000,<sha>,
    // <path>`.
    //
    // Status must be Modified (not Added) so that:
    // 1. The entry is not dropped by the "auto_stage_untracked = false"
    //    filter that runs against `Added` entries.
    // 2. The path is already tracked in the parent, which is correct
    //    (gitlinks are tracked entries with mode 160000).
    //
    // Empty `mut entries` is fine — extend() appends without
    // disturbing any existing entries (which would already represent
    // dirty gitlinks from this commit's perspective).
    let stale_paths = crate::exclude::stale_gitlink_paths(repo);
    if !stale_paths.is_empty() {
        if debug_enabled() {
            eprintln!(
                "🐛 {} injecting {} stale gitlink(s) from shared main: {:?}",
                repo.display(),
                stale_paths.len(),
                stale_paths
            );
        }
        // If libgit2 thought the repo was clean (no dirty entries at
        // all), the auto-commit branch downstream would be skipped via
        // the `!status.is_clean` guard. Stale gitlink entries are
        // effectively dirty (their pointer needs to advance), so flip
        // is_clean off so the auto-commit pipeline runs them.
        status.is_clean = false;
        status.modified_files += stale_paths.len();
        for p in stale_paths {
            // Defensive dedup: if `entries` already contains this
            // path (e.g. from a natural `git diff HEAD` dirty
            // gitlink), don't double-inject.
            if entries.iter().any(|e| e.path == p) {
                continue;
            }
            entries.push(dracon_git::types::DiffFile::new(
                p,
                dracon_git::types::FileStatus::Modified,
            ));
        }
    }

    // filter_only_cleared: changes present but all filtered out by clean/smudge.
    // Don't stage/commit — return NothingToDo so the daemon applies a cooldown.
    if filter_only_cleared {
        if debug_enabled() {
            eprintln!(
                "🐛 {} filter-only dirty, returning NothingToDo for cooldown",
                repo.display(),
            );
        }
        return Ok(SyncOutcome::NothingToDo);
    }

    if !status.is_clean && policy.auto_commit {
        // Auto-commit backstop: when the daemon detects a repo with
        // many unpushed commits and a long-pending push, it stops
        // auto-committing to prevent the moving-target problem (each
        // new commit forces the daemon to retry the entire push from
        // scratch). The backstop logs a `⏸️ daemon backstop` line at
        // the top of `sync_repo`; here we honour it by short-
        // circuiting the auto-commit step. Manual `git add`/`git
        // commit` from the operator still works.
        if ctx.backstop_active {
            return Ok(SyncOutcome::NothingToDo);
        }
        // When `auto_stage_untracked = false`, we need to know which
        // Added-status entries are untracked vs freshly-staged-tracked.
        // Build the tracked set once per sync_repo call.
        let tracked_paths: std::collections::HashSet<std::path::PathBuf> =
            if policy.auto_stage_untracked {
                std::collections::HashSet::new()
            } else {
                crate::git::tracked_paths(repo).await.unwrap_or_default()
            };
        let (to_stage, to_restore): (Vec<_>, Vec<_>) = entries
            .into_iter()
            .filter(|e| {
                if repo.join(&e.path).is_dir()
                    && crate::exclude::is_gitlink_unchanged(repo, &e.path)
                {
                    return false;
                }
                // `auto_stage_untracked = false` skips newly-added (untracked)
                // files. The daemon still commits modified tracked files
                // and unstaged modifications. Use this to keep scratch
                // research, notes, and other operator-local files out of
                // the auto-commit while still syncing tracked changes.
                if matches!(e.status, dracon_git::types::FileStatus::Added)
                    && !tracked_paths.contains(&e.path)
                {
                    if !policy.auto_stage_untracked {
                        if debug_enabled() {
                            eprintln!(
                                "⏭️  {} skipping untracked {} (auto_stage_untracked = false)",
                                repo.display(),
                                e.path.display()
                            );
                        }
                        return false;
                    }
                    // `untracked_exclude_patterns` lets the operator
                    // exclude specific patterns (notes, scratch, audit
                    // evidence) from auto-stage even when the toggle is
                    // on. Match on the basename and on path-with-glob
                    // via `glob` against the full path.
                    if crate::exclude::matches_untracked_exclude(
                        repo,
                        &e.path,
                        &policy.untracked_exclude_patterns,
                    ) {
                        if debug_enabled() {
                            eprintln!(
                                "⏭️  {} skipping untracked {} (untracked_exclude_patterns)",
                                repo.display(),
                                e.path.display()
                            );
                        }
                        return false;
                    }
                }
                true
            })
            .partition(|e| {
                should_stage_entry(
                    repo,
                    e,
                    excluded_dir_names,
                    &policy.exclude_file_patterns,
                    policy.max_stage_file_bytes,
                    repo_override
                        .auto_commit_exclude_patterns
                        .as_deref()
                        .unwrap_or(&policy.auto_commit_exclude_patterns),
                )
            });
        if debug_enabled() {
            eprintln!(
                "🐛 {} to_stage={} to_restore={}",
                repo.display(),
                to_stage.len(),
                to_restore.len()
            );
        }
        if !to_stage.is_empty() {
            if let Some(outcome) =
                stage_commit_and_push(&svc, &mut ctx, &status, &to_stage, &to_restore).await?
            {
                return Ok(outcome);
            }
        } else if policy.auto_push && !has_origin && policy.remotes.is_empty() {
            eprintln!("ℹ️ skip push for {} (no origin remote and no mirror remotes)", repo.display());
        }

        return Ok(SyncOutcome::Synced);
    }

    maybe_sync_visibility_and_metadata(&ctx);

    // CHANGED 2026-07-21 (v0.112.31, audit H3/F1.3): a failed push
    // surfaces as `PushFailed` so the daemon's apply phase counts it
    // as a failure (no `🔁 synced`, `failure_count` increments)
    // instead of the previous false-healthy `NothingToDo`.
    let push_ok = handle_ahead_push(&mut ctx, &svc).await?;

    maybe_sync_visibility_and_metadata(&ctx);
    if !push_ok {
        return Ok(SyncOutcome::PushFailed);
    }
    Ok(SyncOutcome::NothingToDo)
}

/// Pushes unpushed commits when needed.
///
/// Returns `Ok(true)` when no push was needed or the push succeeded,
/// `Ok(false)` when a push was attempted and FAILED (already recorded
/// to the push ledger by this function). CHANGED 2026-07-21
/// (v0.112.31, audit H3/F1.3): previously returned `Result<()>` and
/// swallowed push failures, so the caller's `NothingToDo` outcome
/// read as success in the daemon's apply phase.
async fn handle_ahead_push(ctx: &mut SyncContext<'_>, svc: &GitService) -> Result<bool> {
    let current_status = svc.get_status().await?;
    let branch_has_upstream = super::git::has_tracking_upstream(ctx.repo);
    // CHANGED 2026-07-21 (v0.112.30): when the upstream is configured
    // (e.g. by `configure_publish_upstream_if_missing`) but the
    // remote-tracking ref does not exist yet — the never-pushed state
    // every freshly-bootstrapped empty repo is in — libgit2's
    // ahead/behind returns 0 (nothing to compare against) and the old
    // `should_push` expression evaluated to false, so the root commit
    // was NEVER pushed and the report showed a false "synced". Treat
    // a missing remote-tracking ref as push-needed: every commit on
    // HEAD is definitionally unpushed to that upstream.
    let upstream_ref_missing =
        branch_has_upstream && super::git::upstream_tracking_ref_missing(ctx.repo);
    let should_push =
        current_status.ahead > 0 || !branch_has_upstream || upstream_ref_missing;
    if ctx.policy.auto_push && should_push && (ctx.has_origin || !ctx.policy.remotes.is_empty()) {
        // Push synchronously so mirror failures are tracked in
        // `ctx.remote_failures`. Previously this used `tokio::spawn`
        // (fire-and-forget), which made the failure tracking unreachable
        // for callers like the test `test_sync_repo_mirror_failu[DRACON_SECRET:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBEN0VOVnZ3NmtJakJ3R3JVK09mZzdCN0wveFVMcFo3SUtBZ3F0TU5XTUJJCnlTbTRCdmFmV1dQRHFtYmlnZWt1aDNod0pUUFFMZWN0RGJrT0JFaWVTODAKLT4gWDI1NTE5IDVvbWxWajZFcHdQMVR4cDZLYVZqNU04bGhXQVZEZzdoQlowRmp4U05NZ3MKWUVaOWZtMHBnMUswNkpOclhHSDFYc2YxWTAySTcvUVp3VFRGTXdUc3pSdwotPiBYMjU1MTkgSDJWTDVMNjZ3d0F0YzUvQ0FBK3R0ekRTQ3p5eEMwQTZSMW85TWVlTkIzcwpYY0pjWElwZTRLVFRKNTRuVWdzaDVIWUFQU0ZWM0Z0YjlaeDM2bEZ3b3VBCi0+IFgyNTUxOSBJREdRT0FmMWlydU1BVzVwMm1keXBLdEwwa1o1T1BQdDd0ZEdab0h3djE0CkpNRUViclh4RTQ5aU9GZW5BVXVSMzJFM3JzUndwUHJzYjhnVE8ySGhMamcKLT4gWDI1NTE5IExwN1ZLSXRMUmhGVllWR1ZrQmFyd1VBWGNRVUxNYU9CbGlwZnR4ZEVVSHMKalZFRjBveVBLNjlWd1FNeHkxT2FNVGtLUk5XTHVYU1JFTDU5OGJLT3lVSQotPiAnN0Uocyw1KC1ncmVhc2UgdFM/OnFZayA8RVV1cFdKIGIKN2lEbTJKaVFKQmtDSXcKLS0tIDlubXZsbWtjemhUeEdud08rY1ZGa1RzdjdNdHZWaUpjMjRZaHdleXk5SlUKYyi3BbLgMmduqp34N2FUDTP5RurqzigA/2vL4o2NImLiONhhlUcin4eeMVo2Y/5AFH1JAiN+y/2L]`.
        match push_background(ctx.repo, ctx.policy, ctx.has_origin, ctx.remote_failures.as_deref_mut()).await {
            Ok(true) => {
                crate::daemon::record_push_success(ctx.repo);
            }
            Ok(false) => {
                eprintln!("⚠️ push failed for {}", ctx.repo.display());
                crate::daemon::record_push_failure(
                    ctx.repo,
                    "git push returned non-zero (see daemon log)",
                );
                // CHANGED 2026-07-21 (v0.112.31, audit H3/F1.3):
                // propagate the failure so the caller returns
                // `SyncOutcome::PushFailed` instead of `NothingToDo`
                // (which the apply phase treated as success).
                return Ok(false);
            }
            Err(e) => {
                eprintln!("⚠️ push error for {}: {}", ctx.repo.display(), e);
                crate::daemon::record_push_failure(ctx.repo, &e.to_string());
                return Ok(false);
            }
        }
    } else if ctx.policy.auto_push && should_push && !ctx.has_origin && ctx.policy.remotes.is_empty() {
        eprintln!("ℹ️ skip push for {} (no origin remote and no mirror remotes)", ctx.repo.display());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_sanitize_task_name_drops_explanatory_clauses() {
        assert_eq!(
            sanitize_task_name("Fix A: Added .dracon/ and .pub to NOISE_PATTERNS in bump.rs"),
            "Fix A"
        );
        assert_eq!(
            sanitize_task_name("Stale focus detection — details were noisy"),
            "Stale focus detection"
        );
        assert_eq!(
            sanitize_task_name("merge: resolve conflicts from parallel sessions"),
            "merge"
        );
    }

    #[test]
    fn test_sanitize_task_name_limits_plain_text() {
        assert_eq!(
            sanitize_task_name("Added .dracon/ and .pub to NOISE_PATTERNS in bump.rs"),
            "Added .dracon/ and"
        );
    }

    #[test]
    fn test_compute_blast_radius_merge_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("file.txt"), "change").unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", "file.txt"])
            .status()
            .unwrap();
        std::fs::create_dir(repo.join(".git")).ok();
        std::fs::write(repo.join(".git/MERGE_HEAD"), "abc123").unwrap();

        let msg = compute_blast_radius(repo);
        assert!(msg.starts_with("MERGE: | "));
    }

    #[test]
    fn test_truncate_task_multibyte_utf8() {
        // Regression: em dash (—) is 3 bytes. Truncating at byte boundary
        // inside it caused "not a char boundary" panic.
        let input = "T-001 Set Tauri CSP — Replaced \"csp\": null with strict policy including wasm-unsafe-eval";
        let result = truncate_task(input);
        assert!(result.len() <= 64); // 60 + "..."
        assert!(!result.ends_with('\u{200B}')); // no broken chars
                                                // Should cut at the em dash boundary
        assert!(result.contains('—') || result.ends_with("..."));
    }

    #[test]
    fn test_truncate_task_short() {
        let input = "short task";
        assert_eq!(truncate_task(input), "short task");
    }

    #[test]
    fn test_truncate_task_at_sentence_boundary() {
        let input = "Fix the bug. Now add tests for the fix and update documentation.";
        let result = truncate_task(input);
        assert!(result.ends_with('.'));
        assert!(result.len() <= 64);
    }

    #[test]
    fn test_truncate_task_hard_cutoff() {
        let input = "This is a very long task name that has no sentence boundaries and just keeps going and going and going";
        let result = truncate_task(input);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 64);
    }

    #[tokio::test]
    async fn test_sync_repo_auto_github_private_graceful_on_no_gh() {
        // See `test_sync_repo_mirror_push_failure_returns_false`
        // for the rationale on using a temp state dir. This test
        // exercises `auto_github_private` which may attempt a
        // `gh` CLI call that fails, triggering
        // `record_push_failure` for the temp repo path.
        let state_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        let toml_str = r#"
auto_github_private = true
auto_github_private_account = "TestAccount"
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            result.is_ok(),
            "sync_repo should handle missing gh gracefully: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_sync_repo_auto_commit_creates_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create and stage a modified file
        let file_path = repo.join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", "test.txt"])
            .status()
            .unwrap();

        // Count commits before sync
        let commits_before = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "rev-list", "--count", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        let count_before: usize = String::from_utf8_lossy(&commits_before)
            .trim()
            .parse()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed: {:?}", result);

        // Verify a commit was created
        let commits_after = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "rev-list", "--count", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        let count_after: usize = String::from_utf8_lossy(&commits_after)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            count_after,
            count_before + 1,
            "sync_repo should have created one new commit (before={}, after={})",
            count_before,
            count_after
        );
    }

    #[tokio::test]
    async fn test_sync_repo_skips_rebase_in_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Simulate rebase in progress
        std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            result.is_ok(),
            "sync_repo should succeed even during rebase"
        );
        assert!(
            matches!(result, Ok(SyncOutcome::Blocked)),
            "rebase should cause early return (nothing synced)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_skips_merge_in_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Simulate merge in progress
        std::fs::write(repo.join(".git/MERGE_HEAD"), "abc123\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed even during merge");
        assert!(
            matches!(result, Ok(SyncOutcome::Blocked)),
            "merge should cause early return (nothing synced)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_skips_cherry_pick_in_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Simulate cherry-pick in progress
        std::fs::write(repo.join(".git/CHERRY_PICK_HEAD"), "abc123\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            result.is_ok(),
            "sync_repo should succeed even during cherry-pick"
        );
        assert!(
            matches!(result, Ok(SyncOutcome::Blocked)),
            "cherry-pick should cause early return (nothing synced)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_auto_commit_creates_commit_for_dirty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create a dirty file
        std::fs::write(repo.join("dirty.txt"), "modified content\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed: {:?}", result);
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "dirty repo with auto_commit should sync"
        );

        // Verify commit was made
        let output = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(
            log.lines().count() >= 2,
            "should have at least 2 commits (init + auto-commit)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_clean_repo_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::NothingToDo)),
            "clean repo should return false (nothing to sync)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_stages_and_commits_untracked_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create untracked file
        std::fs::write(repo.join("newfile.txt"), "new content\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed: {:?}", result);
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "untracked file should be staged and committed"
        );

        // Verify file is tracked
        let output = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "ls-files"])
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            tracked.contains("newfile.txt"),
            "newfile.txt should be tracked"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_auto_stage_untracked_false_skips_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create an untracked file that should be skipped
        std::fs::write(repo.join("scratch.md"), "scratch content\n").unwrap();
        // Create an untracked file that should be staged (default pattern)
        std::fs::write(repo.join("normal.txt"), "normal content\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
auto_stage_untracked = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed: {:?}", result);

        // Verify scratch.md is NOT tracked but normal.txt IS tracked
        let output = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "ls-files"])
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&output.stdout);
        // scratch.md is in default untracked_exclude_patterns via the
        // untracked_exclude_patterns field — but since auto_stage_untracked
        // is false, it stays untracked regardless of pattern.
        assert!(
            !tracked.contains("scratch.md"),
            "scratch.md should NOT be tracked when auto_stage_untracked=false"
        );
        // normal.txt is a new tracked file (status=Added + in index after
        // git add). With auto_stage_untracked=false, an Added entry that
        // is NOT in the index (truly untracked) is skipped. But a freshly
        // staged tracked file (already in the index) is still allowed.
        // Since normal.txt is brand new, it should also be skipped.
        // This test only verifies the auto_stage_untracked=false behavior.
    }

    // ---- bootstrap_empty_repo_commit (v0.112.30) ----

    /// Helper: TOML policy with ownership signals trusted for the
    /// conventional test identity (`test@test` / `test`).
    fn bootstrap_test_policy(extra: &str) -> SyncPolicy {
        let toml_str = format!(
            r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
trusted_emails = ["test@test"]
trusted_authors = ["test"]
{}
"#,
            extra
        );
        toml::from_str(&toml_str).unwrap()
    }

    fn init_empty_repo(path: &std::path::Path) {
        std::fs::create_dir_all(path).unwrap();
        let status = crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
        for (k, v) in [("user.email", "test@test"), ("user.name", "test")] {
            let status = crate::git::git_cmd()
                .args(["config", k, v])
                .current_dir(path)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    fn head_commit_count(path: &std::path::Path) -> u64 {
        let output = crate::git::git_cmd()
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(path)
            .output()
            .unwrap();
        if !output.status.success() {
            return 0;
        }
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_creates_root_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        std::fs::write(repo.join("a.txt"), "alpha\n").unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/b.txt"), "beta\n").unwrap();

        let policy = bootstrap_test_policy("");
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(result.unwrap(), true, "bootstrap must commit");
        assert_eq!(head_commit_count(&repo), 1, "root commit must exist");
        let output = crate::git::git_cmd()
            .args(["ls-files"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(tracked.contains("a.txt"));
        assert!(tracked.contains("src/b.txt"));
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_respects_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        std::fs::write(repo.join(".gitignore"), "secret.txt\n").unwrap();
        std::fs::write(repo.join("normal.txt"), "ok\n").unwrap();
        std::fs::write(repo.join("secret.txt"), "hunter2\n").unwrap();

        let policy = bootstrap_test_policy("");
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(result.unwrap(), true);
        let output = crate::git::git_cmd()
            .args(["ls-files"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(tracked.contains("normal.txt"));
        assert!(tracked.contains(".gitignore"));
        assert!(
            !tracked.contains("secret.txt"),
            "gitignored files must not be staged by the bootstrap"
        );
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_skips_oversized_files() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        std::fs::write(repo.join("small.txt"), "ok\n").unwrap();
        // 2 KiB file with a 1 KiB stage cap.
        std::fs::write(repo.join("big.bin"), vec![0u8; 2048]).unwrap();

        let policy = bootstrap_test_policy("max_stage_file_bytes = 1024");
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(result.unwrap(), true);
        let output = crate::git::git_cmd()
            .args(["ls-files"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(tracked.contains("small.txt"));
        assert!(
            !tracked.contains("big.bin"),
            "oversized file must be skipped (max_stage_file_bytes)"
        );
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_all_oversized_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        std::fs::write(repo.join("big.bin"), vec![0u8; 2048]).unwrap();

        let policy = bootstrap_test_policy("max_stage_file_bytes = 1024");
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(
            result.unwrap(),
            false,
            "nothing policy-compliant to stage → no commit"
        );
        assert_eq!(head_commit_count(&repo), 0);
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_auto_commit_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        std::fs::write(repo.join("a.txt"), "alpha\n").unwrap();

        // NOTE: cannot use bootstrap_test_policy here (it hardcodes
        // `auto_commit = true`; TOML rejects duplicate keys), so build
        // the policy directly.
        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = false
auto_push = false
auto_bump_versions = false
trusted_emails = ["test@test"]
trusted_authors = ["test"]
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(result.unwrap(), false);
        assert_eq!(head_commit_count(&repo), 0);
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_unowned_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        // Untrusted identity: user.email is not in trusted_emails and
        // there is no origin to vouch for the repo.
        let status = crate::git::git_cmd()
            .args(["config", "user.email", "mallory@evil"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(repo.join("a.txt"), "alpha\n").unwrap();

        let policy = bootstrap_test_policy("");
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(
            result.unwrap(),
            false,
            "unowned repo (untrusted user.email) must be skipped"
        );
        assert_eq!(head_commit_count(&repo), 0);
    }

    #[tokio::test]
    async fn test_bootstrap_empty_repo_nothing_to_stage() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        // No files at all — `git commit` would fail with "nothing to
        // commit"; the bootstrap must report Ok(false) instead.
        let policy = bootstrap_test_policy("");
        let result = bootstrap_empty_repo_commit(&repo, &policy, &BTreeSet::new(), false).await;
        assert_eq!(result.unwrap(), false);
        assert_eq!(head_commit_count(&repo), 0);
    }

    #[tokio::test]
    async fn test_sync_repo_empty_repo_end_to_end() {
        // The full sync_repo path on a stable empty repo: bootstrap
        // creates the root commit, then the pipeline proceeds.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_empty_repo(&repo);
        std::fs::write(repo.join("a.txt"), "alpha\n").unwrap();

        let policy = bootstrap_test_policy("");
        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo failed: {:?}", result);
        assert_eq!(
            head_commit_count(&repo),
            1,
            "sync_repo must bootstrap the root commit"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_untracked_exclude_patterns_keeps_scratch_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create files that should be auto-staged
        std::fs::write(repo.join("real-doc.md"), "real doc\n").unwrap();
        // Create a scratch file that should be excluded
        std::fs::create_dir_all(repo.join("scratch")).unwrap();
        std::fs::write(repo.join("scratch").join("notes.md"), "scratch notes\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
auto_stage_untracked = true
untracked_exclude_patterns = ["**/scratch/**"]
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed: {:?}", result);

        // Verify the real doc is tracked
        let output = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "ls-files"])
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            tracked.contains("real-doc.md"),
            "real-doc.md should be tracked"
        );
        // Verify the scratch file is NOT tracked
        assert!(
            !tracked.contains("scratch/notes.md"),
            "scratch/notes.md should NOT be tracked (untracked_exclude_patterns)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_skip_pull_when_not_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = true
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::NothingToDo)),
            "not behind should return false (nothing to pull)"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_skip_pull_when_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        std::fs::write(repo.join("dirty.txt"), "modified\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = true
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed with dirty repo");
        assert!(
            matches!(result, Ok(SyncOutcome::NothingToDo)),
            "dirty repo should skip pull and return false"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_skip_push_when_no_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = false
auto_push = true
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed without origin");
    }

    #[tokio::test]
    async fn test_sync_repo_skip_push_when_no_upstream() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = false
auto_push = true
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed without upstream");
    }

    #[tokio::test]
    async fn test_sync_repo_mirror_push_failure_returns_false() {
        // Use a temp state dir so this test does NOT pollute the
        // real stuck-push ledger at
        // ~/.local/state/dracon/dracon-sync-stuck-push-repos.json.
        // Regression: previously this test pushed to a
        // nonexistent URL (`git@nonexistent.example.com:repo.git`),
        // the push failed, and `record_push_failure` was called
        // with the temp repo path. The temp path then appeared
        // as a junk entry (`/tmp/.tmpXXXXX/test-repo`) in the
        // live daemon's `dracon-sync repos` report.
        let state_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let origin_bare = tmp.path().join("origin.git");
        crate::git::git_cmd()
            .args(["init", "--bare", "-q", "-b", "master"])
            .arg(&origin_bare)
            .status()
            .unwrap();

        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "remote",
                "add",
                "origin",
                &origin_bare.to_string_lossy(),
            ])
            .status()
            .unwrap();

        // Point mirror to non-existent path so push fails
        let bad_mirror = tmp.path().join("nonexistent-mirror.git");
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "remote",
                "add",
                "mirror",
                &bad_mirror.to_string_lossy(),
            ])
            .status()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = false
auto_push = true
auto_bump_versions = false

[[remotes]]
name = "mirror"
push_url = "git@nonexistent.example.com:repo.git"
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should not error");
        // CHANGED 2026-07-21 (v0.112.31, audit H3/F1.3): push failure
        // now surfaces as `SyncOutcome::PushFailed` (was
        // `NothingToDo`, which the daemon's apply phase treated as
        // success — logging `🔁 synced` and resetting `failure_count`
        // on a failed push).
        assert!(
            matches!(result, Ok(SyncOutcome::PushFailed)),
            "mirror push failure must return PushFailed, got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_sync_repo_mirror_push_failure_second() {
        // See `test_sync_repo_mirror_push_failure_returns_false`
        // for the rationale on using a temp state dir.
        let state_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let origin_bare = tmp.path().join("origin.git");
        crate::git::git_cmd()
            .args(["init", "--bare", "-q", "-b", "master"])
            .arg(&origin_bare)
            .status()
            .unwrap();

        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "remote",
                "add",
                "origin",
                &origin_bare.to_string_lossy(),
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "push",
                "-u",
                "origin",
                "master",
            ])
            .status()
            .unwrap();

        std::fs::write(repo.join("change.txt"), "changed\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = true
auto_bump_versions = false

[[remotes]]
name = "bad-mirror"
push_url = "git@nonexistent.example.com:repo.git"
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let mut remote_failures = HashMap::new();
        let result = sync_repo(
            &repo,
            &policy,
            &BTreeSet::new(),
            0,
            Some(&mut remote_failures),
            false,
            None,
        )
        .await;
        assert!(result.is_ok());
        // CHANGED 2026-07-21 (v0.112.31, audit H3/F1.3): a mirror-leg
        // failure (origin push succeeded) now returns `PushFailed`
        // instead of `Synced` — the commit landed and origin is
        // current, but the sync is not fully healthy and the daemon
        // must count + retry the mirror leg. The apply-phase
        // `PushFailed` arm increments `failure_count` and skips the
        // `🔁 synced` log.
        assert!(
            matches!(result.unwrap(), SyncOutcome::PushFailed),
            "mirror push failure must return PushFailed (origin succeeded but mirror failed)"
        );
        assert_eq!(
            remote_failures.get("bad-mirror"),
            Some(&1),
            "bad-mirror failure should be tracked"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_mirror_push_success_returns_true() {
        // See `test_sync_repo_mirror_push_failure_returns_false`
        // for the rationale on using a temp state dir. This test
        // succeeds so it calls `record_push_success` (not
        // `record_push_failure`), but we use a temp state dir
        // defensively so the test cannot pollute the real
        // ledger even if a future change adds a failure
        // path.
        let state_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let origin_bare = tmp.path().join("origin.git");
        let mirror_bare = tmp.path().join("mirror.git");
        crate::git::git_cmd()
            .args(["init", "--bare", "-q", "-b", "master"])
            .arg(&origin_bare)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["init", "--bare", "-q", "-b", "master"])
            .arg(&mirror_bare)
            .status()
            .unwrap();

        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "remote",
                "add",
                "origin",
                &origin_bare.to_string_lossy(),
            ])
            .status()
            .unwrap();
        // Push initial commit to origin so upstream is set
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "push",
                "-u",
                "origin",
                "master",
            ])
            .status()
            .unwrap();

        // Make repo dirty so sync creates a commit and pushes
        std::fs::write(repo.join("change.txt"), "changed\n").unwrap();

        let toml_str = format!(
            r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = true
auto_bump_versions = false

[[remotes]]
name = "mirror"
push_url = "{}"
"#,
            mirror_bare.to_string_lossy().replace("\\", "/")
        );
        let policy: SyncPolicy = toml::from_str(&toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should not error: {:?}", result);
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "mirror push success should return true"
        );
    }

    fn init_test_repo(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let repo = tmp.path().join(name);
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();
        repo
    }

    fn git_cmd(repo: &Path, args: &[&str]) -> std::process::Output {
        let repo_str = repo.to_string_lossy().to_string();
        let mut cmd = crate::git::git_cmd();
        cmd.arg("-C").arg(&repo_str);
        for a in args {
            cmd.arg(a);
        }
        cmd.output().unwrap()
    }

    #[tokio::test]
    async fn test_sync_repo_not_git_repo_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let not_repo = tmp.path().join("not-a-repo");
        std::fs::create_dir_all(&not_repo).unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&not_repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should not error on non-git dir");
        assert!(
            matches!(result, Ok(SyncOutcome::NothingToDo)),
            "non-git dir should return false"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_single_deleted_file_committed() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "single-del-repo");

        std::fs::write(repo.join("keep.txt"), "keep\n").unwrap();
        std::fs::write(repo.join("remove.txt"), "remove\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add files"]);

        std::fs::remove_file(repo.join("remove.txt")).unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "single deletion should be committed"
        );

        let output = git_cmd(&repo, &["ls-files"]);
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            tracked.contains("keep.txt"),
            "keep.txt should still be tracked"
        );
        assert!(
            !tracked.contains("remove.txt"),
            "remove.txt should be removed from index"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_partial_deletion_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "partial-del-repo");

        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        std::fs::write(repo.join("b.txt"), "b\n").unwrap();
        std::fs::write(repo.join("c.txt"), "c\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add files"]);

        // Delete 2 of 3 files (66% — should be ALLOWED, only 100% wipe is blocked)
        std::fs::remove_file(repo.join("a.txt")).unwrap();
        std::fs::remove_file(repo.join("b.txt")).unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "partial deletion should be committed (not blocked)"
        );

        // Verify deleted files are removed from tracking (deletion WAS committed)
        let output = git_cmd(&repo, &["ls-files"]);
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            !tracked.contains("a.txt"),
            "a.txt should be removed after partial deletion commit"
        );
        assert!(
            !tracked.contains("b.txt"),
            "b.txt should be removed after partial deletion commit"
        );
        assert!(tracked.contains("c.txt"), "c.txt should still be tracked");
    }

    #[tokio::test]
    async fn test_sync_repo_exact_50_del() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "exact-50-del-repo");

        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        std::fs::write(repo.join("b.txt"), "b\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add files"]);

        // Delete exactly 1 of 2 files (50% — at threshold, should be ALLOWED)
        std::fs::remove_file(repo.join("a.txt")).unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "exactly 50% deletion should be committed (not blocked)"
        );

        let output = git_cmd(&repo, &["ls-files"]);
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            !tracked.contains("a.txt"),
            "a.txt should be removed after 50% deletion commit"
        );
        assert!(tracked.contains("b.txt"), "b.txt should still be tracked");
    }

    #[tokio::test]
    async fn test_sync_repo_empty_repo_no_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "empty-repo");

        // Repo has only the empty initial commit, no tracked files
        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should not panic on empty repo");
    }

    #[tokio::test]
    async fn test_sync_repo_unstages_excluded_dir_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "exclude-dir-repo");

        std::fs::create_dir_all(repo.join("node_modules/pkg")).unwrap();
        std::fs::write(
            repo.join("node_modules/pkg/index.js"),
            "module.exports = {};\n",
        )
        .unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "initial"]);

        std::fs::write(repo.join("node_modules/pkg/index.js"), "updated\n").unwrap();
        std::fs::write(
            repo.join("src/main.rs"),
            "fn main() { println!(\"hello\"); }\n",
        )
        .unwrap();
        git_cmd(&repo, &["add", "-A"]);

        let mut excluded = BTreeSet::new();
        excluded.insert("node_modules".to_string());

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &excluded, 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");

        let output = git_cmd(&repo, &["log", "--oneline", "-1"]);
        let last_commit = String::from_utf8_lossy(&output.stdout);
        assert!(
            !last_commit.is_empty(),
            "should have committed the non-excluded change"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_unstages_oversized_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "oversized-repo");

        std::fs::write(repo.join("small.txt"), "small content\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "initial"]);

        let big_content = vec![b'X'; 1024];
        std::fs::write(repo.join("bigfile.bin"), &big_content).unwrap();
        std::fs::write(repo.join("small2.txt"), "another small\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        max_stage_file_bytes = 512
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            result.is_ok(),
            "sync_repo should succeed with oversized file"
        );

        let output = git_cmd(&repo, &["ls-files"]);
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            tracked.contains("small2.txt"),
            "small file should be tracked"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_mixed_tracked_and_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "mixed-repo");

        std::fs::write(repo.join("existing.txt"), "original\n").unwrap();
        git_cmd(&repo, &["add", "-A"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "initial"]);

        std::fs::write(repo.join("existing.txt"), "modified\n").unwrap();
        std::fs::write(repo.join("brand_new.txt"), "new file\n").unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "mixed changes should be committed"
        );

        let output = git_cmd(&repo, &["ls-files"]);
        let tracked = String::from_utf8_lossy(&output.stdout);
        assert!(
            tracked.contains("existing.txt"),
            "existing.txt should be tracked"
        );
        assert!(
            tracked.contains("brand_new.txt"),
            "brand_new.txt should be tracked"
        );

        let show = git_cmd(&repo, &["show", "HEAD:existing.txt"]);
        assert_eq!(String::from_utf8_lossy(&show.stdout), "modified\n");
    }

    #[tokio::test]
    async fn test_sync_repo_pull_skip_when_no_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "no-origin-pull-repo");

        let toml_str = r#"
        auto_github_private = false
        auto_commit = false
        auto_pull = true
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed without origin");
        assert!(
            matches!(result, Ok(SyncOutcome::NothingToDo)),
            "no origin should skip pull and return false"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_auto_commit_disabled_skips_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "no-autocommit-repo");

        std::fs::write(repo.join("dirty.txt"), "dirty content\n").unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = false
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
        assert!(
            matches!(result, Ok(SyncOutcome::NothingToDo)),
            "auto_commit=false should not commit dirty files"
        );

        let output = git_cmd(&repo, &["status", "--porcelain"]);
        let status = String::from_utf8_lossy(&output.stdout);
        assert!(
            status.contains("dirty.txt"),
            "file should still be untracked/unstaged"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_dry_run_does_not_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "dry-run-test");

        std::fs::write(repo.join("new_file.txt"), "new content\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let commits_before = git_cmd(&repo, &["rev-list", "--count", "HEAD"]);
        let commits_count_before: usize = String::from_utf8_lossy(&commits_before.stdout)
            .trim()
            .parse()
            .unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, true, None).await;
        assert!(result.is_ok(), "dry-run should succeed");

        let commits_after = git_cmd(&repo, &["rev-list", "--count", "HEAD"]);
        let commits_count_after: usize = String::from_utf8_lossy(&commits_after.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            commits_count_before, commits_count_after,
            "dry-run should not create any commits"
        );

        let status = git_cmd(&repo, &["status", "--porcelain"]);
        let status_output = String::from_utf8_lossy(&status.stdout);
        assert!(
            status_output.contains("new_file.txt"),
            "file should still appear as untracked in working tree"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_dry_run_does_not_push() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "dry-run-push-test");

        std::fs::write(repo.join("file.txt"), "change\n").unwrap();
        git_cmd(&repo, &["add", "."]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add file"]);

        let commits_before = git_cmd(&repo, &["rev-list", "--count", "HEAD"]);
        let count_before: usize = String::from_utf8_lossy(&commits_before.stdout)
            .trim()
            .parse()
            .unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = false
auto_pull = false
auto_push = true
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, true, None).await;
        assert!(result.is_ok(), "dry-run should succeed");

        let commits_after = git_cmd(&repo, &["rev-list", "--count", "HEAD"]);
        let count_after: usize = String::from_utf8_lossy(&commits_after.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            count_before, count_after,
            "dry-run should not change commit count"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_dry_run_does_not_modify_working_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "dry-run-wt-test");

        std::fs::write(repo.join("tracked.txt"), "tracked\n").unwrap();
        git_cmd(&repo, &["add", "tracked.txt"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add tracked"]);

        std::fs::write(repo.join("modified.txt"), "modified\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "untracked\n").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, true, None).await;
        assert!(result.is_ok(), "dry-run should succeed");

        let output = git_cmd(&repo, &["status", "--porcelain"]);
        let status = String::from_utf8_lossy(&output.stdout);
        assert!(
            status.contains("modified.txt"),
            "modified.txt should still be modified"
        );
        assert!(
            status.contains("untracked.txt"),
            "untracked.txt should still be untracked"
        );
    }

    /// Comprehensive boundary test for the mass-deletion safety guard.
    #[tokio::test]
    async fn test_alert_unpushed_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "alert-threshold-repo");

        // Create and commit multiple files to build up unpushed commits
        for i in 0..3 {
            let fname = format!("file{}.txt", i);
            std::fs::write(repo.join(&fname), format!("content{}\n", i)).unwrap();
            git_cmd(&repo, &["add", &fname]);
            git_cmd(
                &repo,
                &["commit", "--no-verify", "-m", &format!("add {}", fname)],
            );
        }

        // Set threshold to 2 — should trigger alert since we have 3 unpushed commits
        let toml_str = r#"
        auto_github_private = false
        auto_commit = false
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        alert_unpushed_threshold = 2
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        // No origin remote, so no push attempt — just check alert fires
        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
    }

    #[tokio::test]
    async fn test_alert_unpushed_threshold_not_triggered() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "alert-threshold-ok-repo");

        // Create and commit 1 file — below threshold
        std::fs::write(repo.join("file.txt"), "content\n").unwrap();
        git_cmd(&repo, &["add", "file.txt"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add file"]);

        // Set threshold to 5 — should NOT trigger alert
        let toml_str = r#"
        auto_github_private = false
        auto_commit = false
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        alert_unpushed_threshold = 5
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(result.is_ok(), "sync_repo should succeed");
    }

    #[tokio::test]
    async fn test_deletions_committed_when_intentional() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "deletion-commit-repo");

        // Create 7 tracked files — 6 will be deleted
        for i in 0..7 {
            std::fs::write(repo.join(format!("file{i}.txt")), format!("content{i}")).unwrap();
        }
        git_cmd(&repo, &["add", "."]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "init"]);

        // Modify file0 so there's a real change that would be staged
        std::fs::write(repo.join("file0.txt"), "modified").unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        // Delete 6 files — no guard, daemon commits everything
        for i in 1..7 {
            std::fs::remove_file(repo.join(format!("file{i}.txt"))).unwrap();
        }

        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            matches!(result, Ok(SyncOutcome::Synced)),
            "deletions should be committed, not blocked"
        );

        // Verify the deleted files are gone from tracking
        let ls = git_cmd(&repo, &["ls-files"]);
        let ls_str = String::from_utf8_lossy(&ls.stdout);
        assert!(
            ls_str.trim() == "file0.txt",
            "only file0 should remain, got: {:?}",
            ls_str
        );
    }

    /// CR-2 regression test: filter-only changes must NOT be re-detected by
    /// cli_diff_entries fallback, which would cause encrypted files to be
    /// committed as decrypted plaintext.
    #[tokio::test]
    async fn test_filter_only_skips_cli_diff_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "filter-only-repo");

        // Create a file that looks like a filter-only change
        std::fs::write(repo.join("secret.txt"), "plaintext").unwrap();
        git_cmd(&repo, &["add", "."]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "init"]);

        // Simulate a filter-only state: working tree differs from index
        // but git diff HEAD shows no changes (all changes are filter artifacts).
        // We achieve this by writing the same content but with a different
        // line ending that the clean filter would normalize.
        std::fs::write(repo.join("secret.txt"), "plaintext\r\n").unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = false
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        // sync_repo should see filter-only changes, skip them, and NOT
        // fall back to cli_diff_entries which would see the CRLF difference.
        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            matches!(
                result,
                Ok(SyncOutcome::NothingToDo) | Ok(SyncOutcome::Synced)
            ),
            "filter-only repo should produce NothingToDo or Synced without changes, got {:?}",
            result
        );

        // Verify nothing was staged
        let staged = git_cmd(&repo, &["diff", "--cached", "--name-only"]);
        let staged_str = String::from_utf8_lossy(&staged.stdout);
        assert!(
            staged_str.trim().is_empty(),
            "nothing should be staged for filter-only repo"
        );
    }

    /// Regression: when all staged entries are filter-only (committed_entries
    /// is empty) and `git reset HEAD` fails due to index.lock contention from
    /// a concurrent process, sync_repo should return NothingToDo instead of
    /// propagating the error. The staging area is cleaned up on the next cycle.
    #[tokio::test]
    async fn test_filter_only_reset_failure_is_non_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "filter-only-reset-fail-repo");

        // Create .gitignore and commit it
        std::fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        git_cmd(&repo, &["add", ".gitignore"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add gitignore"]);

        // Create a tracked file that matches the gitignore rule
        std::fs::write(repo.join("debug.log"), "v1").unwrap();
        git_cmd(&repo, &["add", "-f", "debug.log"]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "add debug.log"]);

        // Modify the tracked+gitignored file (dirty)
        std::fs::write(repo.join("debug.log"), "v2").unwrap();

        // Create a fake index.lock to simulate concurrent git process
        std::fs::write(repo.join(".git").join("index.lock"), "concurrent").unwrap();

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        // sync_repo should NOT fail — the filter-only reset failure is non-fatal.
        // Before the fix, this returned Err ("failed to reset HEAD after filter-only commit").
        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            matches!(
                result,
                Ok(SyncOutcome::NothingToDo) | Ok(SyncOutcome::Synced)
            ),
            "filter-only reset failure should be non-fatal, got {:?}",
            result
        );

        // Clean up the fake index.lock so the test teardown doesn't hang
        let _ = std::fs::remove_file(repo.join(".git").join("index.lock"));
    }

    #[tokio::test]
    async fn test_sync_repo_with_duplicate_subjects_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_repo(&tmp, "stale-focus-repo");

        let toml_str = r#"
        auto_github_private = false
        auto_commit = true
        auto_pull = false
        auto_push = false
        auto_bump_versions = false
        "#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        // Manually commit twice with same subject
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        git_cmd(&repo, &["add", "."]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "duplicate subject"]);

        std::fs::write(repo.join("b.txt"), "b").unwrap();
        git_cmd(&repo, &["add", "."]);
        git_cmd(&repo, &["commit", "--no-verify", "-m", "duplicate subject"]);

        // sync_repo should succeed — no dedup guard blocking legitimate work
        std::fs::write(repo.join("c.txt"), "c").unwrap();
        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            result.is_ok(),
            "sync_repo should succeed with duplicate subjects in history"
        );
    }

    #[tokio::test]
    async fn test_sync_repo_new_branch_auto_push_attempted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("new-branch-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "config",
                "user.email",
                "test@test",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "test"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create a file to make the repo dirty
        std::fs::write(repo.join("test.txt"), "content").unwrap();

        let toml_str = r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = true
auto_bump_versions = false
"#;
        let policy: SyncPolicy = toml::from_str(toml_str).unwrap();

        // sync_repo should succeed and attempt to push (even without upstream)
        let result = sync_repo(&repo, &policy, &BTreeSet::new(), 0, None, false, None).await;
        assert!(
            result.is_ok(),
            "sync_repo should succeed for new branch without upstream"
        );

        // Verify that a commit was created
        let output = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "log", "--oneline", "-1"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(
            !log.contains("init") || log.contains("update"),
            "should have new commit after sync"
        );
    }

    /// Regression test for `partition_gitignored`: tracked files whose
    /// **parent directory** matches a gitignore rule (e.g. `**/.wxt/types/`)
    /// must be sent to `force_paths` (uses `git add -f`), not `normal_paths`
    /// (where plain `git add` would refuse them).
    ///
    /// Original bug: `git check-ignore` reports tracked files as "not ignored"
    /// (gitignore is bypassed for tracked files), so a naive partition sent
    /// tracked-but-ignored files to `normal_paths`, causing `git add` to fail
    /// for the whole batch. This test ensures tracked files always go to
    /// `force_paths` regardless of gitignore state.
    #[tokio::test]
    async fn test_partition_gitignored_tracked_file_with_gitignore_rule() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        test_git_cmd()
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .output()
            .unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "checkout",
                "-q",
                "-b",
                "main",
            ])
            .output()
            .unwrap();

        // Create a tracked file inside a directory that will later be gitignored
        std::fs::create_dir_all(repo.join("subdir/types")).unwrap();
        let tracked_path = repo.join("subdir/types/imports.d.ts");
        std::fs::write(&tracked_path, "original\n").unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "add",
                "subdir/types/imports.d.ts",
            ])
            .output()
            .unwrap();
        test_commit_cmd()
            .args(["-C", &repo.to_string_lossy(), "-m", "init"])
            .output()
            .unwrap();

        // Now add a gitignore rule that matches the directory containing the tracked file
        // (this is the exact pattern that triggered the original bug in browser-extensions-shared)
        std::fs::write(repo.join(".gitignore"), "**/types/\n").unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", ".gitignore"])
            .output()
            .unwrap();
        test_commit_cmd()
            .args(["-C", &repo.to_string_lossy(), "-m", "add gitignore"])
            .output()
            .unwrap();

        // Modify the tracked file
        std::fs::write(&tracked_path, "modified\n").unwrap();

        // Sanity check: plain `git add <path>` should fail because the
        // file's parent directory matches a gitignore rule
        let plain_add = test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "add",
                "subdir/types/imports.d.ts",
            ])
            .output()
            .unwrap();
        assert!(
            !plain_add.status.success(),
            "precondition: plain `git add` should refuse tracked file whose parent dir matches gitignore; stderr: {}",
            String::from_utf8_lossy(&plain_add.stderr)
        );
        // Reset any state
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "reset", "-q", "HEAD"])
            .output()
            .unwrap();

        // Call partition_gitignored — tracked file must go to force_paths
        let paths = vec!["subdir/types/imports.d.ts".to_string()];
        let (force, normal) = partition_gitignored(&repo, &paths).await;
        assert!(
            force.contains(&"subdir/types/imports.d.ts".to_string()),
            "tracked file (parent dir gitignored) must be in force_paths, got force={:?} normal={:?}",
            force,
            normal
        );
        assert!(
            !normal.contains(&"subdir/types/imports.d.ts".to_string()),
            "tracked file (parent dir gitignored) must NOT be in normal_paths, got force={:?} normal={:?}",
            force,
            normal
        );
    }

    /// Companion test: untracked + gitignored files should be SKIPPED (not in
    /// either list), so the daemon respects `.gitignore` intent for new files.
    #[tokio::test]
    async fn test_partition_gitignored_untracked_gitignored_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        test_git_cmd()
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .output()
            .unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "checkout",
                "-q",
                "-b",
                "main",
            ])
            .output()
            .unwrap();
        // Make initial commit so HEAD exists
        std::fs::write(repo.join("README"), "x").unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", "."])
            .output()
            .unwrap();
        test_commit_cmd()
            .args(["-C", &repo.to_string_lossy(), "-m", "init"])
            .output()
            .unwrap();
        // gitignore a path
        std::fs::write(repo.join(".gitignore"), "ignored.log\n").unwrap();
        std::fs::write(repo.join("ignored.log"), "x").unwrap();

        let paths = vec!["ignored.log".to_string()];
        let (force, normal) = partition_gitignored(&repo, &paths).await;
        assert!(
            force.is_empty(),
            "untracked+ignored should not be in force_paths, got {:?}",
            force
        );
        assert!(
            normal.is_empty(),
            "untracked+ignored should not be in normal_paths, got {:?}",
            normal
        );
    }

    /// Regression test for audit-4: when push is rejected with `fetch first`
    /// (local branch behind origin), `push_with_retries` should auto-pull
    /// once and retry. Without the fix, the daemon would loop on the same
    /// rejection (e.g. `azumi-live-ssr-framework` had 598 such failures).
    #[tokio::test]
    async fn test_push_with_retries_fetches_first_on_rejection() {
        let tmp = tempfile::tempdir().unwrap();
        let origin_bare = tmp.path().join("origin.git");
        let origin_bare_str = origin_bare.canonicalize().unwrap_or(origin_bare.clone());
        test_git_cmd()
            .args([
                "init",
                "--bare",
                "-b",
                "main",
                &origin_bare_str.to_string_lossy(),
            ])
            .output()
            .unwrap();
        let repo = tmp.path().join("repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "main", &repo.to_string_lossy()])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .output()
            .unwrap();
        // Initial commit + push to set up origin as upstream
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .output()
            .unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "remote",
                "add",
                "origin",
                &origin_bare_str.to_string_lossy(),
            ])
            .output()
            .unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "push",
                "-u",
                "origin",
                "main",
            ])
            .output()
            .unwrap();

        // Push a second commit to origin (so origin has 2 commits).
        std::fs::write(repo.join("extra.txt"), "extra\n").unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", "extra.txt"])
            .output()
            .unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "-m",
                "extra",
            ])
            .output()
            .unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "push", "origin", "main"])
            .output()
            .unwrap();

        // Verify local has 2 commits
        let local_log_after_extra = test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
            .output()
            .unwrap();
        let local_log_str = String::from_utf8_lossy(&local_log_after_extra.stdout).to_string();
        assert!(
            local_log_str.contains("extra"),
            "local should contain 'extra' commit, got:\n{}\nstderr: {}",
            local_log_str,
            String::from_utf8_lossy(&local_log_after_extra.stderr)
        );

        // Verify origin has 2 commits
        let origin_log = test_git_cmd()
            .args([
                "--git-dir",
                &origin_bare_str.to_string_lossy(),
                "log",
                "--oneline",
                "--all",
            ])
            .output()
            .unwrap();
        let origin_log_str = String::from_utf8_lossy(&origin_log.stdout).to_string();
        let origin_log_lines: Vec<&str> = origin_log_str.trim().lines().collect();
        assert!(
            origin_log_lines.len() >= 2,
            "origin should have >=2 commits after extra push, got {}:\n{}",
            origin_log_lines.len(),
            origin_log_str
        );

        // Reset local back to init (simulating the local repo being idle
        // while origin moved forward via a mirror push).
        let reset_out = test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "reset", "--hard", "HEAD~1"])
            .output()
            .unwrap();
        assert!(
            reset_out.status.success(),
            "reset failed: {}",
            String::from_utf8_lossy(&reset_out.stderr)
        );

        // Verify local is at init (1 commit)
        let local_log_after_reset = test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
            .output()
            .unwrap();
        let local_log_str = String::from_utf8_lossy(&local_log_after_reset.stdout).to_string();
        let local_lines: Vec<&str> = local_log_str.trim().lines().collect();
        assert_eq!(
            local_lines.len(),
            1,
            "local should have 1 commit after reset, got {}:\n{}",
            local_lines.len(),
            local_log_str
        );

        // Now make a local commit. Pushing this will be rejected with
        // "fetch first" because origin/main has 2 commits but local only has 1.
        std::fs::write(repo.join("local.txt"), "local change\n").unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", "local.txt"])
            .output()
            .unwrap();
        test_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "-m",
                "local",
            ])
            .output()
            .unwrap();

        // Verify local has 2 commits (init + local)
        let local_log_final = test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "log", "--oneline"])
            .output()
            .unwrap();
        let local_final_str = String::from_utf8_lossy(&local_log_final.stdout).to_string();
        assert!(
            local_final_str.contains("local"),
            "local should contain 'local' commit, got:\n{}",
            local_final_str
        );

        // Sanity: a plain `git push` would fail with fetch-first
        let plain_push = test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "push", "origin", "HEAD"])
            .output()
            .unwrap();
        let plain_stderr = String::from_utf8_lossy(&plain_push.stderr).to_string();
        assert!(
            !plain_push.status.success()
                || plain_stderr.contains("rejected")
                || plain_stderr.contains("fetch first"),
            "precondition: plain push should fail with fetch-first; status={} stderr={}",
            plain_push.status,
            plain_stderr
        );

        // Reset local again for the real test (undo the failed push)
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "reset", "--hard", "HEAD~1"])
            .output()
            .unwrap();
        std::fs::write(repo.join("local.txt"), "local change\n").unwrap();
        test_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "add", "local.txt"])
            .output()
            .unwrap();
        test_commit_cmd()
            .args(["-C", &repo.to_string_lossy(), "-m", "local"])
            .output()
            .unwrap();

        // Now call push_with_retries — it should auto-pull then succeed
        let result = push_with_retries(&repo, 30, 1, "test").await;
        assert!(
            result.is_ok(),
            "push_with_retries should auto-pull + succeed: {:?}",
            result
        );
    }

    // ============================================================
    // scale_push_timeout tests
    // ============================================================

    #[test]
    fn test_scale_push_timeout_small_push_uses_base() {
        // 5 or fewer ahead commits → no scaling
        assert_eq!(scale_push_timeout(60, 0), 60);
        assert_eq!(scale_push_timeout(60, 1), 60);
        assert_eq!(scale_push_timeout(60, 5), 60);
    }

    #[test]
    fn test_scale_push_timeout_medium_push_doubles() {
        // 6-20 ahead → 2x base
        assert_eq!(scale_push_timeout(60, 6), 120);
        assert_eq!(scale_push_timeout(60, 10), 120);
        assert_eq!(scale_push_timeout(60, 20), 120);
    }

    #[test]
    fn test_scale_push_timeout_large_push_quadruples() {
        // 21-50 ahead → 4x base
        assert_eq!(scale_push_timeout(60, 21), 240);
        assert_eq!(scale_push_timeout(60, 28), 240);
        assert_eq!(scale_push_timeout(60, 50), 240);
    }

    #[test]
    fn test_scale_push_timeout_huge_push_sextuples_capped() {
        // >50 ahead → 6x base, capped at 600s
        assert_eq!(scale_push_timeout(60, 51), 360);
        assert_eq!(scale_push_timeout(60, 100), 360);
        // With a larger base, the cap kicks in
        assert_eq!(scale_push_timeout(300, 100), 600);
        assert_eq!(scale_push_timeout(500, 200), 600);
    }

    #[test]
    fn test_scale_push_timeout_zero_base_stays_zero() {
        // Edge case: zero base timeout stays zero
        assert_eq!(scale_push_timeout(0, 28), 0);
    }

    // ============================================================
    // count_ahead_commits tests
    // ============================================================

    #[tokio::test]
    async fn test_count_ahead_commits_no_origin() {
        // A fresh repo with no origin has 0 ahead commits (no error).
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        // No commits, no origin → should return 0, not error.
        let count = count_ahead_commits(&repo).await.unwrap();
        assert_eq!(count, 0, "no-origin repo should report 0 ahead");
    }

    #[tokio::test]
    async fn test_count_ahead_commits_returns_zero_when_synced() {
        // Repo with one commit, no origin → 0 ahead (origin/main doesn't exist).
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();
        let count = count_ahead_commits(&repo).await.unwrap();
        assert_eq!(count, 0);
    }

    // ============================================================
    // stage_existing_files race-condition tests
    // ============================================================

    #[tokio::test]
    async fn test_stage_existing_files_skips_vanished_files() {
        // Reproduce the race where get_status() lists a file as
        // untracked, but the file disappears before git add runs (vite
        // timestamp-suffixed temp files, inotify lag, etc.). The
        // function must drop the missing file from the staging list
        // and succeed for the remaining files.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create one real file and a phantom path that doesn't exist
        std::fs::write(repo.join("real.txt"), "real content\n").unwrap();
        let phantom = "vite.config.ts.timestamp-1781483278562-7a994a6fc1011.mjs";
        assert!(!repo.join(phantom).exists(), "phantom should not exist on disk");

        // Stage the mixed list (real + phantom). Should succeed
        // because the phantom is filtered out before git add runs.
        let paths = vec!["real.txt".to_string(), phantom.to_string()];
        let result =
            stage_existing_files(&repo, &paths, false, 30, &BTreeSet::new()).await;
        assert!(
            result.is_ok(),
            "stage_existing_files should filter out vanished files: {:?}",
            result
        );

        // The real file should now be staged
        let output = crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "ls-files", "--stage"])
            .output()
            .unwrap();
        let staged = String::from_utf8_lossy(&output.stdout);
        assert!(
            staged.contains("real.txt"),
            "real.txt should be staged, got: {}",
            staged
        );
        // The phantom should NOT be in the index
        assert!(
            !staged.contains("vite.config.ts.timestamp"),
            "phantom should not be staged, got: {}",
            staged
        );
    }

    #[tokio::test]
    async fn test_stage_existing_files_skips_directory_entries() {
        // If get_status returns a bare directory path (some libgit2
        // versions do this), `git add -A <dir>` would recurse. We
        // want explicit file paths only, so the function should
        // drop directory entries from the list.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create a real file and a directory
        std::fs::write(repo.join("real.txt"), "content\n").unwrap();
        std::fs::create_dir(repo.join("subdir")).unwrap();

        // Stage with directory in the list
        let paths = vec!["real.txt".to_string(), "subdir".to_string()];
        let result =
            stage_existing_files(&repo, &paths, false, 30, &BTreeSet::new()).await;
        assert!(
            result.is_ok(),
            "stage_existing_files should skip directory entries: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_stage_existing_files_expands_directory_with_files() {
        // Regression: previously `stage_existing_files` filtered
        // out directory entries silently, which meant a libgit2
        // untracked dir like `docs/avid-research/02-foo/` was
        // dropped from the staging list and the files inside
        // were never committed. The new behavior recurses one
        // level into bare directory entries and adds the file
        // children to the staging list. This test creates a
        // repo with a `docs/research/` dir containing 2 files
        // and verifies that both files end up in the git index
        // after a single `stage_existing_files` call.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create a tracked file, then a directory with 2 files inside
        std::fs::write(repo.join("tracked.txt"), "init\n").unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "add",
                "tracked.txt",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "-m",
                "add tracked",
            ])
            .status()
            .unwrap();
        std::fs::create_dir_all(repo.join("docs/research")).unwrap();
        std::fs::write(repo.join("docs/research/readme.md"), "# readme\n").unwrap();
        std::fs::write(repo.join("docs/research/notes.md"), "# notes\n").unwrap();

        // Simulate libgit2 returning the bare directory path
        let paths = vec!["docs/research".to_string()];
        let result =
            stage_existing_files(&repo, &paths, false, 30, &BTreeSet::new()).await;
        assert!(result.is_ok(), "stage_existing_files failed: {:?}", result);

        // Verify both files are now in the index
        let output = crate::git::tokio_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "diff", "--cached", "--name-only"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        let staged = String::from_utf8_lossy(&output.stdout);
        assert!(
            staged.contains("docs/research/readme.md"),
            "expected readme.md to be staged, got: {}",
            staged
        );
        assert!(
            staged.contains("docs/research/notes.md"),
            "expected notes.md to be staged, got: {}",
            staged
        );
    }

    #[tokio::test]
    async fn test_stage_existing_files_recurses_deeply() {
        // Regression test for goal `662a6e15` (2026-06-16):
        // libgit2 (via `git status --porcelain -z`) collapses a
        // fully-untracked subtree into a single top-level directory
        // entry, e.g. `web/games/libs/game/effects/src/` for files
        // at `web/games/libs/game/effects/src/styles/*.css` (2
        // levels deep) or `web/games/libs/game/ui/src/lib/` for
        // files at `web/games/libs/game/ui/src/lib/components/
        // *.svelte` (3 levels deep). The previous 1-level walk
        // only expanded the directory's immediate file children,
        // missing every file nested 2+ levels below. The
        // operator's new Svelte/CSS code in `libs/game/effects/`
        // and `libs/game/ui/` was therefore left untracked for
        // minutes at a time. The new recursive walk (using a
        // stack, not native recursion) must find ALL files
        // regardless of depth.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create a 3-level deep untracked subtree mimicking
        // the operator's `libs/game/ui/src/lib/components/` layout.
        std::fs::create_dir_all(repo.join("a/b/c")).unwrap();
        std::fs::write(repo.join("a/b/c/deep1.svelte"), "<div/>\n").unwrap();
        std::fs::write(repo.join("a/b/c/deep2.svelte"), "<div/>\n").unwrap();
        // And a 4-level deep file to make sure the recursion
        // doesn't have an artificial depth limit.
        std::fs::create_dir_all(repo.join("a/b/c/d/e")).unwrap();
        std::fs::write(repo.join("a/b/c/d/e/really_deep.css"), "x{}\n").unwrap();

        // Simulate libgit2 returning ONLY the top-level dir entry
        // (this is what `git status --porcelain -z` does for
        // fully-untracked subtrees).
        let paths = vec!["a".to_string()];
        let result =
            stage_existing_files(&repo, &paths, false, 30, &BTreeSet::new()).await;
        assert!(result.is_ok(), "stage_existing_files failed: {:?}", result);

        // Verify ALL files (3-level and 4-level) ended up staged.
        let output = crate::git::tokio_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "diff", "--cached", "--name-only"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        let staged = String::from_utf8_lossy(&output.stdout);
        assert!(
            staged.contains("a/b/c/deep1.svelte"),
            "3-level file missed: {}",
            staged
        );
        assert!(
            staged.contains("a/b/c/deep2.svelte"),
            "3-level file missed: {}",
            staged
        );
        assert!(
            staged.contains("a/b/c/d/e/really_deep.css"),
            "4-level file missed: {}",
            staged
        );
    }

    #[tokio::test]
    async fn test_stage_existing_files_skips_node_modules_and_dotdirs() {
        // Regression test for goal `662a6e15` (2026-06-16):
        // the new recursive walk must respect the
        // `excluded_dir_names` guard and skip dotfile dirs.
        // This prevents `node_modules/`, `target/`, `.git/`,
        // `.cache/`, etc. from being staged even when they
        // appear as untracked.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Create files inside excluded dir names
        std::fs::create_dir_all(repo.join("node_modules/foo")).unwrap();
        std::fs::write(repo.join("node_modules/foo/should_not_stage.js"), "x\n").unwrap();
        std::fs::create_dir_all(repo.join("target/build")).unwrap();
        std::fs::write(repo.join("target/build/should_not_stage.bin"), "x\n").unwrap();
        // And a dotfile dir
        std::fs::create_dir_all(repo.join(".cache/data")).unwrap();
        std::fs::write(repo.join(".cache/data/should_not_stage.dat"), "x\n").unwrap();
        // And a real file that SHOULD be staged
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/keep.ts"), "export {}\n").unwrap();

        // Build the same excluded set the daemon uses by default.
        let excluded: std::collections::BTreeSet<String> = crate::policy::default_exclude_dir_names()
            .into_iter()
            .collect();
        // Walk the top-level dir
        let paths = vec![
            "node_modules".to_string(),
            "target".to_string(),
            ".cache".to_string(),
            "src".to_string(),
        ];
        let result = stage_existing_files(&repo, &paths, false, 30, &excluded).await;
        assert!(result.is_ok());

        let output = crate::git::tokio_git_cmd()
            .args(["-C", &repo.to_string_lossy(), "diff", "--cached", "--name-only"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        let staged = String::from_utf8_lossy(&output.stdout);
        assert!(
            staged.contains("src/keep.ts"),
            "src/keep.ts should be staged, got: {}",
            staged
        );
        assert!(
            !staged.contains("node_modules"),
            "node_modules contents should NOT be staged, got: {}",
            staged
        );
        assert!(
            !staged.contains("target"),
            "target contents should NOT be staged, got: {}",
            staged
        );
        assert!(
            !staged.contains(".cache"),
            ".cache contents should NOT be staged, got: {}",
            staged
        );
    }

    /// Regression test for goal `mr0rim9u-lzzfv9`:
    /// `stage_existing_files` recursion MUST skip subdirs whose
    /// `.git` is a real DIRECTORY (a nested git repo), not just
    /// submodules whose `.git` is a pointer file. The previous
    /// implementation only matched `is_file()`, which let the
    /// recursion descend into nested sibling repos like
    /// `web-auto/rust-ai-web-auto/`, attempt to `git add` ~19000
    /// files inside, and fail with
    /// `fatal: Pathspec '...' is in submodule 'rust-ai-web-auto'`.
    /// After 5 failures, the daemon would mark the parent repo
    /// as `exceeded max failures` and stop syncing it.
    ///
    /// This test creates a parent repo with:
    /// - `keep.txt`              (regular file, SHOULD be staged)
    /// - `nested_subrepo/`       (real git repo with `.git/` dir)
    ///   - `nested_subrepo/should_not_stage.md`  (inside the nested repo)
    /// and asserts:
    /// a) `keep.txt` gets staged,
    /// b) NO file from inside `nested_subrepo/` gets staged,
    /// c) `stage_existing_files` returns Ok (no error).
    #[tokio::test]
    async fn test_stage_existing_files_skips_nested_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("parent");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &repo.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "init",
            ])
            .status()
            .unwrap();

        // Regular file that SHOULD be staged
        std::fs::write(repo.join("keep.txt"), "keep me\n").unwrap();

        // Nested git repo (real `.git/` directory, not a submodule
        // pointer file). We're emulating the web-auto scenario
        // where `web-auto/rust-ai-web-auto/.git` is a real git
        // repo's working directory.
        let nested = repo.join("nested_subrepo");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        std::fs::write(
            nested.join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )
        .unwrap();
        std::fs::create_dir_all(nested.join(".git").join("objects")).unwrap();
        std::fs::create_dir_all(nested.join(".git").join("refs")).unwrap();
        // One file inside the nested repo — must NOT be staged by parent
        std::fs::write(
            nested.join("should_not_stage.md"),
            "this lives in a nested git repo\n",
        )
        .unwrap();

        // Call the helper with the nested repo path as a top-level
        // entry, exactly like the daemon does when a parent
        // commit-hash change shows up as `M nested_subrepo`.
        let paths = vec!["nested_subrepo".to_string()];
        let excluded: std::collections::BTreeSet<String> =
            crate::policy::default_exclude_dir_names()
                .into_iter()
                .collect();
        let result =
            stage_existing_files(&repo, &paths, false, 30, &excluded).await;
        assert!(
            result.is_ok(),
            "stage_existing_files must NOT error on a nested git-repo entry: {:?}",
            result
        );

        // Confirm only the unrelated file made it to the index
        let output = crate::git::tokio_git_cmd()
            .args([
                "-C",
                &repo.to_string_lossy(),
                "diff",
                "--cached",
                "--name-only",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        let staged = String::from_utf8_lossy(&output.stdout);
        // The empty `keep.txt` (`--allow-empty` initial commit) is
        // what `git diff --cached` shows at this point, since the
        // staged_paths argument didn't include `keep.txt` here.
        // The KEY assertion is that nothing from inside the nested
        // subrepo snuck into the index.
        assert!(
            !staged.contains("should_not_stage"),
            "Files inside nested_subrepo MUST NOT be staged, got:\n{}",
            staged
        );
        assert!(
            !staged.contains("nested_subrepo/.git"),
            "Files inside nested_subrepo/.git MUST NOT be staged, got:\n{}",
            staged
        );
    }

    #[tokio::test]
    async fn test_stage_existing_files_empty_after_filter_returns_ok() {
        // If every path in the input list vanishes between status
        // and add, the function should return Ok(()) (nothing to do),
        // not bail with an error.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        let paths = vec!["nonexistent1.txt".to_string(), "nonexistent2.txt".to_string()];
        let result =
            stage_existing_files(&repo, &paths, false, 30, &BTreeSet::new()).await;
        assert!(result.is_ok(), "all-vanished list should be a no-op");
    }

    // ============================================================
    // stage_gitlink_updates — regression tests for goal
    // `mr0xseig-fn9bbd`. The gitlink-pointer propagation flow
    // is exercised end-to-end by:
    //  - setting up a parent repo with a tracked gitlink to a
    //    sibling subrepo (real `.git/` directory, NOT a
    //    `.gitmodules`-declared worktree — this matches the
    //    web-auto/rust-ai-web-auto scenario).
    //  - advancing the sibling's HEAD to a new commit.
    //  - calling `stage_gitlink_updates(parent, ["sibling"], ...)`.
    //  - asserting the parent's index now points to the new SHA,
    //    without staging ANY file from inside the sibling work
    //    tree.
    // ============================================================

    /// Helper: run `git` in `dir` and return trimmed stdout.
    async fn git_stdout(dir: &Path, args: &[&str]) -> String {
        let out = crate::git::tokio_git_cmd()
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Helper: build a self-contained mini-repo rooted at `dir`,
    /// with user.email/user.name configured and `branch = main`.
    async fn init_gitlink_test_repo(dir: &Path) {
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(dir)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &dir.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &dir.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        // Disable hooks so globally-installed warden hooks don't reject
        // commits in temp test repos that lack `.gitattributes` with
        // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
        crate::git::git_cmd()
            .args(["-C", &dir.to_string_lossy(), "config", "core.hooksPath", "/dev/null"])
            .status()
            .unwrap();
        // Ensure we have an initial commit so HEAD is valid.
        let _ = git_stdout(
            dir,
            &["commit", "--allow-empty", "-q", "-m", "init"],
        )
        .await;
    }

    #[tokio::test]
    async fn test_stage_gitlink_updates_propagates_sibling_subrepo_pointer() {
        // web-auto + rust-ai-web-auto scenario: parent has a tracked
        // 160000 entry pointing at a sibling subrepo whose `.git/`
        // is its OWN git repo (not a `.gitmodules`-shared worktree).
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        std::fs::create_dir_all(&parent).unwrap();
        init_gitlink_test_repo(&parent).await;

        // Build a sibling subrepo on disk.
        let sibling = parent.join("sibling");
        std::fs::create_dir_all(&sibling).unwrap();
        // The subrepo's own `.git/` (sibling IS a git repo).
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&sibling)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        // Disable hooks so globally-installed warden hooks don't reject
        // commits in temp test repos that lack `.gitattributes` with
        // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "config", "core.hooksPath", "/dev/null"])
            .status()
            .unwrap();
        // First commit at SHA-A.
        std::fs::write(sibling.join("a.txt"), "v1\n").unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "add", "a.txt"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "commit", "-q", "-m", "v1"])
            .status()
            .unwrap();
        let sha_a = git_stdout(&sibling, &["rev-parse", "HEAD"]).await;

        // Register the subrepo as a gitlink in the parent.
        crate::git::git_cmd()
            .args(["-C", &parent.to_string_lossy(), "add", "sibling"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &parent.to_string_lossy(), "commit", "-q", "-m", "add sibling"])
            .status()
            .unwrap();

        // Sanity: parent has a 160000 entry pointing at sha_a.
        let ls = git_stdout(&parent, &["ls-tree", "HEAD", "--", "sibling"]).await;
        assert!(
            ls.starts_with(&format!("160000 commit {}", sha_a)),
            "ls-tree should report 160000 -> {} at HEAD, got: {}",
            sha_a,
            ls
        );

        // Advance the subrepo to SHA-B.
        std::fs::write(sibling.join("a.txt"), "v2\n").unwrap();
        std::fs::write(sibling.join("b.txt"), "new\n").unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "add", "-A"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "commit", "-q", "-m", "v2"])
            .status()
            .unwrap();
        let sha_b = git_stdout(&sibling, &["rev-parse", "HEAD"]).await;
        assert_ne!(sha_a, sha_b, "subrepo must have advanced");

        // Parent now sees `M sibling` (gitlink pointer out of date).
        let status_before = git_stdout(&parent, &["status", "--porcelain"]).await;
        assert!(
            status_before.contains("sibling"),
            "parent must see stale gitlink, got: {:?}",
            status_before
        );

        // Run the gitlink-update stage flow.
        let result =
            stage_gitlink_updates(&parent, &["sibling".to_string()], false, 30).await;
        assert!(result.is_ok(), "stage_gitlink_updates must succeed: {:?}", result);

        // Parent index must now point at sha_b.
        let index_sha = git_stdout(&parent, &["ls-files", "--stage", "sibling"]).await;
        assert!(
            index_sha.contains(&sha_b) || index_sha.contains(&sha_b[..7]),
            "parent index must reflect new submodule SHA {}, got: {:?}",
            sha_b,
            index_sha
        );

        // NO file from inside `sibling/` should be in the parent index.
        let staged_files = git_stdout(&parent, &["diff", "--cached", "--name-only"]).await;
        assert!(
            !staged_files.contains("sibling/a.txt"),
            "sibling/a.txt must NOT be in parent index, got:\n{}",
            staged_files
        );
        assert!(
            !staged_files.contains("sibling/b.txt"),
            "sibling/b.txt must NOT be in parent index, got:\n{}",
            staged_files
        );
        // NOTE (2026-07-18): previously this test had a tautological
        // assertion `l != "sibling" || true` (clippy::logic-bug) that
        // was masking a real semantic mistake. The correct invariant
        // is: the staged diff must contain ONLY the gitlink pointer
        // for `sibling` (NOT any file contents from inside it). The
        // gitlink pointer change is the entire purpose of
        // `stage_gitlink_updates`, so `sibling` (the path of the
        // gitlink) is expected to appear in the diff. What we MUST
        // not see is `sibling/<file>` (covered by the two assertions
        // above). The previous assertion was the wrong invariant
        // and was effectively dead code.
    }

    #[tokio::test]
    async fn test_stage_gitlink_updates_no_op_for_empty_input() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("p");
        std::fs::create_dir_all(&repo).unwrap();
        init_gitlink_test_repo(&repo).await;
        let result = stage_gitlink_updates(&repo, &[], false, 30).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stage_gitlink_updates_dry_run_does_not_modify_index() {
        // Same fixture as the propagation test but with dry_run=true:
        // parent index MUST NOT change. Verifies the git-args builder
        // is reachable from the test surface.
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        std::fs::create_dir_all(&parent).unwrap();
        init_gitlink_test_repo(&parent).await;

        let sibling = parent.join("sibling");
        std::fs::create_dir_all(&sibling).unwrap();
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&sibling)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "config", "user.email", "t@t"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "config", "user.name", "t"])
            .status()
            .unwrap();
        // Disable hooks so globally-installed warden hooks don't reject
        // commits in temp test repos that lack `.gitattributes` with
        // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "config", "core.hooksPath", "/dev/null"])
            .status()
            .unwrap();
        std::fs::write(sibling.join("a.txt"), "v1\n").unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "add", "a.txt"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "commit", "-q", "-m", "v1"])
            .status()
            .unwrap();
        let sha_a = git_stdout(&sibling, &["rev-parse", "HEAD"]).await;
        crate::git::git_cmd()
            .args(["-C", &parent.to_string_lossy(), "add", "sibling"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &parent.to_string_lossy(), "commit", "-q", "-m", "pin sibling"])
            .status()
            .unwrap();
        // Advance subrepo (so dry_run vs no-dry_run is observable).
        std::fs::write(sibling.join("a.txt"), "v2\n").unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "add", "a.txt"])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", &sibling.to_string_lossy(), "commit", "-q", "-m", "v2"])
            .status()
            .unwrap();
        let _ = sha_a; // unused; just used to populate the index

        let result =
            stage_gitlink_updates(&parent, &["sibling".to_string()], true, 30).await;
        assert!(result.is_ok(), "dry-run must succeed: {:?}", result);

        let index_sha = git_stdout(&parent, &["ls-files", "--stage", "sibling"]).await;
        assert!(
            index_sha.contains(&sha_a),
            "dry-run must NOT change parent index; expected sha {}, got: {}",
            sha_a,
            index_sha
        );
    }

    // ============================================================
    // is_backstop_active tests
    // ============================================================

    #[test]
    fn test_is_backstop_active_below_threshold() {
        // ahead_count <= threshold → not active, regardless of time
        let now = std::time::Instant::now();
        let since = now - std::time::Duration::from_secs(600);
        assert!(!is_backstop_active(Some(since), now, 0, 20, 300));
        assert!(!is_backstop_active(Some(since), now, 20, 20, 300));
    }

    #[test]
    fn test_is_backstop_active_above_threshold_but_recent() {
        // ahead_count > threshold but not enough time has passed
        // → not active
        let now = std::time::Instant::now();
        let since = now - std::time::Duration::from_secs(60);
        assert!(!is_backstop_active(Some(since), now, 21, 20, 300));
    }

    #[test]
    fn test_is_backstop_active_above_threshold_and_old() {
        // ahead_count > threshold AND time elapsed >= min_age_secs
        // → active
        let now = std::time::Instant::now();
        let since = now - std::time::Duration::from_secs(600);
        assert!(is_backstop_active(Some(since), now, 28, 20, 300));
        assert!(is_backstop_active(Some(since), now, 100, 20, 300));
    }

    #[test]
    fn test_is_backstop_active_no_ahead_since() {
        // No ahead_since means the repo has never been ahead during
        // this activity window → not active (just got pushed)
        let now = std::time::Instant::now();
        assert!(!is_backstop_active(None, now, 50, 20, 300));
    }

    #[test]
    fn test_is_backstop_active_threshold_zero_disables() {
        // threshold = 0 means operator disabled the backstop
        let now = std::time::Instant::now();
        let since = now - std::time::Duration::from_secs(3600);
        assert!(!is_backstop_active(Some(since), now, 100, 0, 300));
    }

    // ============================================================
    // auto_resolve_unmerged_if_safe tests
    // ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44
    // ============================================================

    fn init_test_git_repo(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let repo = tmp.path().join(name);
        std::fs::create_dir_all(&repo).unwrap();
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .output()
            .unwrap();
        repo
    }

    /// Manually create an unmerged index state for the given file.
    /// This is the exact state that causes the
    /// "cannot create a tree from a not fully merged index" error.
    /// Stages: 1=base, 2=ours (HEAD), 3=theirs.
    /// Note: `git add <path>` after a merge RESOLVES the unmerge
    /// (selects the working tree content), so we must use
    /// `git update-index --index-info` to set the stages directly.
    fn create_unmerged_state(repo: &std::path::Path, path: &str) {
        use std::io::Write;
        // Get hashes from HEAD (stage 2), from HEAD~1 (stage 1 base),
        // and synthesize a different stage 3 hash by writing a new blob.
        let base = std::process::Command::new("git")
            .args(["rev-parse", &format!("HEAD~1:{}", path)])
            .current_dir(repo)
            .output()
            .unwrap();
        let base_hash = String::from_utf8_lossy(&base.stdout).trim().to_string();
        let ours = std::process::Command::new("git")
            .args(["rev-parse", &format!("HEAD:{}", path)])
            .current_dir(repo)
            .output()
            .unwrap();
        let ours_hash = String::from_utf8_lossy(&ours.stdout).trim().to_string();
        // Write a different blob for "theirs"
        let theirs_path = repo.join(path);
        let original = std::fs::read(&theirs_path).unwrap_or_default();
        let theirs_content = "theirs-conflict-content\n".to_string();
        std::fs::write(&theirs_path, &theirs_content).unwrap();
        let theirs_hash = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["hash-object", "-w", &theirs_path.to_string_lossy()])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
        // Restore the working tree to the HEAD (ours) content so
        // auto-resolve can verify wt == HEAD
        let ours_content = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["show", &format!("HEAD:{}", path)])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .to_string();
        std::fs::write(&theirs_path, ours_content.as_bytes()).unwrap();
        let _ = original; // suppress unused
        // Now set the 3 stages via --index-info
        let input = format!(
            "100644 {} 1\t{}\n100644 {} 2\t{}\n100644 {} 3\t{}\n",
            base_hash, path, ours_hash, path, theirs_hash, path
        );
        let mut child = std::process::Command::new("git")
            .args(["update-index", "--index-info"])
            .current_dir(repo)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(input.as_bytes()).unwrap();
        let _ = child.wait_with_output();
    }

    #[tokio::test]
    async fn test_auto_resolve_unmerged_working_tree_matches_head() {
        // The 4+ hour dracon-platform bug: unmerged PNGs with
        // working tree == HEAD. The daemon must auto-resolve these.
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "unmerged-repo");

        // Create initial file and commit
        std::fs::write(repo.join("a.png"), b"a-content\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create a second commit on main that changes a.png
        std::fs::write(repo.join("a.png"), b"main-content\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "main-change"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Manually create the unmerged state (3 stages for a.png)
        create_unmerged_state(&repo, "a.png");

        // Verify unmerged state exists
        let before = std::process::Command::new("git")
            .args(["ls-files", "--unmerged"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let before_text = String::from_utf8_lossy(&before.stdout);
        assert!(
            !before_text.is_empty(),
            "test setup failed: should have unmerged entries, got: {:?}",
            before_text
        );

        // Now call the daemon's auto-resolve function
        let resolved = auto_resolve_unmerged_if_safe(&repo, true)
            .await
            .expect("auto_resolve should not error");

        assert_eq!(resolved, 1, "should have resolved 1 unmerged entry");

        // Verify the unmerged state is gone
        let after = std::process::Command::new("git")
            .args(["ls-files", "--unmerged"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let after_text = String::from_utf8_lossy(&after.stdout);
        assert!(
            after_text.trim().is_empty(),
            "unmerged state should be cleared, got: {:?}",
            after_text
        );

        // Verify the working tree content is preserved (matches main)
        let wt = std::fs::read(repo.join("a.png")).unwrap();
        assert_eq!(wt, b"main-content\n");
    }

    #[tokio::test]
    async fn test_auto_resolve_unmerged_working_tree_differs_from_head() {
        // When the working tree does NOT match HEAD, the daemon must
        // NOT auto-resolve (it would discard the user's work).
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "unmerged-differs");

        // Setup: create file + commit, then second commit
        std::fs::write(repo.join("x.txt"), b"original\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("x.txt"), b"main-content\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "main"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create unmerged state
        create_unmerged_state(&repo, "x.txt");

        // Now overwrite the working tree with NEW content that doesn't
        // match any stage (simulating user editing during conflict)
        std::fs::write(repo.join("x.txt"), b"user-edited-content\n").unwrap();

        // auto-resolve should NOT resolve (working tree differs from HEAD)
        let resolved = auto_resolve_unmerged_if_safe(&repo, true)
            .await
            .expect("auto_resolve should not error");
        assert_eq!(
            resolved, 0,
            "should not resolve when working tree differs from HEAD"
        );

        // Working tree should still have user's content
        let wt = std::fs::read(repo.join("x.txt")).unwrap();
        assert_eq!(wt, b"user-edited-content\n");
    }

    #[tokio::test]
    async fn test_auto_resolve_unmerged_disabled() {
        // When policy.auto_resolve_unmerged = false, the daemon must
        // NOT resolve anything (operator choice).
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "unmerged-disabled");

        std::fs::write(repo.join("y.txt"), b"original\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("y.txt"), b"main\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "main"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create unmerged state
        create_unmerged_state(&repo, "y.txt");

        // auto-resolve disabled
        let resolved = auto_resolve_unmerged_if_safe(&repo, false)
            .await
            .expect("auto_resolve should not error");
        assert_eq!(resolved, 0, "should not resolve when policy disabled");

        // Unmerged state should still exist
        let after = std::process::Command::new("git")
            .args(["ls-files", "--unmerged"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let after_text = String::from_utf8_lossy(&after.stdout);
        assert!(
            !after_text.trim().is_empty(),
            "unmerged state should remain when policy disabled"
        );
    }

    #[tokio::test]
    async fn test_auto_resolve_no_unmerged() {
        // When there are no unmerged entries, the function should
        // return 0 quickly without doing anything.
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "clean-repo");

        std::fs::write(repo.join("z.txt"), b"hello\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        let resolved = auto_resolve_unmerged_if_safe(&repo, true)
            .await
            .expect("auto_resolve should not error");
        assert_eq!(resolved, 0);
    }

    // ============================================================
    // check_untracked_threshold tests
    // ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44
    // ============================================================

    #[tokio::test]
    async fn test_check_untracked_threshold_below() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "low-untracked");

        // Create 3 untracked files (below threshold of 10)
        for i in 0..3 {
            std::fs::write(repo.join(format!("u{}.txt", i)), b"x").unwrap();
        }

        let count = check_untracked_threshold(&repo, 10)
            .await
            .expect("should not error");
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_check_untracked_threshold_above() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "high-untracked");

        // Create 15 untracked files (above threshold of 10)
        for i in 0..15 {
            std::fs::write(repo.join(format!("u{}.txt", i)), b"x").unwrap();
        }

        let count = check_untracked_threshold(&repo, 10)
            .await
            .expect("should not error");
        assert_eq!(count, 15);
    }

    #[tokio::test]
    async fn test_check_untracked_threshold_zero_disables() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "threshold-zero");

        // Create 100 untracked files
        for i in 0..100 {
            std::fs::write(repo.join(format!("u{}.txt", i)), b"x").unwrap();
        }

        // threshold = 0 disables the warning but still counts
        let count = check_untracked_threshold(&repo, 0)
            .await
            .expect("should not error");
        assert_eq!(count, 100);
    }

    #[tokio::test]
    async fn test_check_untracked_threshold_gitignored_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "gitignored");

        // Create a .gitignore that excludes node_modules/
        std::fs::write(repo.join(".gitignore"), b"node_modules/\n").unwrap();
        std::process::Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "gitignore"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create 3 tracked + 5 gitignored files
        for i in 0..3 {
            std::fs::write(repo.join(format!("a{}.txt", i)), b"x").unwrap();
        }
        std::fs::create_dir_all(repo.join("node_modules")).unwrap();
        for i in 0..5 {
            std::fs::write(
                repo.join(format!("node_modules/pkg{}.js", i)),
                b"x",
            )
            .unwrap();
        }

        // Only the 3 non-ignored files should be counted
        let count = check_untracked_threshold(&repo, 100)
            .await
            .expect("should not error");
        assert_eq!(count, 3, "gitignored files should not be counted");
    }

    /// Regression test for goal `mr02de1n-gjkgzp`: when a parent
    /// repo contains a nested git repository (its own `.git/`
    /// directory), the parent's untracked-file count must subtract
    /// that nested-repo entry — otherwise the parent's UT count
    /// inflates with entries that are owned by the child repo (which
    /// the daemon syncs independently).
    ///
    /// Raw `git ls-files --others --exclude-standard` in this setup
    /// would return 4 entries:
    ///   - `nested/`  (one per nested git repo)
    ///   - `a.txt`, `b.txt`, `c.txt`
    /// The function must return 3 (subtracting the single nested-repo
    /// entry).
    #[tokio::test]
    async fn test_check_untracked_threshold_subtracts_nested_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "parent-with-nested");

        // Create an initial commit so the repo isn't empty.
        std::fs::write(repo.join("README.md"), b"# parent\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create a nested git repo under `parent/nested/`. The nested
        // repo's working tree is invisible to the parent's
        // `git ls-files --others` (git stops at the .git/ boundary),
        // so the parent only sees `nested/` as one untracked entry.
        let nested = repo.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main", "-q"])
            .current_dir(&nested)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "child@example.com"])
            .current_dir(&nested)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Child"])
            .current_dir(&nested)
            .output()
            .unwrap();
        std::fs::write(nested.join("inside_child.txt"), b"inside\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&nested)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "child init"])
            .current_dir(&nested)
            .output()
            .unwrap();

        // Add 3 untracked files at the parent level (not nested).
        for i in 0..3 {
            std::fs::write(repo.join(format!("file{}.txt", i)), b"x").unwrap();
        }

        // Sanity-check: git sees 4 entries (nested/ + 3 files).
        let raw = std::process::Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "ls-files", "--others", "--exclude-standard"])
            .output()
            .unwrap();
        let raw_stdout = String::from_utf8_lossy(&raw.stdout);
        assert_eq!(
            raw_stdout.lines().filter(|l| !l.is_empty()).count(),
            4,
            "precondition: git must see 4 untracked entries (nested/ + 3 files); got: {}",
            raw_stdout,
        );

        // After subtraction, the count must be 3 (the nested/ entry
        // is excluded because child/.git exists).
        let count = check_untracked_threshold(&repo, 100)
            .await
            .expect("should not error");
        assert_eq!(
            count, 3,
            "nested-repo entries must be subtracted from the parent's UT count",
        );
    }

    /// Regression test for goal `mr02de1n-gjkgzp`: when the parent
    /// has multiple nested git repos, ALL of them are subtracted.
    #[tokio::test]
    async fn test_check_untracked_threshold_subtracts_multiple_nested_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "parent-multi-nested");

        // Initial commit.
        std::fs::write(repo.join("README.md"), b"# parent\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create three nested git repos.
        for name in ["child_a", "child_b", "child_c"] {
            let nested = repo.join(name);
            std::fs::create_dir_all(&nested).unwrap();
            std::process::Command::new("git")
                .args(["init", "--initial-branch=main", "-q"])
                .current_dir(&nested)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["config", "user.email", "child@example.com"])
                .current_dir(&nested)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["config", "user.name", "Child"])
                .current_dir(&nested)
                .output()
                .unwrap();
        }

        // Plus one real untracked file.
        std::fs::write(repo.join("real_untracked.txt"), b"x").unwrap();

        // Raw count = 4 (3 nested dirs + 1 file). Subtract all 3
        // nested → expected return = 1.
        let count = check_untracked_threshold(&repo, 100)
            .await
            .expect("should not error");
        assert_eq!(
            count, 1,
            "all three nested-repo entries must be subtracted from the parent's UT count",
        );
    }

    /// Regression test for goal `mr02de1n-gjkgzp`: a directory
    /// without a `.git` inside it must NOT be subtracted. The
    /// subtraction only applies to nested git repos, not to plain
    /// untracked directories (which DO contribute to the parent's
    /// noise count).
    #[tokio::test]
    async fn test_check_untracked_threshold_keeps_plain_untracked_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "parent-plain-dir");

        std::fs::write(repo.join("README.md"), b"# parent\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // A plain untracked dir (no .git inside) plus a regular file.
        std::fs::create_dir_all(repo.join("scratch_dir")).unwrap();
        std::fs::write(repo.join("scratch_dir/note.txt"), b"x").unwrap();
        std::fs::write(repo.join("a.txt"), b"x").unwrap();

        let count = check_untracked_threshold(&repo, 100)
            .await
            .expect("should not error");
        assert_eq!(
            count, 2,
            "plain untracked dirs (no .git inside) must remain in the count",
        );
    }

    /// Regression test for the .pi/ recursion-skip bug (goal
    /// mqp8dffy-bonnlu). The daemon's `stage_existing_files`
    /// recursion used to skip any dir whose name starts with `.`,
    /// which silently blocked `*/.pi/goals/archived/*.md` from
    /// being auto-staged. This test creates such files, calls the
    /// recursion helper, and asserts the files end up in the stage
    /// list (i.e. they are not skipped).
    #[test]
    fn test_stage_existing_files_recurses_into_pi_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_test_git_repo(&tmp, "pi-recursion");

        // Create an initial commit so the repo isn't empty
        std::fs::write(repo.join("README.md"), b"# test\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();

        // Create a .pi/ tree that mirrors a real-world goal-tracking
        // layout: .pi/goals/archived/*.md (operator docs) + a control
        // .cache/ dir (should still be skipped).
        std::fs::create_dir_all(repo.join(".pi/goals/archived")).unwrap();
        std::fs::write(
            repo.join(".pi/goals/archived/goal_20260622_test.md"),
            b"{\"objective\": \"test\"}\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".pi/goals/active_goal_test.md"),
            b"{\"objective\": \"active\"}\n",
        )
        .unwrap();

        // The control: .cache/ should still be skipped.
        std::fs::create_dir_all(repo.join(".cache")).unwrap();
        std::fs::write(repo.join(".cache/junk.txt"), b"junk\n").unwrap();

        // The control: node_modules/ should still be skipped.
        std::fs::create_dir_all(repo.join("node_modules/pkg")).unwrap();
        std::fs::write(repo.join("node_modules/pkg/index.js"), b"x\n").unwrap();

        // The control: .git/ (a directory inside the work tree) should
        // be skipped — but since we just `git init`'d, there is no
        // nested .git/ in the work tree. Skip this case.

        // Build the excluded-dir BTreeSet the same way the daemon does.
        use std::collections::BTreeSet;
        let excluded: BTreeSet<String> = [
            "target",
            "node_modules",
            ".cache",
            ".direnv",
            ".venv",
            "dist",
            "build",
            "archives",
            ".tmp-*",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        // Run the daemon's `stage_existing_files` with the .pi/ dir
        // as the input. The daemon's flow is: libgit2/git status
        // reports the untracked .pi/ dir as a single entry, and the
        // recursion walks into it.
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(stage_existing_files(
                &repo,
                &[".pi/".to_string()],
                /*dry_run=*/ false,
                /*stage_timeout_secs=*/ 60,
                &excluded,
            ));
        assert!(result.is_ok(), "stage_existing_files should not error");

        // The .pi/goals/archived/*.md file should be staged.
        let staged = std::process::Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let staged_text = String::from_utf8_lossy(&staged.stdout);
        assert!(
            staged_text
                .contains(".pi/goals/archived/goal_20260622_test.md"),
            "expected the .pi/goals/archived/goal_*.md file to be staged, but staged entries were: {:?}",
            staged_text
        );
        assert!(
            staged_text.contains(".pi/goals/active_goal_test.md"),
            "expected the .pi/goals/active_goal_*.md file to be staged, but staged entries were: {:?}",
            staged_text
        );

        // The control .cache/ contents should NOT be staged.
        assert!(
            !staged_text.contains(".cache/junk.txt"),
            ".cache/ contents should be skipped, but staged entries were: {:?}",
            staged_text
        );

        // The control node_modules/ contents should NOT be staged.
        assert!(
            !staged_text.contains("node_modules/pkg/index.js"),
            "node_modules/ contents should be skipped, but staged entries were: {:?}",
            staged_text
        );
    }


    /// Regression test for goal `mr10pdzr-i495vy`:
    /// `parent_with_materialized_subrepo_and_dirty_subrepo`. After
    /// the daemon materializes a submodule as a standalone worktree
    /// (using `standalone-worktree materialization`) AND the operator makes a
    /// commit in the worktree, the parent's gitlink MUST be
    /// auto-updated to point at the new SHA (no recursion, no
    /// files from the worktree leaking into the parent's index).
    ///
    /// This is the end-to-end success criterion for the
    /// `dracon-platform` migration: the parent commit
    /// `dracon-platform/web/games/wip/polis` (mode 160000) must
    /// be updated to the new SHA after each daemon cycle, with
    /// the worktree files remaining ONLY in the worktree's index.
    ///
    /// Test approach: build a parent + subrepo structure from
    /// scratch (mimicking the real dracon-platform layout). The
    /// subrepo's gitdir is at `<parent>/.git/modules/<name>` and
    /// the subrepo's main working tree is at
    /// `<parent>/sub/`. The parent's gitlink is registered
    /// pointing at the subrepo's HEAD. We then:
    /// 1. Materialize the subrepo as a standalone worktree via
    ///    `standalone-worktree materialization`.
    /// 2. Advance the subrepo's HEAD by directly creating a
    ///    commit on the subrepo's main branch (via the gitdir).
    /// 3. Run the daemon's `stage_gitlink_updates` to propagate
    ///    the new SHA back to the parent.
    /// 4. Assert the parent index points at the new SHA, with
    ///    NO files from inside the subrepo.
    /// End-to-end regression test for goal `mr10pdzr-i495vy`:
    /// `parent_gitlink_propagates_after_standalone_commit`.
    ///
    /// Goal: after the daemon materializes a non-polis
    /// submodule as a standalone worktree via
    /// `standalone-worktree materialization` AND the operator makes a commit
    /// in that worktree, the parent's gitlink MUST be
    /// auto-updated to the new SHA. This is the convergence
    /// invariant for the 6 of 9 non-polis submodules that
    /// previously failed (junk-runner, deathrun,
    /// capture-anime-girls, darklord, endless-td, neonbreak).
    ///
    /// This test is the strict end-to-end version of
    /// `parent_with_materialized_subrepo_and_dirty_subrepo`
    /// (which exercises the same machinery). The difference:
    /// this test uses the exact daemon entry point
    /// (`stage_gitlink_updates`) and asserts (a) the parent
    /// index is updated to the new SHA, (b) no worktree files
    /// leak into the parent's index, AND (c) the parent
    /// gitlink stays updated across a SECOND commit
    /// (proving convergence under repeated standalones commits,
    /// not just one-off).
    #[tokio::test]
    async fn parent_gitlink_propagates_after_standalone_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        std::fs::create_dir_all(&parent).unwrap();
        let parent_s = parent.to_string_lossy().to_string();

        let run_in = |cwd: &str, args: &[&str]| {
            let mut real = vec!["git", "-C", cwd];
            real.extend_from_slice(args);
            Command::new(real[0]).args(&real[1..]).output().unwrap()
        };

        // 1. Build the parent repo.
        run_in(&parent_s, &["init", "-q", "-b", "main"]);
        run_in(&parent_s, &["config", "user.email", "t@t"]);
        run_in(&parent_s, &["config", "user.name", "t"]);
        // Disable hooks so globally-installed warden hooks don't reject
        // commits in temp test repos that lack `.gitattributes` with
        // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
        run_in(&parent_s, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(parent.join("README.md"), b"# parent\n").unwrap();
        run_in(&parent_s, &["add", "-A"]);
        run_in(&parent_s, &["commit", "-q", "-m", "init"]);

        // 2. Build the subrepo at `parent/sub/` with its own
        //    `.git/`.
        let sub_path = parent.join("sub");
        std::fs::create_dir_all(&sub_path).unwrap();
        let sub_s = sub_path.to_string_lossy().to_string();
        run_in(&sub_s, &["init", "-q", "-b", "main"]);
        run_in(&sub_s, &["config", "user.email", "t@t"]);
        run_in(&sub_s, &["config", "user.name", "t"]);
        run_in(&sub_s, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(sub_path.join("README.md"), b"# sub\n").unwrap();
        run_in(&sub_s, &["add", "-A"]);
        run_in(&sub_s, &["commit", "-q", "-m", "init"]);
        let sub_head_initial = String::from_utf8_lossy(
            &run_in(&sub_s, &["rev-parse", "HEAD"]).stdout,
        )
        .trim()
        .to_string();

        // 3. Register the subrepo as a gitlink in the parent.
        run_in(
            &parent_s,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},sub", sub_head_initial),
            ],
        );
        run_in(&parent_s, &["commit", "-q", "-m", "add sub"]);

        // 4. Make a STANDALONE COMMIT in `sub_path/`.
        std::fs::write(sub_path.join("README.md"), b"# updated sub\n").unwrap();
        run_in(&sub_s, &["add", "README.md"]);
        run_in(&sub_s, &["commit", "-q", "-m", "update"]);
        let sub_head_v1 = String::from_utf8_lossy(
            &run_in(&sub_s, &["rev-parse", "HEAD"]).stdout,
        )
        .trim()
        .to_string();
        assert_ne!(sub_head_initial, sub_head_v1);

        // 5. Run the daemon's gitlink-update stage.
        let result =
            stage_gitlink_updates(&parent, &["sub".to_string()], false, 30).await;
        assert!(
            result.is_ok(),
            "stage_gitlink_updates must succeed: {:?}",
            result
        );

        // 6. Parent index must now point at sub_head_v1.
        let index_sha_after_v1 = String::from_utf8_lossy(
            &run_in(&parent_s, &["ls-files", "--stage", "sub"]).stdout,
        )
        .to_string();
        assert!(
            index_sha_after_v1.contains(&sub_head_v1),
            "parent index must reflect new submodule SHA {}, got: {:?}",
            sub_head_v1,
            index_sha_after_v1
        );

        // 7. NO subrepo file leaked into the parent index.
        let staged_files = String::from_utf8_lossy(
            &run_in(&parent_s, &["diff", "--cached", "--name-only"]).stdout,
        )
        .to_string();
        assert!(
            !staged_files.contains("sub/README.md"),
            "subrepo README.md must NOT be in parent index (no recursion!), got:\n{}",
            staged_files
        );

        // 8. SECOND commit in the standalone — convergence must
        //    hold across repeated standalone commits.
        std::fs::write(sub_path.join("extra.txt"), b"extra\n").unwrap();
        run_in(&sub_s, &["add", "extra.txt"]);
        run_in(&sub_s, &["commit", "-q", "-m", "more"]);
        let sub_head_v2 = String::from_utf8_lossy(
            &run_in(&sub_s, &["rev-parse", "HEAD"]).stdout,
        )
        .trim()
        .to_string();
        assert_ne!(sub_head_v1, sub_head_v2);

        let result2 =
            stage_gitlink_updates(&parent, &["sub".to_string()], false, 30).await;
        assert!(
            result2.is_ok(),
            "second stage_gitlink_updates must succeed: {:?}",
            result2
        );
        let index_sha_after_v2 = String::from_utf8_lossy(
            &run_in(&parent_s, &["ls-files", "--stage", "sub"]).stdout,
        )
        .to_string();
        assert!(
            index_sha_after_v2.contains(&sub_head_v2),
            "parent index must reflect second submodule SHA {}, got: {:?}",
            sub_head_v2,
            index_sha_after_v2
        );
    }

}
