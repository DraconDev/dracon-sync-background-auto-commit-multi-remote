//! Explicit destructive opt-in, applied BEFORE staging/hooks, never afterwards.
//! Linux exchange retains displaced bytes in a private durable recovery directory.
use super::*;
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use anyhow::{bail, ensure};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use tokio::io::AsyncReadExt;
use tokio::time::Instant;

// Directory descriptors prohibit ancestor/final symlink traversal at the write
// boundary. Names are exact Git-relative paths, never expanded pathspecs.
struct Location { parent: File, leaf: CString }
impl Location {
    fn open(root: &Path, path: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        ensure!(crate::git::is_safe_git_path(path), "unsafe excluded path");
        let mut parent = OpenOptions::new().read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open(root)?;
        let parts: Vec<_> = path.components().collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            let std::path::Component::Normal(name) = part else { bail!("non-normal excluded path") };
            let name = CString::new(name.as_bytes())?;
            let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
            if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
            parent = unsafe { File::from_raw_fd(fd) };
        }
        let Some(std::path::Component::Normal(name)) = parts.last() else { bail!("empty excluded path") };
        Ok(Self { parent, leaf: CString::new(name.as_bytes())? })
    }
    fn file(&self) -> Result<Option<File>> {
        let fd = unsafe { libc::openat(self.parent.as_raw_fd(), self.leaf.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound { return Ok(None); }
            return Err(error.into());
        }
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }
    async fn fingerprint(&self) -> Result<Option<Fingerprint>> {
        let Some(file) = self.file()? else { return Ok(None); };
        let before = file.metadata()?;
        ensure!(before.is_file(), "excluded path is not a regular file");
        let mut file = tokio::fs::File::from_std(file);
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut length = 0u64;
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 { break; }
            length = length.checked_add(n as u64).context("excluded byte count overflow")?;
            ensure!(length <= before.len(), "excluded file grew during capture");
            hash.update(&buffer[..n]);
        }
        let after = file.metadata().await?;
        ensure!(identity(&before) == identity(&after) && length == before.len(),
            "excluded file changed during capture");
        Ok(Some(Fingerprint { identity: identity(&after), sha256: format!("{:x}", hash.finalize()) }))
    }
    fn same_parent(&self, other: &Self) -> Result<bool> {
        let a = self.parent.metadata()?; let b = other.parent.metadata()?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }
    fn exchange(&self, other: &Self, flags: u32) -> Result<()> {
        let status = unsafe { libc::renameat2(self.parent.as_raw_fd(), self.leaf.as_ptr(),
            other.parent.as_raw_fd(), other.leaf.as_ptr(), flags) };
        if status != 0 { return Err(std::io::Error::last_os_error().into()); }
        self.parent.sync_all()?;
        other.parent.sync_all()?;
        Ok(())
    }
}

// Rename changes ctime, so it is intentionally excluded from exchange equality.
fn identity(m: &std::fs::Metadata) -> (u64,u64,u64,u32,u32,u32,i64,i64) {
    (m.dev(),m.ino(),m.len(),m.mode(),m.uid(),m.gid(),m.mtime(),m.mtime_nsec())
}
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct Fingerprint { identity: (u64,u64,u64,u32,u32,u32,i64,i64), sha256: String }
struct Target { path: PathBuf, source: Location, before: Option<Fingerprint>, desired: Location }

