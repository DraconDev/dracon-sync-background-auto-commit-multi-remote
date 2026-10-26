//! Bounded reaping of abandoned private-epoch spools.
//!
//! ADDED 2026-10-26 (disk-exhaustion incident). `staged_epoch::Staging`
//! and `unmerged_epoch` allocate a 0700 `dracon-staging-*` directory
//! inside the repo's git dir, and `Epoch::retained` deliberately KEEPS
//! that directory (index + an independent object pack of the candidate
//! payloads) so a later foreign index replacement cannot make a proven
//! candidate collectible by prune. That retention is correct for ONE
//! epoch, but nothing ever released it:
//!
//! - `finish()` returns the epoch and the retention survives publishing.
//! - `commit()` consumes `self` and still never removes it.
//! - `Epoch`/`Staging` have no `Drop`, so every early return, refusal
//!   or deadline leaves the spool behind.
//!
//! Observed cost (2026-10-26): browser-extensions-shared accumulated
//! 141 spools totalling 101 GiB in one afternoon (each ~875 MiB: a
//! FULL-tree index plus an independent pack), which filled the 907 GiB
//! root filesystem to 100% and wedged every watched repository with
//! `os error 28` + `index.lock write error. Out of diskspace`. A full
//! disk also means the NEXT spool cannot even be initialized, so the
//! repo cannot converge at all — this is a convergence defect, not just
//! a housekeeping one.
//!
//! Design (mirrors `maybe_auto_gc` / `maybe_prune_stale_backup_branches`):
//! - A spool's own mtime is touched only while it is actively being
//!   created (`tempfile::Builder::tempdir_in` + `keep()`), so an
//!   untouched spool older than a generous age is definitively
//!   abandoned, not in flight.
//! - Spools CURRENTLY REFERENCED by this process are never removed, and
//!   the live registry is consulted twice per candidate (before and
//!   after the age check), so GC cannot race an epoch that is still
//!   publishing.
//! - Cleanup is best-effort and never fatal: a failure is logged and the
//!   repo still converges. Unlike `auto_gc_garbage_threshold_bytes`
//!   (opt-out via 0), a 0 age here falls back to the safe default rather
//!   than silently disabling the only bound on this leak.
//!
//! The reaper only ever deletes directories this daemon created
//! (exact `dracon-staging-*` prefix) under a git dir it resolved itself,
//! so it cannot touch operator files or another tool's spools.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Prefix every private-epoch spool directory starts with.
/// MUST match `tempfile::Builder::prefix` in
/// `sync/staged_epoch.rs` (pinned by a regression test).
pub(crate) const SPOOL_PREFIX: &str = "dracon-staging-";

/// Default age before an unregistered spool is considered abandoned.
/// Deliberately generous: a legitimate epoch lifetime is bounded by the
/// sync deadline (minutes), not hours.
pub(crate) const DEFAULT_SPOOL_MAX_AGE_SECS: u64 = 3600;

/// At most one reaper pass per repo per hour (same pattern as
/// `AUTO_GC_ATTEMPTS`): the steady state costs one directory scan per
/// hour per repo instead of one per sync cycle.
static SPOOL_REAP_ATTEMPTS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Instant>>> =
    OnceLock::new();

/// Spools owned by an epoch that is still alive in this process.
/// Guarded by one mutex so registration, release and reaping can never
/// interleave on the same candidate.
static LIVE_SPOOLS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

fn live_spools() -> &'static Mutex<BTreeSet<PathBuf>> {
    LIVE_SPOOLS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn attempts() -> &'static Mutex<std::collections::HashMap<PathBuf, Instant>> {
    SPOOL_REAP_ATTEMPTS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// RAII marker for a spool a live epoch may still need.
///
/// `sync::staged_epoch` holds this for the epoch's lifetime; dropping it
/// (every early return, refusal, error and success path) releases the
/// spool to the reaper's discretion. It does NOT delete anything itself:
/// removal stays in the periodic reaper so error paths keep their
/// diagnostic spool for a bounded window, exactly as documented today.
pub(crate) struct SpoolLease {
    path: Option<PathBuf>,
}

impl SpoolLease {
    /// Track `path` as live for as long as the returned lease is held.
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        if let Some(path) = &path {
            let mut live = live_spools().lock().unwrap_or_else(|p| p.into_inner());
            live.insert(path.clone());
        }
        Self { path }
    }

    /// The spool this lease protects, if any.
    pub(crate) fn named(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Drop for SpoolLease {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let mut live = live_spools().lock().unwrap_or_else(|p| p.into_inner());
            live.remove(path);
        }
    }
}

/// Whether the reaper may consider `path` abandoned right now.
fn is_leased(path: &Path) -> bool {
    live_spools()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(path)
}

