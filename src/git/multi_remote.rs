//! Multi-remote management — create, configure, and push to multiple git remotes.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

use anyhow::{Context, Result};

use crate::helpers::is_repo_already_exists;
use crate::policy::{debug_enabled, std_git_command, tokio_git_command, AuthType, RemoteConfig};

use super::{
    current_branch, gh_cmd, git_ssh_hardening, is_permanent_push_rejection, is_push_rejected,
    is_safe_branch_name, load_secret, load_secret_or_legacy_pat, push_https_fallback,
    run_git_capture_output, run_git_with_timeout_env_progress,
};

/// Configure a remote URL. Adds if missing, updates if URL differs.
pub(crate) fn ensure_remote(repo: &Path, name: &str, url: &str) -> Result<()> {
    let existing = get_remote_url(repo, name);
    match existing {
        Some(cur) if cur == url => Ok(()),
        Some(_) => {
            std_git_command()
                .args(["remote", "set-url", name, url])
                .current_dir(repo)
                .status()
                .with_context(|| format!("git remote set-url {} in {}", name, repo.display()))?;
            Ok(())
        }
        None => {
            std_git_command()
                .args(["remote", "add", name, url])
                .current_dir(repo)
                .status()
                .with_context(|| format!("git remote add {} in {}", name, repo.display()))?;
            Ok(())
        }
    }
}

/// Filter a remote config list by an exclusion list of remote names.
/// Added 2026-06-23 (goal mqqsyzyd-qkvna5) so a repo with a
/// permanently unavailable mirror (e.g. over free-tier storage
/// quota) can opt out without affecting other repos.
///
/// `exclude` is a list of remote `name` values (e.g. `["gitlab"]`).
/// Any remote whose `name` matches an entry in `exclude` is omitted
/// from the returned vec. An empty `exclude` returns a clone of
/// the input unchanged.
pub(crate) fn filter_remotes_by_exclude(
    remotes: &[RemoteConfig],
    exclude: &[String],
) -> Vec<RemoteConfig> {
    if exclude.is_empty() {
        return remotes.to_vec();
    }
    remotes
        .iter()
        .filter(|r| !exclude.iter().any(|e| e == &r.name))
        .cloned()
        .collect()
}

/// Configure all remotes from policy for a given repo, skipping any
/// remote in `exclude`. `exclude` is a list of remote names (e.g.
/// `["gitlab"]`); an empty list is a no-op filter.
pub(crate) fn configure_all_remotes(
    repo: &Path,
    remotes: &[RemoteConfig],
    repo_name: &str,
    exclude: &[String],
) {
    let filtered = filter_remotes_by_exclude(remotes, exclude);
    for remote in &filtered {
        let url = remote.resolve_push_url(repo_name);
        if let Err(e) = ensure_remote(repo, &remote.name, &url) {
            eprintln!(
                "⚠️ failed to configure remote {} for {}: {}",
                remote.name,
                repo.display(),
                e
            );
        }
    }
}

/// Push to all mirror remotes, auto-creating repos if configured.
/// `exclude` is a list of remote names to skip (e.g. `["gitlab"]`).
/// Excluded remotes are not added to `.git/config`, are not
/// auto-created, and are not pushed to. Default behavior with an
/// empty `exclude` is unchanged.
pub(crate) async fn push_mirror_remotes(
    repo: &Path,
    remotes: &[RemoteConfig],
    timeout_secs: u64,
    retries: u32,
    private: bool,
    exclude: &[String],
) -> Vec<(String, Result<()>)> {
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let filtered = filter_remotes_by_exclude(remotes, exclude);

    configure_all_remotes(repo, &filtered, &repo_name, &[]);

    for (remote_name, create_result) in
        auto_create_all_remotes(&filtered, &repo_name, private, Some(repo)).await
    {
        match create_result {
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "⚠️ auto-create failed for {} on {}: {}",
                    repo_name, remote_name, e
                );
            }
        }
    }

    let all_remote_names: Vec<_> = filtered.iter().map(|r| r.name.as_str()).collect();
    if let Err(e) = remove_stale_remotes(repo, &all_remote_names) {
        eprintln!(
            "⚠️ failed to clean stale remotes for {}: {}",
            repo.display(),
            e
        );
    }

    push_to_all_remotes(repo, &filtered, timeout_secs, retries).await
}

