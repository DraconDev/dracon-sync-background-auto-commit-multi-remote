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
struct Location {
    parent: File,
    leaf: CString,
}
impl Location {
    fn open(root: &Path, path: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        ensure!(crate::git::is_safe_git_path(path), "unsafe excluded path");
        let mut parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)?;
        let parts: Vec<_> = path.components().collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            let std::path::Component::Normal(name) = part else {
                bail!("non-normal excluded path")
            };
            let name = CString::new(name.as_bytes())?;
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            parent = unsafe { File::from_raw_fd(fd) };
        }
        let Some(std::path::Component::Normal(name)) = parts.last() else {
            bail!("empty excluded path")
        };
        Ok(Self {
            parent,
            leaf: CString::new(name.as_bytes())?,
        })
    }
    fn file(&self) -> Result<Option<File>> {
        let fd = unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                self.leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(error.into());
        }
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    }
    async fn fingerprint(&self) -> Result<Option<Fingerprint>> {
        let Some(file) = self.file()? else {
            return Ok(None);
        };
        let before = file.metadata()?;
        ensure!(before.is_file(), "excluded path is not a regular file");
        let mut file = tokio::fs::File::from_std(file);
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut length = 0u64;
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            length = length
                .checked_add(n as u64)
                .context("excluded byte count overflow")?;
            ensure!(length <= before.len(), "excluded file grew during capture");
            hash.update(&buffer[..n]);
        }
        let after = file.metadata().await?;
        ensure!(
            identity(&before) == identity(&after) && length == before.len(),
            "excluded file changed during capture"
        );
        Ok(Some(Fingerprint {
            identity: identity(&after),
            sha256: format!("{:x}", hash.finalize()),
        }))
    }
    fn same_parent(&self, other: &Self) -> Result<bool> {
        let a = self.parent.metadata()?;
        let b = other.parent.metadata()?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }
    fn exchange(&self, other: &Self, flags: u32) -> Result<()> {
        let status = unsafe {
            libc::renameat2(
                self.parent.as_raw_fd(),
                self.leaf.as_ptr(),
                other.parent.as_raw_fd(),
                other.leaf.as_ptr(),
                flags,
            )
        };
        if status != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        self.parent.sync_all()?;
        other.parent.sync_all()?;
        Ok(())
    }
}

// Rename changes ctime, so it is intentionally excluded from exchange equality.
fn identity(m: &std::fs::Metadata) -> (u64, u64, u64, u32, u32, u32, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.mtime(),
        m.mtime_nsec(),
    )
}
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Fingerprint {
    identity: (u64, u64, u64, u32, u32, u32, i64, i64),
    sha256: String,
}
struct Target {
    path: PathBuf,
    source: Location,
    before: Option<Fingerprint>,
    desired_before: Fingerprint,
}

