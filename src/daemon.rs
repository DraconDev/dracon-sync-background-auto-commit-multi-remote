use anyhow::{Context, Result};
use dracon_git::GitService;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::signal::unix::SignalKind;
use tokio::time::sleep;

pub(crate) static VERBOSITY: AtomicU8 = AtomicU8::new(0);

/// Conditional eprintln based on verbosity level.
#[macro_export]
macro_rules! veprintln {
    ($lvl:expr, $($arg:tt)*) => {
        if $lvl <= VERBOSITY.load(Ordering::SeqCst) {
            eprintln!($($arg)*);
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    };
}

use crate::exclude::{excluded_dir_names_set, has_sync_relevant_dirty_entries};
use crate::git::{
    count_unpushed_vs_mirrors, current_branch, discover_git_repos, git_diff_head_files,
    has_both_main_and_master, has_origin_remote, has_tracking_upstream, is_repo_ready,
    is_safe_branch_name, repair_broken_tracking, repo_diff_entries, run_git_with_timeout,
};
use crate::policy::{debug_enabled, freeze_reason, timestamp_secs, SyncPolicy};
use crate::report::{run_repair_concerns, run_repair_warns, ConcernRepairFilter};
use crate::sync::{sync_repo, sync_repo_with_ahead_since, SyncOutcome};
use crate::git::list_submodules;


/// Result of a single spawned `sync_repo` task: the per-repo counters
/// (incremented during the sync) and the outcome the sync reported.
pub(crate) type SyncTaskResult =
    (HashMap<String, usize>, Result<SyncOutcome, anyhow::Error>);

/// Join handle for a spawned sync task, tagged with the repo path so
/// the in-flight collector can route the result back to the right repo.
pub(crate) type SyncTaskJoin = tokio::task::JoinHandle<SyncTaskResult>;

/// Join handle for a spawned sync task that returns the full trio
/// (repo path, counters, outcome) used by the in-flight collector.
pub(crate) type SyncTrioJoin = tokio::task::JoinHandle<(
    PathBuf,
    HashMap<String, usize>,
    Result<SyncOutcome, anyhow::Error>,
)>;
const STUCK_REPO_EXPIRY_SECS: u64 = 24 * 60 * 60; // 24 hours

/// Count unpushed commits by comparing local HEAD to configured remote HEADs.
/// This catches repos that have remotes but no upstream tracking branch and no
/// remote-tracking refs yet (e.g. a repo that just received mirror remotes).
pub(crate) fn count_unpushed_vs_configured_remotes(
    repo: &Path,
    remote_names: &[String],
) -> u64 {
    if remote_names.is_empty() {
        return 0;
    }
    let branch = crate::git::current_branch(repo).unwrap_or_else(|| "main".to_string());
    let local_head = crate::policy::std_git_command()
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });
    let Some(local_head) = local_head else {
        return 0;
    };

    for remote in remote_names {
        let output = crate::policy::std_git_command()
            .args(["ls-remote", remote, &format!("refs/heads/{branch}")])
            .current_dir(repo)
            .output();
        let Ok(output) = output else {
            return 1;
        };
        if !output.status.success() {
            return 1;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some(remote_hash) = stdout.lines().next().and_then(|line| line.split_whitespace().next()) else {
            return 1;
        };
        if remote_hash != local_head {
            return 1;
        }
    }
    0
}

/// Configure standard mirror remotes only when the repo has no remotes yet.
pub(crate) fn configure_standard_remotes_if_missing(repo: &Path, policy: &SyncPolicy) -> bool {
    let has_any_remote = has_origin_remote(repo)
        || !policy.remotes.is_empty()
            && policy.remotes.iter().any(|r| {
                crate::git::multi_remote::get_remote_url(repo, &r.name).is_some()
            });

    // ADDED 2026-07-19 (goal `4555eaf6`): if the repo has mirror
    // remotes (github/gitlab/codeberg) but no `origin`, ensure
    // `origin` is set to the github mirror. This is the case for
    // older repos that were added to the watch list before
    // `origin` was the convention. Without this, VS Code's
    // `git push` falls back to the alphabetically-first remote
    // (often codeberg) and the daemon's PUBLISH cell reads
    // `codeberg/main` for what is actually a github-primary repo.
    //
    // Only fires when mirrors exist but origin is absent. If the
    // repo has no mirrors at all (truly bare), we fall through to
    // the existing configure-mirrors-if-missing path below.
    if has_any_remote && !has_origin_remote(repo) {
        crate::git::multi_remote::ensure_origin_for_vscode(repo, &policy.remotes);
    }
    if !has_any_remote && !policy.remotes.is_empty() {
        let repo_name = repo
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        eprintln!(
            "🔧 {} configuring standard mirror remotes",
            repo.display()
        );
        // CHANGED 2026-06-23 (goal mqqsyzyd-qkvna5): honor
        // per-repo `exclude_remotes` so a repo can opt out of a
        // specific mirror at the very first auto-configure step.
        // Without this, the daemon would add the excluded remote
        // here and then `push_mirror_remotes` would skip it on
        // every push — leaving a useless remote entry in
        // `.git/config`.
        let repo_override = crate::policy::load_repo_override(repo);
        let mut combined_exclude = repo_override.exclude_remotes.clone();
        // ADDED 2026-07-17 (goal `codeberg-public-only`): also exclude
        // codeberg at auto-configure time when the public-only policy
        // is active AND the cached visibility says private. Without
        // this, the codeberg remote would be added to `.git/config`
        // even though the push path would skip it — leaving a dead
        // entry that confuses `git remote -v` and any other tooling
        // that walks the remote list. See
        // `docs/design/codeberg-public-only-policy-2026-07-17.md`.
        let codeberg_public_only_effective = repo_override
            .codeberg_public_only
            .unwrap_or(policy.codeberg_public_only);
        if codeberg_public_only_effective {
            let cached_priv = crate::visibility::cached_repo_visibility(repo);
            let skip_codeberg = match cached_priv {
                Some(true) | None => true,
                Some(false) => false,
            };
            if skip_codeberg && !combined_exclude.iter().any(|e| e == "codeberg") {
                combined_exclude.push("codeberg".to_string());
            }
        }
        // ADDED 2026-07-21 (v0.112.30): also skip codeberg at
        // discovery when its effective auto_create is off (v0.112.28
        // quota posture) and the repo has no codeberg tracking ref.
        // Mirrors the push-level exclusion in `push_mirror_remotes`
        // so the dead remote is never added for new repos in the
        // first place (previously it was added here, then every push
        // failed with "Forgejo: Push to create is not enabled" until
        // `remove_stale_remotes` cleaned it up).
        if crate::git::multi_remote::codeberg_push_excluded(
            &policy.remotes,
            repo_override.auto_create_on_codeberg,
            crate::git::multi_remote::has_codeberg_tracking_ref(repo),
        ) && !combined_exclude.iter().any(|e| e == "codeberg")
        {
            combined_exclude.push("codeberg".to_string());
        }
        crate::git::multi_remote::configure_all_remotes(
            repo,
            &policy.remotes,
            &repo_name,
            &combined_exclude,
        );
        true
    } else {
        false
    }
}

/// Return the primary remote VS Code should use for Publish Branch.
///
/// `origin` wins when present for backwards compatibility. For mirror-only
/// repos, `github` is the conventional primary mirror because VS Code expects a
/// single publish remote and the daemon still pushes explicitly to all mirrors.
fn primary_publish_remote(repo: &Path, policy: &SyncPolicy) -> Option<String> {
    let remotes = crate::git::multi_remote::list_remotes(repo);
    let remote_set: HashSet<&str> = remotes.iter().map(String::as_str).collect();
    if remote_set.contains("origin") {
        return Some("origin".to_string());
    }
    if remote_set.contains("github") {
        return Some("github".to_string());
    }
    policy
        .remotes
        .iter()
        .find(|r| remote_set.contains(r.name.as_str()))
        .map(|r| r.name.clone())
}

/// Configure `branch.<current>.remote` and `branch.<current>.merge` when the
/// current branch has no upstream. This removes VS Code's "Publish Branch"
/// prompt for mirror-only repos while preserving daemon explicit mirror pushes.
pub(crate) fn configure_publish_upstream_if_missing(
    repo: &Path,
    policy: &SyncPolicy,
) -> Result<bool> {
    if has_tracking_upstream(repo) {
        return Ok(false);
    }
    let Some(branch) = current_branch(repo) else {
        return Ok(false);
    };
    if !is_safe_branch_name(&branch) {
        return Ok(false);
    }
    let Some(remote) = primary_publish_remote(repo, policy) else {
        return Ok(false);
    };
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    crate::policy::std_git_command()
        .args(["config", &remote_key, &remote])
        .current_dir(repo)
        .status()
        .with_context(|| format!("failed to set {remote_key} in {}", repo.display()))?;
    crate::policy::std_git_command()
        .args(["config", &merge_key, &format!("refs/heads/{branch}")])
        .current_dir(repo)
        .status()
        .with_context(|| format!("failed to set {merge_key} in {}", repo.display()))?;
    eprintln!(
        "🔧 {} configured publish upstream for {branch} on {remote}",
        repo.display()
    );
    Ok(true)
}

/// Refresh the configured publish upstream after a successful push.
///
/// `configure_publish_upstream_if_missing` writes the branch config so VS Code
/// stops showing "Publish Branch" immediately. Once the branch exists on the
/// primary remote, this fetches that remote-tracking ref and points the local
/// upstream to it so `git status --branch` is clean rather than "gone".
///
/// We skip the refresh when the configured publish upstream is `origin` and
/// the repo also has SSH mirrors (the legacy pattern seen in `dracon-platform`):
/// fetching from an HTTPS `origin` every daemon cycle is slow and unreliable,
/// while the operator has already configured SSH mirror pushes for the actual
/// sync. The publish upstream config itself is still useful (VS Code stops
/// prompting "Publish Branch"), but pointing `@{u}` at an SSH mirror's ref
/// adds no value when the operator chose `origin` as publish.
pub(crate) async fn refresh_publish_upstream(repo: &Path, policy: &SyncPolicy) -> Result<bool> {
    if !has_tracking_upstream(repo) {
        return Ok(false);
    }
    let Some(branch) = current_branch(repo) else {
        return Ok(false);
    };
    if !is_safe_branch_name(&branch) {
        return Ok(false);
    }
    let remote = configured_branch_remote(repo, &branch)
        .or_else(|| primary_publish_remote(repo, policy))
        .unwrap_or_default();
    if remote.is_empty() {
        return Ok(false);
    }
    if remote == "origin" && has_ssh_mirrors(repo) {
        return Ok(false);
    }
    let refspec = format!("{branch}:refs/remotes/{remote}/{branch}");
    let fetch = run_git_with_timeout(
        repo,
        &["fetch", "--prune", &remote, &refspec],
        30,
        &format!("fetch-publish-upstream-{remote}"),
    )
    .await;
    if let Err(e) = fetch {
        if debug_enabled() {
            eprintln!(
                "🐛 {} could not fetch publish upstream {remote}/{branch}: {}",
                repo.display(),
                e
            );
        }
        return Ok(false);
    }
    crate::git::set_upstream_to_remote_branch(repo, &remote, &branch)?;
    Ok(true)
}

fn has_ssh_mirrors(repo: &Path) -> bool {
    crate::git::multi_remote::list_remotes(repo)
        .iter()
        .any(|name| name == "github" || name == "gitlab" || name == "codeberg")
}

fn configured_branch_remote(repo: &Path, branch: &str) -> Option<String> {
    if !is_safe_branch_name(branch) {
        return None;
    }
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    let remote = crate::policy::std_git_command()
        .args(["config", "--get", &remote_key])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })?;
    let merge = crate::policy::std_git_command()
        .args(["config", "--get", &merge_key])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })?;
    let remote_branch = merge.strip_prefix("refs/heads/")?;
    if is_safe_branch_name(remote_branch) {
        Some(remote)
    } else {
        None
    }
}

fn stage_cooldown_remaining(
    stage_cooldowns: &mut HashMap<PathBuf, Instant>,
    repo: &Path,
    now: Instant,
) -> Option<Duration> {
    let until = stage_cooldowns.get(repo).copied()?;
    if now >= until {
        stage_cooldowns.remove(repo);
        return None;
    }
    Some(until.duration_since(now))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct StuckRepoEntry {
    path: PathBuf,
    pub(crate) stuck_since: u64,
    /// Number of consecutive push failures. Reset to 0 on
    /// successful push. Used to detect when the retry budget
    /// is exhausted and the daemon should stop auto-pushing
    /// (the operator can then intervene via `repair-concerns`).
    /// Defaults to 0 for entries written before this field
    /// was added.
    #[serde(default)]
    pub(crate) consecutive_failures: u32,
    /// Last error message from the failed `git push`. Surfaced
    /// in the `repos` HINT column so the operator can see
    /// WHY the push is stuck (auth, non-FF, network, etc.)
    /// without grepping the daemon log.
    #[serde(default)]
    pub(crate) last_error: String,
    /// Epoch seconds of the last push failure.
    #[serde(default)]
    pub(crate) last_error_at: u64,
    /// ADDED 2026-07-21 (v0.112.31, audit H5/F1.2): epoch seconds of
    /// the last retry ATTEMPT. The pre-fix retry path deleted the
    /// ledger entry before dispatching, so a failed retry re-created
    /// it with `consecutive_failures = 1` — the budget reset every
    /// 5-minute cycle and `push_max_retries` could never engage.
    /// Now the entry persists and this field throttles attempts.
    #[serde(default)]
    pub(crate) last_retry_at: u64,
}

/// ADDED 2026-07-21 (v0.112.31, audit H5/F1.2): what the daemon loop
/// should do with a repo that has a stuck-push ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StuckDecision {
    /// Inside the backoff window since the last failure/attempt — skip.
    Backoff,
    /// Allow this cycle through (a retry attempt). The caller MUST
    /// stamp `last_retry_at` so a non-push failure doesn't spin
    /// every cycle.
    Retry,
    /// `consecutive_failures >= push_max_retries` — stop auto-pushing
    /// until the operator intervenes (`unstuck` / `repair-concerns`).
    /// This is the enforcement the pre-fix code never had
    /// (`push_max_retries` was report-display-only).
    Exhausted,
}

/// ADDED 2026-07-21 (v0.112.31, audit H5/F1.2): pure decision —
/// extracted so the stuck-push state machine is unit-testable without
/// spinning up the daemon loop.
pub(crate) fn stuck_decision(
    info: &StuckRepoEntry,
    now_secs: u64,
    max_retries: u32,
    backoff_secs: u64,
) -> StuckDecision {
    if info.consecutive_failures >= max_retries {
        return StuckDecision::Exhausted;
    }
    let last_activity = info
        .last_retry_at
        .max(info.last_error_at)
        .max(info.stuck_since);
    if now_secs.saturating_sub(last_activity) < backoff_secs {
        StuckDecision::Backoff
    } else {
        StuckDecision::Retry
    }
}

