//! Stuck-push routing: file a ledger finding in the owning loop's repo.
//!
//! Background (goal 20261009193116-1ess3v, Phase 3, 2026-10-09): a push-stuck
//! repo sat on the board until a human pasted the status table into chat.
//! The daemon already knows the repo is stuck and (from the push stderr) WHY.
//! When the cause is a repo-policy refusal — a hook/guard decision a loop can
//! act on, not transient network — the daemon appends one NOTE row to the
//! owning loop's findings ledger (`<repo>/.pi-glla/audit-loop/findings.md`),
//! carrying the guard's violation output. Transient network/forge outages
//! self-heal and are never routed.
//!
//! Discipline: append-only single-line rows (the ledger's own convention),
//! one row per stuck episode (keyed by HEAD tip), minimum stuck age before
//! the first filing, repos without a ledger file are skipped silently.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Minimum stuck age before the first routing (seconds).
pub(crate) const STUCK_ROUTE_MIN_AGE_SECS: u64 = 1800;

/// Ledger path relative to a loop-owned repo root.
const LEDGER_RELATIVE: &str = ".pi-glla/audit-loop/findings.md";

/// A stuck-push episode worth routing to the owning loop, or `None` when
/// this failure class must stay daemon-internal (transient network/forge).
pub(crate) struct StuckRouteRequest {
    /// Repo root the finding is filed into.
    pub repo: PathBuf,
    /// Current HEAD tip: the episode key (new commits = new episode).
    pub tip: String,
    /// Consecutive push failures at filing time.
    pub consecutive_failures: u32,
    /// Seconds since `stuck_since`.
    pub stuck_age_secs: u64,
    /// Raw stderr of the failed push (may embed guard JSON).
    pub last_error: String,
}

/// Extract a compact cause from push stderr: prefer the guard's structured
/// `code` + `errors` when the output embeds its JSON, else the first two
/// non-empty lines. Always single-line, bounded.
pub(crate) fn extract_route_cause(last_error: &str) -> String {
    if let Some(json) = extract_embedded_json(last_error) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            let code = value
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("GUARD");
            let mut errors: Vec<String> = value
                .get("errors")
                .and_then(|e| e.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if errors.is_empty() {
                if let Some(message) = value.get("message").and_then(|m| m.as_str()) {
                    errors.push(message.to_owned());
                }
            }
            errors.truncate(4);
            if !errors.is_empty() {
                return truncate_single_line(&format!("{}: {}", code, errors.join("; ")), 500);
            }
        }
    }
    let lines: Vec<String> = last_error
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(2)
        .map(str::to_owned)
        .collect();
    if lines.is_empty() {
        return "push failure cause unavailable".to_owned();
    }
    truncate_single_line(&lines.join(" / "), 500)
}

/// Find the first `{...}` JSON object embedded in hook stderr output.
fn extract_embedded_json(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + ch.len_utf8()].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