/// false = uncertain ownership/content; callers preserve work and drain backlog.
/// An error is never successful hygiene. All subprocesses use the epoch runner;
/// the caller supplies ONE absolute deadline across the whole preparation.
pub(super) async fn restore(
    repo: &Path,
    paths: &[dracon_git::types::DiffFile],
    deadline: Instant,
) -> Result<bool> {
    tokio::time::timeout_at(deadline, restore_inner(repo, paths, deadline))
        .await
        .context("excluded restore absolute deadline expired; inspect retained recovery journal")?
}
async fn restore_inner(
    repo: &Path,
    paths: &[dracon_git::types::DiffFile],
    deadline: Instant,
) -> Result<bool> {
    let canonical_repo = std::fs::canonicalize(repo)?;
    let repo = canonical_repo.as_path();
    let lock = match crate::git::IndexLock::try_acquire_path(crate::git::index_lock_path(repo)) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let snapshot = StagedIntentSnapshot::capture(repo, deadline).await?;
    if !snapshot.changed.is_empty() {
        return Ok(false);
    }
    let CapturedHead::Commit(head) = &snapshot.head else {
        return Ok(true);
    };
    let tracked: Vec<_> = paths
        .iter()
        .filter(|entry| {
            snapshot
                .entries
                .contains_key(entry.path.as_os_str().as_bytes())
        })
        .collect();
    if tracked.is_empty() {
        return Ok(true);
    } // never delete untracked excluded work
    for entry in &tracked {
        let mode = snapshot.entries[entry.path.as_os_str().as_bytes()].mode;
        ensure!(
            mode == 0o100644 || mode == 0o100755,
            "excluded restore requires regular tracked file"
        );
    }
    let mut sources = Vec::new();
    for entry in &tracked {
        let source = Location::open(repo, &entry.path)?;
        let before = source.fingerprint().await?;
        sources.push((entry.path.clone(), source, before));
    }
    let gitdir = staged_epoch::git(repo, &["rev-parse", "--absolute-git-dir"], None).await?;
    let gitdir = PathBuf::from(std::str::from_utf8(&gitdir.stdout)?.trim_end_matches('\n'));
    let scratch = tempfile::Builder::new()
        .prefix("dracon-excluded-recovery-")
        .tempdir_in(gitdir)?;
    let index = scratch.path().join("index");
    staged_epoch::run_index_git(
        repo,
        &["read-tree", head],
        3600,
        "excluded target index",
        Some(&index),
    )
    .await?;
    let tree = scratch.path().join("files");
    std::fs::create_dir(&tree)?;
    let prefix = format!("--prefix={}/", tree.display());
    let mut input = Vec::new();
    for entry in &tracked {
        input.extend_from_slice(entry.path.as_os_str().as_bytes());
        input.push(0);
    }
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(repo)
        .env("GIT_INDEX_FILE", &index)
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["checkout-index", "--force", &prefix, "--stdin", "-z"]);
    // Required smudge filters are NOT bypassed. NUL stdin uses exact filenames,
    // not pathspecs or lossy Unicode, and avoids argv-size expansion.
    let output = crate::git::run_command_captured_output(
        command.into_tokio(),
        3600,
        "excluded smudge target",
        Some(&input),
    )
    .await?;
    ensure!(
        output.status.success(),
        "excluded smudge target refused: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut targets = Vec::new();
    for (path, source, before) in sources {
        let desired = Location::open(&tree, &path)?;
        let desired_before = desired
            .fingerprint()
            .await?
            .context("missing excluded restore target")?;
        targets.push(Target {
            path,
            source,
            before,
            desired_before,
        });
    }
    if !lock.is_owned() || snapshot != StagedIntentSnapshot::capture(repo, deadline).await? {
        return Ok(false);
    }
    for target in &targets {
        if !target
            .source
            .same_parent(&Location::open(repo, &target.path)?)?
            || target.source.fingerprint().await? != target.before
        {
            return Ok(false);
        }
    }
    let journal = RestoreRequest {
        source_id: env!("DRACON_EPOCH_SOURCE_ID").to_owned(),
        repo: repo.as_os_str().as_bytes().to_vec(),
        head: head.clone(),
        targets: targets
            .into_iter()
            .map(|t| RestoreTarget {
                path: t.path.as_os_str().as_bytes().to_vec(),
                before: t.before,
                desired: t.desired_before,
            })
            .collect(),
    };
    let journal_path = scratch.path().join("recovery.json");
    std::fs::write(&journal_path, serde_json::to_vec_pretty(&journal)?)?;
    File::open(&journal_path)?.sync_all()?;
    File::open(scratch.path())?.sync_all()?;
    // Persist BEFORE any exchange/cancellation. Never remove displaced bytes.
    let retained = scratch.keep();
    eprintln!(
        "🛟 {} explicit excluded restore recovery: {}",
        repo.display(),
        retained.display()
    );
    let executable = std::env::current_exe()?;
    #[cfg(test)]
    let executable = {
        let _ = executable;
        PathBuf::from(
            std::env::var_os("DRACON_EPOCH_TEST_HELPER")
                .context("matching revision-qualified excluded restore helper required")?,
        )
    };
    ensure!(executable.is_absolute(), "excluded helper must be absolute");
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--dracon-excluded-restore-helper")
        .arg(&journal_path)
        .arg(std::process::id().to_string())
        .env("TOKIO_WORKER_THREADS", "2")
        .current_dir(repo);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let output =
        crate::git::run_command_captured_output(command, 3600, "excluded restore helper", None)
            .await?;
    ensure!(
        output.status.success(),
        "excluded restore helper refused: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        lock.is_owned(),
        "excluded restore lock replaced; recovery retained"
    );
    let response: (String, bool) = serde_json::from_slice(&output.stdout)?;
    ensure!(
        response.0 == env!("DRACON_EPOCH_SOURCE_ID"),
        "excluded helper source identity mismatch"
    );
    // A completed/deferred helper is NOT sufficient proof of recovery. Inspect
    // retained bytes while this parent still owns the actual index lock.
    if !settle_directory(repo, &retained, deadline).await? {
        return Ok(false);
    }
    Ok(response.1)
}

