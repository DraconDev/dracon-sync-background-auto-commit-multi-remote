//! Linux create-only checkout publication with retained pointer backups.
//!
//! This is a filesystem transaction, not a Git commit or permission to fetch.
//! The caller verifies the selected manifest/attributes before invoking it.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use super::index::CommitLock;
use super::journal::{self, Limits, SnapshotKind};
use super::reference::{Fingerprint, Pointer};
use super::restore::RestoredAsset;

/// A verified checkout result. A displaced pointer is retained, never deleted.
pub struct HydratedAsset {
    path: PathBuf,
    backup: Option<PathBuf>,
}

impl HydratedAsset {
    /// The selected working asset path.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Private retained original file, if a pointer was displaced.
    pub fn backup(&self) -> Option<&Path> {
        self.backup.as_deref()
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    repo_id: String,
    path_hex: String,
    payload: Fingerprint,
    manifest_payload: Fingerprint,
    commit: String,
    source: Fingerprint,
    mode: u32,
    root: (u64, u64),
    parent: (u64, u64),
}

/// Private per-checkout transactions; all retained versions count toward limits.
pub struct HydrationStore {
    directory: PathBuf,
    namespace: File,
    repo_id: String,
    limits: Limits,
}

impl HydrationStore {
    /// Use an explicit private root on the same filesystem as working assets.
    pub fn open(root: &Path, repo_id: &str, limits: Limits) -> Result<Self> {
        super::reference::validate_sha256(repo_id)?;
        if limits.max_records == 0
            || limits.max_snapshot_bytes == 0
            || limits.max_retained_snapshot_bytes < limits.max_snapshot_bytes
        {
            bail!("invalid hydration retention limits");
        }
        journal::private_directory(root, true)?;
        journal::runtime::protect(root)?;
        let directory = root.join(repo_id);
        journal::private_directory(&directory, true)?;
        journal::runtime::protect(&directory)?;
        let namespace = directory_at_path(&directory)?;
        let info = namespace.metadata()?;
        if info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o077 != 0 {
            bail!("hydration namespace must remain private and owned");
        }
        Ok(Self {
            directory,
            namespace,
            repo_id: repo_id.into(),
            limits,
        })
    }

    /// Publish a verified recovery only over its exact working pointer, or into
    /// a missing path. Refuse local edits; capture-before-publish is resumable.
    /// No replacement operation writes over a concurrently created destination.
    pub fn hydrate(
        &self,
        repo: &git2::Repository,
        asset: &RestoredAsset,
        manifest_path: &Path,
        selected_commit: git2::Oid,
        check_placement: impl FnOnce(&git2::Repository) -> Result<()>,
    ) -> Result<HydratedAsset> {
        let _lease = journal::try_lock(&fd_path(&self.namespace).join("hydrate.lock"))?;
        self.hydrate_locked(repo, asset, manifest_path, selected_commit, check_placement)
    }

