//! Networkless exact-version clean transformation for enrolled assets.
//!
//! No raw input is written to the output. Operator binding, required Git filter
//! installation and manifest/index grouping remain the caller's responsibilities.

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

use super::backend::BackendFailure;
use super::journal::{JobLease, Phase};
use super::manifest::Manifest;
use super::metadata::{MetadataStore, PreparedMetadata};
use super::reference::{Fingerprint, Pointer};

/// A verified local reference for one manifest version, held under its job lease.
/// Private plaintext fingerprints are deliberately absent from Debug/serialization.
pub struct PreparedClean<'a> {
    _lease: &'a JobLease,
    source: Fingerprint,
    pointer: Pointer,
}

impl<'a> PreparedClean<'a> {
    /// Require matching protected metadata, immutable snapshots and verified copies.
    /// The caller supplies the exact approved path rather than trusting a job's path.
    /// No backend, encryption subprocess, key discovery or working-file write runs.
    pub fn new(
        store: &MetadataStore,
        prepared: &PreparedMetadata,
        manifest: &Manifest,
        path_hex: &str,
        lease: &'a JobLease,
    ) -> Result<Self> {
        super::journal::validate_path_hex(path_hex)?;
        store.check_manifest(prepared, manifest)?;
        let job = lease.load()?;
        if job.spec().repo_id != manifest.repo_id()
            || job.spec().path_hex != path_hex
            || !matches!(
                job.phase(),
                Phase::ReadyToStage | Phase::Staged | Phase::Committed | Phase::Preserved
            )
            || job.failure().is_some()
        {
            bail!("asset has no eligible exact-version clean reference");
        }
        let enrolled = manifest
            .enrollment(path_hex)
            .ok_or(BackendFailure::Security)?;
        let mut copies = job.spec().required_copies.clone();
        copies.sort();
        let pointer = Pointer::new(job.payload().cloned().ok_or(BackendFailure::Integrity)?)?;
        if enrolled.contract_sha256 != job.spec().policy_sha256
            || enrolled.primary != job.spec().primary
            || enrolled.required_copies != copies
            || enrolled.encryption != job.spec().encryption
            || !enrolled.matches_pointer(&pointer)
        {
            bail!(BackendFailure::Security);
        }
        lease.source_snapshot()?;
        lease.payload_snapshot()?;
        Ok(Self {
            _lease: lease,
            source: job.spec().source.clone(),
            pointer,
        })
    }

    /// Stream and verify Git's exact input, then write only its canonical pointer.
    /// Unhydrated input may be the same canonical pointer; a different pointer
    /// cannot be adopted. Errors before verification leave output untouched.
    /// Read requests/memory are bounded; input length is capped by the selected
    /// source length plus the small pointer recognition window.
    pub fn clean(&self, input: &mut dyn Read, output: &mut dyn Write) -> Result<()> {
        let mut prefix = Vec::with_capacity(1025);
        input.take(1025).read_to_end(&mut prefix)?;
        if Pointer::parse(&prefix).ok().as_ref() == Some(&self.pointer) {
            output.write_all(&self.pointer.encode())?;
            return Ok(());
        }
        let mut bytes = prefix.len() as u64;
        if bytes > self.source.bytes() {
            bail!("asset changed; prepare its exact version before staging");
        }
        let mut hash = Sha256::new();
        hash.update(&prefix);
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let available = self.source.bytes().saturating_sub(bytes).saturating_add(1);
            let requested = usize::try_from(available.min(buffer.len() as u64))?;
            let count = input.read(&mut buffer[..requested])?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .ok_or(BackendFailure::Capacity)?;
            if bytes > self.source.bytes() {
                bail!("asset changed; prepare its exact version before staging");
            }
            hash.update(&buffer[..count]);
        }
        if bytes != self.source.bytes() || format!("{:x}", hash.finalize()) != self.source.sha256()
        {
            bail!("asset changed; prepare its exact version before staging");
        }
        output.write_all(&self.pointer.encode())?;
        Ok(())
    }
}
