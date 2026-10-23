//! Publication round planning shared by the daemon workers and the legacy
//! wrapper. All decisions that read the checkout or mutate Git configuration
//! happen here, under local mutation ownership; `upload_publication` then sees
//! an immutable plan and cannot observe a different tip mid-flight.
use super::*;

/// Immutable description of one publication round.
pub(crate) struct PublicationPlan {
    /// Exact source object + destination branch. `None` means there is no
    /// publishable tip (unborn HEAD); it must never silently degrade to
    /// "publish whatever HEAD is when the upload finally starts".
    pub(crate) target: Option<crate::git::PublicationTarget>,
    pub(crate) timeout_secs: u64,
    pub(crate) has_origin: bool,
    pub(crate) origin_url: Option<String>,
    pub(crate) remotes: Vec<crate::policy::RemoteConfig>,
    /// Per-remote failure/pause memory as observed when the round was planned.
    /// The owner replaces its map with this after the round, so counters are
    /// only ever written by the publication authority.
    pub(crate) remote_failures: HashMap<String, crate::daemon::RemoteFailInfo>,
}

/// Host-verified github origin check (pure, unit-tested by the sync tests).
#[cfg(test)]
pub(super) fn is_github_url(url: &str) -> bool {
    crate::git::canonical_repository_url(url)
        .map(|canonical| canonical.starts_with("github.com/"))
        .unwrap_or(false)
}

/// Plan a round: pin the tip, measure the pushable pack, exclude destinations
/// that must not receive it, and prepare mirror configuration. Preparation
/// touches `.git/config`, so callers must hold local mutation ownership.
pub(super) async fn plan_publication(
    repo: &Path,
    policy: &SyncPolicy,
    has_origin: bool,
    remote_failures: Option<&HashMap<String, crate::daemon::RemoteFailInfo>>,
    ahead_count: u64,
) -> Result<PublicationPlan> {
    let target = pin_current_tip(repo).await?;
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
    // The relevant size is the pack we would actually send for the pushed
    // branch, NOT the whole `.git` (dracon-platform's 332 tags make `.git`
    // far larger than its pushable `main`). Retrying cannot fix a rejected
    // pack, so github is excluded for this round; self-healing resumes once
    // the branch shrinks below the limit.
    let (too_big_for_github, pushable_size) =
        crate::git::github_pack_too_large_async(repo, None).await;
    let github_already_flagged = remote_failures
        .as_ref()
        .and_then(|rf| rf.get("github"))
        .is_some_and(|f| f.consecutive > 0);
    let origin_url = if has_origin {
        crate::git::multi_remote::get_remote_url(repo, "origin")
    } else {
        None
    };
    let github_mirror_url = crate::git::multi_remote::get_remote_url(repo, "github");
    // Host comparison alone is insufficient: origin and the named github
    // mirror may be DIFFERENT repositories on github.com.
    let github_mirror_is_origin =
        github_mirror_matches_origin(origin_url.as_deref(), github_mirror_url.as_deref());

    let repo_override = load_repo_override(repo);
    let mut exclude: Vec<String> = repo_override.exclude_remotes.clone();
    // Codeberg's 85 GiB quota is reserved for public marketing mirrors.
    // Private or unknown-visibility repos skip it (unknown is the safe default).
    let codeberg_public_only = repo_override
        .codeberg_public_only
        .unwrap_or(policy.codeberg_public_only);
    if codeberg_public_only {
        let skip_codeberg = !matches!(
            cached_repo_visibility(repo, policy.sync_visibility_interval_hours),
            Some(false)
        );
        if skip_codeberg && !exclude.iter().any(|e| e == "codeberg") {
            exclude.push("codeberg".to_string());
        }
    }
    if github_mirror_is_origin && !exclude.iter().any(|e| e == "github") {
        exclude.push("github".to_string());
    }
    let mut planned_failures = remote_failures.as_deref().cloned().unwrap_or_default();
    if too_big_for_github {
        if !exclude.iter().any(|e| e == "github") {
            exclude.push("github".to_string());
        }
        // Synthetic guard entry, not a real attempt: leave `last_attempt_unix`
        // at its previous value so the pause gate keeps treating it as due.
        // The 2 GiB exclusion above governs it, not the pause gate.
        let fail = planned_failures.entry("github".to_string()).or_default();
        fail.consecutive += 1;
        if fail.last_error.is_empty() {
            fail.last_error =
                "skipped: pushable branch exceeds github's 2 GiB pack limit".to_string();
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

    // Mirror preparation (auto-create + remote configuration) is local
    // mutation. It runs here, never inside the upload, and the result is
    // re-filtered because a failed codeberg creation must not fall through to
    // a guaranteed-rejected push.
    let prepared = crate::git::multi_remote::prepare_mirror_remotes(
        repo,
        &policy.remotes,
        true,
        &exclude,
        repo_override.auto_create_on_codeberg,
        policy.sync_visibility_interval_hours,
    )
    .await;
    let remotes = crate::git::multi_remote::filter_remotes_by_exclude(&prepared, &exclude);
    Ok(PublicationPlan {
        target,
        timeout_secs: scaled_timeout,
        has_origin,
        origin_url,
        remotes,
        remote_failures: planned_failures,
    })
}

/// Read the exact tip to publish. Deliberately narrow: an unborn HEAD yields
/// `None`, while any other resolution/deadline failure surfaces so a round is
/// never planned against an unknown or moving source.
async fn pin_current_tip(repo: &Path) -> Result<Option<crate::git::PublicationTarget>> {
    let branch = crate::git::current_branch(repo).unwrap_or_else(|| "main".to_owned());
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
