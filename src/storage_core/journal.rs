//! Versioned exact-source job records with atomic writes and process-backed leases.
//!
//! Records contain private source fingerprints and paths. They belong in an
//! operator-controlled private directory, never in Git or a public asset manifest.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::reference::{validate_sha256, Fingerprint, Pointer};

const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Representation expected for a prepared payload.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Encryption {
    /// Operator-classified non-sensitive bytes; still subject to security checks.
    None,
    /// Whole-payload age encryption through the approved Warden security adapter.
    WardenAge,
}

/// Durable progress; a failed attempt retains the last proven phase.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Exact version discovered; a stable source snapshot is not yet captured.
    PendingCapture,
    /// Source snapshot matches the selected source fingerprint.
    Captured,
    /// Approved representation and immutable payload identity are prepared.
    Prepared,
    /// An upload attempt is in progress or awaits readback after interruption.
    Uploading,
    /// Primary payload copy was verified; other required copies may be pending.
    PrimaryVerified,
    /// All required copies were verified for the same payload.
    ReadyToStage,
    /// Matching pointer and metadata were recorded in the Git index.
    Staged,
    /// A Git commit contains the prepared version's references.
    Committed,
    /// Required payload copies and configured Git destinations were acknowledged.
    Preserved,
    /// Unpublished work was cancelled; preserved local data is not deleted.
    Cancelled,
}

/// Typed failures avoid recording credential-bearing subprocess/server messages.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureCode {
    /// Transient backend/network failure.
    Transient,
    /// Captured/source bytes no longer match the selected version.
    SourceChanged,
    /// Credentials need intervention.
    Credentials,
    /// Storage capacity/quota needs intervention.
    Capacity,
    /// Object is missing or corrupt.
    Integrity,
    /// Security requirements or authorization could not be satisfied.
    Security,
    /// A manual index change conflicts with automatic staging.
    IndexConflict,
}

impl FailureCode {
    fn retryable(self) -> bool {
        matches!(self, Self::Transient | Self::IndexConflict)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    code: FailureCode,
    retry_at: Option<u64>,
}

/// Immutable job inputs; modifying a version or policy creates a different job.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSpec {
    /// Stable operator-bound repository identifier, encoded as 64 lowercase hex digits.
    pub repo_id: String,
    /// Lossless relative path bytes as lowercase hex, confined to the owning repo.
    pub path_hex: String,
    /// Private fingerprint of the selected plaintext source version.
    pub source: Fingerprint,
    /// Fingerprint of the effective approved placement/security policy.
    pub policy_sha256: String,
    /// Operator-defined primary backend identifier, never an endpoint/credential.
    pub primary: String,
    /// All required payload backend identifiers, including the primary.
    pub required_copies: Vec<String>,
    /// All required configured Git remote identifiers.
    pub required_git_targets: Vec<String>,
    /// Security representation required by the approved policy.
    pub encryption: Encryption,
}

fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid configured destination identifier");
    }
    Ok(())
}

/// Encode relative path bytes for private durable state, rejecting unsafe paths.
pub fn encode_relative_path(bytes: &[u8]) -> Result<String> {
    if bytes.is_empty()
        || bytes.len() > 4096
        || bytes.contains(&0)
        || bytes.contains(&b'\\')
        || bytes.contains(&b':')
    {
        bail!("unsupported relative payload path");
    }
    for component in bytes.split(|b| *b == b'/') {
        if component.is_empty()
            || component == b"."
            || component == b".."
            || component.eq_ignore_ascii_case(b".git")
        {
            bail!("payload path must be relative and exclude Git internals");
        }
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn validate_path_hex(hex: &str) -> Result<()> {
    if hex.len() > 8192 || hex.len() % 2 != 0 {
        bail!("invalid encoded path");
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        if !pair
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            bail!("invalid encoded path");
        }
        bytes.push(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?);
    }
    if encode_relative_path(&bytes)? != hex {
        bail!("noncanonical encoded path");
    }
    Ok(())
}

impl JobSpec {
    /// Check path, identities, destination bindings, and mandatory copy sets.
    pub fn validate(&self) -> Result<()> {
        validate_sha256(&self.repo_id)?;
        validate_sha256(&self.policy_sha256)?;
        validate_path_hex(&self.path_hex)?;
        self.source.validate()?;
        identifier(&self.primary)?;
        for set in [&self.required_copies, &self.required_git_targets] {
            if set.is_empty() || set.len() > 64 {
                bail!("required destination set is empty or exceeds limit");
            }
            let mut seen = BTreeSet::new();
            for id in set {
                identifier(id)?;
                if !seen.insert(id) {
                    bail!("duplicate required destination");
                }
            }
        }
        if !self.required_copies.contains(&self.primary) {
            bail!("primary must be a required copy");
        }
        Ok(())
    }

    fn id(&self) -> Result<String> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"dracon-storage-job-v1\0");
        hash.update(serde_json::to_vec(self)?);
        Ok(format!("{:x}", hash.finalize()))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    payload: Fingerprint,
    verified_at: u64,
}