/// Default policy value: number of consecutive push failures
/// before the daemon stops auto-pushing and surfaces a
/// `🛑 push-stuck` state in the ACTIVITY column. The operator
/// can override via `push_max_retries` in the policy.
// NOTE: removed 2026-07-11 (audit AUDIT-3-UTILITIES-2026-07-10.md
// CONCERN #6). The canonical `default_push_max_retries` lives in
// `policy.rs` (used by `#[serde(default = "...")]` on
// `SyncPolicy::push_max_retries`). The duplicate here was dead code.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{AuthType, RemoteConfig};

    fn git_config_value(repo: &Path, key: &str) -> String {
        String::from_utf8_lossy(
            &crate::git::git_cmd()
                .args(["config", "--get", key])
                .current_dir(repo)
                .output()
                .expect("git config")
                .stdout,
        )
        .trim()
        .to_string()
    }

    #[test]
    fn test_configure_standard_remotes_if_missing_adds_remotes() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        let mut policy = crate::policy::test_sync_policy();
        policy.remotes = vec![RemoteConfig {
            name: "github".to_string(),
            push_url: "git@github.com:DraconDev/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "DraconDev".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];

        assert!(configure_standard_remotes_if_missing(&repo, &policy));
        assert_eq!(
            crate::git::multi_remote::get_remote_url(&repo, "github"),
            Some("git@github.com:DraconDev/test-repo.git".to_string())
        );
    }

    #[test]
    fn test_configure_standard_remotes_if_missing_preserves_existing_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "origin", "git@github.com:Other/test-repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add")
            .success());
        let mut policy = crate::policy::test_sync_policy();
        policy.remotes = vec![RemoteConfig {
            name: "github".to_string(),
            push_url: "git@github.com:DraconDev/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "DraconDev".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];

        assert!(!configure_standard_remotes_if_missing(&repo, &policy));
        assert_eq!(
            crate::git::multi_remote::get_remote_url(&repo, "origin"),
            Some("git@github.com:Other/test-repo.git".to_string())
        );
        assert_eq!(
            crate::git::multi_remote::get_remote_url(&repo, "github"),
            None
        );
    }

    #[test]
    fn test_configure_publish_upstream_if_missing_adds_github_upstream() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success());
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        assert!(crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "github", "git@github.com:DraconDev/test-repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add")
            .success());

        let policy = crate::policy::test_sync_policy();
        assert!(configure_publish_upstream_if_missing(&repo, &policy).expect("configure upstream"));
        assert_eq!(git_config_value(&repo, "branch.main.remote"), "github");
        assert_eq!(
            git_config_value(&repo, "branch.main.merge"),
            "refs/heads/main"
        );
        assert!(!configure_publish_upstream_if_missing(&repo, &policy).expect("already configured"));
    }

    #[test]
    fn test_configure_publish_upstream_if_missing_preserves_existing_upstream() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.remote", "origin"])
            .current_dir(&repo)
            .status()
            .expect("remote config")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.merge", "refs/heads/main"])
            .current_dir(&repo)
            .status()
            .expect("merge config")
            .success());

        let policy = crate::policy::test_sync_policy();
        assert!(!configure_publish_upstream_if_missing(&repo, &policy).expect("preserve upstream"));
        assert_eq!(git_config_value(&repo, "branch.main.remote"), "origin");
    }

    #[tokio::test]
    async fn test_refresh_publish_upstream_fetches_primary_remote_ref() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        let bare = tmp.path().join("remote.git");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "--bare"])
            .arg(&bare)
            .status()
            .expect("bare init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success());
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        assert!(crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "github"])
            .arg(&bare)
            .current_dir(&repo)
            .status()
            .expect("git remote add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.remote", "github"])
            .current_dir(&repo)
            .status()
            .expect("remote config")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.merge", "refs/heads/main"])
            .current_dir(&repo)
            .status()
            .expect("merge config")
            .success());
        assert!(crate::git::git_cmd()
            .args(["push", "github", "main"])
            .current_dir(&repo)
            .status()
            .expect("initial push")
            .success());
        crate::git::git_cmd()
            .args(["update-ref", "-d", "refs/remotes/github/main"])
            .current_dir(&repo)
            .status()
            .ok();

        let policy = crate::policy::test_sync_policy();
        assert!(refresh_publish_upstream(&repo, &policy).await.expect("refresh upstream"));
        let upstream = crate::git::git_cmd()
            .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .current_dir(&repo)
            .output()
            .expect("upstream")
            .stdout;
        assert_eq!(String::from_utf8_lossy(&upstream).trim(), "github/main");
    }

    #[tokio::test]
    async fn test_refresh_publish_upstream_skips_origin_when_ssh_mirrors_exist() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success());
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        assert!(crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "origin", "https://github.com/DraconDev/test-repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add origin")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "github", "git@github.com:DraconDev/test-repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add github")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.remote", "origin"])
            .current_dir(&repo)
            .status()
            .expect("remote config")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.merge", "refs/heads/main"])
            .current_dir(&repo)
            .status()
            .expect("merge config")
            .success());

        let policy = crate::policy::test_sync_policy();
        // Should skip cleanly (return false) without attempting HTTPS fetch.
        assert!(!refresh_publish_upstream(&repo, &policy).await.expect("skip origin"));
    }

    #[test]
    fn test_count_unpushed_vs_configured_remotes_detects_new_remote_head() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let bare = tmp.path().join("remote.git");
        let repo = tmp.path().join("work");
        let bare_path = bare.to_str().expect("bare path");
        assert!(crate::git::git_cmd()
            .args(["init", "--bare", "-q", bare_path])
            .status()
            .expect("git init --bare")
            .success());
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init work")
            .success());
        std::fs::write(repo.join("file.txt"), "hello\n").expect("write file");
        assert!(crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("git config email")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo)
            .status()
            .expect("git config name")
            .success());
        assert!(crate::git::git_cmd()
            .args(["add", "file.txt"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["commit", "--no-verify", "-q", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "github", bare_path])
            .current_dir(&repo)
            .status()
            .expect("git remote add")
            .success());

        let remotes = vec!["github".to_string()];
        assert_eq!(
            count_unpushed_vs_configured_remotes(&repo, &remotes),
            1,
            "local HEAD should be unpushed before the first push"
        );
        assert!(crate::git::git_cmd()
            .args(["push", "--no-verify", "-q", "github", "master:refs/heads/master"])
            .current_dir(&repo)
            .status()
            .expect("git push")
            .success());
        assert_eq!(
            count_unpushed_vs_configured_remotes(&repo, &remotes),
            0,
            "local HEAD should match remote branch after push"
        );
    }

    #[test]
    fn test_stuck_repo_entry_serialization() {
        let entry = StuckRepoEntry {
            path: PathBuf::from("/test/repo"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"/test/repo\""));
        assert!(json.contains("1000"));
    }

    #[test]
    fn test_stuck_repo_entry_deserialization() {
        let json = r#"{"path":"/test/repo","stuck_since":1000}"#;
        let entry: StuckRepoEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.path, PathBuf::from("/test/repo"));
        assert_eq!(entry.stuck_since, 1000);
    }

    #[test]
    fn test_stuck_repo_expiry_constant() {
        assert_eq!(STUCK_REPO_EXPIRY_SECS, 24 * 60 * 60);
    }

    #[test]
    fn test_stuck_repo_expiry_one_day() {
        assert_eq!(STUCK_REPO_EXPIRY_SECS, 86400);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_stuck_repo_expiry_not_zero() {
        assert!(STUCK_REPO_EXPIRY_SECS > 0);
    }

    #[test]
    fn test_stage_cooldown_remaining_removes_expired_and_keeps_active() {
        let repo = PathBuf::from("/tmp/repo");
        let now = Instant::now();
        let mut cooldowns = HashMap::new();
        cooldowns.insert(repo.clone(), now + Duration::from_secs(60));
        let active = stage_cooldown_remaining(&mut cooldowns, &repo, now).expect("active");
        assert!(active <= Duration::from_secs(60));
        assert!(cooldowns.contains_key(&repo));

        let expired =
            stage_cooldown_remaining(&mut cooldowns, &repo, now + Duration::from_secs(61));
        assert!(expired.is_none());
        assert!(!cooldowns.contains_key(&repo));
    }

    #[test]
    fn test_stage_cooldown_remaining_missing_is_none() {
        let repo = PathBuf::from("/tmp/repo");
        let mut cooldowns = HashMap::new();
        let remaining = stage_cooldown_remaining(&mut cooldowns, &repo, Instant::now());
        assert!(remaining.is_none());
    }

    #[test]
    fn test_stuck_repo_entry_debug() {
        let entry = StuckRepoEntry {
            path: PathBuf::from("/test/repo"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        let debug = format!("{:?}", entry);
        assert!(debug.contains("/test/repo"));
        assert!(debug.contains("1000"));
    }

    #[test]
    fn test_stuck_repo_entry_clone() {
        let entry = StuckRepoEntry {
            path: PathBuf::from("/test/repo"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        let cloned = entry.clone();
        assert_eq!(cloned.path, entry.path);
        assert_eq!(cloned.stuck_since, entry.stuck_since);
    }

    #[test]
    fn test_stuck_repo_entry_equality() {
        let entry1 = StuckRepoEntry {
            path: PathBuf::from("/test/repo"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        let entry2 = StuckRepoEntry {
            path: PathBuf::from("/test/repo"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        let entry3 = StuckRepoEntry {
            path: PathBuf::from("/other/repo"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        assert_eq!(entry1.path, entry2.path);
        assert_ne!(entry1.path, entry3.path);
    }

    #[test]
    fn test_stuck_repo_entry_path_stored_correctly() {
        let path = PathBuf::from("/home/user/code/my-project");
        let entry = StuckRepoEntry {
            path: path.clone(),
            stuck_since: 12345,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        assert_eq!(entry.path, path);
        assert_eq!(entry.path.to_string_lossy(), "/home/user/code/my-project");
    }

    #[test]
    fn test_stuck_repo_entry_timestamp_ordering() {
        let old = StuckRepoEntry {
            path: PathBuf::from("/old"),
            stuck_since: 1000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,
            
        };
        let new = StuckRepoEntry {
            path: PathBuf::from("/new"),
            stuck_since: 2000,
            consecutive_failures: 0,
            last_error: String::new(),
            last_error_at: 0,
            last_retry_at: 0,

        };
        assert!(old.stuck_since < new.stuck_since);
    }

    // ============================================================
    // record_push_failure / record_push_success / get_stuck_push_info
    // ============================================================

    /// Use a unique fake repo path so tests don't interfere with
    /// each other or with real daemon state.
    fn make_test_repo_path(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("/tmp/dracon-sync-test-{}-{}", name, std::process::id()))
    }

    #[test]
    fn test_record_push_failure_increments_counter() {
        // Use a temp state dir so this test does NOT pollute the
        // real stuck-push ledger at
        // ~/.local/state/dracon/dracon-sync-stuck-push-repos.json.
        // Regression: previously this test wrote to the real
        // ledger with the fake repo path
        // `/tmp/dracon-sync-test-failure-increments-{pid}`, which
        // then appeared as a junk entry in the live daemon's
        // `dracon-sync repos` report.
        let temp_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            temp_dir.path().to_string_lossy().as_ref(),
        );
        let repo = make_test_repo_path("failure-increments");
        // Ensure clean state
        let _ = crate::daemon::unstuck_repo(&repo);

        record_push_failure(&repo, "permission denied");
        let info = get_stuck_push_info(&repo).expect("entry should exist");
        assert_eq!(info.consecutive_failures, 1);
        assert_eq!(info.last_error, "permission denied");

        record_push_failure(&repo, "connection timeout");
        let info = get_stuck_push_info(&repo).expect("entry should still exist");
        assert_eq!(info.consecutive_failures, 2);
        assert_eq!(info.last_error, "connection timeout");

        // Cleanup
        let _ = crate::daemon::unstuck_repo(&repo);
    }

    #[test]
    fn test_record_push_success_clears_entry() {
        // See `test_record_push_failu[DRACON_SECRET:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBpR29oN2dGRHpLVG93UVRtR01ScnpyK3dURVF4NS9SREd5bm9hVFc0OFdFCm53RTBVdHQxdzBSd1d5bTN6OHVTWGIybFVxWjd5eXc5Smd4YTJ5dmtLWHMKLT4gWDI1NTE5IHVEVEhvTmJpUVp1SDA2ZXMwOFo4eG5UZWhuVlo1dEpPbWJDelBpbHJIeTgKUnBjRGpXdEtzenR6ZnpCdURlTHZqMTlza2M0aTRMYXQ1YnNJTk1DaUd3UQotPiBYMjU1MTkgR01WWXlYZ3Y5WkFnOERrRFpvZGpzNDM0RWJFZ0FyY1pkMGN4ZXB2ZzRIYwoxZ0RYZytZSkVvdjZHb1VxMHV1OElvb2w5THY3c284V2pjOGNnaVNQb2t3Ci0+IFgyNTUxOSB1UWcraFNXdWw5RGY3TDZ1WndCQzZZTi9zVmNiSGl3NmE3cFFHbHZFR1RNCmN2Zkd3VzdiSXBkMmhrSCtWMFhKcWJGUit6emhqaTd0NGNVaUN2dFZaUWcKLT4gWDI1NTE5IDdHY1Z6QWVXMW10R0NvV1lvMk5kaXo5U3RFS25lS3FGRG5yeXN1V3dOVGsKZmEvVVZQdnJ1azFEaHVGRnphbWUzdklhcGVpcnpUNHFXaERTcG5CSzVDbwotPiAoNF58VTEtZ3JlYXNlIGxJQ1hTTXkgJHEgTQpKN1dOb0laUHl1d01BU2VHbVYremcxL1pzam5uZ0t4THkvdEFDR0kweGhnNDFoenFTeEZUcHFNCi0tLSB5OUhQSGZlK1VXclRqbWdiZVBYRDJrS1IvY3NTbXczcmRoaU52Umw5N3NzCj/aK0ntHuvwUkgJw+IQoVBPVJASljLDE3uww4CzGLc6qn/MA7IZbpIUQIEZW130K6X2yyqgzQ3WIiwMAST5uFf5Ag==]`
        // for the rationale on using a temp state dir.
        let temp_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            temp_dir.path().to_string_lossy().as_ref(),
        );
        let repo = make_test_repo_path("success-clears");
        let _ = crate::daemon::unstuck_repo(&repo);

        // First record some failures
        record_push_failure(&repo, "err 1");
        record_push_failure(&repo, "err 2");
        assert!(get_stuck_push_info(&repo).is_some());

        // Now record success
        record_push_success(&repo);
        assert!(get_stuck_push_info(&repo).is_none(), "success should clear the entry");
    }

    #[test]
    fn test_record_push_failure_returns_repo_state() {
        // See `test_record_push_failu[DRACON_SECRET:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSA1bXlHRU1mdjgzSnJSTWpDSjIvYVBLQ3R1R3lMTDllVmNtZ1ViVlU5TFhVCndBa0s0UXlVNzRjUndmSVRTMzN2ZmNFU3RkcEMxSEZlUjJJZzREK0FsTTAKLT4gWDI1NTE5IEVYbVJFbjNlNjFrN0gwdTJ1N1l4dG5PL0N2UFJMMVBiT1FhMkpYVTRxeG8KWHdSRWY3U1hVWUo2NHFON1o2WnFFSm92U29QaDJUc294L0dDOTdnbmdmSQotPiBYMjU1MTkgNVpBVW1BSUdkUDF1N1hkQUNwSHNHakJ4NW5rdDRpRTIyQlNFR1hrNm5tOApVY0pseWR3WXZ4NGJ5RDA1eTdLSW9MM2ZoQmdZd3p0cFlZNkI1Rm94Y1RRCi0+IFgyNTUxOSBGMUZXM0x3Lythcmwzb0FuSjRkMWZCcjlHbHc5S0VhSVpCa1JEWURYcFdzCjZDRGFldk1SRnJvcGp5MlBCM0l2OTZ5UmR6czJ1V1RON250RDBWMStMcUEKLT4gWDI1NTE5IHlnZGE5amZKbVVrWWRoZXBaTzZQN2JOd245VW1VWkVBVnRqU1lSbk82bWsKcCswYXRuMHEwVXE2Q3FqcHJxbzlSalU4Qm9OUHNaQzVQTEMwTGZUMnRLcwotPiB0NEctZ3JlYXNlCmlLMXZjR0tkU01KLytYSGp2TTdDcTcyQnJOc0M2TzFWVDZ6dlprMlV3aWtMRFJTK1AvdGlXK0xLCi0tLSBBeTA4S3VLQzh1ZERaL0wvVDRpaUFyNXF1MFh1dlNZRnE0WmpOL3k4bndvCjBF2hEU1d+fE8vRM+6Gcz+22zj64LeHthNoCxuxGZrAPy41PicyAu4JFMc/XJTT5qjRlOdg6SwQcC2D6g+JPrTyjw==]`
        // for the rationale on using a temp state dir.
        let temp_dir = tempfile::tempdir().unwrap();
        let _state_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            temp_dir.path().to_string_lossy().as_ref(),
        );
        let repo = make_test_repo_path("first-call-stuck-since");
        let _ = crate::daemon::unstuck_repo(&repo);

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        record_push_failure(&repo, "first err");
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let info = get_stuck_push_info(&repo).expect("entry should exist");
        assert!(
            info.stuck_since >= before && info.stuck_since <= after,
            "stuck_since should be set to current time on first failure: {} not in [{}, {}]",
            info.stuck_since,
            before,
            after
        );

        let _ = crate::daemon::unstuck_repo(&repo);
    }
}

#[cfg(test)]
mod daemon_tests {
    use super::*;

    #[test]
    fn test_stuck_repos_path_format() {
        let path = stuck_repos_path();
        assert!(path.to_string_lossy().contains(".local"));
    }

    /// Smoke test: confirm the policy field `sem_max_concurrent_sync`
    /// is wired into the daemon. This is a static check that the
    /// field exists and the default is 4 (so concurrent sync_repo
    /// calls are bounded to 4 at a time). The actual parallel
    /// dispatch is exercised by the live daemon cycle and tested
    /// in the integration suite.
    #[test]
    fn test_sem_max_concurrent_sync_default_is_four() {
        use crate::policy::default_sem_max_concurrent_sync;
        assert_eq!(default_sem_max_concurrent_sync(), 4);
    }

    #[test]
    fn test_stuck_repos_path_format_full() {
        let path = stuck_repos_path();
        assert!(path
            .to_string_lossy()
            .contains("dracon-sync-stuck-push-repos.json"));
    }

    /// Unit test for the no-redispatch invariant: a repo with an
    /// in-flight `sync_repo` task is NOT re-dispatched. The
    /// `in_flight: HashSet<PathBuf>` is consulted by the COLLECT
    /// phase's eligibility check. This test verifies the
    /// invariant by simulating the data structure: a repo inserted
    /// into `in_flight` is "skipped" in the next cycle's
    /// eligibility check, and only after removal from
    /// `in_flight` is the repo eligible again.
    #[test]
    fn test_no_redispatch_invariant() {
        use std::path::PathBuf;
        let mut in_flight: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let repo = PathBuf::from("/tmp/test-repo");

        // Initially, the repo is NOT in flight and IS eligible.
        assert!(!in_flight.contains(&repo));

        // Dispatch: insert into in_flight. Now the repo is NOT
        // eligible for re-dispatch in the next cycle.
        in_flight.insert(repo.clone());
        assert!(in_flight.contains(&repo));

        // The apply phase removes the repo from in_flight when
        // the task completes. After removal, the repo is
        // eligible again.
        in_flight.remove(&repo);
        assert!(!in_flight.contains(&repo));

        // Verify the set can hold multiple repos concurrently
        // (the daemon processes up to `sem_max_concurrent_sync`
        // repos in parallel).
        let repo2 = PathBuf::from("/tmp/test-repo-2");
        let repo3 = PathBuf::from("/tmp/test-repo-3");
        in_flight.insert(repo.clone());
        in_flight.insert(repo2.clone());
        in_flight.insert(repo3.clone());
        assert_eq!(in_flight.len(), 3);

        // Removing one repo does not affect the others.
        in_flight.remove(&repo2);
        assert_eq!(in_flight.len(), 2);
        assert!(in_flight.contains(&repo));
        assert!(!in_flight.contains(&repo2));
        assert!(in_flight.contains(&repo3));
    }

    /// Regression test for the trailing-drain bug discovered
    /// on 2026-06-15 during the `dracon-platform` push
    /// investigation. The bug: if a sync task (e.g. a 60s
    /// push) didn't complete within the trailing-drain
    /// deadline, the `in_flight` HashSet was never cleared for
    /// that task. The result: the COLLECT phase of every
    /// subsequent cycle skipped the repo, and it was never
    /// processed again until the daemon restarted.
    ///
    /// Fix: on trailing-drain timeout, clear all `in_flight`
    /// entries that were dispatched in this cycle but not
    /// drained. This test simulates the data structure: a
    /// repo inserted into `in_flight` (simulating dispatch) is
    /// still present after the dispatch; the trailing-drain
    /// timeout (simulated by not removing) would normally
    /// leave it stuck; the fix clears it.
    #[test]
    fn test_trailing_drain_clears_stuck_in_flight() {
        use std::path::PathBuf;
        let mut in_flight: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut dispatched_this_cycle: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();

        // Simulate dispatching 3 slow tasks.
        let repo1 = PathBuf::from("/tmp/slow-repo-1");
        let repo2 = PathBuf::from("/tmp/slow-repo-2");
        let repo3 = PathBuf::from("/tmp/slow-repo-3");
        in_flight.insert(repo1.clone());
        in_flight.insert(repo2.clone());
        in_flight.insert(repo3.clone());
        dispatched_this_cycle.insert(repo1.clone());
        dispatched_this_cycle.insert(repo2.clone());
        dispatched_this_cycle.insert(repo3.clone());
        assert_eq!(in_flight.len(), 3);

        // Simulate the trailing drain: only repo1 completes
        // within the deadline. The other 2 timeout.
        in_flight.remove(&repo1);
        dispatched_this_cycle.remove(&repo1);

        // The fix: on trailing-drain completion, clear all
        // remaining `dispatched_this_cycle` entries from
        // `in_flight` (these are the tasks that timed out and
        // are still running in the background).
        for repo in &dispatched_this_cycle {
            in_flight.remove(repo);
        }

        // After the fix, `in_flight` is empty. The next
        // cycle can re-dispatch repo2 and repo3.
        assert!(
            in_flight.is_empty(),
            "trailing-drain should clear all dispatched entries, but `in_flight` still contains: {:?}",
            in_flight
        );
    }

    #[test]
    fn test_load_stuck_push_repos_nonexistent() {
        let temp_dir = tempfile::tempdir().unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            temp_dir.path().to_string_lossy().as_ref(),
        );
        let repos = load_stuck_push_repos();
        assert!(repos.is_empty());
    }

    #[test]
    fn test_unstuck_repo_nonexistent() {
        let result = unstuck_repo(Path::new("/nonexistent/path"));
        assert!(!result);
    }

    #[test]
    fn test_list_stuck_repos_empty() {
        list_stuck_repos();
    }

    #[test]
    fn test_is_repo_stuck_false() {
        assert!(!is_repo_stuck(Path::new("/nonexistent/path")));
    }

    #[test]
    fn test_stuck_repos_path_home() {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let expected_base = home.join(".local").join("state").join("dracon");
        let path = stuck_repos_path();
        assert!(path.starts_with(expected_base));
    }

    #[test]
    fn test_skips_nonexistent_repo() {
        // If a repo is deleted between discovery and processing, the daemon
        // should skip it gracefully rather than panicking or erroring.
        use crate::git::discover_git_repos;
        use crate::policy::SyncPolicy;

        let policy = SyncPolicy::default();
        let excluded = crate::exclude::excluded_dir_names_set(&policy);

        // Nonexistent repo should not be discovered
        let repos = discover_git_repos(&[PathBuf::from("/nonexistent/path")], &excluded, &[], None);
        assert!(repos.is_empty(), "should not discover nonexistent paths");
    }

    #[test]
    fn test_is_repo_ready_nonexistent_path() {
        // is_repo_ready should return false for a repo path that doesn't exist
        assert!(!is_repo_ready(Path::new("/nonexistent/repo")));
    }

    #[test]
    fn test_policy_clone_at_repo_iteration() {
        // Verifies that a cloned SyncPolicy is an independent snapshot:
        // each repo iteration should clone the policy to avoid race conditions
        // from mid-cycle policy reloads (e.g., SIGHUP).
        use crate::policy::SyncPolicy;

        let policy = SyncPolicy::default();
        let cloned = policy.clone();

        // Debug format should match — same field values
        assert_eq!(format!("{:?}", policy), format!("{:?}", cloned));

        // Verify key fields are carried over
        assert_eq!(policy.auto_commit, cloned.auto_commit);
        assert_eq!(policy.auto_pull, cloned.auto_pull);
        assert_eq!(policy.auto_push, cloned.auto_push);
        assert_eq!(policy.pulse_interval_secs, cloned.pulse_interval_secs);
        assert_eq!(policy.push_retries, cloned.push_retries);
        assert_eq!(policy.max_stage_file_bytes, cloned.max_stage_file_bytes);
    }

    #[tokio::test]
    async fn test_get_status_refreshes_index() {
        // Verify that get_status() calls git update-index --refresh
        // by checking that a newly created repo returns correct status.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");

        // Initialize repo with a commit
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("file.txt"), "content").unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "commit",
                "-m",
                "init",
                "--no-verify",
            ])
            .status()
            .unwrap();

        // Get status should work and return clean repo with ahead=0
        let svc = GitService::new(&repo).unwrap();
        let status = svc.get_status().await.unwrap();
        assert!(status.is_clean, "repo should be clean");
        assert_eq!(status.ahead, 0, "ahead should be 0");
        assert_eq!(status.branch, "main");
    }

    #[tokio::test]
    async fn test_get_status_detects_unpushed_commits() {
        // Verify that get_status() correctly detects unpushed commits
        // after git update-index --refresh.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");

        // Initialize repo with remote
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        let remote = tmp.path().join("remote.git");
        crate::git::git_cmd()
            .args(["init", "--bare", "-q"])
            .arg(&remote)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                remote.to_str().unwrap(),
            ])
            .status()
            .unwrap();

        // Initial commit and push
        std::fs::write(repo.join("file.txt"), "v1").unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "commit",
                "-m",
                "init",
                "--no-verify",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "push", "-u", "origin", "main"])
            .status()
            .unwrap();

        // Unpushed commit
        std::fs::write(repo.join("file.txt"), "v2").unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "commit",
                "-m",
                "unpushed",
                "--no-verify",
            ])
            .status()
            .unwrap();

        let svc = GitService::new(&repo).unwrap();
        let status = svc.get_status().await.unwrap();
        assert_eq!(status.ahead, 1, "should detect 1 unpushed commit");
        assert!(
            !status.is_clean || status.ahead > 0,
            "repo should not be fully synced"
        );
    }

    #[tokio::test]
    async fn test_get_status_after_push() {
        // Verify that get_status() returns ahead=0 after pushing,
        // confirming git update-index --refresh works correctly.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("test-repo");

        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        let remote = tmp.path().join("remote.git");
        crate::git::git_cmd()
            .args(["init", "--bare", "-q"])
            .arg(&remote)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                remote.to_str().unwrap(),
            ])
            .status()
            .unwrap();

        // Initial commit and push
        std::fs::write(repo.join("file.txt"), "v1").unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "commit",
                "-m",
                "init",
                "--no-verify",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "push", "-u", "origin", "main"])
            .status()
            .unwrap();

        // Create and push another commit
        std::fs::write(repo.join("file.txt"), "v2").unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "-C",
                repo.to_str().unwrap(),
                "commit",
                "-m",
                "second",
                "--no-verify",
            ])
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["-C", repo.to_str().unwrap(), "push"])
            .status()
            .unwrap();

        let svc = GitService::new(&repo).unwrap();
        let status = svc.get_status().await.unwrap();
        assert_eq!(status.ahead, 0, "ahead should be 0 after push");
        assert!(status.is_clean, "repo should be clean after push");
    }
}

