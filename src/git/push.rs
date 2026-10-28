//! Push operations — HTTPS fallback, transport fallbacks, retry logic.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

/// Redact embedded credentials from an error string before it is logged
/// or classified. FIXED 2026-10-03 (audit R3-L06): the old body matched
/// only literal `https://`, so `http://user:pass@...` (or ssh://, or any
/// other scheme) userinfo leaked in git error echoes — and the push
/// paths carried a second, narrower redactor diverging from the
/// all-scheme `ownership::redact_url_credentials` the ledger writes
/// use. One redactor now: delegate. Output shape changes from
/// `https://***@host/` to `https://host/` (the git/mod.rs shape test
/// was updated to the unified contract in the same change).
pub(crate) fn redact_credentials_for_log(msg: &str) -> String {
    crate::ownership::redact_url_credentials(msg)
}

/// Truncate retained error detail so per-forge context stays ledger-sized.
pub(crate) fn clip_error_detail(msg: &str) -> String {
    const LIMIT: usize = 500;
    let flat: String = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.len() <= LIMIT {
        return flat;
    }
    // Cut on a char boundary, never mid-codepoint.
    let mut end = LIMIT;
    while !flat.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &flat[..end])
}

/// Operator-facing ledger entry for an HTTPS fallback leg skipped for
/// lack of a token (R3-L05). Carries no token material — only the fact
/// of the skip, so the operator checks secrets instead of transport.
fn token_skip_entry(forge: &str) -> String {
    format!("{forge}: no token configured (skipped)")
}

