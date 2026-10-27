//! Explicit destructive opt-in, applied BEFORE staging/hooks, never afterwards.
//! Linux exchange retains displaced bytes in a private durable recovery directory.
#[path = "excluded_archive.rs"]
mod archive;
#[cfg(test)]
#[path = "excluded_revert_crash_tests.rs"]
mod crash_tests;
#[cfg(test)]
#[path = "excluded_revert_recovery_tests.rs"]
mod recovery_tests;
use super::*;
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use anyhow::{bail, ensure};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, PermissionsExt},
};
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
        Ok(self.observed().await?.map(|(fingerprint, _)| fingerprint))
    }
    async fn observed(&self) -> Result<Option<(Fingerprint, archive::Stamp)>> {
        let Some(file) = self.file()? else {
            return Ok(None);
        };
        let before = file.metadata()?;
        ensure!(
            before.is_file() && before.nlink() == 1,
            "excluded path must be a single-link regular file; alias bytes retained"
        );
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
        let observed = archive::Stamp::of(&after);
        ensure!(
            archive::Stamp::of(&before) == observed
                && length == before.len()
                && archive::stamp(self)? == Some(observed.clone()),
            "excluded file changed during capture"
        );
        Ok(Some((
            Fingerprint {
                identity: identity(&after),
                sha256: format!("{:x}", hash.finalize()),
            },
            observed,
        )))
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Observe, render and install ONE generated `.gitignore` under one authority.
/// Never recapture a newer canonical file as the baseline for older rendered
/// bytes. The exact observed fingerprint travels through the same lease,
/// exchange and recovery machinery as explicit restore, without its opt-in.
pub(super) async fn apply_ignore(
    repo: &Path,
    patterns: &[String],
    deadline: Instant,
) -> Result<(bool, Option<String>)> {
    tokio::time::timeout_at(deadline, async {
        use std::os::unix::fs::OpenOptionsExt;
        const LIMIT: u64 = 8 * 1024 * 1024;
        let canonical = std::fs::canonicalize(repo)?;
        let repo = canonical.as_path();
        let writer_fence = crate::git::write_fence::WriteFence::acquire_until(repo, deadline).await?;
        let lock = match crate::git::IndexLock::try_acquire_path(crate::git::index_lock_path(repo))
        {
            Ok(lock) => lock,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!("generated ignore deferred by foreign index lock; canonical bytes preserved")
            }
            Err(error) => return Err(error.into()),
        };
        let snapshot = StagedIntentSnapshot::capture(repo, deadline).await?;
        ensure!(
            snapshot.changed.is_empty(),
            "generated ignore preserves foreign staging"
        );
        let path = Path::new(".gitignore");
        let source = Location::open(repo, path)?;
        let mut bytes = Vec::new();
        let before = if let Some(file) = source.file()? {
            let meta = file.metadata()?;
            ensure!(
                meta.is_file()
                    && meta.nlink() == 1
                    && meta.uid() == unsafe { libc::geteuid() }
                    && meta.len() <= LIMIT,
                "ignore source must be owned, regular, single-link and bounded"
            );
            let expected = archive::Stamp::of(&meta);
            let mut file = tokio::fs::File::from_std(file);
            (&mut file).take(LIMIT + 1).read_to_end(&mut bytes).await?;
            ensure!(
                bytes.len() as u64 == meta.len()
                    && archive::Stamp::of(&file.metadata().await?) == expected
                    && archive::stamp(&source)? == Some(expected),
                "ignore source changed during observation"
            );
            Some(Fingerprint {
                identity: identity(&meta),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            })
        } else {
            None
        };
        let current =
            std::str::from_utf8(&bytes).context("ignore source is not UTF-8; bytes preserved")?;
        let Some(rendered) = crate::exclude::hygiene::render_positive_additions(current, patterns)?
        else {
            return Ok((false, None));
        };
        ensure!(
            rendered.len() as u64 <= LIMIT,
            "generated ignore exceeds size budget"
        );
        let CapturedHead::Commit(head) = &snapshot.head else {
            return Ok((false, Some(rendered)));
        };
        let gitdir = staged_epoch::git(repo, &["rev-parse", "--absolute-git-dir"], None).await?;
        let gitdir = PathBuf::from(std::str::from_utf8(&gitdir.stdout)?.trim_end_matches('\n'));
        // Same recognized namespace as all other canonical recovery: pending
        // records and late archive writes MUST survive opt-out/restart reports.
        let scratch = tempfile::Builder::new()
            .prefix("dracon-excluded-recovery-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(&gitdir)?;
        let files = scratch.path().join("files");
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&files)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(before.as_ref().map_or(0o600, |f| f.identity.3 & 0o777))
            .open(files.join(path))?;
        file.write_all(rendered.as_bytes())?;
        file.sync_all()?;
        drop(file);
        let desired_before = Location::open(&files, path)?
            .fingerprint()
            .await?
            .context("generated ignore target missing")?;
        let head = head.clone();
        let applied = apply_prepared(
            repo,
            scratch,
            vec![Target {
                path: path.to_path_buf(),
                source,
                before,
                desired_before,
            }],
            head,
            deadline,
            lock,
            snapshot,
            &writer_fence,
        )
        .await?;
        Ok((applied, Some(rendered)))
    })
    .await
    .context("generated ignore deadline expired; canonical/recovery bytes preserved")?
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
    let writer_fence = crate::git::write_fence::WriteFence::acquire_until(repo, deadline).await?;
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
        .permissions(std::fs::Permissions::from_mode(0o700))
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
    apply_prepared(
        repo,
        scratch,
        targets,
        head.clone(),
        deadline,
        lock,
        snapshot,
        &writer_fence,
    )
    .await
}

