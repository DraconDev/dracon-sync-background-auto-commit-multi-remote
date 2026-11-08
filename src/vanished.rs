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
use std::collections::BTreeMap;
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

/// Key-sorted map (v0.113.82): deterministic serialization order.
/// `save_seen_ledger` skips byte-identical writes to keep the hosting
/// repo quiet — that compare only works when identical logical content
/// serializes identically, which a `HashMap` (per-instance random order)
/// never guarantees. A fresh load→no-op-update→save round-trip must be
/// byte-stable, or the ledger rewrites every pass and the daemon burns
/// a commit cycle per tick on machine state (observed 2026-09-20:
/// per-minute `.dracon` commits with zero entry diffs).
pub(crate) type SeenLedger = BTreeMap<String, SeenRepo>;

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

pub(crate) fn save_seen_ledger(path: &Path, ledger: &SeenLedger) -> bool {
    let Ok(s) = serde_json::to_string(ledger) else {
        return false;
    };
    // ADDED 2026-09-20 (v0.113.81): compare-before-write. The ledger
    // used to be rewritten every discovery pass with fresh
    // `last_seen_secs` timestamps, so its file changed every ~25 s
    // and the daemon spent a full status/classify/commit/push cycle
    // on the machine-state repo (~142 commits/hr on .dracon),
    // starving user-content repos of scheduler slots (ai-auto-writer
    // canary: 83 s dirty-to-commit). With quantized timestamps most
    // passes serialize identically — skip the write so the worktree
    // stays clean and no commit cycle fires. Best-effort throughout:
    // a failed ledger write must never break the cycle.
    match std::fs::read_to_string(path) {
        Ok(current) if current == s => false,
        _ => std::fs::write(path, s).is_ok(),
    }
}

/// Fold one discovery pass into the ledger:
/// - paths in `current`: refresh `last_seen`, clear any vanished marker;
/// - paths already in the ledger but absent from `current`: stamp
///   `first_vanished_secs` if not yet stamped;
/// - paths not in the ledger at all are NOT added here — callers add
///   newly-discovered repos explicitly via [`mark_seen`] so a ledger can
///   also be maintained from non-daemon contexts.
///
/// CHANGED 2026-09-20 (v0.113.81): the `last_seen` refresh is quantized
/// to [`SEEN_LAST_SEEN_QUANTUM_SECS`]. Unquantized, every pass stamped
/// fresh timestamps so the serialized ledger changed every cycle and
/// the hosting repo burned a commit cycle per tick (see
/// [`save_seen_ledger`]). `first_vanished_secs` stamping AND clearing
/// still happen the very first pass they are observed — only the
/// steady-state "still here" heartbeat is quantized. Vanish detection
/// (day-scale TTLs, human response) cannot tell a 10-minute-quantized
/// heartbeat from a live one.
///
/// Quantum for the `last_seen` steady-state heartbeat (v0.113.81).
/// Bounds the machine-state churn that starved user commits: the ledger
/// file can change at most once per quantum per repo, instead of every
/// discovery pass.
///
/// CHANGED 2026-11-07: the quantum is now applied to the STORED VALUE,
/// not only to the comparison (see [`quantum_bucket`]). v0.113.81
/// quantized per ENTRY — `now - entry.last_seen_secs >= QUANTUM` — but
/// entries were first stamped at different moments, so they crossed the
/// threshold at DIFFERENT wall-clock times. The serialized file therefore
/// changed whenever ANY entry crossed, roughly every 16-60 s for a
/// ~38-repo fleet, and `save_seen_ledger`'s compare-before-write never
/// skipped a write. Measured on 2026-11-07: the hosting repo committed
/// ~43 times/hour (506 in 24 h), was dirty 25% of the time, and flipped
/// between `✅ CLEAN` and `❔ UNKNOWN` in the `repos` report — the
/// scheduler-slot starvation v0.113.81 set out to stop, still happening.
///
/// Storing the BUCKET makes every entry refreshed inside the same window
/// hold the SAME value, so the serialized ledger is byte-identical between
/// buckets and the write is skipped entirely. The file then changes once
/// per quantum, not once per entry-crossing.
pub(crate) const SEEN_LAST_SEEN_QUANTUM_SECS: u64 = 600;

