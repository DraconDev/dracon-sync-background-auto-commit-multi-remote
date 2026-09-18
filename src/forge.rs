//! Forge-degraded mode (ADDED 2026-09-18, v0.113.73, full-program
//! P1-4): a cross-repo, cross-restart per-forge outage signal.
//!
//! A single repo failing with a transient-class error (Gitaly/5xx —
//! see `is_transient_forge_outage`) still burns the repo-level stuck
//! budget: one sick repo is indistinguishable from one stuck repo,
//! and escalation to the operator is correct there. But when MULTIPLE
//! repos report transient errors for the SAME forge host inside a
//! short window, the cause is forge-side, not repo-side — hammering
//! every repo's retry loop against a down forge burns budgets
//! fleet-wide and spams per-repo alerts for one underlying incident.
//!
//! This module persists per-host transient hits
//! (`dracon-sync-forge-health.json` in the state dir) so the signal
//! survives daemon restarts (the in-memory per-remote pause counters
//! do not — every restart re-hammers a sick forge 3 times before the
//! pause re-arms; that restart amnesia is what this file fixes for
//! the transient class). Consumers:
//!
//! - `push_background` observes hits for attempted remotes whose
//!   error is transient-class, and stretches the per-remote re-probe
//!   (15 min -> 60 min) for remotes on incident hosts.
//! - the two `record_push_failure` sites shield failures whose
//!   transient hosts are ALL under a declared incident (visibility-
//!   only `record_push_transient_outage` instead of budget burn).
//! - the daemon scan suppresses per-repo Stuck-Retry/Exhausted alerts
//!   for incident-covered repos (one incident alert instead of N)
//!   and polls recovery once per cycle.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Window in which transient hits corroborate an incident (10 min).
pub(crate) const FORGE_INCIDENT_WINDOW_SECS: u64 = 600;
/// Distinct repos that must report transient errors for one host
/// before an incident declares. 2 = corroborated, 1 = anecdote.
pub(crate) const FORGE_INCIDENT_MIN_REPOS: usize = 2;
/// Per-host hit cap (memory/disk bound for the firehose case).
pub(crate) const FORGE_HIT_CAP_PER_HOST: usize = 50;
/// Per-remote re-probe while the remote's host is under a declared
/// incident (1h vs the normal 15 min `MIRROR_PAUSE_REPROBE_SECS`).
pub(crate) const FORGE_INCIDENT_REPROBE_SECS: u64 = 3600;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ForgeHit {
    pub repo: String,
    pub at_unix: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ForgeHostHealth {
    #[serde(default)]
    pub hits: Vec<ForgeHit>,
    /// Edge-latched incident flag: set on declaration (alert once),
    /// cleared by `poll_forge_recovery` (alert once).
    #[serde(default)]
    pub incident: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ForgeHealth {
    #[serde(default)]
    pub hosts: HashMap<String, ForgeHostHealth>,
}

pub(crate) fn forge_health_path() -> PathBuf {
    if let Ok(state_dir) = std::env::var("DRACON_SYNC_STATE_DIR") {
        if !state_dir.is_empty() {
            return PathBuf::from(state_dir).join("dracon-sync-forge-health.json");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("state")
        .join("dracon")
        .join("dracon-sync-forge-health.json")
}

pub(crate) fn load_forge_health() -> ForgeHealth {
    let path = forge_health_path();
    if !path.exists() {
        return ForgeHealth::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

pub(crate) fn save_forge_health(health: &ForgeHealth) {
    let path = forge_health_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(health) {
        let _ = std::fs::write(&path, text);
    }
}

/// Extract the forge host from a push URL, lowercased. Returns None
/// for local paths (no forge involved) and unparseable URLs — those
/// can never corroborate a forge incident.
///
/// Handles `git@host:path`, `ssh://[user@]host[:port]/path`,
/// `https?://[user@]host[:port]/path`, and `ext::...`. Matching is
/// deliberately syntactic (no DNS): `forge-test.invalid` is a host.
pub(crate) fn forge_host_of_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    // ext:: carries a shell command, not a host — check before the
    // scheme test (`ext::ssh ...` has no `://` and would parse as an
    // scp-like `ext` host).
    if url.starts_with("ext::") {
        return None;
    }
    // scp-like syntax: [user@]host:path (but not C:\ windows paths —
    // irrelevant on this daemon's platforms, and a single-letter
    // "host" with a windows drive shape never matches a forge).
    if !url.contains("://") {
        let mut parts = url.splitn(2, ':');
        let left = parts.next().unwrap_or("");
        let right = parts.next().unwrap_or("");
        if right.is_empty() || left.is_empty() || left.contains('/') {
            return None;
        }
        let host = left.rsplit('@').next().unwrap_or(left);
        if host.is_empty() {
            return None;
        }
        return Some(host.to_lowercase());
    }
    // Scheme URLs: strip scheme, optional userinfo, then host[:port].
    let after_scheme = url.split("://").nth(1).unwrap_or("");
    let authority = after_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    // Strip port (but not IPv6 brackets: [::1]:22 -> ::1).
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("").to_string()
    } else if authority.matches(':').count() == 1 {
        authority.split(':').next().unwrap_or("").to_string()
    } else {
        authority.to_string()
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

fn prune_host(host: &mut ForgeHostHealth, now_unix: u64) {
    let cutoff = now_unix.saturating_sub(FORGE_INCIDENT_WINDOW_SECS);
    host.hits.retain(|h| h.at_unix >= cutoff);
}

/// Record a transient-class failure for `host` from `repo` at
/// `now_unix`. Returns true exactly on the none->incident transition
/// (caller alerts once); subsequent hits return false.
pub(crate) fn observe_forge_hit(host: &str, repo: &str, now_unix: u64) -> bool {
    let mut health = load_forge_health();
    let entry = health.hosts.entry(host.to_string()).or_default();
    prune_host(entry, now_unix);
    entry.hits.push(ForgeHit {
        repo: repo.to_string(),
        at_unix: now_unix,
    });
    if entry.hits.len() > FORGE_HIT_CAP_PER_HOST {
        let overflow = entry.hits.len() - FORGE_HIT_CAP_PER_HOST;
        entry.hits.drain(..overflow);
    }
    let distinct: HashSet<&str> = entry.hits.iter().map(|h| h.repo.as_str()).collect();
    let declared = !entry.incident && distinct.len() >= FORGE_INCIDENT_MIN_REPOS;
    if declared {
        entry.incident = true;
    }
    save_forge_health(&health);
    declared
}

/// Currently declared incident hosts (flag-latched; cleared by
/// `poll_forge_recovery` when the window goes quiet).
pub(crate) fn forge_incident_hosts() -> HashSet<String> {
    load_forge_health()
        .hosts
        .into_iter()
        .filter(|(_, h)| h.incident)
        .map(|(host, _)| host)
        .collect()
}

/// Clear incidents whose window has gone quiet. Returns the recovered
/// hosts (caller alerts once each). Runs once per daemon scan; the
/// file read is one small JSON, the write happens only on change.
pub(crate) fn poll_forge_recovery(now_unix: u64) -> Vec<String> {
    let mut health = load_forge_health();
    let mut recovered = Vec::new();
    for (host, entry) in health.hosts.iter_mut() {
        if entry.incident {
            prune_host(entry, now_unix);
            if entry.hits.is_empty() {
                entry.incident = false;
                recovered.push(host.clone());
            }
        }
    }
    if !recovered.is_empty() {
        save_forge_health(&health);
    }
    recovered
}

/// True when `repo`'s most recent transient hit is on an incident
/// host — the per-repo alert coalescing gate. A repo that never hit,
/// or whose latest hit is on a healthy host, is NOT covered (its
/// alerts fire normally).
pub(crate) fn forge_incident_covers_repo(repo: &str) -> bool {
    let health = load_forge_health();
    let mut latest: Option<(&String, u64)> = None;
    for (host, entry) in health.hosts.iter() {
        for hit in entry.hits.iter() {
            if hit.repo == repo && latest.map_or(true, |(_, at)| hit.at_unix >= at) {
                latest = Some((host, hit.at_unix));
            }
        }
    }
    match latest {
        Some((host, _)) => health.hosts.get(host).is_some_and(|h| h.incident),
        None => false,
    }
}

/// Pure shield decision for the `record_push_failure` sites: skip the
/// budget burn only when there is at least one attributed transient
/// host, NO recent non-transient failure (a real failure burns
/// regardless of incidents), and EVERY attributed host is under a
/// declared incident. Single-repo transient (no incident yet) still
/// burns — one sick repo is indistinguishable from one stuck repo.
pub(crate) fn incident_shields_failure(
    transient_hosts: &[String],
    has_recent_non_transient_failure: bool,
    incident_hosts: &HashSet<String>,
) -> bool {
    !transient_hosts.is_empty()
        && !has_recent_non_transient_failure
        && transient_hosts.iter().all(|h| incident_hosts.contains(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forge_host_of_url_matrix() {
        // scp-like
        assert_eq!(
            forge_host_of_url("git@github.com:DraconDev/repo.git"),
            Some("github.com".to_string())
        );
        assert_eq!(
            forge_host_of_url("git@gitlab.com:DraconDev/doomtap.git"),
            Some("gitlab.com".to_string())
        );
        // case normalization
        assert_eq!(
            forge_host_of_url("git@GitLab.COM:x/y.git"),
            Some("gitlab.com".to_string())
        );
        // scheme URLs with userinfo + port
        assert_eq!(
            forge_host_of_url("ssh://git@gitlab.com:22/DraconDev/x.git"),
            Some("gitlab.com".to_string())
        );
        assert_eq!(
            forge_host_of_url("ssh://127.0.0.1:9/unpushable.git"),
            Some("127.0.0.1".to_string())
        );
        assert_eq!(
            forge_host_of_url("https://user@github.com/org/repo.git"),
            Some("github.com".to_string())
        );
        assert_eq!(
            forge_host_of_url("https://codeberg.org/a/b"),
            Some("codeberg.org".to_string())
        );
        // synthetic test forges are hosts too
        assert_eq!(
            forge_host_of_url("ssh://git@forge-test.invalid:22/r.git"),
            Some("forge-test.invalid".to_string())
        );
        // local paths are NOT forges
        assert_eq!(forge_host_of_url("/tmp/scope-demo/repos/mirror.git"), None);
        assert_eq!(forge_host_of_url("./relative/path.git"), None);
        assert_eq!(forge_host_of_url("file:///srv/git/repo.git"), None);
        // ext:: carries a command, not a host
        assert_eq!(forge_host_of_url("ext::ssh -i key git@h x"), None);
        // garbage
        assert_eq!(forge_host_of_url(""), None);
        assert_eq!(forge_host_of_url("://"), None);
    }

    #[test]
    fn test_forge_incident_declares_on_two_repos_recovers_on_quiet() {
        let state_dir = tempfile::tempdir().unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let now = 1_800_000_000u64;
        // One repo is an anecdote, not an incident.
        assert!(!observe_forge_hit("gitlab.com", "/r/a", now));
        assert!(forge_incident_hosts().is_empty());
        // Second distinct repo declares.
        assert!(observe_forge_hit("gitlab.com", "/r/b", now + 10));
        assert!(forge_incident_hosts().contains("gitlab.com"));
        // Third hit: already declared, no second alert.
        assert!(!observe_forge_hit("gitlab.com", "/r/a", now + 20));
        // Same repo hammering never declares on its own.
        assert!(!observe_forge_hit("github.com", "/r/c", now));
        assert!(!observe_forge_hit("github.com", "/r/c", now + 30));
        assert!(!forge_incident_hosts().contains("github.com"));
        // Window expiry recovers (alert once).
        let recovered = poll_forge_recovery(now + FORGE_INCIDENT_WINDOW_SECS + 60);
        assert_eq!(recovered, vec!["gitlab.com".to_string()]);
        assert!(forge_incident_hosts().is_empty());
        // Recovery is idempotent: no second alert.
        assert!(poll_forge_recovery(now + FORGE_INCIDENT_WINDOW_SECS + 120).is_empty());
    }

    #[test]
    fn test_forge_incident_covers_repo_latest_hit_wins() {
        let state_dir = tempfile::tempdir().unwrap();
        let _guard = crate::test_helpers::EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            state_dir.path().to_string_lossy().as_ref(),
        );
        let now = 1_800_000_000u64;
        // Declare an incident on gitlab.com via two repos.
        observe_forge_hit("gitlab.com", "/r/a", now);
        observe_forge_hit("gitlab.com", "/r/b", now + 5);
        assert!(forge_incident_covers_repo("/r/a"));
        assert!(!forge_incident_covers_repo("/r/never-seen"));
        // A LATER hit on a healthy host moves coverage away: the
        // repo's current problem is not the incident.
        observe_forge_hit("github.com", "/r/a", now + 10);
        assert!(!forge_incident_covers_repo("/r/a"));
    }

    #[test]
    fn test_incident_shields_failure_matrix() {
        let mut incidents = HashSet::new();
        incidents.insert("gitlab.com".to_string());
        // Shielded: only transient hosts, all under incident.
        assert!(incident_shields_failure(
            &["gitlab.com".to_string()],
            false,
            &incidents
        ));
        // Not shielded: a real failure alongside (burns regardless).
        assert!(!incident_shields_failure(
            &["gitlab.com".to_string()],
            true,
            &incidents
        ));
        // Not shielded: unattributed (empty) — single anecdote.
        assert!(!incident_shields_failure(&[], false, &incidents));
        // Not shielded: transient on a healthy host.
        assert!(!incident_shields_failure(
            &["github.com".to_string()],
            false,
            &incidents
        ));
        // Not shielded: mixed hosts, one not under incident.
        assert!(!incident_shields_failure(
            &["gitlab.com".to_string(), "github.com".to_string()],
            false,
            &incidents
        ));
    }
}