fn stuck_repos_path() -> PathBuf {
    if let Ok(state_dir) = std::env::var("DRACON_SYNC_STATE_DIR") {
        if !state_dir.is_empty() {
            return PathBuf::from(state_dir).join("dracon-sync-stuck-push-repos.json");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("state")
        .join("dracon")
        .join("dracon-sync-stuck-push-repos.json")
}

fn load_stuck_push_repos() -> HashMap<PathBuf, StuckRepoEntry> {
    let path = stuck_repos_path();
    if !path.exists() {
        return HashMap::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("⚠️ failed reading stuck repos ({}): {}", path.display(), e);
            return HashMap::new();
        }
    };
    let entries: Vec<StuckRepoEntry> = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!("⚠️ failed parsing stuck repos ({}): {}", path.display(), e);
        Vec::new()
    });
    let now = timestamp_secs();
    let cutoff = now.saturating_sub(STUCK_REPO_EXPIRY_SECS);
    entries
        .into_iter()
        .filter(|e| e.stuck_since > cutoff)
        .map(|e| (e.path.clone(), e))
        .collect()
}

    // ── Tests for the new settling_max_delay_secs + DirtyMaxAgeAction + ownership ──

    /// Verify the new policy fields default to safe values. A
    /// regression here would mean a new release accidentally
    /// changed the default for `auto_skip_unowned` (which MUST
    /// stay `true` for safety) or `settling_max_delay_secs` (which
    /// is the user-visible "auto-commit delay" knob).
    #[test]
    fn test_settling_max_delay_default_is_60() {
        use crate::policy::{
            default_dirty_max_age_action, default_min_commit_interval_secs,
            default_settling_max_delay_secs,
        };
        assert_eq!(default_settling_max_delay_secs(), 60);
        assert_eq!(default_min_commit_interval_secs(), 5);
        assert_eq!(default_dirty_max_age_action(), crate::policy::DirtyMaxAgeAction::Commit);
    }

    /// Verify `auto_skip_unowned` defaults to `true` (safety
    /// first). A regression here would silently disable the
    /// ownership safety guard rail.
    #[test]
    fn test_auto_skip_unowned_default_is_true() {
        use crate::policy::default_true;
        assert!(default_true());
    }

    // ---- stuck_decision + ledger accumulation (v0.112.31, audit H5/F1.2) ----

    #[cfg(test)]
    fn stuck_entry(failures: u32, stuck_since: u64, last_error_at: u64, last_retry_at: u64) -> StuckRepoEntry {
        StuckRepoEntry {
            path: std::path::PathBuf::from("/tmp/test-repo"),
            stuck_since,
            consecutive_failures: failures,
            last_error: "boom".to_string(),
            last_error_at,
            last_retry_at,
        }
    }

    #[test]
    fn test_stuck_decision_backoff_inside_window() {
        let info = stuck_entry(2, 1000, 1900, 0);
        // now = 2000, last activity 1900 → 100s < 300s backoff
        assert_eq!(
            stuck_decision(&info, 2000, 5, 300),
            StuckDecision::Backoff
        );
    }

    #[test]
    fn test_stuck_decision_retry_after_window() {
        let info = stuck_entry(2, 1000, 1500, 0);
        // 500s since last failure → retry
        assert_eq!(
            stuck_decision(&info, 2000, 5, 300),
            StuckDecision::Retry
        );
    }

    #[test]
    fn test_stuck_decision_retry_respects_last_retry_stamp() {
        // A retry was attempted recently (last_retry_at = 1950) —
        // even though the last FAILURE is old, the backoff runs from
        // the attempt so a non-push failure doesn't spin every cycle.
        let info = stuck_entry(2, 1000, 1500, 1950);
        assert_eq!(
            stuck_decision(&info, 2000, 5, 300),
            StuckDecision::Backoff
        );
        assert_eq!(
            stuck_decision(&info, 2300, 5, 300),
            StuckDecision::Retry
        );
    }

    #[test]
    fn test_stuck_decision_exhausted_at_budget() {
        // Regression: pre-fix, the retry path deleted the entry and
        // the budget reset to 1 every 5 minutes, so Exhausted could
        // never fire. Now consecutive_failures accumulates.
        let info = stuck_entry(5, 1000, 1999, 0);
        assert_eq!(
            stuck_decision(&info, 2000, 5, 300),
            StuckDecision::Exhausted,
            "consecutive_failures >= push_max_retries must stop auto-push"
        );
        // Exhausted wins even inside the backoff window.
        let info2 = stuck_entry(6, 1000, 1999, 1999);
        assert_eq!(stuck_decision(&info2, 2000, 5, 300), StuckDecision::Exhausted);
        // Just below budget → normal retry flow.
        let info3 = stuck_entry(4, 1000, 1500, 0);
        assert_eq!(stuck_decision(&info3, 2000, 5, 300), StuckDecision::Retry);
    }

    #[test]
    fn test_record_push_failure_accumulates_across_calls() {
        // Regression for the budget-reset defect: consecutive
        // failures must ACCUMULATE (the old loop deleted the entry
        // before each retry, so record_push_failure re-created it
        // with consecutive_failures = 1 every 5 minutes).
        let state_dir = tempfile::tempdir().unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let repo = std::path::Path::new("/tmp/h5-accum-test-repo");
        record_push_failure(repo, "fail 1");
        record_push_failure(repo, "fail 2");
        record_push_failure(repo, "fail 3");
        let repos = load_stuck_push_repos();
        let entry = repos.get(repo).expect("entry must exist");
        assert_eq!(entry.consecutive_failures, 3);
        assert_eq!(entry.last_error, "fail 3");
        assert!(entry.last_error_at > 0);
        // Success clears the entry.
        record_push_success(repo);
        assert!(load_stuck_push_repos().get(repo).is_none());
    }

    /// ADDED 2026-07-21 (v0.112.31, audit H4/F1.1): the notification
    /// throttle must (a) allow the FIRST notification, (b) suppress
    /// repeats inside the cooldown, and (c) RE-FIRE after the
    /// deadline. The previous `Entry::Vacant` pattern never expired —
    /// (c) is the regression this guards.
    #[test]
    fn test_notify_throttled_fires_then_suppresses_then_refires() {
        let mut map: HashMap<String, Instant> = HashMap::new();
        let cooldown = Duration::from_secs(1800);

        // (a) first call fires.
        assert!(notify_throttled(&mut map, "k1", cooldown));
        // (b) immediate repeat is suppressed.
        assert!(!notify_throttled(&mut map, "k1", cooldown));
        assert!(!notify_throttled(&mut map, "k1", cooldown));
        // Different key still fires.
        assert!(notify_throttled(&mut map, "k2", cooldown));

        // (c) after the deadline, the notification re-fires.
        // Simulate an expired entry by backdating it.
        map.insert(
            "k1".to_string(),
            Instant::now() - Duration::from_secs(1),
        );
        assert!(
            notify_throttled(&mut map, "k1", cooldown),
            "expired cooldown must re-fire (regression: old Entry::Vacant pattern fired once ever)"
        );
        // And the re-fire re-arms the cooldown.
        assert!(!notify_throttled(&mut map, "k1", cooldown));
    }

    /// Verify the ownership detection works end-to-end on a real
    /// test repo with an untrusted email.
    #[test]
    fn test_ownership_detection_end_to_end() {
        use crate::ownership::{detect_ownership, OwnershipReport, TrustedSet};
        use crate::test_helpers::create_test_repo;
        let repo = create_test_repo();
        let trusted = TrustedSet {
            emails: vec!["dracsharp@gmail.com".to_string()],
            authors: vec!["DraconDev".to_string()],
            remote_hosts: vec!["github.com/DraconDev".to_string()],
        };
        // Override the trusted list to one the actual email
        // is NOT in → expect Unowned with reason
        // `untrusted_email`.
        let mut untrusted = trusted.clone();
        untrusted.emails = vec!["definitely-not-our-email@void".to_string()];
        let report = detect_ownership(&repo, &untrusted, None);
        match report {
            OwnershipReport::Unowned { reason, .. } => {
                assert_eq!(reason, "untrusted_email");
            }
            other => panic!("expected Unowned, got {:?}", other),
        }
        // With the right trusted list, the override path
        // always returns Owned.
        let owned_report = detect_ownership(&repo, &trusted, Some(true));
        assert!(matches!(owned_report, OwnershipReport::Owned { .. }));
    }

    /// Verify the per-repo override `auto_skip_unowned = false`
    /// forces Unowned even on a fully-trusted repo.
    #[test]
    fn test_ownership_per_repo_override_forces_unowned() {
        use crate::ownership::{detect_ownership, OwnershipReport, TrustedSet};
        use crate::test_helpers::create_test_repo;
        let repo = create_test_repo();
        let trusted = TrustedSet {
            emails: vec!["dracsharp@gmail.com".to_string()],
            authors: vec!["DraconDev".to_string()],
            remote_hosts: vec!["github.com/DraconDev".to_string()],
        };
        // With `owned = false` override on a fully-trusted
        // repo, the result is Unowned with reason `override`.
        let report = detect_ownership(&repo, &trusted, Some(false));
        match report {
            OwnershipReport::Unowned { reason, .. } => {
                assert_eq!(reason, "override");
            }
            other => panic!("expected Unowned, got {:?}", other),
        }
    }