/// A durable exact-version job. Debug output deliberately excludes private inputs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    schema_version: u32,
    id: String,
    revision: u64,
    spec: JobSpec,
    phase: Phase,
    capture: Option<Fingerprint>,
    payload: Option<Fingerprint>,
    copies: BTreeMap<String, Receipt>,
    commit: Option<String>,
    git_receipts: BTreeMap<String, String>,
    attempts: u32,
    failure: Option<Failure>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("phase", &self.phase)
            .field("revision", &self.revision)
            .field("attempts", &self.attempts)
            .finish_non_exhaustive()
    }
}

fn git_oid(value: &str) -> Result<()> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid Git commit identity");
    }
    Ok(())
}

impl Job {
    /// Create a pending job. Persist through a journal before performing work.
    pub fn new(spec: JobSpec) -> Result<Self> {
        let id = spec.id()?;
        Ok(Self {
            schema_version: VERSION,
            id,
            revision: 1,
            spec,
            phase: Phase::PendingCapture,
            capture: None,
            payload: None,
            copies: BTreeMap::new(),
            commit: None,
            git_receipts: BTreeMap::new(),
            attempts: 0,
            failure: None,
        })
    }

    /// Local job identity; do not expose it as a public plaintext-content identity.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Immutable source/policy inputs; sensitive and local-only.
    pub fn spec(&self) -> &JobSpec {
        &self.spec
    }
    /// Last proven progress phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }
    /// Prepared immutable representation, when available.
    pub fn payload(&self) -> Option<&Fingerprint> {
        self.payload.as_ref()
    }
    /// Typed failure, without raw secrets/path/server details.
    pub fn failure(&self) -> Option<FailureCode> {
        self.failure.as_ref().map(|f| f.code)
    }

    fn require_phase(&self, phases: &[Phase]) -> Result<()> {
        if !phases.contains(&self.phase) {
            bail!("operation is invalid in current job phase");
        }
        Ok(())
    }

    /// Record a verified stable capture of exactly the selected source version.
    pub fn record_capture(&mut self, snapshot: Fingerprint) -> Result<()> {
        self.require_phase(&[Phase::PendingCapture])?;
        if snapshot != self.spec.source {
            bail!("capture does not match selected source version");
        }
        self.capture = Some(snapshot);
        self.phase = Phase::Captured;
        self.failure = None;
        Ok(())
    }

    /// Record an approved representation after security processing succeeds.
    pub fn record_prepared(&mut self, payload: Fingerprint) -> Result<()> {
        self.require_phase(&[Phase::Captured])?;
        self.validate_payload(&payload)?;
        self.payload = Some(payload);
        self.phase = Phase::Prepared;
        self.failure = None;
        Ok(())
    }

    fn validate_payload(&self, payload: &Fingerprint) -> Result<()> {
        payload.validate()?;
        match self.spec.encryption {
            Encryption::None if payload != &self.spec.source => {
                bail!("non-sensitive representation changed source bytes")
            }
            Encryption::WardenAge if payload.sha256() == self.spec.source.sha256() => {
                bail!("encrypted representation cannot equal plaintext source")
            }
            _ => Ok(()),
        }
    }

    /// Start/resume upload; permanent failures need explicit intervention first.
    pub fn begin_upload(&mut self, now: u64) -> Result<()> {
        self.require_phase(&[Phase::Prepared, Phase::Uploading, Phase::PrimaryVerified])?;
        if let Some(failure) = &self.failure {
            if !failure.code.retryable() || failure.retry_at.is_some_and(|at| now < at) {
                bail!("job is not eligible for automatic retry");
            }
        }
        self.attempts = self
            .attempts
            .checked_add(1)
            .context("upload attempt counter overflow")?;
        // Retain previously verified copies and primary phase across retries.
        if self.phase == Phase::Prepared {
            self.phase = Phase::Uploading;
        }
        self.failure = None;
        Ok(())
    }

