//! Cross-process ownership for private epoch recovery directories.
//!
//! An old mtime is not evidence that a producer has finished. A producer holds
//! the same kernel lock from directory initialization until epoch settlement.
//! A reclaimer never creates a missing lock for a legacy directory.

use anyhow::{ensure, Context, Result};
use std::collections::HashMap;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

const OWNER_FILE: &str = "reclaim-owner.lock";
const SETTLED_FILE: &str = "reclaim-settled.json";
const CLOSING_FILE: &str = "reclaim-closing.json";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnerIdentity {
    version: u8,
    directory_device: u64,
    directory_inode: u64,
    lock_device: u64,
    lock_inode: u64,
}

impl OwnerIdentity {
    fn of(directory: &Metadata, lock: &Metadata) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                version: 1,
                directory_device: directory.dev(),
                directory_inode: directory.ino(),
                lock_device: lock.dev(),
                lock_inode: lock.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (directory, lock);
            anyhow::bail!("qualified spool identities require Unix")
        }
    }
}

type Scopes = HashMap<PathBuf, Weak<super::producer::EpochProducerScope>>;
fn scopes() -> &'static Mutex<Scopes> {
    static SCOPES: OnceLock<Mutex<Scopes>> = OnceLock::new();
    SCOPES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn producer_scope(
    path: &Path,
) -> Result<Option<Arc<super::producer::EpochProducerScope>>> {
    ensure!(
        !path.join(SETTLED_FILE).exists() && !path.join(CLOSING_FILE).exists(),
        "closed spool cannot resume production"
    );
    Ok(scopes()
        .lock()
        .map_err(|_| anyhow::anyhow!("spool scope registry poisoned"))?
        .get(path)
        .and_then(Weak::upgrade))
}

pub(crate) struct SpoolLease {
    path: Option<PathBuf>,
    owner: Option<File>,
    identity: Option<OwnerIdentity>,
    scope: Option<Arc<super::producer::EpochProducerScope>>,
}

impl SpoolLease {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            owner: None,
            identity: None,
            scope: None,
        }
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
        ensure!(
            same_identity(&directory, &std::fs::symlink_metadata(path)?),
            "spool directory replaced"
        );
        ensure!(
            same_identity(
                &owner.metadata()?,
                &std::fs::symlink_metadata(path.join(OWNER_FILE))?
            ),
            "spool lock replaced"
        );
        let identity = OwnerIdentity::of(&directory, &owner.metadata()?)?;
        Ok(Self {
            path: Some(path.to_path_buf()),
            owner: Some(owner),
            identity: Some(identity),
            scope: None,
        })
    }

    pub(crate) fn named(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Register only once, before the first private-index/pack child. The map
    /// transfers a scope handle, never ownership authority: the kernel lock and
    /// identity-bound marker remain the cross-process protocol.
    pub(crate) fn begin_producers(&mut self, runtime: Duration) -> Result<()> {
        ensure!(
            self.scope.is_none() && self.owner.is_some(),
            "producer scope already initialized or unowned"
        );
        let path = self.path.as_ref().context("no spool path")?;
        ensure!(
            !path.join(SETTLED_FILE).exists() && !path.join(CLOSING_FILE).exists(),
            "closed spool cannot resume production"
        );
        let identity = self.identity.clone().context("no producer identity")?;
        let scope = Arc::new(super::producer::EpochProducerScope::new(runtime)?.bind(identity));
        let mut registry = scopes()
            .lock()
            .map_err(|_| anyhow::anyhow!("spool scope registry poisoned"))?;
        ensure!(
            registry.get(path).and_then(Weak::upgrade).is_none(),
            "producer scope already registered"
        );
        registry.insert(path.clone(), Arc::downgrade(&scope));
        self.scope = Some(scope);
        Ok(())
    }

    /// Call outside the producer timeout, after that future can no longer spawn.
    /// An uncertain/pending family never creates a marker. Failed attempts are
    /// not resumable production: subsequent dispatch against a sealed path fails.
    pub(crate) async fn settle_for_reclaim(&mut self, deadline: Duration) -> Result<()> {
        let scope = self
            .scope
            .take()
            .context("producer containment was not established")?;
        let path = self.path.as_ref().context("no spool path")?;
        let mut closing_options = owner_options();
        closing_options.create_new(true);
        let mut closing = closing_options.open(path.join(CLOSING_FILE))?;
        closing.write_all(&serde_json::to_vec(
            self.identity.as_ref().context("no spool identity")?,
        )?)?;
        closing.sync_all()?;
        File::open(path)?.sync_all()?;
        if let Some(path) = &self.path {
            scopes()
                .lock()
                .map_err(|_| anyhow::anyhow!("spool scope registry poisoned"))?
                .remove(path);
        }
        let scope = Arc::try_unwrap(scope)
            .map_err(|_| anyhow::anyhow!("producer scope still borrowed; retain spool"))?;
        let proof = scope.settle(deadline).await?;
        self.seal(proof)?;
        self.release_for_reclaim();
        Ok(())
    }

    fn seal(&mut self, proof: super::producer::SettledProducer) -> Result<()> {
        let path = self.path.as_ref().context("no spool path")?;
        let identity = self.identity.as_ref().context("no spool identity")?;
        ensure!(
            proof.into_identity().as_ref() == Some(identity),
            "settlement proof belongs to a different producer"
        );
        let directory = std::fs::symlink_metadata(path)?;
        ensure!(directory.is_dir(), "spool directory replaced");
        let owner = self
            .owner
            .as_ref()
            .context("producer no longer owns spool")?;
        let named = std::fs::symlink_metadata(path.join(OWNER_FILE))?;
        ensure!(
            same_identity(&owner.metadata()?, &named)
                && OwnerIdentity::of(&directory, &named)? == *identity,
            "producer identity changed before sealing"
        );
        let mut options = owner_options();
        options.create_new(true);
        let mut marker = options.open(path.join(SETTLED_FILE))?;
        marker.write_all(&serde_json::to_vec(identity)?)?;
        marker.sync_all()?;
        File::open(path)?.sync_all()?;
        Ok(())
    }

    /// Hand ownership back to the kernel before invoking the ordinary reclaimer;
    /// independently opening a lock we still hold would self-contend.
    pub(crate) fn release_for_reclaim(&mut self) {
        self.owner = None;
    }
}