fn save_stuck_push_repos(repos: &HashMap<PathBuf, StuckRepoEntry>) {
    let path = stuck_repos_path();
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("⚠️ failed creating stuck repos dir: {}", e);
                return;
            }
        }
    }
    let entries: Vec<StuckRepoEntry> = repos.values().cloned().collect();
    let content = serde_json::to_string_pretty(&entries).unwrap_or_else(|e| {
        eprintln!("⚠️ failed serializing stuck repos: {}", e);
        String::new()
    });
    if content.is_empty() {
        return;
    }
    let tmp_path = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp_path, &content) {
        eprintln!(
            "⚠️ failed writing stuck repos tmp ({}): {}",
            tmp_path.display(),
            e
        );
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        eprintln!("⚠️ failed renaming stuck repos ({}): {}", path.display(), e);
        let _ = std::fs::remove_file(&tmp_path);
    }
}

pub(crate) fn unstuck_repo(repo: &Path) -> bool {
    let path = stuck_repos_path();
    if !path.exists() {
        return false;
    }
    let mut repos = load_stuck_push_repos();
    if repos.remove(repo).is_some() {
        save_stuck_push_repos(&repos);
        eprintln!("🔓 unstuck: {}", repo.display());
        true
    } else {
        eprintln!("ℹ️ {} not in stuck repos", repo.display());
        false
    }
}

/// Record a successful push for `repo`. Resets
/// `consecutive_failures` to 0 and removes the entry from
/// the stuck repos file if present. Called from
/// `push_background`'s callers when a push succeeds.
pub(crate) fn record_push_success(repo: &Path) {
    let mut repos = load_stuck_push_repos();
    if repos.remove(repo).is_some() {
        save_stuck_push_repos(&repos);
        eprintln!("✅ push recovered for {}", repo.display());
    }
}

/// Record a failed push for `repo` with the given error
/// message. Increments `consecutive_failures` and updates
/// `last_error` + `last_error_at`. If `consecutive_failures`
/// reaches `push_max_retries`, the entry's `last_error` is
/// preserved (so the operator can see WHY it's stuck) and the
/// report will surface a `🛑 push-stuck` state.
pub(crate) fn record_push_failure(repo: &Path, error: &str) {
    let mut repos = load_stuck_push_repos();
    let now = timestamp_secs();
    let entry = repos.entry(repo.to_path_buf()).or_insert_with(|| StuckRepoEntry {
        path: repo.to_path_buf(),
        stuck_since: now,
        consecutive_failures: 0,
        last_error: String::new(),
        last_error_at: 0,
        last_retry_at: 0,
    });
    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
    entry.last_error = error.to_string();
    entry.last_error_at = now;
    // If this is the first time the repo gets stuck, set the
    // stuck_since timestamp so the 5-minute retry backoff works.
    if entry.consecutive_failures == 1 {
        entry.stuck_since = now;
    }
    save_stuck_push_repos(&repos);
}

/// Read-only access to the stuck repos map, for the report
/// to surface `consecutive_failures` and `last_error` in the
/// HINT column.
pub(crate) fn get_stuck_push_info(repo: &Path) -> Option<StuckRepoEntry> {
    load_stuck_push_repos().get(repo).cloned()
}

/// ADDED 2026-07-21 (v0.112.31, audit H4/F1.1): notification throttle
/// that ACTUALLY expires. The previous pattern at every call site was:
///
/// ```rust,ignore
/// if let Entry::Vacant(e) = map.entry(key) {
///     notify();
///     e.insert(Instant::now() + Duration::from_secs(1800));
/// }
/// ```
///
/// The stored deadline was NEVER read — once inserted, the key was
/// `Occupied` forever, so every throttled notification (ownership-skip
/// reminder, stuck-retry alert, push-failure, Stuck-Ahead/Behind,
/// Mirror-Degraded) fired exactly ONCE per daemon lifetime.
///
/// This helper returns `true` (and re-arms the cooldown) when a
/// notification with `key` should be sent: either it was never sent,
/// or the stored deadline has passed.
pub(crate) fn notify_throttled(
    map: &mut HashMap<String, Instant>,
    key: &str,
    cooldown: Duration,
) -> bool {
    let now = Instant::now();
    let expired = map.get(key).is_none_or(|until| now >= *until);
    if expired {
        map.insert(key.to_string(), now + cooldown);
        true
    } else {
        false
    }
}

/// Path to the in-flight state file. The daemon writes the current
/// `in_flight: HashSet<PathBuf>` to this file on every cycle, atomically
/// (write-temp + rename). The `repos` command reads this file to
/// distinguish between "actively being processed" and "stalled" rows.
///
/// File location: `~/.local/state/dracon/dracon-sync-in-flight.json`.
/// Self-cleaning: when `in_flight` is empty, the daemon removes the file.
fn in_flight_path() -> std::path::PathBuf {
    // Locate the state directory. We use the same convention as
    // `incident_ledger_path` in report.rs:
    //   $DRACON_SYNC_LEDGER parent > ~/.local/state/dracon > /tmp/dracon
    if let Ok(custom) = std::env::var("DRACON_SYNC_LEDGER") {
        let p = std::path::PathBuf::from(custom);
        if !p.as_os_str().is_empty() {
            if let Some(parent) = p.parent() {
                return parent.join("dracon-sync-in-flight.json");
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        return home
            .join(".local")
            .join("state")
            .join("dracon")
            .join("dracon-sync-in-flight.json");
    }
    std::path::PathBuf::from("/tmp/dracon-sync-in-flight.json")
}

/// Atomically write the current `in_flight` set to disk. Used by the
/// `repos` command to display whether a row is actively being processed
/// (`now`) or has been quiet for a while (`stalled Xm`).
pub(crate) fn save_in_flight(repos: &HashSet<PathBuf>) {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut paths: Vec<String> = repos.iter().map(|p| p.display().to_string()).collect();
    paths.sort();
    save_in_flight_at(&paths, now_unix);
}

/// Test-only helper: write the in_flight file with a caller-supplied
/// `written_at` epoch so staleness tests can simulate "old file".
#[cfg(test)]
pub(crate) fn save_in_flight_for_test(paths: &[std::path::PathBuf], written_at: u64) {
    let strs: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    save_in_flight_at(&strs, written_at);
}

fn save_in_flight_at(paths: &[String], written_at: u64) {
    let path = in_flight_path();
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if paths.is_empty() {
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        return;
    }
    let content = match serde_json::to_string_pretty(&serde_json::json!({
        "in_flight": paths,
        "written_at": written_at,
    })) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("⚠️ failed serializing in_flight: {}", e);
            return;
        }
    };
    let tmp_path = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp_path, &content) {
        let _ = std::fs::remove_file(&tmp_path);
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("⚠️ failed writing in_flight tmp: {}", e);
        }
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        eprintln!("⚠️ failed renaming in_flight file: {}", e);
        let _ = std::fs::remove_file(&tmp_path);
    }
}

/// Test-only helper: return the on-disk in_flight file path so
/// tests can clean up after themselves.
#[cfg(test)]
pub(crate) fn in_flight_path_for_test() -> std::path::PathBuf {
    in_flight_path()
}

/// Read the current `in_flight` set from disk. Used by the
/// `repos` command to render the ACTIVITY column with the
/// active/stalled distinction. Returns an empty set if the file
/// does not exist (no daemon activity, or daemon not running).
pub(crate) fn load_in_flight() -> HashSet<PathBuf> {
    let path = in_flight_path();
    if !path.exists() {
        return HashSet::new();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return HashSet::new(),
    };
    let mut set = HashSet::new();
    if let Some(arr) = parsed.get("in_flight").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(s) = v.as_str() {
                set.insert(std::path::PathBuf::from(s));
            }
        }
    }
    set
}

/// Return the age of the on-disk in_flight file in seconds, or `None`
/// if the file is missing or its `written_at` field is unparseable.
/// Used by the report to filter out stale "🔄 now" indicators when
/// the daemon's in_flight file hasn't been refreshed in over a
/// cycle's worth of time (a sign that a slow push from the
/// previous cycle is still running while the daemon has moved on).
pub(crate) fn in_flight_file_age_secs() -> Option<u64> {
    let path = in_flight_path();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let written_at = parsed.get("written_at").and_then(|v| v.as_u64())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(now.saturating_sub(written_at))
}

pub(crate) fn list_stuck_repos() {
    let repos = load_stuck_push_repos();
    if repos.is_empty() {
        eprintln!("✅ no stuck repos");
        return;
    }
    eprintln!("🔒 stuck repos (expire after 24h):");
    let now = timestamp_secs();
    for (path, info) in repos {
        let age_hrs = (now.saturating_sub(info.stuck_since)) / 3600;
        eprintln!(
            "   {} ({}h ago, {} consecutive failures)",
            path.display(),
            age_hrs,
            info.consecutive_failures
        );
    }
}

pub(crate) fn is_repo_stuck(repo: &Path) -> bool {
    load_stuck_push_repos().contains_key(repo)
}

/// Run startup cleanup: prune stale state from previous runs.
/// Called by both `run_once` (for one-shot sync) and `run_daemon` (on startup).
/// Returns the number of stale index.lock files removed.
pub(crate) async fn run_startup_cleanup(policy_path: &Path) -> (BTreeSet<PathBuf>, u64) {
    eprintln!("🧹 startup: running cleanup...");
    let policy = match SyncPolicy::load(policy_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("⚠️ failed loading policy for startup cleanup: {}", e);
            SyncPolicy::default()
        }
    };
    let roots = policy.watch_root_paths();
    let excluded_dir_names = excluded_dir_names_set(&policy);
    let discovered = discover_git_repos(
        &roots,
        &excluded_dir_names,
        &policy.exclude_repos,
        Some(&policy.system_repo),
    );
    let repo_set: BTreeSet<PathBuf> = discovered.iter().cloned().collect();

    // Prune stuck repos no longer on disk
    let mut stuck_push_repos = load_stuck_push_repos();
    let before = stuck_push_repos.len();
    stuck_push_repos.retain(|repo, _| repo_set.contains(repo));
    if stuck_push_repos.len() != before {
        save_stuck_push_repos(&stuck_push_repos);
        eprintln!(
            "🧹 startup: pruned {} stale stuck repos",
            before - stuck_push_repos.len()
        );
    }

    // Enforce incident ledger retention now
    if let Err(e) = crate::report::enforce_retention_at_startup(policy_path, &policy) {
        eprintln!("⚠️ startup: incident ledger cleanup failed: {}", e);
    }

    // Prune visibility cache for deleted repos
    if let Err(e) = crate::visibility::prune_stale_visibility_cache(&repo_set) {
        eprintln!("⚠️ startup: visibility cache cleanup failed: {}", e);
    }

    // Repair broken upstream tracking references (e.g. origin/master: gone)
    let discovered_refs: Vec<PathBuf> = repo_set.iter().cloned().collect();
    let fixed = repair_broken_tracking(&discovered_refs);
    if fixed > 0 {
        eprintln!(
            "🧹 startup: repaired {} broken upstream tracking refs",
            fixed
        );
    }

    // Remove stale .git/index.lock files from crashed git processes.
    // A lock file with no holding process prevents all git operations.
    let mut locks_removed = 0u64;
    for repo in &repo_set {
        let lock = repo.join(".git/index.lock");
        if lock.exists() {
            eprintln!(
                "🧹 startup: found index.lock in {} (checking fuser...)",
                repo.display()
            );
            let in_use = std::process::Command::new("fuser")
                .arg(&lock)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !in_use {
                if let Err(e) = std::fs::remove_file(&lock) {
                    eprintln!("⚠️ startup: failed to remove {}: {}", lock.display(), e);
                } else {
                    locks_removed += 1;
                }
            }
        }
    }
    if locks_removed > 0 {
        eprintln!(
            "🧹 startup: removed {} stale .git/index.lock files",
            locks_removed
        );
    }

    (repo_set, locks_removed)
}

