//! Last *observed* daemon push result, independent of current convergence.
//! Per-repository files avoid fleet-wide read/modify/write races.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Observation {
    pub version: u32,
    pub repo: String,
    pub attempted_at: u64,
    pub succeeded: bool,
}
fn path(repo: &Path) -> PathBuf {
    let base = std::env::var_os("DRACON_SYNC_STATE_DIR").map(PathBuf::from)
        .unwrap_or_else(|| dirs::state_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("dracon"));
    let digest = format!("{:x}", Sha256::digest(repo.to_string_lossy().as_bytes()));
    base.join("push-history").join(format!("{digest}.json"))
}
pub(super) fn record(repo: &Path, succeeded: bool) {
    let observation = Observation { version: 1, repo: repo.display().to_string(),
        attempted_at: super::scheduler_evidence::epoch(), succeeded };
    if let Err(error) = save_at(&path(repo), &observation) { eprintln!("⚠️ push history observation write failed: {error}"); }
}
fn save_at(path: &Path, observation: &Observation) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(observation)?)?;
    if let Err(error) = std::fs::rename(&tmp, path) { let _ = std::fs::remove_file(tmp); return Err(error); }
    Ok(())
}
pub(crate) fn load(repo: &Path) -> Option<Observation> {
    decode(&std::fs::read(path(repo)).ok()?, repo, super::scheduler_evidence::epoch())
}
fn decode(bytes: &[u8], repo: &Path, now: u64) -> Option<Observation> {
    let result: Observation = serde_json::from_slice(bytes).ok()?;
    (result.version == 1 && result.repo == repo.display().to_string() && result.attempted_at > 0 && result.attempted_at <= now).then_some(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_push_history_survives_new_edits_and_rejects_invalid_records() {
        let dir = tempfile::tempdir().unwrap(); let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let mut observation = Observation { version: 1, repo: repo.display().to_string(), attempted_at: 100, succeeded: true };
        let file = dir.path().join("history.json"); save_at(&file, &observation).unwrap();
        std::fs::write(repo.join("new-change"), "not published").unwrap();
        assert_eq!(decode(&std::fs::read(&file).unwrap(), &repo, 200), Some(observation.clone()));
        assert!(decode(&std::fs::read(&file).unwrap(), &repo, 99).is_none());
        observation.succeeded = false; save_at(&file, &observation).unwrap();
        assert!(!decode(&std::fs::read(&file).unwrap(), &repo, 200).unwrap().succeeded);
        observation.version = 2; assert!(decode(&serde_json::to_vec(&observation).unwrap(), &repo, 200).is_none());
        assert!(decode(b"{}", &repo, 200).is_none());
    }
}
