//! Durable epoch recovery and guarded reclamation. Age schedules inventory only;
//! kernel ownership, irreversible producer settlement, complete independent
//! recovery and final source checks are all required before any removal.
//! Qualification remains disabled until regression/fault/non-vacuity gates pass.
use anyhow::{ensure, Result};
use std::path::{Path, PathBuf};

const MAX_SPOOLS_PER_PASS: usize = 64;
const LOSSLESS_RECLAIM_QUALIFIED: bool = false;

#[cfg(test)]
#[path = "epoch_spool_fixture_gate.rs"]
mod fixture_gate;
#[cfg(test)]
pub(crate) use fixture_gate::allow_reclaim_in_fixture;

fn reclaim_is_qualified(repo: &Path) -> bool {
    #[cfg(test)]
    {
        LOSSLESS_RECLAIM_QUALIFIED || fixture_gate::permitted(repo)
    }
    #[cfg(not(test))]
    {
        let _ = repo;
        LOSSLESS_RECLAIM_QUALIFIED
    }
}

#[path = "epoch_spool_dispatch.rs"]
mod dispatch;
#[path = "epoch_spool_liveness.rs"]
mod liveness;
#[path = "epoch_spool_ownership.rs"]
mod ownership;
#[path = "epoch_spool_producer.rs"]
mod producer;
#[path = "epoch_spool_reclaim.rs"]
mod reclaim;
#[path = "epoch_spool_recovery.rs"]
mod recovery;
pub(crate) use dispatch::run_spool_git;
pub(crate) use ownership::SpoolLease;
pub(crate) use producer::epoch_producer_trampoline;

/// Whole-pass budget as well as a directory-count cap. Cancelling preparation
/// retains its kernel fence until its own Git processes have joined; that
/// detached work can retain recovery data but never remove an original source.
pub(crate) async fn reap_abandoned_spools(
    repo: &Path,
    max_age_secs: u64,
    op_timeout_secs: u64,
) -> usize {
    if !reclaim_is_qualified(repo) {
        return 0;
    }
    let seconds = op_timeout_secs.clamp(30, 600);
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 {
        3600
    } else {
        max_age_secs
    });
    let mut reaped = 0;
    let pass = async {
        let Some(gitdir) = absolute_git_dir(repo).await else {
            return;
        };
        let candidates = tokio::task::spawn_blocking(move || spool_candidates(&gitdir, floor))
            .await
            .unwrap_or_default();
        for spool in candidates.into_iter().take(MAX_SPOOLS_PER_PASS) {
            if reclaim_spool_if_lossless(repo, &spool, seconds, floor).await {
                reaped += 1;
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(seconds), pass).await;
    reaped
}

/// Exactly the same transaction is used after a settled-unpublished deadline,
/// after publication/empty candidates and by the automatic reaper. No branch
/// treats a present object, an empty pack list or publication as a proof.
pub(crate) async fn reclaim_spool_if_lossless(
    repo: &Path,
    spool: &Path,
    timeout: u64,
    legacy_floor: std::time::Duration,
) -> bool {
    if !reclaim_is_qualified(repo) {
        eprintln!(
            "⚠️ {}: epoch spool {} preserved; durable recovery qualification incomplete",
            repo.display(),
            spool.display()
        );
        return false;
    }
    match reclaim::attempt(repo, spool, timeout, legacy_floor, reclaim::Fault::None).await {
        Ok(receipt) => {
            eprintln!(
                "♻️ {}: durable epoch recovery {} before removal of {}; {} main-missing objects",
                repo.display(),
                receipt.record.display(),
                spool.display(),
                receipt.unique_objects
            );
            true
        }
        Err(error) => {
            eprintln!(
                "⚠️ {}: epoch reclaim {} failed; inspect source and retained recovery: {error:#}",
                repo.display(),
                spool.display()
            );
            false
        }
    }
}

/// The age-only command remains report-only. Its apply refusal is independent
/// of the qualified recovery transaction and can never be turned into deletion.
pub(crate) async fn sweep_epoch_spools(
    repo: &Path,
    max_age_secs: u64,
    apply: bool,
) -> (usize, u64) {
    if apply {
        eprintln!(
            "⚠️ {}: recovery spools preserved; age-only deletion is unsafe",
            repo.display()
        );
        return (0, 0);
    }
    let Some(gitdir) = absolute_git_dir(repo).await else {
        return (0, 0);
    };
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 {
        3600
    } else {
        max_age_secs
    });
    let candidates = tokio::task::spawn_blocking(move || spool_candidates(&gitdir, floor))
        .await
        .unwrap_or_default();
    let mut bytes = 0u64;
    for spool in &candidates {
        match directory_bytes(spool) {
            Ok(size) => {
                bytes = bytes.saturating_add(size);
            }
            Err(error) => eprintln!(
                "⚠️ inventory could not account {}: {error:#}",
                spool.display()
            ),
        }
    }
    (candidates.len(), bytes)
}

fn spool_candidates(gitdir: &Path, floor: std::time::Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(gitdir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if !["dracon-staging-", "dracon-epoch-message-"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                return None;
            }
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            if !metadata.is_dir() {
                return None;
            }
            let modified = metadata.modified().ok()?;
            (modified.elapsed().ok()? >= floor).then(|| (modified, entry.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

async fn absolute_git_dir(repo: &Path) -> Option<PathBuf> {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(repo);
    let output =
        crate::git::run_command_captured_output(command.into_tokio(), 60, "spool gitdir", None)
            .await
            .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(
        std::str::from_utf8(&output.stdout)
            .ok()?
            .trim_end_matches('\n'),
    );
    path.is_absolute().then_some(path)
}
fn directory_bytes(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let metadata = std::fs::symlink_metadata(entry?.path())?;
        ensure!(
            metadata.is_file(),
            "unsupported spool entry during accounting"
        );
        total = total
            .checked_add(metadata.len())
            .ok_or_else(|| anyhow::anyhow!("spool byte count overflow"))?;
    }
    Ok(total)
}
pub(crate) fn human_bytes(bytes: u64) -> String {
    format!("{:.2} MiB", bytes as f64 / 1048576.0)
}

#[cfg(test)]
#[path = "epoch_spool_tests.rs"]
mod tests;
