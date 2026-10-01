//! Atomic publication of matched pointers/protected metadata into a Git index.
//!
//! This library gate performs no working-tree writes, filters, commits or pushes.
//! Production setup, working-file races and outgoing-commit validation remain
//! caller responsibilities. Existing raw tracked paths require reviewed migration.

use anyhow::{bail, Context, Result};
use git2::{Index, IndexEntry, IndexTime, ObjectType, Oid, Repository};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::backend::BackendFailure;
use super::journal::{self, Phase};
use super::manifest::Manifest;
use super::reference::{Fingerprint, Pointer};
use super::staging::{decode_path, StageBundle};

const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INTENT_BYTES: u64 = 16 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// An observed index version. Mutation rejects a different version under Git's lock.
#[derive(Clone)]
pub struct IndexSnapshot(Option<Fingerprint>);

/// Actual published index evidence. This is staging evidence, not a Git commit.
pub struct IndexProof {
    metadata_oid: String,
}

impl IndexProof {
    /// Git's blob OID for the protected manifest, distinct from content SHA-256.
    pub fn metadata_oid(&self) -> &str {
        &self.metadata_oid
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    group: String,
    base: Option<Fingerprint>,
    candidate: Fingerprint,
    filename: String,
    device: u64,
    inode: u64,
    metadata_oid: String,
}

/// Operator-bound repository and dedicated private transaction state.
pub struct IndexTransaction {
    repo: Repository,
    repo_id: String,
    state: PathBuf,
    metadata_path: Vec<u8>,
}

impl IndexTransaction {
    /// Bind an actual worktree with a matching local `dracon.storageRepoId`.
    /// The caller approves the reserved metadata path and a separate private root.
    pub fn open(
        repo: &Path,
        repo_id: &str,
        state_root: &Path,
        metadata_path: &[u8],
    ) -> Result<Self> {
        super::reference::validate_sha256(repo_id)?;
        journal::encode_relative_path(metadata_path)?;
        let repository = Repository::open(repo)?;
        let workdir = repository.workdir().ok_or(BackendFailure::Security)?;
        if workdir.canonicalize()? != repo.canonicalize()?
            || repository
                .config()?
                .open_level(git2::ConfigLevel::Local)?
                .get_string("dracon.storageRepoId")
                .ok()
                .as_deref()
                != Some(repo_id)
        {
            bail!(BackendFailure::Security);
        }
        journal::private_directory(state_root, true)?;
        journal::runtime::protect(state_root)?;
        let context = format!(
            "{:x}",
            Sha256::digest(path_bytes(&repository.path().canonicalize()?)?)
        );
        let state = state_root.join(format!("{repo_id}-{context}"));
        journal::private_directory(&state, true)?;
        journal::runtime::protect(&state)?;
        Ok(Self {
            repo: repository,
            repo_id: repo_id.into(),
            state,
            metadata_path: metadata_path.to_vec(),
        })
    }

    /// Observe the actual per-worktree index without creating it or running filters.
    pub fn snapshot(&self) -> Result<IndexSnapshot> {
        Ok(IndexSnapshot(fingerprint_optional(
            &self.repo.path().join("index"),
        )?))
    }

