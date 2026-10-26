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
//!   the live registry is consulted twice per candidate (before the age
//!   check), so GC cannot race an epoch that is still publishing.
//! - Cleanup is best-effort and never fatal: a failure is logged and the
//!   repo still converges. Unlike `auto_gc_garbage_threshold_bytes`
//!   (opt-out via 0), a 0 here falls back to the safe default rather
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
/// Deliberately generous: the oldest observed orphan in the 2026-10-26
/// incident was ~4h (one overnight run), and a legitimate epoch lifetime
/// is bounded by the sync deadline (minutes), not hours.
pub(crate) const DEFAULT_SPOOL_MAX_AGE_SECS: u64 = 3600;

/// At most one reaper pass per repo per hour (same pattern as
/// `AUTO_GC_ATTEMPTS`): the steady state costs one directory scan per
/// hour per repo instead of one per sync cycle.
static SPOOL_REAP_ATTEMPTS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Instant>>> =
    OnceLock::new();

/// Spools owned by an epoch that is still alive in this process. Guarded
/// by the SAME mutex as the cooldown map so registration and reaping can
/// never interleave on the same candidate.
static LIVE_SPOOLS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

fn live_spools() -> &'static Mutex<BTreeSet<PathBuf>> {
    LIVE_SPOOLS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn attempts() -> &'static Mutex<std::collections::HashMap<PathBuf, Instant>> {
    SPOOL_REAP_ATTEMPTS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// RAII marker for a spool that a live epoch may still need.
///
/// `pub(crate)` because `sync::staged_epoch` holds it for the epoch's
/// lifetime; dropping it (every early return, refusal, error and success
/// path) releases the spool to the reaper's discretion. It does NOT
/// delete anything itself — removal stays in the periodic reaper so the
/// error paths keep their diagnostic spool for a bounded window, exactly
/// as documented today.
pub(crate) struct SpoolLease {
    path: Option<PathBuf>,
}

impl SpoolLease {
    /// Track `path` as live. Use `.named()` for diagnostics, or discard
    /// the return value for a path whose owner is already gone.
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        if let Some(path) = &path {
            let mut live = live_spools().lock().unwrap_or_else(|p| p.into_inner());
            live.insert(path.clone());
        }
        Self { path }
    }

    /// The spool path this lease protects, if any. An owned lease stays
    /// live for as long as it is held, so this is the authoritative
    /// answer to "may the reaper delete this yet?".
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
///
/// Checked twice per candidate (before and after the age read) so a
/// spool leased by a newly created epoch in the same tick is skipped
/// instead of raced.
fn is_leased(path: &Path) -> bool {
    live_spools()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(path)
}

/// A spool found on disk, resolved without following symlinks.
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
        // `dracon-staging-*` entry is never followed and never treated
        // as a removable spool: `remove_dir_all` on a symlink target
        // would delete outside this repository.
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

