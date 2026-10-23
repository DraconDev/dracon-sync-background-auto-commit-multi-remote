//! Publication round planning shared by the legacy wrapper and the owned actor.
//!
//! Destination/pack decisions are taken by the local authority that owns the
//! checkout. The resulting plan is immutable for the whole round, so a slow
//! upload can never observe a different tip or silently skip a guard.
use super::*;

/// Select origin/mirror destinations and pack-limit exclusions under local
/// mutation ownership. Returns the pinned source plus the exact destination
/// set. This function performs no network push and no checkout mutation, but
/// mirror preparation (excluding configured remotes) belongs to the caller.
pub(crate) struct PublicationPlan {
    pub(crate) target: Option<crate::git::PublicationTarget>,
    pub(crate) branch: String,
    pub(crate) timeout_secs: u64,
    pub(crate) has_origin: bool,
    pub(crate) origin_is_github: bool,
    pub(crate) pack_too_large: bool,
    pub(crate) origin_url: Option<String>,
    pub(crate) github_candidate: bool,
    pub(crate) remotes: Vec<crate::policy::RemoteConfig>,
    pub(crate) repo_override_auto_create: Option<bool>,
    pub(crate) remote_failures: Option<HashMap<String, crate::daemon::RemoteFailInfo>>,
}

pub(super) async fn plan_publication(
    repo: &Path,
    policy: &SyncPolicy,
    has_origin: bool,
    mut remote_failures: Option<&mut HashMap<String, crate::daemon::RemoteFailInfo>>,
    ahead_count: u64,
) -> Result<PublicationPlan> {
    let target = pin_current_tip(repo).await?;
    let branch = target
        .as_ref()
        .map(|target| target.branch().to_owned())
        .unwrap_or_else(|| "main".to_owned());
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
    let (too_big_for_github, pushable_size) =
        crate::git::github_pack_too_large_async(repo, None).await;
    let github_already_flagged = remote_failures
        .as_ref()
        .map(|rf| rf.get("github").is_some_and(|f| f.consecutive > 0))
        .unwrap_or(false);
    let origin_url = if has_origin {
        crate::git::multi_remote::get_remote_url(repo, "origin")
    } else {
        None
    };
    let origin_is_github = origin_url
        .as_deref()
        .map(is_github_repository_url)
        .unwrap_or(false);
    let github_mirror_url = crate::git::multi_remote::get_remote_url(repo, "github");
    let github_mirror_is_origin =
        github_mirror_matches_origin(origin_url.as_deref(), github_mirror_url.as_deref());
    let repo_override = load_repo_override(repo);
    let mut combined_exclude: Vec<String> = repo_override.exclude_remotes.clone();
    let codeberg_public_only_effective = repo_override
        .codeberg_public_only
        .unwrap_or(policy.codeberg_public_only);
    if codeberg_public_only_effective {
        let cached_priv = cached_repo_visibility(repo, policy.sync_visibility_interval_hours);
        let skip_codeberg = match cached_priv {
            Some(true) => true,
            Some(false) => false,
            None => true,
        };
        if skip_codeberg && !combined_exclude.iter().any(|e| e == "codeberg") {
            combined_exclude.push("codeberg".to_string());
        }
    }
    if github_mirror_is_origin && !combined_exclude.iter().any(|e| e == "github") {
        combined_exclude.push("github".to_string());
    }
    if too_big_for_github && !combined_exclude.iter().any(|e| e == "github") {
        combined_exclude.push("github".to_string());
        if let Some(rf) = remote_failures.as_deref_mut() {
            let fail = rf.entry("github".to_string()).or_default();
            fail.consecutive += 1;
            if fail.last_error.is_empty() {
                fail.last_error =
                    "skipped: pushable branch exceeds github's 2 GiB pack limit".to_string();
            }
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
    let remotes =
        crate::git::multi_remote::filter_remotes_by_exclude(&policy.remotes, &combined_exclude);
    Ok(PublicationPlan {
        target,
        branch,
        timeout_secs: scaled_timeout,
        has_origin,
        origin_is_github,
        pack_too_large: too_big_for_github,
        origin_url,
        github_candidate: too_big_for_github && github_mirror_url.is_some(),
        remotes,
        repo_override_auto_create: repo_override.auto_create_on_codeberg,
        remote_failures: remote_failures.map(|rf| rf.clone()),
    })
}

/// Capture exactly the branch and object currently being published. `None` is
/// a real non-publication state (unreadable/non-checkout repo), never a reason
/// to fall back to moving HEAD in the upload phase.
async fn pin_current_tip(repo: &Path) -> Result<Option<crate::git::PublicationTarget>> {
    let branch = match crate::git::current_branch(repo) {
        Some(branch) => branch,
        None => "main".to_owned(),
    };
    let child = crate::git::spawn_git_command_cancellable(
        repo,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        "publication-pin",
    )?;
    let (status, stdout, stderr) = tokio::time::timeout(
        Duration::from_secs(20),
        crate::git::run_git_captured_output(child, repo, "publication-pin"),
    )
    .await
    .context("publication tip resolution deadline exceeded")??;
    if !status.success() {
        // An unborn HEAD has nothing to publish; every other failure must
        // surface rather than silently publishing a different source.
        let text = String::from_utf8_lossy(&stderr);
        if text.contains("Needed a single revision") || text.contains("unknown revision") {
            return Ok(None);
        }
        anyhow::bail!(
            "cannot pin publication tip in {}: {}",
            repo.display(),
            text.trim()
        );
    }
    let object_id = String::from_utf8_lossy(&stdout).trim().to_owned();
    Ok(Some(crate::git::PublicationTarget::new(
        &object_id, &branch,
    )?))
}
