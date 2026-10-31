//! Cross-process ownership for private epoch recovery directories.
//!
//! An old mtime is not evidence that a producer has finished. A producer holds
//! the same kernel lock from directory initialization until epoch settlement.
//! A reclaimer never creates a missing lock for a legacy directory.

use anyhow::{ensure, Context, Result};
use std::fs::{File, Metadata, OpenOptions};
use std::path::{Path, PathBuf};

const OWNER_FILE: &str = "reclaim-owner.lock";

pub(crate) struct SpoolLease {
    path: Option<PathBuf>,
    owner: Option<File>,
}

impl SpoolLease {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self { path, owner: None }
    }

    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        let directory = std::fs::symlink_metadata(path)?;
        ensure!(directory.is_dir(), "spool must be a real directory");
        let mut options = owner_options();
        options.create_new(true);
        let owner = options.open(path.join(OWNER_FILE))?;
        validate_lock(&owner.metadata()?)?;
        owner.try_lock().context("spool owner already held")?;
        // If another reclaimer won the lock before initialization, or the
        // directory was replaced, do not continue writing through a stale path.
        ensure!(same_identity(&directory, &std::fs::symlink_metadata(path)?), "spool directory replaced");
        ensure!(same_identity(&owner.metadata()?, &std::fs::symlink_metadata(path.join(OWNER_FILE))?), "spool lock replaced");
        Ok(Self { path: Some(path.to_path_buf()), owner: Some(owner) })
    }

    pub(crate) fn named(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The settled owner has finished writing and terminated owned helpers.
    /// Hand ownership back to the kernel before invoking the ordinary reclaimer;
    /// independently opening a lock we still hold would self-contend.
    pub(crate) fn release_for_reclaim(&mut self) {
        self.owner = None;
    }
}

impl Drop for SpoolLease {
    fn drop(&mut self) {
        self.release_for_reclaim();
    }
}

/// Holds ownership and the identities against which final deletion is checked.
pub(super) struct ReclaimOwner {
    _file: File,
    spool: PathBuf,
    directory: Metadata,
    lock: Metadata,
}

impl ReclaimOwner {
    pub(super) fn acquire(gitdir: &Path, spool: &Path) -> Result<Self> {
        let name = spool.file_name().and_then(|n| n.to_str()).context("invalid spool name")?;
        ensure!(name.starts_with("dracon-staging-") || name.starts_with("dracon-epoch-message-"), "not a spool");
        ensure!(spool.parent().context("no spool parent")?.canonicalize()? == gitdir.canonicalize()?, "spool outside gitdir");
        let directory = std::fs::symlink_metadata(spool)?;
        ensure!(directory.is_dir(), "spool must not be a symlink");
        // Open only. Missing legacy metadata is uncertainty, not abandonment.
        let before = std::fs::symlink_metadata(spool.join(OWNER_FILE))?;
        validate_lock(&before)?;
        let file = owner_options().open(spool.join(OWNER_FILE))?;
        validate_lock(&file.metadata()?)?;
        ensure!(same_identity(&before, &file.metadata()?), "spool lock changed during open");
        file.try_lock().context("spool is still owned")?;
        let held = Self { lock: file.metadata()?, _file: file, spool: spool.to_path_buf(), directory };
        ensure!(held.still_owned(), "spool identity changed while acquiring ownership");
        Ok(held)
    }

    pub(super) fn still_owned(&self) -> bool {
        let Ok(directory) = std::fs::symlink_metadata(&self.spool) else { return false };
        let Ok(lock) = std::fs::symlink_metadata(self.spool.join(OWNER_FILE)) else { return false };
        directory.is_dir() && lock.is_file()
            && same_identity(&self.directory, &directory)
            && same_identity(&self.lock, &lock)
    }
}

fn owner_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // NONBLOCK prevents an unexpected FIFO from hanging before validation.
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    options
}