impl Drop for SpoolLease {
    fn drop(&mut self) {
        if self.scope.is_some() {
            if let (Some(path), Ok(mut registry)) = (&self.path, scopes().lock()) {
                registry.remove(path);
            }
        }
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
        let name = spool
            .file_name()
            .and_then(|n| n.to_str())
            .context("invalid spool name")?;
        ensure!(
            name.starts_with("dracon-staging-") || name.starts_with("dracon-epoch-message-"),
            "not a spool"
        );
        ensure!(
            spool.parent().context("no spool parent")?.canonicalize()? == gitdir.canonicalize()?,
            "spool outside gitdir"
        );
        let directory = std::fs::symlink_metadata(spool)?;
        ensure!(directory.is_dir(), "spool must not be a symlink");
        // Open only. Missing legacy metadata is uncertainty, not abandonment.
        let before = std::fs::symlink_metadata(spool.join(OWNER_FILE))?;
        validate_lock(&before)?;
        let file = owner_options().open(spool.join(OWNER_FILE))?;
        validate_lock(&file.metadata()?)?;
        ensure!(
            same_identity(&before, &file.metadata()?),
            "spool lock changed during open"
        );
        file.try_lock().context("spool is still owned")?;
        let held = Self {
            lock: file.metadata()?,
            _file: file,
            spool: spool.to_path_buf(),
            directory,
        };
        ensure!(
            held.still_owned(),
            "spool identity changed while acquiring ownership"
        );
        Ok(held)
    }

    /// An unlocked crash-abandoned producer is not settled. Never manufacture
    /// this marker in the sweeper, including for legacy directories.
    pub(super) fn is_settled(&self) -> bool {
        let checked = || -> Result<bool> {
            ensure!(self.still_owned(), "spool identity changed");
            let path = self.spool.join(SETTLED_FILE);
            let named = std::fs::symlink_metadata(&path)?;
            validate_lock(&named)?;
            let marker = owner_options().open(&path)?;
            ensure!(
                same_identity(&named, &marker.metadata()?),
                "settled marker replaced"
            );
            let mut bytes = Vec::new();
            marker.take(4097).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 4096, "oversized settled marker");
            let identity: OwnerIdentity = serde_json::from_slice(&bytes)?;
            Ok(identity == OwnerIdentity::of(&self.directory, &self.lock)?)
        };
        checked().unwrap_or(false)
    }

    pub(super) fn still_owned(&self) -> bool {
        let Ok(directory) = std::fs::symlink_metadata(&self.spool) else {
            return false;
        };
        let Ok(lock) = std::fs::symlink_metadata(self.spool.join(OWNER_FILE)) else {
            return false;
        };
        directory.is_dir()
            && validate_lock(&lock).is_ok()
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
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    options
}

