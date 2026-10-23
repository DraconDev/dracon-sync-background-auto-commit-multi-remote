//! Kernel-backed ownership shared by daemon and one-shot sync processes.
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum LeaseKind {
    Local,
    Publication,
}

impl LeaseKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Local => "dracon-sync.operation-lease",
            Self::Publication => "dracon-sync.publication-lease",
        }
    }
}

/// Canonical identity shared by aliases and all linked worktrees. Registry
/// keys and kernel leases must agree; checkout paths alone are not identities.
pub(crate) fn repository_identity(repo: &Path) -> Result<PathBuf> {
    let gitdir = super::resolve_gitdir_direct(repo).context("cannot resolve sync lease gitdir")?;
    super::resolve_commondir_direct(&gitdir)
        .canonicalize()
        .context("cannot resolve repository common directory")
}

/// Exclusive local mutation ownership. Never unlink its persistent lock file:
/// replacing its inode would let another process bypass an existing holder.
pub(crate) fn try_acquire(repo: &Path) -> Result<Option<File>> {
    try_acquire_kind(repo, LeaseKind::Local)
}

/// One publication owner per common directory, independent of local commits.
/// Keep this guard until owned publication work has terminated. Code needing
/// both leases takes publication BEFORE local, or defers without waiting.
pub(crate) fn try_acquire_publication(repo: &Path) -> Result<Option<File>> {
    try_acquire_kind(repo, LeaseKind::Publication)
}

fn try_acquire_kind(repo: &Path, kind: LeaseKind) -> Result<Option<File>> {
    let common = repository_identity(repo)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options
        .open(common.join(kind.filename()))
        .context("cannot open repository sync lease")?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "sync lease must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.nlink() == 1,
            "sync lease must be operator-owned and not hard-linked"
        );
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => {
            Err(error).context("cannot acquire repository sync lease")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_process_lease_probe() {
        let Some(repo) = std::env::var_os("DRACON_SYNC_LEASE_TEST_REPO") else {
            return;
        };
        let kind = match std::env::var("DRACON_SYNC_LEASE_TEST_KIND").as_deref() {
            Ok("publication") => LeaseKind::Publication,
            _ => LeaseKind::Local,
        };
        let acquired = try_acquire_kind(Path::new(&repo), kind).unwrap().is_some();
        assert_eq!(
            acquired,
            std::env::var("DRACON_SYNC_LEASE_TEST_EXPECT").unwrap() == "free"
        );
    }

    #[test]
    fn lease_excludes_other_processes_and_releases_without_stale_lock_cleanup() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let probe = |expected| {
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "git::sync_lease::tests::child_process_lease_probe"
                ])
                .env("DRACON_SYNC_LEASE_TEST_REPO", repo.path())
                .env("DRACON_SYNC_LEASE_TEST_EXPECT", expected)
                .status()
                .unwrap()
                .success());
        };
        let held = try_acquire(repo.path()).unwrap().unwrap();
        assert!(try_acquire(repo.path()).unwrap().is_none());
        probe("busy");
        drop(held);
        probe("free");
        assert!(repo
            .path()
            .join(".git/dracon-sync.operation-lease")
            .exists());
    }

    #[test]
    fn publication_excludes_publishers_but_not_local_commits_across_processes() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let probe = |kind, expected| {
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "git::sync_lease::tests::child_process_lease_probe",
                ])
                .env("DRACON_SYNC_LEASE_TEST_REPO", repo.path())
                .env("DRACON_SYNC_LEASE_TEST_KIND", kind)
                .env("DRACON_SYNC_LEASE_TEST_EXPECT", expected)
                .status()
                .unwrap()
                .success());
        };
        let publisher = try_acquire_publication(repo.path()).unwrap().unwrap();
        assert!(try_acquire_publication(repo.path()).unwrap().is_none());
        probe("publication", "busy");
        probe("local", "free");
        let local = try_acquire(repo.path()).unwrap().unwrap();
        probe("local", "busy");
        drop(publisher);
        // Publication admission does not require local ownership to be free.
        probe("publication", "free");
        drop(local);
        assert!(repo
            .path()
            .join(".git/dracon-sync.publication-lease")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_worktrees_share_ownership_and_symlink_locks_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let linked = root.path().join("linked");
        let gitdir = repo.join(".git/worktrees/linked");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::create_dir(&linked).unwrap();
        std::fs::write(linked.join(".git"), format!("gitdir: {}", gitdir.display())).unwrap();
        std::fs::write(gitdir.join("commondir"), "../..").unwrap();
        assert_eq!(
            repository_identity(&repo).unwrap(),
            repository_identity(&linked).unwrap()
        );
        let held = try_acquire(&repo).unwrap().unwrap();
        let publisher = try_acquire_publication(&linked).unwrap().unwrap();
        assert!(try_acquire(&linked).unwrap().is_none());
        assert!(try_acquire_publication(&repo).unwrap().is_none());
        drop(publisher);
        drop(held);
        let lock = repo.join(".git/dracon-sync.operation-lease");
        std::fs::remove_file(&lock).unwrap();
        std::os::unix::fs::symlink(root.path().join("other"), &lock).unwrap();
        assert!(try_acquire(&repo).is_err());
        assert!(!root.path().join("other").exists());
        let publication = repo.join(".git/dracon-sync.publication-lease");
        std::fs::remove_file(&publication).unwrap();
        std::os::unix::fs::symlink(root.path().join("other-publication"), &publication).unwrap();
        assert!(try_acquire_publication(&repo).is_err());
        assert!(!root.path().join("other-publication").exists());
    }
}