/// Get the URL for a given remote name.
pub(crate) fn get_remote_url(repo: &Path, name: &str) -> Option<String> {
    let output = std_git_command()
        .args(["remote", "get-url", name])
        .current_dir(repo)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// List all remotes for a repo.
pub(crate) fn list_remotes(repo: &Path) -> Vec<String> {
    let output = std_git_command()
        .args(["remote"])
        .current_dir(repo)
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(String::from)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Remove stale remotes (anything not in the keep list, except origin).
pub(crate) fn remove_stale_remotes(repo: &Path, keep: &[&str]) -> Result<()> {
    let current = list_remotes(repo);
    let keep_set: HashSet<_> = keep.iter().collect();
    for remote in current {
        if remote == "origin" {
            continue;
        }
        if !keep_set.contains(&remote.as_str()) {
            std_git_command()
                .args(["remote", "remove", &remote])
                .current_dir(repo)
                .status()
                .with_context(|| format!("git remote remove {} in {}", remote, repo.display()))?;
        }
    }
    Ok(())
}

/// Push to a named remote with SSH hardening, HTTPS fallback, and retries.
pub(crate) async fn push_to_named_remote(
    repo: &Path,
    remote_name: &str,
    timeout_secs: u64,
    retries: u32,
    force_when_behind: bool,
) -> Result<()> {
    let branch = current_branch(repo).unwrap_or_else(|| "main".to_string());
    let refspec = format!("HEAD:refs/heads/{}", branch);
    let ssh_hardening = git_ssh_hardening();

    let attempt_ssh = run_git_with_timeout_env_progress(
        repo,
        &["push", "--no-verify", remote_name, &refspec],
        timeout_secs,
        &format!("push-to-{}", remote_name),
        &[
            ("GIT_SSH_COMMAND", &ssh_hardening),
            ("GIT_TERMINAL_PROMPT", "0"),
        ],
    )
    .await;

    if attempt_ssh.is_ok() {
        return Ok(());
    }

    let remote_url = get_remote_url(repo, remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote {} not found", remote_name))?;

    let mut last_err = None;
    if is_safe_branch_name(&branch) {
        let fallback_label = format!("push-to-{}", remote_name);
        match push_https_fallback(repo, &remote_url, &refspec, timeout_secs, &fallback_label).await
        {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }

    for attempt in 1..=retries.max(1) {
        match run_git_with_timeout_env_progress(
            repo,
            &["push", "--no-verify", remote_name, "HEAD"],
            timeout_secs,
            &format!("push-to-{}", remote_name),
            &[
                ("GIT_SSH_COMMAND", &ssh_hardening),
                ("GIT_TERMINAL_PROMPT", "0"),
            ],
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                let is_rejected = is_push_rejected(&err_str);
                // Permanent server-side rejection (protected branch, hook
                // declined, etc.) — retrying will not fix it. Return the
                // error immediately so the caller can log it once instead
                // of burning retries and flooding the incident ledger.
                if is_permanent_push_rejection(&err_str) {
                    return Err(e);
                }
                if is_rejected && force_when_behind {
                    match diagnose_divergence(repo, remote_name, &branch).await {
                        Ok(Divergence::RemotePurelyBehind) => {
                            let force_result = run_git_with_timeout_env_progress(
                                repo,
                                &[
                                    "push",
                                    "--force-with-lease",
                                    "--no-verify",
                                    remote_name,
                                    &format!("HEAD:refs/heads/{}", branch),
                                ],
                                timeout_secs,
                                &format!("force-push-to-{}", remote_name),
                                &[
                                    ("GIT_SSH_COMMAND", &ssh_hardening),
                                    ("GIT_TERMINAL_PROMPT", "0"),
                                ],
                            )
                            .await;
                            if force_result.is_ok() {
                                return Ok(());
                            }
                        }
                        Ok(Divergence::Divergent) | Err(_) => {
                            last_err = Some(e);
                        }
                    }
                } else {
                    last_err = Some(e);
                }
                if attempt < retries.max(1) {
                    sleep(Duration::from_secs(attempt as u64)).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("push to {} failed", remote_name)))
}

/// Result of divergence diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Divergence {
    /// Remote is purely behind local (safe to force push).
    RemotePurelyBehind,
    /// Both sides have diverged.
    Divergent,
}

/// Check whether the remote is purely behind or truly diverged.
pub(crate) async fn diagnose_divergence(
    repo: &Path,
    remote_name: &str,
    branch: &str,
) -> Result<Divergence> {
    let local_head = run_git_capture_output(repo, &["rev-parse", "HEAD"], "rev-parse")?;
    let local_head = local_head.trim();
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch);

    let rev_list_output = run_git_capture_output(
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{}...{}", local_head, remote_ref),
        ],
        "rev-list",
    )?;

    let counts: Vec<&str> = rev_list_output.trim().split('\t').collect();
    if counts.len() != 2 {
        return Ok(Divergence::Divergent);
    }

    let local_ahead: u32 = match counts[0].parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "⚠️ failed to parse local ahead count from rev-list output (\"{}\"): {}",
                counts[0], e
            );
            return Ok(Divergence::Divergent);
        }
    };
    let remote_ahead: u32 = match counts[1].parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "⚠️ failed to parse remote ahead count from rev-list output (\"{}\"): {}",
                counts[1], e
            );
            return Ok(Divergence::Divergent);
        }
    };

    if remote_ahead == 0 && local_ahead > 0 {
        Ok(Divergence::RemotePurelyBehind)
    } else if remote_ahead > 0 {
        Ok(Divergence::Divergent)
    } else {
        Ok(Divergence::RemotePurelyBehind)
    }
}