    /// Resume only a matching, previously authorized local transaction. No
    /// backend fetch or new decryption occurs; retained plaintext must verify.
    pub fn resume(
        &self,
        repo: &git2::Repository,
        manifest_path: &Path,
        asset_path: &Path,
        check_placement: impl FnOnce(&git2::Repository) -> Result<()>,
    ) -> Result<HydratedAsset> {
        let _lease = journal::try_lock(&fd_path(&self.namespace).join("hydrate.lock"))?;
        let path_hex = journal::encode_relative_path(asset_path.as_os_str().as_bytes())?;
        let commit = repo.head()?.peel_to_commit()?.id();
        let workdir = repo
            .workdir()
            .context("hydration requires a checkout")?
            .canonicalize()?;
        let root = directory_at_path(&workdir)?;
        let parent = relative_directory(
            &root,
            asset_path.parent().context("hydration parent missing")?,
        )?;
        let mut selected = None;
        let mut count = 0usize;
        for entry in std::fs::read_dir(fd_path(&self.namespace))? {
            let entry = entry?;
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some(".gitignore" | ".runtime-ignore.lock" | "hydrate.lock")
            ) {
                continue;
            }
            super::reference::validate_sha256(name.to_str().context("unknown hydration entry")?)?;
            count += 1;
            if count > self.limits.max_records {
                bail!("hydration version capacity exceeded");
            }
            let transaction = child_directory(&self.namespace, &name, false)?;
            let Some(file) = file_at(&transaction, OsStr::new("intent.json"))? else {
                continue;
            };
            let info = file.metadata()?;
            if info.len() > 16 * 1024 || info.mode() & 0o077 != 0 {
                bail!("invalid private hydration intent");
            }
            let mut bytes = Vec::new();
            file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
            let intent: Intent = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("invalid hydration intent encoding"))?;
            intent.source.validate()?;
            intent.payload.validate()?;
            intent.manifest_payload.validate()?;
            if intent.version != 1
                || intent.repo_id != self.repo_id
                || !matches!(intent.mode, 0o100644 | 0o100755)
            {
                bail!("invalid hydration intent binding");
            }
            journal::validate_path_hex(&intent.path_hex)?;
            let expected_id = transaction_id(&intent)?;
            if name.as_os_str() != OsStr::new(&expected_id) {
                bail!("hydration intent identity mismatch");
            }
            let pinned = git2::Oid::from_str(&intent.commit)
                .map_err(|_| anyhow::anyhow!("invalid hydration commit binding"))?;
            if pinned.to_string() != intent.commit {
                bail!("noncanonical hydration commit binding");
            }
            if intent.path_hex != path_hex
                || pinned != commit
                || intent.root != identity(&root)?
                || intent.parent != identity(&parent)?
            {
                continue;
            }
            let source_path = if file_at(&transaction, OsStr::new("publish.source"))?.is_some() {
                fd_path(&transaction).join("publish.source")
            } else {
                workdir.join(asset_path)
            };
            // The directory handle keeps the selected retained file bound while
            // it is independently checked and consumed by hydrate_locked.
            let candidate = (
                transaction,
                RestoredAsset {
                    path: source_path,
                    source: intent.source,
                    repo_id: intent.repo_id,
                    path_hex: intent.path_hex,
                    payload: intent.payload,
                    manifest_payload: intent.manifest_payload,
                },
            );
            if selected.replace(candidate).is_some() {
                bail!("ambiguous local hydration transactions");
            }
        }
        let (_transaction, asset) =
            selected.context("no matching retained hydration transaction")?;
        self.hydrate_locked(repo, &asset, manifest_path, commit, check_placement)
    }

    fn hydrate_locked(
        &self,
        repo: &git2::Repository,
        asset: &RestoredAsset,
        manifest_path: &Path,
        selected_commit: git2::Oid,
        check_placement: impl FnOnce(&git2::Repository) -> Result<()>,
    ) -> Result<HydratedAsset> {
        if asset.repo_id != self.repo_id || asset.source.bytes() > self.limits.max_snapshot_bytes {
            bail!("hydration recovery binding or output budget mismatch");
        }
        let _index_lock = CommitLock::acquire(repo, &self.repo_id)?;
        if repo.head()?.peel_to_commit()?.id() != selected_commit {
            bail!("hydration requires the selected checked-out commit");
        }
        check_placement(repo)?;
        let relative = PathBuf::from(OsStr::from_bytes(&super::staging::decode_path(
            &asset.path_hex,
        )?));
        let index = repo.index()?;
        if index.has_conflicts() {
            bail!("hydration requires a resolved index");
        }
        journal::encode_relative_path(manifest_path.as_os_str().as_bytes())?;
        let metadata = index
            .get_path(manifest_path, 0)
            .context("hydration metadata is not staged")?;
        let (bytes, kind) = repo.odb()?.read_header(metadata.id)?;
        if metadata.mode != 0o100644
            || kind != git2::ObjectType::Blob
            || bytes as u64 != asset.manifest_payload.bytes()
            || bytes as u64 > super::metadata::MAX_PROTECTED_MANIFEST_BYTES
        {
            bail!("hydration metadata and verified recovery disagree");
        }
        if format!(
            "{:x}",
            Sha256::digest(repo.find_blob(metadata.id)?.content())
        ) != asset.manifest_payload.sha256()
        {
            bail!("hydration metadata and verified recovery disagree");
        }
        let entry = index
            .get_path(&relative, 0)
            .context("hydration reference is not staged")?;
        let (size, kind) = repo.odb()?.read_header(entry.id)?;
        if !matches!(entry.mode, 0o100644 | 0o100755)
            || kind != git2::ObjectType::Blob
            || size > 1024
            || Pointer::parse(repo.find_blob(entry.id)?.content())?.payload() != &asset.payload
        {
            bail!("hydration reference and verified recovery disagree");
        }
        let workdir = repo
            .workdir()
            .context("hydration requires a checkout")?
            .canonicalize()?;
        let root = directory_at_path(&workdir)?;
        let parent_relative = relative.parent().context("hydration parent missing")?;
        let parent = relative_directory(&root, parent_relative)?;
        if parent.metadata()?.dev() != self.namespace.metadata()?.dev() {
            bail!("hydration root and working asset must share a filesystem");
        }
        let name = relative.file_name().context("hydration filename missing")?;
        let intent = Intent {
            version: 1,
            repo_id: self.repo_id.clone(),
            path_hex: asset.path_hex.clone(),
            payload: asset.payload.clone(),
            manifest_payload: asset.manifest_payload.clone(),
            commit: selected_commit.to_string(),
            source: asset.source.clone(),
            mode: entry.mode,
            root: identity(&root)?,
            parent: identity(&parent)?,
        };
        // The immutable recovered file is independently verified before any
        // working-tree operation; its inode is never shared with editable output.
        let mut source = journal::verify_snapshot(&asset.path, &asset.source)?;
        source.seek(SeekFrom::Start(0))?;
        let id = transaction_id(&intent)?;
        let transaction_path = self.directory.join(&id);
        let receipt = |backup| HydratedAsset {
            path: workdir.join(&relative),
            backup,
        };
        let existing = file_at(&parent, name)?;
        if let Some(file) = existing.as_ref() {
            if matches_fingerprint(file, &asset.source)? {
                let backup = match child_directory(&self.namespace, OsStr::new(&id), false) {
                    Ok(transaction) => match file_at(&transaction, OsStr::new("original"))? {
                        Some(original) => {
                            if !matches_pointer(&original, &asset.payload)? {
                                bail!("retained hydration original changed; edits preserved for manual recovery");
                            }
                            Some(transaction_path.join("original"))
                        }
                        None => None,
                    },
                    Err(error)
                        if error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                    {
                        None
                    }
                    Err(error) => return Err(error),
                };
                verify_parent(&workdir, &root, parent_relative, &parent)?;
                return Ok(receipt(backup));
            }
            if !matches_pointer(file, &asset.payload)? {
                bail!("working asset has local edits; hydration refused");
            }
        }
        let original_reservation = if existing.is_some() {
            Pointer::new(asset.payload.clone())?.encode().len() as u64
        } else {
            0
        };
        self.check_budget(&id, asset.source.bytes(), original_reservation)?;
        let transaction = child_directory(&self.namespace, OsStr::new(&id), true)?;
        let private_path = fd_path(&transaction);
        let raw = serde_json::to_vec(&intent)?;
        if raw.len() > 16 * 1024 {
            bail!("hydration intent exceeds budget");
        }
        let intent_path = private_path.join("intent.json");
        if let Some(mut previous) = file_at(&transaction, OsStr::new("intent.json"))? {
            let mut bytes = Vec::new();
            (&mut previous)
                .take(16 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes != raw {
                bail!("hydration transaction binding differs");
            }
        } else {
            let mut temporary = tempfile::NamedTempFile::new_in(&private_path)?;
            temporary.write_all(&raw)?;
            temporary.as_file().sync_all()?;
            temporary
                .persist_noclobber(&intent_path)
                .map_err(|_| anyhow::anyhow!("hydration intent publication refused"))?;
            transaction.sync_all()?;
        }
        hydration_crash("after-intent");
        // Copy with a bounded digest check, never hard-link an editable checkout
        // to the immutable recovered cache.
        journal::retain_snapshot(
            &private_path,
            "publish",
            SnapshotKind::Source,
            self.limits,
            &mut source,
            &asset.source,
        )?;
        let prepared = private_path.join("publish.source");
        journal::verify_snapshot(&prepared, &asset.source)?;
        verify_parent(&workdir, &root, parent_relative, &parent)?;
        if let Some(original) = file_at(&transaction, OsStr::new("original"))? {
            if !matches_pointer(&original, &asset.payload)? {
                bail!("retained hydration original changed; manual recovery required");
            }
            if file_at(&parent, name)?.is_some() {
                bail!("working path changed during hydration; retained files preserved");
            }
        } else if existing.is_some() {
            hydration_step("before-original-capture", &parent, name);
            verify_parent(&workdir, &root, parent_relative, &parent)?;
            move_create_only(&parent, name, &transaction, OsStr::new("original"))?;
            parent.sync_all()?;
            transaction.sync_all()?;
            hydration_crash("after-original-capture");
            let valid_original = file_at(&transaction, OsStr::new("original"))
                .and_then(|file| file.context("captured hydration original missing"))
                .and_then(|file| matches_pointer(&file, &asset.payload));
            if !matches!(valid_original, Ok(true)) {
                // The captured file changed after preflight. Restore it only if
                // the working path is still absent; a new operator file wins.
                verify_parent(&workdir, &root, parent_relative, &parent)?;
                let _ = move_create_only(&transaction, OsStr::new("original"), &parent, name);
                parent.sync_all()?;
                transaction.sync_all()?;
                bail!(
                    "working asset changed during capture; hydration refused and files preserved"
                );
            }
        }
        verify_parent(&workdir, &root, parent_relative, &parent)?;
        let output = journal::verify_snapshot(&prepared, &asset.source)?;
        let mode = if entry.mode == 0o100755 { 0o700 } else { 0o600 };
        if unsafe { libc::fchmod(output.as_raw_fd(), mode) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        output.sync_all()?;
        hydration_step("before-working-publication", &parent, name);
        verify_parent(&workdir, &root, parent_relative, &parent)?;
        move_create_only(&transaction, OsStr::new("publish.source"), &parent, name)
            .context("working path changed; verified output and original retained")?;
        parent.sync_all()?;
        transaction.sync_all()?;
        hydration_crash("after-working-publication");
        verify_parent(&workdir, &root, parent_relative, &parent)?;
        let published = file_at(&parent, name)?.context("hydrated working file disappeared")?;
        if !matches_fingerprint(&published, &asset.source)? {
            bail!("hydrated working asset changed; edits preserved");
        }
        if let Some(original) = file_at(&transaction, OsStr::new("original"))? {
            if !matches_pointer(&original, &asset.payload)? {
                bail!("retained hydration original changed; edits preserved for manual recovery");
            }
        }
        let backup = transaction_path.join("original");
        Ok(receipt(
            file_at(&transaction, OsStr::new("original"))?
                .is_some()
                .then_some(backup),
        ))
    }

    fn check_budget(&self, id: &str, additional: u64, original_reservation: u64) -> Result<()> {
        let mut count = 0usize;
        let mut retained = 0u64;
        let mut selected = 0u64;
        for entry in std::fs::read_dir(fd_path(&self.namespace))? {
            let entry = entry?;
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some(".gitignore" | ".runtime-ignore.lock" | "hydrate.lock")
            ) {
                continue;
            }
            super::reference::validate_sha256(name.to_str().context("unknown hydration entry")?)?;
            count += 1;
            if count > self.limits.max_records {
                bail!("hydration version capacity exceeded");
            }
            let directory = child_directory(&self.namespace, &name, false)?;
            let mut files = 0usize;
            for file in std::fs::read_dir(fd_path(&directory))? {
                files += 1;
                if files > 32 {
                    bail!("hydration transaction file budget exceeded");
                }
                let file = file?;
                let info = std::fs::symlink_metadata(file.path())?;
                if !info.is_file() || info.uid() != unsafe { libc::geteuid() } || info.nlink() != 1
                {
                    bail!("unsafe retained hydration entry");
                }
                if matches!(
                    file.file_name().to_str(),
                    Some("publish.source" | "publish.capture" | "original")
                ) {
                    retained = retained
                        .checked_add(info.len())
                        .context("hydration retention overflow")?;
                    if name == id && file.file_name() != "original" {
                        selected = selected
                            .checked_add(info.len())
                            .context("hydration selected byte overflow")?;
                    }
                } else {
                    let filename = file.file_name();
                    let name = filename
                        .to_str()
                        .context("unknown hydration transaction entry")?;
                    if !(name == "intent.json"
                        || name == "source-budget.lock"
                        || name.starts_with(".tmp"))
                        || info.len() > 16 * 1024
                        || info.mode() & 0o077 != 0
                    {
                        bail!("unknown or unsafe hydration transaction entry");
                    }
                }
            }
        }
        if (count == self.limits.max_records && !fd_path(&self.namespace).join(id).try_exists()?)
            || retained
                .checked_add(additional.saturating_sub(selected))
                .and_then(|bytes| bytes.checked_add(original_reservation))
                .context("hydration budget overflow")?
                > self.limits.max_retained_snapshot_bytes
        {
            bail!("hydration retention capacity exceeded; previous versions preserved");
        }
        Ok(())
    }
}