/// Walk the just-discovered repos and ensure each submodule declared in
/// `.gitmodules` has its multi-remote set (github / gitlab / codeberg)
/// configured on the **nested** submodule path when that path is checked
/// out on `main`. The daemon syncs the nested path directly (the
/// nested-on-main architecture); it does NOT materialize a standalone
/// worktree at the watch root.
///
/// Materializing standalone worktrees at `/home/dracon/Dev/<name>/` was
/// added 2026-06-30 (goal `mr10pdzr-i495vy`) but contradicted the
/// documented design ("there is no standalone at /home/dracon/Dev/<name>/"),
/// and was removed 2026-07-08 (goal `730eaf2a`): discovery filters the
/// standalone out, so it is never synced, and detached-agent checkouts
/// kept recreating it.
///
/// If the nested submodule is NOT on `main` (detached HEAD or not yet
/// initialized), the daemon simply skips it — it does not create a
/// redundant standalone. Any pre-existing standalone is pruned
/// out-of-band by the operator (`git worktree remove --force`).
pub(crate) async fn materialize_pending_submodules(
    repos: &[PathBuf],
    _roots: &[PathBuf],
    policy: &SyncPolicy,
) {
    for parent in repos {
        let subs = list_submodules(parent);
        for sub in subs {
            // Skip submodules with no tracked SHA (not in
            // parent's index). list_submodules returns these with
            // sha = ""; without a SHA we can't materialize.
            if sub.sha.is_empty() {
                if debug_enabled() {
                    eprintln!(
                        "🐛 {} submodule {} declared in .gitmodules but not tracked in index; skipping materialize",
                        parent.display(),
                        sub.name
                    );
                }
                continue;
            }
            // Compute the candidate worktree path. We anchor on
            // the watch root the parent was found under so the
            // worktree is at the same level as the parent
            // (e.g. /home/dracon/Dev/polis/ for dracon-platform's
            // web-games-polis submodule), not nested inside the
            // parent.
            let worktree_name = Path::new(&sub.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| sub.name.clone());
            // Nested-on-main architecture: the canonical watch path is the
            // nested submodule path inside the parent (e.g.
            // `<parent>/<sub.path>`). The daemon syncs that path directly
            // and must NOT create a standalone worktree at the watch root.
            // Materialization was removed 2026-07-08 (goal `730eaf2a`) because
            // it contradicted the documented design and kept being recreated
            // by detached-agent checkouts.
            let nested_submodule_path = parent.join(&sub.path);
            let nested_on_main = nested_submodule_path.exists()
                && nested_submodule_path.join(".git").exists()
                && is_on_main_branch(&nested_submodule_path);
            if nested_on_main {
                if debug_enabled() {
                    eprintln!(
                        "🐛 skipping materialize for {} (nested submodule at {} is on main, canonical watch path)",
                        sub.name,
                        nested_submodule_path.display()
                    );
                }
                // Still configure remotes on the nested path so
                // the daemon can push from there. Fall through
                // to the multi-remote configure step but using
                // the nested path as the target.
                let repo_override =
                    crate::policy::load_repo_override(&nested_submodule_path);
                let mut combined_exclude = repo_override.exclude_remotes.clone();
                // ADDED 2026-07-17 (goal `codeberg-public-only`):
                // same gate as the standalone path above. Nested
                // submodules need the same exclusion so they
                // don't accumulate dead codeberg entries.
                let codeberg_public_only_effective = repo_override
                    .codeberg_public_only
                    .unwrap_or(policy.codeberg_public_only);
                if codeberg_public_only_effective {
                    let cached_priv =
                        crate::visibility::cached_repo_visibility(&nested_submodule_path);
                    let skip_codeberg = match cached_priv {
                        Some(true) | None => true,
                        Some(false) => false,
                    };
                    if skip_codeberg && !combined_exclude.iter().any(|e| e == "codeberg") {
                        combined_exclude.push("codeberg".to_string());
                    }
                }
                crate::git::multi_remote::configure_all_remotes(
                    &nested_submodule_path,
                    &policy.remotes,
                    &worktree_name,
                    &combined_exclude,
                );
                continue;
            }
            // Nested submodule is NOT on `main` (detached HEAD, or not yet
            // initialized). Per the nested-on-main architecture the daemon
            // syncs the nested path directly and must NOT materialize a
            // redundant standalone worktree at the watch root: discovery
            // filters the standalone out (so it is never synced), and
            // creating it contradicts the documented design ("there is no
            // standalone at /home/dracon/Dev/<name>/"). Any pre-existing
            // standalone is pruned out-of-band by the operator
            // (`git worktree remove --force`); the daemon simply skips it
            // here.
            continue;
        }
    }
}


/// Returns true if the worktree at `path` is checked out on a real
/// branch (e.g. `main`), not in detached HEAD state. Used by
/// `materialize_pending_submodules` to decide whether the nested
/// submodule path is the canonical watch path (skip standalone
/// materialize) or whether the legacy standalone-at-watch-root is
/// still needed.
///
/// ADDED 2026-07-02 (goal `mr3g843f-lajfpg`).
fn is_on_main_branch(path: &Path) -> bool {
    // We can't directly use `git -C <path> branch --show-current`
    // from a sync context (it would block if a long-running
    // operation is in progress), so we read the worktree's HEAD
    // file directly. For worktree-style checkouts (`.git` file
    // pointing to `<shared_gitdir>/worktrees/<X>`), the HEAD
    // file is at `<shared_gitdir>/worktrees/<X>/HEAD`.
    let dot_git = path.join(".git");
    if !dot_git.exists() {
        return false;
    }
    // For `.git` files (worktree-style), the HEAD is in
    // `<gitdir>/HEAD`. For `.git/` directories (main worktree),
    // the HEAD is `<gitdir>/HEAD`.
    let head_path = if dot_git.is_file() {
        let Ok(content) = std::fs::read_to_string(&dot_git) else {
            return false;
        };
        let Some(rest) = content.trim().strip_prefix("gitdir:") else {
            return false;
        };
        let gitdir_rel = rest.trim();
        let base_canon =
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let resolved = base_canon.join(gitdir_rel);
        let Ok(canon_resolved) = std::fs::canonicalize(&resolved) else {
            return false;
        };
        canon_resolved.join("HEAD")
    } else {
        std::fs::canonicalize(&dot_git)
            .ok()
            .map(|p| p.join("HEAD"))
            .unwrap_or_else(|| dot_git.join("HEAD"))
    };
    let Ok(head_content) = std::fs::read_to_string(&head_path) else {
        return false;
    };
    head_content.starts_with("ref: refs/heads/")
}

pub(crate) async fn run_once(policy_path: &Path) -> Result<()> {
    if let Some(reason) = freeze_reason(policy_path) {
        eprintln!("⏸️ sync frozen ({})", reason);
        return Ok(());
    }

    // Clean up stale state from previous runs (including index.lock files)
    let (repo_set, _) = run_startup_cleanup(policy_path).await;

    let policy = SyncPolicy::load(policy_path)?;
    let excluded_dir_names = excluded_dir_names_set(&policy);

    let mut changed = 0usize;
    for repo in &repo_set {
        // Guard against repo-discovery race
        if !repo.exists() {
            eprintln!(
                "⚠️ {} repo path vanished between discovery and sync, skipping",
                repo.display()
            );
            continue;
        }
        match sync_repo(
            repo,
            &policy,
            &excluded_dir_names,
            0,
            None,
            false,
            Some(policy_path),
        )
        .await
        {
            Ok(SyncOutcome::Synced) => {
                changed += 1;
                println!("🔁 synced {}", repo.display());
            }
            // ADDED 2026-07-21 (v0.112.31, audit H3/F1.3): count as
            // changed (the commit landed) but warn about the push.
            Ok(SyncOutcome::PushFailed) => {
                changed += 1;
                eprintln!("⚠️ {} committed but push failed", repo.display());
            }
            Ok(SyncOutcome::NothingToDo) | Ok(SyncOutcome::Blocked) => {}
            Err(e) => {
                eprintln!("⚠️ sync failed for {}: {}", repo.display(), e);
            }
        }
    }

    println!("✅ sync pass complete (repos changed: {})", changed);
    if policy.auto_repair_concerns {
        if let Err(e) = run_repair_concerns(
            policy_path,
            true,
            None,
            Some(policy.push_op_timeout_secs),
            policy.push_retries,
            policy.auto_rewrite_large_blobs,
            ConcernRepairFilter::All,
            false,
        )
        .await
        {
            eprintln!("⚠️ auto-repair concerns failed: {}", e);
        }
    }
    if policy.auto_repair_warns {
        if let Err(e) = run_repair_warns(policy_path, true, None, false).await {
            eprintln!("⚠️ auto-repair warns failed: {}", e);
        }
    }
    Ok(())
}

