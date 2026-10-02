//! Exact-version recovery into private immutable files, without checkout mutation.

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::backend::BackendFailure;
use super::bindings::RestoreBinding;
use super::journal::{self, Encryption, Limits, SnapshotKind};
use super::manifest::Manifest;
use super::metadata::{MetadataStore, PreparedMetadata};
use super::reference::{validate_sha256, Fingerprint};
use super::security::WardenAdapter;

/// Private exact-version recovery receipt; no plaintext digest is exposed.
pub struct RestoredAsset {
    pub(super) path: PathBuf,
    pub(super) source: Fingerprint,
    pub(super) repo_id: String,
    pub(super) path_hex: String,
    pub(super) payload: Fingerprint,
}

impl RestoredAsset {
    /// A private ordinary file, never an automatically rewritten checkout path.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Actual verified/decrypted output length.
    pub fn bytes(&self) -> u64 {
        self.source.bytes()
    }
}

/// Dedicated private recovery namespace. Versions are never overwritten/evicted.
pub struct RestoreStore {
    directory: PathBuf,
    repo_id: String,
    limits: Limits,
}

impl RestoreStore {
    /// Open an operator-selected private root with bounded payload/output retention.
    pub fn open(root: &Path, repo_id: &str, limits: Limits) -> Result<Self> {
        validate_sha256(repo_id)?;
        if !root.is_absolute()
            || limits.max_records == 0
            || limits.max_snapshot_bytes == 0
            || limits.max_payload_bytes == 0
            || limits.max_retained_snapshot_bytes < limits.max_snapshot_bytes
        {
            bail!("invalid private recovery root or byte limits");
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

    /// Recover one exact enrolled version from an explicitly authorized copy.
    /// Private anonymous output is published only after backend integrity and
    /// authenticated decryption succeed. A failed copy never downgrades security.
    pub async fn recover(
        &self,
        metadata: &MetadataStore,
        prepared: &PreparedMetadata,
        manifest: &Manifest,
        path_hex: &str,
        binding: &RestoreBinding<'_>,
        warden: Option<&WardenAdapter>,
    ) -> Result<RestoredAsset> {
        if manifest.repo_id() != self.repo_id {
            bail!(BackendFailure::Security);
        }
        metadata.check_manifest(prepared, manifest)?;
        let enrollment = manifest
            .enrollment(path_hex)
            .ok_or(BackendFailure::Security)?;
        let payload = enrollment
            .payload
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("deleted enrollment has no payload to restore"))?;
        if payload.bytes() > self.limits.max_payload_bytes {
            bail!(BackendFailure::Capacity);
        }
        let backend = binding.backend_for(manifest, enrollment)?;
        if enrollment.encryption == Encryption::WardenAge {
            warden
                .ok_or(BackendFailure::Security)?
                .require_repo(&self.repo_id)?;
        }
        journal::runtime::protect(&self.directory)?;
        // Serialize only this namespace's recovery, never Git or another repo.
        let _lease = journal::try_lock(&self.directory.join("restore.lock"))?;
        // Logical contract/version determines the cache path, not a source hash
        // or arbitrary manifest-supplied filesystem destination.
        let mut hash = Sha256::new();
        hash.update(b"dracon-private-restored-asset-v1\0");
        hash.update(self.repo_id.as_bytes());
        hash.update(serde_json::to_vec(enrollment)?);
        let id = format!("{:x}", hash.finalize());
        let mut versions = std::collections::BTreeSet::new();
        let mut scanned = 0usize;
        for entry in std::fs::read_dir(&self.directory)? {
            scanned = scanned.saturating_add(1);
            if scanned > self.limits.max_records.saturating_mul(4).saturating_add(32) {
                bail!(BackendFailure::Capacity);
            }
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "source" || ext == "capture")
            {
                if let Some(stem) = path.file_stem() {
                    versions.insert(stem.to_owned());
                }
                if versions.len() > self.limits.max_records {
                    bail!(BackendFailure::Capacity);
                }
            }
        }
        if versions.len() == self.limits.max_records
            && !versions.contains(std::ffi::OsStr::new(&id))
        {
            bail!(BackendFailure::Capacity);
        }
        let mut ciphertext = tempfile::tempfile_in(&self.directory)?;
        let mut sink = VerifiedSink {
            output: &mut ciphertext,
            limit: payload.bytes(),
            bytes: 0,
            hash: Sha256::new(),
        };
        backend.get_verified(payload, &mut sink)?;
        let actual = Fingerprint::new(format!("{:x}", sink.hash.finalize()), sink.bytes)?;
        if actual != *payload {
            bail!(BackendFailure::Integrity);
        }
        ciphertext.seek(SeekFrom::Start(0))?;
        let mut plain = tempfile::tempfile_in(&self.directory)?;
        let source = match enrollment.encryption {
            Encryption::None => {
                if payload.bytes() > self.limits.max_snapshot_bytes {
                    bail!(BackendFailure::Capacity);
                }
                std::io::copy(&mut ciphertext, &mut plain)?;
                payload.clone()
            }
            Encryption::WardenAge => {
                warden
                    .ok_or(BackendFailure::Security)?
                    .decrypt(
                        ciphertext,
                        payload.bytes(),
                        &mut plain,
                        self.limits.max_snapshot_bytes,
                    )
                    .await?
            }
        };
        plain.seek(SeekFrom::Start(0))?;
        journal::retain_snapshot(
            &self.directory,
            &id,
            SnapshotKind::Source,
            self.limits,
            &mut plain,
            &source,
        )?;
        let path = self.directory.join(format!("{id}.source"));
        journal::verify_snapshot(&path, &source)?;
        Ok(RestoredAsset {
            path,
            source,
            repo_id: self.repo_id.clone(),
            path_hex: enrollment.path_hex.clone(),
            payload: payload.clone(),
        })
    }
}

struct VerifiedSink<'a> {
    output: &'a mut File,
    limit: u64,
    bytes: u64,
    hash: Sha256,
}

impl Write for VerifiedSink<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "recovery payload byte budget exceeded",
            ));
        }
        self.output.write_all(bytes)?;
        self.hash.update(bytes);
        self.bytes += bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

#[cfg(all(test, unix))]
mod tests;