/// Bucket-align a heartbeat timestamp.
///
/// Every entry refreshed inside the same quantum window stores the same
/// value, so the whole ledger serializes identically and the hosting
/// repo's worktree stays clean.
fn quantum_bucket(now_secs: u64) -> u64 {
    now_secs - (now_secs % SEEN_LAST_SEEN_QUANTUM_SECS)
}

/// Refresh one entry's heartbeat, honoring the quantum. Returns true
/// when the entry changed (caller signal for write-skipping).
fn refresh_seen(entry: &mut SeenRepo, now_secs: u64) -> bool {
    // A cleared vanish marker is SIGNAL — never delay it.
    if entry.first_vanished_secs.is_some() {
        entry.last_seen_secs = quantum_bucket(now_secs);
        entry.first_vanished_secs = None;
        return true;
    }
    // The stored value IS the bucket, so "did this window roll over" is
    // the whole comparison. A legacy entry holding an un-bucketed
    // timestamp migrates on the first pass.
    if now_secs.saturating_sub(entry.last_seen_secs) >= SEEN_LAST_SEEN_QUANTUM_SECS {
        entry.last_seen_secs = now_secs;
        return true;
    }
    false
}
pub(crate) fn update_seen_ledger(ledger: &mut SeenLedger, current: &[PathBuf], now_secs: u64) {
    let current_set: std::collections::HashSet<String> =
        current.iter().map(|p| p.display().to_string()).collect();
    for (path, entry) in ledger.iter_mut() {
        if current_set.contains(path) {
            refresh_seen(entry, now_secs);
        } else if entry.first_vanished_secs.is_none() {
            entry.first_vanished_secs = Some(now_secs);
        }
    }
    for path in current {
        mark_seen(ledger, path, now_secs);
    }
}

