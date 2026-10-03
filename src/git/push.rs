//! Push operations — HTTPS fallback, transport fallbacks, retry logic.

use anyhow::Result;
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

/// Push with SSH first, then try HTTPS fallbacks.
pub(crate) async fn push_with_transport_fallbacks(
    repo: &Path,
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    let ssh_hardening = crate::git::git_ssh_hardening();
    // CHANGED 2026-07-02 (goal `354fe3cb`):
    // When the worktree is detached, `git push origin HEAD` fails with
    // "The destination you provided is not a full refname" because HEAD
    // is a SHA, not a ref. Build a fully-qualified refspec instead.
    // This is the case for nested-on-main architectures where the
    // nested submodule path is watched while still detached at the
    // parent's gitlink SHA (during migration windows).
    //
    // CHANGED 2026-08-09 (v0.113.48, pi-goal-loop-audit incident):
    // always use the fully-qualified `HEAD:refs/heads/<branch>` form
    // when a branch is known. Bare `HEAD` is interpreted as a commit
    // SHA by git when HEAD is detached, even if `current_branch()`
    // returned `Some(branch)` (worktree-state race: HEAD-file cached
    // while the worktree is mid-detach). The fully-qualified form is
    // safe in both attached and detached HEADs — git pushes the commit
    // pointed at by HEAD to `refs/heads/<branch>`. The detached
    // fallback to `main` is preserved as a last resort.
    let ssh_refspec = match crate::git::branch::current_branch(repo) {
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
    };
    match super::run_git_with_timeout_env_progress(
        repo,
        &["push", "origin", &ssh_refspec],
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
            // Server-side policy errors AND oversized-pack errors cannot be
            // fixed by retries. Return immediately so the caller logs one
            // incident per cycle instead of burning the retry budget.
            if is_permanent_push_rejection(&err_msg) || is_pack_too_large(&err_msg) {
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
            // FIX (audit M3, 2026-10-02): chain the original SSH error
            // into the fallback failure so the ledger records the cause,
            // not just the symptom. Classifier-safe: matching is
            // substring-based, so appended context only adds signal.
            push_https_fallback(repo, &origin, &refspec, timeout_secs, op_label)
                .await
                .map_err(|fallback_err| {
                    anyhow::anyhow!(
                        "{} [SSH attempt failed: {}]",
                        fallback_err,
                        clip_error_detail(&redact_credentials_for_log(&err_msg))
                    )
                })
        }
    }
}