/// Inspect only retained bytes; NEVER replay an old restore or modify canonical
/// files/index. Mixed interrupted transactions may settle only when EVERY target
/// still has either the exact private desired inode or exact displaced original.
async fn settle_directory(repo: &Path, directory: &Path, deadline: Instant) -> Result<bool> {
    use std::os::unix::fs::DirBuilderExt;
    tokio::time::timeout_at(deadline, async {
        let source = Location::open(directory, Path::new("recovery.json"))?.file()?
            .context("missing recovery request")?;
        ensure!(source.metadata()?.is_file(), "nonregular recovery request");
        let mut bytes = Vec::new();
        tokio::fs::File::from_std(source).take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes).await?;
        ensure!(bytes.len() <= 64 * 1024 * 1024, "recovery request too large");
        let request: RestoreRequest = serde_json::from_slice(&bytes)?;
        if std::fs::canonicalize(repo)?.as_os_str().as_bytes() != request.repo {
            eprintln!("⚠️ excluded recovery repository mismatch: {}", directory.display());
            return Ok(false);
        }
        let tree = directory.join("files");
        let mut states = Vec::new();
        for target in request.targets {
            use std::os::unix::ffi::OsStringExt;
            let path = PathBuf::from(std::ffi::OsString::from_vec(target.path.clone()));
            let retained = Location::open(&tree, &path)?.fingerprint().await?;
            let state = if retained.as_ref() == Some(&target.desired) {
                "untouched-or-rolled-back"
            } else if target.before.is_some() && retained == target.before {
                "captured-original-retained"
            } else if target.before.is_none() && retained.is_none() {
                "absent-baseline-no-original-displaced"
            } else {
                eprintln!("⚠️ unresolved excluded recovery: {}; retained bytes differ from BOTH captured identities; canonical files/index left untouched", directory.display());
                return Ok(false);
            };
            states.push(serde_json::json!({"path":target.path,"state":state}));
        }
        let record = serde_json::json!({"inspectedBy":env!("DRACON_EPOCH_SOURCE_ID"),
            "originalSource":request.source_id,"states":states,
            "action":"read-only classification; no old restore replayed"});
        let mut note = tempfile::Builder::new().prefix("settlement-").suffix(".json").tempfile_in(directory)?;
        note.write_all(&serde_json::to_vec_pretty(&record)?)?;
        note.as_file().sync_all()?;
        let _ = note.keep()?;
        File::open(directory)?.sync_all()?;
        let gitdir = directory.parent().context("recovery directory missing parent")?;
        let archive = gitdir.join("dracon-excluded-archive");
        match std::fs::DirBuilder::new().mode(0o700).create(&archive) {
            Ok(()) => {},
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(error) => return Err(error.into()),
        }
        let meta = std::fs::symlink_metadata(&archive)?;
        ensure!(meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "excluded archive must be an owned private real directory");
        let name = directory.file_name().context("missing recovery directory name")?;
        let from = Location::open(gitdir, Path::new(name))?;
        let to = Location::open(&archive, Path::new(name))?;
        from.exchange(&to, libc::RENAME_NOREPLACE)?;
        eprintln!("🛟 verified excluded recovery retained at {}", archive.join(name).display());
        Ok(true)
    }).await.context("excluded recovery inspection deadline expired; records retained")?
}