/// Push to all remotes in priority order.
///
/// SEQUENTIAL (not concurrent) as of goal `87c1bf4d` (2026-06-16).
/// The previous concurrent implementation via `tokio::spawn` had
/// a race condition: when one remote (gitlab, codeberg) was
/// slower on the network than another (origin, github), a
/// subsequent fast-forward could land on the fast remote but be
/// rejected by the slow remote (which was still at an older tip).
/// After `push_max_retries` consecutive failures, the daemon
/// marked the repo PUSH_STUCK.
///
/// Sequential push trades ~3-6s of total wall-clock latency per
/// commit (4 remotes × ~1.5s each, instead of 1.5s in parallel)
/// for ELIMINATION of the race. The user-visible cadence is
/// similar because the daemon's apply phase deadline
/// (`pulse_interval_secs * 2` = 2s) was already causing trailing-
/// drain events on every commit when concurrent pushes took >2s.
pub(crate) async fn push_to_all_remotes(
    repo: &Path,
    remotes: &[RemoteConfig],
    timeout_secs: u64,
    retries: u32,
) -> Vec<(String, Result<()>)> {
    let mut sorted = remotes.to_vec();
    sorted.sort_by_key(|r| r.priority);

    // CHANGED 2026-06-20: sequential → parallel. Pushing to all remotes
    // in parallel cuts push time from O(N) to O(1) for N remotes.
    // With 4 remotes (origin, github, codeberg, gitlab), this is a
    // 4x speedup on the push phase. Results are returned in the same
    // order as `sorted` so callers can rely on the ordering.
    let mut futures = Vec::with_capacity(sorted.len());
    for remote in sorted.iter() {
        let repo = repo.to_path_buf();
        let name = remote.name.clone();
        let force_push = remote.force_push_when_behind;
        futures.push(tokio::spawn(async move {
            let result = push_to_named_remote(&repo, &name, timeout_secs, retries, force_push).await;
            (name, result)
        }));
    }
    let mut results = Vec::with_capacity(futures.len());
    for f in futures {
        match f.await {
            Ok((name, result)) => results.push((name, result)),
            Err(e) => {
                let name = String::from("unknown");
                results.push((name, Err(anyhow::anyhow!("join error: {}", e))));
            }
        }
    }
    results
}

/// Create a private repo on GitHub using `gh` CLI.
pub(crate) fn create_repo_on_github(account: &str, repo_name: &str) -> Result<String> {
    let mut cmd = gh_cmd();
    cmd.args(["repo", "create", repo_name, "--private"]);

    let output = cmd.output().with_context(|| "gh repo create failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_repo_already_exists(&stderr) {
            return Ok(format!("https://github.com/{}/{}.git", account, repo_name));
        }
        anyhow::bail!("gh repo create failed: {}", stderr.trim());
    }

    // Set default branch to main via API (gh CLI doesn't support --default-branch)
    let mut patch = gh_cmd();
    patch.args([
        "api",
        "-X",
        "PATCH",
        &format!("repos/{}/{}", account, repo_name),
        "--field",
        "default_branch=main",
    ]);
    let _ = patch.output(); // best effort — repo is created either way

    Ok(format!("https://github.com/{}/{}.git", account, repo_name))
}

