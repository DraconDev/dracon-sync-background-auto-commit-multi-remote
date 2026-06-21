//! Push operations — HTTPS fallback, transport fallbacks, retry logic.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

/// Push with HTTPS fallback for GitHub/GitLab/Codeberg.
pub(crate) async fn push_https_fallback(
    repo: &Path,
    remote_url: &str,
    refspec: &str,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    let no_prompt = &[("GIT_TERMINAL_PROMPT", "0")];

    if let Some(https) = super::github_https_url(remote_url) {
        let result = super::run_git_with_timeout_env_progress(
            repo,
            &["push", "--no-verify", &https, refspec],
            timeout_secs,
            &format!("{}-github-https", op_label),
            no_prompt,
        )
        .await;
        if result.is_ok() {
            return Ok(());
        }
    }

    if let Some(https) = super::gitlab_https_url(remote_url) {
        if let Some(token) = super::load_secret("GITLAB_TOKEN") {
            match super::git_askpass_script(&token).await {
                Ok(askpass) => {
                    let result = super::run_git_with_timeout_env_progress(
                        repo,
                        &["push", "--no-verify", &https, refspec],
                        timeout_secs,
                        &format!("{}-gitlab-https", op_label),
                        &[
                            ("GIT_ASKPASS", askpass.to_str().unwrap_or("/bin/false")),
                            ("GIT_TERMINAL_PROMPT", "0"),
                        ],
                    )
                    .await;
                    let _ = tokio::fs::remove_file(&askpass).await;
                    if result.is_ok() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ failed to create GIT_ASKPASS helper for GitLab: {}", e);
                }
            }
        }
    }

    if let Some(https) = super::codeberg_https_url(remote_url) {
        if let Some(token) = super::load_secret("CODEBERG_TOKEN") {
            match super::git_askpass_script(&token).await {
                Ok(askpass) => {
                    let result = super::run_git_with_timeout_env_progress(
                        repo,
                        &["push", "--no-verify", &https, refspec],
                        timeout_secs,
                        &format!("{}-codeberg-https", op_label),
                        &[
                            ("GIT_ASKPASS", askpass.to_str().unwrap_or("/bin/false")),
                            ("GIT_TERMINAL_PROMPT", "0"),
                        ],
                    )
                    .await;
                    let _ = tokio::fs::remove_file(&askpass).await;
                    if result.is_ok() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ failed to create GIT_ASKPASS helper for Codeberg: {}", e);
                }
            }
        }
    }

    Err(anyhow::anyhow!("all HTTPS push attempts failed"))
}

/// Push with SSH first, then try HTTPS fallbacks.
pub(crate) async fn push_with_transport_fallbacks(
    repo: &Path,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    let ssh_hardening = crate::git::git_ssh_hardening();
    match super::run_git_with_timeout_env_progress(
        repo,
        &["push", "--no-verify", "origin", "HEAD"],
        timeout_secs,
        &format!("{op_label}-ssh-hardened"),
        &[
            ("GIT_SSH_COMMAND", ssh_hardening.as_str()),
            ("GIT_TERMINAL_PROMPT", "0"),
        ],
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            let err_msg = e.to_string();
            if is_permanent_push_rejection(&err_msg) {
                return Err(e);
            }
            let origin = super::origin_url(repo).unwrap_or_default();
            let branch = super::current_branch(repo).unwrap_or_else(|| "main".to_string());
            if !super::is_safe_branch_name(&branch) {
                eprintln!(
                    "⚠️ branch name '{}' is unsafe, skipping https fallback",
                    branch
                );
                return Err(e);
            }
            let refspec = format!("HEAD:refs/heads/{branch}");
            push_https_fallback(repo, &origin, &refspec, timeout_secs, op_label).await
        }
    }
}