fn validate_lock(metadata: &Metadata) -> Result<()> {
    ensure!(metadata.is_file(), "spool lock must be a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.nlink() == 1
                && metadata.mode() & 0o022 == 0,
            "spool protocol file must be operator-owned, non-shared and not externally writable"
        );
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
    fn unlocked_without_settlement_is_uncertain_and_marker_is_identity_bound() {
        let (_root, gitdir, spool) = fixture();
        let mut producer = SpoolLease::acquire(&spool).unwrap();
        producer.release_for_reclaim();
        let held = ReclaimOwner::acquire(&gitdir, &spool).unwrap();
        assert!(
            !held.is_settled(),
            "process/FD exit alone never proves family completion"
        );
        drop(held);
        let (_other_root, other_gitdir, other_spool) = fixture();
        let mut other = SpoolLease::acquire(&other_spool).unwrap();
        // No child exists in this ownership-protocol fixture. Real family
        // settlement is separately exercised by the producer host fixture.
        let proof =
            super::super::producer::SettledProducer::empty_fixture(other.identity.clone().unwrap());
        other.seal(proof).unwrap();
        assert!(
            producer_scope(&other_spool).is_err(),
            "sealed production must not resume"
        );
        other.release_for_reclaim();
        assert!(ReclaimOwner::acquire(&other_gitdir, &other_spool)
            .unwrap()
            .is_settled());
        std::fs::copy(other_spool.join(SETTLED_FILE), spool.join(SETTLED_FILE)).unwrap();
        assert!(
            !ReclaimOwner::acquire(&gitdir, &spool).unwrap().is_settled(),
            "copied marker must not authorize a different spool"
        );
    }

    #[test]
    fn wrong_producer_proof_and_closing_marker_cannot_seal_or_restart() {
        let (_root, _gitdir, spool) = fixture();
        let mut producer = SpoolLease::acquire(&spool).unwrap();
        let (_other_root, _other_gitdir, other_spool) = fixture();
        let other = SpoolLease::acquire(&other_spool).unwrap();
        let wrong =
            super::super::producer::SettledProducer::empty_fixture(other.identity.clone().unwrap());
        assert!(producer.seal(wrong).is_err());
        assert!(!spool.join(SETTLED_FILE).exists());
        std::fs::write(spool.join(CLOSING_FILE), b"unsettled closing attempt").unwrap();
        assert!(producer_scope(&spool).is_err());
        assert!(producer.begin_producers(Duration::from_secs(5)).is_err());
        assert!(!spool.join(SETTLED_FILE).exists());
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
        let Some(path) = std::env::var_os("DRACON_SPOOL_OWNER_PROBE") else {
            return;
        };
        let path = PathBuf::from(path);
        let _lease = SpoolLease::acquire(&path).unwrap();
        std::fs::write(path.join("probe-ready"), b"held").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.join("probe-exit").exists() {
            assert!(
                Instant::now() < deadline,
                "parent did not release child probe"
            );
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
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "git::epoch_spool::ownership::tests::child_holds_spool_owner_probe",
                ])
                .env("DRACON_SPOOL_OWNER_PROBE", &spool)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !spool.join("probe-ready").exists() {
            assert!(Instant::now() < deadline, "child did not acquire ownership");
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited without ready signal"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        File::open(&spool)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        assert!(
            ReclaimOwner::acquire(&gitdir, &spool).is_err(),
            "old age must not override a live owner"
        );
        std::fs::write(spool.join("probe-exit"), b"exit").unwrap();
        while child.0.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "child did not settle");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ReclaimOwner::acquire(&gitdir, &spool).is_ok(),
            "process exit must release kernel ownership"
        );
    }
}
