//! Cooperative metadata fence v1, interoperable with Warden/editor workers.
//! Keep this small OS protocol local so standalone utilities do not gain a
//! build/release dependency on each other. Cross-binary qualification is still
//! required; using a fence in the daemon alone is not all-writer coverage.
use anyhow::{ensure, Context, Result};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;

const FILE_NAME: &str = "dracon-canonical-writer-v1.lock";
const INHERITED_FD: &str = "DRACON_CANONICAL_WRITER_FD_V1";

pub(crate) struct WriteFence {
    file: File,
    path: PathBuf,
}
impl WriteFence {
    /// Acquire before ANY canonical target lookup. No blocking thread survives
    /// cancellation: all waits are nonblocking flock plus cancellable sleep.
    pub(crate) async fn acquire_until(repo: &Path, deadline: Instant) -> Result<Self> {
        ensure!(Instant::now() < deadline, "canonical writer fence deadline expired");
        let gitdir = super::path_gitdir(repo).context("writer fence requires a worktree Git directory")?;
        let path = gitdir.join(FILE_NAME);
        let file = if let Some(raw) = std::env::var_os(INHERITED_FD) {
            let fd: i32 = raw.to_str().context("non-text writer fence descriptor")?.parse()?;
            ensure!(fd >= 3, "invalid inherited writer fence descriptor");
            let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
            ensure!(duplicate >= 0, "cannot duplicate inherited writer fence descriptor");
            unsafe { File::from_raw_fd(duplicate) }
        } else {
            OpenOptions::new().read(true).write(true).create(true).truncate(false)
                .mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path).context("open canonical writer fence")?
        };
        let fence = Self { file, path };
        fence.verify()?;
        loop {
            ensure!(Instant::now() < deadline, "canonical writer fence deadline expired");
            if unsafe { libc::flock(fence.file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                fence.verify()?;
                return Ok(fence);
            }
            let error = std::io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(libc::EWOULDBLOCK) | Some(libc::EINTR)) {
                return Err(error).context("acquire canonical writer fence");
            }
            tokio::time::sleep(Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now()))).await;
        }
    }
    pub(crate) fn verify(&self) -> Result<()> {
        let held = self.file.metadata()?;
        let named = std::fs::symlink_metadata(&self.path)?;
        ensure!(held.is_file() && named.is_file(), "writer fence is not a regular file");
        ensure!(held.uid() == unsafe { libc::geteuid() } && held.nlink() == 1
            && held.mode() & 0o077 == 0, "unsafe canonical writer fence ownership/mode/links");
        ensure!(held.dev() == named.dev() && held.ino() == named.ino(), "canonical writer fence was replaced");
        Ok(())
    }
    /// Preserve exactly this descriptor in the owned helper, never clear its
    /// parent's CLOEXEC bit (which would leak locks into unrelated children).
    pub(crate) fn inherit_into(&self, command: &mut tokio::process::Command) {
        let fd = self.file.as_raw_fd();
        command.env(INHERITED_FD, fd.to_string());
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".git")).unwrap();
        d
    }
    #[tokio::test]
    async fn deadline_refuses_then_drop_recovers_without_lock_stealing() {
        let a = repo(); let b = repo();
        let held = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        let _independent = WriteFence::acquire_until(b.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        assert!(WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_millis(20)).await.is_err());
        let inode = held.file.metadata().unwrap().ino(); drop(held);
        let next = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        assert_eq!(next.file.metadata().unwrap().ino(), inode);
    }
    #[tokio::test]
    async fn inherited_ofd_is_held_but_parent_descriptor_remains_cloexec() {
        let a = repo();
        let held = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        let mut command = tokio::process::Command::new("python3");
        command.args(["-c", "import os,fcntl;fd=int(os.environ['DRACON_CANONICAL_WRITER_FD_V1']);fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)"]);
        held.inherit_into(&mut command);
        assert!(command.status().await.unwrap().success());
        assert_ne!(unsafe { libc::fcntl(held.file.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
    }
    #[tokio::test]
    async fn cancelled_waiter_does_not_survive_or_later_take_the_fence() {
        let a = repo();
        let held = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        let inode = held.file.metadata().unwrap().ino();
        // Outer cancellation, not the acquisition's own much later deadline.
        assert!(tokio::time::timeout(
            Duration::from_millis(30),
            WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(60)),
        ).await.is_err());
        drop(held);
        let recovered = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        assert_eq!(recovered.file.metadata().unwrap().ino(), inode);
        assert!(WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_millis(30)).await.is_err());
        drop(recovered);
        // Reacquisition has no stranded waiter/descriptor or lock-file theft.
        let final_owner = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        assert_eq!(final_owner.file.metadata().unwrap().ino(), inode);
    }
    #[tokio::test]
    async fn actual_helper_holds_fence_after_parent_handle_drops_until_final_close() {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let a = repo();
        let held = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        let inode = held.file.metadata().unwrap().ino();
        let mut command = tokio::process::Command::new("python3");
        command.args(["-c", "import os,fcntl,sys; fd=int(os.environ['DRACON_CANONICAL_WRITER_FD_V1']); fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB); print('held',flush=True); sys.stdin.buffer.read(1); os.close(fd)"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        held.inherit_into(&mut command);
        let mut child = command.spawn().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut ready = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut ready)).await.unwrap().unwrap();
        assert_eq!(ready, "held\n");
        drop(held);
        assert!(WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_millis(30)).await.is_err());
        child.stdin.take().unwrap().write_all(b"x").await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), child.wait()).await.unwrap().unwrap().success());
        let recovered = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        assert_eq!(recovered.file.metadata().unwrap().ino(), inode);
    }
    #[tokio::test]
    async fn unsafe_or_replaced_namespace_is_refused() {
        let a = repo(); let outside = tempfile::NamedTempFile::new().unwrap();
        let path = a.path().join(".git").join(FILE_NAME);
        std::os::unix::fs::symlink(outside.path(), &path).unwrap();
        assert!(WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.is_err());
        std::fs::remove_file(&path).unwrap();
        let held = WriteFence::acquire_until(a.path(), Instant::now() + Duration::from_secs(1)).await.unwrap();
        std::fs::rename(&path, path.with_extension("displaced")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        assert!(held.verify().is_err());
    }
}