/// false = uncertain ownership/content; callers preserve work and drain backlog.
/// An error is never successful hygiene. All subprocesses use the epoch runner;
/// the caller supplies ONE absolute deadline across the whole preparation.
pub(super) async fn restore(
    repo: &Path, paths: &[dracon_git::types::DiffFile], deadline: Instant,
) -> Result<bool> {
    tokio::time::timeout_at(deadline, restore_inner(repo, paths, deadline)).await
        .context("excluded restore absolute deadline expired; inspect retained recovery journal")?
}
async fn restore_inner(repo: &Path, paths: &[dracon_git::types::DiffFile], deadline: Instant) -> Result<bool> {
    let snapshot = StagedIntentSnapshot::capture(repo, deadline).await?;
    if !snapshot.changed.is_empty() { return Ok(false); }
    let CapturedHead::Commit(head) = &snapshot.head else { return Ok(true); };
    let tracked: Vec<_> = paths.iter().filter(|entry|
        snapshot.entries.contains_key(entry.path.as_os_str().as_bytes())).collect();
    if tracked.is_empty() { return Ok(true); } // never delete untracked excluded work
    for entry in &tracked {
        let mode = snapshot.entries[entry.path.as_os_str().as_bytes()].mode;
        ensure!(mode == 0o100644 || mode == 0o100755, "excluded restore requires regular tracked file");
    }
    let mut sources = Vec::new();
    for entry in &tracked {
        let source = Location::open(repo, &entry.path)?;
        let before = source.fingerprint().await?;
        sources.push((entry.path.clone(), source, before));
    }
    let gitdir = staged_epoch::git(repo, &["rev-parse", "--absolute-git-dir"], None).await?;
    let gitdir = PathBuf::from(std::str::from_utf8(&gitdir.stdout)?.trim_end_matches('\n'));
    let scratch = tempfile::Builder::new().prefix("dracon-excluded-recovery-").tempdir_in(gitdir)?;
    let index = scratch.path().join("index");
    staged_epoch::run_index_git(repo, &["read-tree", head], 3600, "excluded target index", Some(&index)).await?;
    let tree = scratch.path().join("files");
    std::fs::create_dir(&tree)?;
    let prefix = format!("--prefix={}/", tree.display());
    let strings: Vec<_> = tracked.iter().map(|entry| entry.path.to_str()
        .context("non-UTF8 excluded restore path retained").map(str::to_owned)).collect::<Result<_>>()?;
    let literal = crate::git::literal_pathspecs(&strings);
    let mut args = vec!["checkout-index", "--force", &prefix, "--"];
    args.extend(literal.iter().map(String::as_str));
    // Required smudge filters are NOT bypassed. Failed checkout cannot mutate
    // the actual index or working files; the private scratch is disposable here.
    staged_epoch::run_index_git(repo, &args, 3600, "excluded smudge target", Some(&index)).await?;
    let mut targets = Vec::new();
    for (path, source, before) in sources {
        let desired = Location::open(&tree, &path)?;
        ensure!(desired.fingerprint().await?.is_some(), "missing excluded restore target");
        targets.push(Target { path, source, before, desired });
    }
    let lock = match crate::git::IndexLock::try_acquire_path(crate::git::index_lock_path(repo)) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !lock.is_owned() || snapshot != StagedIntentSnapshot::capture(repo, deadline).await? { return Ok(false); }
    for target in &targets {
        if !target.source.same_parent(&Location::open(repo, &target.path)?)?
            || target.source.fingerprint().await? != target.before { return Ok(false); }
    }
    let journal = serde_json::json!({"head":head,"phase":"prepared; files retain exchanged originals",
        "targets":targets.iter().map(|t| serde_json::json!({"path":t.path,"before":t.before})).collect::<Vec<_>>()});
    let journal_path = scratch.path().join("recovery.json");
    std::fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;
    File::open(&journal_path)?.sync_all()?;
    File::open(scratch.path())?.sync_all()?;
    // Persist BEFORE any exchange/cancellation. Never remove displaced bytes.
    let retained = scratch.keep();
    eprintln!("🛟 {} explicit excluded restore recovery: {}", repo.display(), retained.display());
    for target in targets {
        ensure!(lock.is_owned(), "excluded restore index lock replaced; recovery retained");
        if target.source.fingerprint().await? != target.before { return Ok(false); }
        if target.before.is_none() {
            // Atomic no-replace: a newly created producer file wins.
            target.desired.exchange(&target.source, libc::RENAME_NOREPLACE)?;
        } else {
            target.source.exchange(&target.desired, libc::RENAME_EXCHANGE)?;
            if target.desired.fingerprint().await? != target.before {
                // An intervening producer won. Put it back; both displaced
                // inodes remain reachable, including edits via existing FDs.
                target.source.exchange(&target.desired, libc::RENAME_EXCHANGE)?;
                eprintln!("⏳ excluded restore raced; work retained: {}", retained.display());
                return Ok(false);
            }
            if let Some(file) = target.desired.file()? { file.sync_all()?; }
        }
    }
    Ok(true)
}
