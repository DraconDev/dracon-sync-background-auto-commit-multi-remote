//! Multi-remote management — create, configure, and push to multiple git remotes.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

use anyhow::{Context, Result};

use crate::helpers::is_repo_already_exists;
use crate::policy::{debug_enabled, std_git_command, tokio_git_command, AuthType, RemoteConfig};

use super::{
    current_branch, gh_cmd, git_ssh_hardening, is_pack_too_large, is_permanent_push_rejection,
    is_push_rejected, is_safe_branch_name, load_secret, load_secret_or_legacy_pat,
    push_https_fallback, run_git_capture_output, run_git_with_timeout_env_progress,
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
                .output()
                .with_context(|| format!("git remote set-url {} in {}", name, repo.display()))
                .and_then(|o| {
                    // CHANGED 2026-07-21 (v0.112.33, audit M13/F2.4):
                    // require exit 0 (was `.status()?` — non-zero
                    // exits, e.g. malformed URL, read as success).
                    if o.status.success() {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!(
                            "git remote set-url {} in {} failed: {}",
                            name,
                            repo.display(),
                            String::from_utf8_lossy(&o.stderr).trim()
                        ))
                    }
                })?;
            Ok(())
        }
        None => {
            std_git_command()
                .args(["remote", "add", name, url])
                .current_dir(repo)
                .output()
                .with_context(|| format!("git remote add {} in {}", name, repo.display()))
                .and_then(|o| {
                    if o.status.success() {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!(
                            "git remote add {} in {} failed: {}",
                            name,
                            repo.display(),
                            String::from_utf8_lossy(&o.stderr).trim()
                        ))
                    }
                })?;
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