fn truncate_single_line(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut out: String = flat.chars().take(max_chars).collect();
    if flat.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// Returns `true` when this failure class is loop-actionable (a policy/hook
/// decision or a divergence/auth state a human must resolve) as opposed to
/// transient network/forge weather that clears on its own.
pub(crate) fn is_routeable_cause(last_error: &str) -> bool {
    use crate::git::{
        is_local_hook_rejection, is_permanent_push_rejection, is_push_rejected,
        is_transient_forge_outage, is_transient_network_outage,
    };
    if last_error.trim().is_empty() {
        return false;
    }
    if is_transient_network_outage(last_error) || is_transient_forge_outage(last_error) {
        return false;
    }
    is_local_hook_rejection(last_error)
        || is_permanent_push_rejection(last_error)
        || is_push_rejected(last_error)
}

/// File the finding. Returns `Ok(true)` when a row was appended.
pub(crate) fn maybe_route_stuck_push(request: &StuckRouteRequest) -> Result<bool> {
    if request.stuck_age_secs < STUCK_ROUTE_MIN_AGE_SECS {
        return Ok(false);
    }
    if !is_routeable_cause(&request.last_error) {
        return Ok(false);
    }
    let Some(ledger) = filing_gate(&request.repo, &request.tip)? else {
        return Ok(false);
    };
    let cause = extract_route_cause(&request.last_error);
    file_finding(
        &ledger,
        &request.repo,
        &request.tip,
        request.consecutive_failures,
        request.stuck_age_secs,
        &cause,
    )
}

/// Preferred entry point for the daemon: re-derive the CURRENT guard verdict
/// for bucket-managed repos instead of trusting the stuck-ledger summary,
/// which may record a stale or misclassified failure (observed live: hook
/// refusals summarized as "transport/auth failure"). A live violation is
/// always current; a clean range means the failure is genuinely non-guard
/// and nothing is filed. Non-bucket repos fall back to the summary path.
/// `timeout_secs` bounds the guard re-run; `env_override` mirrors the
/// `DRACON_BUCKET_GUARD` override.
pub(crate) async fn maybe_route_with_rederive(
    repo: &Path,
    tip: &str,
    consecutive_failures: u32,
    stuck_age_secs: u64,
    last_error: &str,
    timeout_secs: u64,
    env_override: Option<&str>,
) -> Result<bool> {
    if stuck_age_secs < STUCK_ROUTE_MIN_AGE_SECS {
        return Ok(false);
    }
    let Some(ledger) = filing_gate(repo, tip)? else {
        return Ok(false);
    };
    match crate::bucket_guard::check_push_range(repo, timeout_secs, env_override).await {
        Ok(Some(verdict)) if !verdict.ok => {
            let mut errors = verdict.errors;
            errors.truncate(4);
            let cause = if errors.is_empty() {
                verdict.code.clone()
            } else {
                format!("{}: {}", verdict.code, errors.join("; "))
            };
            match file_finding(
                &ledger,
                repo,
                tip,
                consecutive_failures,
                stuck_age_secs,
                &truncate_single_line(&cause, 500),
            ) {
                Ok(filed) => Ok(filed),
                Err(_) => Ok(false),
            }
        }
        Ok(_) => Ok(false),
        Err(_) => {
            // Guard re-run failed (infra): fall back to the summary path
            // rather than dropping a possibly-routeable refusal.
            let request = StuckRouteRequest {
                repo: repo.to_path_buf(),
                tip: tip.to_owned(),
                consecutive_failures,
                stuck_age_secs,
                last_error: last_error.to_owned(),
            };
            maybe_route_stuck_push(&request)
        }
    }
}

/// Shared preconditions: the repo carries a loop ledger and this tip has no
/// finding yet. Returns the ledger path when fileable.
fn filing_gate(repo: &Path, tip: &str) -> Result<Option<PathBuf>> {
    let ledger = repo.join(LEDGER_RELATIVE);
    if !ledger.is_file() {
        return Ok(None);
    }
    let short_tip: String = tip.chars().take(7).collect();
    let marker = format!("[STUCK-PUSH tip:{short_tip}]");
    let existing = std::fs::read_to_string(&ledger).unwrap_or_default();
    if existing.contains(&marker) {
        return Ok(None);
    }
    Ok(Some(ledger))
}

/// Append one NOTE row. The tip-dedup gate must have passed already.
fn file_finding(
    ledger: &Path,
    repo: &Path,
    tip: &str,
    consecutive_failures: u32,
    stuck_age_secs: u64,
    cause: &str,
) -> Result<bool> {
    let short_tip: String = tip.chars().take(7).collect();
    let marker = format!("[STUCK-PUSH tip:{short_tip}]");
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo.display().to_string());
    let row = format!(
        "- [ ] NOTE: {} {} push blocked ({} consecutive failures over {}m): {}. Fix the cause, then run `dracon-sync repair stuck-unstuck {}`.\n",
        marker, name, consecutive_failures,
        stuck_age_secs / 60,
        cause, name,
    );
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(ledger)?;
    file.write_all(row.as_bytes())?;
    Ok(true)
}

