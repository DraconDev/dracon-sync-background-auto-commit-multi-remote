//! Durable private preparation of protected restore manifests, separate from uploads.
//!
//! Prepared metadata is an encrypted Git-blob candidate, not an object-copy job.
//! This module does not stage, install filters, contact backends or acknowledge Git.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::backend::BackendFailure;
use super::journal::{self, FailureCode, Limits, SnapshotKind};
use super::manifest::{Manifest, MAX_MANIFEST_BYTES};
use super::reference::{validate_sha256, Fingerprint};
use super::security::{require_age_header, WardenAdapter};

/// Encrypted metadata budget, including age headers for authorized recipients.
pub const MAX_PROTECTED_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    PendingCapture,
    Captured,
    Prepared,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    code: FailureCode,
    retry_at: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Spec {
    version: u32,
    repo_id: String,
    source: Fingerprint,
    policy_sha256: String,
}

impl Spec {
    fn id(&self) -> Result<String> {
        if self.version != 1 || self.source.bytes() > MAX_MANIFEST_BYTES as u64 {
            bail!("unsupported protected metadata specification");
        }
        validate_sha256(&self.repo_id)?;
        validate_sha256(&self.policy_sha256)?;
        self.source.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"dracon-protected-manifest-v1\0");
        hash.update(serde_json::to_vec(self)?);
        Ok(format!("{:x}", hash.finalize()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    spec: Spec,
    revision: u64,
    phase: Phase,
    approved: Option<Fingerprint>,
    failure: Option<Failure>,
}

impl Record {
    fn validate(&self) -> Result<()> {
        self.spec.id()?;
        if self.phase == Phase::PendingCapture && self.approved.is_some()
            || self.phase == Phase::Prepared && self.approved.is_none()
        {
            bail!("invalid protected metadata approval state");
        }
        if let Some(identity) = &self.approved {
            identity.validate()?;
            if identity.bytes() > MAX_PROTECTED_MANIFEST_BYTES {
                bail!(BackendFailure::Capacity);
            }
        }
        if let Some(failure) = &self.failure {
            if (failure.code == FailureCode::Transient) != failure.retry_at.is_some() {
                bail!("invalid metadata retry state");
            }
        }
        Ok(())
    }

    fn eligible(&self, now: u64) -> bool {
        self.failure.as_ref().is_none_or(|failure| {
            failure.code == FailureCode::Transient
                && failure.retry_at.is_some_and(|deadline| now >= deadline)
        })
    }

    fn prepared(&self) -> Result<PreparedMetadata> {
        if self.phase != Phase::Prepared || self.failure.is_some() {
            bail!("protected metadata is not ready");
        }
        Ok(PreparedMetadata {
            id: self.spec.id()?,
            repo_id: self.spec.repo_id.clone(),
            payload: self.approved.clone().context("metadata approval missing")?,
        })
    }
}

/// Identity of a retained protected metadata candidate; contains no source digest.
#[derive(Clone)]
pub struct PreparedMetadata {
    id: String,
    repo_id: String,
    payload: Fingerprint,
}

impl PreparedMetadata {
    /// Private local version identifier; do not commit it as restore metadata.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Exact ciphertext identity, suitable for checking the eventual Git blob.
    pub fn payload(&self) -> &Fingerprint {
        &self.payload
    }
}

/// Operator-controlled private metadata namespace; no local versions are evicted.
pub struct MetadataStore {
    directory: PathBuf,
    repo_id: String,
    limits: Limits,
}

impl MetadataStore {
    /// Open a dedicated private root, separate from the asset job journal.
    /// Snapshot/payload limits are additionally capped by the metadata format.
    pub fn open(root: &Path, repo_id: &str, mut limits: Limits) -> Result<Self> {
        validate_sha256(repo_id)?;
        limits.max_snapshot_bytes = limits.max_snapshot_bytes.min(MAX_MANIFEST_BYTES as u64);
        limits.max_payload_bytes = limits.max_payload_bytes.min(MAX_PROTECTED_MANIFEST_BYTES);
        if limits.max_records == 0
            || limits.max_snapshot_bytes == 0
            || limits.max_retained_snapshot_bytes < limits.max_snapshot_bytes
            || limits.max_payload_bytes == 0
            || limits.max_retained_payload_bytes < limits.max_payload_bytes
        {
            bail!("invalid protected metadata limits");
        }
        journal::private_directory(root, true)?;
        journal::runtime::protect(root)?;
        let directory = root.join(repo_id);
        journal::private_directory(&directory, true)?;
        journal::runtime::protect(&directory)?;
        Ok(Self {
            directory,
            repo_id: repo_id.into(),
            limits,
        })
    }

    /// Capture, encrypt and retain a manifest under an approved metadata policy.
    /// Unchanged versions reuse their approved ciphertext, including after crashes.
    /// The caller must approve manifest contents, policy and Warden bindings.
    pub async fn prepare(
        &self,
        manifest: &Manifest,
        policy_sha256: &str,
        adapter: &WardenAdapter,
        now: u64,
    ) -> Result<PreparedMetadata> {
        if manifest.repo_id() != self.repo_id || now == 0 {
            bail!(BackendFailure::Security);
        }
        adapter.require_repo(&self.repo_id)?;
        validate_sha256(policy_sha256)?;
        journal::runtime::protect(&self.directory)?;
        let raw = manifest.encode_private()?;
        let spec = Spec {
            version: 1,
            repo_id: self.repo_id.clone(),
            source: Fingerprint::new(format!("{:x}", Sha256::digest(&raw)), raw.len() as u64)?,
            policy_sha256: policy_sha256.into(),
        };
        let id = spec.id()?;
        let _lease = journal::try_lock(&self.directory.join(format!("{id}.lock")))?;
        let mut record = self.create_or_load(spec)?;
        if !record.eligible(now) {
            bail!("protected metadata requires intervention or retry deadline");
        }
        if record.phase == Phase::PendingCapture {
            if let Err(error) = journal::retain_snapshot(
                &self.directory,
                &id,
                SnapshotKind::Source,
                self.limits,
                &mut raw.as_slice(),
                &record.spec.source,
            ) {
                return self.fail(&mut record, super::transfer::classify(&error), now);
            }
            record.phase = Phase::Captured;
            record.failure = None;
            self.save(&mut record)?;
        }
        drop(raw);
        if record.approved.is_some() {
            if let Err(error) = self.finish(&mut record) {
                return self.fail(&mut record, super::transfer::classify(&error), now);
            }
            return record.prepared();
        }
        let mut source = match self.source(&record) {
            Ok(source) => source,
            Err(error) => return self.fail(&mut record, super::transfer::classify(&error), now),
        };
        source.seek(SeekFrom::Start(0))?;
        let _budget = journal::try_lock(&self.directory.join("payload-budget.lock"))?;
        let spool_path = self.path(&id, "security-output");
        let (retained, previous) = journal::snapshot_bytes(
            &self.directory,
            &spool_path,
            &["payload", "security-output"],
        )?;
        let capacity = self
            .limits
            .max_retained_payload_bytes
            .saturating_sub(
                retained
                    .checked_sub(previous)
                    .context("metadata spool accounting")?,
            )
            .min(self.limits.max_payload_bytes);
        if capacity == 0 {
            return self.fail(&mut record, FailureCode::Capacity, now);
        }
        let mut spool = journal::open_private(&spool_path, true, true)?;
        // Only unapproved transform output may be reset; the source was reverified.
        spool.set_len(0)?;
        let identity = match adapter
            .transform(source, record.spec.source.bytes(), &mut spool, capacity)
            .await
        {
            Ok(identity) => identity,
            Err(error) => return self.fail(&mut record, super::transfer::classify(&error), now),
        };
        if let Err(error) = self.source(&record) {
            return self.fail(&mut record, super::transfer::classify(&error), now);
        }
        spool.sync_all()?;
        journal::verify_snapshot(&spool_path, &identity)?;
        record.approved = Some(identity);
        metadata_crash("before-metadata-approval");
        self.save(&mut record)?;
        metadata_crash("after-metadata-approval");
        self.finish(&mut record)?;
        record.prepared()
    }

    /// Open verified retained ciphertext for the future Git transaction.
    /// This never returns private decoded metadata or marks anything committed.
    pub fn open_prepared(&self, prepared: &PreparedMetadata) -> Result<File> {
        if prepared.repo_id != self.repo_id {
            bail!(BackendFailure::Security);
        }
        let record = self.read(prepared.id())?;
        let actual = record.prepared()?;
        if actual.payload != prepared.payload {
            bail!(BackendFailure::Integrity);
        }
        let mut file =
            journal::verify_snapshot(&self.path(prepared.id(), "payload"), &actual.payload)?;
        require_age_header(&mut file).map_err(|_| BackendFailure::Security)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }

    /// Clear a failed preparation explicitly; this does not establish new evidence.
    pub fn clear_failure(&self, id: &str) -> Result<()> {
        validate_sha256(id)?;
        journal::runtime::protect(&self.directory)?;
        let _lease = journal::try_lock(&self.path(id, "lock"))?;
        let mut record = self.read(id)?;
        record.failure = None;
        self.save(&mut record)
    }

    fn path(&self, id: &str, suffix: &str) -> PathBuf {
        self.directory.join(format!("{id}.{suffix}"))
    }

    fn source(&self, record: &Record) -> Result<File> {
        journal::verify_snapshot(
            &self.path(&record.spec.id()?, "source"),
            &record.spec.source,
        )
    }

    fn create_or_load(&self, spec: Spec) -> Result<Record> {
        let id = spec.id()?;
        let _catalog = journal::try_lock(&self.directory.join("catalog.lock"))?;
        if journal::exists_without_symlink(&self.path(&id, "json"))? {
            let record = self.read(&id)?;
            if record.spec != spec {
                bail!(BackendFailure::Integrity);
            }
            return Ok(record);
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
            bail!(BackendFailure::Capacity);
        }
        let record = Record {
            spec,
            revision: 0,
            phase: Phase::PendingCapture,
            approved: None,
            failure: None,
        };
        self.write(&record, true)?;
        Ok(record)
    }

    fn read(&self, id: &str) -> Result<Record> {
        validate_sha256(id)?;
        let file = journal::open_private(&self.path(id, "json"), false, false)?;
        let mut raw = Vec::new();
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut raw)?;
        if raw.len() as u64 > MAX_RECORD_BYTES {
            bail!("metadata record exceeds limit");
        }
        let record: Record = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid protected metadata record"))?;
        record.validate()?;
        if record.spec.repo_id != self.repo_id || record.spec.id()? != id {
            bail!(BackendFailure::Integrity);
        }
        Ok(record)
    }

    fn write(&self, record: &Record, create: bool) -> Result<()> {
        record.validate()?;
        journal::runtime::protect(&self.directory)?;
        let raw = serde_json::to_vec(record)?;
        if raw.len() as u64 > MAX_RECORD_BYTES {
            bail!("metadata record exceeds limit");
        }
        journal::atomic_bytes(
            &self.directory,
            &self.path(&record.spec.id()?, "json"),
            &raw,
            create,
        )
    }

    fn save(&self, record: &mut Record) -> Result<()> {
        let current = self.read(&record.spec.id()?)?;
        if current.spec != record.spec
            || current.revision != record.revision
            || current.phase > record.phase
            || current
                .approved
                .as_ref()
                .is_some_and(|old| Some(old) != record.approved.as_ref())
        {
            bail!("stale or incompatible metadata update");
        }
        record.revision = record
            .revision
            .checked_add(1)
            .context("metadata revision overflow")?;
        self.write(record, false)
    }

    fn finish(&self, record: &mut Record) -> Result<()> {
        let identity = record
            .approved
            .as_ref()
            .context("metadata approval missing")?;
        let id = record.spec.id()?;
        let destination = self.path(&id, "payload");
        let source = if journal::exists_without_symlink(&destination)? {
            destination.clone()
        } else {
            self.path(&id, "security-output")
        };
        let mut verified = journal::verify_snapshot(&source, identity)?;
        require_age_header(&mut verified).map_err(|_| BackendFailure::Security)?;
        verified.sync_all()?;
        if source != destination {
            std::fs::rename(source, destination)?;
        }
        File::open(&self.directory)?.sync_all()?;
        metadata_crash("after-metadata-publish");
        if record.phase != Phase::Prepared || record.failure.is_some() {
            record.phase = Phase::Prepared;
            record.failure = None;
            self.save(record)?;
        }
        Ok(())
    }

    fn fail(&self, record: &mut Record, code: FailureCode, now: u64) -> Result<PreparedMetadata> {
        record.failure = Some(Failure {
            code,
            retry_at: (code == FailureCode::Transient).then(|| now.saturating_add(30)),
        });
        self.save(record)?;
        bail!("protected metadata preparation failed: {code:?}")
    }
}

#[cfg(not(test))]
fn metadata_crash(_point: &str) {}
#[cfg(test)]
fn metadata_crash(point: &str) {
    if std::env::var("DRACON_METADATA_CRASH_POINT").ok().as_deref() == Some(point) {
        std::process::exit(74);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    const APPROVED: &str = "printf x >> calls\ncat >/dev/null\nprintf 'age-encryption.org/v1\\napproved metadata fixture\\n'";

    fn fixture(
        root: &Path,
        script: &str,
        limits: Limits,
    ) -> (MetadataStore, Manifest, WardenAdapter) {
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let binary = root.join("warden-fixture");
        std::fs::write(&binary, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manifest = Manifest::new("a".repeat(64), vec![]).unwrap();
        let store =
            MetadataStore::open(&root.join("metadata"), manifest.repo_id(), limits).unwrap();
        let adapter =
            WardenAdapter::new(&binary, &repo, manifest.repo_id(), Duration::from_secs(5)).unwrap();
        (store, manifest, adapter)
    }

    fn id(manifest: &Manifest) -> String {
        let raw = manifest.encode_private().unwrap();
        Spec {
            version: 1,
            repo_id: manifest.repo_id().into(),
            source: Fingerprint::new(format!("{:x}", Sha256::digest(&raw)), raw.len() as u64)
                .unwrap(),
            policy_sha256: "b".repeat(64),
        }
        .id()
        .unwrap()
    }

    #[tokio::test]
    async fn unchanged_manifest_reuses_ciphertext_after_reopen_without_record_churn() {
        let temp = tempfile::tempdir().unwrap();
        let (store, manifest, adapter) = fixture(temp.path(), APPROVED, Limits::default());
        let prepared = store
            .prepare(&manifest, &"b".repeat(64), &adapter, 1)
            .await
            .unwrap();
        let before = std::fs::read(store.path(prepared.id(), "json")).unwrap();
        let mut bytes = Vec::new();
        store
            .open_prepared(&prepared)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(bytes.starts_with(b"age-encryption.org/v1\n"));
        let root = temp.path().join("metadata");
        drop(store);
        std::fs::write(temp.path().join("warden-fixture"), "#!/bin/sh\nexit 90\n").unwrap();
        let reopened = MetadataStore::open(&root, manifest.repo_id(), Limits::default()).unwrap();
        let again = reopened
            .prepare(&manifest, &"b".repeat(64), &adapter, 2)
            .await
            .unwrap();
        assert_eq!(again.payload(), prepared.payload());
        assert_eq!(
            std::fs::read(reopened.path(again.id(), "json")).unwrap(),
            before
        );
        assert_eq!(std::fs::read(temp.path().join("repo/calls")).unwrap(), b"x");
    }

    #[tokio::test]
    async fn bad_exit_plain_output_and_budget_failure_keep_source_unapproved() {
        for (script, max_payload, expected) in [
            ("cat", 1024, FailureCode::Security),
            (
                "printf 'private-diagnostic' >&2\nprintf 'age-encryption.org/v1\\n'\nexit 9",
                1024,
                FailureCode::Security,
            ),
            (APPROVED, 23, FailureCode::Capacity),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (store, manifest, adapter) = fixture(
                temp.path(),
                script,
                Limits {
                    max_payload_bytes: max_payload,
                    ..Limits::default()
                },
            );
            let error = store
                .prepare(&manifest, &"b".repeat(64), &adapter, 1)
                .await
                .err()
                .unwrap();
            assert!(!format!("{error:#}").contains("private-diagnostic"));
            let record = store.read(&id(&manifest)).unwrap();
            assert!(record.phase == Phase::Captured);
            assert_eq!(record.failure.as_ref().unwrap().code, expected);
            assert!(record.approved.is_none());
            assert!(store.source(&record).is_ok());
            assert!(!store.path(&id(&manifest), "payload").exists());
        }
    }

    #[tokio::test]
    async fn foreign_binding_and_held_version_lease_refuse_before_capture() {
        let temp = tempfile::tempdir().unwrap();
        let (store, manifest, _) = fixture(temp.path(), APPROVED, Limits::default());
        let foreign = WardenAdapter::new(
            &temp.path().join("warden-fixture"),
            &temp.path().join("repo"),
            &"c".repeat(64),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &foreign, 1)
            .await
            .is_err());
        let version = id(&manifest);
        assert!(!store.path(&version, "json").exists());
        assert!(!temp.path().join("repo/calls").exists());
        let owning = WardenAdapter::new(
            &temp.path().join("warden-fixture"),
            &temp.path().join("repo"),
            manifest.repo_id(),
            Duration::from_secs(5),
        )
        .unwrap();
        let _held = journal::try_lock(&store.path(&version, "lock")).unwrap();
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &owning, 1)
            .await
            .is_err());
        assert!(!store.path(&version, "json").exists());
    }

