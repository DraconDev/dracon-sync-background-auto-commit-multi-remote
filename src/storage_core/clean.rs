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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_core::journal::{
        encode_relative_path, Encryption, JobSpec, Journal, Limits,
    };

    fn fixture(bytes: &[u8]) -> (tempfile::TempDir, Journal, String, Fingerprint) {
        let temp = tempfile::tempdir().unwrap();
        let source =
            Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
        let journal = Journal::open(
            &temp.path().join("journal"),
            &"a".repeat(64),
            Limits::default(),
        )
        .unwrap();
        let job = journal
            .create(JobSpec {
                repo_id: "a".repeat(64),
                path_hex: encode_relative_path(b"asset.bin").unwrap(),
                source: source.clone(),
                policy_sha256: "b".repeat(64),
                primary: "primary".into(),
                required_copies: vec!["primary".into()],
                required_git_targets: vec!["github".into()],
                encryption: Encryption::WardenAge,
            })
            .unwrap();
        (temp, journal, job.id().into(), source)
    }

    // These unit tests isolate streaming mechanics. Real Git integration exercises
    // the public constructor with verified snapshots, copies and protected metadata.
    fn clean<'a>(lease: &'a JobLease, source: Fingerprint) -> PreparedClean<'a> {
        PreparedClean {
            _lease: lease,
            source,
            pointer: Pointer::new(Fingerprint::new("c".repeat(64), 200).unwrap()).unwrap(),
        }
    }

    #[test]
    fn unchanged_input_and_exact_unhydrated_pointer_are_deterministic() {
        let bytes = b"private source version";
        let (_temp, journal, id, source) = fixture(bytes);
        let lease = journal.lease(&id).unwrap();
        let clean = clean(&lease, source.clone());
        let mut output = Vec::new();
        clean.clean(&mut &bytes[..], &mut output).unwrap();
        assert_eq!(output, clean.pointer.encode());
        assert!(!String::from_utf8_lossy(&output).contains(source.sha256()));
        let mut repeated = Vec::new();
        clean.clean(&mut &output[..], &mut repeated).unwrap();
        assert_eq!(repeated, output);
    }

    #[test]
    fn changed_shorter_longer_and_foreign_pointer_inputs_emit_nothing() {
        let bytes = b"selected source";
        let (_temp, journal, id, source) = fixture(bytes);
        let lease = journal.lease(&id).unwrap();
        let clean = clean(&lease, source);
        let foreign = Pointer::new(Fingerprint::new("d".repeat(64), 200).unwrap())
            .unwrap()
            .encode();
        for input in [
            b"Selected source".to_vec(),
            b"selected sourc".to_vec(),
            b"selected source!".to_vec(),
            foreign,
        ] {
            let mut output = b"existing output".to_vec();
            assert!(clean.clean(&mut &input[..], &mut output).is_err());
            assert_eq!(output, b"existing output");
        }
    }

    #[test]
    fn large_source_uses_bounded_read_requests() {
        struct Bounded<'a>(&'a [u8]);
        impl Read for Bounded<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                assert!(buffer.len() <= 64 * 1024);
                self.0.read(buffer)
            }
        }
        let bytes = vec![7; 4 * 1024 * 1024];
        let (_temp, journal, id, source) = fixture(&bytes);
        let lease = journal.lease(&id).unwrap();
        let clean = clean(&lease, source);
        let mut output = Vec::new();
        clean.clean(&mut Bounded(&bytes), &mut output).unwrap();
        assert_eq!(output, clean.pointer.encode());
    }

    #[test]
    fn oversized_unknown_stream_stops_at_bounded_recognition_window() {
        struct Endless {
            bytes: usize,
        }
        impl Read for Endless {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.bytes += buffer.len();
                assert!(self.bytes <= 1025);
                buffer.fill(7);
                Ok(buffer.len())
            }
        }
        let (_temp, journal, id, source) = fixture(b"short");
        let lease = journal.lease(&id).unwrap();
        let clean = clean(&lease, source);
        let mut input = Endless { bytes: 0 };
        let mut output = Vec::new();
        assert!(clean.clean(&mut input, &mut output).is_err());
        assert_eq!(input.bytes, 1025);
        assert!(output.is_empty());
    }

    #[test]
    fn read_failure_cannot_publish_partial_reference_or_raw_input() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture read failed"))
            }
        }
        let (_temp, journal, id, source) = fixture(b"private bytes");
        let lease = journal.lease(&id).unwrap();
        let clean = clean(&lease, source);
        let mut output = Vec::new();
        assert!(clean.clean(&mut Failing, &mut output).is_err());
        assert!(output.is_empty());
    }
}
