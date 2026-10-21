//! Kernel-backed ownership shared by daemon and one-shot sync processes.
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

/// Keep this file open for the entire repository operation. Never unlink it:
/// replacing its inode would let another process bypass an existing holder.
pub(crate) fn try_acquire(repo: &Path) -> Result<Option<File>> {
    let gitdir = super::resolve_gitdir_direct(repo).context("cannot resolve sync lease gitdir")?;
    let common = super::resolve_commondir_direct(&gitdir);
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
        .open(common.join("dracon-sync.operation.lock"))
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
        let acquired = try_acquire(Path::new(&repo)).unwrap().is_some();
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
        assert!(repo.path().join(".git/dracon-sync.operation.lock").exists());
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
        let held = try_acquire(&repo).unwrap().unwrap();
        assert!(try_acquire(&linked).unwrap().is_none());
        drop(held);
        let lock = repo.join(".git/dracon-sync.operation.lock");
        std::fs::remove_file(&lock).unwrap();
        std::os::unix::fs::symlink(root.path().join("other"), &lock).unwrap();
        assert!(try_acquire(&repo).is_err());
        assert!(!root.path().join("other").exists());
    }
}