    #[tokio::test]
    async fn corrupted_approved_ciphertext_is_permanent_and_never_reencrypted() {
        let temp = tempfile::tempdir().unwrap();
        let (store, manifest, adapter) = fixture(temp.path(), APPROVED, Limits::default());
        let prepared = store
            .prepare(&manifest, &"b".repeat(64), &adapter, 1)
            .await
            .unwrap();
        std::fs::write(
            store.path(prepared.id(), "payload"),
            b"corrupt approved bytes",
        )
        .unwrap();
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &adapter, 2)
            .await
            .is_err());
        let record = store.read(prepared.id()).unwrap();
        assert_eq!(record.approved.as_ref(), Some(prepared.payload()));
        assert_eq!(
            record.failure.as_ref().unwrap().code,
            FailureCode::Integrity
        );
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &adapter, 100)
            .await
            .is_err());
        assert!(store.open_prepared(&prepared).is_err());
        assert_eq!(std::fs::read(temp.path().join("repo/calls")).unwrap(), b"x");
        assert!(store.source(&record).is_ok());
    }

    #[tokio::test]
    async fn source_record_and_aggregate_limits_never_evict_previous_versions() {
        for limits in [
            Limits {
                max_records: 1,
                ..Limits::default()
            },
            Limits {
                max_snapshot_bytes: 1,
                max_retained_snapshot_bytes: 1,
                ..Limits::default()
            },
            Limits {
                max_payload_bytes: 64,
                max_retained_payload_bytes: 64,
                ..Limits::default()
            },
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (store, manifest, adapter) = fixture(temp.path(), APPROVED, limits);
            let first = store.prepare(&manifest, &"b".repeat(64), &adapter, 1).await;
            if limits.max_snapshot_bytes == 1 {
                assert!(first.is_err());
                assert_eq!(
                    store
                        .read(&id(&manifest))
                        .unwrap()
                        .failure
                        .as_ref()
                        .unwrap()
                        .code,
                    FailureCode::Capacity
                );
            } else {
                let first = first.unwrap();
                assert!(store
                    .prepare(&manifest, &"c".repeat(64), &adapter, 2)
                    .await
                    .is_err());
                assert!(store.open_prepared(&first).is_ok());
            }
        }
    }

    #[tokio::test]
    async fn approval_publication_crashes_preserve_exact_manifest_ciphertext() {
        for point in [
            "before-metadata-approval",
            "after-metadata-approval",
            "after-metadata-publish",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (store, manifest, adapter) = fixture(temp.path(), APPROVED, Limits::default());
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage_core::metadata::tests::metadata_crash_child",
                    "--ignored",
                ])
                .env("DRACON_METADATA_CRASH_POINT", point)
                .env("DRACON_METADATA_FIXTURE", temp.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(74));
            if point != "before-metadata-approval" {
                std::fs::write(temp.path().join("warden-fixture"), "#!/bin/sh\nexit 91\n").unwrap();
            }
            let saved = store.read(&id(&manifest)).unwrap().approved;
            let prepared = store
                .prepare(&manifest, &"b".repeat(64), &adapter, 2)
                .await
                .unwrap();
            if let Some(saved) = saved {
                assert_eq!(prepared.payload(), &saved);
            }
            assert!(store.open_prepared(&prepared).is_ok());
            assert_eq!(
                std::fs::read(temp.path().join("repo/calls")).unwrap().len(),
                if point == "before-metadata-approval" {
                    2
                } else {
                    1
                }
            );
        }
    }

    #[tokio::test]
    async fn timeout_respects_retry_deadline_and_does_not_discard_captured_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let (store, manifest, _) = fixture(temp.path(), "exec sleep 10", Limits::default());
        let adapter = WardenAdapter::new(
            &temp.path().join("warden-fixture"),
            &temp.path().join("repo"),
            manifest.repo_id(),
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &adapter, 1)
            .await
            .is_err());
        let record = store.read(&id(&manifest)).unwrap();
        assert_eq!(
            record.failure.as_ref().unwrap().code,
            FailureCode::Transient
        );
        assert_eq!(record.failure.as_ref().unwrap().retry_at, Some(31));
        assert!(store.source(&record).is_ok());
        std::fs::write(
            temp.path().join("warden-fixture"),
            format!("#!/bin/sh\n{APPROVED}\n"),
        )
        .unwrap();
        let adapter = WardenAdapter::new(
            &temp.path().join("warden-fixture"),
            &temp.path().join("repo"),
            manifest.repo_id(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &adapter, 30)
            .await
            .is_err());
        assert!(!temp.path().join("repo/calls").exists());
        assert!(store
            .prepare(&manifest, &"b".repeat(64), &adapter, 31)
            .await
            .is_ok());
    }

    #[tokio::test]
    #[ignore = "operational check: requires age-keygen and DRACON_STORAGE_TEST_WARDEN source-build binary"]
    async fn real_warden_manifest_restores_without_original_metadata_store() {
        use crate::storage_core::journal::{encode_relative_path, Encryption};
        use crate::storage_core::manifest::Enrollment;
        use std::process::{Command, Stdio};
        let binary = PathBuf::from(
            std::env::var_os("DRACON_STORAGE_TEST_WARDEN")
                .expect("source-built Warden binary required"),
        );
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(home.join(".dracon/keys")).unwrap();
        let keys = home.join(".dracon/keys/identity.age");
        assert!(Command::new("age-keygen")
            .arg("-o")
            .arg(&keys)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600)).unwrap();
        let manifest = Manifest::new(
            "a".repeat(64),
            vec![Enrollment {
                path_hex: encode_relative_path(b"assets/private-\xff title.mp4").unwrap(),
                contract_sha256: "b".repeat(64),
                primary: "primary".into(),
                required_copies: vec!["primary".into(), "recovery".into()],
                encryption: Encryption::WardenAge,
                payload: Some(Fingerprint::new("c".repeat(64), 101 * 1024 * 1024).unwrap()),
            }],
        )
        .unwrap();
        let store_root = temp.path().join("metadata");
        let store =
            MetadataStore::open(&store_root, manifest.repo_id(), Limits::default()).unwrap();
        let adapter =
            WardenAdapter::new(&binary, &repo, manifest.repo_id(), Duration::from_secs(30))
                .unwrap()
                .with_identity_home(&home)
                .unwrap();
        let prepared = store
            .prepare(&manifest, &"d".repeat(64), &adapter, 1)
            .await
            .unwrap();
        let cipher_path = temp.path().join("saved-manifest.age");
        let mut saved = journal::open_private(&cipher_path, true, true).unwrap();
        std::io::copy(&mut store.open_prepared(&prepared).unwrap(), &mut saved).unwrap();
        saved.sync_all().unwrap();
        let raw = manifest.encode_private().unwrap();
        let cipher = std::fs::read(&cipher_path).unwrap();
        assert!(!cipher.windows(raw.len()).any(|bytes| bytes == raw));
        drop(store);
        // Only this test-owned temporary store is removed. Recovery gets Git's
        // ciphertext candidate and separately retained keys, not local records.
        std::fs::remove_dir_all(&store_root).unwrap();
        let plain_path = temp.path().join("recovered-private-metadata");
        let output = journal::open_private(&plain_path, true, true).unwrap();
        assert!(Command::new(&binary)
            .args(["storage-decrypt", "--repo"])
            .arg(&repo)
            .arg("--max-bytes")
            .arg(MAX_MANIFEST_BYTES.to_string())
            .env("HOME", &home)
            .env_remove("ARCANE_MACHINE_KEY")
            .stdin(Stdio::from(File::open(&cipher_path).unwrap()))
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        assert!(Manifest::parse_private(&std::fs::read(&plain_path).unwrap()).unwrap() == manifest);
        assert!(!store_root.exists());
        assert!(!repo.join(".gitattributes").exists());
        assert!(!repo.join(".arcane").exists());
    }

    #[tokio::test]
    #[ignore = "subprocess helper, invoked by metadata crash test"]
    async fn metadata_crash_child() {
        let root = PathBuf::from(std::env::var_os("DRACON_METADATA_FIXTURE").unwrap());
        let store = MetadataStore::open(&root.join("metadata"), &"a".repeat(64), Limits::default())
            .unwrap();
        let manifest = Manifest::new("a".repeat(64), vec![]).unwrap();
        let adapter = WardenAdapter::new(
            &root.join("warden-fixture"),
            &root.join("repo"),
            manifest.repo_id(),
            Duration::from_secs(5),
        )
        .unwrap();
        store
            .prepare(&manifest, &"b".repeat(64), &adapter, 1)
            .await
            .unwrap();
        panic!("metadata crash injection did not fire");
    }
}