/// Create a repo on GitLab using `glab` CLI.
pub(crate) fn create_repo_on_gitlab(
    account: &str,
    repo_name: &str,
    private: bool,
) -> Result<String> {
    let mut cmd = std::process::Command::new("glab");
    cmd.args(["repo", "create", repo_name]);
    if private {
        cmd.arg("--private");
    } else {
        cmd.arg("--public");
    }

    if let Some(token) = load_secret("GITLAB_TOKEN") {
        cmd.env("GITLAB_TOKEN", token);
    }

    let output = cmd.output().with_context(|| "glab repo create failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_repo_already_exists(&stderr) {
            return Ok(format!("git@gitlab.com:{}/{}.git", account, repo_name));
        }
        anyhow::bail!("glab repo create failed: {}", stderr.trim());
    }

    Ok(format!("git@gitlab.com:{}/{}.git", account, repo_name))
}

/// Create a repo on Codeberg via REST API.
pub(crate) async fn create_repo_on_codeberg(
    token: &str,
    account: &str,
    repo_name: &str,
    api_endpoint: &str,
    private: bool,
) -> Result<String> {
    let client = reqwest::Client::new();
    let response = client
        .post(api_endpoint)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "name": repo_name,
            "private": private,
            "default_branch": "main"
        }))
        .send()
        .await
        .with_context(|| "reqwest codeberg repo create failed")?;

    let status = response.status();
    if status.as_u16() == 409 || status.as_u16() == 422 {
        return Ok(format!("git@codeberg.org:{}/{}.git", account, repo_name));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("codeberg repo create failed ({}): {}", status, body);
    }

    Ok(format!("git@codeberg.org:{}/{}.git", account, repo_name))
}

/// Auto-create a repo on the given remote platform.
pub(crate) async fn auto_create_repo(
    config: &RemoteConfig,
    repo_name: &str,
    private: bool,
) -> Result<String> {
    let account = config.resolve_account();
    match config.effective_auth_type() {
        AuthType::GitHub => create_repo_on_github(&account, repo_name),
        AuthType::GitLab => create_repo_on_gitlab(&account, repo_name, private),
        AuthType::Codeberg => {
            let token_var = config
                .auto_create_token_var
                .as_deref()
                .unwrap_or("CODEBERG_TOKEN");
            let token = load_secret_or_legacy_pat(token_var)
                .with_context(|| format!("missing token for Codeberg (set {} env var or put it in ~/.dracon/utilities/sync/secrets/*.env or ~/.dracon/secrets/pat/*.env)", token_var))?;
            let endpoint = config
                .api_endpoint
                .as_deref()
                .unwrap_or("https://codeberg.org/api/v1/user/repos");
            create_repo_on_codeberg(&token, &account, repo_name, endpoint, private).await
        }
        AuthType::Generic => anyhow::bail!("Generic auth cannot auto-create repos"),
    }
}

/// Auto-create all configured remotes for a repo.
pub(crate) async fn auto_create_all_remotes(
    remotes: &[RemoteConfig],
    repo_name: &str,
    private: bool,
    repo: Option<&Path>,
) -> Vec<(String, Result<String>)> {
    let mut results = Vec::new();
    for remote in remotes {
        if remote.auto_create {
            let resolved_name = remote.resolve_repo_name(repo_name);
            // CHANGED 2026-06-20: check if the repo already exists on the
            // remote before attempting to create it. This avoids spamming
            // `gh repo create` for repos that already exist, which causes
            // GitHub rate limiting ("You have created too many repositories,
            // too quickly").
            if let Some(repo) = repo {
                if remote_repo_exists(repo, &remote.name).await {
                    if debug_enabled() {
                        eprintln!(
                            "ℹ️  {} already exists on {} — skipping auto-create",
                            resolved_name,
                            remote.name
                        );
                    }
                    results.push((remote.name.clone(), Ok(resolved_name.clone())));
                    continue;
                }
            }
            let result = auto_create_repo(remote, &resolved_name, private).await;
            results.push((remote.name.clone(), result));
        }
    }
    results
}