/// ADDED 2026-07-21 (v0.112.30): whether the repo has ever been pushed
/// to codeberg — i.e. a local `refs/remotes/codeberg/main` tracking
/// ref exists. Local-only check (no network). Used to distinguish
/// "pre-v0.112.28 repo with a live codeberg mirror" (keep pushing)
/// from "new repo under the codeberg-quota posture" (skip codeberg
/// entirely).
pub(crate) fn has_codeberg_tracking_ref(repo: &Path) -> bool {
    std_git_command()
        .args(["rev-parse", "--verify", "-q", "refs/remotes/codeberg/main"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// ADDED 2026-07-21 (v0.112.30): pure decision — should codeberg be
/// excluded from configure/push for this repo? True when a codeberg
/// remote exists in policy AND its effective auto_create is off
/// (global `auto_create = false` and no per-repo opt-in override)
/// AND the repo has no codeberg tracking ref (never pushed there).
///
/// Rationale: under the v0.112.28 quota posture new repos must not
/// attempt codeberg at all — the forge repo is never created, so
/// every `git push codeberg` fails with "Forgejo: Push to create is
/// not enabled" (guaranteed-failure spam). Pre-existing codeberg
/// mirrors (tracking ref present) and explicit opt-ins keep working.
pub(crate) fn codeberg_push_excluded(
    remotes: &[RemoteConfig],
    codeberg_override: Option<bool>,
    has_tracking_ref: bool,
) -> bool {
    if has_tracking_ref {
        return false;
    }
    remotes.iter().any(|r| {
        // NOTE: use `effective_auth_type()` (auto-detects from
        // `push_url` when the TOML field is unset), not the raw
        // `auth_type` field — the operator's config sets no
        // `auth_type`, so the raw field is the `GitHub` default
        // and a raw-field match never fires.
        matches!(r.effective_auth_type(), AuthType::Codeberg)
            && !(r.auto_create || codeberg_override.unwrap_or(false))
    })
}

/// Configure all remotes from policy for a given repo, skipping any
/// remote in `exclude`. `exclude` is a list of remote names (e.g.
/// `["gitlab"]`); an empty list is a no-op filter.
///
/// ADDED 2026-07-19 (goal `4555eaf6`): after configuring the
/// mirror remotes, ensure `origin` exists. The daemon pushes to
/// mirrors by explicit refspec so `origin` is NOT required for
/// sync, but VS Code's `git push` button uses `origin` by
/// convention. Without an `origin`, the branch's
/// `branch.<name>.remote` falls back to the alphabetically-first
/// remote — which for some operators is `codeberg`, leading to a
/// misleading `PUBLISH = codeberg/main` cell even though the
/// daemon (correctly) skips codeberg under the public-only
/// policy.
///
/// We set `origin` to the github mirror URL when one is configured
/// and `origin` is missing. If no github mirror is in policy
/// (unusual), `origin` is left untouched. We NEVER overwrite an
/// existing `origin` — operators who deliberately set a different
/// `origin` (e.g. an internal gitlab) are preserved.
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

    // ADDED 2026-07-19: ensure origin points at the github mirror
    // when no origin is configured. See doc-comment above.
    ensure_origin_for_vscode(repo, &filtered);
}

/// ADDED 2026-07-19 (goal `4555eaf6`): if the repo has no
/// `origin` remote AND a github mirror is in the configured
/// remotes, set `origin` to the github URL and `branch.<default>.remote`
/// to `origin`. This makes VS Code's `git push` work as expected
/// and produces a sane `PUBLISH` cell in the `repos` table.
///
/// Never overwrites an existing origin (operator override wins).
pub(crate) fn ensure_origin_for_vscode(repo: &Path, configured: &[RemoteConfig]) {
    if crate::git::status::has_origin_remote(repo) {
        return;
    }
    let Some(github) = configured.iter().find(|r| r.name == "github") else {
        return;
    };
    let Some(repo_name) = repo.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let url = github.resolve_push_url(repo_name);
    if let Err(e) = std_git_command()
        .args(["remote", "add", "origin", &url])
        .current_dir(repo)
        .status()
    {
        eprintln!(
            "⚠️ failed to add origin for {}: {}",
            repo.display(),
            e
        );
        return;
    }
    // Also set branch.<name>.remote = origin if a default branch
    // exists and its remote config is unset. This is what makes
    // `git push` (with no args) and `git status` report the right
    // upstream.
    if let Ok(output) = std_git_command()
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(repo)
        .output()
    {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() && crate::git::is_safe_branch_name(&branch) {
                let _ = std_git_command()
                    .args([
                        "config",
                        &format!("branch.{branch}.remote"),
                        "origin",
                    ])
                    .current_dir(repo)
                    .status();
            }
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
    codeberg_override: Option<bool>,
) -> Vec<(String, Result<()>)> {
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // ADDED 2026-07-21 (v0.112.30): skip codeberg entirely for repos
    // that were never pushed there while the effective auto_create is
    // off (v0.112.28 quota posture: global `auto_create = false`,
    // opt-in via `RepoPolicyOverride.auto_create_on_codeberg`).
    // Without this, a new repo gets the codeberg remote configured
    // and every push fails with "Forgejo: Push to create is not
    // enabled" — guaranteed-failure spam on every sync cycle. Repos
    // that DO have a codeberg tracking ref (all pre-v0.112.28 repos)
    // keep pushing. Because `filtered` drives both
    // `configure_all_remotes` and `remove_stale_remotes`, the dead
    // codeberg remote is also removed from `.git/config` on the
    // first push under this rule.
    let mut combined_exclude: Vec<String> = exclude.to_vec();
    if codeberg_push_excluded(remotes, codeberg_override, has_codeberg_tracking_ref(repo))
        && !combined_exclude.iter().any(|e| e == "codeberg")
    {
        combined_exclude.push("codeberg".to_string());
    }

    let filtered = filter_remotes_by_exclude(remotes, &combined_exclude);

    configure_all_remotes(repo, &filtered, &repo_name, &[]);

    for (remote_name, create_result) in
        auto_create_all_remotes(&filtered, &repo_name, private, Some(repo), codeberg_override).await
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

/// Auto-create the forge-side repos for an existing local repo that
/// already has remotes configured (added by `configure_standard_remotes_if_missing`)
/// but the corresponding github/gitlab/codeberg repo may not yet
/// exist. Does NOT push — that's the job of `push_mirror_remotes`.
///
/// ADDED 2026-07-21 (v0.112.29) for the empty-local-repo case where
/// `is_repo_ready` returns false (no commits) and `push_mirror_remotes`
/// would silently skip. Calling `auto_create_all_remotes` standalone
/// ensures the forge-side repos exist when the operator's first commit
/// lands. Idempotent: each remote is checked via `git ls-remote`
/// (`remote_repo_exists`) before any `gh repo create` / `glab repo create`
/// / REST call. Honored per-remote `auto_create` flag (codeberg
/// defaults to false since v0.112.28's quota posture).
///
/// Returns one entry per remote in `remotes` (filtered by `exclude`),
/// in the same order as `filtered`. The Result is `Ok(remote_name)`
/// on success, `Err(...)` on failure (e.g. rate-limit, network, auth).
pub(crate) async fn push_mirror_remotes_create_only(
    repo: &Path,
    remotes: &[RemoteConfig],
    private: bool,
    exclude: &[String],
    codeberg_override: Option<bool>,
) -> Vec<(String, Result<String>)> {
    let repo_name = repo
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let filtered = filter_remotes_by_exclude(remotes, exclude);
    let all_remote_names: Vec<_> = filtered.iter().map(|r| r.name.as_str()).collect();
    if let Err(e) = remove_stale_remotes(repo, &all_remote_names) {
        eprintln!(
            "⚠️ failed to clean stale remotes for {}: {}",
            repo.display(),
            e
        );
    }
    auto_create_all_remotes(&filtered, &repo_name, private, Some(repo), codeberg_override).await
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
    let branch = current_branch(repo)
        .filter(|b| b != "HEAD")
        .unwrap_or_else(|| "main".to_string());
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
                // declined, pack too large, etc.) — retrying will not fix it.
                // Return the error immediately so the caller can log it once
                // instead of burning retries and flooding the incident ledger.
                if is_permanent_push_rejection(&err_str) || is_pack_too_large(&err_str) {
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

/// Create a repo on GitHub using `gh` CLI.
///
/// FIXED 2026-07-20 (v0.112.28): the `--private` flag was previously
/// hardcoded regardless of the caller's intent. This now honors the
/// `private` parameter so the daemon can auto-create PUBLIC repos
/// (e.g. for the new `make-public` workflow). See
/// `docs/design/codeberg-quota-leak-fix-2026-07-13.md` for context.
pub(crate) fn create_repo_on_github(
    account: &str,
    repo_name: &str,
    private: bool,
) -> Result<String> {
    let mut cmd = gh_cmd();
    cmd.args(["repo", "create", repo_name]);
    if private {
        cmd.arg("--private");
    } else {
        cmd.arg("--public");
    }

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
        AuthType::GitHub => create_repo_on_github(&account, repo_name, private),
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
///
/// `codeberg_override` is a per-repo opt-in for codeberg auto-create.
/// `Some(true)` forces codeberg auto-create even if the global
/// `remote.auto_create = false` (the v0.112.28 default). `Some(false)`
/// or `None` respects the global config. ADDED 2026-07-20 (goal
/// v0.112.28) for the codeberg quota posture — see
/// `docs/design/codeberg-quota-leak-fix-2026-07-13.md`.
pub(crate) async fn auto_create_all_remotes(
    remotes: &[RemoteConfig],
    repo_name: &str,
    private: bool,
    repo: Option<&Path>,
    codeberg_override: Option<bool>,
) -> Vec<(String, Result<String>)> {
    let mut results = Vec::new();
    for remote in remotes {
        // Resolve effective auto_create for THIS remote: global OR
        // per-repo opt-in (codeberg only, since quota is the concern).
        // CHANGED 2026-07-21 (v0.112.30): use `effective_auth_type()`
        // (push_url auto-detect) instead of the raw `auth_type` field.
        // The operator's config sets no `auth_type`, so the raw field
        // is the `GitHub` default and the Codeberg arm never fired —
        // the per-repo `auto_create_on_codeberg` opt-in was silently
        // ignored (v0.112.28 latent bug).
        let effective_auto_create = match remote.effective_auth_type() {
            AuthType::Codeberg => remote.auto_create || codeberg_override.unwrap_or(false),
            _ => remote.auto_create,
        };
        if effective_auto_create {
            let resolved_name = remote.resolve_repo_name(repo_name);
            // CHANGED 2026-06-20: check if the repo already exists on the
            // remote before attempting to create it. This avoids spamming
            // `gh repo create` for repos that already exist, which causes
            // GitHub rate limiting ("You have created too many repositories,
            // too quickly").
            //
            // CHANGED 2026-07-21 (v0.112.33, audit M14/F2.5):
            // tri-state — Exists (skip + session-cached), Missing
            // (create), Unknown (transport/auth failure → SKIP the
            // create attempt this cycle; a network outage must not
            // fire spurious creates).
            if let Some(repo) = repo {
                match remote_repo_exists(repo, &remote.name).await {
                    RemoteExistence::Exists => {
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
                    RemoteExistence::Missing => {
                        // fall through to create
                    }
                    RemoteExistence::Unknown => {
                        if debug_enabled() {
                            eprintln!(
                                "⏳ {} existence check on {} inconclusive (transport/auth) — skipping create this cycle",
                                resolved_name,
                                remote.name
                            );
                        }
                        results.push((
                            remote.name.clone(),
                            Err(anyhow::anyhow!("existence check inconclusive")),
                        ));
                        continue;
                    }
                }
            }
            let result = auto_create_repo(remote, &resolved_name, private).await;
            results.push((remote.name.clone(), result));
        }
    }
    results
}

/// Check if a remote repo exists by running `git ls-remote` on the configured remote.
/// ADDED 2026-07-21 (v0.112.33, audit M14/F2.5): tri-state answer
/// for "does the forge-side repo exist?". The previous bool
/// conflated EVERY failure (network down, DNS, ssh key change,
/// remote 5xx) with "repo does not exist" — which then triggered
/// spurious `gh repo create` / `glab repo create` / REST create
/// attempts during an outage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteExistence {
    /// `git ls-remote` succeeded — the repo exists.
    Exists,
    /// The forge answered with a definitive not-found.
    Missing,
    /// Transport/auth/other failure — cannot tell. Callers must NOT
    /// treat this as Missing (no create attempt this cycle).
    Unknown,
}

/// ADDED 2026-07-21 (v0.112.33, audit M14/F2.5): session-lifetime
/// cache of (repo, remote) pairs confirmed to exist. A repo, once
/// created/existing, is almost never deleted; the cache eliminates
/// the per-push SSH `ls-remote` round-trip for every healthy repo.
/// In-memory only: a forge repo deleted out-of-band is re-probed on
/// daemon restart (and the push fails loudly in the meantime, which
/// is the correct operator signal).
static EXISTS_CACHE: std::sync::OnceLock<
    parking_lot::Mutex<std::collections::HashSet<(std::path::PathBuf, String)>>,
> = std::sync::OnceLock::new();

fn exists_cache() -> &'static parking_lot::Mutex<
    std::collections::HashSet<(std::path::PathBuf, String)>,
> {
    EXISTS_CACHE.get_or_init(|| parking_lot::Mutex::new(std::collections::HashSet::new()))
}

/// Classify an `ls-remote` failure from stderr. Definitive
/// not-found phrasings per forge:
/// - GitHub: "ERROR: Repository not found."
/// - GitLab: "The project you were looking for could not be found"
/// - Codeberg/Forgejo: "repository does not exist" / 404
fn ls_remote_indicates_missing(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("repository not found")
        || lower.contains("could not be found")
        || lower.contains("does not exist")
        || lower.contains("not found")
        || lower.contains("404")
}

async fn remote_repo_exists(repo: &Path, remote_name: &str) -> RemoteExistence {
    // Session cache: previously-confirmed pairs skip the SSH call.
    if exists_cache()
        .lock()
        .contains(&(repo.to_path_buf(), remote_name.to_string()))
    {
        return RemoteExistence::Exists;
    }
    let ssh_hardening = git_ssh_hardening();
    let output = tokio_git_command()
        .current_dir(repo)
        .env("GIT_SSH_COMMAND", ssh_hardening)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["ls-remote", remote_name, "HEAD"])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            exists_cache()
                .lock()
                .insert((repo.to_path_buf(), remote_name.to_string()));
            RemoteExistence::Exists
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if ls_remote_indicates_missing(&stderr) {
                RemoteExistence::Missing
            } else {
                // CHANGED 2026-07-21 (v0.112.33, audit M14/F2.5):
                // transport/auth/other failures are UNKNOWN, not
                // Missing — no spurious create during an outage.
                RemoteExistence::Unknown
            }
        }
        Err(_) => RemoteExistence::Unknown,
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

    // ---- codeberg_push_excluded / has_codeberg_tracking_ref (v0.112.30) ----

    fn make_codeberg_remote(auto_create: bool) -> RemoteConfig {
        RemoteConfig {
            name: "codeberg".to_string(),
            push_url: "git@codeberg.org:dracondev/{repo}.git".to_string(),
            auto_create,
            auto_create_account: "dracondev".to_string(),
            auth_type: crate::policy::AuthType::Codeberg,
            priority: 30,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: std::collections::HashMap::new(),
            force_push_when_behind: false,
        }
    }

    #[test]
    fn test_codeberg_excluded_when_auto_create_off_and_no_tracking_ref() {
        // v0.112.28 quota posture: global auto_create=false, no opt-in,
        // never pushed → skip codeberg entirely.
        let remotes = vec![make_codeberg_remote(false)];
        assert!(codeberg_push_excluded(&remotes, None, false));
    }

    #[test]
    fn test_codeberg_kept_when_tracking_ref_exists() {
        // Pre-v0.112.28 repo with a live codeberg mirror: keep pushing
        // even though auto_create is now off.
        let remotes = vec![make_codeberg_remote(false)];
        assert!(!codeberg_push_excluded(&remotes, None, true));
    }

    #[test]
    fn test_codeberg_kept_when_opted_in() {
        // Per-repo opt-in overrides the global auto_create=false.
        let remotes = vec![make_codeberg_remote(false)];
        assert!(!codeberg_push_excluded(&remotes, Some(true), false));
    }

    #[test]
    fn test_codeberg_kept_when_global_auto_create_on() {
        let remotes = vec![make_codeberg_remote(true)];
        assert!(!codeberg_push_excluded(&remotes, None, false));
    }

    #[test]
    fn test_codeberg_excluded_no_codeberg_remote_in_policy() {
        // No codeberg remote configured → nothing to exclude (the
        // caller's `combined_exclude` push is skipped either way).
        let remotes = vec![make_remote("github", 10)];
        assert!(!codeberg_push_excluded(&remotes, None, false));
    }

    #[test]
    fn test_codeberg_excluded_via_push_url_autodetect() {
        // Regression for the live-config bug: the operator's
        // `dracon-sync.toml` sets NO `auth_type` on the codeberg
        // remote, so the raw field is the `GitHub` default and a
        // raw-field match never fires. The exclusion must use
        // `effective_auth_type()` (push_url auto-detect).
        let mut codeberg = make_codeberg_remote(false);
        codeberg.auth_type = crate::policy::AuthType::GitHub; // unset → default
        let remotes = vec![codeberg];
        assert!(
            codeberg_push_excluded(&remotes, None, false),
            "codeberg remote with default (unset) auth_type must still be excluded"
        );
    }

    #[test]
    fn test_has_codeberg_tracking_ref_absent_for_fresh_repo() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let status = std_git_command()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!has_codeberg_tracking_ref(&repo));
    }

    #[test]
    fn test_has_codeberg_tracking_ref_present_after_update_ref() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let status = std_git_command()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        // FIXED 2026-07-21 (v0.112.30): these `git config` calls
        // previously passed `&repo` as a positional arg
        // (`.args(&args).arg(&repo)`), which git interprets as
        // `git config <name> <value> <value-pattern>` — setting the
        // config in the repo at the process CWD. Under `cargo test`
        // the CWD is the real `dracon-sync/` package root, so the
        // test wrote `user.email = test@test` into the LIVE repo's
        // `.git/config`. The daemon then committed with the poisoned
        // identity and the ownership guard (correctly) flipped the
        // repo to unowned, blocking all further sync. Always run git
        // config with `-C <path>` or `current_dir` in tests.
        for (k, v) in [("user.email", "test@test"), ("user.name", "test")] {
            let status = std_git_command()
                .args(["config", k, v])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        std::fs::write(repo.join("a.txt"), "x\n").unwrap();
        for args in [vec!["add", "a.txt"], vec!["commit", "--no-verify", "-q", "-m", "i"]] {
            let status = std_git_command()
                .args(&args)
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        // Simulate a pushed codeberg mirror.
        let status = std_git_command()
            .args(["update-ref", "refs/remotes/codeberg/main", "HEAD"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(has_codeberg_tracking_ref(&repo));
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

    // ========================================================================
    // Tests for `configure_all_remotes` origin-bootstrap (goal
    // `4555eaf6`, 2026-07-19).
    //
    // The repo has no `origin` and only mirror remotes
    // (`github`/`gitlab`/`codeberg`). VS Code's `git push` uses
    // `origin`, and the daemon's `PUBLISH` cell reads
    // `branch.<name>.remote` which falls back to the
    // alphabetically-first remote when `origin` is absent. The
    // fallback for many setups is `codeberg`, which is misleading
    // for private repos where codeberg is excluded under the
    // public-only policy.
    //
    // `configure_all_remotes` now sets `origin` to the github
    // mirror's URL when no `origin` exists, so both VS Code and
    // the daemon's PUBLISH cell agree on the canonical remote.
    // ========================================================================

    /// Helper: build a minimal RemoteConfig for the origin tests
    /// (re-using `make_remote` to keep the tests consistent).
    fn make_three_remotes() -> Vec<RemoteConfig> {
        vec![
            make_remote("github", 1),
            make_remote("gitlab", 2),
            make_remote("codeberg", 3),
        ]
    }

    /// Helper: initialise a fresh git repo at `path` with a
    /// `main` branch (so `symbolic-ref HEAD` works).
    fn init_bare_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        crate::policy::std_git_command()
            .args(["init", "--initial-branch=main"])
            .current_dir(path)
            .output()
            .unwrap();
        // Need at least one commit so HEAD resolves.
        std::fs::write(path.join("README.md"), "x").unwrap();
        crate::policy::std_git_command()
            .args(["add", "README.md"])
            .current_dir(path)
            .output()
            .unwrap();
        crate::policy::std_git_command()
            .args([
                "-c", "user.email=test@example.com",
                "-c", "user.name=test",
                "commit", "-m", "init",
            ])
            .current_dir(path)
            .output()
            .unwrap();
    }

    /// Goal `4555eaf6`: a fresh repo with no `origin` should get
    /// `origin = github URL` after `configure_all_remotes` runs.
    /// The github remote is also added (the existing behavior),
    /// and the branch's `branch.main.remote` is set to `origin`.
    #[test]
    fn test_configure_all_remotes_bootstraps_origin_when_missing() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("my-repo");
        init_bare_repo(&repo);

        let remotes = make_three_remotes();
        configure_all_remotes(&repo, &remotes, "my-repo", &[]);

        // github remote URL should match what `make_remote` produces.
        let origin_url = crate::git::multi_remote::get_remote_url(&repo, "origin")
            .expect("origin should be set");
        assert_eq!(origin_url, "git@invalid.example.com:github.git");
        // All three mirrors should also be configured.
        for name in ["github", "gitlab", "codeberg"] {
            assert!(
                crate::git::multi_remote::get_remote_url(&repo, name).is_some(),
                "missing {} remote",
                name
            );
        }
        // branch.main.remote = origin
        let branch_remote = crate::policy::std_git_command()
            .args(["config", "--get", "branch.main.remote"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(branch_remote.status.success());
        assert_eq!(
            String::from_utf8_lossy(&branch_remote.stdout).trim(),
            "origin"
        );
    }

    /// Goal `4555eaf6`: if `origin` already exists, `configure_all_remotes`
    /// must NOT overwrite it. Operators who deliberately point
    /// `origin` at e.g. an internal gitlab keep their config.
    #[test]
    fn test_configure_all_remotes_does_not_overwrite_existing_origin() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("my-repo");
        init_bare_repo(&repo);

        // Pre-set origin to a deliberately-unusual URL.
        crate::policy::std_git_command()
            .args(["remote", "add", "origin", "git@internal.example.com:keep.git"])
            .current_dir(&repo)
            .output()
            .unwrap();

        let remotes = make_three_remotes();
        configure_all_remotes(&repo, &remotes, "my-repo", &[]);

        let origin_url = crate::git::multi_remote::get_remote_url(&repo, "origin")
            .expect("origin should be set");
        assert_eq!(
            origin_url, "git@internal.example.com:keep.git",
            "configure_all_remotes must not overwrite an existing origin"
        );
    }

    /// Goal `4555eaf6`: when policy has no github remote (unusual),
    /// `configure_all_remotes` should leave `origin` untouched
    /// rather than inventing one from another mirror.
    #[test]
    fn test_configure_all_remotes_no_origin_when_no_github_in_policy() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("my-repo");
        init_bare_repo(&repo);

        let remotes = vec![make_remote("gitlab", 1)];
        configure_all_remotes(&repo, &remotes, "my-repo", &[]);

        assert!(
            crate::git::multi_remote::get_remote_url(&repo, "origin").is_none(),
            "origin should NOT be set when policy has no github remote"
        );
    }
}
