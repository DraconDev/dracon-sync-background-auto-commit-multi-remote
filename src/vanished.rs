//! Watched-repo-vanished ledger (disappearance doc G2, added 2026-08-21).
//!
//! The daemon discovers repositories from disk on every cycle, so a
//! deleted watch path simply stops appearing: nothing remembers it, no
//! concern is raised, and the loss is invisible until an operator notices
//! missing commit streams. That is exactly how all three utility checkouts
//! stayed gone for two days (see
//! `docs/design/utilities-checkout-disappearance-2026-08-21.md`, gap G2).
//!
//! This module persists a ledger of every repo path the daemon has ever
//! synced. When a previously-seen path disappears from discovery, the
//! entry records when it was first seen missing; `run_repair_concerns`
//! then surfaces it as a CONCERN and the daemon logs it once. An entry
//! clears automatically when the path exists again at discovery time.
//!
//! The ledger is bookkeeping only — it never gates syncing or repair.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Ledger file name; lives next to the policy file (same pattern as
/// `repos-size-cache.json`).
pub(crate) const SEEN_LEDGER_FILE: &str = "repos-seen-ledger.json";

/// Auto-expire vanished entries after 90 days: a repo intentionally
/// deleted by the operator must not nag forever. Re-cloning or restoring
/// the path clears the entry immediately instead.
pub(crate) const VANISHED_ENTRY_TTL_SECS: u64 = 90 * 24 * 3600;

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub(crate) struct SeenRepo {
    /// Last discovery cycle in which this path existed.
    pub(crate) last_seen_secs: u64,
    /// First discovery cycle in which the path was absent. `None` while
    /// the path still exists.
    #[serde(default)]
    pub(crate) first_vanished_secs: Option<u64>,
}

pub(crate) type SeenLedger = HashMap<String, SeenRepo>;

/// A repo that was previously synced but whose path no longer exists.
#[derive(PartialEq, Eq, Clone, Debug)]
pub(crate) struct VanishedRepo {
    pub(crate) path: String,
    pub(crate) last_seen_secs: u64,
    pub(crate) first_vanished_secs: u64,
}

pub(crate) fn seen_ledger_path(policy_path: &Path) -> PathBuf {
    policy_path
        .parent()
        .map(|p| p.join(SEEN_LEDGER_FILE))
        .unwrap_or_else(|| PathBuf::from(SEEN_LEDGER_FILE))
}

pub(crate) fn load_seen_ledger(path: &Path) -> SeenLedger {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => SeenLedger::new(),
    }
}

pub(crate) fn save_seen_ledger(path: &Path, ledger: &SeenLedger) {
    if let Ok(s) = serde_json::to_string(ledger) {
        // Best-effort: a failed ledger write must never break the cycle.
        let _ = std::fs::write(path, s);
    }
}

/// Fold one discovery pass into the ledger:
/// - paths in `current`: refresh `last_seen`, clear any vanished marker;
/// - paths already in the ledger but absent from `current`: stamp
///   `first_vanished_secs` if not yet stamped;
/// - paths not in the ledger at all are NOT added here — callers add
///   newly-discovered repos explicitly via [`mark_seen`] so a ledger can
///   also be maintained from non-daemon contexts.
pub(crate) fn update_seen_ledger(ledger: &mut SeenLedger, current: &[PathBuf], now_secs: u64) {
    let current_set: std::collections::HashSet<String> =
        current.iter().map(|p| p.display().to_string()).collect();
    for (path, entry) in ledger.iter_mut() {
        if current_set.contains(path) {
            entry.last_seen_secs = now_secs;
            entry.first_vanished_secs = None;
        } else if entry.first_vanished_secs.is_none() {
            entry.first_vanished_secs = Some(now_secs);
        }
    }
    for path in current {
        mark_seen(ledger, path, now_secs);
    }
}

/// Record a path as seen now (adding it to the ledger if new).
pub(crate) fn mark_seen(ledger: &mut SeenLedger, path: &Path, now_secs: u64) {
    let key = path.display().to_string();
    match ledger.get_mut(&key) {
        Some(entry) => {
            entry.last_seen_secs = now_secs;
            entry.first_vanished_secs = None;
        }
        None => {
            ledger.insert(
                key,
                SeenRepo {
                    last_seen_secs: now_secs,
                    first_vanished_secs: None,
                },
            );
        }
    }
}

/// Ledger entries currently missing from disk and within the TTL.
/// Callers should re-check existence (`!Path::exists`) before reporting —
/// the ledger may lag a just-restored checkout by one cycle.
pub(crate) fn detect_vanished_repos(ledger: &SeenLedger, now_secs: u64) -> Vec<VanishedRepo> {
    let mut vanished: Vec<VanishedRepo> = ledger
        .iter()
        .filter_map(|(path, entry)| {
            let first = entry.first_vanished_secs?;
            if now_secs.saturating_sub(first) >= VANISHED_ENTRY_TTL_SECS {
                return None;
            }
            Some(VanishedRepo {
                path: path.clone(),
                last_seen_secs: entry.last_seen_secs,
                first_vanished_secs: first,
            })
        })
        .collect();
    // Deterministic order for reports and tests.
    vanished.sort_by(|a, b| a.path.cmp(&b.path));
    vanished
}