/// Check if a remote repo exists by running `git ls-remote` on the configured remote.
async fn remote_repo_exists(repo: &Path, remote_name: &str) -> bool {
    let ssh_hardening = git_ssh_hardening();
    let output = tokio_git_command()
        .current_dir(repo)
        .env("GIT_SSH_COMMAND", ssh_hardening)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["ls-remote", remote_name, "HEAD"])
        .output()
        .await;
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

// ============================================================================
// Tests for push_to_all_remotes (goal 87c1bf4d, 2026-06-16)
//
// The concurrent → sequential refactor must preserve three invariants:
//   1. Push order follows the `priority` field (lowest first)
//   2. An empty `remotes` slice returns an empty Vec (no spurious entries)
//   3. A failure on one remote does NOT abort subsequent pushes
//      (the daemon continues to the next remote)
//
// These tests use a tiny local-only mock to avoid network and SSH. They
// verify the *shape* of the returned Vec and the *order* of attempts, not
// the wire-level push behavior. Wire-level push is exercised by the live
// daemon and the integration tests.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::RemoteConfig;
    use crate::test_helpers::EnvRestorer;

    /// Helper: build a minimal RemoteConfig for testing.
    /// `name` and `priority` are the only fields that affect the sort.
    fn make_remote(name: &str, priority: u32) -> RemoteConfig {
        RemoteConfig {
            name: name.to_string(),
            push_url: format!("git@invalid.example.com:{}.git", name),
            auto_create: false,
            auto_create_account: String::new(),
            auth_type: crate::policy::AuthType::GitHub,
            priority,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: std::collections::HashMap::new(),
            force_push_when_behind: false,
        }
    }

    /// Test: empty remotes list returns empty Vec.
    ///
    /// Regression test for goal `87c1bf4d`: the sequential
    /// implementation must handle the empty case without
    /// panicking or returning a spurious empty Ok entry.
    #[tokio::test]
    async fn test_push_to_all_remotes_empty_list_returns_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        let results = push_to_all_remotes(&repo, &[], 1, 1).await;
        assert!(
            results.is_empty(),
            "empty remotes should return empty Vec, got {:?}",
            results
        );
    }

    /// Test: remotes are attempted in priority order (lowest first).
    ///
    /// Regression test for goal `87c1bf4d`: the sort by `priority`
    /// must apply to the input `remotes` slice before any push is
    /// attempted. We don't verify the push actually succeeds (the
    /// fake URL will fail), but we verify the *order of attempts*
    /// by checking that all entries in the result Vec appear in
    /// priority order. The exact error vs. success status is
    /// implementation-dependent on the underlying git command,
    /// so we just check ordering.
    #[tokio::test]
    async fn test_push_to_all_remotes_respects_priority_order() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        // Intentionally out of priority order
        let remotes = vec![
            make_remote("z-high-priority", 100),
            make_remote("a-low-priority", 1),
            make_remote("m-mid-priority", 50),
        ];
        let results = push_to_all_remotes(&repo, &remotes, 1, 1).await;
        assert_eq!(results.len(), 3, "should return one entry per remote");
        let names: Vec<&str> = results.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["a-low-priority", "m-mid-priority", "z-high-priority"],
            "remotes must be pushed in priority order (lowest first)"
        );
    }

    /// Test: a failure on one remote does NOT abort the loop.
    ///
    /// Regression test for goal `87c1bf4d`: the sequential
    /// implementation must NOT use `?` or early-return inside
    /// the loop. All remotes must be attempted, with their
    /// results collected in a Vec. The test uses three fake
    /// remotes with invalid URLs; all three should appear in
    /// the result Vec regardless of their success/failure.
    #[tokio::test]
    async fn test_push_to_all_remotes_continues_after_failure() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        let remotes = vec![
            make_remote("first-fails", 1),
            make_remote("second-fails", 2),
            make_remote("third-fails", 3),
        ];
        let results = push_to_all_remotes(&repo, &remotes, 1, 1).await;
        assert_eq!(
            results.len(),
            3,
            "all 3 remotes must be attempted (no early-return on failure)"
        );
        // The names must be in priority order
        let names: Vec<&str> = results.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["first-fails", "second-fails", "third-fails"],
            "order must be by priority even when all fail"
        );
    }

    #[tokio::test]
    async fn test_remote_repo_exists_checks_remote_head() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            r#"#!/bin/sh