    /// Stage the complete matching pair atomically and record verified job staging.
    /// Unrelated entries survive; a changed manual index is never overwritten.
    /// Retrying a saved matching transaction reconciles before/after publication.
    pub fn stage(&self, snapshot: &IndexSnapshot, bundle: &StageBundle<'_>) -> Result<IndexProof> {
        if bundle.repo_id() != self.repo_id
            || bundle
                .manifest()
                .enrollment(&journal::encode_relative_path(&self.metadata_path)?)
                .is_some()
        {
            bail!(BackendFailure::Security);
        }
        journal::runtime::protect(&self.state)?;
        let _lease = journal::try_lock(&self.state.join("transaction.lock"))?;
        let group = group_id(bundle, &self.metadata_path)?;
        let metadata_oid = self.write_metadata(bundle)?;
        let current = self.repo.index()?;
        let intent_path = self.state.join("intent.json");
        if journal::exists_without_symlink(&intent_path)? {
            let intent = self.read_intent()?;
            if intent.group != group || intent.metadata_oid != metadata_oid.to_string() {
                bail!("a different staging transaction needs reconciliation");
            }
            if desired(
                &self.repo,
                &current,
                bundle,
                &self.metadata_path,
                metadata_oid,
            )? {
                self.cleanup(&intent)?;
                return self.acknowledge(bundle, metadata_oid);
            }
            self.publish(&intent)?;
            self.verify(bundle, metadata_oid)?;
            self.cleanup(&intent)?;
            return self.acknowledge(bundle, metadata_oid);
        }
        if desired(
            &self.repo,
            &current,
            bundle,
            &self.metadata_path,
            metadata_oid,
        )? {
            return self.acknowledge(bundle, metadata_oid);
        }
        if self.snapshot()?.0 != snapshot.0 {
            bail!("manual index change conflicts with staging");
        }
        self.check_touched(&current, bundle)?;
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let filename = format!(".dracon-storage-index-{}-{sequence}", std::process::id());
        let temporary = self.repo.path().join(&filename);
        let result = (|| -> Result<()> {
            if snapshot.0.is_some() {
                let mut source = owned_file(&self.repo.path().join("index"))?;
                let mut output = create_private(&temporary)?;
                let fp = copy_fingerprint(&mut source, &mut output, MAX_INDEX_BYTES)?;
                if Some(fp) != snapshot.0 {
                    bail!("index changed while preparing staging");
                }
                output.sync_all()?;
            }
            let mut candidate = Index::open(&temporary)?;
            for (path, pointer) in bundle.pointers() {
                let oid = self.repo.blob(&pointer.encode())?;
                let mode = candidate
                    .iter()
                    .find(|entry| entry.path == *path)
                    .map_or(0o100644, |entry| entry.mode);
                candidate.add(&entry(path, oid, mode, pointer.encode().len() as u32))?;
            }
            for enrolled in bundle.manifest().enrollments() {
                if enrolled.payload.is_none() {
                    let path = decode_path(&enrolled.path_hex)?;
                    if candidate.iter().any(|entry| entry.path == path) {
                        candidate.remove_path(&os_path(&path)?)?;
                    }
                }
            }
            candidate.add(&entry(
                &self.metadata_path,
                metadata_oid,
                0o100644,
                u32::try_from(bundle.prepared().payload().bytes())?,
            ))?;
            candidate.write()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
            }
            owned_file(&temporary)?.sync_all()?;
            if !desired(
                &self.repo,
                &candidate,
                bundle,
                &self.metadata_path,
                metadata_oid,
            )? {
                bail!(BackendFailure::Integrity);
            }
            let info = owned_file(&temporary)?.metadata()?;
            let (device, inode) = identity(&info)?;
            let intent = Intent {
                version: 1,
                group,
                base: snapshot.0.clone(),
                candidate: fingerprint_optional(&temporary)?.ok_or(BackendFailure::Integrity)?,
                filename,
                device,
                inode,
                metadata_oid: metadata_oid.to_string(),
            };
            let raw = serde_json::to_vec(&intent)?;
            journal::atomic_bytes(&self.state, &intent_path, &raw, true)?;
            index_crash("after-index-intent");
            self.publish(&intent)?;
            self.verify(bundle, metadata_oid)?;
            self.cleanup(&intent)
        })();
        // Once an intent is durable, its complete candidate belongs to recovery.
        if !intent_path.exists() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        self.acknowledge(bundle, metadata_oid)
    }

    fn write_metadata(&self, bundle: &StageBundle<'_>) -> Result<Oid> {
        let mut input = bundle.metadata()?;
        let odb = self.repo.odb()?;
        let mut output = odb.writer(
            usize::try_from(bundle.prepared().payload().bytes())?,
            ObjectType::Blob,
        )?;
        let actual = copy_fingerprint(
            &mut input,
            &mut output,
            super::metadata::MAX_PROTECTED_MANIFEST_BYTES,
        )?;
        if &actual != bundle.prepared().payload() {
            bail!(BackendFailure::Integrity);
        }
        Ok(output.finalize()?)
    }