fn identity(file: &File) -> Result<(u64, u64)> {
    let info = file.metadata()?;
    Ok((info.dev(), info.ino()))
}
fn transaction_id(intent: &Intent) -> Result<String> {
    let raw = serde_json::to_vec(intent)?;
    if raw.len() > 16 * 1024 {
        bail!("hydration intent exceeds budget");
    }
    let mut hash = Sha256::new();
    hash.update(b"dracon-checkout-hydration-v1\0");
    hash.update(raw);
    Ok(format!("{:x}", hash.finalize()))
}
fn fd_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}
fn component(value: &OsStr) -> Result<CString> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        bail!("invalid hydration path component");
    }
    Ok(CString::new(bytes)?)
}
fn child_directory(parent: &File, name: &OsStr, create: bool) -> Result<File> {
    let name = component(name)?;
    if create && unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
        parent.sync_all()?;
    } else if create {
        parent.sync_all()?;
    }
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
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn directory_at_path(path: &Path) -> Result<File> {
    if !path.is_absolute() {
        bail!("hydration directory must be absolute");
    }
    let root = File::open("/")?;
    let relative = path.strip_prefix("/")?;
    relative_directory(&root, relative)
}
fn relative_directory(root: &File, path: &Path) -> Result<File> {
    let mut directory = root.try_clone()?;
    for part in path.components() {
        let Component::Normal(name) = part else {
            bail!("hydration parent must be confined");
        };
        directory = child_directory(&directory, name, false)?;
    }
    Ok(directory)
}
fn verify_parent(workdir: &Path, root: &File, relative: &Path, pinned: &File) -> Result<()> {
    if identity(&directory_at_path(workdir)?)? != identity(root)?
        || identity(&relative_directory(root, relative)?)? != identity(pinned)?
    {
        bail!("hydration parent directory changed");
    }
    Ok(())
}
fn file_at(directory: &File, name: &OsStr) -> Result<Option<File>> {
    let name = component(name)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.into())
        };
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let info = file.metadata()?;
    if !info.is_file() || info.uid() != unsafe { libc::geteuid() } || info.nlink() != 1 {
        bail!("hydration file must be an ordinary owned file without hard links");
    }
    Ok(Some(file))
}
fn matches_pointer(file: &File, payload: &Fingerprint) -> Result<bool> {
    let expected = Pointer::new(payload.clone())?.encode();
    if file.metadata()?.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    reader
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes == expected)
}
fn matches_fingerprint(file: &File, expected: &Fingerprint) -> Result<bool> {
    if file.metadata()?.len() != expected.bytes() {
        return Ok(false);
    }
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .context("hydration length overflow")?;
        if bytes > expected.bytes() {
            return Ok(false);
        }
        hash.update(&buffer[..count]);
    }
    Ok(bytes == expected.bytes() && format!("{:x}", hash.finalize()) == expected.sha256())
}
fn move_create_only(from: &File, from_name: &OsStr, to: &File, to_name: &OsStr) -> Result<()> {
    let from_name = component(from_name)?;
    let to_name = component(to_name)?;
    if unsafe {
        libc::renameat2(
            from.as_raw_fd(),
            from_name.as_ptr(),
            to.as_raw_fd(),
            to_name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
#[cfg(not(test))]
fn hydration_crash(_: &str) {}
#[cfg(test)]
fn hydration_crash(phase: &str) {
    if std::env::var("DRACON_HYDRATION_CRASH_POINT")
        .ok()
        .as_deref()
        == Some(phase)
    {
        std::process::exit(73);
    }
}

fn hydration_step(phase: &str, _parent: &File, _name: &OsStr) {
    #[cfg(test)]
    tests::race_at(phase, _parent, _name);
    hydration_crash(phase);
}

#[cfg(test)]
mod tests;