    /// Record a backend readback receipt for the exact prepared payload.
    ///
    /// The adapter must actually verify bytes/access before invoking this method.
    pub fn record_copy(
        &mut self,
        backend: &str,
        payload: Fingerprint,
        verified_at: u64,
    ) -> Result<()> {
        self.require_phase(&[
            Phase::Uploading,
            Phase::PrimaryVerified,
            Phase::ReadyToStage,
        ])?;
        if !self.spec.required_copies.iter().any(|id| id == backend)
            || self.payload.as_ref() != Some(&payload)
            || verified_at == 0
        {
            bail!("receipt does not match a required destination and prepared version");
        }
        self.copies.insert(
            backend.to_owned(),
            Receipt {
                payload,
                verified_at,
            },
        );
        if self.copies.contains_key(&self.spec.primary) {
            self.phase = if self
                .spec
                .required_copies
                .iter()
                .all(|id| self.copies.contains_key(id))
            {
                Phase::ReadyToStage
            } else {
                Phase::PrimaryVerified
            };
        }
        Ok(())
    }

    /// Acknowledge matching pointer/manifest staging, after actual index verification.
    pub fn record_staged(&mut self, pointer: &Pointer) -> Result<()> {
        self.require_phase(&[Phase::ReadyToStage])?;
        if self.failure.is_some() || self.payload.as_ref() != Some(pointer.payload()) {
            bail!("staged pointer is not the verified prepared version");
        }
        self.phase = Phase::Staged;
        Ok(())
    }

    /// Acknowledge an inspected Git commit containing this version's references.
    pub fn record_commit(&mut self, commit: String) -> Result<()> {
        self.require_phase(&[Phase::Staged])?;
        git_oid(&commit)?;
        if self.failure.is_some() {
            bail!("job has an unresolved failure");
        }
        self.commit = Some(commit);
        self.phase = Phase::Committed;
        Ok(())
    }

    /// Acknowledge a configured Git destination's exact committed version.
    pub fn record_git_push(&mut self, target: &str, commit: &str) -> Result<()> {
        self.require_phase(&[Phase::Committed, Phase::Preserved])?;
        if !self.spec.required_git_targets.iter().any(|id| id == target)
            || self.commit.as_deref() != Some(commit)
        {
            bail!("Git receipt does not match required destination and commit");
        }
        self.git_receipts
            .insert(target.to_owned(), commit.to_owned());
        if self
            .spec
            .required_git_targets
            .iter()
            .all(|id| self.git_receipts.contains_key(id))
        {
            self.phase = Phase::Preserved;
        }
        Ok(())
    }

    /// Retain phase/snapshots/receipts while recording a typed failure and backoff.
    pub fn note_failure(&mut self, code: FailureCode, retry_at: Option<u64>) -> Result<()> {
        if self.phase == Phase::Cancelled || (!code.retryable() && retry_at.is_some()) {
            bail!("invalid failure/backoff");
        }
        self.failure = Some(Failure { code, retry_at });
        Ok(())
    }

    /// Explicitly acknowledge intervention; does not manufacture new evidence.
    pub fn clear_failure(&mut self) {
        self.failure = None;
    }

    /// Cancel only unpublished work, retaining its state and local artifacts.
    pub fn cancel(&mut self) -> Result<()> {
        if matches!(self.phase, Phase::Committed | Phase::Preserved) {
            bail!("published job cannot be cancelled");
        }
        self.phase = Phase::Cancelled;
        Ok(())
    }

