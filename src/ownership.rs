//! Ownership detection for the daemon's auto-commit / auto-push safety guard.
//!
//! The daemon is configured to be very helpful: it auto-commits dirty
//! files and auto-pushes unpushed commits in any repo under the watch
//! roots. That is a footgun if some of those repos are not actually
//! ours — e.g. a `zerostack-reference` clone whose `origin` remote
//! points to `github.com/gi-dellav/...` (someone else's fork), or a
//! `dracon-ai-lib` checkout whose HEAD author is the historical bad
//! `Dracon <dracon@void>` instead of the current `DraconDev
//! <dracsharp@gmail.com>`.
//!
//! This module classifies a repo as one of:
//!
//! - `Owned { reason }` — at least one trusted signal matches. The
//!   daemon is allowed to commit and push.
//! - `Unowned { reason, detail }` — clearly not ours. The daemon
//!   should skip the repo entirely (no commit, no push, no working
//!   tree modification).
//! - `Unknown { detail }` — could not determine (e.g. brand-new repo
//!   with no commits yet, or git invocation failed). The daemon
//!   defaults to skipping this too, because "unknown" is closer to
//!   "unowned" than to "owned" in the safety-first default.
//!
//! The signal checks are config-driven: `policy.trusted_emails`,
//! `policy.trusted_authors`, and `policy.trusted_remote_hosts`. The
//! `RepoPolicyOverride.owned` field can override an individual repo
//! back to Owned when the operator knows better than the heuristic.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Classified ownership state of a repository.
///
/// `reason` is a stable kebab-case string the operator can match on
/// (e.g. `untrusted_origin`, `untrusted_author`, `untrusted_email`,
/// `trusted_email`). `detail` is a human-readable explanation that
/// may include the actual value that didn't match (e.g. the literal
/// `gi-dellav` substring of the bad origin URL).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnershipReport {
    /// The repo is owned by the operator. The daemon may commit and
    /// push. `reason` is one of `trusted_email`, `trusted_author`,
    /// `trusted_origin`, `override`.
    Owned { reason: String },
    /// The repo is clearly not owned. The daemon skips it.
    /// `reason` is one of `untrusted_email`, `untrusted_author`,
    /// `untrusted_origin`, `no_trusted_signals`.
    Unowned { reason: String, detail: String },
    /// Could not determine ownership. Defaults to skip when
    /// `auto_skip_unowned = true`.
    Unknown { detail: String },
}

impl OwnershipReport {
    /// Short human-readable label suitable for the ACTIVITY column.
    /// Format: `<icon> <reason>: <detail>`. Detail is truncated to
    /// 60 chars to keep the table narrow.
    #[allow(dead_code)] // intentional public API for future CLI consumers
    pub fn label(&self) -> String {
        match self {
            OwnershipReport::Owned { reason } => format!("✓ owned ({})", reason),
            OwnershipReport::Unowned { reason, detail } => {
                let trimmed = truncate(detail, 60);
                format!("🚫 unowned: {} ({})", reason, trimmed)
            }
            OwnershipReport::Unknown { detail } => {
                let trimmed = truncate(detail, 60);
                format!("❓ unknown: {}", trimmed)
            }
        }
    }

    /// Hint text for the HINT column.
    #[allow(dead_code)] // intentional public API for future CLI consumers
    pub fn hint(&self) -> &'static str {
        match self {
            OwnershipReport::Owned { .. } => "owned by operator",
            OwnershipReport::Unowned { .. } | OwnershipReport::Unknown { .. } => {
                "repo not owned by operator (run ownership --explain <repo>)"
            }
        }
    }
}

// NOTE: `truncate` helper kept 2026-07-11 (audit
// AUDIT-3-UTILITIES-2026-07-10.md CONCERN #6) because it is the
// only consumer of the (also `#[allow(dead_code)]`) public-API
// `OwnershipReport::label` and `::hint` methods. The whole
// `label`/`hint`/`truncate` cluster is intentionally retained as
// public API surface for future CLI consumers; the methods
// are reachable from tests but not from current production.
#[allow(dead_code)] // paired with `OwnershipReport::label` / `::hint`
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Inputs to the ownership classifier. The daemon constructs one
/// per repo per cycle and caches it in `RepoEntry.ownership`.
#[derive(Debug, Clone)]
pub struct OwnershipInputs {
    /// `git config user.email` for the repo (local `.git/config`).
    pub user_email: Option<String>,
    /// HEAD commit author email.
    pub head_author_email: Option<String>,
    /// HEAD commit author name.
    pub head_author_name: Option<String>,
    /// `git remote get-url origin` (None if no origin).
    pub origin_url: Option<String>,
    /// `RepoPolicyOverride.owned` — explicit override. `Some(true)`
    /// forces Owned, `Some(false)` forces Unowned.
    pub override_owned: Option<bool>,
}