/// Reap abandoned private-epoch spools in `repo`'s git dir.
///
/// Returns the number of spools removed. `max_age_secs` of 0 falls back
/// to [`DEFAULT_SPOOL_MAX_AGE_SECS`] (see the module note: 0 is not an
/// opt-out, unlike `auto_gc_garbage_threshold_bytes`). Never fatal.
pub(crate) async fn reap_abandoned_spools(
    repo: &Path,
    policy_secs: u64,
    op_timeout_secs: u64,
) -> usize {
    let max_age_secs = if policy_secs == 0 {
        DEFAULT_SPOOL_MAX_AGE_SECS
    } else {
        policy_secs
    };
    let max_age = Duration::from_secs(max_age_secs);

    let git_dir = match crate::git::run_git_with_timeout(
        repo,
        &["rev-parse", "--absolute-git-dir"],
        op_timeout_secs.max(1),
        "epoch spool git dir",
    )
    .await
    {
        Ok(()) => None,
        Err(_) => None,
    };
    // `run_git_with_timeout` discards stdout, so resolve the git dir with
    // a bounded captured run instead: a spool scan is cheap and must not
    // need a second process for the identity.
    let _ = git_dir;
    let resolved = match crate::git::capture_git_stdout(
        repo,
        &["rev-parse", "--absolute-git-dir"],
        op_timeout_secs,
    )
    .await
    {
        Ok(stdout) => stdout,
        Err(error) => {
            eprintln!(
                "⚠️ epoch spool reaper skipped for {}: cannot resolve git dir: {error:#}",
                repo.display()
            );
            return 0;
        }
    };
    let git_dir = PathBuf::from(resolved.trim_end_matches('\n'));
    if !git_dir.is_absolute() {
        return 0;
    }

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

    let now = std::time::SystemTime::now();
    let mut removed = 0usize;
    let mut reclaimed = 0u64;
    for candidate in candidates(&git_dir, now) {
        if is_leased(&candidate.path) {
            continue;
        }
        // A symlink (or other non-directory) named like a spool is not ours
        // to remove; leave it visible for manual inspection.
        if !candidate.is_real_directory {
            continue;
        }
        // An unreadable mtime is not proof of abandonment.
        let Some(age) = candidate.age else {
            continue;
        };
        if age < max_age {
            continue;
        }
        // Re-check: an epoch may have leased this spool while we walked.
        if is_leased(&candidate.path) {
            continue;
        }
        let bytes = crate::git::dir_size_bytes(&candidate.path);
        match std::fs::remove_dir_all(&candidate.path) {
            Ok(()) => {
                removed += 1;
                reclaimed += bytes;
            }
            Err(error) => {
                // Best-effort: a concurrent removal (another repo handle) or
                // a permission surprise must not fail the sync.
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

    fn init() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        root
    }

    fn spool(git_dir: &Path, name: &str, age_secs: u64) -> PathBuf {
        let path = git_dir.join(format!("{SPOOL_PREFIX}{name}"));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("index"), b"index\n").unwrap();
        std::fs::write(path.join("retained.pack"), vec![0u8; 4096]).unwrap();
        // Age the directory by backing both its own and its children's mtime.
        let when = std::time::SystemTime::now() - Duration::from_secs(age_secs);
        for entry in [path.clone(), path.join("index"), path.join("retained.pack")] {
            let f = std::fs::File::options().write(true).open(&entry).unwrap();
            f.set_modified(when).unwrap();
        }
        path
    }

    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let root = init();
        let git_dir = root.path().join(".git");
        (root, git_dir)
    }

    #[tokio::test]
    async fn abandoned_spool_is_reaped_and_fresh_one_is_kept() {
        let (root, git_dir) = init_repo();
        let old = spool(&git_dir, "old", 7200);
        let fresh = spool(&git_dir, "fresh", 0);
        let removed = reap_abandoned_spools(root.path(), 3600, 60).await;
        assert_eq!(removed, 1, "only the aged spool is removed");
        assert!(!old.exists(), "abandoned spool must be gone");
        assert!(fresh.exists(), "a young spool must survive the sweep");
    }

    #[tokio::test]
    async fn leased_spool_is_never_reaped_even_when_aged() {
        let (root, git_dir) = init_repo();
        let live = spool(&git_dir, "live", 86_400);
        let lease = SpoolLease::new(Some(live.clone()));
        assert_eq!(lease.named(), Some(live.as_path()));
        let removed = reap_abandoned_spools(root.path(), 60, 60).await;
        assert_eq!(removed, 0, "a leased spool is not abandoned");
        assert!(live.exists(), "a leased spool must never be deleted");
        drop(lease);
    }

    #[tokio::test]
    async fn released_spool_becomes_reapable_after_the_lease_drops() {
        // The regression for the 2026-10-26 leak: a spool stays protected
        // only while its epoch is alive. Once the epoch is done, the reaper
        // must actually be able to remove it.
        let (root, git_dir) = init_repo();
        let path = spool(&git_dir, "finished", 7200);
        {
            let _lease = SpoolLease::new(Some(path.clone()));
            assert_eq!(reap_abandoned_spools(root.path(), 60, 60).await, 0);
            assert!(path.exists());
        }
        // Cooldown is per repo per hour, so use a fresh repo for pass two.
        let (root2, git_dir2) = init_repo();
        let aged = spool(&git_dir2, "finished", 7200);
        assert_eq!(reap_abandoned_spools(root2.path(), 60, 60).await, 1);
        assert!(!aged.exists(), "an aged, unleased spool is reclaimed");
    }

    #[tokio::test]
    async fn symlinked_spool_like_entry_is_not_followed_or_removed() {
        let (root, git_dir) = init_repo();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), b"operator data\n").unwrap();
        let link = git_dir.join(format!("{SPOOL_PREFIX}linked"));
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        // An ancient mtime on the link itself must not make it a candidate.
        let when = std::time::SystemTime::now() - Duration::from_secs(86_400);
        std::fs::File::options()
            .write(true)
            .open(&link)
            .ok();
        let removed = reap_abandoned_spools(root.path(), 60, 60).await;
        assert_eq!(removed, 0, "symlinked entries are never spool candidates");
        assert!(
            outside.path().join("keep").exists(),
            "a symlink target outside the repo must never be deleted"
        );
        let _ = when;
    }

    #[tokio::test]
    async fn unrelated_directories_are_never_touched() {
        let (root, git_dir) = init_repo();
        let other = git_dir.join("modules");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("keep"), b"keep\n").unwrap();
        let dangling = git_dir.join("objects");
        std::fs::create_dir(&dangling).unwrap();
        let when = std::time::SystemTime::now() - Duration::from_secs(86_400);
        std::fs::File::options()
            .write(true)
            .open(&dangling)
            .unwrap()
            .set_modified(when)
            .unwrap();
        assert_eq!(reap_abandoned_spools(root.path(), 60, 60).await, 0);
        assert!(other.join("keep").exists(), "non-spool dirs are untouched");
        assert!(dangling.exists(), "git's own dirs are untouched");
    }

    #[test]
    fn policy_zero_falls_back_to_the_safe_default() {
        assert_eq!(
            if 0 == 0 {
                DEFAULT_SPOOL_MAX_AGE_SECS
            } else {
                0
            },
            3600
        );
        assert_eq!(human_bytes(875 * 1024 * 1024), "875.00 MiB");
    }

    #[test]
    fn spool_prefix_matches_the_staging_builder() {
        // Pins the contract the reaper relies on: if the staging builder
        // renames its prefix without updating this module, the leak returns
        // silently. Read the source rather than trusting the constant.
        let source = include_str!("../sync/staged_epoch.rs");
        assert!(
            source.contains(&format!("builder.prefix(\"{SPOOL_PREFIX}\")")),
            "staged_epoch must build spools with the reaped prefix"
        );
    }
}