/// Current HEAD tip of `repo`, for episode keying. `None` when unresolvable
/// (caller skips routing rather than filing under a wrong key).
pub(crate) fn head_tip(repo: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_repo(with_ledger: bool) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        if with_ledger {
            let dir = tmp.path().join(".pi-glla/audit-loop");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("findings.md"), "# Findings\n").unwrap();
        }
        tmp
    }

    fn request(repo: &Path, tip: &str, age: u64, error: &str) -> StuckRouteRequest {
        StuckRouteRequest {
            repo: repo.to_path_buf(),
            tip: tip.to_owned(),
            consecutive_failures: 5,
            stuck_age_secs: age,
            last_error: error.to_owned(),
        }
    }

    const HOOK_ERROR: &str = "error: failed to push some refs\npre-push: bucket high-water/forward-only guard blocked this push";

    #[test]
    fn routes_hook_refusal_with_guard_json_cause() {
        let tmp = fixture_repo(true);
        let error = format!(
            "{{\"ok\": false, \"code\": \"BUCKET_STRATEGY_GUARD_FORWARD_ONLY\", \"errors\": [\"commit: static/a.png\"]}}\n{HOOK_ERROR}"
        );
        let req = request(tmp.path(), "abc1234def", 3600, &error);
        assert!(maybe_route_stuck_push(&req).unwrap());
        let content = std::fs::read_to_string(tmp.path().join(LEDGER_RELATIVE)).unwrap();
        assert!(content.contains("[STUCK-PUSH tip:abc1234]"), "{content}");
        assert!(
            content.contains("BUCKET_STRATEGY_GUARD_FORWARD_ONLY"),
            "{content}"
        );
        assert!(content.contains("static/a.png"), "{content}");
    }

    #[test]
    fn dedups_same_tip_episode() {
        let tmp = fixture_repo(true);
        let req = request(tmp.path(), "abc1234def", 3600, HOOK_ERROR);
        assert!(maybe_route_stuck_push(&req).unwrap());
        assert!(!maybe_route_stuck_push(&req).unwrap());
        let content = std::fs::read_to_string(tmp.path().join(LEDGER_RELATIVE)).unwrap();
        assert_eq!(content.matches("[STUCK-PUSH").count(), 1);
    }

    #[test]
    fn new_tip_is_a_new_episode() {
        let tmp = fixture_repo(true);
        assert!(maybe_route_stuck_push(&request(tmp.path(), "aaa0000", 3600, HOOK_ERROR)).unwrap());
        assert!(maybe_route_stuck_push(&request(tmp.path(), "bbb1111", 3600, HOOK_ERROR)).unwrap());
    }

    #[test]
    fn skips_young_stuck() {
        let tmp = fixture_repo(true);
        assert!(!maybe_route_stuck_push(&request(tmp.path(), "aaa0000", 60, HOOK_ERROR)).unwrap());
    }

    #[test]
    fn skips_repo_without_ledger() {
        let tmp = fixture_repo(false);
        assert!(
            !maybe_route_stuck_push(&request(tmp.path(), "aaa0000", 3600, HOOK_ERROR)).unwrap()
        );
    }

    #[test]
    fn skips_transient_network() {
        let tmp = fixture_repo(true);
        let error =
            "ssh: Could not resolve hostname gitlab.com: Temporary failure in name resolution";
        assert!(!maybe_route_stuck_push(&request(tmp.path(), "aaa0000", 7200, error)).unwrap());
    }

    #[tokio::test]
    async fn rederive_files_despite_misclassified_summary() {
        // Live observation (hellhunter/deathrun 2026-10-09): the stuck
        // summary can read "transport/auth failure" while the live guard
        // refuses. The re-derive path must file on fresh guard truth, not
        // the stale summary.
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("proj");
        let script_dir = proj.join("web/scripts");
        std::fs::create_dir_all(&script_dir).unwrap();
        let script = script_dir.join("bucket-strategy-guard.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"ok\": false, \"code\": \"BUCKET_STRATEGY_GUARD_FORWARD_ONLY\", \"errors\": [\"commit: static/a.png\"]}'\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let repo = proj.join("repo");
        let upstream = proj.join("upstream.git");
        let git = |args: &[&str], cwd: Option<&std::path::Path>| {
            let mut cmd = std::process::Command::new("git");
            cmd.args(args);
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            assert!(cmd.status().unwrap().success());
        };
        git(&["init", "-q", "--bare", upstream.to_str().unwrap()], None);
        git(
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                repo.to_str().unwrap(),
            ],
            None,
        );
        git(&["checkout", "-qb", "main"], Some(&repo));
        git(&["config", "user.email", "t@t"], Some(&repo));
        git(&["config", "user.name", "t"], Some(&repo));
        std::fs::write(repo.join("f"), "x").unwrap();
        git(&["add", "."], Some(&repo));
        git(&["commit", "-qm", "init"], Some(&repo));
        git(&["push", "-q", "-u", "origin", "main"], Some(&repo));
        let ledger_dir = repo.join(".pi-glla/audit-loop");
        std::fs::create_dir_all(&ledger_dir).unwrap();
        std::fs::write(ledger_dir.join("findings.md"), "# Findings\n").unwrap();
        let tip = super::head_tip(&repo).expect("tip");
        let misclassified = "git push returned non-zero (remotes: gitlab, origin) — transport/auth failure (network, timeout, or credentials)";
        let filed = maybe_route_with_rederive(&repo, &tip, 5, 3600, misclassified, 10, None)
            .await
            .unwrap();
        assert!(filed);
        let content = std::fs::read_to_string(repo.join(super::LEDGER_RELATIVE)).unwrap();
        assert!(
            content.contains("BUCKET_STRATEGY_GUARD_FORWARD_ONLY"),
            "{content}"
        );
        assert!(content.contains("static/a.png"), "{content}");
    }

    #[test]
    fn cause_falls_back_to_first_lines() {
        let cause = extract_route_cause(
            "error: failed to push some refs to 'x'\nerror: src refspec main does not match\n",
        );
        assert!(cause.contains("failed to push"), "{cause}");
        assert!(!cause.contains('\n'), "{cause}");
    }
}