    fn check_touched(&self, index: &Index, bundle: &StageBundle<'_>) -> Result<()> {
        if index.has_conflicts() {
            bail!("unresolved index conflicts block staging");
        }
        let mut touched: BTreeSet<Vec<u8>> = bundle
            .pointers()
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        touched.insert(self.metadata_path.clone());
        for enrolled in bundle.manifest().enrollments() {
            if enrolled.payload.is_none() {
                touched.insert(decode_path(&enrolled.path_hex)?);
            }
        }
        let head = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok());
        for existing in index.iter() {
            if touched.contains(&existing.path) {
                let original = head
                    .as_ref()
                    .and_then(|tree| tree.get_path(&os_path(&existing.path).ok()?).ok());
                if original
                    .as_ref()
                    .map(|entry| (entry.id(), entry.filemode() as u32))
                    != Some((existing.id, existing.mode))
                {
                    bail!("manual staged edit conflicts with a managed path");
                }
                if existing.path != self.metadata_path {
                    if !matches!(existing.mode, 0o100644 | 0o100755)
                        || Pointer::parse(self.repo.find_blob(existing.id)?.content()).is_err()
                    {
                        bail!("tracked raw paths require reviewed external migration");
                    }
                }
            }
            for path in &touched {
                if path != &existing.path
                    && (prefix(path, &existing.path) || prefix(&existing.path, path))
                {
                    bail!("directory or gitlink collision blocks staging");
                }
            }
        }
        Ok(())
    }

    fn read_intent(&self) -> Result<Intent> {
        let mut raw = Vec::new();
        journal::open_private(&self.state.join("intent.json"), false, false)?
            .take(MAX_INTENT_BYTES + 1)
            .read_to_end(&mut raw)?;
        if raw.len() as u64 > MAX_INTENT_BYTES {
            bail!(BackendFailure::Capacity);
        }
        let intent: Intent =
            serde_json::from_slice(&raw).map_err(|_| anyhow::anyhow!("invalid staging intent"))?;
        super::reference::validate_sha256(&intent.group)?;
        intent.candidate.validate()?;
        if let Some(base) = &intent.base {
            base.validate()?;
        }
        if intent.version != 1
            || !intent.filename.starts_with(".dracon-storage-index-")
            || !intent
                .filename
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
            || intent.filename.len() > 128
            || intent.candidate.bytes() > MAX_INDEX_BYTES
        {
            bail!(BackendFailure::Integrity);
        }
        Oid::from_str(&intent.metadata_oid)?;
        Ok(intent)
    }

    fn publish(&self, intent: &Intent) -> Result<()> {
        let candidate = self.repo.path().join(&intent.filename);
        let file = owned_file(&candidate)?;
        if identity(&file.metadata()?)? != (intent.device, intent.inode)
            || fingerprint_optional(&candidate)?.as_ref() != Some(&intent.candidate)
        {
            bail!(BackendFailure::Integrity);
        }
        let lock = self.repo.path().join("index.lock");
        if journal::exists_without_symlink(&lock)? {
            if identity(&owned_file(&lock)?.metadata()?)? != (intent.device, intent.inode) {
                bail!("foreign Git index lock blocks staging");
            }
        } else {
            std::fs::hard_link(&candidate, &lock)?;
            File::open(self.repo.path())?.sync_all()?;
        }
        if self.snapshot()?.0 != intent.base {
            bail!("manual index change conflicts with saved staging");
        }
        index_crash("before-index-publish");
        std::fs::rename(&lock, self.repo.path().join("index"))?;
        File::open(self.repo.path())?.sync_all()?;
        index_crash("after-index-publish");
        Ok(())
    }

    fn verify(&self, bundle: &StageBundle<'_>, metadata: Oid) -> Result<()> {
        if !desired(
            &self.repo,
            &self.repo.index()?,
            bundle,
            &self.metadata_path,
            metadata,
        )? {
            bail!(BackendFailure::Integrity);
        }
        Ok(())
    }

    fn cleanup(&self, intent: &Intent) -> Result<()> {
        let lock = self.repo.path().join("index.lock");
        if journal::exists_without_symlink(&lock)?
            && identity(&owned_file(&lock)?.metadata()?)? == (intent.device, intent.inode)
        {
            std::fs::remove_file(lock)?;
        }
        let temporary = self.repo.path().join(&intent.filename);
        if journal::exists_without_symlink(&temporary)? {
            if identity(&owned_file(&temporary)?.metadata()?)? != (intent.device, intent.inode) {
                bail!(BackendFailure::Security);
            }
            std::fs::remove_file(temporary)?;
        }
        File::open(self.repo.path())?.sync_all()?;
        std::fs::remove_file(self.state.join("intent.json"))?;
        File::open(&self.state)?.sync_all()?;
        Ok(())
    }

    fn acknowledge(&self, bundle: &StageBundle<'_>, metadata: Oid) -> Result<IndexProof> {
        self.verify(bundle, metadata)?;
        for lease in bundle.jobs() {
            let mut job = lease.load()?;
            if job.phase() == Phase::ReadyToStage {
                job.record_staged(&Pointer::new(
                    job.payload().cloned().ok_or(BackendFailure::Integrity)?,
                )?)?;
                lease.save(&mut job)?;
            }
        }
        Ok(IndexProof {
            metadata_oid: metadata.to_string(),
        })
    }
}