/// Push with retries (SSH) and then HTTPS fallback.
///
/// `retries` counts the SSH retry-loop attempts (min 1), unified with
/// `push_to_named_remote` (audit L5). CORRECTED 2026-10-03 (audit
/// R3-L02): the old claim of "TOTAL push attempts" was wrong — after
/// the loop exhausts, one EXTRA recovery attempt runs via
/// `push_with_transport_fallbacks` (a fresh SSH push plus the full
/// per-forge HTTPS chain), so retries=0 can spawn up to 5 pushes.
/// That extra attempt is deliberate (a final transport-fallback
/// sweep before giving up); only the budget claim was fixed.
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
        // CHANGED 2026-07-02 (goal `354fe3cb`):
        // When the worktree is detached, `git push origin HEAD` fails.
        // Build a fully-qualified refspec instead.
        //
        // CHANGED 2026-08-09 (v0.113.48): see `push_with_transport_fallbacks`
        // — always use the fully-qualified `HEAD:refs/heads/<branch>` form
        // when a branch is known. Bare `HEAD` fails with the same refspec
        // error on a detached worktree.
        let ssh_refspec = match crate::git::branch::current_branch(repo) {
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
                // Server-side policy errors (protected branch, hook declined,
                // etc.) AND oversized-pack errors cannot be fixed by retries,
                // pull, or HTTPS fallback. Return immediately so the caller
                // logs one incident per cycle instead of burning the retry
                // budget.
                if is_permanent_push_rejection(&err_msg) || is_pack_too_large(&err_msg) {
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
                    let pull_refspec = pull_refspec_for_branch(
                        crate::git::branch::current_branch(repo),
                        repo,
                    )?;
                    eprintln!(
                        "🔄 push rejected (non-fast-forward) for {} — pulling origin {} and retrying",
                        repo.display(),
                        pull_refspec
                    );
                    let pull_result = super::run_git_with_timeout_env_progress(
                        repo,
                        &["pull", "--no-rebase", "--no-edit", "origin", &pull_refspec],
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
                                "⚠️ auto-pull failed for {}: {} — aborting any partial merge, continuing with retry",
                                repo.display(),
                                redact_credentials_for_log(&pull_err.to_string())
                            );
                            // Best-effort: don't leave the repo in
                            // MERGING state for the next sync cycle to
                            // trip over. No-op when no merge is open.
                            let _ = super::run_git_with_timeout(
                                repo,
                                &["merge", "--abort"],
                                15,
                                "auto-pull-abort",
                            )
                            .await;
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
    // FIXED 2026-10-03 (audit R3-L03): the old `if let Ok(())` threw
    // away the fallback error (per-forge HTTPS verdicts + fresh SSH
    // cause) and returned the stale loop `last_err`. Join both with
    // the fallback verdict primary so the ledger shows the final
    // verdicts, never just an earlier SSH attempt.
    match push_with_transport_fallbacks(repo, timeout_secs, op_label).await {
        Ok(()) => Ok(()),
        Err(fallback_err) => Err(match last_err {
            Some(prev) => anyhow::anyhow!(
                "{} [earlier SSH attempts failed: {}]",
                fallback_err,
                clip_error_detail(&redact_credentials_for_log(&prev.to_string()))
            ),
            None => fallback_err,
        }),
    }
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
    } else if is_permanent_push_rejection(err_msg) {
        "server-side policy rejection (protected branch / hook declined / missing repo / lost key)"
    } else if is_push_rejected(err_msg) {
        "history divergence (non-fast-forward: remote has commits not on local; needs operator reconciliation)"
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
mod tests {
    use super::*;

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
                "normal" => push_with_transport_fallbacks(&repo, 10, "fixture").await,
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
                "normal" => push_with_transport_fallbacks(&repo, 10, "fixture").await,
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
    async fn test_push_with_retries_joins_fallback_and_loop_errors() {
        // R3-L03: when the SSH loop AND the transport fallback both
        // fail, the returned error must carry the fallback verdict
        // (primary) joined with the earlier loop error — never the
        // stale loop error alone.
        let tmp = tempfile::tempdir().unwrap();
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            "#!/bin/sh\nif [ \"$1\" = \"push\" ]; then\n    echo \"ssh: connect to host gitlab.com port 22: Connection timed out\" >&2\n    echo \"fatal: Could not read from remote repository.\" >&2\n    exit 1\nfi\nexit 0\n",
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
        // retries=0 → attempts=1: one SSH failure, then the fallback
        // (fresh SSH fails too; origin lookup is empty so the HTTPS
        // chain reports the generic summary chained with the SSH cause).
        let err = push_with_retries(&repo, 5, 0, "r3-l03")
            .await
            .expect_err("all-transient push must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("[SSH attempt failed:"),
            "fallback verdict must chain its SSH cause (M3): {msg}"
        );
        assert!(
            msg.contains("[earlier SSH attempts failed:"),
            "loop error must be joined, fallback primary (R3-L03): {msg}"
        );
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
        assert_eq!(
            pull_refspec_for_branch(None, repo).unwrap(),
            "HEAD"
        );
        for bad in ["-evil", "a..b", "trailing.", "li\nne", ""] {
            let err = pull_refspec_for_branch(Some(bad.to_string()), repo).unwrap_err();
            assert!(
                format!("{err:#}").contains("unsafe current branch"),
                "exotic branch {bad:?} must bail, got: {err:#}"
            );
        }
    }
}