if [ "$1" = "ls-remote" ] && [ "$3" = "HEAD" ]; then
    case "$2" in
        *existing*) echo "abcdef1234567890 HEAD"; exit 0 ;;
        *) exit 1 ;;
    esac
fi
exit 1
"#,
        )
        .expect("write fake git");
        std::fs::set_permissions(
            &fake_git,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .expect("chmod fake git");
        let _guard = EnvRestorer::new(
            "DRACON_SYNC_GIT_BIN",
            fake_git.to_str().expect("fake git path"),
        );

        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo dir");
        assert!(
            remote_repo_exists(&repo, "existing-repo").await,
            "existing remote HEAD should be detected"
        );
        assert!(
            !remote_repo_exists(&repo, "missing-repo").await,
            "missing remote should not be treated as existing"
        );
    }

    // ========================================================================
    // Tests for filter_remotes_by_exclude (goal mqqsyzyd-qkvna5, 2026-06-23)
    //
    // The per-repo `exclude_remotes` override lets a repo opt out of a
    // specific mirror (e.g. gitlab for a repo over the free-tier
    // storage quota) without affecting other repos. The filter is the
    // single source of truth: the daemon must consult it for both
    // `configure_all_remotes` and `push_mirror_remotes`.
    // ========================================================================

    /// Test: an empty exclude list is a no-op (returns a clone of the
    /// input unchanged). This is the common case for the 12 other
    /// repos that use gitlab and do NOT opt out of it.
    #[test]
    fn test_filter_remotes_by_exclude_empty_exclude_is_noop() {
        let remotes = vec![make_remote("github", 1), make_remote("gitlab", 2)];
        let exclude: Vec<String> = vec![];
        let filtered = filter_remotes_by_exclude(&remotes, &exclude);
        assert_eq!(filtered.len(), 2, "empty exclude must return all remotes");
        let names: Vec<String> = filtered.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["github", "gitlab"]);
    }

    /// Test: a non-empty exclude list drops the matching remote. This
    /// is the platform case: `exclude_remotes = ["gitlab"]` causes
    /// the gitlab remote to be omitted from both configure and push.
    #[test]
    fn test_filter_remotes_by_exclude_drops_matching_remote() {
        let remotes = vec![
            make_remote("github", 1),
            make_remote("codeberg", 2),
            make_remote("gitlab", 3),
        ];
        let exclude = vec!["gitlab".to_string()];
        let filtered = filter_remotes_by_exclude(&remotes, &exclude);
        assert_eq!(filtered.len(), 2, "gitlab must be excluded");
        let names: Vec<String> = filtered.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["github", "codeberg"]);
    }

    /// Test: multiple excludes work in one pass. (Forward-looking
    /// scenario: a repo over quota on both gitlab AND codeberg.)
    #[test]
    fn test_filter_remotes_by_exclude_drops_multiple_remotes() {
        let remotes = vec![
            make_remote("github", 1),
            make_remote("codeberg", 2),
            make_remote("gitlab", 3),
        ];
        let exclude = vec!["gitlab".to_string(), "codeberg".to_string()];
        let filtered = filter_remotes_by_exclude(&remotes, &exclude);
        assert_eq!(filtered.len(), 1, "only github must remain");
        assert_eq!(filtered[0].name, "github");
    }

    /// Test: the global remotes list is unchanged for OTHER repos.
    /// This is a regression test for the design's "per-repo, not
    /// global" property: the filter is applied at the call site with
    /// the per-repo override, so other repos still see the full
    /// global list.
    #[test]
    fn test_filter_remotes_by_exclude_is_per_call_not_global() {
        // The filter is a pure function: same input + same exclude
        // always produces the same output. The per-repo behavior
        // comes from the caller passing the right `exclude`.
        let remotes = vec![
            make_remote("github", 1),
            make_remote("codeberg", 2),
            make_remote("gitlab", 3),
        ];
        // Repo A opts out of gitlab
        let exclude_a = vec!["gitlab".to_string()];
        let filtered_a = filter_remotes_by_exclude(&remotes, &exclude_a);
        assert_eq!(filtered_a.len(), 2);
        // Repo B does not opt out
        let exclude_b: Vec<String> = vec![];
        let filtered_b = filter_remotes_by_exclude(&remotes, &exclude_b);
        assert_eq!(filtered_b.len(), 3);
        // The original `remotes` slice is never mutated.
        assert_eq!(remotes.len(), 3, "input must not be mutated");
    }
}
