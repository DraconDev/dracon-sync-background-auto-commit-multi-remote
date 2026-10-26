//! Canonical `.gitignore` reconciliation for hygiene plans. Reuses the proven
//! lease/exchange helper: no write through a live pathname, no stranded writers.
use super::excluded_revert::{apply_generated_ignore, GeneratedIgnore};
use super::*;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

pub(super) const IGNORE: &str = ".gitignore";
const MAX_IGNORE: u64 = 8 * 1024 * 1024;

/// Bounded no-follow observation bound to the descriptor that produced it.
struct Observed {
    file: File,
    bytes: Vec<u8>,
    stamp: (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64, u64),
}
fn stamp(meta: &std::fs::Metadata) -> (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64, u64) {
    (
        meta.dev(), meta.ino(), meta.len(), meta.mode(), meta.uid(), meta.gid(),
        meta.mtime(), meta.mtime_nsec(), meta.ctime(), meta.ctime_nsec(), meta.nlink(),
    )
}
impl Observed {
    fn open(repo: &Path) -> Result<Option<Self>> {
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(repo)?;
        let leaf = CString::new(IGNORE)?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(), leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(error).context("cannot open .gitignore");
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() == 1 && meta.uid() == unsafe { libc::geteuid() }
                && meta.len() <= MAX_IGNORE,
            "generated ignore requires an owned single-link bounded regular file"
        );
        let bytes = std::fs::read(format!("/proc/self/fd/{fd}"))?;
        ensure!((bytes.len() as u64) == meta.len(), ".gitignore changed during capture");
        Ok(Some(Self { file, bytes, stamp: stamp(&meta) }))
    }
    fn current_text(&self) -> Result<&str> {
        std::str::from_utf8(&self.bytes).context("generated ignore requires UTF-8 operator content")
    }
    fn unchanged(&self, repo: &Path) -> Result<bool> {
        Ok(stamp(&self.file.metadata()?)
            == self.stamp
            && Observed::open(repo)?.is_some_and(|named| named.bytes == self.bytes && named.stamp == self.stamp))
    }
}
fn private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).mode(0o600).open(path)?;
    file.write_all(bytes)?; file.sync_all()?; Ok(())
}

/// Prepare and apply generated ignore bytes once. Returns (applied, rendered)
/// so the caller can admit the new blob into its private index only after the
/// canonical change is durable and observable; never claims atomicity across
/// the two durable stores.
pub(super) async fn apply(repo: &Path, patterns: &[String], deadline: Instant) -> Result<(bool, Option<String>)> {
    tokio::time::timeout_at(deadline, async {
        let canonical=std::fs::canonicalize(repo)?; let repo=canonical.as_path();
        let Some(observed)=Observed::open(repo)? else { return Ok((false,None)) };
        let Some(rendered)=crate::exclude::hygiene::render_positive_additions(observed.current_text()?,patterns)? else {
            return Ok((false,None))
        };
        ensure!(rendered.len() as u64 <= MAX_IGNORE, "generated ignore exceeds bounded size");
        let snapshot=StagedIntentSnapshot::capture(repo,deadline).await?;
        ensure!(snapshot.changed.is_empty(), "generated ignore preserves concurrent staged intent");
        let CapturedHead::Commit(head)=snapshot.head.clone() else { return Ok((false,Some(rendered))) };
        let gitdir=PathBuf::from(std::str::from_utf8(&staged_epoch::git(repo,&["rev-parse","--absolute-git-dir"],None).await?.stdout)?.trim_end_matches('\n'));
        let scratch=tempfile::Builder::new().prefix("dracon-ignore-recovery-")
            .permissions(std::fs::Permissions::from_mode(0o700)).tempdir_in(gitdir)?;
        private(&scratch.path().join(IGNORE),rendered.as_bytes())?;
        let desired=Observed::open(scratch.path())?.context("generated ignore target missing")?;
        let lock=match crate::git::IndexLock::try_acquire_path(crate::git::index_lock_path(repo)) {
            Ok(lock)=>lock,
            Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>return Ok((false,Some(rendered))),
            Err(error)=>return Err(error.into()),
        };
        ensure!(observed.unchanged(repo)?,".gitignore changed before install; operator bytes preserved");
        let applied=apply_generated_ignore(repo,scratch,GeneratedIgnore{
            head, before:excluded_revert::ignore_before(&observed.bytes),
            desired_before:excluded_revert::ignore_observed(&desired.bytes),
        },deadline,lock,snapshot).await?;
        Ok((applied,Some(rendered)))
    }).await.context("generated ignore reconciliation deadline expired; recovery retained")?
}