/// Classify a repo as Owned / Unowned / Unknown given the inputs.
///
/// Order of evaluation (first match wins for Unowned):
///
/// 1. `override_owned` — `Some(true)` returns Owned, `Some(false)`
///    returns Unowned with reason `override`.
/// 2. `user_email` not in `trusted_emails` → Unowned
///    `untrusted_email`. (The local git identity is the strongest
///    signal: if the repo was set up with the wrong `user.email`,
///    every new commit would be attributed to the wrong person.)
/// 3. `head_author_email` not in `trusted_emails` AND
///    `head_author_name` not in `trusted_authors` → Unowned
///    `untrusted_author`. (Catches historical bad config like
///    `Dracon <dracon@void>` left in a repo's commit log.)
/// 4. `origin_url` set AND its host/path doesn't match any
///    `trusted_remote_hosts` → Unowned `untrusted_origin`.
///    (Catches repos whose `origin` was redirected to someone
///    else's GitHub/GitLab/Codeberg account — the exact
///    `zerostack-reference` case.)
/// 5. All three signals present and trusted → Owned
///    `trusted_email` (the strongest positive signal).
/// 6. None of the above → Unknown.
pub fn classify(inputs: &OwnershipInputs, trusted: &TrustedSet) -> OwnershipReport {
    // 1. Override
    if let Some(forced) = inputs.override_owned {
        return if forced {
            OwnershipReport::Owned {
                reason: "override".to_string(),
            }
        } else {
            OwnershipReport::Unowned {
                reason: "override".to_string(),
                detail: "RepoPolicyOverride.owned = false".to_string(),
            }
        };
    }

    // Track which signals are available (not None) for the
    // fallback Unknown case.
    let have_user_email = inputs.user_email.is_some();
    let have_head = inputs.head_author_email.is_some()
        || inputs.head_author_name.is_some();
    let have_origin = inputs.origin_url.is_some();

    // 2. user.email (strongest negative signal — local config error)
    // Only flag if the user_email is set AND not in the trusted
    // list. If it's not set (e.g. brand-new repo), defer to the
    // HEAD/origin checks.
    if let Some(ref email) = inputs.user_email {
        if !trusted.emails.iter().any(|e| e == email) {
            return OwnershipReport::Unowned {
                reason: "untrusted_email".to_string(),
                detail: format!("git config user.email = {}", email),
            };
        }
    }

    // 3. HEAD author (catches historical bad config)
    // Only flag if we have a HEAD commit AND both author email
    // and author name are missing/empty/untrusted. If the email
    // is trusted, we accept the repo even if the name is
    // unfamiliar (e.g. "DraconDev (work)" vs "DraconDev").
    if have_head {
        let head_email_trusted = inputs
            .head_author_email
            .as_ref()
            .map(|e| trusted.emails.iter().any(|t| t == e))
            .unwrap_or(false);
        let head_name_trusted = inputs
            .head_author_name
            .as_ref()
            .map(|n| trusted.authors.iter().any(|t| t == n))
            .unwrap_or(false);
        if !head_email_trusted && !head_name_trusted {
            let detail = match (&inputs.head_author_email, &inputs.head_author_name) {
                (Some(e), Some(n)) => format!("HEAD author = {} <{}>", n, e),
                (Some(e), None) => format!("HEAD author email = {}", e),
                (None, Some(n)) => format!("HEAD author name = {}", n),
                (None, None) => "no HEAD author".to_string(),
            };
            return OwnershipReport::Unowned {
                reason: "untrusted_author".to_string(),
                detail,
            };
        }
    }

    // 4. origin URL
    if have_origin {
        let url = inputs.origin_url.as_ref().unwrap();
        if !is_trusted_origin(url, &trusted.remote_hosts) {
            return OwnershipReport::Unowned {
                reason: "untrusted_origin".to_string(),
                detail: format!("origin = {}", url),
            };
        }
    }

    // 5. All available signals are trusted → Owned. Prefer the
    // most specific positive reason.
    if have_user_email {
        return OwnershipReport::Owned {
            reason: "trusted_email".to_string(),
        };
    }
    if have_head {
        return OwnershipReport::Owned {
            reason: "trusted_author".to_string(),
        };
    }
    if have_origin {
        return OwnershipReport::Owned {
            reason: "trusted_origin".to_string(),
        };
    }

    // 6. No signals at all — could be a brand-new repo with no
    // commits and no origin. Unknown defaults to skip in the
    // daemon.
    OwnershipReport::Unknown {
        detail: "no signals available (no user.email, no HEAD, no origin)"
            .to_string(),
    }
}