/// Reusable authorization boundary for a fully prepared private desired set.
/// Neither the excluded restore nor the generated-ignore path may skip the
/// owned lock, unchanged-candidate check, lease/exchange helper or settlement.
async fn apply_prepared(
    repo: &Path,
    scratch: tempfile::TempDir,
    targets: Vec<Target>,
    head: String,
    deadline: Instant,
    lock: crate::git::IndexLock,
    snapshot: StagedIntentSnapshot,
    writer_fence: &crate::git::write_fence::WriteFence,
) -> Result<bool> {
    writer_fence.verify()?;
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
    writer_fence.inherit_into(&mut command);
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
    writer_fence.verify()?;
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
        let request_stamp = archive::Stamp::of(&source.metadata()?);
        let mut bytes = Vec::new();
        tokio::fs::File::from_std(source).take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes).await?;
        ensure!(archive::stamp(&Location::open(directory, Path::new("recovery.json"))?)? == Some(request_stamp.clone()),
            "recovery request changed during inspection");
        ensure!(bytes.len() <= 64 * 1024 * 1024, "recovery request too large");
        let request: RestoreRequest = serde_json::from_slice(&bytes)?;
        if std::fs::canonicalize(repo)?.as_os_str().as_bytes() != request.repo {
            eprintln!("⚠️ excluded recovery repository mismatch: {}", directory.display());
            return Ok(false);
        }
        let tree = directory.join("files");
        let mut states = Vec::new();
        let mut observed_targets = Vec::new();
        for target in request.targets {
            use std::os::unix::ffi::OsStringExt;
            let path = PathBuf::from(std::ffi::OsString::from_vec(target.path.clone()));
            let retained = Location::open(&tree, &path)?.observed().await?;
            let retained_identity = retained.as_ref().map(|(fingerprint, _)| fingerprint);
            let state = if retained_identity == Some(&target.desired) {
                // The private file is still the DESIRED payload: the exchange may
                // have happened (canonical now holds it) or not. Never assume.
                "private-desired-payload-retained"
            } else if target.before.is_some() && retained_identity == target.before.as_ref() {
                // A different inode now holds the ORIGINAL bytes privately, so
                // the single exchange completed. Reuse of that inode after
                // archival is detected by the archive stamp probe, and the
                // canonical inode is never rewritten by a rollback.
                "canonical-exchange-settled"
            } else if target.before.is_none() && retained.is_none() {
                "absent-baseline-no-original-displaced"
            } else {
                eprintln!("⚠️ unresolved excluded recovery: {}; retained bytes differ from BOTH captured identities; canonical files/index left untouched", directory.display());
                return Ok(false);
            };
            states.push(serde_json::json!({"path":target.path,"state":state}));
            observed_targets.push(archive::Entry { path: target.path, observed: retained.map(|(_, stamp)| stamp) });
        }
        let record = serde_json::json!({"inspectedBy":env!("DRACON_EPOCH_SOURCE_ID"),
            "originalSource":request.source_id,"states":states,
            "action":"read-only classification; no old restore replayed; canonical exchange is never rolled back"});
        let mut note = tempfile::Builder::new().prefix("settlement-").suffix(".json").tempfile_in(directory)?;
        note.write_all(&serde_json::to_vec_pretty(&record)?)?;
        note.as_file().sync_all()?;
        let _ = note.keep()?;
        File::open(directory)?.sync_all()?;
        archive::persist(directory, &archive::Seal { schema: 1, repo: request.repo,
            request: request_stamp, targets: observed_targets })?;
        let gitdir = crate::git::path_gitdir(repo).context("recovery directory missing Git directory")?;
        let archive = gitdir.join("dracon-excluded-archive");
        if directory.parent() == Some(archive.as_path()) {
            // Upgrade an unsealed older archive only after complete identity
            // classification. A changed existing seal is NEVER rewritten.
            return Ok(true);
        }
        ensure!(directory.parent() == Some(gitdir.as_path()), "unexpected pending recovery location");
        match std::fs::DirBuilder::new().mode(0o700).create(&archive) {
            Ok(()) => {},
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(error) => return Err(error.into()),
        }
        let meta = std::fs::symlink_metadata(&archive)?;
        ensure!(meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "excluded archive must be an owned private real directory");
        let name = directory.file_name().context("missing recovery directory name")?;
        let from = Location::open(&gitdir, Path::new(name))?;
        let to = Location::open(&archive, Path::new(name))?;
        from.exchange(&to, libc::RENAME_NOREPLACE)?;
        eprintln!("🛟 verified excluded recovery retained at {}", archive.join(name).display());
        Ok(true)
    }).await.context("excluded recovery inspection deadline expired; records retained")?
}