fn validate_lock(metadata: &Metadata) -> Result<()> {
    ensure!(metadata.is_file(), "spool lock must be a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(metadata.uid() == unsafe { libc::geteuid() } && metadata.nlink() == 1,
            "spool lock must be operator-owned and not hard-linked");
    }
    Ok(())
}

fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        // No qualified identity protocol on this platform: preserve rather than
        // treat timestamps or size as a stable filesystem identity.
        let _ = (left, right);
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let gitdir = root.path().join("gitdir");
        let spool = gitdir.join("dracon-staging-locked");
        std::fs::create_dir_all(&spool).unwrap();
        (root, gitdir, spool)
    }

    #[test]
    fn legacy_owner_metadata_is_not_manufactured() {
        let (_root, gitdir, spool) = fixture();
        assert!(ReclaimOwner::acquire(&gitdir, &spool).is_err());
        assert!(!spool.join(OWNER_FILE).exists());
    }

    #[test]
    fn settled_owner_handoff_and_two_reclaimers_are_exclusive() {
        let (_root, gitdir, spool) = fixture();
        let mut producer = SpoolLease::acquire(&spool).unwrap();
        assert!(ReclaimOwner::acquire(&gitdir, &spool).is_err());
        producer.release_for_reclaim();
        let first = ReclaimOwner::acquire(&gitdir, &spool).unwrap();
        assert!(first.still_owned());
        assert!(ReclaimOwner::acquire(&gitdir, &spool).is_err());
        drop(first);
        assert!(ReclaimOwner::acquire(&gitdir, &spool).is_ok());
    }

    #[test]
    fn replaced_lock_and_symlink_directory_fail_closed() {
        let (root, gitdir, spool) = fixture();
        let producer = SpoolLease::acquire(&spool).unwrap();
        drop(producer);
        let held = ReclaimOwner::acquire(&gitdir, &spool).unwrap();
        std::fs::rename(spool.join(OWNER_FILE), spool.join("old-owner.lock")).unwrap();
        std::fs::write(spool.join(OWNER_FILE), b"replacement").unwrap();
        assert!(!held.still_owned());
        let alias = root.path().join("dracon-staging-alias");
        std::os::unix::fs::symlink(&spool, &alias).unwrap();
        assert!(ReclaimOwner::acquire(root.path(), &alias).is_err());
        assert!(ReclaimOwner::acquire(root.path(), &spool).is_err());
        assert!(ReclaimOwner::acquire(&gitdir, &spool.join("nested")).is_err());
    }

    #[test]
    fn child_holds_spool_owner_probe() {
        let Some(path) = std::env::var_os("DRACON_SPOOL_OWNER_PROBE") else { return };
        let path = PathBuf::from(path);
        let _lease = SpoolLease::acquire(&path).unwrap();
        std::fs::write(path.join("probe-ready"), b"held").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.join("probe-exit").exists() {
            assert!(Instant::now() < deadline, "parent did not release child probe");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn old_but_live_child_is_excluded_then_exit_releases_ownership() {
        let (_root, gitdir, spool) = fixture();
        let mut child = OwnedChild(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "git::epoch_spool::ownership::tests::child_holds_spool_owner_probe"])
            .env("DRACON_SPOOL_OWNER_PROBE", &spool)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit())
            .spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !spool.join("probe-ready").exists() {
            assert!(Instant::now() < deadline, "child did not acquire ownership");
            assert!(child.0.try_wait().unwrap().is_none(), "child exited without ready signal");
            std::thread::sleep(Duration::from_millis(10));
        }
        File::open(&spool).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();
        assert!(ReclaimOwner::acquire(&gitdir, &spool).is_err(), "old age must not override a live owner");
        std::fs::write(spool.join("probe-exit"), b"exit").unwrap();
        while child.0.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "child did not settle");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ReclaimOwner::acquire(&gitdir, &spool).is_ok(), "process exit must release kernel ownership");
    }
}
