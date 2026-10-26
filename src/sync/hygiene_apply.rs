//! Canonical `.gitignore` reconciliation for hygiene plans. Reuses the proven
//! lease/exchange helper: no write through a live pathname, no stranded writers.
use super::excluded_revert;
use super::staged_epoch;
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use anyhow::{ensure, Context};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tokio::time::Instant;

pub(super) const IGNORE: &str = ".gitignore";
const MAX_IGNORE: u64 = 8 * 1024 * 1024;

/// Bounded no-follow observation bound to the descriptor that produced it.
struct Observed {
    bytes: Vec<u8>,
    stamp: (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64, u64),
}
fn stamp(meta: &std::fs::Metadata) -> (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64, u64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mode(),
        meta.uid(),
        meta.gid(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
        meta.nlink(),
    )
}
impl Observed {
    fn open(directory: &Path) -> Result<Option<Self>> {
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory)?;
        let leaf = CString::new(IGNORE)?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
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
            meta.is_file()
                && meta.nlink() == 1
                && meta.uid() == unsafe { libc::geteuid() }
                && meta.len() <= MAX_IGNORE,
            "generated ignore requires an owned single-link bounded regular file"
        );
        let mut bytes = Vec::new();
        use std::io::Read;
        (&file).take(MAX_IGNORE + 1).read_to_end(&mut bytes)?;
        ensure!(
            (bytes.len() as u64) == meta.len(),
            ".gitignore changed during capture"
        );
        Ok(Some(Self {
            bytes,
            stamp: stamp(&meta),
        }))
    }
    fn unchanged(&self, directory: &Path) -> Result<bool> {
        Ok(Observed::open(directory)?
            .is_some_and(|named| named.bytes == self.bytes && named.stamp == self.stamp))
    }
}

/// Prepare and apply generated ignore bytes once. Returns the rendered bytes
/// only when the caller must admit them into its private index or report a
/// preserved follow-up; canonical and index durability are separate stores.
pub(super) async fn apply(
    repo: &Path,
    patterns: &[String],
    deadline: Instant,
) -> Result<(bool, Option<String>)> {
    tokio::time::timeout_at(deadline, async {
        let canonical = std::fs::canonicalize(repo)?;
        let repo = canonical.as_path();
        let observed = Observed::open(repo)?;
        let current = observed
            .as_ref()
            .map(|observed| std::str::from_utf8(&observed.bytes))
            .transpose()?
            .unwrap_or("");
        let Some(rendered) = crate::exclude::hygiene::render_positive_additions(current, patterns)?
        else {
            return Ok((false, None));
        };
        ensure!(
            rendered.len() as u64 <= MAX_IGNORE,
            "generated ignore exceeds bounded size"
        );
        let snapshot = StagedIntentSnapshot::capture(repo, deadline).await?;
        ensure!(
            snapshot.changed.is_empty(),
            "generated ignore preserves concurrent staged intent"
        );
        let CapturedHead::Commit(head) = snapshot.head.clone() else {
            return Ok((false, Some(rendered)));
        };
        let gitdir = PathBuf::from(
            std::str::from_utf8(
                &staged_epoch::git(repo, &["rev-parse", "--absolute-git-dir"], None)
                    .await?
                    .stdout,
            )?
            .trim_end_matches('\n'),
        );
        let scratch = tempfile::Builder::new()
            .prefix("dracon-ignore-recovery-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(gitdir)?;
        let files = scratch.path().join("files");
        std::fs::create_dir(&files)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(files.join(IGNORE))?;
        file.write_all(rendered.as_bytes())?;
        file.sync_all()?;
        // The exchange helper opens the canonical path again; require the exact
        // observation that produced `rendered` to still be the live content.
        if !observed
            .as_ref()
            .is_none_or(|prior| prior.unchanged(repo).unwrap_or(false))
        {
            return Ok((false, Some(rendered)));
        }
        let applied =
            excluded_revert::apply_generated(repo, scratch, Path::new(IGNORE), head, deadline)
                .await?;
        Ok((applied, Some(rendered)))
    })
    .await
    .context("generated ignore reconciliation deadline expired; recovery retained")?
}