/// Record a path as seen now (adding it to the ledger if new).
/// First sightings insert immediately; steady-state refreshes honor
/// the [`SEEN_LAST_SEEN_QUANTUM_SECS`] quantum (see [`refresh_seen`]).
/// Returns true when the ledger changed.
pub(crate) fn mark_seen(ledger: &mut SeenLedger, path: &Path, now_secs: u64) -> bool {
    let key = path.display().to_string();
    match ledger.get_mut(&key) {
        Some(entry) => refresh_seen(entry, now_secs),
        None => {
            ledger.insert(
                key,
                SeenRepo {
                    // Bucketed like every other heartbeat: a brand-new
                    // entry must not make the serialized ledger differ
                    // from the ones refreshed in the same window.
                    last_seen_secs: quantum_bucket(now_secs),
                    first_vanished_secs: None,
                },
            );
            true
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

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(offset: u64) -> u64 {
        1_700_000_000 + offset
    }

    /// A base aligned to the quantum boundary.
    ///
    /// The stored heartbeat is an ABSOLUTE quantum bucket, so the window
    /// boundaries are fixed multiples of the quantum since the epoch — not
    /// "600 s after this entry was first seen". Anchoring the fixtures to
    /// a real boundary makes "inside the window" read literally.
    fn aligned(offset: u64) -> u64 {
        1_700_000_400 + offset
    }

    /// The bucket a heartbeat at `offset` is stored as.
    fn bucket_of(offset: u64) -> u64 {
        quantum_bucket(secs(offset))
    }

    #[test]
    fn mark_seen_adds_and_clears_vanished_stamp() {
        let mut ledger = SeenLedger::new();
        let path = Path::new("/tmp/dracon-vanished-test/r");
        mark_seen(&mut ledger, path, secs(0));
        let entry = ledger.get("/tmp/dracon-vanished-test/r").unwrap();
        assert_eq!(entry.last_seen_secs, bucket_of(0));
        assert_eq!(entry.first_vanished_secs, None);

        // Re-mark after a vanish clears the stamp.
        ledger
            .get_mut("/tmp/dracon-vanished-test/r")
            .unwrap()
            .first_vanished_secs = Some(secs(5));
        mark_seen(&mut ledger, path, secs(10));
        let entry = ledger.get("/tmp/dracon-vanished-test/r").unwrap();
        assert_eq!(entry.last_seen_secs, bucket_of(10));
        assert_eq!(entry.first_vanished_secs, None);
    }

    #[test]
    fn update_stamps_newly_absent_and_clears_returned() {
        let mut ledger = SeenLedger::new();
        mark_seen(&mut ledger, Path::new("/w/alpha"), secs(0));
        mark_seen(&mut ledger, Path::new("/w/beta"), secs(0));

        // Cycle 1: alpha gone.
        update_seen_ledger(&mut ledger, &[PathBuf::from("/w/beta")], secs(60));
        assert_eq!(
            ledger["/w/alpha"].first_vanished_secs,
            Some(secs(60)),
            "absent path must be stamped exactly once"
        );
        assert_eq!(ledger["/w/beta"].first_vanished_secs, None);

        // Cycle 2: alpha still gone — the stamp must NOT advance.
        // Beta is continuously seen but inside the quantum: its
        // heartbeat HOLDS (v0.113.81 quantization — steady-state
        // "still here" no longer rewrites the ledger every pass).
        update_seen_ledger(&mut ledger, &[PathBuf::from("/w/beta")], secs(120));
        assert_eq!(ledger["/w/alpha"].first_vanished_secs, Some(secs(60)));
        assert_eq!(
            ledger["/w/beta"].last_seen_secs,
            bucket_of(0),
            "quantum must hold the heartbeat inside 600 s"
        );

        // Cycle 2b: past the quantum — the heartbeat rolls forward.
        update_seen_ledger(
            &mut ledger,
            &[PathBuf::from("/w/beta")],
            secs(0) + super::SEEN_LAST_SEEN_QUANTUM_SECS,
        );
        assert_eq!(
            ledger["/w/beta"].last_seen_secs,
            bucket_of(0) + super::SEEN_LAST_SEEN_QUANTUM_SECS
        );

        // Cycle 3: alpha returns — vanished marker cleared.
        // Clearing is SIGNAL, never quantized: immediate even though
        // only secs(180) elapsed.
        update_seen_ledger(
            &mut ledger,
            &[PathBuf::from("/w/alpha"), PathBuf::from("/w/beta")],
            secs(180),
        );
        assert_eq!(ledger["/w/alpha"].first_vanished_secs, None);
        assert_eq!(ledger["/w/alpha"].last_seen_secs, bucket_of(180));
    }

    #[test]
    fn quantum_holds_heartbeat_but_not_vanish_signals() {
        // Steady entries stay byte-identical across passes inside the
        // quantum (the write-skipping that quiets the hosting repo),
        // while vanish stamps and clears land immediately.
        let mut ledger = SeenLedger::new();
        mark_seen(&mut ledger, Path::new("/w/steady"), aligned(0));
        // 599 s later: no change.
        update_seen_ledger(&mut ledger, &[PathBuf::from("/w/steady")], aligned(599));
        assert_eq!(ledger["/w/steady"].last_seen_secs, aligned(0));
        // Vanish stamps immediately even inside the quantum.
        update_seen_ledger(&mut ledger, &[], aligned(600));
        assert_eq!(ledger["/w/steady"].first_vanished_secs, Some(aligned(600)));
        // Return clears immediately even inside the quantum.
        update_seen_ledger(&mut ledger, &[PathBuf::from("/w/steady")], aligned(601));
        assert_eq!(ledger["/w/steady"].first_vanished_secs, None);
        assert_eq!(ledger["/w/steady"].last_seen_secs, aligned(600));
    }

    /// Entries that a LEGACY (pre-fix) ledger left in DIFFERENT buckets
    /// must converge on the next pass.
    ///
    /// This is the discriminating property. v0.113.81 refreshed each
    /// entry against its OWN stored value, so entries desynchronised into
    /// separate windows and the file changed whenever any one of them
    /// crossed — the hosting repo then churned a commit every ~16-60 s
    /// (measured 2026-11-07: ~43 commits/hour, dirty 25% of the time, and
    /// the `repos` report flipping between `✅ CLEAN` and `❔ UNKNOWN`).
    ///
    /// Comparing against the current bucket instead makes every entry
    /// refreshed in the same window hold the same value, so the file is
    /// byte-stable and the write is skipped.
    #[test]
    fn entries_in_different_buckets_converge_on_the_next_pass() {
        let mut ledger = SeenLedger::new();
        // A pre-fix ledger: the two entries sit in different windows.
        ledger.insert(
            "/w/early".to_string(),
            SeenRepo {
                last_seen_secs: aligned(0),
                first_vanished_secs: None,
            },
        );
        ledger.insert(
            "/w/late".to_string(),
            SeenRepo {
                last_seen_secs: aligned(300),
                first_vanished_secs: None,
            },
        );
        let current = vec![PathBuf::from("/w/early"), PathBuf::from("/w/late")];
        update_seen_ledger(&mut ledger, &current, aligned(600));
        let values: std::collections::BTreeSet<u64> =
            ledger.values().map(|entry| entry.last_seen_secs).collect();
        assert_eq!(
            values.len(),
            1,
            "entries in different buckets must converge on one heartbeat value, got {values:?}"
        );
    }

    /// The v0.113.81 quantum was applied PER ENTRY, so entries first
    /// stamped at different moments crossed the threshold at DIFFERENT
    /// wall-clock times: the serialized file changed whenever any one of
    /// them crossed, and `save_seen_ledger`'s compare-before-write never
    /// skipped a write. Measured on 2026-11-07, the hosting repo committed
    /// ~43 times/hour, was dirty 25% of the time, and flipped between
    /// `✅ CLEAN` and `❔ UNKNOWN` in the `repos` report.
    ///
    /// Storing the BUCKET makes every entry refreshed inside one window
    /// hold the SAME value, so the ledger is byte-stable and the hosting
    /// repo's worktree stays clean.
    #[test]
    fn staggered_entries_share_one_heartbeat_value_inside_a_quantum() {
        let mut ledger = SeenLedger::new();
        // Eight repos first seen 37 s apart — the real fleet discovers
        // repos at different moments, which is what desynchronised them.
        let current: Vec<PathBuf> = (0..8).map(|i| PathBuf::from(format!("/w/repo{i}"))).collect();
        for (i, path) in current.iter().enumerate() {
            mark_seen(&mut ledger, path, secs(i as u64 * 37));
        }
        // A pass inside the next window must leave every entry on ONE
        // value — that is the file-level property the per-entry quantum
        // failed to provide.
        update_seen_ledger(&mut ledger, &current, secs(400));
        let values: std::collections::BTreeSet<u64> =
            ledger.values().map(|entry| entry.last_seen_secs).collect();
        assert_eq!(
            values.len(),
            1,
            "every entry must share one heartbeat value inside a quantum window, got {values:?}"
        );
    }

    /// The same property, observed through the write-skip: once the
    /// entries are synchronised, a pass inside the same window must not
    /// touch the file at all (no dirty worktree, no commit, no scheduler
    /// slot burned on the machine-state repo).
    #[test]
    fn synchronised_ledger_does_not_touch_the_file_inside_a_quantum() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("ledger.json");
        let mut ledger = SeenLedger::new();
        let current: Vec<PathBuf> = (0..8).map(|i| PathBuf::from(format!("/w/repo{i}"))).collect();
        for (i, p) in current.iter().enumerate() {
            mark_seen(&mut ledger, p, secs(i as u64 * 37));
        }
        assert!(save_seen_ledger(&path, &ledger), "first write lands");

        // Synchronise, then write the synchronised state.
        update_seen_ledger(&mut ledger, &current, secs(400));
        assert!(save_seen_ledger(&path, &ledger), "synchronised state writes once");

        // A pass later in the SAME window must not change the bytes.
        update_seen_ledger(&mut ledger, &current, secs(500));
        assert!(
            !save_seen_ledger(&path, &ledger),
            "a pass inside the same quantum window must not dirty the hosting repo"
        );
    }

    #[test]
    fn save_skips_identical_content() {
        // The commit-cycle killer: unchanged ledgers must not touch
        // the file (no dirty → no commit → no scheduler slot burned).
        let dir = tempfile::TempDir::new().expect("tempdir");
        let p = dir.path().join("ledger.json");
        let mut ledger = SeenLedger::new();
        mark_seen(&mut ledger, Path::new("/w/a"), secs(0));
        assert!(save_seen_ledger(&p, &ledger), "first write lands");
        assert!(
            !save_seen_ledger(&p, &ledger),
            "identical content must skip the write"
        );
        mark_seen(&mut ledger, Path::new("/w/b"), secs(0));
        assert!(save_seen_ledger(&p, &ledger), "changed content writes");
    }

    #[test]
    fn save_stable_across_reload_round_trip() {
        // The 0.113.82 regression: a load → no-op-update → save cycle
        // must be byte-identical, or the hosting repo dirties every
        // pass on key ORDER alone (HashMap's per-instance random
        // iteration defeated the 0.113.81 write-skip: per-minute
        // `.dracon` commits with zero entry diffs).
        let dir = tempfile::TempDir::new().expect("tempdir");
        let p = dir.path().join("ledger.json");
        let mut ledger = SeenLedger::new();
        for r in ["/w/zeta", "/w/alpha", "/w/mid"] {
            mark_seen(&mut ledger, Path::new(r), secs(0));
        }
        assert!(save_seen_ledger(&p, &ledger));
        let before = std::fs::read(&p).expect("read");
        let mut reloaded = load_seen_ledger(&p);
        update_seen_ledger(
            &mut reloaded,
            &[
                PathBuf::from("/w/zeta"),
                PathBuf::from("/w/alpha"),
                PathBuf::from("/w/mid"),
            ],
            secs(10),
        );
        assert!(
            !save_seen_ledger(&p, &reloaded),
            "steady reload round-trip must skip the write"
        );
        let after = std::fs::read(&p).expect("read");
        assert_eq!(
            before, after,
            "reload round-trip inside the quantum must be byte-stable"
        );
    }

    #[test]
    fn detect_reports_only_unexpired_vanished_entries() {
        let mut ledger = SeenLedger::new();
        ledger.insert(
            "/w/gone".to_string(),
            SeenRepo {
                last_seen_secs: secs(0),
                first_vanished_secs: Some(secs(100)),
            },
        );
        ledger.insert(
            "/w/healthy".to_string(),
            SeenRepo {
                last_seen_secs: secs(50),
                first_vanished_secs: None,
            },
        );
        // Expired entry (older than the TTL) must not be reported.
        ledger.insert(
            "/w/ancient".to_string(),
            SeenRepo {
                last_seen_secs: secs(0),
                first_vanished_secs: Some(secs(1) + VANISHED_ENTRY_TTL_SECS - 1 - secs(1)),
            },
        );

        let detected = detect_vanished_repos(&ledger, secs(200));
        let paths: Vec<&str> = detected.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(paths, vec!["/w/gone"], "only unexpired vanished entries");
        assert_eq!(detected[0].first_vanished_secs, secs(100));

        // At TTL expiry it drops out.
        assert!(detect_vanished_repos(&ledger, secs(100) + VANISHED_ENTRY_TTL_SECS).is_empty());
    }

    #[test]
    fn detect_is_sorted_by_path_for_deterministic_reports() {
        let mut ledger = SeenLedger::new();
        for name in ["zeta", "alpha", "mid"] {
            ledger.insert(
                format!("/w/{name}"),
                SeenRepo {
                    last_seen_secs: secs(0),
                    first_vanished_secs: Some(secs(10)),
                },
            );
        }
        let detected = detect_vanished_repos(&ledger, secs(20));
        let paths: Vec<&str> = detected.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(paths, vec!["/w/alpha", "/w/mid", "/w/zeta"]);
    }

    #[test]
    fn ledger_roundtrips_through_json() {
        let dir = std::env::temp_dir().join("dracon-vanished-ledger-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SEEN_LEDGER_FILE);
        let mut ledger = SeenLedger::new();
        mark_seen(&mut ledger, Path::new("/w/present"), secs(7));
        ledger.insert(
            "/w/gone".to_string(),
            SeenRepo {
                last_seen_secs: secs(3),
                first_vanished_secs: Some(secs(9)),
            },
        );
        save_seen_ledger(&path, &ledger);
        let loaded = load_seen_ledger(&path);
        assert_eq!(loaded, ledger);
        // Legacy/empty file loads as an empty ledger, never panics.
        std::fs::write(&path, "not json at all").unwrap();
        assert!(load_seen_ledger(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
