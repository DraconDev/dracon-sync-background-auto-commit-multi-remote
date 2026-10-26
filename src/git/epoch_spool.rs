//! Recovery spool inventory. Age alone never authorizes deleting recovery data.
//! Successful ordinary epochs release their own directory in staged_epoch;
//! refused, rejected, crashed and original-conflict candidates remain preserved.
use std::path::{Path, PathBuf};

pub(crate) struct SpoolLease {
    path: Option<PathBuf>,
}
impl SpoolLease {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
    pub(crate) fn named(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

pub(crate) async fn reap_abandoned_spools(
    _repo: &Path,
    _policy_secs: u64,
    _op_timeout_secs: u64,
) -> usize {
    // Neither age nor a process-local registry proves that retained objects
    // are published or unused by a different daemon/CLI process. Fail closed.
    0
}

/// Report retained directories. `apply` deliberately refuses destructive
/// cleanup until exact content/reachability and cross-process ownership can
/// be established; a prefix and old mtime are not that proof.
pub(crate) async fn sweep_epoch_spools(repo: &Path, max_age_secs: u64, apply: bool) -> (usize, u64) {
    if apply {
        eprintln!("⚠️ {}: recovery spools preserved; age-only deletion is unsafe", repo.display());
        return (0, 0);
    }
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command.args(["rev-parse", "--absolute-git-dir"]).current_dir(repo);
    let Ok(output) = crate::git::run_command_captured_output(command.into_tokio(), 60, "spool inventory", None).await else {
        return (0, 0);
    };
    if !output.status.success() { return (0, 0); }
    let Ok(text) = std::str::from_utf8(&output.stdout) else { return (0, 0); };
    let directory = PathBuf::from(text.trim_end_matches('\n'));
    if !directory.is_absolute() { return (0, 0); }
    let Ok(entries) = std::fs::read_dir(directory) else { return (0, 0); };
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 { 3600 } else { max_age_secs });
    let mut count = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue; };
        if !["dracon-staging-", "dracon-epoch-message-"].iter().any(|p| name.starts_with(p)) { continue; }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue; };
        if !meta.is_dir() || meta.modified().ok().and_then(|m| m.elapsed().ok()).is_none_or(|age| age < floor) { continue; }
        count += 1;
        if let Ok(files) = std::fs::read_dir(entry.path()) {
            for file in files.flatten() {
                if let Ok(meta) = std::fs::symlink_metadata(file.path()) {
                    if meta.is_file() { bytes += meta.len(); }
                }
            }
        }
    }
    (count, bytes)
}

pub(crate) fn human_bytes(bytes: u64) -> String {
    format!("{:.2} MiB", bytes as f64 / 1048576.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn old_unpublished_recovery_survives_apply_and_automatic_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let spool = root.path().join("dracon-staging-unpublished");
        std::fs::create_dir(&spool).unwrap();
        std::fs::write(spool.join("retained.pack"), b"only surviving candidate").unwrap();
        std::fs::File::open(&spool).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();
        assert_eq!(reap_abandoned_spools(root.path(), 1, 1).await, 0);
        assert_eq!(sweep_epoch_spools(root.path(), 1, true).await, (0, 0));
        assert_eq!(std::fs::read(spool.join("retained.pack")).unwrap(), b"only surviving candidate");
    }
}