/// Push with HTTPS fallback for GitHub/GitLab/Codeberg.
pub(crate) async fn push_https_fallback(
    repo: &Path,
    remote_url: &str,
    refspec: &str,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    let no_prompt = &[("GIT_TERMINAL_PROMPT", "0")];
    // FIX (audit M2, 2026-10-02): retain per-forge errors instead of
    // discarding them — the classifier mislabels policy rejections as
    // transport/auth when it only sees the generic summary.
    let mut failures: Vec<String> = Vec::new();

    if let Some(https) = super::github_https_url(remote_url) {
        // FIXED 2026-10-03 (audit R4-SC-06): wire GH_TOKEN through
        // git_askpass_script like the gitlab/codeberg legs — the old
        // leg pushed with no credentials at all, so the transport
        // fallback was ineffective exactly for github-primary repos.
        // Unlike those legs, a missing token does NOT skip: the
        // operator's `store` credential helper (verified live:
        // github.com entry present) rescues the unauthenticated
        // attempt, so skipping would regress working pushes.
        if let Some(token) = super::load_secret("GH_TOKEN") {
            match super::git_askpass_script(&token).await {
                Ok(askpass) => {
                    // HARDENED 2026-10-03 (unmapped; not a numbered R3 finding): a non-UTF8
                    // askpass path fails LOUD per-forge instead of
                    // silently pointing GIT_ASKPASS at /bin/false
                    // (every push then fails with a misleading error).
                    if let Some(askpass_str) = askpass.to_str() {
                        let _askpass_guard = super::AskpassScript::new(askpass.clone());
                        let result = super::run_git_with_timeout_env_progress(
                            repo,
                            &["push", &https, refspec],
                            timeout_secs,
                            &format!("{}-github-https", op_label),
                            &[("GIT_ASKPASS", askpass_str), ("GIT_TERMINAL_PROMPT", "0")],
                        )
                        .await;
                        match result {
                            Ok(()) => return Ok(()),
                            Err(e) => failures.push(format!(
                                "github: {}",
                                clip_error_detail(&redact_credentials_for_log(&e.to_string()))
                            )),
                        }
                    } else {
                        eprintln!("⚠️ GIT_ASKPASS path is not UTF-8 for GitHub; skipping forge");
                        failures.push("github: askpass path not UTF-8".to_string());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ failed to create GIT_ASKPASS helper for GitHub: {}", e);
                    failures.push("github: askpass setup failed".to_string());
                }
            }
        } else {
            // No GH_TOKEN: ambient credential helpers (e.g. `store`
            // with a github.com entry) may still authenticate this.
            let result = super::run_git_with_timeout_env_progress(
                repo,
                &["push", &https, refspec],
                timeout_secs,
                &format!("{}-github-https", op_label),
                no_prompt,
            )
            .await;
            match result {
                Ok(()) => return Ok(()),
                Err(e) => failures.push(format!(
                    "github: {}",
                    clip_error_detail(&redact_credentials_for_log(&e.to_string()))
                )),
            }
        }
    }

    if let Some(https) = super::gitlab_https_url(remote_url) {
        if let Some(token) = super::load_secret("GITLAB_TOKEN") {
            match super::git_askpass_script(&token).await {
                Ok(askpass) => {
                    // HARDENED 2026-10-03 (unmapped; not a numbered R3 finding): a non-UTF8
                    // askpass path fails LOUD per-forge instead of
                    // silently pointing GIT_ASKPASS at /bin/false
                    // (every push then fails with a misleading error).
                    if let Some(askpass_str) = askpass.to_str() {
                        let _askpass_guard = super::AskpassScript::new(askpass.clone());
                        let result = super::run_git_with_timeout_env_progress(
                            repo,
                            &["push", &https, refspec],
                            timeout_secs,
                            &format!("{}-gitlab-https", op_label),
                            &[("GIT_ASKPASS", askpass_str), ("GIT_TERMINAL_PROMPT", "0")],
                        )
                        .await;
                        match result {
                            Ok(()) => return Ok(()),
                            Err(e) => failures.push(format!(
                                "gitlab: {}",
                                clip_error_detail(&redact_credentials_for_log(&e.to_string()))
                            )),
                        }
                    } else {
                        eprintln!("⚠️ GIT_ASKPASS path is not UTF-8 for GitLab; skipping forge");
                        failures.push("gitlab: askpass path not UTF-8".to_string());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ failed to create GIT_ASKPASS helper for GitLab: {}", e);
                    failures.push("gitlab: askpass setup failed".to_string());
                }
            }
        } else {
            // FIXED 2026-10-03 (audit R3-L05): a missing/unreadable
            // token file silently skipped this leg, leaving only the
            // generic "all HTTPS push attempts failed" — the operator
            // chased transport instead of secrets. No token material.
            failures.push(token_skip_entry("gitlab"));
        }
    }

    if let Some(https) = super::codeberg_https_url(remote_url) {
        if let Some(token) = super::load_secret("CODEBERG_TOKEN") {
            match super::git_askpass_script(&token).await {
                Ok(askpass) => {
                    // HARDENED 2026-10-03 (unmapped; not a numbered R3 finding): a non-UTF8
                    // askpass path fails LOUD per-forge instead of
                    // silently pointing GIT_ASKPASS at /bin/false
                    // (every push then fails with a misleading error).
                    if let Some(askpass_str) = askpass.to_str() {
                        let _askpass_guard = super::AskpassScript::new(askpass.clone());
                        let result = super::run_git_with_timeout_env_progress(
                            repo,
                            &["push", &https, refspec],
                            timeout_secs,
                            &format!("{}-codeberg-https", op_label),
                            &[("GIT_ASKPASS", askpass_str), ("GIT_TERMINAL_PROMPT", "0")],
                        )
                        .await;
                        match result {
                            Ok(()) => return Ok(()),
                            Err(e) => failures.push(format!(
                                "codeberg: {}",
                                clip_error_detail(&redact_credentials_for_log(&e.to_string()))
                            )),
                        }
                    } else {
                        eprintln!("⚠️ GIT_ASKPASS path is not UTF-8 for Codeberg; skipping forge");
                        failures.push("codeberg: askpass path not UTF-8".to_string());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ failed to create GIT_ASKPASS helper for Codeberg: {}", e);
                    failures.push("codeberg: askpass setup failed".to_string());
                }
            }
        } else {
            // FIXED 2026-10-03 (audit R3-L05): see the GitLab leg —
            // skipped legs must say so (no token material).
            failures.push(token_skip_entry("codeberg"));
        }
    }

    if failures.is_empty() {
        return Err(anyhow::anyhow!("all HTTPS push attempts failed"));
    }
    Err(anyhow::anyhow!(
        "all HTTPS push attempts failed ({})",
        failures.join("; ")
    ))
}

// REMOVED 2026-10-03 (audit R4-SC-13): `push_with_transport_fallbacks`
// (redundant fresh SSH push + full HTTPS chain AFTER the retry loop
// exhausted) lived here. The HTTPS chain now runs at most once INSIDE
// `push_with_retries` under the shared total budget (mirror parity);
// the R3-L03 error join went with the sweep. `push_https_fallback`
// (the per-forge chain) is unchanged below.

/// Push with retries (SSH) and HTTPS fallback interleaved in ONE budget.
///
/// `retries` counts TOTAL push attempts (min 1), unified with
/// `push_to_named_remote` (audit L5): every executed `git push` — SSH
/// loop legs and HTTPS chain legs alike — spends one slot, so a sick
/// origin is never hammered harder per cycle than a sick mirror.
///
/// SUPERSEDED 2026-10-03 (audit R4-SC-13): the R3-L02 post-loop
/// `push_with_transport_fallbacks` sweep (a redundant fresh SSH push
/// plus the full per-forge HTTPS chain AFTER the loop exhausted —
/// retries=0 spawned extra pushes outside the budget) is gone. The
/// HTTPS chain now runs at most once inside the loop, right after the
/// first non-rejection SSH failure (mirror order: SSH, HTTPS, SSH…),
/// and the R3-L03 loop/fallback error join went with the sweep — the
/// final error is the last leg's (M3 cause-chaining preserved at the
/// HTTPS leg). Rejections (fetch-first/non-fast-forward) skip the
/// chain: a transport change cannot fix server state.
///
/// On a `[rejected] (fetch first)` error (i.e. the local branch is behind
/// origin), runs `git pull --no-rebase origin HEAD` once and retries the
/// push. This unblocks repos where the local ahead has commits but origin
/// has moved forward (e.g. mirror pushed while local was idle). Without this,
/// the daemon would loop indefinitely on the same `fetch first` rejection.
/// Build the fetch-first auto-pull refspec for the current branch.
///
/// FIXED 2026-10-03 (audit R4-SC-09): validated exactly like the push
/// refspecs — an exotic branch name bails here instead of reaching
/// `git pull origin <ref>` unvalidated (arg confusion). Extracted so
/// the gate is unit-testable (reaching it end-to-end needs a branch
/// rename to win the race between the push and the pull attempts).
pub(crate) fn pull_refspec_for_branch(
    branch: Option<String>,
    repo: &std::path::Path,
) -> anyhow::Result<String> {
    match branch {
        Some(b) if super::is_safe_branch_name(&b) => Ok(format!("refs/heads/{}", b)),
        Some(b) => Err(anyhow::anyhow!(
            "unsafe current branch '{}' in {}",
            b,
            repo.display()
        )),
        None => Ok("HEAD".to_string()),
    }
}

fn merge_in_progress(repo: &Path) -> Result<bool> {
    let gitdir = super::path_gitdir(repo)
        .context("cannot resolve Git directory while fencing pull/merge")?;
    match std::fs::symlink_metadata(gitdir.join("MERGE_HEAD")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("inspect merge state while fencing pull/merge"),
    }
}

/// Pull the current origin branch under the cooperative worktree fence.
/// Branch discovery, merge-state checks, Git's checkout, and any conflict
/// cleanup all happen while the same inherited fence remains held.
pub(crate) async fn pull_merge_origin(
    repo: &Path,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    pull_merge_origin_inner(repo, timeout_secs, op_label, false).await
}

/// Same fenced pull for the push-recovery path, preserving its progress-aware
/// idle timeout while still bounding the Git process family.
pub(crate) async fn pull_merge_origin_progress(
    repo: &Path,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    pull_merge_origin_inner(repo, timeout_secs, op_label, true).await
}

/// Repair-path pull using the branch's configured upstream, matching plain
/// `git pull` semantics while serializing its possible worktree checkout.
pub(crate) async fn pull_merge_upstream(
    repo: &Path,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    let fence = acquire_pull_fence(repo, timeout_secs).await?;
    pull_with_fence(repo, &["pull", "--no-rebase", "--no-edit"], timeout_secs, op_label, false, &fence)
        .await
}

async fn acquire_pull_fence(
    repo: &Path,
    timeout_secs: u64,
) -> Result<super::write_fence::WriteFence> {
    let wait_secs = timeout_secs.clamp(1, 30);
    super::write_fence::WriteFence::acquire_until(
        repo,
        tokio::time::Instant::now() + Duration::from_secs(wait_secs),
    )
    .await
}

async fn pull_merge_origin_inner(
    repo: &Path,
    timeout_secs: u64,
    op_label: &str,
    progress_aware: bool,
) -> Result<()> {
    let fence = acquire_pull_fence(repo, timeout_secs).await?;
    let pull_refspec = pull_refspec_for_branch(super::current_branch_from_head(repo), repo)?;
    eprintln!(
        "🔄 {} pulling origin {} in {}",
        op_label,
        pull_refspec,
        repo.display()
    );
    let pull_args = ["pull", "--no-rebase", "--no-edit", "origin", &pull_refspec];
    pull_with_fence(
        repo,
        &pull_args,
        timeout_secs,
        op_label,
        progress_aware,
        &fence,
    )
    .await
}

async fn pull_with_fence(
    repo: &Path,
    pull_args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    progress_aware: bool,
    fence: &super::write_fence::WriteFence,
) -> Result<()> {
    anyhow::ensure!(
        !merge_in_progress(repo)?,
        "refusing pull/merge in {}: a merge was already in progress",
        repo.display()
    );
    let ssh_hardening = super::git_ssh_hardening();
    let env = [
        ("GIT_SSH_COMMAND", ssh_hardening.as_str()),
        ("GIT_TERMINAL_PROMPT", "0"),
    ];
    let pull_result = if progress_aware {
        super::run_git_with_timeout_env_progress_fenced(
            repo,
            pull_args,
            timeout_secs,
            op_label,
            &env,
            fence,
        )
        .await
    } else {
        super::run_git_with_timeout_env_fenced(
            repo,
            pull_args,
            timeout_secs,
            op_label,
            &env,
            fence,
        )
        .await
    };
    if let Err(pull_error) = pull_result {
        // Abort only a merge created by this fenced pull. A pre-existing
        // operator merge is refused above and is never cleaned up here.
        if merge_in_progress(repo)? {
            if let Err(abort_error) = super::run_git_with_timeout_fenced(
                repo,
                &["merge", "--abort"],
                15,
                "auto-pull-abort",
                fence,
            )
            .await
            {
                return Err(
                    pull_error.context(format!("pull/merge abort also failed: {abort_error:#}"))
                );
            }
        }
        return Err(pull_error);
    }
    Ok(())
}

pub(crate) fn is_merge_conflict_error(error: &anyhow::Error) -> bool {
    error.to_string().to_ascii_lowercase().contains("conflict")
}

fn eligible_ref_race(error: &str) -> bool {
    let lower = error.to_lowercase();
    // The generic classifier recognizes pre-push banners. A captured success
    // banner is also common in ref-update races; explicit refusals still win.
    let gates_passed = lower.contains("4-gate passed for all")
        && !lower.contains("4-gate failed")
        && !lower.contains("possible plaintext secrets")
        && !lower.contains("refusing")
        && !lower.contains("dracon-warden:")
        && !lower.contains("hook declined")
        && !lower.contains("pre-commit");
    error.contains("cannot lock ref")
        && error.contains("but expected")
        && (!is_local_hook_rejection(error) || gates_passed)
}

fn ref_race_is_converged(
    error: &str,
    before: &str,
    after: &str,
    advertised: &str,
    branch: &str,
) -> bool {
    if !eligible_ref_race(error) || before != after || !super::is_valid_object_id(before) {
        return false;
    }
    let expected = format!("refs/heads/{branch}");
    let lines: Vec<_> = advertised.lines().collect();
    lines.len() == 1
        && lines[0].split_whitespace().collect::<Vec<_>>() == [before, expected.as_str()]
}

/// A concurrent sender may have published exactly our desired tip after Git's
/// advertisement. Reconcile only that ref-update race, using fresh remote
/// evidence and an unchanged local HEAD. Hook/policy errors stay failures.
pub(crate) async fn converged_after_ref_race_for_target(
    repo: &Path,
    remote: &str,
    error: &str,
    target: Option<&PublicationTarget>,
) -> bool {
    if !eligible_ref_race(error) {
        return false;
    }
    let branch = target
        .map(|target| target.branch().to_owned())
        .unwrap_or_else(|| super::branch::current_branch(repo).unwrap_or_else(|| "main".into()));
    if !super::is_safe_branch_name(&branch) || remote.starts_with('-') {
        return false;
    }
    async fn capture(repo: &Path, args: &[&str]) -> Option<String> {
        let child =
            super::spawn_git_command_cancellable(repo, args, "ref-race-verification").ok()?;
        let (status, stdout, _) = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            super::run_git_captured_output(child, repo, "ref-race-verification"),
        )
        .await
        .ok()?
        .ok()?;
        status
            .success()
            .then(|| String::from_utf8_lossy(&stdout).trim().to_string())
    }
    let before = match target {
        Some(target) => target.object_id().to_owned(),
        None => match capture(repo, &["rev-parse", "--verify", "HEAD"]).await {
            Some(head) => head,
            None => return false,
        },
    };
    let ssh = format!("core.sshCommand={}", crate::git::git_ssh_hardening());
    let remote_ref = format!("refs/heads/{branch}");
    let Some(advertised) = capture(
        repo,
        &["-c", &ssh, "ls-remote", "--refs", remote, &remote_ref],
    )
    .await
    else {
        return false;
    };
    let after = match target {
        Some(target) => target.object_id().to_owned(),
        None => match capture(repo, &["rev-parse", "--verify", "HEAD"]).await {
            Some(head) => head,
            None => return false,
        },
    };
    ref_race_is_converged(error, &before, &after, &advertised, &branch)
}

/// A validated immutable source and destination branch for one publication
/// round. It deliberately cannot represent moving HEAD or a force refspec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicationTarget {
    object_id: String,
    branch: String,
}

impl PublicationTarget {
    pub(crate) fn new(object_id: &str, branch: &str) -> Result<Self> {
        anyhow::ensure!(
            super::is_valid_object_id(object_id) && object_id.bytes().any(|byte| byte != b'0'),
            "invalid or null publication object ID"
        );
        anyhow::ensure!(
            super::is_safe_branch_name(branch)
                && git2::Reference::is_valid_name(&format!("refs/heads/{branch}")),
            "unsafe publication branch"
        );
        Ok(Self {
            object_id: object_id.to_ascii_lowercase(),
            branch: branch.to_owned(),
        })
    }

    pub(crate) fn object_id(&self) -> &str {
        &self.object_id
    }

    pub(crate) fn branch(&self) -> &str {
        &self.branch
    }

    pub(crate) fn refspec(&self) -> String {
        format!("{}:refs/heads/{}", self.object_id, self.branch)
    }
}

pub(crate) async fn push_with_retries(
    repo: &Path,
    timeout_secs: u64,
    retries: u32,
    op_label: &str,
) -> Result<()> {
    push_with_retries_for_target(repo, timeout_secs, retries, op_label, None).await
}

/// A pinned round never reconciles by mutating the checkout. A rejection is
/// returned to its owner for separately leased local reconciliation.
pub(crate) async fn push_with_retries_for_target(
    repo: &Path,
    timeout_secs: u64,
    retries: u32,
    op_label: &str,
    target: Option<&PublicationTarget>,
) -> Result<()> {
    let budget = retries.max(1);
    let ssh_hardening = crate::git::git_ssh_hardening();
    let mut last_err: Option<anyhow::Error> = None;
    let mut tried_pull = false;
    let mut tried_https = false;
    let mut spent: u32 = 0;
    while spent < budget {
        spent += 1;
        // CHANGED 2026-07-02 (goal `354fe3cb`):
        // When the worktree is detached, `git push origin HEAD` fails.
        // Build a fully-qualified refspec instead.
        //
        // CHANGED 2026-08-09 (v0.113.48): always use the
        // fully-qualified `HEAD:refs/heads/<branch>` form when a branch
        // is known. Bare `HEAD` fails with the same refspec error on a
        // detached worktree.
        let ssh_refspec = match target {
            Some(target) => target.refspec(),
            None => match crate::git::branch::current_branch(repo) {
                Some(branch) if super::is_safe_branch_name(&branch) => {
                    format!("HEAD:refs/heads/{branch}")
                }
                Some(branch) => {
                    return Err(anyhow::anyhow!(
                        "unsafe current branch '{}' in {}",
                        branch,
                        repo.display()
                    ));
                }
                None => "HEAD:refs/heads/main".to_string(),
            },
        };
        match super::run_git_with_timeout_env_progress(
            repo,
            &["push", "origin", &ssh_refspec],
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
                if Box::pin(converged_after_ref_race_for_target(
                    repo, "origin", &err_msg, target,
                ))
                .await
                {
                    eprintln!(
                        "✅ {} origin already has the requested tip after a concurrent push",
                        repo.display()
                    );
                    return Ok(());
                }
                // Server-side policy errors (protected branch, hook declined,
                // etc.) AND oversized-pack errors cannot be fixed by retries,
                // pull, or HTTPS fallback. Return immediately so the caller
                // logs one incident per cycle instead of burning the retry
                // budget.
                //
                // FIXED 2026-10-08 (audit F111): a LOCAL pre-push hook
                // refusal belongs in this set. commit f8a543a added
                // `is_local_hook_rejection` for the classifier ("local
                // pre-push hook refused the push") and its own message
                // named "burned the full stuck budget on retries that can
                // never succeed" as part of the defect — but this fail-fast
                // set was never extended, so a local hook refusal still ran
                // the pull-on-rejected probe and the entire retry budget
                // every cycle. The two sets are disjoint (server-side
                // refusals say "pre-receive", not "pre-push"), so adding
                // the arm cannot swallow a server policy rejection.
                if is_permanent_push_rejection(&err_msg)
                    || is_pack_too_large(&err_msg)
                    || is_local_hook_rejection(&err_msg)
                {
                    return Err(e);
                }
                if target.is_some() && is_push_rejected(&err_msg) {
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
                    // CHANGED 2026-07-26 (v0.113.3, audit M7): three
                    // hazards in the pre-fix auto-pull — (1) `HEAD` as
                    // a fetch refspec resolves to the remote's DEFAULT
                    // branch, which may differ from the branch being
                    // pushed (merging the WRONG branch into the pushed
                    // one); pull the explicit branch instead. (2) No
                    // `--no-edit`: git opens $EDITOR for the merge
                    // commit when stdin is a tty (`dracon-sync once` /
                    // `dracon-sync repair concerns --apply` from a terminal could
                    // hang inside vim). (3) On conflict the pull left
                    // the repo in MERGING state (which the pre-v0.113.2
                    // conflict check couldn't even detect for nested
                    // submodules); abort instead.
                    let pull_result = pull_merge_origin_progress(
                        repo,
                        timeout_secs,
                        &format!("{}-auto-pull", op_label),
                    )
                    .await;
                    match pull_result {
                        Ok(()) => {
                            // Pull succeeded — retry the push immediately
                            // (skipping the backoff sleep below). Note that
                            // this `continue` DOES advance `attempt`: the
                            // range iterator advances on every iteration,
                            // so the post-pull retry consumes one slot of
                            // the retry budget — the pull is recovery, not
                            // a free retry (CORRECTED 2026-08-10, audit
                            // LOW: the pre-fix note claimed "we don't
                            // increment `attempt` either", which was wrong).
                            continue;
                        }
                        Err(pull_err) => {
                            // FIXED 2026-10-03 (audit R3-L06): this raw
                            // journal print echoed the pull error verbatim,
                            // bypassing the redactor (a credential-bearing
                            // remote URL would land in the journal).
                            eprintln!(
                                "⚠️ auto-pull failed for {}: {} — continuing with retry",
                                repo.display(),
                                redact_credentials_for_log(&pull_err.to_string())
                            );
                        }
                    }
                }

                // ADDED 2026-10-03 (audit R4-SC-13): HTTPS interleave
                // inside the shared budget (mirror order: SSH, HTTPS,
                // SSH…). Runs at most once, after the first SSH failure
                // a transport change could fix — rejections
                // (fetch-first/non-fast-forward) are server state, so
                // the chain would burn a slot on a doomed push. The
                // slot is spent only when the chain can execute: the
                // forge mappers are mutually exclusive (at most one leg
                // runs), and with no match at all the call pushes
                // nothing. M3 cause-chaining preserved (mirror shape).
                if !tried_https && spent < budget && !is_push_rejected(&err_msg) {
                    tried_https = true;
                    let origin = super::origin_url(repo).unwrap_or_default();
                    if super::github_https_url(&origin).is_some()
                        || super::gitlab_https_url(&origin).is_some()
                        || super::codeberg_https_url(&origin).is_some()
                    {
                        let branch = target
                            .map(|target| target.branch().to_owned())
                            .unwrap_or_else(|| {
                                super::current_branch(repo).unwrap_or_else(|| "main".to_string())
                            });
                        if !super::is_safe_branch_name(&branch) {
                            return Err(anyhow::anyhow!(
                                "unsafe current branch '{}' in {}",
                                branch,
                                repo.display()
                            ));
                        }
                        spent += 1;
                        let refspec = target
                            .map(PublicationTarget::refspec)
                            .unwrap_or_else(|| format!("HEAD:refs/heads/{branch}"));
                        match push_https_fallback(repo, &origin, &refspec, timeout_secs, op_label)
                            .await
                        {
                            Ok(()) => return Ok(()),
                            Err(fallback_err) => {
                                let prev =
                                    last_err.take().map(|e| e.to_string()).unwrap_or_default();
                                last_err = Some(anyhow::anyhow!(
                                    "{} [SSH attempt failed: {}]",
                                    fallback_err,
                                    clip_error_detail(&redact_credentials_for_log(&prev))
                                ));
                            }
                        }
                    }
                }

                if spent < budget {
                    let backoff = (spent as u64).min(5);
                    eprintln!(
                        "⏱️ push retry {}/{} for {} after {}s",
                        spent + 1,
                        budget,
                        repo.display(),
                        backoff
                    );
                    sleep(Duration::from_secs(backoff)).await;
                    continue;
                }
            }
        }
    }
    // REMOVED 2026-10-03 (audit R4-SC-13): the post-loop
    // `push_with_transport_fallbacks` sweep (and its R3-L03 error join)
    // with it — the HTTPS chain now runs inside the loop under the
    // shared budget. Final error is the last leg's (mirror parity);
    // the HTTPS leg already carries its M3 SSH-cause chain.
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("push to origin failed")))
}