/// Resolve the effective age floor. `0` is NOT an opt-out: the default
/// keeps the leak bounded even under a zeroed config.
fn effective_max_age(policy_secs: u64) -> Duration {
    Duration::from_secs(if policy_secs == 0 {
        DEFAULT_SPOOL_MAX_AGE_SECS
    } else {
        policy_secs
    })
}

/// A spool-like entry found on disk, resolved WITHOUT following symlinks.
struct Candidate {
    path: PathBuf,
    is_real_directory: bool,
    age: Option<Duration>,
}

fn candidates(git_dir: &Path, now: std::time::SystemTime) -> Vec<Candidate> {
    let Ok(entries) = std::fs::read_dir(git_dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(SPOOL_PREFIX) {
            continue;
        }
        // `symlink_metadata` (NOT `metadata`) so a symlinked
        // `dracon-staging-*` entry is never followed: `remove_dir_all`
        // through a symlink would delete outside this repository.
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let age = meta
            .modified()
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok());
        found.push(Candidate {
            path: entry.path(),
            is_real_directory: meta.file_type().is_dir() && !meta.file_type().is_symlink(),
            age,
        });
    }
    found
}

/// Resolve the repo's absolute git dir with a finite deadline.
async fn resolve_git_dir(repo: &Path, op_timeout_secs: u64) -> anyhow::Result<PathBuf> {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(["--no-optional-locks", "rev-parse", "--absolute-git-dir"])
        .current_dir(repo)
        .env("GIT_OPTIONAL_LOCKS", "0");
    let output = crate::git::run_command_captured_output(
        command.into_tokio(),
        op_timeout_secs.max(1),
        "epoch spool git dir",
        None,
    )
    .await?;
    anyhow::ensure!(
        output.status.success(),
        "cannot resolve git dir: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let path = PathBuf::from(std::str::from_utf8(&output.stdout)?.trim_end_matches('\n'));
    anyhow::ensure!(path.is_absolute(), "git dir identity is not absolute");
    Ok(path)
}

/// Sum regular-file sizes below `dir` without following symlinks.
/// Best-effort: unreadable entries contribute 0 (this only feeds a log).
fn dir_size_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            total += dir_size_bytes(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

/// Reap abandoned private-epoch spools in `repo`'s git dir.
///
/// Returns the number of spools removed. Never fatal: every failure is
/// logged and the caller's sync proceeds.
pub(crate) async fn reap_abandoned_spools(
    repo: &Path,
    policy_secs: u64,
    op_timeout_secs: u64,
) -> usize {
    let max_age = effective_max_age(policy_secs);

    // Cooldown FIRST: the steady state must not pay for a scan (or a
    // `git rev-parse`) on every cycle.
    {
        let attempts = attempts();
        let mut map = attempts.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(last) = map.get(repo) {
            if last.elapsed() < Duration::from_secs(3600) {
                return 0;
            }
        }
        map.insert(repo.to_path_buf(), Instant::now());
    }

    let git_dir = match resolve_git_dir(repo, op_timeout_secs).await {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!(
                "⚠️ epoch spool reaper skipped for {}: {error:#}",
                repo.display()
            );
            return 0;
        }
    };

    let mut removed = 0usize;
    let mut reclaimed = 0u64;
    for candidate in candidates(&git_dir, std::time::SystemTime::now()) {
        // A symlink (or other non-directory) named like a spool is not
        // ours to remove; leave it visible for manual inspection.
        if !candidate.is_real_directory {
            continue;
        }
        if is_leased(&candidate.path) {
            continue;
        }
        // An unreadable mtime is not proof of abandonment.
        let Some(age) = candidate.age else {
            continue;
        };
        if age < max_age {
            continue;
        }
        // Re-check: an epoch may have leased this spool during the walk.
        if is_leased(&candidate.path) {
            continue;
        }
        let bytes = dir_size_bytes(&candidate.path);
        match std::fs::remove_dir_all(&candidate.path) {
            Ok(()) => {
                removed += 1;
                reclaimed += bytes;
            }
            Err(error) => {
                // Best-effort: a concurrent removal or permission surprise
                // must never fail the sync.
                eprintln!(
                    "⚠️ cannot reap abandoned epoch spool {}: {error}",
                    candidate.path.display()
                );
            }
        }
    }
    if removed > 0 {
        eprintln!(
            "🗑️ reaped {} abandoned epoch spool(s) in {} ({} freed)",
            removed,
            repo.display(),
            human_bytes(reclaimed)
        );
    }
    removed
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        let git_dir = root.path().join(".git");
        (root, git_dir)
    }

    fn spool(git_dir: &Path, name: &str, age_secs: u64) -> PathBuf {
        let path = git_dir.join(format!("{SPOOL_PREFIX}{name}"));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("index"), b"index\n").unwrap();
        std::fs::write(path.join("retained.pack"), vec![0u8; 4096]).unwrap();
        age_mtime(&path, age_secs);
        age_mtime(&path.join("index"), age_secs);
        age_mtime(&path.join("retained.pack"), age_secs);
        path
    }

    /// Back `path`'s mtime by `secs`. A directory needs a read-only file
    /// handle (`File::open`); opening it with `write(true)` fails with
    /// EISDIR, which is why this is a helper and not inlined.
    fn age_mtime(path: &Path, secs: u64) {
        let when = std::time::SystemTime::now() - Duration::from_secs(secs);
        std::fs::File::open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[tokio::test]
    async fn abandoned_spool_is_reaped_and_fresh_one_is_kept() {
        let (root, git_dir) = init_repo();
        let old = spool(&git_dir, "old", 7200);
        let fresh = spool(&git_dir, "fresh", 0);
        assert_eq!(reap_abandoned_spools(root.path(), 3600, 60).await, 1);
        assert!(!old.exists(), "abandoned spool must be gone");
        assert!(fresh.exists(), "a young spool must survive the sweep");
    }

    #[tokio::test]
    async fn leased_spool_is_never_reaped_even_when_aged() {
        let (root, git_dir) = init_repo();
        let live = spool(&git_dir, "live", 86_400);
        let lease = SpoolLease::new(Some(live.clone()));
        assert_eq!(lease.named(), Some(live.as_path()));
        assert_eq!(reap_abandoned_spools(root.path(), 60, 60).await, 0);
        assert!(live.exists(), "a leased spool must never be deleted");
        drop(lease);
    }

    #[tokio::test]
    async fn released_spool_is_reapable_after_the_lease_drops() {
        // The regression for the 2026-10-26 leak: a spool is protected only
        // while its epoch is alive. Once released, an aged spool must go.
        let (root, git_dir) = init_repo();
        let aged = spool(&git_dir, "finished", 7200);
        {
            let _lease = SpoolLease::new(Some(aged.clone()));
            assert_eq!(reap_abandoned_spools(root.path(), 60, 60).await, 0);
            assert!(aged.exists());
        }
        // Cooldown is per repo per hour, so prove the release on a fresh repo.
        let (root2, git_dir2) = init_repo();
        let aged2 = spool(&git_dir2, "finished", 7200);
        assert_eq!(reap_abandoned_spools(root2.path(), 60, 60).await, 1);
        assert!(!aged2.exists(), "an aged, unleased spool is reclaimed");
    }

    #[tokio::test]
    async fn symlinked_spool_like_entry_is_not_followed_or_removed() {
        let (root, git_dir) = init_repo();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), b"operator data\n").unwrap();
        let link = git_dir.join(format!("{SPOOL_PREFIX}linked"));
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert_eq!(reap_abandoned_spools(root.path(), 60, 60).await, 0);
        assert!(
            outside.path().join("keep").exists(),
            "a symlink target outside the repo must never be deleted"
        );
        assert!(link.symlink_metadata().is_ok(), "the link itself stays");
    }

    #[tokio::test]
    async fn unrelated_directories_are_never_touched() {
        let (root, git_dir) = init_repo();
        let other = git_dir.join("modules");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("keep"), b"keep\n").unwrap();
        let objects = git_dir.join("objects");
        assert!(objects.exists(), "git init already created objects");
        // Even an ancient mtime is not enough: only spool-prefixed names
        // are ever candidates.
        age_mtime(&other, 86_400);
        age_mtime(&objects, 86_400);
        assert_eq!(reap_abandoned_spools(root.path(), 60, 60).await, 0);
        assert!(other.join("keep").exists(), "non-spool dirs are untouched");
        assert!(objects.exists(), "git's own dirs are untouched");
    }

    #[test]
    fn zero_age_falls_back_to_the_safe_default_not_to_disabled() {
        assert_eq!(
            effective_max_age(0),
            Duration::from_secs(DEFAULT_SPOOL_MAX_AGE_SECS)
        );
        assert_eq!(effective_max_age(120), Duration::from_secs(120));
        assert_eq!(human_bytes(875 * 1024 * 1024), "875.00 MiB");
    }

    #[test]
    fn spool_prefix_matches_the_staging_builder() {
        // Pins the contract the reaper relies on: if the staging builder
        // renames its prefix without updating this module, the leak returns
        // silently. Read the real source rather than trusting the constant.
        let source = include_str!("../sync/staged_epoch.rs");
        assert!(
            source.contains(&format!("builder.prefix(\"{SPOOL_PREFIX}\")")),
            "staged_epoch must build spools with the reaped prefix"
        );
    }
}