/// A producer can edit an excluded file again after its pre-staging restore.
/// Empty candidates must retain that fact rather than return healthy no-work.
pub(super) async fn paths_dirty(
    repo: &Path,
    paths: &[dracon_git::types::DiffFile],
    deadline: Instant,
) -> Result<bool> {
    tokio::time::timeout_at(deadline, async {
        let mut command = crate::policy::tokio_git_command();
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(repo)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .args([
                "--no-optional-locks",
                "-c",
                "core.fsmonitor=false",
                "diff",
                "--name-only",
                "-z",
                "HEAD",
                "--",
            ]);
        for entry in paths {
            ensure!(
                crate::git::is_safe_git_path(&entry.path),
                "unsafe excluded follow-up path"
            );
            let mut literal = std::ffi::OsString::from(":(literal)");
            literal.push(entry.path.as_os_str());
            command.arg(literal);
        }
        let out = crate::git::run_command_captured_output(
            command.into_tokio(),
            3600,
            "excluded restored-path follow-up",
            None,
        )
        .await?;
        ensure!(
            out.status.success(),
            "excluded follow-up inspection failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(!out.stdout.is_empty())
    })
    .await
    .context("excluded follow-up absolute deadline expired")?
}

/// Must also run with the opt-in NOW DISABLED: changing policy does not make an
/// earlier interrupted transaction disappear. No worktree/index writes here.
pub(super) async fn settle_pending(repo: &Path, deadline: Instant) -> Result<bool> {
    let gitdir = crate::git::path_gitdir(repo).context("cannot locate recovery Git directory")?;
    match std::fs::symlink_metadata(crate::git::index_lock_path(repo)) {
        Ok(_) => return Ok(!super::has_pending_excluded_recovery(repo)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    archive::tighten_legacy(repo)?;
    for directory in archive::unsealed(repo)? {
        if !settle_directory(repo, &directory, deadline).await? {
            return Ok(false);
        }
    }
    for entry in std::fs::read_dir(gitdir)? {
        ensure!(
            Instant::now() < deadline,
            "excluded recovery inspection deadline expired"
        );
        let entry = entry?;
        if !super::is_canonical_recovery_name(&entry.file_name()) {
            continue;
        }
        ensure!(
            entry.file_type()?.is_dir(),
            "non-directory excluded recovery record"
        );
        if !settle_directory(repo, &entry.path(), deadline).await? {
            return Ok(false);
        }
    }
    Ok(!super::has_pending_excluded_recovery(repo)?)
}

pub(super) fn archived_pending(repo: &Path) -> Result<bool> {
    archive::pending(repo)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RestoreTarget {
    path: Vec<u8>,
    before: Option<Fingerprint>,
    desired: Fingerprint,
    // Stamp of the DISPLACED (private) inode at request time. Optional so legacy
    // journals stay readable and are never assumed settled by identity alone.
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
    // The parent passes the same held OFD; direct helper callers acquire before
    // any canonical path lookup. Keep the actual helper PID and existing parent
    // death/cancellation binding unchanged (no intermediary writer process).
    let writer_fence = crate::git::write_fence::WriteFence::acquire_until(
        &repo, Instant::now() + Duration::from_secs(30),
    ).await?;
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
            || desired.fingerprint().await? != Some(item.desired.clone())
        {
            return response(false);
        }
        targets.push((source, desired, item.before, item.desired));
    }
    let snapshot = StagedIntentSnapshot::capture(&repo, started + lease_budget).await?;
    if snapshot.head != CapturedHead::Commit(request.head) || !snapshot.changed.is_empty() {
        return response(false);
    }
    for (source, desired, before, expected) in targets {
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
        writer_fence.verify()?;
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
            // ONE exchange finalizes. Rolling back would relocate a writer that
            // already opened the newly canonical inode; an inode cannot be
            // atomically un-swapped under queued openers. Any later open of the
            // displaced inode surfaces as a real concern through the archive
            // stamp probe, never through an automatic canonical rewrite.
            source.exchange(&desired, libc::RENAME_EXCHANGE)?;
            if let Some(file) = desired.file()? {
                file.sync_all()?;
            }
            // Prove BOTH sides after the exchange. A mismatch means some party
            // swapped or substituted an inode; report uncertainty and never
            // attempt a second exchange (that is what relocated live writers).
            if desired.fingerprint().await? != before
                || source.fingerprint().await? != Some(expected)
            {
                return response(false);
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