/// ADDED 2026-10-07: a LOCAL pre-push/pre-commit hook refused the push.
///
/// The bytes never left the machine and no retry, credential refresh or
/// network repair can change the outcome — the hook is reporting a policy
/// decision (dracon-platform's bucket asset-retirement guard, its repo
/// size budget, the warden's secret scan, or the history guard). Before
/// this arm existed, none of the predicates matched such a message and it
/// fell through to the final `else`, which reported
///
///     transport/auth failure (network, timeout, or credentials)
///
/// That sent the operator to the network and credentials instead of to
/// the policy that actually blocked the push, and it burned the entire
/// stuck budget on a rejection that retrying can never fix. This is the
/// same class of misdirection `classify_push_failure` was introduced in
/// v0.113.50 to prevent, third instance.
///
/// The markers are the client-side hook labels and warnings Git echoes from
/// the hook's own stderr. Server-side rejections use `pre-receive` /
/// `hook declined` and are matched by `is_permanent_push_rejection`, so the
/// two sets do not collide — `pre-receive` does not contain `pre-push`.
///
/// Not every warden refusal names a `pre-push` label: the secret scan prints
/// only its own warning, so `Possible plaintext secrets` is matched too. That
/// omission is not theoretical — it is why the 2026-10-07 `pi-goal-list-loop-audit`
/// wedge was reported as a transport failure.
///
/// ADDED 2026-10-14 (fleet-wide push-stuck triage, this session): the
/// predicate was CASE-SENSITIVE, so every repo whose pre-push hook labels
/// its own output with a capital `P` never matched and still reported
/// `transport/auth failure (network, timeout, or credentials)`. The
/// browser-extensions-shared warden hook (`scripts/git-hooks/pre-push`,
/// installed via `core.hooksPath`) prints
///
///     🔍 Pre-push: 6 affected workspace(s) — running 4-gate (typecheck, build, test, lint).
///     ❌ Pre-push: 4-gate FAILED for:
///       - extensions/standalones/page-ninja/TEST (see /tmp/pre-push-…log)
///
/// Neither line contains lowercase `pre-push`, so a repo blocked by its own
/// typecheck/build/test/lint gate was reported as a network or credential
/// problem and burned the 5-failure stuck budget on it — the operator was sent
/// to the network while the failing gate log sat on disk. Matching now runs on
/// a lowercased copy, and the 4-gate verdict line is matched directly so the
/// class is recognized even if a hook rewords its banner. The lowercased
/// `pre-commit` arm is unchanged in meaning (git's own hook-echo is lowercase);
/// server-side markers (`pre-receive`, `hook declined`) contain neither string
/// and stay in the `is_permanent_push_rejection` arm, so the sets remain
/// disjoint.
pub(crate) fn is_local_hook_rejection(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    // FIXED 2026-10-28 (browser-extensions-shared GitLab wedge): a PASSING
    // gate is not a refusal. The daemon's runner appends captured hook STDOUT
    // to the error text (src/git/ops.rs), and the workspace gate prints its
    // own "Pre-push:" banner on SUCCESS as well as on refusal. So the
    // substring test below matched a SUCCESSFUL gate, and the transport
    // failure that followed it (GitLab closing the SSH session at its 600s
    // cap, while the ~12-minute gate was still running inside that session)
    // was classified as "local pre-push hook failed".
    //
    // That misclassification was not cosmetic: `is_local_hook_rejection`
    // gates the fail-fast arms in push_with_retries_for_target and
    // push_to_named_remote_inner, so the daemon never attempted the
    // HTTPS transport (which has no session cap) and never retried with a
    // fresh connection — the mirror stayed 111+ commits ahead for hours.
    //
    // A refusal is now required to carry a refusal marker. A bare banner
    // ("Pre-push: N affected workspace(s)") without any refusal evidence
    // still classifies as a refusal, because a gate that cannot even report
    // its verdict is not something to retry blindly.
    if gate_refusal_markers(&lower) {
        return true;
    }
    // No refusal evidence: the only "pre-push" text may be a success banner
    // that merely travelled with the error. Such a message describes a
    // transport problem, not a policy decision.
    if gate_passed_markers(&lower) {
        return false;
    }
    lower.contains("pre-push")
        || lower.contains("pre-commit")
        || lower.contains("dracon-warden:")
        || lower.contains("dracon-warden hook")
        || lower.contains("possible plaintext secrets")
        // A workspace gate refusal (typecheck / build / test / lint) is the
        // same class: the bytes never left the machine.
        || lower.contains("4-gate failed")
        || lower.contains("pre-push:")
}

/// Markers that positively identify a LOCAL hook refusal. Only these make a
/// message a policy decision; a success banner alone does not.
fn gate_refusal_markers(lower: &str) -> bool {
    lower.contains("4-gate failed")
        || lower.contains("4-gate failed for")
        || lower.contains("possible plaintext secrets")
        || lower.contains("dracon-warden:")
        || lower.contains("dracon-warden hook")
        || lower.contains("refusing")
        || lower.contains("blocked this push")
        // NOT "hook declined": that is a SERVER-side marker ("pre-receive
        // hook declined") and must stay in the permanent-rejection arm so
        // the two sets remain disjoint.
        || lower.contains("pre-commit")
        || lower.contains("pre-commit:")
}

/// Markers that identify a SUCCESSFUL gate. Their presence in an error text
/// means the hook ran and PASSED, so the failure is downstream of the hook.
fn gate_passed_markers(lower: &str) -> bool {
    lower.contains("4-gate passed for all")
        || lower.contains("4-gate passed")
        || lower.contains("pre-push: 4-gate passed")
}