/// Check whether a remote URL's host (and account path segment) is
/// in the trusted list. The trusted list uses substrings like
/// `github.com/DraconDev` so we match anywhere in the URL.
///
/// Handles both HTTPS (`https://github.com/DraconDev/repo.git`) and
/// SSH (`git@github.com:DraconDev/repo.git`) URL formats by
/// normalizing the SSH form to the HTTPS form before matching.
fn is_trusted_origin(url: &str, trusted_hosts: &[String]) -> bool {
    if trusted_hosts.is_empty() {
        return false;
    }
    // Normalize SSH form `git@host:path` → `host/path` so the same
    // trusted substring works for both.
    let normalized = if let Some(idx) = url.find('@') {
        if let Some(colon_idx) = url[idx..].find(':') {
            let host = &url[idx + 1..idx + colon_idx];
            let path = &url[idx + colon_idx + 1..];
            format!("{}/{}", host, path)
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    };
    trusted_hosts.iter().any(|h| normalized.contains(h))
}

/// Aggregated trust lists built from `SyncPolicy`.
#[derive(Debug, Clone, Default)]
pub struct TrustedSet {
    pub emails: Vec<String>,
    pub authors: Vec<String>,
    pub remote_hosts: Vec<String>,
}

/// Read the signals from a git repo. Each `git` invocation is
/// independent — failures on any one do not block the others.
///
/// Returns `OwnershipInputs` with `None` for signals that could not
/// be read. Callers should treat a fully-empty `OwnershipInputs`
/// result as `Unknown`.
pub fn read_signals(repo: &Path) -> OwnershipInputs {
    OwnershipInputs {
        user_email: git_config_user_email(repo),
        head_author_email: git_head_author_email(repo),
        head_author_name: git_head_author_name(repo),
        origin_url: git_origin_url(repo),
        override_owned: None,
    }
}

fn git_config_user_email(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["config", "--get", "user.email"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn git_head_author_email(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["log", "-1", "--pretty=%ae"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn git_head_author_name(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["log", "-1", "--pretty=%an"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn git_origin_url(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Top-level entry point: read the signals, classify, return
/// OwnershipReport. This is what the daemon calls per repo per
/// cycle.
pub fn detect_ownership(
    repo: &Path,
    trusted: &TrustedSet,
    override_owned: Option<bool>,
) -> OwnershipReport {
    let mut inputs = read_signals(repo);
    inputs.override_owned = override_owned;
    classify(&inputs, trusted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_trusted() -> TrustedSet {
        TrustedSet {
            emails: vec!["dracsharp@gmail.com".to_string()],
            authors: vec!["DraconDev".to_string()],
            remote_hosts: vec![
                "github.com/DraconDev".to_string(),
                "gitlab.com/dracondev".to_string(),
                "codeberg.org/dracondev".to_string(),
            ],
        }
    }

    #[test]
    fn test_classify_trusted_email_matches() {
        let inputs = OwnershipInputs {
            user_email: Some("dracsharp@gmail.com".to_string()),
            head_author_email: Some("dracsharp@gmail.com".to_string()),
            head_author_name: Some("DraconDev".to_string()),
            origin_url: Some("git@github.com:DraconDev/repo.git".to_string()),
            override_owned: None,
        };
        let report = classify(&inputs, &default_trusted());
        assert!(matches!(report, OwnershipReport::Owned { .. }));
        if let OwnershipReport::Owned { reason } = report {
            assert_eq!(reason, "trusted_email");
        }
    }

    #[test]
    fn test_classify_unowned_user_email() {
        let inputs = OwnershipInputs {
            user_email: Some("dracon@void".to_string()),
            head_author_email: Some("dracsharp@gmail.com".to_string()),
            head_author_name: Some("DraconDev".to_string()),
            origin_url: Some("git@github.com:DraconDev/repo.git".to_string()),
            override_owned: None,
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Unowned { reason, detail } => {
                assert_eq!(reason, "untrusted_email");
                assert!(detail.contains("dracon@void"));
            }
            other => panic!("expected Unowned, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_unowned_origin_url() {
        // Covers the zerostack-reference case: origin points to
        // gi-dellav instead of DraconDev.
        let inputs = OwnershipInputs {
            user_email: Some("dracsharp@gmail.com".to_string()),
            head_author_email: Some("dracsharp@gmail.com".to_string()),
            head_author_name: Some("DraconDev".to_string()),
            origin_url: Some("https://github.com/gi-dellav/zerostack.git".to_string()),
            override_owned: None,
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Unowned { reason, detail } => {
                assert_eq!(reason, "untrusted_origin");
                assert!(detail.contains("gi-dellav"));
            }
            other => panic!("expected Unowned, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_unowned_head_author() {
        // Covers the dracon-ai-lib case: HEAD author is the
        // historical "Dracon <dracon@void>" instead of DraconDev.
        let inputs = OwnershipInputs {
            user_email: Some("dracsharp@gmail.com".to_string()),
            head_author_email: Some("dracon@void".to_string()),
            head_author_name: Some("Dracon".to_string()),
            origin_url: Some("git@github.com:DraconDev/dracon-ai-lib.git".to_string()),
            override_owned: None,
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Unowned { reason, detail } => {
                assert_eq!(reason, "untrusted_author");
                assert!(detail.contains("Dracon"));
            }
            other => panic!("expected Unowned, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_per_repo_override_owned() {
        let inputs = OwnershipInputs {
            user_email: Some("dracon@void".to_string()),
            head_author_email: Some("dracon@void".to_string()),
            head_author_name: Some("Dracon".to_string()),
            origin_url: Some("https://github.com/gi-dellav/zerostack.git".to_string()),
            override_owned: Some(true),
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Owned { reason } => assert_eq!(reason, "override"),
            other => panic!("expected Owned, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_per_repo_override_unowned() {
        let inputs = OwnershipInputs {
            user_email: Some("dracsharp@gmail.com".to_string()),
            head_author_email: Some("dracsharp@gmail.com".to_string()),
            head_author_name: Some("DraconDev".to_string()),
            origin_url: Some("git@github.com:DraconDev/repo.git".to_string()),
            override_owned: Some(false),
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Unowned { reason, .. } => assert_eq!(reason, "override"),
            other => panic!("expected Unowned, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_unknown_no_signals() {
        let inputs = OwnershipInputs {
            user_email: None,
            head_author_email: None,
            head_author_name: None,
            origin_url: None,
            override_owned: None,
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Unknown { detail } => {
                assert!(detail.contains("no signals"));
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_trusted_origin_only() {
        // No user.email or HEAD (brand-new repo), but origin is
        // trusted. Falls through to step 5 → Owned.
        let inputs = OwnershipInputs {
            user_email: None,
            head_author_email: None,
            head_author_name: None,
            origin_url: Some("git@github.com:DraconDev/fresh.git".to_string()),
            override_owned: None,
        };
        let report = classify(&inputs, &default_trusted());
        match report {
            OwnershipReport::Owned { reason } => assert_eq!(reason, "trusted_origin"),
            other => panic!("expected Owned, got {:?}", other),
        }
    }

    #[test]
    fn test_is_trusted_origin_substring() {
        let hosts = vec!["github.com/DraconDev".to_string()];
        assert!(is_trusted_origin(
            "https://github.com/DraconDev/repo.git",
            &hosts
        ));
        assert!(is_trusted_origin(
            "git@github.com:DraconDev/repo.git",
            &hosts
        ));
        assert!(!is_trusted_origin(
            "https://github.com/gi-dellav/repo.git",
            &hosts
        ));
    }

    #[test]
    fn test_is_trusted_origin_empty_hosts() {
        // Empty trusted list → nothing is trusted. Forces Unowned.
        let hosts: Vec<String> = vec![];
        assert!(!is_trusted_origin("https://github.com/DraconDev/r.git", &hosts));
    }

    #[test]
    fn test_label_format() {
        let owned = OwnershipReport::Owned {
            reason: "trusted_email".to_string(),
        };
        assert!(owned.label().contains("owned"));
        assert!(owned.label().contains("trusted_email"));

        let unowned = OwnershipReport::Unowned {
            reason: "untrusted_origin".to_string(),
            detail: "origin = https://github.com/gi-dellav/zerostack.git".to_string(),
        };
        assert!(unowned.label().contains("🚫"));
        assert!(unowned.label().contains("untrusted_origin"));
    }
}