    /// Validate persisted invariants before resuming or reporting a job.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != VERSION || self.revision == 0 || self.spec.id()? != self.id {
            bail!("unsupported or invalid journal record");
        }
        if let Some(capture) = &self.capture {
            if capture != &self.spec.source {
                bail!("captured source mismatch");
            }
        }
        if self.phase != Phase::Cancelled && self.phase < Phase::Captured && self.capture.is_some()
        {
            bail!("capture evidence in an uncaptured phase");
        }
        if self.phase != Phase::Cancelled && self.phase < Phase::Prepared && self.payload.is_some()
        {
            bail!("payload in an unprepared phase");
        }
        if self.phase != Phase::Cancelled
            && self.phase < Phase::Uploading
            && !self.copies.is_empty()
        {
            bail!("copy receipt before upload phase");
        }
        if self.phase != Phase::Cancelled && self.phase >= Phase::Captured && self.capture.is_none()
        {
            bail!("capture evidence missing");
        }
        if let Some(payload) = &self.payload {
            self.validate_payload(payload)?;
        }
        if self.phase != Phase::Cancelled && self.phase >= Phase::Prepared && self.payload.is_none()
        {
            bail!("prepared payload missing");
        }
        for (backend, receipt) in &self.copies {
            if !self.spec.required_copies.contains(backend)
                || self.payload.as_ref() != Some(&receipt.payload)
                || receipt.verified_at == 0
            {
                bail!("invalid copy receipt");
            }
        }
        if self.phase != Phase::Cancelled
            && self.phase >= Phase::PrimaryVerified
            && !self.copies.contains_key(&self.spec.primary)
        {
            bail!("primary verification missing");
        }
        if self.phase != Phase::Cancelled
            && self.phase >= Phase::ReadyToStage
            && !self
                .spec
                .required_copies
                .iter()
                .all(|id| self.copies.contains_key(id))
        {
            bail!("required copy verification missing");
        }
        if let Some(commit) = &self.commit {
            git_oid(commit)?;
        }
        if self.phase != Phase::Cancelled && self.phase >= Phase::Committed && self.commit.is_none()
        {
            bail!("commit evidence missing");
        }
        if self.commit.is_some() && !matches!(self.phase, Phase::Committed | Phase::Preserved) {
            bail!("commit in an unpublished phase");
        }
        for (target, commit) in &self.git_receipts {
            if !self.spec.required_git_targets.contains(target)
                || self.commit.as_ref() != Some(commit)
            {
                bail!("invalid Git receipt");
            }
        }
        if self.phase == Phase::Preserved
            && !self
                .spec
                .required_git_targets
                .iter()
                .all(|id| self.git_receipts.contains_key(id))
        {
            bail!("required Git verification missing");
        }
        if let Some(failure) = &self.failure {
            if !failure.code.retryable() && failure.retry_at.is_some() {
                bail!("invalid permanent-failure backoff");
            }
        }
        Ok(())
    }
}

/// Per-repository limits on retained job records; no cleanup is implied.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum records in a single repository namespace.
    pub max_records: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 10_000,
        }
    }
}

/// Private per-repository journal. Transfers never need a global Git index lock.
pub struct Journal {
    directory: PathBuf,
    repo_id: String,
    limits: Limits,
}

/// Exclusive job lease, automatically released by the OS on process death.
pub struct JobLease {
    directory: PathBuf,
    repo_id: String,
    id: String,
    _lock: File,
}

pub(super) fn private_directory(path: &Path, create: bool) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("journal root must be absolute and normalized");
    }
    let mut current = PathBuf::new();
    let mut missing = Vec::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("journal directories must not follow symlinks")
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.clone());
            }
            Err(e) => return Err(e.into()),
        }
    }
    if create {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        // Persist newly created namespace entries as well as later record renames.
        for directory in missing.iter().rev() {
            File::open(directory)?.sync_all()?;
            if let Some(parent) = directory.parent() {
                File::open(parent)?.sync_all()?;
            }
        }
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("journal root is not a real directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
            bail!("journal directory must be owned by the operator with mode 0700");
        }
    }
    #[cfg(not(unix))]
    bail!("journal requires a supported private-directory permission adapter");
    Ok(())
}