/// Must also run with the opt-in NOW DISABLED: changing policy does not make an
/// earlier interrupted transaction disappear. No worktree/index writes here.
pub(super) async fn settle_pending(repo: &Path, deadline: Instant) -> Result<bool> {
    let gitdir = crate::git::path_gitdir(repo).context("cannot locate recovery Git directory")?;
    if std::fs::symlink_metadata(crate::git::index_lock_path(repo)).is_ok() {
        return Ok(!super::has_pending_excluded_recovery(repo)?);
    }
    for entry in std::fs::read_dir(gitdir)? {
        let entry = entry?;
        if !entry.file_name().as_bytes().starts_with(b"dracon-excluded-recovery-") { continue; }
        ensure!(entry.file_type()?.is_dir(), "non-directory excluded recovery record");
        if !settle_directory(repo, &entry.path(), deadline).await? { return Ok(false); }
    }
    Ok(true)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RestoreTarget {
    path: Vec<u8>,
    before: Option<Fingerprint>,
    desired: Fingerprint,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct RestoreRequest {
    source_id: String,
    repo: Vec<u8>,
    head: String,
    targets: Vec<RestoreTarget>,
}

/// Private child ONLY: signal disposition and kernel leases must never affect
/// daemon/editor threads. Parent cancellation kills the owned child group and
/// releases leases; no detached blocking thread can restore files afterwards.
pub(super) async fn helper(request_path: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let file = tokio::fs::File::open(request_path).await?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "excluded recovery request too large"
    );
    let request: RestoreRequest = serde_json::from_slice(&bytes)?;
    ensure!(
        request.source_id == env!("DRACON_EPOCH_SOURCE_ID"),
        "excluded helper source identity mismatch"
    );
    let repo = PathBuf::from(std::ffi::OsString::from_vec(request.repo.clone()));
    let tree = request_path
        .parent()
        .context("recovery root missing")?
        .join("files");
    // An ignored SIGIO is scoped to this otherwise idle dedicated child. A
    // competing new writer waits only for this short critical section; an
    // ALREADY open writable FD/mmap makes lease acquisition fail, not relocate
    // the producer's future writes into an invisible recovery file.
    unsafe {
        libc::signal(libc::SIGIO, libc::SIG_IGN);
    }
    let break_seconds: u64 = std::fs::read_to_string("/proc/sys/fs/lease-break-time")?
        .trim()
        .parse()?;
    ensure!(
        break_seconds > 1,
        "kernel lease break interval too short; excluded bytes retained"
    );
    let started = Instant::now();
    let lease_budget = Duration::from_secs(break_seconds - 1);
    let mut leases = Vec::new();
    let mut targets = Vec::new();
    for item in request.targets {
        let path = PathBuf::from(std::ffi::OsString::from_vec(item.path));
        let source = Location::open(&repo, &path)?;
        if let Some(file) = source.file()? {
            let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLEASE, libc::F_RDLCK) };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EAGAIN) {
                    return response(false);
                }
                return Err(error)
                    .context("cannot prove excluded file has no active writer; bytes retained");
            }
            leases.push(file);
        }
        let desired = Location::open(&tree, &path)?;
        if source.fingerprint().await? != item.before
            || desired.fingerprint().await? != Some(item.desired)
        {
            return response(false);
        }
        targets.push((source, desired, item.before));
    }
    let snapshot = StagedIntentSnapshot::capture(&repo, started + lease_budget).await?;
    if snapshot.head != CapturedHead::Commit(request.head) || !snapshot.changed.is_empty() {
        return response(false);
    }
    for (source, desired, before) in targets {
        if started.elapsed() >= lease_budget
            || leases.iter().any(
                |file| unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLEASE) } != libc::F_RDLCK,
            )
        {
            return response(false);
        }
        if source.fingerprint().await? != before {
            return response(false);
        }
        // Hashing itself must not run past kernel-enforced lease expiry.
        if started.elapsed() >= lease_budget
            || leases.iter().any(
                |file| unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLEASE) } != libc::F_RDLCK,
            )
        {
            return response(false);
        }
        if before.is_none() {
            match desired.exchange(&source, libc::RENAME_NOREPLACE) {
                Ok(()) => {}
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
                {
                    return response(false)
                }
                Err(error) => return Err(error),
            }
        } else {
            source.exchange(&desired, libc::RENAME_EXCHANGE)?;
            if desired.fingerprint().await? != before || leases.iter().any(|file|
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLEASE) } != libc::F_RDLCK) {
                source.exchange(&desired, libc::RENAME_EXCHANGE)?;
                return response(false);
            }
            if let Some(file) = desired.file()? {
                file.sync_all()?;
            }
        }
    }
    response(true)
}
fn response(restored: bool) -> Result<()> {
    use std::io::Write;
    let out = serde_json::to_vec(&(env!("DRACON_EPOCH_SOURCE_ID"), restored))?;
    std::io::stdout().write_all(&out)?;
    Ok(())
}
