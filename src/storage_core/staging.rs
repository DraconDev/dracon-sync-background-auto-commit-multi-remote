//! Exact-version staging bundles, validated before any Git index mutation.
//!
//! A bundle binds verified object jobs to the decoded manifest that produced its
//! retained ciphertext. Git publication/acknowledgment is a separate transaction.

use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::fs::File;

use super::backend::BackendFailure;
use super::journal::{validate_path_hex, JobLease, Phase};
use super::manifest::Manifest;
use super::metadata::{MetadataStore, PreparedMetadata};
use super::reference::Pointer;

/// A local validated bundle retaining asset leases until Git verification completes.
/// It grants no new destinations and never includes raw source fingerprints.
pub struct StageBundle<'a> {
    repo_id: String,
    manifest: Manifest,
    metadata: File,
    prepared: PreparedMetadata,
    jobs: Vec<&'a JobLease>,
    pointers: Vec<(Vec<u8>, Pointer)>,
}

impl<'a> StageBundle<'a> {
    /// Validate exact ownership, durability, policy and representation correspondence.
    /// The caller must approve the complete manifest/enrollment contract and hold
    /// the supplied leases in a consistent order. No Git/filter/backend I/O runs.
    pub fn build(
        store: &MetadataStore,
        prepared: &PreparedMetadata,
        manifest: &Manifest,
        leases: Vec<&'a JobLease>,
    ) -> Result<Self> {
        let metadata = store.check_manifest(prepared, manifest)?;
        let mut seen = BTreeSet::new();
        let mut pointers = Vec::with_capacity(leases.len());
        for lease in &leases {
            let job = lease.load()?;
            if job.spec().repo_id != manifest.repo_id()
                || !matches!(job.phase(), Phase::ReadyToStage | Phase::Staged)
                || job.failure().is_some()
                || !seen.insert(job.spec().path_hex.clone())
            {
                bail!("staging jobs do not have unique owned verified versions");
            }
            let enrollment = manifest.enrollment(&job.spec().path_hex)
                .ok_or(BackendFailure::Integrity)?;
            let mut copies = job.spec().required_copies.clone();
            copies.sort();
            if enrollment.contract_sha256 != job.spec().policy_sha256
                || enrollment.primary != job.spec().primary
                || enrollment.required_copies != copies
                || enrollment.encryption != job.spec().encryption
            {
                bail!(BackendFailure::Security);
            }
            let pointer = Pointer::new(job.payload().cloned().ok_or(BackendFailure::Integrity)?)?;
            if !enrollment.matches_pointer(&pointer) {
                bail!(BackendFailure::Integrity);
            }
            // Source snapshots are also verified before building an index plan.
            // Working-file race checks remain a Git-worker responsibility.
            lease.source_snapshot()?;
            lease.payload_snapshot()?;
            pointers.push((decode_path(&job.spec().path_hex)?, pointer));
        }
        pointers.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self {
            repo_id: manifest.repo_id().into(), manifest: manifest.clone(), metadata,
            prepared: prepared.clone(), jobs: leases, pointers,
        })
    }

    /// Stable operator-bound owning repository identity.
    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    /// Exact encoded pointers for this batch; these contain representation identities.
    pub fn pointers(&self) -> &[(Vec<u8>, Pointer)] {
        &self.pointers
    }

    /// Complete decoded metadata for validating untouched enrolled index paths.
    /// Keep this private; it is not the metadata bytes to publish in Git.
    pub(crate) fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub(crate) fn metadata(&self) -> Result<File> {
        use std::io::{Seek, SeekFrom};
        let mut file = self.metadata.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }

    pub(crate) fn prepared(&self) -> &PreparedMetadata {
        &self.prepared
    }

    pub(crate) fn jobs(&self) -> &[&JobLease] {
        &self.jobs
    }
}

pub(crate) fn decode_path(hex: &str) -> Result<Vec<u8>> {
    validate_path_hex(hex)?;
    hex.as_bytes().chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}