use std::collections::BTreeSet;

fn desired(
    repo: &Repository,
    index: &Index,
    bundle: &StageBundle<'_>,
    metadata_path: &[u8],
    metadata: Oid,
) -> Result<bool> {
    let Some(meta) = index.iter().find(|entry| entry.path == metadata_path) else {
        return Ok(false);
    };
    if meta.id != metadata || meta.mode != 0o100644 {
        return Ok(false);
    }
    for enrolled in bundle.manifest().enrollments() {
        let path = decode_path(&enrolled.path_hex)?;
        let entry = index.iter().find(|entry| entry.path == path);
        match (&enrolled.payload, entry) {
            (None, None) => {}
            (Some(payload), Some(entry)) if matches!(entry.mode, 0o100644 | 0o100755) => {
                let blob = repo.find_blob(entry.id)?;
                if Pointer::parse(blob.content())
                    .ok()
                    .as_ref()
                    .map(Pointer::payload)
                    != Some(payload)
                {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn group_id(bundle: &StageBundle<'_>, metadata_path: &[u8]) -> Result<String> {
    let mut jobs = bundle
        .jobs()
        .iter()
        .map(|lease| lease.load().map(|job| job.id().to_owned()))
        .collect::<Result<Vec<_>>>()?;
    jobs.sort();
    let mut hash = Sha256::new();
    hash.update(b"dracon-index-group-v1\0");
    hash.update(serde_json::to_vec(&(
        bundle.repo_id(),
        bundle.prepared().id(),
        metadata_path,
        jobs,
    ))?);
    Ok(format!("{:x}", hash.finalize()))
}

fn entry(path: &[u8], oid: Oid, mode: u32, size: u32) -> IndexEntry {
    IndexEntry {
        ctime: IndexTime::new(0, 0),
        mtime: IndexTime::new(0, 0),
        dev: 0,
        ino: 0,
        mode,
        uid: 0,
        gid: 0,
        file_size: size,
        id: oid,
        flags: 0,
        flags_extended: 0,
        path: path.to_vec(),
    }
}

fn prefix(left: &[u8], right: &[u8]) -> bool {
    right.starts_with(left) && right.get(left.len()) == Some(&b'/')
}

fn fingerprint_optional(path: &Path) -> Result<Option<Fingerprint>> {
    if !journal::exists_without_symlink(path)? {
        return Ok(None);
    }
    let mut file = owned_file(path)?;
    Ok(Some(copy_fingerprint(
        &mut file,
        &mut std::io::sink(),
        MAX_INDEX_BYTES,
    )?))
}

fn copy_fingerprint(input: &mut File, output: &mut dyn Write, limit: u64) -> Result<Fingerprint> {
    input.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or(BackendFailure::Capacity)?;
        if bytes > limit {
            bail!(BackendFailure::Capacity);
        }
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }
    Fingerprint::new(format!("{:x}", hash.finalize()), bytes)
}

fn owned_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!(BackendFailure::Security);
        }
    }
    if !metadata.is_file() {
        bail!(BackendFailure::Security);
    }
    Ok(file)
}

fn create_private(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    Ok(options.open(path)?)
}

#[cfg(unix)]
fn identity(metadata: &std::fs::Metadata) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}
#[cfg(not(unix))]
fn identity(_metadata: &std::fs::Metadata) -> Result<(u64, u64)> {
    bail!("index transactions require Unix")
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(path.as_os_str().as_bytes().to_vec())
}
#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Result<Vec<u8>> {
    Ok(path
        .to_str()
        .context("unsupported path encoding")?
        .as_bytes()
        .to_vec())
}

#[cfg(unix)]
fn os_path(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_vec(bytes.to_vec()).into())
}
#[cfg(not(unix))]
fn os_path(bytes: &[u8]) -> Result<PathBuf> {
    Ok(std::str::from_utf8(bytes)?.into())
}

#[cfg(not(test))]
fn index_crash(_point: &str) {}
#[cfg(test)]
fn index_crash(point: &str) {
    if std::env::var("DRACON_INDEX_CRASH_POINT").ok().as_deref() == Some(point) {
        std::process::exit(75);
    }
}