/// Check if an error message indicates the DAEMON'S OWN op-timeout kill —
/// the local process runner gave up waiting for the push and killed the
/// process group.
///
/// The runner's message is (see `run_child_inner` in `git/ops.rs`):
///
///     git push-to-gitlab timeout in <repo> after <n>s idle (<m>s hard cap)
///
/// ADDED 2026-10-22 (dracon-strategy 19h wedge): this message matched NONE
/// of the predicates above, so it fell through `classify_push_failure` to the
/// generic final `else` and was reported as
///
///     transport/auth failure (network, timeout, or credentials)
///
/// The runner's deadline event should be identifiable without implying a
/// credential failure. It does not alone identify why progress stopped:
/// local hooks and remote transfers both run under that deadline. Even a
/// successful separate remote advertisement cannot prove the timed-out
/// transfer was healthy. Repeated timeouts can exhaust the stuck budget.
///
/// This is a LABEL fix only. It deliberately does NOT change the budget
/// behaviour: a locally wedged push must still escalate against the stuck
/// budget (same contract as the op-timeout note in
/// `is_transient_forge_outage`) — the operator just gets told the truth
/// instead of being sent to inspect a working network.
pub(crate) fn is_local_op_timeout(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    // The daemon's own runner format: "<label> timeout in <dir> after <n>s
    // idle (<m>s hard cap)". `hard cap` is unique to that runner (verified:
    // the only producer in the tree is `git/ops.rs`), so this cannot collide
    // with a forge-side "Push operation timed out".
    lower.contains("hard cap)") && lower.contains(" timeout in ")
}

/// Check if an error message indicates a rejected push.
pub(crate) fn is_push_rejected(err_msg: &str) -> bool {
    err_msg.contains("rejected")
        || err_msg.contains("non-fast-forward")
        || err_msg.contains("fetch first")
        || err_msg.contains("[rejected]")
}

/// ADDED 2026-08-09 (v0.113.50, pi-goal-loop-audit divergence incident):
/// human-readable cause for a failed push, so the Mirror Degraded
/// alert and the stuck-ledger `last_error` say WHY instead of the
/// pre-fix "mirror may be unreachable" (which misdirected the operator
/// to network/credentials when the true cause was a history fork).
/// Mirrors the predicate set above; keep the arms in the same order.
pub(crate) fn classify_push_failure(err_msg: &str) -> &'static str {
    // ADDED 2026-09-20 (v0.113.83): DNS failures get their own cause
    // string (local network, not forge infra) but share the transient
    // handling — keep this arm FIRST so the message names DNS, while
    // the predicate order below is unchanged.
    if is_transient_network_outage(err_msg) {
        "local network/DNS failure (name resolution; retrying with backoff, excluded from stuck budget)"
    } else if is_transient_forge_outage(err_msg) {
        "forge-side outage (transient infra: Gitaly/5xx; retrying with backoff, excluded from stuck budget)"
    } else if is_pack_too_large(err_msg) {
        "pack exceeds forge size limit (needs history rewrite)"
    } else if is_local_hook_rejection(err_msg) {
        "local pre-push hook failed (inspect retained hook output, product gates and the service environment; this does not establish a network failure or justify weakening policy)"
    } else if is_permanent_push_rejection(err_msg) {
        "server-side policy rejection (protected branch / hook declined / missing repo / lost key)"
    } else if is_push_rejected(err_msg) {
        "history divergence (non-fast-forward: remote has commits not on local; needs operator reconciliation)"
    } else if is_local_op_timeout(err_msg) {
        // ADDED 2026-10-22 (dracon-strategy 19h wedge): the daemon's own
        // op-timeout kill. Placed AFTER the transient arms so a genuine
        // forge-side outage still wins, and after the divergence arm so a
        // non-fast-forward is not relabelled. This identifies the deadline
        // event, not its root cause: a hook, payload, or remote transfer can
        // all consume the budget. It cannot establish remote reachability.
        "local operation timeout (daemon stopped waiting at its deadline — inspect hook/transfer progress and the operation budget; remote health is not established)"
    } else {
        "transport/auth failure (network, timeout, or credentials)"
    }
}

