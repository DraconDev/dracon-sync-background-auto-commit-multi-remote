//! Versioned observational evidence; never changes admission or ownership.
use super::{recovery::SyncRecovery, RepoActivity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase { Queued, Working, Cancelling, Inspecting, Retrying, Blocked }

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RepoEvidence {
    pub phase: Option<Phase>,
    pub execution_secs: Option<u64>,
    pub retry_secs: Option<u64>,
    /// First continuously dirty observation, NOT last commit/file age.
    pub dirty_secs: Option<u64>,
    /// Effective filter-aware classification completed this cycle.
    pub eligible: bool,
    /// Admission is not progress; kept separate from execution age.
    pub admitted_secs: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Snapshot {
    pub version: u32,
    pub written_at: u64,
    pub mode: String,
    pub repos: BTreeMap<PathBuf, RepoEvidence>,
}
impl Snapshot {
    pub(super) fn empty(mode: &str) -> Self {
        Self { version: 1, written_at: epoch(), mode: mode.into(), repos: BTreeMap::new() }
    }
    /// Future clocks and old/unknown schemas cannot establish current work.
    pub(crate) fn is_fresh(&self, now: u64) -> bool {
        self.version == 1 && self.written_at <= now && now - self.written_at <= 30
            && matches!(self.mode.as_str(), "running" | "paused" | "discovering" | "stopped")
    }
}

pub(super) struct Inputs<'a> {
    pub activity: &'a HashMap<PathBuf, RepoActivity>,
    pub eligible: &'a HashSet<PathBuf>,
    pub probes: &'a HashSet<PathBuf>,
    pub maintenance: &'a HashSet<PathBuf>,
    pub retries: &'a [&'a HashMap<PathBuf, Instant>],
    pub recovery: &'a SyncRecovery,
    pub admitted: &'a HashMap<PathBuf, Instant>,
}

pub(super) fn build(inputs: Inputs<'_>, now: Instant, written_at: u64) -> Snapshot {
    let mut repos = inputs.recovery.observations(now);
    for path in inputs.activity.keys().chain(inputs.probes).chain(inputs.maintenance) {
        repos.entry(path.clone()).or_default();
    }
    for map in inputs.retries {
        for (path, until) in *map {
            if *until > now {
                let row = repos.entry(path.clone()).or_default();
                let wait = until.duration_since(now).as_secs().max(1);
                row.retry_secs = Some(row.retry_secs.unwrap_or(0).max(wait));
            }
        }
    }
    for (path, row) in &mut repos {
        let activity = inputs.activity.get(path);
        row.dirty_secs = activity.and_then(|a| a.dirty_since).map(|since| now.saturating_duration_since(since).as_secs());
        row.eligible = inputs.eligible.contains(path);
        row.admitted_secs = inputs.admitted.get(path).map(|since| now.saturating_duration_since(*since).as_secs());
        // Actual ownership wins over old cooldowns, followed by known gates.
        if row.phase.is_none() {
            row.phase = if inputs.maintenance.contains(path) { Some(Phase::Working) }
                else if inputs.probes.contains(path) { Some(Phase::Inspecting) }
                else if row.retry_secs.is_some() && activity.is_some_and(|a| a.blocked_since.is_some()) { Some(Phase::Blocked) }
                else if row.retry_secs.is_some() { Some(Phase::Retrying) }
                else { None };
        }
    }
    Snapshot { version: 1, written_at, mode: "running".into(), repos }
}

fn path() -> PathBuf {
    dirs::state_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("dracon/dracon-sync-scheduler-evidence.json")
}
pub(super) fn epoch() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|t| t.as_secs()).unwrap_or(0) }
pub(super) fn save(snapshot: &Snapshot) {
    if let Err(error) = save_at(&path(), snapshot) { eprintln!("⚠️ scheduler evidence write failed: {error}"); }
}
fn save_at(path: &Path, snapshot: &Snapshot) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec(snapshot)?;
    std::fs::write(&tmp, bytes)?;
    if let Err(error) = std::fs::rename(&tmp, path) { let _ = std::fs::remove_file(tmp); return Err(error); }
    Ok(())
}
pub(crate) fn load() -> Option<Snapshot> {
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(path()).ok()?).ok()?;
    snapshot.is_fresh(epoch()).then_some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheduler_evidence_freshness_and_roundtrip_are_fail_closed() {
        let mut snapshot = Snapshot::empty("running");
        snapshot.written_at = 100;
        for (now, expected) in [(99,false),(100,true),(130,true),(131,false)] { assert_eq!(snapshot.is_fresh(now),expected); }
        snapshot.version = 2; assert!(!snapshot.is_fresh(100));
        snapshot.version = 1; snapshot.mode = "invented".into(); assert!(!snapshot.is_fresh(100));
        snapshot.mode = "running".into();
        let dir = tempfile::tempdir().unwrap(); let file = dir.path().join("state.json");
        save_at(&file, &snapshot).unwrap();
        let decoded: Snapshot = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert!(decoded.repos.is_empty()); assert!(decoded.is_fresh(100));
        assert!(serde_json::from_slice::<Snapshot>(b"{}").is_err());
    }
    #[test]
    fn scheduler_evidence_distinguishes_admission_execution_cancel_and_retry() {
        let now = Instant::now(); let path = PathBuf::from("fixture");
        let mut recovery = SyncRecovery::default(); let stamp = recovery.reserve(&path);
        assert_eq!(recovery.observations(now)[&path].phase, Some(Phase::Queued));
        stamp.set(now).unwrap();
        assert_eq!(recovery.observations(now)[&path].phase, Some(Phase::Working));
        recovery.expired(now, std::time::Duration::ZERO);
        assert_eq!(recovery.observations(now)[&path].phase, Some(Phase::Cancelling));
        recovery.finish(&path, true, now);
        assert_eq!(recovery.observations(now)[&path].phase, Some(Phase::Retrying));
        recovery.finish(&path, false, now); assert!(recovery.observations(now).is_empty());
    }
}