fn exists_without_symlink(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("journal path must not be a symlink")
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn open_private(path: &Path, write: bool, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(write)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("journal entry must be a regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
        {
            bail!("journal entry must be private, operator-owned, and not hard-linked");
        }
    }
    Ok(file)
}

fn try_lock(path: &Path) -> Result<File> {
    let file = open_private(path, true, true)?;
    file.try_lock()
        .map_err(|_| anyhow::anyhow!("journal lease is busy or unavailable"))?;
    Ok(file)
}

fn read_job(path: &Path) -> Result<Job> {
    let file = open_private(path, false, false)?;
    if file.metadata()?.len() > MAX_RECORD_BYTES {
        bail!("journal record exceeds limit");
    }
    let mut raw = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_RECORD_BYTES {
        bail!("journal record exceeds limit");
    }
    // Do not include raw JSON/path/hash contents in parser diagnostics.
    let job: Job = serde_json::from_slice(&raw)
        .map_err(|_| anyhow::anyhow!("invalid journal record encoding"))?;
    job.validate()?;
    Ok(job)
}

fn atomic_write(directory: &Path, path: &Path, job: &Job, create_only: bool) -> Result<()> {
    job.validate()?;
    let raw = serde_json::to_vec(job)?;
    if raw.len() as u64 > MAX_RECORD_BYTES {
        bail!("journal record exceeds limit");
    }
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = directory.join(format!(
        ".record-{}-{nanos}-{sequence}.tmp",
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(&raw)?;
        file.sync_all()?;
        crash_point("before-publish");
        // The caller holds the per-job lease, so no cooperating writer can
        // create/replace this record between the existence check and rename.
        if create_only {
            match std::fs::symlink_metadata(path) {
                Ok(_) => bail!("job already exists"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        std::fs::rename(&temporary, path)?;
        crash_point("after-publish");
        File::open(directory)?.sync_all()?;
        Ok(())
    })();
    // Only temporary record spools are removed; source snapshots/objects never are.
    let _ = std::fs::remove_file(&temporary);
    result
}

#[cfg(not(test))]
fn crash_point(_phase: &str) {}
#[cfg(test)]
fn crash_point(phase: &str) {
    if std::env::var("DRACON_STORAGE_CRASH_POINT").ok().as_deref() == Some(phase) {
        std::process::exit(73);
    }
}

impl Journal {
    /// Create/open a private namespace. No existing records or payloads are deleted.
    pub fn open(root: &Path, repo_id: &str, limits: Limits) -> Result<Self> {
        validate_sha256(repo_id)?;
        if limits.max_records == 0 {
            bail!("journal record limit must be positive");
        }
        private_directory(root, true)?;
        let directory = root.join(repo_id);
        private_directory(&directory, true)?;
        Ok(Self {
            directory,
            repo_id: repo_id.into(),
            limits,
        })
    }

    /// Obtain a nonblocking exclusive lease for one exact-source job.
    pub fn lease(&self, id: &str) -> Result<JobLease> {
        validate_sha256(id)?;
        let lock = try_lock(&self.directory.join(format!("{id}.lock")))?;
        Ok(JobLease {
            directory: self.directory.clone(),
            repo_id: self.repo_id.clone(),
            id: id.into(),
            _lock: lock,
        })
    }

    /// Durably create a pending job, or return the identical already-recorded version.
    pub fn create(&self, spec: JobSpec) -> Result<Job> {
        if spec.repo_id != self.repo_id {
            bail!("job belongs to another repository");
        }
        let job = Job::new(spec)?;
        let lease = self.lease(job.id())?;
        let catalog = try_lock(&self.directory.join("catalog.lock"))?;
        let path = self.directory.join(format!("{}.json", job.id()));
        match lease.load() {
            Ok(existing) => return Ok(existing),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
        let mut count = 0;
        for entry in std::fs::read_dir(&self.directory)? {
            if entry?
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                count += 1;
            }
        }
        if count >= self.limits.max_records {
            bail!("journal record budget exhausted; retained data was not deleted");
        }
        atomic_write(&self.directory, &path, &job, true)?;
        drop(catalog);
        Ok(job)
    }

    /// Read a redacted summary without opening a write lease or creating directories.
    pub fn inspect(root: &Path, repo_id: &str) -> Result<Summary> {
        validate_sha256(repo_id)?;
        if !exists_without_symlink(root)? {
            return Ok(Summary::default());
        }
        private_directory(root, false)?;
        let directory = root.join(repo_id);
        if !exists_without_symlink(&directory)? {
            return Ok(Summary::default());
        }
        private_directory(&directory, false)?;
        let mut summary = Summary {
            initialized: true,
            ..Summary::default()
        };
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "json")
            {
                continue;
            }
            let expected = entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned);
            let loaded = read_job(&entry.path());
            match loaded {
                Ok(job) if expected.as_deref() == Some(job.id()) && job.spec.repo_id == repo_id => {
                    summary.records += 1;
                    *summary.phases.entry(job.phase).or_default() += 1;
                    if job.failure.is_some() {
                        summary.failed_records += 1;
                    }
                    if matches!(job.phase, Phase::Uploading | Phase::PrimaryVerified) {
                        summary.awaiting_upload_verification += 1;
                    }
                    if job.phase != Phase::Preserved && job.phase != Phase::Cancelled {
                        summary.pending_source_bytes = summary
                            .pending_source_bytes
                            .saturating_add(job.spec.source.bytes());
                    }
                    if job.phase == Phase::Preserved && job.failure.is_none() {
                        summary.recorded_preserved += 1;
                    }
                }
                _ => summary.invalid_records += 1,
            }
        }
        Ok(summary)
    }
}

impl JobLease {
    /// Read and validate the leased exact-source version; corrupt records are preserved.
    pub fn load(&self) -> Result<Job> {
        let job = read_job(&self.directory.join(format!("{}.json", self.id)))?;
        if job.id != self.id || job.spec.repo_id != self.repo_id {
            bail!("journal record identity mismatch");
        }
        Ok(job)
    }

    /// Atomically save the next revision, rejecting stale or retargeted records.
    pub fn save(&self, job: &mut Job) -> Result<()> {
        let current = self.load()?;
        job.validate()?;
        if job.id != self.id || job.spec.repo_id != self.repo_id || job.revision != current.revision
        {
            bail!("stale or retargeted journal update");
        }
        validate_update(&current, job)?;
        let previous = job.revision;
        job.revision = previous
            .checked_add(1)
            .context("journal revision overflow")?;
        let path = self.directory.join(format!("{}.json", self.id));
        let result = atomic_write(&self.directory, &path, job, false);
        if result.is_err() {
            job.revision = previous;
        }
        result
    }
}

fn validate_update(current: &Job, next: &Job) -> Result<()> {
    let permitted = current.phase == next.phase
        || (next.phase == Phase::Cancelled && current.phase < Phase::Committed)
        || matches!(
            (current.phase, next.phase),
            (Phase::PendingCapture, Phase::Captured)
                | (Phase::Captured, Phase::Prepared)
                | (Phase::Prepared, Phase::Uploading)
                | (Phase::Uploading, Phase::PrimaryVerified)
                | (Phase::Uploading, Phase::ReadyToStage)
                | (Phase::PrimaryVerified, Phase::ReadyToStage)
                | (Phase::ReadyToStage, Phase::Staged)
                | (Phase::Staged, Phase::Committed)
                | (Phase::Committed, Phase::Preserved)
        );
    if !permitted {
        bail!("journal phase transition would skip or discard proven progress");
    }
    if current.capture.is_some() && current.capture != next.capture
        || current.payload.is_some() && current.payload != next.payload
        || current.commit.is_some() && current.commit != next.commit
        || next.attempts < current.attempts
    {
        bail!("journal update changed immutable evidence");
    }
    for (backend, receipt) in &current.copies {
        if !next.copies.get(backend).is_some_and(|next| {
            next.payload == receipt.payload && next.verified_at >= receipt.verified_at
        }) {
            bail!("journal update discarded copy evidence");
        }
    }
    for (target, commit) in &current.git_receipts {
        if next.git_receipts.get(target) != Some(commit) {
            bail!("journal update discarded Git evidence");
        }
    }
    Ok(())
}

/// Redacted local evidence summary; it does not claim backend objects are healthy now.
#[derive(Debug, Default, Serialize)]
pub struct Summary {
    /// Whether the repository journal namespace exists.
    pub initialized: bool,
    /// Valid records inspected.
    pub records: u64,
    /// Recorded progress counts; no source paths/fingerprints are exposed.
    pub phases: BTreeMap<Phase, u64>,
    /// Jobs with a typed unresolved failure.
    pub failed_records: u64,
    /// Records that failed schema, permission, encoding, or invariant validation.
    pub invalid_records: u64,
    /// Uploads that must be resumed/readback-verified; may include active workers.
    pub awaiting_upload_verification: u64,
    /// Source bytes not yet recorded as preserved or cancelled.
    pub pending_source_bytes: u64,
    /// Completed historical acknowledgments, not a live backend restore check.
    pub recorded_preserved: u64,
}

#[cfg(test)]
mod tests;