/// Check if an error message indicates a transient LOCAL-NETWORK outage
/// (DNS resolution failure), as opposed to a forge-side problem or a repo
/// policy decision.
///
/// ADDED 2026-09-20 (v0.113.83): during a dead-LAN/DNS window every repo
/// × every remote fails with `Could not resolve hostname ...: Temporary
/// failure in name resolution`. Observed live 2026-09-20 02:20–02:28:
/// the DNS outage was not transient-class, so it burned the stuck budget
/// on two repos (5 consecutive → Exhausted → auto-push latched paused →
/// manual `repair stuck-unstuck` for a condition that self-healed in
/// minutes) and paged per-repo/per-remote through the whole window.
/// These share the transient handling contract (no budget burn, backoff,
/// corroborate the outage, shield while covered) even though they are
/// client-side, not forge-side — hence a SEPARATE predicate from
/// [`is_transient_forge_outage`]: callers must OR both. Matching stays
/// narrow to DNS-resolution strings; bare `Connection timed out` and
/// op-timeout kills still count (a locally wedged push must escalate).
pub(crate) fn is_transient_network_outage(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    // `could not resolve host` covers both `host` and `hostname` forms.
    lower.contains("could not resolve host")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("name or service not known")
}
/// Check if an error message indicates a transient forge-side infrastructure
/// outage (NOT a repo policy decision): GitLab's Gitaly storage backend
/// unavailable, HTTP 5xx from the forge, explicit try-again-later replies.
/// Observed live 2026-09-18: `web-games-doomtap` pack receipt failed with
/// "ERROR: The git server, Gitaly, is not available at this time" while the
/// identical commits pushed cleanly to github and to the `doomtap` GitLab
/// project seconds apart, and the GitLab API returned HTTP 500s at the same
/// time. Retrying will not fix it NOW, but unlike a policy rejection it can
/// clear on its own — so callers must back off and retry WITHOUT burning
/// the needs-human stuck-push budget (see `record_push_attempt_error`).
/// Matching is deliberately narrow: only unambiguous infra strings. A
/// `pre-receive hook declined` stays permanent (a rule decision), even
/// though a sick backend can also trip hooks spuriously — misclassifying a
/// real secret/branch rule as transient would retry a doomed push forever.
pub(crate) fn is_transient_forge_outage(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    // Native hook pools can report local EAGAIN using the same phrase as a
    // forge outage. An explicit local worker-spawn failure is not evidence
    // that a forge is unavailable, and must not waive the stuck budget.
    if is_local_hook_rejection(err_msg) && lower.contains("os can't spawn worker thread") {
        return false;
    }
    lower.contains("gitaly")
        || lower.contains("is not available at this time")
        || lower.contains("internal server error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("temporarily unavailable")
        || lower.contains("try again later")
        // ADDED 2026-09-18 (v0.113.66): GitLab server-side push timeout
        // ("remote: GitLab: Push operation timed out") seen live on
        // web-games-endless-td during the same Gitaly degradation that
        // produced doomtap's Gitaly-unavailable. The forge gave up on
        // ITS side — retryable, not a rule decision. Deliberately NOT
        // matching bare client-side timeouts ("connection timed out",
        // our own op-timeout kills): those still count, so a locally
        // wedged push still escalates to the operator.
        || lower.contains("push operation timed out")
        || lower.contains("operation timed out")
        // ADDED 2026-09-18 (v0.113.73): git's own HTTP-transport
        // message for forge 5xx ("The requested URL returned error:
        // 503") — previously only spelled-out phrases matched, so a
        // 5xx with an empty body fell through to transport/auth.
        || lower.contains("returned error: 500")
        || lower.contains("returned error: 502")
        || lower.contains("returned error: 503")
        || lower.contains("returned error: 504")
        || lower.contains("error 520")
        || lower.contains("error 522")
        || lower.contains("error 524")
        || lower.contains("code: 520")
        || lower.contains("code: 522")
        || lower.contains("code: 524")
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
        // ADDED 2026-07-21 (v0.112.33, audit M15/F2.6): deleted or
        // never-created forge repo, and lost key access —
        // definitionally unfixable by retrying. The pre-fix code
        // burned the full retry budget (with backoff sleeps) on
        // every cycle forever for exactly the repos the v0.112.28
        // codeberg posture creates (auto_create off + repo deleted).
        // Failing fast hands the repo to the H5 stuck-push budget
        // (v0.112.31), which provides the actual stop condition.
        || err_msg.contains("Repository not found")
        || err_msg.contains("repository does not exist")
        || err_msg.contains("Push to create is not enabled")
        || err_msg.contains("The project you were looking for could not be found")
        || err_msg.contains("Permission denied (publickey)")
}

/// Check if an error message indicates the push was rejected because the
/// pack (or a single file) exceeds the remote's size limit. These are NOT
/// fixable by retrying — the history must be rewritten (or the asset moved
/// out of git) before the push can succeed.
///
/// github's hard limit is 2 GiB per pack; GitLab/Codeberg have much higher
/// (or no practical) limits, so this is overwhelmingly a github-specific
/// failure. Retrying it is pure waste: git still has to re-pack the entire
/// local history (slow, and it saturates the daemon's push semaphore),
/// only for the remote to reject it again. Treat as permanent — stop
/// retrying this remote immediately.
///
/// Proactive handling (skipping the push entirely when `.git` > 2 GB) lives
/// in `push_background` via `measure_git_size_bytes`; this function is the
/// defensive backstop for when the remote actually returns the error.
pub(crate) fn is_pack_too_large(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    lower.contains("gh001")
        || lower.contains("large files detected")
        || lower.contains("pack exceeds")
        || lower.contains("exceeds the maximum allowed size")
        || lower.contains("maximum allowed size")
        || lower.contains("remote error: pack")
        || lower.contains("pack is too large")
        || lower.contains("deny updating a hidden ref")
}

/// ADDED 2026-07-26 (v0.113.3, audit SYNC-H6): force-push one remote
/// after a history rewrite, leased to the PRE-REWRITE upstream sha.
///
/// Why not `push_with_retries`: (a) the rewrite intentionally
/// diverges local from remote, so a non-force push is rejected by
/// design; (b) the auto-pull-on-reject recovery would merge the
/// PRE-REWRITE history back in — the exact catastrophe SYNC-H6
/// documents (the >100 MiB blob returns to local history and is
/// pushed to all mirrors). The lease anchors the force to the sha the
/// remote held before the rewrite: if the remote moved since (a
/// racing push), the lease fails and we log instead of clobbering.
///
/// `lease` = (full ref name, expected pre-rewrite sha) captured from
/// the pre-rewrite upstream tracking ref. When `lease` is `None`
/// (repo had no upstream — practically unreachable here since the
/// large-blob detector itself needs `@{u}`), falls back to plain
/// `--force` with a loud log: the auto-repair is documented to
/// force-push, and the lease is belt-and-braces.
pub(crate) async fn force_push_after_rewrite(
    repo: &Path,
    remote: &str,
    branch: &str,
    lease: &Option<(String, String)>,
    timeout_secs: u64,
) -> Result<()> {
    if !super::is_safe_branch_name(branch) {
        return Err(anyhow::anyhow!("unsafe branch name '{}'", branch));
    }
    let lease_flag = match lease {
        Some((reference, expect)) => format!("--force-with-lease={}:{}", reference, expect),
        None => {
            eprintln!(
                "⚠️ no pre-rewrite upstream sha for {} — force-pushing {} WITHOUT lease",
                repo.display(),
                remote
            );
            "--force".to_string()
        }
    };
    let refspec = format!("HEAD:refs/heads/{}", branch);
    let ssh_hardening = crate::git::git_ssh_hardening();
    super::run_git_with_timeout_env_progress(
        repo,
        &["push", &lease_flag, remote, &refspec],
        timeout_secs,
        &format!("push-after-rewrite ({})", remote),
        &[
            // This specific maintenance operation is policy-approved. Keep
            // pre-push content checks and operator hook chaining enabled.
            ("DRACON_ALLOW_REWRITE", "1"),
            ("GIT_SSH_COMMAND", ssh_hardening.as_str()),
            ("GIT_TERMINAL_PROMPT", "0"),
        ],
    )
    .await
}

#[cfg(test)]
#[path = "push_publication_tests.rs"]
mod publication_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::write_fence::WriteFence;

    #[cfg(unix)]
    #[tokio::test]
    async fn fenced_pull_keeps_native_git_merge_serialized_through_post_merge_close() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        fn git(cwd: &Path, args: &[&str]) -> std::process::Output {
            crate::test_helpers::test_git_cmd()
                .args(args)
                .current_dir(cwd)
                .output()
                .unwrap()
        }
        fn git_ok(cwd: &Path, args: &[&str]) {
            let output = git(cwd, args);
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let bare = temp.path().join("origin.git");
        let remote_worktree = temp.path().join("remote-edit");
        let quiet_hooks = temp.path().join("quiet-hooks");
        let active_hooks = temp.path().join("active-hooks");
        std::fs::create_dir(&quiet_hooks).unwrap();
        std::fs::create_dir(&active_hooks).unwrap();
        std::fs::create_dir(&repo).unwrap();
        git_ok(
            temp.path(),
            &["init", "--quiet", "--bare", bare.to_str().unwrap()],
        );
        git_ok(
            temp.path(),
            &[
                "init",
                "--quiet",
                "--initial-branch=main",
                repo.to_str().unwrap(),
            ],
        );
        for worktree in [&repo] {
            git_ok(worktree, &["config", "user.name", "Fence Test"]);
            git_ok(worktree, &["config", "user.email", "fence@example.invalid"]);
            git_ok(
                worktree,
                &["config", "core.hooksPath", quiet_hooks.to_str().unwrap()],
            );
        }
        std::fs::write(
            repo.join(".gitignore"),
            "one\nTWO-BASE\nthree\nfour\nfive\nsix\nseven\nEIGHT-BASE\nnine\n",
        )
        .unwrap();
        git_ok(&repo, &["add", ".gitignore"]);
        git_ok(&repo, &["commit", "--no-verify", "--quiet", "-m", "base"]);
        git_ok(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git_ok(
            &repo,
            &["push", "--quiet", "--set-upstream", "origin", "main"],
        );
        git_ok(
            temp.path(),
            &[
                "clone",
                "--quiet",
                "--branch",
                "main",
                bare.to_str().unwrap(),
                remote_worktree.to_str().unwrap(),
            ],
        );
        git_ok(
            &remote_worktree,
            &["config", "user.name", "Fence Test Remote"],
        );
        git_ok(
            &remote_worktree,
            &["config", "user.email", "remote@example.invalid"],
        );
        git_ok(
            &remote_worktree,
            &["config", "core.hooksPath", quiet_hooks.to_str().unwrap()],
        );
        std::fs::write(
            remote_worktree.join(".gitignore"),
            "one\nTWO-BASE\nthree\nfour\nfive\nsix\nseven\nEIGHT-REMOTE\nnine\n",
        )
        .unwrap();
        git_ok(&remote_worktree, &["add", ".gitignore"]);
        git_ok(
            &remote_worktree,
            &["commit", "--no-verify", "--quiet", "-m", "remote-only"],
        );
        git_ok(&remote_worktree, &["push", "--quiet", "origin", "main"]);

        std::fs::write(
            repo.join(".gitignore"),
            "one\nTWO-LOCAL\nthree\nfour\nfive\nsix\nseven\nEIGHT-BASE\nnine\n",
        )
        .unwrap();
        git_ok(&repo, &["add", ".gitignore"]);
        git_ok(
            &repo,
            &["commit", "--no-verify", "--quiet", "-m", "local-only"],
        );

        let entered = temp.path().join("post-merge-entered");
        let release = temp.path().join("post-merge-release");
        let hook = active_hooks.join("post-merge");
        let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\n: > {}\nwhile [ ! -e {} ]; do sleep 0.01; done\n",
                quote(&entered),
                quote(&release)
            ),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
        git_ok(
            &repo,
            &["config", "core.hooksPath", active_hooks.to_str().unwrap()],
        );

        let task_repo = repo.clone();
        let pull =
            tokio::spawn(
                async move { pull_merge_origin(&task_repo, 20, "test-fenced-pull").await },
            );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !entered.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("native git must reach the post-merge hook");
        let merged = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(merged.contains("TWO-LOCAL") && merged.contains("EIGHT-REMOTE"));
        assert!(
            WriteFence::acquire_until(
                &repo,
                tokio::time::Instant::now() + Duration::from_millis(60)
            )
            .await
            .is_err(),
            "another canonical writer must not enter before Git's final hook closes"
        );
        std::fs::write(&release, b"continue\n").unwrap();
        tokio::time::timeout(Duration::from_secs(5), pull)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let _writer =
            WriteFence::acquire_until(&repo, tokio::time::Instant::now() + Duration::from_secs(1))
                .await
                .unwrap();
        std::fs::write(repo.join(".gitignore"), "cooperative writer bytes\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).unwrap(),
            "cooperative writer bytes\n"
        );
    }

    #[tokio::test]
    async fn fenced_pull_refuses_and_preserves_a_preexisting_merge() {
        let repo = crate::test_helpers::create_test_repo();
        let merge_head = repo.join(".git").join("MERGE_HEAD");
        std::fs::write(&merge_head, "operator-owned merge state\n").unwrap();
        let error = pull_merge_origin(&repo, 1, "test-preexisting-merge")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("merge was already in progress"));
        assert_eq!(
            std::fs::read_to_string(&merge_head).unwrap(),
            "operator-owned merge state\n",
            "the fenced pull must not abort an operator-owned merge"
        );
    }

    #[test]
    fn ref_update_races_require_exact_fresh_convergence_and_never_hide_hook_failures() {
        let tip = "a".repeat(40);
        let other = "b".repeat(40);
        let race = "cannot lock ref 'refs/heads/main': is at tip but expected previous";
        let advertised = format!("{tip}\trefs/heads/main\n");
        assert!(ref_race_is_converged(race, &tip, &tip, &advertised, "main"));
        let passed = format!("{race}\n✅ Pre-push: 4-gate passed for all 3 workspace(s).");
        assert!(ref_race_is_converged(
            &passed,
            &tip,
            &tip,
            &advertised,
            "main"
        ));
        for failure in [
            "4-gate FAILED",
            "Possible plaintext secrets",
            "dracon-warden: refusing push",
        ] {
            assert!(!ref_race_is_converged(
                &format!("{passed}\n{failure}"),
                &tip,
                &tip,
                &advertised,
                "main"
            ));
        }
        assert!(!ref_race_is_converged(
            race,
            &tip,
            &other,
            &advertised,
            "main"
        ));
        assert!(!ref_race_is_converged(
            race,
            &other,
            &other,
            &advertised,
            "main"
        ));
        assert!(!ref_race_is_converged(race, &tip, &tip, "", "main"));
        assert!(!ref_race_is_converged(
            race,
            &tip,
            &tip,
            &advertised,
            "other"
        ));
        assert!(!ref_race_is_converged(
            "permission denied",
            &tip,
            &tip,
            &advertised,
            "main"
        ));
        let hook_failure = format!("{race}\n❌ Pre-push: 4-gate FAILED for:");
        assert!(!ref_race_is_converged(
            &hook_failure,
            &tip,
            &tip,
            &advertised,
            "main"
        ));
    }

    #[tokio::test]
    async fn ref_race_verification_reads_the_actual_remote_and_rejects_unpublished_work() {
        let binary = crate::policy::git_binary();
        let _git = crate::test_helpers::GitBinRestorer::new(binary.to_str().unwrap());
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let remote = temp.path().join("remote.git");
        std::fs::create_dir(&repo).unwrap();
        let git = |cwd: &Path, args: &[&str]| {
            let output = std::process::Command::new(&binary)
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ])
                .args(args)
                .current_dir(cwd)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(temp.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(&repo, &["init", "-b", "main"]);
        std::fs::write(repo.join("file"), "first").unwrap();
        git(&repo, &["add", "--", "file"]);
        git(&repo, &["commit", "-m", "first"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&repo, &["push", "origin", "HEAD:refs/heads/main"]);
        let race = "cannot lock ref 'refs/heads/main': is at tip but expected previous";
        assert!(converged_after_ref_race_for_target(&repo, "origin", race, None).await);
        std::fs::write(repo.join("file"), "second").unwrap();
        git(&repo, &["add", "--", "file"]);
        git(&repo, &["commit", "-m", "second"]);
        assert!(!converged_after_ref_race_for_target(&repo, "origin", race, None).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn daemon_push_paths_honor_pre_push_hooks() {
        use std::os::unix::fs::PermissionsExt;
        let git_bin = crate::policy::git_binary();
        let _git_bin = crate::test_helpers::GitBinRestorer::new(git_bin.to_str().unwrap());
        for route in ["normal", "mirror", "maintenance"] {
            let fixture = tempfile::tempdir().unwrap();
            let repo = fixture.path().join("repo");
            let bare = fixture.path().join("remote.git");
            let hooks = fixture.path().join("hooks");
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::create_dir_all(&hooks).unwrap();
            let git = |cwd: &Path, args: &[&str]| {
                let output = std::process::Command::new(&git_bin)
                    .args([
                        "-c",
                        "core.hooksPath=/dev/null",
                        "-c",
                        "user.name=Audit Fixture",
                        "-c",
                        "user.email=audit-fixture@invalid",
                    ])
                    .args(args)
                    .current_dir(cwd)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                String::from_utf8(output.stdout).unwrap().trim().to_owned()
            };
            git(
                fixture.path(),
                &["init", "--bare", "--quiet", bare.to_str().unwrap()],
            );
            git(&repo, &["init", "--quiet", "-b", "main"]);
            git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
            std::fs::write(repo.join("file"), "harmless fixture").unwrap();
            git(&repo, &["add", "--", "file"]);
            git(&repo, &["commit", "--quiet", "-m", "fixture"]);
            // Set a repository-local hook; the daemon's commands must honor it.
            git(
                &repo,
                &["config", "core.hooksPath", hooks.to_str().unwrap()],
            );
            let hook = hooks.join("pre-push");
            std::fs::write(
                &hook,
                "#!/bin/sh\nprintf '%s' \"${DRACON_ALLOW_REWRITE:-0}\" > hook-ran\nexit 1\n",
            )
            .unwrap();
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
            let result = match route {
                // CHANGED 2026-10-03 (audit R4-SC-13): the sweep is
                // gone; the production entry is push_with_retries
                // (retries=0 → exactly one attempt).
                "normal" => push_with_retries(&repo, 10, 0, "fixture").await,
                "mirror" => {
                    super::super::multi_remote::push_to_named_remote(&repo, "origin", 10, 0, false)
                        .await
                }
                _ => force_push_after_rewrite(&repo, "origin", "main", &None, 10).await,
            };
            assert!(result.is_err(), "{route} bypassed the rejecting hook");
            assert_eq!(
                std::fs::read_to_string(repo.join("hook-ran")).unwrap(),
                if route == "maintenance" { "1" } else { "0" }
            );
            assert!(git(&bare, &["for-each-ref", "--format=%(refname)"]).is_empty());
            // An accepting hook permits the same operation and is still called.
            std::fs::write(&hook, "#!/bin/sh\nprintf 'accepted' > hook-ran\nexit 0\n").unwrap();
            match route {
                // CHANGED 2026-10-03 (audit R4-SC-13): see above.
                "normal" => push_with_retries(&repo, 10, 0, "fixture").await,
                "mirror" => {
                    super::super::multi_remote::push_to_named_remote(&repo, "origin", 10, 0, false)
                        .await
                }
                _ => force_push_after_rewrite(&repo, "origin", "main", &None, 10).await,
            }
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(repo.join("hook-ran")).unwrap(),
                "accepted"
            );
            assert_eq!(
                git(&bare, &["rev-parse", "refs/heads/main"]),
                git(&repo, &["rev-parse", "HEAD"])
            );
        }
    }

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

    /// ADDED 2026-07-21 (v0.112.33, audit M15/F2.6): deleted /
    /// never-created forge repos and lost key access are permanent
    /// (definitionally unfixable by retrying) — the pre-fix code
    /// burned the full retry budget every cycle forever.
    #[test]
    fn test_is_permanent_push_rejection_recognises_repo_gone() {
        assert!(is_permanent_push_rejection(
            "ERROR: Repository not found.\nfatal: Could not read from remote repository."
        ));
        assert!(is_permanent_push_rejection(
            "Forgejo: Push to create is not enabled for users."
        ));
        assert!(is_permanent_push_rejection(
            "remote: The project you were looking for could not be found"
        ));
        assert!(is_permanent_push_rejection(
            "git@github.com: Permission denied (publickey)."
        ));
        assert!(is_permanent_push_rejection("repository does not exist"));
        // Transient errors still NOT permanent.
        assert!(!is_permanent_push_rejection("ssh: Connection refused"));
        assert!(!is_permanent_push_rejection("HTTP 502"));
    }

    #[test]
    fn test_is_push_rejected_still_works() {
        assert!(is_push_rejected(
            "[rejected] main -> main (non-fast-forward)"
        ));
        assert!(!is_push_rejected("connection timed out"));
    }

    /// ADDED 2026-10-03 (audit R3-L04): a fail-fast-eligible rejection
    /// (M1) must return BEFORE the fetch-first auto-pull — the pull is
    /// a network op that cannot fix a policy rejection. Mock git fails
    /// push with a protected-branch message and records every argv.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_permanent_rejection_bypasses_auto_pull() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("argv.log");
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            format!(
                "#!/bin/sh\necho \"$@\" >> \"{}\"\nif [ \"$1\" = \"push\" ]; then\n    echo \"GitLab: You are not allowed to push code to protected branches on this project.\" >&2\n    echo \"pre-receive hook declined\" >&2\n    exit 1\nfi\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_git,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_GIT_BIN",
            fake_git.to_str().unwrap(),
        );
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let result = push_with_retries(&repo, 5, 3, "r3-l04").await;
        assert!(result.is_err(), "protected-branch push must fail");
        let argv = std::fs::read_to_string(&log).unwrap();
        let first_words: Vec<&str> = argv
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        assert!(
            !first_words.contains(&"pull"),
            "auto-pull must not run for a fail-fast rejection: {argv}"
        );
        assert_eq!(
            first_words.iter().filter(|w| **w == "push").count(),
            1,
            "fail fast: exactly one push attempt, no retry/fallback: {argv}"
        );
    }

    /// ADDED 2026-10-08 (audit F111): completes the f8a543a classifier fix
    /// — a LOCAL pre-push hook refusal must fail fast in the origin retry
    /// loop (no auto-pull probe, no retry), exactly like the server-side
    /// permanent set. The message uses the same verbatim strings as the
    /// 2026-10-07 incident (bucket guard + warden secret scan).
    #[cfg(unix)]
    #[tokio::test]
    async fn test_local_hook_rejection_fails_fast_in_origin_retries() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("argv.log");
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            format!(
                "#!/bin/sh\necho \"$@\" >> \"{}\"\nif [ \"$1\" = \"push\" ]; then\n    echo \"pre-push: bucket high-water guard blocked this push\" >&2\n    echo \"⚠️  Possible plaintext secrets detected in push.\" >&2\n    echo \"error: failed to push some refs to 'github.com:DraconDev/dracon-platform.git'\" >&2\n    exit 1\nfi\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_git,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_GIT_BIN",
            fake_git.to_str().unwrap(),
        );
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        // retries=3: pre-fix this ran 4 pushes (SSH + retries) after a
        // pointless pull probe.
        let result = push_with_retries(&repo, 5, 3, "f111").await;
        assert!(result.is_err(), "a local hook refusal must still fail");
        let argv = std::fs::read_to_string(&log).unwrap();
        let first_words: Vec<&str> = argv
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        assert!(
            !first_words.contains(&"pull"),
            "auto-pull must not run for a local hook refusal: {argv}"
        );
        assert_eq!(
            first_words.iter().filter(|w| **w == "push").count(),
            1,
            "fail fast: exactly one push attempt, no retries: {argv}"
        );
    }

    /// ADDED 2026-10-08 (audit F111): same completion for the named-remote
    /// path — the SSH-side early return must fire before the HTTPS fallback,
    /// otherwise every cycle burns an extra timeout_secs on a doomed attempt.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_local_hook_rejection_fails_fast_in_named_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("argv.log");
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            format!(
                "#!/bin/sh\necho \"$@\" >> \"{}\"\nif [ \"$1\" = \"push\" ]; then\n    echo \"⚠️  Possible plaintext secrets detected in push.\" >&2\n    echo \"error: failed to push some refs to 'gitlab.com:DraconDev/web-games-polis.git'\" >&2\n    exit 1\nfi\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_git,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_GIT_BIN",
            fake_git.to_str().unwrap(),
        );
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let result =
            super::super::multi_remote::push_to_named_remote(&repo, "origin", 5, 3, false).await;
        assert!(result.is_err(), "a local hook refusal must still fail");
        let argv = std::fs::read_to_string(&log).unwrap();
        let pushes = argv.lines().filter(|l| l.starts_with("push")).count();
        assert_eq!(
            pushes, 1,
            "fail fast: the HTTPS fallback must not run for a local hook refusal: {argv}"
        );
    }

    #[test]
    fn test_redact_credentials_for_log_covers_all_schemes() {
        // R3-L06: the push-path redactor must not be https-only.
        assert_eq!(
            redact_credentials_for_log("err: http://user:pass@host/x.git failed"),
            "err: http://host/x.git failed"
        );
        assert_eq!(
            redact_credentials_for_log("err: ssh://git:token@gitlab.com/o/r.git denied"),
            "err: ssh://gitlab.com/o/r.git denied"
        );
        assert_eq!(
            redact_credentials_for_log("plain error, no url"),
            "plain error, no url"
        );
    }

    #[test]
    fn test_token_skip_entry_names_forge_without_material() {
        // R3-L05: pin the exact operator-facing skip entry.
        assert_eq!(
            token_skip_entry("gitlab"),
            "gitlab: no token configured (skipped)"
        );
        assert_eq!(
            token_skip_entry("codeberg"),
            "codeberg: no token configured (skipped)"
        );
    }

    /// ADDED 2026-10-03 (audit R4-SC-06): the github HTTPS leg must
    /// carry GH_TOKEN via GIT_ASKPASS like the gitlab/codeberg legs.
    /// Mock git records whether GIT_ASKPASS was set: a configured
    /// token must produce an askpass-backed attempt, while no token
    /// keeps the legacy unauthenticated attempt (ambient `store`
    /// helper rescue) instead of skipping.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_github_https_leg_wires_gh_token_via_askpass() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("askpass.env");
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            format!(
                "#!/bin/sh\necho \"${{GIT_ASKPASS:-unset}}\" >> \"{}\"\nexit 1\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_git,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let _git_guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_GIT_BIN",
            fake_git.to_str().unwrap(),
        );
        // Isolate from the operator's real secrets dir (load_secret
        // falls back to ~/.dracon/.../*.env when the env var is unset).
        let fake_home = tmp.path().join("home");
        std::fs::create_dir_all(&fake_home).unwrap();
        let _home_guard =
            crate::test_helpers::EnvRestorer::new("HOME", fake_home.to_str().unwrap());
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let url = "git@github.com:DraconDev/fixture.git";

        // Case 1: GH_TOKEN set → askpass-backed attempt.
        let _token_guard = crate::test_helpers::EnvRestorer::new("GH_TOKEN", "test-token-123");
        let result = push_https_fallback(&repo, url, "main", 10, "sc06").await;
        assert!(result.is_err(), "mock git always fails");
        drop(_token_guard);
        let seen = std::fs::read_to_string(&log).unwrap();
        assert!(
            seen.lines().any(|l| l.contains("dracon-git-askpass")),
            "token-configured leg must set GIT_ASKPASS, got: {seen:?}"
        );

        // Case 2: no token anywhere → legacy attempt, no askpass, not skipped.
        let _no_token = crate::test_helpers::EnvRestorer::remove("GH_TOKEN");
        std::fs::write(&log, "").unwrap();
        let result = push_https_fallback(&repo, url, "main", 10, "sc06").await;
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("github:"),
            "legacy attempt must record a github failure entry, got: {err}"
        );
        assert!(
            !err.contains("skipped"),
            "github leg must attempt, never skip: {err}"
        );
        let seen = std::fs::read_to_string(&log).unwrap();
        assert_eq!(seen.trim(), "unset", "no-token leg sets no GIT_ASKPASS");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_push_with_retries_total_budget_counts_https_chain() {
        // RESHAPED 2026-10-03 (audit R4-SC-13): retries counts TOTAL
        // pushes (mirror parity). The R3-L02 post-loop sweep (extra SSH
        // + chain after the loop) and its R3-L03 error join are gone, so
        // the old join assertions are replaced by exact push counts.
        // The mock fails every push with a transport error and logs
        // invocations; the github origin makes the HTTPS chain execute
        // its leg. Pre-fix, retries=0 spawned 3 pushes (loop SSH +
        // sweep SSH + chain leg) — now exactly 1.
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let real_git = super::super::real_git_path();
        let log = tmp.path().join("push-log");
        let mock_git = tmp.path().join("git");
        std::fs::write(
            &mock_git,
            format!(
                "#!/bin/sh\necho \"$@\" >> {}\nif [ \"$1\" = \"push\" ]; then\n    echo 'ssh: connect to host github.com port 22: Connection timed out' >&2\n    exit 1\nfi\nexec {} \"$@\"\n",
                log.display(),
                real_git.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&mock_git, PermissionsExt::from_mode(0o755)).unwrap();
        let repo = tmp.path().join("repo");
        for args in [
            vec!["init", "-q", "-b", "main", repo.to_str().unwrap()],
            vec![
                "-C",
                repo.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                "git@github.com:example/repo.git",
            ],
            vec![
                "-C",
                repo.to_str().unwrap(),
                "config",
                "user.email",
                "test@test",
            ],
            vec!["-C", repo.to_str().unwrap(), "config", "user.name", "test"],
        ] {
            assert!(
                std::process::Command::new(&real_git)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success(),
                "setup {args:?} failed"
            );
        }
        std::fs::write(repo.join("f"), "content").unwrap();
        for args in [
            vec!["-C", repo.to_str().unwrap(), "add", "f"],
            vec![
                "-C",
                repo.to_str().unwrap(),
                "commit",
                "--no-verify",
                "-q",
                "-m",
                "init",
            ],
        ] {
            assert!(
                std::process::Command::new(&real_git)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success(),
                "setup {args:?} failed"
            );
        }
        let _guard = crate::test_helpers::GitBinRestorer::new(mock_git.to_str().unwrap());
        let push_count = || {
            std::fs::read_to_string(&log)
                .unwrap()
                .lines()
                .filter(|l| l.split_whitespace().next() == Some("push"))
                .count()
        };
        // retries=0 → exactly 1 push (was 3 with the sweep).
        std::fs::write(&log, "").unwrap();
        let err = push_with_retries(&repo, 5, 0, "sc13")
            .await
            .expect_err("all-transient push must fail");
        assert_eq!(push_count(), 1, "retries=0 must push exactly once");
        assert!(
            !err.to_string().contains("[earlier SSH attempts failed:"),
            "R3-L03 join retired with the sweep: {err}"
        );
        // retries=2 → SSH fail + HTTPS-chain fail, M3-chained.
        std::fs::write(&log, "").unwrap();
        let err = push_with_retries(&repo, 5, 2, "sc13")
            .await
            .expect_err("all-transient push must fail");
        assert_eq!(push_count(), 2, "retries=2 must push exactly twice");
        assert!(
            err.to_string().contains("[SSH attempt failed:"),
            "chain verdict must chain its SSH cause (M3): {err}"
        );
        // retries=3 → SSH, HTTPS, SSH = 3 pushes.
        std::fs::write(&log, "").unwrap();
        push_with_retries(&repo, 5, 3, "sc13")
            .await
            .expect_err("all-transient push must fail");
        assert_eq!(push_count(), 3, "retries=3 must push exactly 3 times");
    }

    /// ADDED 2026-08-09 (v0.113.50): the classifier must map each
    /// failure mode to the operator-actionable cause the alert and
    /// stuck-ledger will show. Divergence (non-fast-forward) is the
    /// headline case from the pi-goal-loop-audit incident.
    #[test]
    fn test_classify_push_failure_maps_every_mode() {
        // Divergence: non-fast-forward rejection (the 2026-08-09
        // pi-goal-loop-audit case).
        let divergence = classify_push_failure(
            "! [rejected] HEAD -> main (non-fast-forward)\nerror: failed to push some refs",
        );
        assert!(
            divergence.contains("history divergence"),
            "got: {}",
            divergence
        );
        // Policy rejection (protected branch).
        let policy_msg = classify_push_failure(
            "remote: error: GH006: Protected branch update failed for main.\n! [remote rejected] main -> main (protected branch hook declined)",
        );
        assert!(
            policy_msg.contains("server-side policy"),
            "got: {}",
            policy_msg
        );
        // Pack too large (github GH001).
        let pack_msg = classify_push_failure("remote: error: GH001: Large files detected.");
        assert!(pack_msg.contains("pack exceeds"), "got: {}", pack_msg);
        // Transport: no rejection markers at all.
        let transport = classify_push_failure("Connection timed out");
        assert!(transport.contains("transport/auth"), "got: {}", transport);
    }

    #[test]
    fn test_transient_forge_outage_gitaly_unavailable() {
        // Live 2026-09-18: GitLab pack receipt failed while the identical
        // commits pushed cleanly to github and to a second GitLab project.
        let msg = "git push failed with status exit status: 128: remote:\nremote: ERROR: The git server, Gitaly, is not available at this time. Please contact your administrator.";
        assert!(is_transient_forge_outage(msg));
        let class = classify_push_failure(msg);
        assert!(class.contains("forge-side outage"), "got: {}", class);
    }

    #[test]
    fn test_transient_forge_outage_gitlab_push_timeout() {
        // Live 2026-09-18: web-games-endless-td gitlab push failed with
        // a server-side timeout during the Gitaly degradation. Retryable
        // infra, not a rule — must not burn the stuck budget.
        let msg = "git push-to-gitlab failed with status exit status: 1: remote: GitLab: Push operation timed out\nerror: failed to push some refs";
        assert!(is_transient_forge_outage(msg));
        assert!(classify_push_failure(msg).contains("forge-side outage"));
        // Bare client-side timeouts still count (local wedge must
        // escalate, not retry silently forever).
        assert!(!is_transient_forge_outage("Connection timed out"));
        assert!(!is_transient_forge_outage("op timed out after 300s"));
    }

    #[test]
    fn test_transient_forge_outage_http_numeric_5xx() {
        // git's HTTP-transport message for forge 5xx (body may be
        // empty — the spelled-out phrases don't cover this form).
        let msg = "git push-to-mirror failed with status exit status: 128: fatal: unable to access 'http://127.0.0.1:18923/x.git/': The requested URL returned error: 503";
        assert!(is_transient_forge_outage(msg));
        assert!(classify_push_failure(msg).contains("forge-side outage"));
        // 4xx stays transport/auth (client error, not forge infra).
        assert!(!is_transient_forge_outage(
            "The requested URL returned error: 403"
        ));
        assert!(!is_transient_forge_outage(
            "The requested URL returned error: 404"
        ));
    }

    #[test]
    fn test_transient_network_outage_dns_resolution() {
        // Live 2026-09-20 02:21: dead DNS failed every repo x every
        // remote with ssh `Could not resolve hostname ...: Temporary
        // failure in name resolution`. Pre-fix this fell through to
        // transport/auth, burned the 5-fail stuck budget on two repos
        // (latched pause + tray storm for a self-healed cause).
        // Fail-before: all three asserts failed (all returned false /
        // transport-auth) before the v0.113.83 predicate existed.
        let ssh_dns = "git push-to-github failed in /home/dracon/.dracon with status exit status: 128: ssh: Could not resolve hostname github.com: Temporary failure in name resolution";
        assert!(is_transient_network_outage(ssh_dns));
        assert!(
            !is_transient_forge_outage(ssh_dns),
            "forge predicate stays precise; DNS is network-class"
        );
        assert!(
            classify_push_failure(ssh_dns).contains("DNS"),
            "got: {}",
            classify_push_failure(ssh_dns)
        );
        // Alternate resolver wordings.
        assert!(is_transient_network_outage(
            "fatal: unable to access 'https://gitlab.com/x.git/': Could not resolve host: gitlab.com"
        ));
        assert!(is_transient_network_outage(
            "ssh: Could not resolve hostname gitlab.com: Name or service not known"
        ));
        // Deliberate exclusions hold: bare client timeouts still count
        // (local wedge must escalate), policy rejections stay permanent.
        assert!(!is_transient_network_outage("Connection timed out"));
        assert!(!is_transient_network_outage(
            "! [remote rejected] HEAD -> main (pre-receive hook declined)"
        ));
    }

    #[test]
    fn test_transient_forge_outage_does_not_swallow_policy() {
        // A real rule decision must stay permanent: retrying it forever
        // instead of pausing for the operator would be the wrong call.
        let hook = "! [remote rejected] HEAD -> main (pre-receive hook declined)";
        assert!(!is_transient_forge_outage(hook));
        assert!(classify_push_failure(hook).contains("server-side policy"));
        let prot = "remote: error: GH006: Protected branch update failed for main.\n! [remote rejected] main -> main (protected branch hook declined)";
        assert!(!is_transient_forge_outage(prot));
        assert!(classify_push_failure(prot).contains("server-side policy"));
    }

    // ADDED 2026-10-07: a LOCAL hook refusal is a policy decision, not a
    // transport failure. Before this, the three messages below all classified
    // as "transport/auth failure (network, timeout, or credentials)", which
    // misdirected the operator and burned the stuck budget on retries that
    // could never succeed. The strings are verbatim from the 2026-10-07
    // four-repo push-stuck incident.
    #[test]
    fn test_local_hook_rejection_is_not_transport_failure() {
        let bucket = "pre-push: bucket high-water guard blocked this push\nerror: failed to push some refs to 'github.com:DraconDev/dracon-platform.git'";
        assert!(is_local_hook_rejection(bucket));
        let class = classify_push_failure(bucket);
        assert!(class.contains("local pre-push hook"), "got: {}", class);
        assert!(
            !class.contains("transport/auth"),
            "a local policy refusal must never be reported as transport: {}",
            class
        );

        let size = "pre-push: bucket high-water/forward-only guard blocked this push\nerror: failed to push some refs to 'gitlab.com:DraconDev/web-games-polis.git'";
        assert!(classify_push_failure(size).contains("local pre-push hook"));

        // The binding is deliberately NOT named for a credential. The warden
        // pre-push hook's quoted-assignment shapes match such a name followed
        // by a string literal, so the obvious name blocks this very push —
        // the same false positive that wedged pi-goal-list-loop-audit on
        // 2026-10-07. Renaming it keeps the test's intent and stops the test
        // from self-blocking. Keep this comment free of any matching shape.
        let scan_msg = "\u{26a0}\u{fe0f}  Possible plaintext secrets detected in push.\nerror: failed to push some refs to 'https://github.com/DraconDev/pi-goal-list-loop-audit.git'";
        assert!(classify_push_failure(scan_msg).contains("local pre-push hook"));

        let warden_history = "\u{274c} dracon-warden: refusing non-fast-forward push to refs/heads/main (history guard).\nerror: failed to push some refs to 'origin'";
        assert!(classify_push_failure(warden_history).contains("local pre-push hook"));
    }

    // ADDED 2026-10-14: a repo blocked by its OWN workspace gate
    // (typecheck/build/test/lint) is a local policy decision. The verbatim
    // strings come from the browser-extensions-shared warden hook
    // (scripts/git-hooks/pre-push, installed via core.hooksPath), which
    // labels its output with a capital `P`. The pre-2026-10-14 predicate was
    // case-sensitive, so this exact push — stuck 22 commits ahead for hours —
    // was reported as "transport/auth failure (network, timeout, or
    // credentials)" and burned the 5-failure stuck budget.
    #[test]
    fn test_local_workspace_gate_refusal_is_not_transport_failure() {
        let gate_msg = "push-to-github failed in /home/dracon/Dev/browser-extensions-shared with status exit status: 1: \u{1f50d} Pre-push: 6 affected workspace(s) — running 4-gate (typecheck, build, test, lint).\n  \u{25b8} extensions/standalones/page-ninja TEST\n\n\u{274c} Pre-push: 4-gate FAILED for:\n  - extensions/standalones/page-ninja/TEST (see /tmp/pre-push-extensions_standalones_page-ninja-TEST.log)";
        assert!(
            is_local_hook_rejection(gate_msg),
            "a 4-gate refusal must be recognized as a local policy decision"
        );
        let class = classify_push_failure(gate_msg);
        assert!(class.contains("local pre-push hook"), "got: {}", class);
        assert!(
            !class.contains("transport/auth"),
            "a workspace-gate refusal must never be reported as transport: {}",
            class
        );

        // Capital-P banner only (no verdict line) still classifies.
        let banner_only =
            "git push failed with status exit status: 1: Pre-push: 3 affected workspace(s)";
        assert!(classify_push_failure(banner_only).contains("local pre-push hook"));
    }

    #[test]
    fn test_local_hook_resource_failure_does_not_prescribe_policy_weakening() {
        let message = "Pre-push: snapshot dependency installation exited 128\nOS can't spawn worker thread: Resource temporarily unavailable (os error 11)\npostinstall terminated by SIGABRT";
        assert!(is_local_hook_rejection(message));
        let class = classify_push_failure(message);
        assert!(class.contains("local pre-push hook"));
        assert!(class.contains("service environment"));
        assert!(!class.contains("fix the policy"));
        assert!(!is_transient_forge_outage(message));
        assert!(!is_transient_network_outage(message));
        assert!(is_transient_forge_outage(
            "remote: Gitaly is temporarily unavailable: OS can't spawn worker thread"
        ));
    }

    #[test]
    fn test_local_hook_rejection_does_not_swallow_server_side_policy() {
        // The server-side arms must keep winning: `pre-receive` does not
        // contain `pre-push`, so the two sets stay disjoint.
        let remote_hook = "! [remote rejected] HEAD -> main (pre-receive hook declined)";
        assert!(!is_local_hook_rejection(remote_hook));
        assert!(classify_push_failure(remote_hook).contains("server-side policy"));

        let prot = "remote: error: GH006: Protected branch update failed for main.\n! [remote rejected] main -> main (protected branch hook declined)";
        assert!(!is_local_hook_rejection(prot));
        assert!(classify_push_failure(prot).contains("server-side policy"));
    }

    #[test]
    fn test_local_hook_rejection_leaves_real_transport_alone() {
        for msg in [
            "Connection timed out",
            "ssh: Could not resolve hostname github.com: Name or service not known",
            "Permission denied (publickey)",
        ] {
            assert!(!is_local_hook_rejection(msg), "wrongly matched: {}", msg);
        }
        assert!(classify_push_failure("Connection timed out").contains("transport/auth"));
    }

    /// ADDED 2026-10-28 (browser-extensions-shared GitLab wedge).
    ///
    /// A PASSING workspace gate must never be read as a local refusal. The
    /// daemon appends captured hook STDOUT to the error, and this repo's gate
    /// prints its banner on success too, so the substring arms matched a
    /// successful gate. The real failure was GitLab closing the SSH session at
    /// its 600s cap while the ~12-minute gate still ran inside that session.
    ///
    /// Misclassification was not cosmetic: it armed the fail-fast arms, so the
    /// daemon never tried the HTTPS transport and never retried on a fresh
    /// connection — the mirror sat 111+ commits ahead.
    #[test]
    fn test_passing_gate_banner_is_not_a_local_hook_refusal() {
        // Verbatim shape from the live failure: success verdict + transport
        // closure in the SAME error text.
        let passed_then_dropped = "git push-to-gitlab failed in /home/dracon/Dev/browser-extensions-shared with status signal: 13 (SIGPIPE): Connection to gitlab.com closed by remote host.\n<hook/stdout diagnostics>\n\u{1f9fe} Pre-push attempt logs: /home/dracon/.local/state/dracon/sync/gates/browser-extensions-shared/dracon-pre-push.pYDVVD\n\u{1f50d} Pre-push: 3 affected workspace(s) at 6e810574d51c4c319096c6e47619cd02ec5f9027 \u{2014} running 4-gate.\n\u{2705} Pre-push: 4-gate passed for all affected workspaces.";
        assert!(
            !is_local_hook_rejection(passed_then_dropped),
            "a passing gate must not be classified as a local hook refusal"
        );
        let class = classify_push_failure(passed_then_dropped);
        assert!(
            !class.contains("local pre-push hook"),
            "a passing gate must not report as a local hook failure: {}",
            class
        );

        // The identical message WITHOUT the pass verdict is still a refusal:
        // a gate that cannot report a verdict must not be retried blindly.
        let banner_only =
            "Pre-push: 3 affected workspace(s) \u{2014} running 4-gate (typecheck, build, test, lint).";
        assert!(is_local_hook_rejection(banner_only));

        // Real refusals must keep winning even beside transport noise.
        let failed = "Pre-push: 4-gate FAILED for:\n  - extensions/standalones/x/TEST\nerror: failed to push some refs to 'gitlab.com:DraconDev/x.git'";
        assert!(is_local_hook_rejection(failed));
        assert!(classify_push_failure(failed).contains("local pre-push hook"));

        // A server-side refusal coexisting with a pass banner stays
        // server-side policy, not a local refusal.
        let passed_then_rejected = format!(
            "{}\n! [remote rejected] main -> main (pre-receive hook declined)",
            passed_then_dropped
        );
        assert!(classify_push_failure(&passed_then_rejected).contains("server-side policy"));
    }

    #[test]
    fn test_local_op_timeout_is_labelled_not_transport_auth() {
        // ADDED 2026-10-22 (dracon-strategy 19h wedge): the daemon's own
        // op-timeout kill used to fall through to the generic
        // "transport/auth failure" arm, sending the operator to the network
        // and the credentials while the mirror was reachable the whole time.
        let msg = "git push-to-gitlab timeout in /home/dracon/Dev/dracon-strategy after 3600s idle (14400s hard cap)";
        assert!(is_local_op_timeout(msg));
        let class = classify_push_failure(msg);
        assert!(
            class.contains("local operation timeout"),
            "op-timeout must not be labelled transport/auth; got: {}",
            class
        );
        assert!(!class.contains("transport/auth"));
    }

    #[test]
    fn test_local_op_timeout_does_not_capture_forge_side_timeout() {
        // The forge-side "Push operation timed out" is a transient forge
        // outage and must keep its own label — the two timeouts are
        // different classes (one is the daemon giving up locally, the other
        // is GitLab giving up on its side).
        let forge = "git push-to-gitlab failed with status exit status: 1: remote: GitLab: Push operation timed out\nerror: failed to push some refs";
        assert!(!is_local_op_timeout(forge));
        assert!(classify_push_failure(forge).contains("forge-side outage"));
    }

    #[test]
    fn test_local_op_timeout_leaves_transport_alone() {
        // A genuine transport failure still reports transport/auth.
        for msg in [
            "Connection timed out",
            "ssh: Could not resolve hostname github.com: Name or service not known",
            "Permission denied (publickey)",
        ] {
            assert!(!is_local_op_timeout(msg), "wrongly matched: {}", msg);
        }
        assert!(classify_push_failure("Connection timed out").contains("transport/auth"));
    }

    #[test]
    fn test_is_pack_too_large_recognises_github_gh001() {
        // github's oversized-pack / large-file rejection.
        let msg = "remote: error: GH001: Large files detected.\nremote: error: File static/assets/music/theme.mp3 is 2500.00 MB; this exceeds GitHub's file size limit.";
        assert!(is_pack_too_large(msg));
    }

    #[test]
    fn test_is_pack_too_large_recognises_pack_exceeds() {
        let msg = "remote: error: pack exceeds the maximum allowed size of 2 GB";
        assert!(is_pack_too_large(msg));
    }

    #[test]
    fn test_is_pack_too_large_case_insensitive() {
        // The matcher lowercases, so an all-caps remote message still matches.
        let msg = "REMOTE ERROR: PACK IS TOO LARGE";
        assert!(is_pack_too_large(msg));
    }

    #[test]
    fn test_is_pack_too_large_ignores_transient_errors() {
        // A non-fast-forward is recoverable, not a size rejection.
        assert!(!is_pack_too_large("non-fast-forward"));
        // A network timeout is transient.
        assert!(!is_pack_too_large("connection timed out"));
        // A protected-branch policy error is permanent but NOT size-related
        // (covered by is_permanent_push_rejection, not is_pack_too_large).
        assert!(!is_pack_too_large("protected branch hook declined"));
    }

    #[test]
    fn test_pull_refspec_for_branch_rejects_unsafe_names() {
        // ADDED 2026-10-03 (audit R4-SC-09): the auto-pull refspec is
        // validated exactly like the push refspecs — the same
        // "unsafe current branch" bail, so an exotic name never
        // reaches `git pull origin <ref>` unvalidated.
        let repo = std::path::Path::new("/tmp/fixture");
        assert_eq!(
            pull_refspec_for_branch(Some("main".to_string()), repo).unwrap(),
            "refs/heads/main"
        );
        assert_eq!(pull_refspec_for_branch(None, repo).unwrap(), "HEAD");
        for bad in ["-evil", "a..b", "trailing.", "li\nne", ""] {
            let err = pull_refspec_for_branch(Some(bad.to_string()), repo).unwrap_err();
            assert!(
                format!("{err:#}").contains("unsafe current branch"),
                "exotic branch {bad:?} must bail, got: {err:#}"
            );
        }
    }
}