/// Push with retries (SSH) and then HTTPS fallback.
///
/// On a `[rejected] (fetch first)` error (i.e. the local branch is behind
/// origin), runs `git pull --no-rebase origin HEAD` once and retries the
/// push. This unblocks repos where the local ahead has commits but origin
/// has moved forward (e.g. mirror pushed while local was idle). Without this,
/// the daemon would loop indefinitely on the same `fetch first` rejection.
pub(crate) async fn push_with_retries(
    repo: &Path,
    timeout_secs: u64,
    retries: u32,
    op_label: &str,
) -> Result<()> {
    let attempts = retries.max(1);
    let ssh_hardening = crate::git::git_ssh_hardening();
    let mut last_err: Option<anyhow::Error> = None;
    let mut tried_pull = false;
    for attempt in 1..=attempts {
        match super::run_git_with_timeout_env_progress(
            repo,
            &["push", "--no-verify", "origin", "HEAD"],
            timeout_secs,
            op_label,
            &[
                ("GIT_SSH_COMMAND", ssh_hardening.as_str()),
                ("GIT_TERMINAL_PROMPT", "0"),
            ],
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                let err_msg = e.to_string();
                // Server-side policy errors (protected branch, hook declined,
                // etc.) cannot be fixed by retries, pull, or HTTPS fallback.
                // Return immediately so the caller logs one incident per
                // cycle instead of burning the retry budget.
                if is_permanent_push_rejection(&err_msg) {
                    return Err(e);
                }
                last_err = Some(e);

                // On the first failure that looks like a non-fast-forward
                // (e.g. `! [rejected] HEAD -> main (non-fast-forward)` or
                // `! [rejected] HEAD -> main (fetch first)`), run
                // `git pull --no-rebase origin HEAD` once and let the
                // outer loop retry. This handles the common case where
                // the local branch is behind origin (e.g. a mirror
                // pushed while this repo was idle).
                if !tried_pull && is_push_rejected(&err_msg) {
                    tried_pull = true;
                    eprintln!(
                        "🔄 push rejected (non-fast-forward) for {} — pulling origin HEAD and retrying",
                        repo.display()
                    );
                    let pull_result = super::run_git_with_timeout_env_progress(
                        repo,
                        &["pull", "--no-rebase", "origin", "HEAD"],
                        timeout_secs,
                        &format!("{}-auto-pull", op_label),
                        &[
                            ("GIT_SSH_COMMAND", ssh_hardening.as_str()),
                            ("GIT_TERMINAL_PROMPT", "0"),
                        ],
                    )
                    .await;
                    match pull_result {
                        Ok(()) => {
                            // Don't sleep — retry the push immediately
                            // (we don't increment `attempt` either; treat
                            // the pull as part of the recovery).
                            continue;
                        }
                        Err(pull_err) => {
                            eprintln!(
                                "⚠️ auto-pull failed for {}: {} — continuing with retry",
                                repo.display(),
                                pull_err
                            );
                        }
                    }
                }

                if attempt < attempts {
                    let backoff = (attempt as u64).min(5);
                    eprintln!(
                        "⏱️ push retry {}/{} for {} after {}s",
                        attempt + 1,
                        attempts,
                        repo.display(),
                        backoff
                    );
                    sleep(Duration::from_secs(backoff)).await;
                    continue;
                }
            }
        }
    }
    if let Ok(()) = push_with_transport_fallbacks(repo, timeout_secs, op_label).await {
        return Ok(());
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("push failed")))
}

/// Check if an error message indicates a rejected push.
pub(crate) fn is_push_rejected(err_msg: &str) -> bool {
    err_msg.contains("rejected")
        || err_msg.contains("non-fast-forward")
        || err_msg.contains("fetch first")
        || err_msg.contains("[rejected]")
}

/// Check if an error message indicates a permanent push rejection that
/// retrying will not fix. These are server-side policy errors (protected
/// branches, required reviews, deny rules) that the daemon should
/// acknowledge once and stop retrying per cycle.
pub(crate) fn is_permanent_push_rejection(err_msg: &str) -> bool {
    err_msg.contains("pre-receive hook declined")
        || err_msg.contains("protected branch")
        || err_msg.contains("not allowed to push")
        || err_msg.contains("deny updating")
        || err_msg.contains("hook declined")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_permanent_push_rejection_recognises_gitlab_protected_branch() {
        let msg = "GitLab: You are not allowed to push code to protected branches on this project.\npre-receive hook declined";
        assert!(is_permanent_push_rejection(msg));
    }

    #[test]
    fn test_is_permanent_push_rejection_recognises_github_protected_branch() {
        let msg = "remote: error: GH006: Protected branch update failed for main.\n! [remote rejected] main -> main (protected branch hook declined)";
        assert!(is_permanent_push_rejection(msg));
    }

    #[test]
    fn test_is_permanent_push_rejection_ignores_transient_errors() {
        // A non-fast-forward is recoverable via rebase/fetch, not permanent.
        let msg = "non-fast-forward";
        assert!(!is_permanent_push_rejection(msg));
        // A network timeout is transient, not permanent.
        let msg = "connection timed out";
        assert!(!is_permanent_push_rejection(msg));
    }

    #[test]
    fn test_is_push_rejected_still_works() {
        assert!(is_push_rejected(
            "[rejected] main -> main (non-fast-forward)"
        ));
        assert!(!is_push_rejected("connection timed out"));
    }
}