pub(crate) async fn run_daemon(
    policy_path: PathBuf,
    override_interval_secs: Option<u64>,
) -> Result<()> {
    // Note: Rust's stdio buffers are separate from C's FILE* buffers.
    // When running under systemd (socket-based journal capture), Rust defaults
    // to block buffering. We can't use setvbuf on Rust's handles, so instead
    // we flush stderr at strategic points in the daemon loop (see flush calls below).
    eprintln!("🔄 dracon-sync daemon started");
    #[derive(Debug, Clone)]
    struct RepoActivity {
        fingerprint: String,
        changed_at: Instant,
        /// When the repo first became dirty in this cycle.
        /// Unlike changed_at, this doesn't reset on fingerprint changes.
        dirty_since: Option<Instant>,
        /// When the repo first became ahead of origin (unpushed commits).
        ahead_since: Option<Instant>,
        /// When the repo first became behind origin (unpulled commits).
        behind_since: Option<Instant>,
        /// Which mirrors have failed consecutively (name → consecutive fail count).
        mirror_consecutive_fails: HashMap<String, usize>,
        failure_count: usize,
        remote_failures: HashMap<String, usize>,
        /// Cached ownership report for this repo. Re-computed
        /// once per cycle when missing; never re-computed during
        /// the same cycle. The daemon uses this to skip
        /// auto-commit / auto-push for repos classified as
        /// `Unowned` or `Unknown` (when `auto_skip_unowned = true`).
        /// `None` means not yet classified this cycle.
        ownership: Option<crate::ownership::OwnershipReport>,
    }

    let mut activity: HashMap<PathBuf, RepoActivity> = HashMap::new();
    let mut pending_repos: HashMap<PathBuf, Instant> = HashMap::new();
    let mut initial_repos: HashSet<PathBuf>; // populated after first scan
    let mut repair_cooldowns: HashMap<PathBuf, Instant> = HashMap::new();
    let mut filter_cooldowns: HashMap<PathBuf, Instant> = HashMap::new();
    let mut stage_cooldowns: HashMap<PathBuf, Instant> = HashMap::new();
    // ADDED 2026-07-21 (v0.112.30): per-repo cooldown for the
    // empty-repo root-commit bootstrap. On bootstrap failure (e.g.
    // git user.email not configured anywhere), back off for 5 minutes
    // instead of retrying — and logging — every 1s cycle.
    let mut empty_bootstrap_cooldowns: HashMap<PathBuf, Instant> = HashMap::new();
    // ADDED 2026-07-21 (v0.112.30): per-repo cooldown for the
    // auto-create-on-discovery call (`push_mirror_remotes_create_only`
    // runs `git ls-remote` against every configured remote — an SSH
    // round-trip). The v0.112.29 version ran it every 1s cycle for
    // every not-ready repo, producing 2 SSH connections/sec per empty
    // repo forever. Throttle to one attempt per 5 minutes per repo;
    // the first attempt is immediate (map starts empty).
    let mut auto_create_cooldowns: HashMap<PathBuf, Instant> = HashMap::new();
    // CHANGED 2026-07-21 (v0.112.31, audit H5/F1.2): the loop
    // reloads the ledger from disk at the top of every cycle; the
    // startup load below is used for the operator-visible summary of
    // repos entering the daemon already stuck (and keeps the
    // initial assignment read, not dead).
    let mut stuck_push_repos: HashMap<PathBuf, StuckRepoEntry> = load_stuck_push_repos();
    if !stuck_push_repos.is_empty() {
        eprintln!(
            "📋 startup: {} repo(s) in the stuck-push ledger: {}",
            stuck_push_repos.len(),
            stuck_push_repos
                .iter()
                .map(|(p, e)| format!(
                    "{} ({} failures)",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    e.consecutive_failures
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let mut remote_notify_cooldowns: HashMap<String, Instant> = HashMap::new();
    let mut cycle_count: u64 = 0;
    // Repos with an active `sync_repo` task. The COLLECT phase
    // consults this set and skips re-dispatching repos that already
    // have an in-flight task. This is the no-redispatch invariant:
    // once we start a push, we don't start another one for the same
    // repo until the in-flight one completes (success or failure).
    // Without this, a slow push (e.g. dracon-platform's 60s timeout
    // on a 19-commit reorg) causes the next cycle to dispatch a
    // *second* push for the same repo while the first is still
    // running, saturating the SSH agent and network, and creating a
    // 2-3 minute "traffic jam" that delays smaller pushes (1-commit
    // rust-ai-web-auto, folder-auto-banner, one-mil-girls.deprecated).
    let mut in_flight: HashSet<PathBuf> = HashSet::new();

    // ── Startup cleanup: prune stale state from previous runs ──
    let (repo_set, _) = run_startup_cleanup(&policy_path).await;
    initial_repos = repo_set.iter().cloned().collect();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_sigterm = shutdown.clone();
    let shutdown_sigint = shutdown.clone();
    let reload = Arc::new(AtomicBool::new(false));
    let reload_sighup = reload.clone();

    tokio::spawn(async move {
        if let Ok(mut sig) = tokio::signal::unix::signal(SignalKind::terminate()) {
            sig.recv().await;
            veprintln!(1, "sync: received SIGTERM, shutting down gracefully...");
            shutdown_sigterm.store(true, Ordering::SeqCst);
        } else {
            eprintln!("sync: failed to set up SIGTERM handler");
        }
    });

    tokio::spawn(async move {
        if let Ok(mut sig) = tokio::signal::unix::signal(SignalKind::interrupt()) {
            sig.recv().await;
            veprintln!(1, "sync: received SIGINT, shutting down gracefully...");
            shutdown_sigint.store(true, Ordering::SeqCst);
        } else {
            eprintln!("sync: failed to set up SIGINT handler");
        }
    });

    tokio::spawn(async move {
        if let Ok(mut sig) = tokio::signal::unix::signal(SignalKind::hangup()) {
            while sig.recv().await.is_some() {
                veprintln!(1, "sync: received SIGHUP, will reload policy...");
                reload_sighup.store(true, Ordering::SeqCst);
            }
        } else {
            eprintln!("sync: failed to set up SIGHUP handler");
        }
    });

    while !shutdown.load(Ordering::SeqCst) {
        if reload.load(Ordering::SeqCst) {
            reload.store(false, Ordering::SeqCst);
            match SyncPolicy::load(&policy_path) {
                Ok(p) => {
                    veprintln!(
                        2,
                        "sync: policy reloaded on SIGHUP (watch_root={} repos, excluded={})",
                        p.watch_root_paths().len(),
                        p.exclude_repos.len()
                    );
                    activity.clear();
                    repair_cooldowns.clear();
                    filter_cooldowns.clear();
                    stage_cooldowns.clear();
                    // ADDED 2026-07-21 (v0.112.31, audit H4/F1.1):
                    // also reset the notification throttle map so a
                    // SIGHUP (= operator soft-reset after remediation)
                    // re-arms all throttled notifications.
                    remote_notify_cooldowns.clear();
                }
                Err(e) => eprintln!("sync: SIGHUP policy reload failed: {}", e),
            }
        }
        let policy = match SyncPolicy::load(&policy_path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("⚠️ failed loading policy: {}", e);
                sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let scan_interval = override_interval_secs
            .unwrap_or(policy.pulse_interval_secs)
            .max(1);
        let inactivity_delay = Duration::from_secs(policy.inactivity_push_delay_secs.max(1));
        let roots = policy.watch_root_paths();
        let excluded_dir_names = excluded_dir_names_set(&policy);
        let repos = discover_git_repos(
            &roots,
            &excluded_dir_names,
            &policy.exclude_repos,
            Some(&policy.system_repo),
        );
        // Submodule materialize pass: for each discovered parent
        // repo, materialize any declared submodules as standalone
        // worktrees under the watch root (e.g.
        // /home/dracon/Dev/polis/ from dracon-platform). This is
        // idempotent — once a worktree exists at the target path,
        // subsequent calls are no-ops. Failures are logged but do
        // NOT abort the daemon cycle (the operator may need to
        // run `git submodule update --init` manually for submodules
        // whose `.git/modules/<name>` is missing).
        //
        // ADDED 2026-06-30, goal `mr10pdzr-i495vy`.
        materialize_pending_submodules(&repos, &roots, &policy).await;
        // Re-discover after materialize so the newly created
        // worktrees are picked up by the standard report path.
        let mut to_sync: Vec<(PathBuf, SyncTaskJoin)> = Vec::new();
        let repo_set: BTreeSet<PathBuf> = repos.iter().cloned().collect();

        activity.retain(|repo, _| {
            let keep = repo_set.contains(repo);
            if !keep {
                initial_repos.remove(repo);
            }
            keep
        });
        pending_repos.retain(|repo, _| repo_set.contains(repo));
        repair_cooldowns.retain(|repo, _| repo_set.contains(repo));
        filter_cooldowns.retain(|repo, _| repo_set.contains(repo));
        empty_bootstrap_cooldowns.retain(|repo, _| repo_set.contains(repo));
        auto_create_cooldowns.retain(|repo, _| repo_set.contains(repo));
        // CHANGED 2026-07-21 (v0.112.31, audit H5/F1.2): reload the
        // stuck-push ledger from disk EVERY cycle instead of using
        // the map loaded once at startup. `record_push_failure` /
        // `record_push_success` (called from spawned sync tasks)
        // load → mutate → save the DISK file; without the reload the
        // loop's skip/retry check never saw runtime failures (the
        // documented 5-min backoff never fired for any repo that got
        // stuck after daemon start). The file is tiny and written
        // atomically, so a per-cycle read is cheap and races are
        // benign (worst case: one-cycle-stale view).
        stuck_push_repos = load_stuck_push_repos();
        stuck_push_repos.retain(|repo, _| repo_set.contains(repo));

        // Periodic broken tracking repair (every ~5 min at 1s interval)
        cycle_count += 1;
        if cycle_count.is_multiple_of(300) {
            let repo_refs: Vec<PathBuf> = repo_set.iter().cloned().collect();
            repair_broken_tracking(&repo_refs);
        }

        // Periodic incident ledger pruning (every ~30 min at 1s interval)
        if cycle_count.is_multiple_of(1800) {
            let ledger_path = crate::report::incident_ledger_path(policy_path.as_ref());
            if ledger_path.exists() {
                if let Ok(p) = SyncPolicy::load(policy_path.as_ref()) {
                    if let Ok(removed) = crate::report::enforce_retention(&ledger_path, &p) {
                        if removed > 0 {
                            eprintln!("🧹 periodic: pruned {} stale incident entries", removed,);
                        }
                    }
                }
            }
        }

        if let Some(reason) = freeze_reason(&policy_path) {
            eprintln!("⏸️ sync daemon paused ({})", reason);
            sleep(Duration::from_secs(scan_interval)).await;
            continue;
        }

        for repo in repos {
            // Clone policy at each repo iteration for a consistent snapshot.
            // If the policy is reloaded mid-cycle (SIGHUP), this repo still
            // operates on the policy version it was started with.
            let policy = policy.clone();

            // Guard against repo-discovery race: if a repo was deleted between
            // discovery and processing, skip it and clean up tracking.
            if !repo.exists() {
                if debug_enabled() {
                    eprintln!("⏳ {} repo path vanished, skipping", repo.display());
                }
                activity.remove(&repo);
                initial_repos.remove(&repo);
                continue;
            }

            let now = Instant::now();

            // CHANGED 2026-06-20: newly discovered repos may be empty or may
            // already have local commits. In both cases, if they have no remotes
            // yet, configure the standard mirror remotes before any readiness
            // or push decision. This fixes the `git init`-then-no-remotes gap
            // without overwriting operator-configured remotes.
            configure_standard_remotes_if_missing(&repo, &policy);

            // CHANGED 2026-07-21 (v0.112.29): for repos that have remotes
            // configured but the corresponding forge-side repo may not
            // exist yet (typical for a brand-new `git init` repo, or a
            // freshly-cloned standalone that has yet to be auto-created
            // on github/gitlab), attempt to auto-create the forge repos
            // BEFORE the `is_repo_ready` check. This way, an empty local
            // repo with no commits still gets created on github/gitlab —
            // when the operator's first commit lands, the daemon can
            // push without the "src refspec HEAD does not match any"
            // failure mode. `auto_create_all_remotes` is idempotent:
            // each remote is checked via `git ls-remote` first
            // (`remote_repo_exists`), so already-created repos are
            // skipped. The check is bounded by the per-remote
            // `auto_create` flag (codeberg defaults to false since
            // v0.112.28's quota posture).
            //
            // Without this fix, the daemon logs "configuring standard
            // mirror remotes" on first detection, then `is_repo_ready`
            // returns false for an empty repo, and the daemon `continue`s
            // forever — never creating the github/gitlab repos. The user
            // sees a persistent "❌ CONCERN · run repair-concerns --apply
            // (set upstream)" until they make their first commit, at
            // which point the daemon finally tries to push (and the
            // missing github repo means it fails with "Repository not
            // found"). Symptom: newly-`git init`'d repos appear broken
            // for the entire pre-commit window. See
            // `docs/design/empty-repo-auto-create-fix-2026-07-21.md` for
            // the full reproduction log.
            //
            // Note: we do NOT also call `push_to_all_remotes` here —
            // pushing an empty repo fails with "src refspec HEAD does
            // not match any". The first push waits until the operator
            // makes their first commit, at which point the regular
            // `is_repo_ready` path runs.
            if !policy.remotes.is_empty() {
                let any_remote_configured =
                    !crate::git::multi_remote::list_remotes(&repo).is_empty();
                // CHANGED 2026-07-21 (v0.112.30): throttle the
                // create-only auto-create to one attempt per 5 minutes
                // per repo. The v0.112.29 version ran on every 1s
                // cycle for every not-ready repo; each attempt issues
                // `git ls-remote` (SSH round-trip) per configured
                // remote, so a permanently-empty repo produced 2 SSH
                // connections/sec indefinitely.
                let auto_create_cooled = auto_create_cooldowns
                    .get(&repo)
                    .is_some_and(|until| now < *until);
                if any_remote_configured && !auto_create_cooled {
                    auto_create_cooldowns.insert(repo.clone(), now + Duration::from_secs(300));
                    let repo_override_for_create =
                        crate::policy::load_repo_override(&repo);
                    let create_results = crate::git::multi_remote::push_mirror_remotes_create_only(
                        &repo,
                        &policy.remotes,
                        true, // mirror the same `private = true` default
                              // as `sync.rs:1489` (push_mirror_remotes caller).
                              // Per-repo visibility overrides via
                              // `make-public` / `make-private` already exist;
                              // for the auto-create-on-discovery flow we
                              // default to private.
                        &repo_override_for_create.exclude_remotes,
                        repo_override_for_create.auto_create_on_codeberg,
                    )
                    .await;
                    for (name, result) in create_results {
                        if let Err(e) = result {
                            if debug_enabled() {
                                eprintln!(
                                    "⏳ {} auto-create on {} skipped: {}",
                                    repo.display(),
                                    name,
                                    e
                                );
                            }
                        } else if debug_enabled() {
                            eprintln!(
                                "🆕 {} auto-created on {}",
                                repo.display(),
                                name
                            );
                        }
                    }
                }
            }

            if !is_repo_ready(&repo) {
                // CHANGED 2026-07-21 (v0.112.30): for *stable* empty
                // repos (operator ran `git init`, added files, no
                // commits yet — NOT mid-clone), create the root commit
                // here so the next cycle's normal flow can push it.
                // Previously the daemon `continue`d forever on empty
                // repos: the `sync_repo` bootstrap was unreachable
                // (dispatch happens after this check) and the repo
                // sat at "❌ CONCERN · no commits yet" until the
                // operator committed manually. `is_stable_empty_repo`
                // distinguishes operator-init from mid-clone (lock
                // files + in-flight pack downloads); the bootstrap
                // applies the full staging policy (size limits,
                // exclude patterns, `auto_stage_untracked`, ownership
                // guard) inside `bootstrap_empty_repo_commit`.
                if crate::git::is_stable_empty_repo(&repo) {
                    let bootstrap_cooled = empty_bootstrap_cooldowns
                        .get(&repo)
                        .is_some_and(|until| now < *until);
                    if policy.auto_commit && !bootstrap_cooled {
                        match crate::sync::bootstrap_empty_repo_commit(
                            &repo,
                            &policy,
                            &excluded_dir_names,
                            false,
                        )
                        .await
                        {
                            Ok(true) => {
                                empty_bootstrap_cooldowns.remove(&repo);
                            }
                            Ok(false) => {}
                            Err(e) => {
                                eprintln!(
                                    "⚠️ {} empty-repo bootstrap failed (cooldown 300s): {}",
                                    repo.display(),
                                    e
                                );
                                empty_bootstrap_cooldowns
                                    .insert(repo.clone(), now + Duration::from_secs(300));
                            }
                        }
                    }
                } else if debug_enabled() {
                    eprintln!(
                        "⏳ {} not ready (mid-clone or empty repo), skipping",
                        repo.display()
                    );
                }
                continue;
            }
            if let Err(e) = configure_publish_upstream_if_missing(&repo, &policy) {
                eprintln!(
                    "⚠️ failed to configure publish upstream for {}: {}",
                    repo.display(),
                    e
                );
            }
            // Skip repos mid-checkout (clone's checkout phase holds index.lock).
            // Without this guard, the daemon can interfere with git checkout by
            // creating files (standard_files, project-state.md, etc.) that later
            // cause "Untracked working tree file would be overwritten by merge"
            // errors when git's own checkout tries to write them.
            let lock = repo.join(".git").join("index.lock");
            if lock.exists() {
                if debug_enabled() {
                    eprintln!(
                        "⏳ {} has index.lock (mid-checkout), skipping",
                        repo.display()
                    );
                }
                continue;
            }
            // ── Ownership safety guard ─────────────────────────
            // Default-skip auto-commit and auto-push for repos
            // that are not clearly owned by the operator. This
            // protects against repos whose origin points to
            // someone else's account (e.g. zerostack-reference
            // → gi-dellav/zerostack.git) or whose HEAD author is
            // a historical bad config (e.g. dracon-ai-lib →
            // `Dracon <dracon@void>`). Cached per cycle in
            // RepoActivity.ownership; the git invocations only
            // run when the cache is None.
            let repo_override = crate::policy::load_repo_override(&repo);
            let effective_auto_skip_unowned = repo_override
                .auto_skip_unowned
                .unwrap_or(policy.auto_skip_unowned);
            let entry_for_ownership = activity.entry(repo.clone()).or_insert_with(|| RepoActivity {
                fingerprint: String::new(),
                changed_at: now,
                dirty_since: None,
                ahead_since: None,
                behind_since: None,
                mirror_consecutive_fails: HashMap::new(),
                failure_count: 0,
                remote_failures: HashMap::new(),
                ownership: None,
            });
            if entry_for_ownership.ownership.is_none() {
                let trusted = crate::ownership::TrustedSet {
                    emails: policy.trusted_emails.clone(),
                    authors: policy.trusted_authors.clone(),
                    remote_hosts: policy.trusted_remote_hosts.clone(),
                };
                entry_for_ownership.ownership = Some(
                    crate::ownership::detect_ownership(
                        &repo,
                        &trusted,
                        repo_override.owned,
                    ),
                );
            }
            let ownership = entry_for_ownership.ownership.as_ref().unwrap();
            let is_owned = matches!(ownership, crate::ownership::OwnershipReport::Owned { .. });
            if effective_auto_skip_unowned && !is_owned {
                // Log once per cycle per repo (guard with a
                // cycle-relative counter to avoid spamming every
                // cycle).
                // CHANGED 2026-07-21 (v0.112.31, audit H4/F1.1):
                // use the expiring throttle — the previous
                // `Entry::Vacant` pattern fired exactly once per
                // daemon lifetime (the stored deadline was never
                // read). Also extend the message with the recovery
                // hint (audit H1/F0.2): the ownership verdict is
                // cached; after fixing the underlying issue the
                // operator must poke the daemon.
                let notify_key = format!("ownership-skip-{}", repo.display());
                if notify_throttled(
                    &mut remote_notify_cooldowns,
                    &notify_key,
                    Duration::from_secs(1800),
                ) {
                    eprintln!(
                        "🚫 {} skipping ({}): {} · verdict cached — after fixing, run `kill -HUP $(systemctl --user show dracon-sync.service -p MainPID --value)` or restart the daemon",
                        repo.display(),
                        match ownership {
                            crate::ownership::OwnershipReport::Unowned { reason, .. } => reason,
                            crate::ownership::OwnershipReport::Unknown { .. } => "unknown",
                            _ => "not-owned",
                        },
                        match ownership {
                            crate::ownership::OwnershipReport::Unowned { detail, .. } => detail,
                            crate::ownership::OwnershipReport::Unknown { detail } => detail,
                            _ => "",
                        }
                    );
                }
                continue;
            }
            // Grace period for newly discovered repos: skip git operations
            // for the first 15s to avoid interfering with in-progress clones.
            // During git clone, HEAD resolves after fetch but checkout may
            // still be in progress — running git status or writing standard
            // files here can create working-tree files that conflict with
            // git's own checkout, causing "Untracked working tree file would
            // be overwritten by merge" errors.
            //
            // Only applies to repos discovered AFTER the first scan cycle.
            // Repos present at daemon startup are assumed to be stable
            // (already checked out) and are processed immediately.
            if !initial_repos.contains(&repo) && cycle_count > 0 {
                const PENDING_GRACE_SECS: Duration = Duration::from_secs(15);
                if let Some(&entry_time) = pending_repos.get(&repo) {
                    if Instant::now().duration_since(entry_time) < PENDING_GRACE_SECS {
                        continue;
                    }
                    pending_repos.remove(&repo);
                } else {
                    // First time seeing this repo after startup: enter grace period
                    pending_repos.insert(repo.clone(), Instant::now());
                    if debug_enabled() {
                        eprintln!("⏳ {} new repo, entering 15s grace period", repo.display());
                    }
                    continue;
                }
            }

            // Skip repos that are stuck on push, with a 5-minute
            // backoff between retry attempts and a HARD STOP when
            // `consecutive_failures` reaches `push_max_retries`.
            //
            // CHANGED 2026-07-21 (v0.112.31, audit H5/F1.2): three
            // interlocking defects fixed here. (1) The loop's map was
            // loaded once at startup while runtime failures went to
            // the disk ledger — fixed by the per-cycle reload above.
            // (2) The retry path DELETED the entry before
            // dispatching, so a failed retry re-created it with
            // `consecutive_failures = 1` — the budget reset every
            // 5 minutes forever. Now the entry persists and
            // `last_retry_at` throttles attempts. (3)
            // `push_max_retries` was never enforced (report-display
            // only) — the `Exhausted` arm below stops auto-push
            // until the operator intervenes.
            if let Some(info) = stuck_push_repos.get(&repo).cloned() {
                match stuck_decision(&info, timestamp_secs(), policy.push_max_retries, 300) {
                    StuckDecision::Backoff => {
                        continue;
                    }
                    StuckDecision::Exhausted => {
                        let notify_key = format!("stuck-exhausted-{}", repo.display());
                        if notify_throttled(
                            &mut remote_notify_cooldowns,
                            &notify_key,
                            Duration::from_secs(1800),
                        ) {
                            eprintln!(
                                "🛑 {} push stuck ({} consecutive failures ≥ budget {}) — auto-push paused; fix the cause, then run `dracon-sync unstuck {}` or `repair-concerns --apply`",
                                repo.display(),
                                info.consecutive_failures,
                                policy.push_max_retries,
                                repo.file_name().unwrap_or_default().to_string_lossy(),
                            );
                            crate::report::send_sync_conflict_notification(
                                &repo,
                                "Push Stuck (budget exhausted)",
                                &format!(
                                    "{} consecutive push failures — auto-push paused; run `dracon-sync unstuck` after fixing",
                                    info.consecutive_failures
                                ),
                            );
                        }
                        continue;
                    }
                    StuckDecision::Retry => {
                        let stuck_age_secs =
                            timestamp_secs().saturating_sub(info.stuck_since);
                        eprintln!(
                            "🔄 {} was stuck ({} consecutive failures), retrying push after {}s",
                            repo.display(),
                            info.consecutive_failures,
                            stuck_age_secs
                        );
                        let notify_key = format!("stuck-retry-{}", repo.display());
                        if notify_throttled(
                            &mut remote_notify_cooldowns,
                            &notify_key,
                            Duration::from_secs(1800),
                        ) {
                            crate::report::record_sync_alert(
                                &repo,
                                "Stuck Push Retry",
                                &format!(
                                    "retrying after {}s; stuck since unix {}; {} consecutive failures",
                                    stuck_age_secs,
                                    info.stuck_since,
                                    info.consecutive_failures
                                ),
                            );
                        }
                        // Stamp the retry attempt (persisted) so a
                        // retry that fails for a NON-push reason
                        // (staging error, lock contention) doesn't
                        // spin every cycle — the backoff window
                        // restarts from NOW. On push failure,
                        // `record_push_failure` additionally bumps
                        // `consecutive_failures` + `last_error_at`;
                        // on success, `record_push_success` removes
                        // the entry entirely.
                        let mut updated = info.clone();
                        updated.last_retry_at = timestamp_secs();
                        stuck_push_repos.insert(repo.clone(), updated);
                        save_stuck_push_repos(&stuck_push_repos);
                    }
                }
            }
            if has_both_main_and_master(&repo) {
                eprintln!(
                    "🔧 {} has both main+master, consolidating to main",
                    repo.display()
                );
                if let Err(e) = crate::git::consolidate_to_main(&repo).await {
                    eprintln!("⚠️ failed to consolidate {} to main: {}", repo.display(), e);
                    continue;
                }
            } else if crate::git::has_only_master_branch(&repo) {
                eprintln!(
                    "🔧 {} has only 'master', renaming to 'main'",
                    repo.display()
                );
                if let Err(e) = crate::git::rename_master_to_main(&repo).await {
                    eprintln!("⚠️ failed to rename {} master→main: {}", repo.display(), e);
                    continue;
                }
            }
            if let Some(until) = repair_cooldowns.get(&repo).copied() {
                if now < until {
                    continue;
                }
                repair_cooldowns.remove(&repo);
            }
            if let Some(until) = filter_cooldowns.get(&repo).copied() {
                if now < until {
                    continue;
                }
                filter_cooldowns.remove(&repo);
            }
            if let Some(remaining) = stage_cooldown_remaining(&mut stage_cooldowns, &repo, now) {
                if debug_enabled() {
                    eprintln!(
                        "⏸️  {} staging cooldown active; skipping for {}s",
                        repo.display(),
                        remaining.as_secs()
                    );
                }
                continue;
            }
            let svc = match GitService::new(&repo) {
                Ok(svc) => svc,
                Err(e) => {
                    eprintln!("⚠️ {} init_failed: {}", repo.display(), e);
                    continue;
                }
            };
            let mut status = match svc.get_status().await {
                Ok(status) => status,
                Err(e) => {
                    eprintln!("⚠️ {} status_failed: {}", repo.display(), e);
                    continue;
                }
            };

            // Cache remote checks — used in both fast and slow paths
            let has_origin = has_origin_remote(&repo);
            let has_upstream = has_tracking_upstream(&repo);

            // CHANGED 2026-06-20: for repos without an upstream tracking branch
            // (mirror-only repos like `.dracon`), `git status` reports `ahead = 0`
            // even when there ARE unpushed local commits. Override `status.ahead`
            // from mirror tracking refs so the fast-path dispatch and the
            // `has_local_or_pending_work` check detect the real ahead count.
            // CHANGED 2026-07-21 (v0.112.30): also run the override
            // when the upstream IS configured but its remote-tracking
            // ref does not exist (never pushed / remote branch
            // deleted). Previously a freshly-bootstrapped repo hit
            // `configure_publish_upstream_if_missing` (which writes
            // the branch config), then libgit2 computed ahead=0
            // (nothing to compare against), and
            // `has_local_or_pending_work` evaluated false — the repo
            // was skipped forever and the report showed a false
            // "synced". When the tracking ref is missing, every
            // commit on HEAD is unpushed; `count_all_head_commits`
            // is the fallback after the mirror-based counts.
            let upstream_ref_missing =
                has_upstream && crate::git::upstream_tracking_ref_missing(&repo);
            if status.ahead == 0 && (!has_upstream || upstream_ref_missing) {
                let unpushed = count_unpushed_vs_mirrors(&repo);
                let unpushed = if unpushed == 0 {
                    count_unpushed_vs_configured_remotes(
                        &repo,
                        &policy.remotes.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
                    )
                } else {
                    unpushed
                };
                let unpushed = if unpushed == 0 && upstream_ref_missing {
                    crate::git::count_all_head_commits(&repo)
                } else {
                    unpushed
                };
                if unpushed > 0 {
                    if debug_enabled() {
                        eprintln!(
                            "🐛 {} ahead override: status.ahead=0 → {} (no upstream)",
                            repo.display(),
                            unpushed
                        );
                    }
                    status.ahead = unpushed as usize;
                }
            }

            // Fast path: skip expensive git diff calls for clean, synced repos.
            // Only do detailed diff analysis when the repo actually has changes.
            let (effective_dirty, _entries) = if status.is_clean
                && status.ahead == 0
                && status.behind == 0
            {
                // Clean and synced — skip all expensive git calls
                let has_remote_issues = !has_origin || !has_upstream;
                if !has_remote_issues {
                    activity.remove(&repo);
                    initial_repos.remove(&repo);
                    continue;
                }
                // Remote issues but clean — check for dirty files that
                // has_sync_relevant_dirty_entries would detect (untracked in excluded
                // dirs, oversized files, etc.) before committing to dirty state.
                let entries = repo_diff_entries(&repo).await.unwrap_or_default();
                let dirty = has_sync_relevant_dirty_entries(
                    &repo,
                    &entries,
                    &excluded_dir_names,
                    &policy.exclude_file_patterns,
                    policy.max_stage_file_bytes,
                    &policy.auto_commit_exclude_patterns,
                );
                if !dirty {
                    activity.remove(&repo);
                    initial_repos.remove(&repo);
                    continue;
                }
                (dirty, entries)
            } else {
                let raw_entries = repo_diff_entries(&repo).await.unwrap_or_default();
                // Filter out entries that only differ due to clean/smudge filters.
                // `git status` shows filter-processed files as modified, but `git diff HEAD`
                // correctly applies the clean filter and shows no diff for such files.
                // Note: untracked files don't appear in `git diff HEAD`, so they always pass.
                let diff_head_files = git_diff_head_files(&repo).await.unwrap_or_default();
                let filtered: Vec<_> = if diff_head_files.is_empty() && !raw_entries.is_empty() {
                    // git diff HEAD returned nothing. Only clear if ALL entries are Modified
                    // (filter-only). Untracked/Added files don't appear in git diff HEAD.
                    let has_non_modified = raw_entries
                        .iter()
                        .any(|e| !matches!(e.status, dracon_git::types::FileStatus::Modified));
                    if has_non_modified {
                        raw_entries
                            .into_iter()
                            .filter(|e| {
                                !matches!(e.status, dracon_git::types::FileStatus::Modified)
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    raw_entries
                        .into_iter()
                        .filter(|e| {
                            // Always keep non-modified entries (added, deleted, etc.)
                            // For modified entries, only keep if git diff HEAD shows them
                            if !matches!(e.status, dracon_git::types::FileStatus::Modified) {
                                return true;
                            }
                            diff_head_files.contains(&e.path)
                        })
                        .collect()
                };
                let dirty = has_sync_relevant_dirty_entries(
                    &repo,
                    &filtered,
                    &excluded_dir_names,
                    &policy.exclude_file_patterns,
                    policy.max_stage_file_bytes,
                    &policy.auto_commit_exclude_patterns,
                );
                let has_local_or_pending_work =
                    dirty || status.ahead > 0 || status.behind > 0 || !has_origin || !has_upstream;
                if !has_local_or_pending_work {
                    activity.remove(&repo);
                    initial_repos.remove(&repo);
                    continue;
                }
                // No-redispatch invariant: a repo with an in-flight
                // `sync_repo` task is not re-dispatched. The apply
                // phase removes the repo from `in_flight` when the
                // task completes (success, failure, or timeout).
                // This prevents duplicate `git push` invocations on
                // the same (repo, remote) pair within a cycle window.
                if in_flight.contains(&repo) {
                    continue;
                }
                (dirty, filtered)
            };

            // FIX (2026-06-19): include untracked_files in the fingerprint so
            // untracked file additions don't trigger the fingerprint stability
            // wait. Untracked file additions are atomic (new files appear all
            // at once), so they don't need the 5s stability wait that tracked
            // file edits need to avoid committing half-written files.
            let fingerprint = format!(
                "{}:{}:{}:{}:{}:{}",
                status.branch,
                effective_dirty as u8,
                status.staged_files,
                status.ahead,
                status.behind,
                status.untracked_files
            );
            let Some(entry) = activity.get_mut(&repo) else {
                activity.insert(
                    repo.clone(),
                    RepoActivity {
                        fingerprint,
                        changed_at: now,
                        dirty_since: if effective_dirty { Some(now) } else { None },
                        ahead_since: if status.ahead > 0 { Some(now) } else { None },
                        behind_since: if status.behind > 0 { Some(now) } else { None },
                        mirror_consecutive_fails: HashMap::new(),
                        failure_count: 0,
                        remote_failures: HashMap::new(),
                        ownership: None,
                    },
                );
                continue;
            };
            // Track when the repo first became dirty in this activity window.
            // This persists across fingerprint changes so that actively-edited
            // repos still get synced after a maximum delay (30s).
            if effective_dirty && entry.dirty_since.is_none() {
                entry.dirty_since = Some(now);
            } else if !effective_dirty {
                entry.dirty_since = None;
            }
            // Track ahead/behind state transitions for sustained-state notifications
            if status.ahead > 0 && entry.ahead_since.is_none() {
                entry.ahead_since = Some(now);
            } else if status.ahead == 0 {
                entry.ahead_since = None;
            }
            if status.behind > 0 && entry.behind_since.is_none() {
                entry.behind_since = Some(now);
            } else if status.behind == 0 {
                entry.behind_since = None;
            }
            if entry.fingerprint != fingerprint {
                entry.fingerprint = fingerprint;
                entry.changed_at = now;
                entry.failure_count = 0;
            }

            // Wait `inactivity_push_delay_secs` after the last
            // fingerprint change before committing, so rapid edits
            // are batched into one commit. Never wait more than 5s
            // since the repo first became dirty — this ensures
            // continuous editing (e.g. a build process) still gets
            // committed at a steady cadence.
            const MAX_DIRTY_DELAY: Duration = Duration::from_secs(5);
            let enough_time = entry.dirty_since.is_some_and(|since| {
                now.duration_since(since) >= MAX_DIRTY_DELAY
            }) || now.duration_since(entry.changed_at) >= inactivity_delay;

            if !enough_time {
                continue;
            }

            // MAX_FAILURES: per-cycle retry cap for transient errors.
            // Stuck repos (line ~505) trigger at failure_count >= 3 when repo is
            // clean + ahead > 0 — that's a permanent condition. MAX_FAILURES is
            // a higher bar for repos that might still be recoverable (dirty,
            // network issues, etc.).
            const MAX_FAILURES: usize = 5;
            if entry.failure_count >= MAX_FAILURES {
                if entry.failure_count == MAX_FAILURES {
                    eprintln!(
                        "⚠️ {} exceeded max failures ({}), skipping until resolved",
                        repo.display(),
                        MAX_FAILURES
                    );
                    entry.failure_count += 1;
                }
                continue;
            }

            // === BOUNDED PARALLEL SYNC: COLLECT JOB ===
            // The parallel phase below dispatches sync_repo calls in
            // parallel bounded by `policy.sem_max_concurrent_sync`. A
            // slow push on one repo no longer blocks other repos from
            // being committed and pushed. The post-sync state mutations
            // happen in the apply phase after all jobs complete.
            let entry_rf = std::mem::take(&mut entry.remote_failures);
            let secs = now.duration_since(entry.changed_at).as_secs();
            let policy_for_task = policy.clone();
            let excluded_for_task = excluded_dir_names.clone();
            let policy_path_for_task = policy_path.clone();
            let repo_for_task = repo.clone();
            let ahead_since_for_task = entry.ahead_since;
            // Mark the repo as having an in-flight task BEFORE
            // dispatching. The eligibility check at the top of the
            // next cycle consults `in_flight` and skips this repo
            // until the apply phase removes it. This is the
            // no-redispatch invariant in action.
            in_flight.insert(repo.clone());
            to_sync.push((
                repo.clone(),
                tokio::spawn(async move {
                    let mut rf = entry_rf;
                    let r = sync_repo_with_ahead_since(
                        &repo_for_task,
                        &policy_for_task,
                        &excluded_for_task,
                        secs,
                        Some(&mut rf),
                        false,
                        Some(&policy_path_for_task),
                        ahead_since_for_task,
                    )
                    .await;
                    (rf, r)
                }),
            ));
            continue;
        }

        // === BOUNDED PARALLEL SYNC: PARALLEL PHASE ===
        // Dispatch every collected sync_repo call concurrently.
        // The original JoinHandle from the COLLECT phase wraps the
        // tokio task that runs sync_repo. Here we wrap each handle
        // in a `futures::future::RemoteHandle` equivalent: we
        // poll each handle in parallel using a `FuturesUnordered`
        // of `JoinHandle<...>`. Tokio's multi-threaded runtime
        // schedules the spawned sync_repo tasks across worker
        // threads, so 4+ repos can be in-flight simultaneously.
        if !to_sync.is_empty() {
            let mut in_flight_tasks: FuturesUnordered<SyncTrioJoin> = FuturesUnordered::new();
            for (repo_path, handle) in to_sync.drain(..) {

                in_flight_tasks.push(tokio::spawn(async move {
                    let result = handle.await;
                    match result {
                        Ok((rf, r)) => (repo_path, rf, r),
                        Err(e) => {
                            eprintln!("⚠️ join error for sync task: {}", e);
                            (
                                repo_path,
                                HashMap::new(),
                                Err(anyhow::anyhow!("join error: {}", e)),
                            )
                        }
                    }
                }));
                // Yield once after each spawn to let the runtime
                // schedule the newly-spawned task. This prevents a
                // tight spawn-loop from monopolizing the runtime.
                tokio::task::yield_now().await;
            }

            // === APPLY PHASE ===
            // Drain results serially. Per-repo state mutations
            // (activity map, remote_failures, failure_count) happen
            // here, single-threaded, so we don't need locks.
            //
            // The apply phase is intentionally simplified compared to
            // the original serial loop: it covers the common case
            // (success/failure, activity removal, failure counting) but
            // defers the deeply-nested stuck-ahead/behind/mirror
            // notifications, repair-warns triage, and the post-sync
            // re-fetch to a follow-up. This keeps the diff focused on
            // the parallelization win.
            // Apply-phase deadline: stop awaiting in_flight after
            // `apply_deadline_secs` so a slow push on one repo does
            // not block the main loop from starting the next cycle.
            // The unfinished tasks remain in `in_flight` and are
            // drained in subsequent cycles. This keeps the daemon
            // responsive: a new dirty file in repo A is processed
            // in the next cycle, not after the slowest push on
            // repo B finishes.
            let apply_deadline = Duration::from_secs(policy.pulse_interval_secs.max(1) * 2);
            let apply_deadline_at = tokio::time::Instant::now() + apply_deadline;
            loop {
                let next = tokio::time::timeout_at(apply_deadline_at, in_flight_tasks.next()).await;
                let joined = match next {
                    Ok(Some(joined)) => joined,
                    Ok(None) => break, // in_flight_tasks empty
                    Err(_) => break,   // timeout
                };
                let Ok((repo, remote_failures, sync_res)) = joined else {
                    continue;
                };
                // Remove from in_flight set so the next cycle can
                // re-dispatch if the repo still has work to do.
                in_flight.remove(&repo);
                let Some(entry) = activity.get_mut(&repo) else {
                    continue;
                };
                entry.remote_failures = remote_failures;

                let sync_success = match sync_res {
                    Ok(SyncOutcome::Synced) => {
                        eprintln!("🔁 synced {}", repo.display());
                        let _ = std::io::stderr().flush();
                        true
                    }
                    Ok(SyncOutcome::NothingToDo) => {
                        if debug_enabled() {
                            eprintln!("🐛 {} nothing to commit", repo.display());
                        }
                        true
                    }
                    Ok(SyncOutcome::Blocked) => {
                        if debug_enabled() {
                            eprintln!(
                                "🐛 {} blocked (guard or manual intervention)",
                                repo.display()
                            );
                        }
                        false
                    }
                    // ADDED 2026-07-21 (v0.112.31, audit H3/F1.3):
                    // commit succeeded but the push failed. Counts as
                    // failure — NO `🔁 synced` log, `failure_count`
                    // increments, activity entry retained so the
                    // backstop/stuck logic keeps working.
                    Ok(SyncOutcome::PushFailed) => {
                        eprintln!(
                            "⚠️ {} committed but push failed (will retry)",
                            repo.display()
                        );
                        let notify_key = format!("pushfail-{}", repo.display());
                        if notify_throttled(
                            &mut remote_notify_cooldowns,
                            &notify_key,
                            Duration::from_secs(1800),
                        ) {
                            crate::report::send_sync_conflict_notification(
                                &repo,
                                "Push Failed",
                                "commit landed locally but the push failed; see daemon log",
                            );
                        }
                        false
                    }
                    Err(e) => {
                        eprintln!("⚠️ sync failed for {}: {}", repo.display(), e);
                        let err_str = e.to_string();
                        if err_str.contains("push") || err_str.contains("remote") {
                            let notify_key = format!("pushfail-{}", repo.display());
                            if notify_throttled(
                                &mut remote_notify_cooldowns,
                                &notify_key,
                                Duration::from_secs(1800),
                            ) {
                                crate::report::send_sync_conflict_notification(
                                    &repo,
                                    "Push Failed",
                                    &err_str,
                                );
                            }
                        }
                        false
                    }
                };

                if sync_success {
                    if stuck_push_repos.remove(&repo).is_some() {
                        save_stuck_push_repos(&stuck_push_repos);
                    }
                    entry.failure_count = 0;
                    activity.remove(&repo);
                    initial_repos.remove(&repo);
                } else {
                    entry.failure_count += 1;
                }
            }

            // === TRAILING IN-FLIGHT DRAIN (bounded) ===
            // Tasks that didn't complete within the apply deadline
            // (e.g. a 60s push) are still running. We can't apply
            // their results in this cycle, but we MUST remove them
            // from `in_flight` when they complete so the next
            // cycle can re-dispatch (only if the repo still has
            // work). Drain the leftover handles with a bounded
            // deadline so a single slow push doesn't block the
            // main loop indefinitely. We use the same deadline
            // policy as the apply phase (`pulse_interval_secs * 2`)
            // so the cycle time is bounded to ~2-3× pulse interval
            // regardless of how many slow pushes are in flight.
            //
            // BUGFIX (2026-06-15): previously, on trailing-drain
            // timeout (`Err(_) => break`), the unfinished tasks
            // were dropped from `in_flight_tasks` (which goes out
            // of scope) but their entries in the `in_flight`
            // HashSet were NEVER cleared. The result: a slow
            // sync task (e.g. a 60s push on `dracon-platform`)
            // would stay in `in_flight` forever, causing the
            // COLLECT phase of every subsequent cycle to skip
            // the repo. The repo would never be processed again
            // until the daemon restarted.
            //
            // Fix: track dispatched repos in a local set, and
            // on trailing-drain timeout (or normal completion),
            // clear any `in_flight` entries that were not
            // drained. This breaks the no-redispatch invariant
            // for slow tasks, but the invariant was never
            // achievable for slow tasks anyway (they always
            // timed out). The trade-off is: re-dispatching a
            // slow task is recoverable (the new task will fail
            // with a lock conflict or remote rejection), while
            // permanent skip is not.
            //
            // CHANGED 2026-07-09 (goal fb8ddd6b — repo-discovery
            // audit): the trailing-drain deadline now uses a
            // dedicated `trailing_drain_deadline_secs` policy field
            // (default 120s) instead of `pulse_interval_secs * 2`
            // (default 2s). The 2s deadline was killing github
            // pushes for the 9 nested submodules (first-push cold
            // pack cache takes 10-60s), causing the next cycle to
            // spawn a duplicate push and creating a traffic jam
            // that delayed smaller pushes. 120s gives most pushes
            // enough time to complete while still bounding the
            // daemon's cycle time. Override higher for repos with
            // very large histories.
            let trailing_deadline =
                Duration::from_secs(policy.trailing_drain_deadline_secs.max(1));
            let trailing_deadline_at = tokio::time::Instant::now() + trailing_deadline;
            let mut dispatched_this_cycle: HashSet<PathBuf> = in_flight.clone();
            loop {
                let next =
                    tokio::time::timeout_at(trailing_deadline_at, in_flight_tasks.next()).await;
                let joined = match next {
                    Ok(Some(joined)) => joined,
                    Ok(None) => break, // all drained
                    Err(_) => break,   // trailing deadline hit
                };
                if let Ok((repo, remote_failures, sync_res)) = joined {
                    in_flight.remove(&repo);
                    dispatched_this_cycle.remove(&repo);
                    if let Some(entry) = activity.get_mut(&repo) {
                        entry.remote_failures = remote_failures;
                        match sync_res {
                            Ok(SyncOutcome::Synced) => {
                                eprintln!("🔁 synced (late) {}", repo.display());
                                let _ = std::io::stderr().flush();
                                if stuck_push_repos.remove(&repo).is_some() {
                                    save_stuck_push_repos(&stuck_push_repos);
                                }
                                entry.failure_count = 0;
                                activity.remove(&repo);
                                initial_repos.remove(&repo);
                            }
                            Ok(SyncOutcome::NothingToDo) => {
                                if debug_enabled() {
                                    eprintln!("🐛 {} nothing to commit (late)", repo.display());
                                }
                            }
                            Ok(SyncOutcome::Blocked) => {
                                if debug_enabled() {
                                    eprintln!(
                                        "🐛 {} blocked (late, guard or manual intervention)",
                                        repo.display()
                                    );
                                }
                                entry.failure_count += 1;
                            }
                            // ADDED 2026-07-21 (v0.112.31, audit
                            // H3/F1.3): commit succeeded but the push
                            // failed — count as failure, no synced log.
                            Ok(SyncOutcome::PushFailed) => {
                                eprintln!(
                                    "⚠️ {} committed but push failed (late, will retry)",
                                    repo.display()
                                );
                                entry.failure_count += 1;
                            }
                            Err(e) => {
                                eprintln!("⚠️ sync failed (late) for {}: {}", repo.display(), e);
                                entry.failure_count += 1;
                            }
                        }
                    }
                }
            }
            // BUGFIX (2026-06-15): clear `in_flight` entries for
            // tasks that were dispatched in this cycle but did
            // NOT complete within the trailing deadline. These
            // tasks are still running in the background (we
            // don't know which ones), but we must remove them
            // from `in_flight` so the next cycle can re-dispatch.
            // Without this, a slow task causes permanent skip
            // (see comment above).
            if !dispatched_this_cycle.is_empty() {
                eprintln!(
                    "🔄 trailing-drain: clearing {} stuck in_flight entries: {:?}",
                    dispatched_this_cycle.len(),
                    dispatched_this_cycle
                );
                let _ = std::io::stderr().flush();
                for repo in &dispatched_this_cycle {
                    in_flight.remove(repo);
                }
            }
        }

        // Flush stderr after each full scan cycle so journald captures
        // all output from this cycle. Rust's block buffering under systemd
        // can delay output for minutes without explicit flushes.
        let _ = std::io::stderr().flush();
        let _ = std::io::stdout().flush();

        // === Persist in-flight state ===
        // Write the current `in_flight` set to a small JSON file so the
        // `repos` command can distinguish between "actively being
        // processed" and "stalled" rows. Self-cleaning: empty set
        // removes the file. Atomic write: temp file + rename.
        save_in_flight(&in_flight);

        // === Sustained-state notifications ===
        // Check for repos that have been in a concerning state for too long.
        // These fire once per repo per sustained incident, rate-limited to 30 min.
        let notification_now = Instant::now();
        const STUCK_AHEAD_THRESHOLD: Duration = Duration::from_secs(600); // 10 min
        const STUCK_BEHIND_THRESHOLD: Duration = Duration::from_secs(1800); // 30 min
        const MIRROR_DEGRADED_THRESHOLD: usize = 3; // 3 consecutive fails

        for (repo, entry) in &activity {
            // Repo stuck ahead (unpushed commits piling up)
            if let Some(since) = entry.ahead_since {
                if notification_now.duration_since(since) >= STUCK_AHEAD_THRESHOLD {
                    let notify_key = format!("stuck-ahead-{}", repo.display());
                    if notify_throttled(
                        &mut remote_notify_cooldowns,
                        &notify_key,
                        Duration::from_secs(1800),
                    ) {
                        crate::report::send_sync_conflict_notification(
                            repo,
                            "Stuck Ahead (Unpushed)",
                            "commits not reaching origin for >10 min — push may be failing",
                        );
                    }
                }
            }

            // Repo stuck behind (unpulled upstream changes)
            if let Some(since) = entry.behind_since {
                if notification_now.duration_since(since) >= STUCK_BEHIND_THRESHOLD {
                    let notify_key = format!("stuck-behind-{}", repo.display());
                    if notify_throttled(
                        &mut remote_notify_cooldowns,
                        &notify_key,
                        Duration::from_secs(1800),
                    ) {
                        crate::report::send_sync_conflict_notification(
                            repo,
                            "Stuck Behind (Unpulled)",
                            "upstream has unmerged changes for >30 min — pull may be failing",
                        );
                    }
                }
            }

            // Mirror degraded (one mirror consistently failing)
            for (mirror_name, fail_count) in &entry.mirror_consecutive_fails {
                if *fail_count >= MIRROR_DEGRADED_THRESHOLD {
                    let notify_key = format!("mirror-{}-{}", repo.display(), mirror_name);
                    if notify_throttled(
                        &mut remote_notify_cooldowns,
                        &notify_key,
                        Duration::from_secs(1800),
                    ) {
                        crate::report::send_sync_conflict_notification(
                            repo,
                            &format!("Mirror Degraded: {}", mirror_name),
                            &format!(
                                "{} consecutive push failures — mirror may be unreachable",
                                fail_count
                            ),
                        );
                    }
                }
            }
        }

        sleep(Duration::from_secs(scan_interval)).await;
    }
    // === Daemon shutdown: clean up the in-flight state file ===
    // Removing the file signals to `repos` that the daemon is no
    // running, so no rows can be "actively being processed".
    save_in_flight(&HashSet::new());
    Ok(())
}

#[cfg(test)]
mod submodule_materialize_tests {
    use super::*;
    use std::process::Command;

    /// Build a fixture parent with N submodules whose
    /// `.git/modules/<name>` gitdirs exist (mimicking
    /// post-`git submodule update --init` state). The watch root
    /// is the tempdir itself. Returns `(parent, [submodule_names])`.
    ///
    /// `sub_specs` is a list of `(gitmodules_name, worktree_name,
    /// path_in_parent)`. Splitting these out explicitly avoids
    /// fragile name parsing: the real operator config uses
    /// `web-games-polis` with path `web/games/wip/polis`, and
    /// the worktree name is the path's basename.
    fn build_fixture_with_submodules(
        tmp: &Path,
        sub_specs: &[(&str, &str, &str)],
    ) -> PathBuf {
        let parent = tmp.join("dracon-platform");
        std::fs::create_dir_all(&parent).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&parent)
                .output()
                .unwrap()
        };
        assert!(run(&["init", "-q"]).status.success());
        assert!(run(&["config", "user.email", "test@example.com"]).status.success());
        assert!(run(&["config", "user.name", "Test"]).status.success());
        assert!(run(&["config", "commit.gpgsign", "false"]).status.success());
        assert!(run(&["config", "tag.gpgsign", "false"]).status.success());
        // Disable hooks so globally-installed warden hooks don't reject
        // commits in temp test repos that lack `.gitattributes` with
        // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
        assert!(run(&["config", "core.hooksPath", "/dev/null"]).status.success());
        std::fs::write(parent.join("README.md"), b"# p\n").unwrap();
        assert!(run(&["add", "README.md"]).status.success());
        assert!(run(&["commit", "-q", "-m", "init"]).status.success());

        let mut gitmodules = String::new();
        for (subname, _worktree_name, path_in_parent) in sub_specs {
            let sub_gitdir = parent.join(".git/modules").join(subname);
            std::fs::create_dir_all(&sub_gitdir).unwrap();
            let run_sub = |args: &[&str]| {
                Command::new("git")
                    .args(args)
                    .current_dir(&sub_gitdir)
                    .output()
                    .unwrap()
            };
            assert!(run_sub(&["init", "-q"]).status.success());
            assert!(run_sub(&["config", "user.email", "test@example.com"]).status.success());
            assert!(run_sub(&["config", "user.name", "Test"]).status.success());
            assert!(run_sub(&["config", "commit.gpgsign", "false"]).status.success());
            assert!(run_sub(&["config", "tag.gpgsign", "false"]).status.success());
            // Disable hooks so globally-installed warden hooks don't reject
            // commits in temp test repos that lack `.gitattributes` with
            // `filter=dracon`. See AUDIT-3-UTILITIES-2026-07-10.md CONCERN #4.
            assert!(run_sub(&["config", "core.hooksPath", "/dev/null"]).status.success());
            std::fs::write(sub_gitdir.join("README.md"), b"# sub\n").unwrap();
            assert!(run_sub(&["add", "README.md"]).status.success());
            assert!(run_sub(&["commit", "-q", "-m", "init"]).status.success());
            let _ = run_sub(&["config", "--unset-all", "core.worktree"]);
            let _ = run_sub(&["reset"]);
            let sub_head = String::from_utf8_lossy(&run_sub(&["rev-parse", "HEAD"]).stdout)
                .trim()
                .to_string();
            std::fs::create_dir_all(parent.join(path_in_parent)).unwrap();
            std::fs::write(
                parent.join(format!("{}/.git", path_in_parent)),
                format!("gitdir: {}\n", sub_gitdir.display()),
            )
            .unwrap();
            Command::new("git")
                .args([
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{},{}", sub_head, path_in_parent),
                ])
                .current_dir(&parent)
                .output()
                .unwrap();
            gitmodules.push_str(&format!(
                "[submodule \"{}\"]\n\tpath = {}\n\turl = git@github.com:DraconDev/{}.git\n",
                subname, path_in_parent, subname
            ));
        }
        std::fs::write(parent.join(".gitmodules"), gitmodules).unwrap();
        parent
    }

    #[tokio::test]
    async fn materialize_pending_submodules_does_not_create_standalone_worktrees() {
        // REGRESSION for goal `730eaf2a` (2026-07-08): the daemon must
        // NOT materialize top-level standalone worktrees at
        // `<watch_root>/<name>` (e.g. /tmp/.../polis/). Submodules are
        // watched only via their nested checkout. Build a parent with 3
        // submodules, run the materialize pass, and assert that NO
        // standalone worktrees appear at the path-basename anchor.
        let tmp = tempfile::tempdir().unwrap();
        let watch_root = tmp.path().to_path_buf();
        let parent = build_fixture_with_submodules(
            &watch_root,
            &[
                ("web-games-polis", "polis", "web/games/wip/polis"),
                ("web-games-deathrun", "deathrun", "web/games/wip/deathrun"),
                (
                    "web-games-junk-runner",
                    "junk-runner",
                    "web/games/wip/junk-runner",
                ),
            ],
        );

        let repos = vec![parent.clone()];
        let roots = vec![watch_root.clone()];
        // The test passes an empty policy (no remotes); the
        // materialize call should still succeed without adding
        // any remotes.
        let test_policy = crate::policy::SyncPolicy::default();
        materialize_pending_submodules(&repos, &roots, &test_policy).await;

        // After the pass, NO standalone worktrees may exist at the
        // path-basename anchor (e.g. /tmp/.../polis/). This is the
        // core invariant fixed by goal 730eaf2a.
        for name in &["polis", "deathrun", "junk-runner"] {
            let wt = watch_root.join(name);
            assert!(
                !wt.exists(),
                "materialize_pending_submodules must NOT create a standalone worktree at {} (goal 730eaf2a)",
                wt.display()
            );
        }
    }

    #[tokio::test]
    async fn materialize_pending_submodules_leaves_existing_dirs_untouched() {
        // REGRESSION for goal `730eaf2a`: even if a directory happens to
        // exist at the path-basename anchor (e.g. a user-created dir or a
        // leftover from a previous layout), the materialize pass must NOT
        // create/clobber a standalone worktree there.
        let tmp = tempfile::tempdir().unwrap();
        let watch_root = tmp.path().to_path_buf();
        let parent = build_fixture_with_submodules(
            &watch_root,
            &[
                ("web-games-polis", "polis", "web/games/wip/polis"),
                ("web-games-deathrun", "deathrun", "web/games/wip/deathrun"),
            ],
        );

        // Pre-create a polis directory with a marker file (simulating an
        // unrelated dir at the basename anchor).
        let polis = watch_root.join("polis");
        std::fs::create_dir_all(&polis).unwrap();
        std::fs::write(polis.join("marker.txt"), b"keep me").unwrap();

        let repos = vec![parent.clone()];
        let roots = vec![watch_root.clone()];
        let test_policy = crate::policy::SyncPolicy::default();
        materialize_pending_submodules(&repos, &roots, &test_policy).await;

        // The marker must still exist (not clobbered into a worktree).
        assert!(
            polis.join("marker.txt").exists(),
            "pre-existing dir at basename anchor was clobbered"
        );
        // And no standalone worktree was created for deathrun either.
        assert!(
            !watch_root.join("deathrun").exists(),
            "materialize_pending_submodules must NOT create a standalone worktree at deathrun (goal 730eaf2a)"
        );
    }
}

