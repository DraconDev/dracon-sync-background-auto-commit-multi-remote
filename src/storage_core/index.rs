//! Atomic publication of matched pointers/protected metadata into a Git index.
//!
//! This library gate performs no working-tree writes, filters, commits or pushes.
//! Production setup, working-file races and outgoing-commit validation remain
//! caller responsibilities. Existing raw tracked paths require reviewed migration.

#[cfg(not(unix))]
use anyhow::Context;
use anyhow::{bail, Result};
use git2::{Index, IndexEntry, IndexTime, ObjectType, Oid, Repository};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::backend::BackendFailure;
use super::journal::{self, Phase};
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
        let current = fresh_index(&self.repo)?;
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
        self.check_enrollment(&current, bundle)?;
        self.check_touched(&current, bundle)?;
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let filename = format!(".dracon-storage-index-{}-{sequence}", std::process::id());
        let temporary = self.repo.path().join(&filename);
        if journal::exists_without_symlink(&temporary)? {
            bail!("staging candidate filename is already occupied");
        }
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

    fn check_enrollment(&self, index: &Index, bundle: &StageBundle<'_>) -> Result<()> {
        let previous =
            if let Some(entry) = index.iter().find(|entry| entry.path == self.metadata_path) {
                if entry.mode != 0o100644 {
                    bail!(BackendFailure::Security);
                }
                let (size, kind) = self.repo.odb()?.read_header(entry.id)?;
                if kind != ObjectType::Blob
                    || size as u64 > super::metadata::MAX_PROTECTED_MANIFEST_BYTES
                {
                    bail!(BackendFailure::Capacity);
                }
                let blob = self.repo.find_blob(entry.id)?;
                let fp = Fingerprint::new(
                    format!("{:x}", Sha256::digest(blob.content())),
                    blob.size() as u64,
                )?;
                Some(bundle.previous_manifest(&fp)?)
            } else {
                None
            };
        if let Some(previous) = &previous {
            if previous.repo_id() != self.repo_id {
                bail!(BackendFailure::Security);
            }
            for old in previous.enrollments() {
                if bundle
                    .manifest()
                    .enrollment(&old.path_hex)
                    .is_none_or(|new| !old.same_contract(new))
                {
                    bail!("sticky enrollment changes require reviewed migration");
                }
            }
        }
        for new in bundle.manifest().enrollments() {
            let path = decode_path(&new.path_hex)?;
            let old = previous
                .as_ref()
                .and_then(|manifest| manifest.enrollment(&new.path_hex));
            if previous.is_none() && index.iter().any(|entry| entry.path == path) {
                bail!("existing tracked paths require reviewed enrollment migration");
            }
            if new.payload.is_some()
                && !bundle
                    .pointers()
                    .iter()
                    .any(|(updated, _)| updated == &path)
                && old.is_none_or(|old| old.payload != new.payload)
            {
                bail!("changed asset reference lacks a verified staging job");
            }
            if new.payload.is_none() && old.is_none() {
                bail!("new enrollment requires a verified asset version");
            }
        }
        Ok(())
    }

    fn check_touched(&self, index: &Index, bundle: &StageBundle<'_>) -> Result<()> {
        if index.has_conflicts() {
            bail!("unresolved index conflicts block staging");
        }
        let mut touched = BTreeSet::new();
        touched.insert(self.metadata_path.clone());
        for enrolled in bundle.manifest().enrollments() {
            touched.insert(decode_path(&enrolled.path_hex)?);
        }
        let head = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_tree().ok());
        for path in &touched {
            let existing = index.iter().find(|entry| &entry.path == path);
            let original = head
                .as_ref()
                .and_then(|tree| tree.get_path(&os_path(path).ok()?).ok());
            if original
                .as_ref()
                .map(|entry| (entry.id(), entry.filemode() as u32))
                != existing.as_ref().map(|entry| (entry.id, entry.mode))
            {
                bail!("manual staged edit or deletion conflicts with a managed path");
            }
            if let Some(existing) = existing {
                if path != &self.metadata_path
                    && (!matches!(existing.mode, 0o100644 | 0o100755)
                        || bounded_pointer(&self.repo, existing.id)?.is_none())
                {
                    bail!("tracked raw paths require reviewed external migration");
                }
            }
        }
        for existing in index.iter() {
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
            self.cleanup(intent)?;
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
            &fresh_index(&self.repo)?,
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

fn fresh_index(repo: &Repository) -> Result<Index> {
    let path = repo.path().join("index");
    if journal::exists_without_symlink(&path)?
        && owned_file(&path)?.metadata()?.len() > MAX_INDEX_BYTES
    {
        bail!(BackendFailure::Capacity);
    }
    let mut index = repo.index()?;
    index.read(true)?;
    Ok(index)
}

fn bounded_pointer(repo: &Repository, oid: Oid) -> Result<Option<Pointer>> {
    let (size, kind) = repo.odb()?.read_header(oid)?;
    if kind != ObjectType::Blob || size > 1024 {
        return Ok(None);
    }
    Ok(Pointer::parse(repo.find_blob(oid)?.content()).ok())
}

fn desired(
    repo: &Repository,
    index: &Index,
    bundle: &StageBundle<'_>,
    metadata_path: &[u8],
    metadata: Oid,
) -> Result<bool> {
    if index.has_conflicts() {
        return Ok(false);
    }
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
                if bounded_pointer(repo, entry.id)?
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::storage_core::backend::LocalBackend;
    use crate::storage_core::bindings::{ApprovedBackend, CopyBindings};
    use crate::storage_core::journal::{Encryption, JobSpec, Journal, Limits};
    use crate::storage_core::manifest::{Enrollment, Manifest};
    use crate::storage_core::metadata::{MetadataStore, PreparedMetadata};
    use crate::storage_core::security::WardenAdapter;
    use crate::storage_core::transfer::transfer_copies;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    struct Fixture {
        journal: Journal,
        job: String,
        store: MetadataStore,
        manifest: Manifest,
        prepared: PreparedMetadata,
        transaction: IndexTransaction,
        adapter: WardenAdapter,
    }

    async fn fixture(root: &Path) -> Fixture {
        fixture_path(root, b"asset.bin").await
    }

    async fn fixture_path(root: &Path, asset_path: &[u8]) -> Fixture {
        let repo_path = root.join("repo");
        let repo = Repository::init(&repo_path).unwrap();
        repo.config()
            .unwrap()
            .set_str("dracon.storageRepoId", &"a".repeat(64))
            .unwrap();
        if repo.head().is_err() {
            let oid = repo.blob(b"original note").unwrap();
            let mut index = repo.index().unwrap();
            index.add(&entry(b"note.md", oid, 0o100644, 13)).unwrap();
            index.write().unwrap();
            let tree = index.write_tree().unwrap();
            let signature = git2::Signature::now("DraconDev", "dracsharp@gmail.com").unwrap();
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                "fixture",
                &repo.find_tree(tree).unwrap(),
                &[],
            )
            .unwrap();
        }
        let bytes = b"explicitly approved non-sensitive fixture asset";
        std::fs::write(repo_path.join(os_path(asset_path).unwrap()), bytes).unwrap();
        let payload =
            Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
        let spec = JobSpec {
            repo_id: "a".repeat(64),
            path_hex: journal::encode_relative_path(asset_path).unwrap(),
            source: payload.clone(),
            policy_sha256: "b".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into(), "recovery".into()],
            required_git_targets: vec!["github".into()],
            encryption: Encryption::None,
        };
        let journal =
            Journal::open(&root.join("journal"), &spec.repo_id, Limits::default()).unwrap();
        let job = journal.create(spec.clone()).unwrap();
        {
            let lease = journal.lease(job.id()).unwrap();
            if lease.load().unwrap().phase() < Phase::Prepared {
                lease.capture_snapshot(&mut &bytes[..]).unwrap();
                lease.retain_payload(&mut &bytes[..], &payload).unwrap();
            }
            if lease.load().unwrap().phase() < Phase::ReadyToStage {
                let primary = LocalBackend::open(&root.join("primary"), 1024).unwrap();
                let recovery = LocalBackend::open(&root.join("recovery"), 1024).unwrap();
                let bindings = CopyBindings::new(
                    spec.repo_id.clone(),
                    BTreeMap::from([
                        (
                            "primary".into(),
                            ApprovedBackend::for_security(&primary, vec![Encryption::None])
                                .unwrap(),
                        ),
                        (
                            "recovery".into(),
                            ApprovedBackend::for_security(&recovery, vec![Encryption::None])
                                .unwrap(),
                        ),
                    ]),
                )
                .unwrap();
                transfer_copies(&lease, &bindings, 1).unwrap();
            }
        }
        let manifest = Manifest::new(
            spec.repo_id.clone(),
            vec![Enrollment {
                path_hex: spec.path_hex,
                contract_sha256: spec.policy_sha256,
                primary: spec.primary,
                required_copies: spec.required_copies,
                encryption: spec.encryption,
                payload: Some(payload),
            }],
        )
        .unwrap();
        let binary = root.join("warden-fixture");
        // Synthetic subprocess contract fixture, not a cryptography test.
        std::fs::write(
            &binary,
            "#!/bin/sh\nprintf 'age-encryption.org/v1\\n'\ncat\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let adapter = WardenAdapter::new(
            &binary,
            &repo_path,
            manifest.repo_id(),
            Duration::from_secs(5),
        )
        .unwrap();
        let store = MetadataStore::open(
            &root.join("metadata"),
            manifest.repo_id(),
            Limits::default(),
        )
        .unwrap();
        let prepared = store
            .prepare(&manifest, &"b".repeat(64), &adapter, 1)
            .await
            .unwrap();
        let transaction = IndexTransaction::open(
            &repo_path,
            manifest.repo_id(),
            &root.join("transactions"),
            b".dracon/assets.manifest",
        )
        .unwrap();
        Fixture {
            journal,
            job: job.id().into(),
            store,
            manifest,
            prepared,
            transaction,
            adapter,
        }
    }

    fn stage(f: &Fixture) -> Result<IndexProof> {
        let lease = f.journal.lease(&f.job)?;
        let bundle = StageBundle::build(&f.store, &f.prepared, &f.manifest, vec![&lease])?;
        f.transaction.stage(&f.transaction.snapshot()?, &bundle)
    }

    #[tokio::test]
    async fn matched_index_pair_preserves_unrelated_staging_and_source() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        let repo = &f.transaction.repo;
        let oid = repo.blob(b"operator staged note").unwrap();
        let mut index = repo.index().unwrap();
        index.add(&entry(b"note.md", oid, 0o100644, 20)).unwrap();
        index.write().unwrap();
        let source = std::fs::read(repo.workdir().unwrap().join("asset.bin")).unwrap();
        let proof = stage(&f).unwrap();
        let index = fresh_index(repo).unwrap();
        assert_eq!(index.get_path(Path::new("note.md"), 0).unwrap().id, oid);
        let asset = index.get_path(Path::new("asset.bin"), 0).unwrap();
        assert!(f.manifest.enrollments()[0].matches_pointer(
            &Pointer::parse(repo.find_blob(asset.id).unwrap().content()).unwrap()
        ));
        let meta = index
            .get_path(Path::new(".dracon/assets.manifest"), 0)
            .unwrap();
        assert_eq!(meta.id.to_string(), proof.metadata_oid());
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(repo.find_blob(meta.id).unwrap().content())
            ),
            f.prepared.payload().sha256()
        );
        assert_eq!(
            std::fs::read(repo.workdir().unwrap().join("asset.bin")).unwrap(),
            source
        );
        assert_eq!(
            f.journal.lease(&f.job).unwrap().load().unwrap().phase(),
            Phase::Staged
        );
        assert!(stage(&f).is_ok());
        assert!(!repo.path().join("index.lock").exists());
    }

    #[tokio::test]
    async fn manual_index_changes_and_foreign_locks_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        let snapshot = f.transaction.snapshot().unwrap();
        let lease = f.journal.lease(&f.job).unwrap();
        let bundle = StageBundle::build(&f.store, &f.prepared, &f.manifest, vec![&lease]).unwrap();
        let mut index = f.transaction.repo.index().unwrap();
        let oid = f
            .transaction
            .repo
            .blob(b"manually staged raw bytes")
            .unwrap();
        index.add(&entry(b"asset.bin", oid, 0o100644, 25)).unwrap();
        index.write().unwrap();
        let before = std::fs::read(f.transaction.repo.path().join("index")).unwrap();
        assert!(f.transaction.stage(&snapshot, &bundle).is_err());
        assert!(f
            .transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .is_err());
        assert_eq!(
            std::fs::read(f.transaction.repo.path().join("index")).unwrap(),
            before
        );
        index.remove_path(Path::new("asset.bin")).unwrap();
        index.write().unwrap();
        let lock = f.transaction.repo.path().join("index.lock");
        std::fs::write(&lock, b"foreign Git process owns this").unwrap();
        assert!(f
            .transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .is_err());
        assert_eq!(
            std::fs::read(&lock).unwrap(),
            b"foreign Git process owns this"
        );
        assert_eq!(lease.load().unwrap().phase(), Phase::ReadyToStage);
        // Test-owned foreign lock only.
        std::fs::remove_file(lock).unwrap();
        f.transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .unwrap();
    }

    #[tokio::test]
    async fn approved_metadata_cannot_be_substituted_for_another_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        let lease = f.journal.lease(&f.job).unwrap();
        let wrong = Manifest::new("a".repeat(64), vec![]).unwrap();
        assert!(StageBundle::build(&f.store, &f.prepared, &wrong, vec![&lease]).is_err());
        assert!(
            StageBundle::build(&f.store, &f.prepared, &f.manifest, vec![&lease, &lease]).is_err()
        );
        let mut job = lease.load().unwrap();
        job.note_failure(journal::FailureCode::Integrity, None)
            .unwrap();
        lease.save(&mut job).unwrap();
        assert!(StageBundle::build(&f.store, &f.prepared, &f.manifest, vec![&lease]).is_err());
        assert!(f
            .transaction
            .repo
            .index()
            .unwrap()
            .get_path(Path::new("asset.bin"), 0)
            .is_none());
    }

    #[tokio::test]
    async fn sticky_contracts_cannot_disappear_or_change_without_migration() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        stage(&f).unwrap();
        let empty = Manifest::new("a".repeat(64), vec![]).unwrap();
        let prepared = f
            .store
            .prepare(&empty, &"b".repeat(64), &f.adapter, 1)
            .await
            .unwrap();
        let bundle = StageBundle::build(&f.store, &prepared, &empty, vec![]).unwrap();
        let before = std::fs::read(f.transaction.repo.path().join("index")).unwrap();
        assert!(f
            .transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .is_err());
        assert_eq!(
            std::fs::read(f.transaction.repo.path().join("index")).unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn changed_reference_without_verified_job_and_unknown_prior_metadata_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        stage(&f).unwrap();
        commit_index(&f.transaction.repo);
        let mut enrolled = f.manifest.enrollments().to_vec();
        enrolled[0].payload = Some(Fingerprint::new("f".repeat(64), 123).unwrap());
        let manifest = Manifest::new(f.manifest.repo_id().into(), enrolled).unwrap();
        let prepared = f
            .store
            .prepare(&manifest, &"b".repeat(64), &f.adapter, 1)
            .await
            .unwrap();
        let bundle = StageBundle::build(&f.store, &prepared, &manifest, vec![]).unwrap();
        let before = std::fs::read(f.transaction.repo.path().join("index")).unwrap();
        assert!(f
            .transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .is_err());
        assert_eq!(
            std::fs::read(f.transaction.repo.path().join("index")).unwrap(),
            before
        );
        let mut index = fresh_index(&f.transaction.repo).unwrap();
        let unknown = f
            .transaction
            .repo
            .blob(b"age-encryption.org/v1\nunknown prior metadata")
            .unwrap();
        index
            .add(&entry(b".dracon/assets.manifest", unknown, 0o100644, 47))
            .unwrap();
        index.write().unwrap();
        let before = std::fs::read(f.transaction.repo.path().join("index")).unwrap();
        assert!(f
            .transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .is_err());
        assert_eq!(
            std::fs::read(f.transaction.repo.path().join("index")).unwrap(),
            before
        );
    }

    fn commit_index(repo: &Repository) {
        let mut index = fresh_index(repo).unwrap();
        let tree = index.write_tree().unwrap();
        let signature = git2::Signature::now("DraconDev", "dracsharp@gmail.com").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "prepared pair fixture",
            &repo.find_tree(tree).unwrap(),
            &[&parent],
        )
        .unwrap();
    }

    #[tokio::test]
    async fn manual_managed_deletion_is_preserved_but_explicit_tombstone_stages() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        stage(&f).unwrap();
        commit_index(&f.transaction.repo);
        let mut enrolled = f.manifest.enrollments().to_vec();
        enrolled[0].payload = None;
        let tombstone = Manifest::new(f.manifest.repo_id().into(), enrolled).unwrap();
        let prepared = f
            .store
            .prepare(&tombstone, &"b".repeat(64), &f.adapter, 1)
            .await
            .unwrap();
        let bundle = StageBundle::build(&f.store, &prepared, &tombstone, vec![]).unwrap();
        let mut index = fresh_index(&f.transaction.repo).unwrap();
        index.remove_path(Path::new("asset.bin")).unwrap();
        index.write().unwrap();
        let before = std::fs::read(f.transaction.repo.path().join("index")).unwrap();
        assert!(f
            .transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .is_err());
        assert_eq!(
            std::fs::read(f.transaction.repo.path().join("index")).unwrap(),
            before
        );
        // Restore only the fixture index to its original committed pointer.
        let tree = f.transaction.repo.head().unwrap().peel_to_tree().unwrap();
        index.read_tree(&tree).unwrap();
        index.write().unwrap();
        f.transaction
            .stage(&f.transaction.snapshot().unwrap(), &bundle)
            .unwrap();
        let index = fresh_index(&f.transaction.repo).unwrap();
        assert!(index.get_path(Path::new("asset.bin"), 0).is_none());
        assert!(index
            .get_path(Path::new(".dracon/assets.manifest"), 0)
            .is_some());
        assert!(f
            .transaction
            .repo
            .workdir()
            .unwrap()
            .join("asset.bin")
            .exists());
    }

    #[tokio::test]
    async fn lossless_non_utf8_paths_are_staged_as_exact_index_entries() {
        let temp = tempfile::tempdir().unwrap();
        let path = b"asset [raw] \xff.bin";
        let f = fixture_path(temp.path(), path).await;
        stage(&f).unwrap();
        let index = fresh_index(&f.transaction.repo).unwrap();
        let asset = index.iter().find(|entry| entry.path == path).unwrap();
        assert!(Pointer::parse(f.transaction.repo.find_blob(asset.id).unwrap().content()).is_ok());
        assert!(f
            .transaction
            .repo
            .workdir()
            .unwrap()
            .join(os_path(path).unwrap())
            .exists());
    }

    #[tokio::test]
    async fn saved_intent_conflict_releases_only_its_owned_lock() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        let lock = f.transaction.repo.path().join("index.lock");
        std::fs::write(&lock, b"foreign process").unwrap();
        assert!(stage(&f).is_err());
        assert!(f.transaction.state.join("intent.json").exists());
        std::fs::remove_file(&lock).unwrap();
        let mut index = fresh_index(&f.transaction.repo).unwrap();
        let note = f
            .transaction
            .repo
            .blob(b"concurrent unrelated edit")
            .unwrap();
        index.add(&entry(b"note.md", note, 0o100644, 25)).unwrap();
        index.write().unwrap();
        let before = std::fs::read(f.transaction.repo.path().join("index")).unwrap();
        assert!(stage(&f).is_err());
        assert!(!lock.exists());
        assert!(!f.transaction.state.join("intent.json").exists());
        assert_eq!(
            std::fs::read(f.transaction.repo.path().join("index")).unwrap(),
            before
        );
        stage(&f).unwrap();
        assert_eq!(
            fresh_index(&f.transaction.repo)
                .unwrap()
                .get_path(Path::new("note.md"), 0)
                .unwrap()
                .id,
            note
        );
    }

    #[tokio::test]
    async fn oversized_physical_index_is_rejected_before_libgit2_decode() {
        let temp = tempfile::tempdir().unwrap();
        let f = fixture(temp.path()).await;
        let path = f.transaction.repo.path().join("index");
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_INDEX_BYTES + 1).unwrap();
        let error = fresh_index(&f.transaction.repo).err().unwrap();
        assert!(matches!(
            error.downcast_ref::<BackendFailure>(),
            Some(BackendFailure::Capacity)
        ));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_INDEX_BYTES + 1);
        assert_eq!(
            f.journal.lease(&f.job).unwrap().load().unwrap().phase(),
            Phase::ReadyToStage
        );
    }

    #[tokio::test]
    #[ignore = "subprocess helper invoked by crash recovery test"]
    async fn index_crash_helper() {
        let root = std::env::var_os("DRACON_INDEX_TEST_ROOT").unwrap();
        let f = fixture(Path::new(&root)).await;
        stage(&f).unwrap();
        panic!("crash point was not reached");
    }

    #[tokio::test]
    async fn matched_pair_recovers_before_and_after_atomic_publication() {
        for point in [
            "after-index-intent",
            "before-index-publish",
            "after-index-publish",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "storage_core::index::tests::index_crash_helper",
                    "--ignored",
                    "--exact",
                ])
                .env("DRACON_INDEX_TEST_ROOT", temp.path())
                .env("DRACON_INDEX_CRASH_POINT", point)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(75), "{point}");
            let f = fixture(temp.path()).await;
            let index = fresh_index(&f.transaction.repo).unwrap();
            assert_eq!(
                index.get_path(Path::new("asset.bin"), 0).is_some(),
                point == "after-index-publish"
            );
            assert_eq!(
                index
                    .get_path(Path::new(".dracon/assets.manifest"), 0)
                    .is_some(),
                point == "after-index-publish"
            );
            stage(&f).unwrap();
            assert!(!f.transaction.state.join("intent.json").exists());
            assert!(!f.transaction.repo.path().join("index.lock").exists());
            assert_eq!(
                f.journal.lease(&f.job).unwrap().load().unwrap().phase(),
                Phase::Staged
            );
        }
    }
}
