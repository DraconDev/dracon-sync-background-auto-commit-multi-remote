//! Exact-payload copy execution, independent of Git staging and daemon scheduling.

use super::backend::BackendFailure;
use super::bindings::CopyBindings;
use super::journal::{FailureCode, Job, JobLease};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;

/// Upload/read back every required copy using operator-resolved backend adapters.
///
/// The caller must authorize the backend bindings and successfully complete
/// security processing before retaining the payload. The exact copy set, owning
/// repo ID and allowed security classes are checked before any upload/readback.
/// No working-tree source file is read,
/// encrypted again, staged, committed or deleted here. Retries always use the
/// same private prepared snapshot and reverify every required copy, even when
/// historical receipts exist. This holds only the job lease, never a Git lock.
pub fn transfer_copies(lease: &JobLease, backends: &CopyBindings<'_>, now: u64) -> Result<Job> {
    let mut job = lease.load()?;
    if now == 0 {
        bail!("positive transfer timestamp required");
    }
    backends.validate_job(job.spec())?;
    let expected = job
        .payload()
        .context("payload has not been prepared")?
        .clone();
    job.begin_upload(now)?;
    lease.save(&mut job)?;
    for id in job.spec().required_copies.clone() {
        let backend = backends.backend(&id);
        let mut input = match lease.payload_snapshot() {
            Ok(input) => input,
            Err(_) => return fail(lease, &mut job, FailureCode::Integrity, now),
        };
        let mut selected = SelectedInput::new(&mut input, &expected);
        let actual = match backend.put(&mut selected) {
            Ok(actual) => actual,
            Err(error) => return fail(lease, &mut job, classify(&error), now),
        };
        if actual != expected || !selected.finished {
            return fail(lease, &mut job, FailureCode::Integrity, now);
        }
        if let Err(error) = backend.get_verified(&expected, &mut std::io::sink()) {
            // A successful put is insufficient evidence on its own.
            return fail(lease, &mut job, classify(&error), now);
        }
        job.record_copy(&id, expected.clone(), now)?;
        lease.save(&mut job)?;
    }
    Ok(job)
}

// Known adapters capture through EOF before remote publication. Validate the
// selected identity at that boundary, rather than detecting changed bytes only
// after a put has already published an unapproved representation.
struct SelectedInput<'a> {
    input: &'a mut dyn Read,
    expected: &'a super::reference::Fingerprint,
    hash: Sha256,
    bytes: u64,
    finished: bool,
    failed: bool,
}
impl<'a> SelectedInput<'a> {
    fn new(input: &'a mut dyn Read, expected: &'a super::reference::Fingerprint) -> Self {
        Self {
            input,
            expected,
            hash: Sha256::new(),
            bytes: 0,
            finished: false,
            failed: false,
        }
    }
    fn invalid(&mut self) -> std::io::Error {
        self.failed = true;
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "selected payload changed before publication",
        )
    }
}
impl Read for SelectedInput<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.failed {
            return Err(self.invalid());
        }
        if self.finished || output.is_empty() {
            return Ok(0);
        }
        let limit = self
            .expected
            .bytes()
            .saturating_sub(self.bytes)
            .saturating_add(1)
            .min(output.len() as u64) as usize;
        let count = self.input.read(&mut output[..limit])?;
        if count == 0 {
            if self.bytes != self.expected.bytes()
                || format!("{:x}", self.hash.clone().finalize()) != self.expected.sha256()
            {
                return Err(self.invalid());
            }
            self.finished = true;
            return Ok(0);
        }
        self.bytes = self
            .bytes
            .checked_add(count as u64)
            .ok_or_else(|| self.invalid())?;
        if self.bytes > self.expected.bytes() {
            return Err(self.invalid());
        }
        self.hash.update(&output[..count]);
        Ok(count)
    }
}

pub(super) fn classify(error: &anyhow::Error) -> FailureCode {
    if let Some(kind) = error.downcast_ref::<BackendFailure>() {
        return match kind {
            BackendFailure::Capacity => FailureCode::Capacity,
            BackendFailure::Integrity => FailureCode::Integrity,
            BackendFailure::Security => FailureCode::Security,
        };
    }
    #[cfg(unix)]
    if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
        matches!(
            error.raw_os_error(),
            Some(libc::ENOSPC) | Some(libc::EDQUOT)
        )
    }) {
        return FailureCode::Capacity;
    }
    #[cfg(unix)]
    if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.raw_os_error() == Some(libc::ELOOP))
    {
        return FailureCode::Security;
    }
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        return match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData => {
                FailureCode::Integrity
            }
            std::io::ErrorKind::PermissionDenied => FailureCode::Security,
            _ => FailureCode::Transient,
        };
    }
    FailureCode::Transient
}

fn fail(lease: &JobLease, job: &mut Job, code: FailureCode, now: u64) -> Result<Job> {
    let retry_at = matches!(code, FailureCode::Transient).then(|| now.saturating_add(30));
    job.note_failure(code, retry_at)?;
    lease.save(job)?;
    // Never persist or surface arbitrary backend/credential-bearing messages.
    bail!("payload copy failed: {code:?}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_core::backend::{ImmutableBackend, LocalBackend};
    use crate::storage_core::bindings::ApprovedBackend;
    use crate::storage_core::journal::{
        encode_relative_path, Encryption, JobSpec, Journal, Limits, Phase,
    };
    use crate::storage_core::reference::Fingerprint;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};

    fn approved<'a>(backends: BTreeMap<String, &'a dyn ImmutableBackend>) -> CopyBindings<'a> {
        CopyBindings::new(
            "a".repeat(64),
            backends
                .into_iter()
                .map(|(id, backend)| {
                    (
                        id,
                        ApprovedBackend::for_security(backend, vec![Encryption::None]).unwrap(),
                    )
                })
                .collect(),
        )
        .unwrap()
    }

    fn fixture(temp: &std::path::Path) -> (Journal, Job) {
        let bytes = b"approved retained representation";
        let fingerprint =
            Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
        let spec = JobSpec {
            repo_id: "a".repeat(64),
            path_hex: encode_relative_path(b"asset.bin").unwrap(),
            source: fingerprint.clone(),
            policy_sha256: "c".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into(), "recovery".into()],
            required_git_targets: vec!["github".into()],
            encryption: Encryption::None,
        };
        let journal =
            Journal::open(&temp.join("journal"), &spec.repo_id, Limits::default()).unwrap();
        let job = journal.create(spec).unwrap();
        let lease = journal.lease(job.id()).unwrap();
        lease.capture_snapshot(&mut &bytes[..]).unwrap();
        let job = lease.retain_payload(&mut &bytes[..], &fingerprint).unwrap();
        (journal, job)
    }

    #[test]
    fn required_copies_use_retained_bytes_and_restart_rechecks_corruption() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let primary = LocalBackend::open(&temp.path().join("primary"), 1024).unwrap();
        let recovery = LocalBackend::open(&temp.path().join("recovery"), 1024).unwrap();
        let backends = approved(BTreeMap::from([
            ("primary".into(), &primary as &dyn ImmutableBackend),
            ("recovery".into(), &recovery as &dyn ImmutableBackend),
        ]));
        let lease = journal.lease(job.id()).unwrap();
        let ready = transfer_copies(&lease, &backends, 1).unwrap();
        assert_eq!(ready.phase(), Phase::ReadyToStage);
        let identity = ready.payload().unwrap().clone();
        drop(lease);
        // A backend object can be damaged after an earlier receipt.
        let damaged = temp.path().join("primary").join(identity.sha256());
        std::fs::write(&damaged, b"corrupt").unwrap();
        let lease = journal.lease(job.id()).unwrap();
        assert!(transfer_copies(&lease, &backends, 2).is_err());
        assert_eq!(
            lease.load().unwrap().failure(),
            Some(FailureCode::Integrity)
        );
        assert!(lease.payload_snapshot().is_ok());
        assert_eq!(std::fs::read(&damaged).unwrap(), b"corrupt");
        assert!(transfer_copies(&lease, &backends, 100).is_err());
    }

    #[test]
    fn missing_bindings_and_capacity_failure_cannot_mark_ready() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let lease = journal.lease(job.id()).unwrap();
        assert!(transfer_copies(&lease, &approved(BTreeMap::new()), 1).is_err());
        assert_eq!(lease.load().unwrap().phase(), Phase::Prepared);
        let primary = LocalBackend::open(&temp.path().join("primary"), 1024).unwrap();
        let recovery = LocalBackend::open(&temp.path().join("recovery"), 1).unwrap();
        let backends = approved(BTreeMap::from([
            ("primary".into(), &primary as &dyn ImmutableBackend),
            ("recovery".into(), &recovery as &dyn ImmutableBackend),
        ]));
        assert!(transfer_copies(&lease, &backends, 2).is_err());
        let failed = lease.load().unwrap();
        assert_eq!(failed.phase(), Phase::PrimaryVerified);
        assert_eq!(failed.failure(), Some(FailureCode::Capacity));
        assert!(lease.payload_snapshot().is_ok());
    }

    #[test]
    fn foreign_or_unapproved_security_bindings_refuse_before_backend_io() {
        struct NoIo;
        impl ImmutableBackend for NoIo {
            fn put(&self, _input: &mut dyn Read) -> Result<Fingerprint> {
                panic!("unapproved binding performed an upload");
            }
            fn get_verified(&self, _identity: &Fingerprint, _output: &mut dyn Write) -> Result<()> {
                panic!("unapproved binding performed a readback");
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let lease = journal.lease(job.id()).unwrap();
        for repo_id in ["a".repeat(64), "b".repeat(64)] {
            let bindings = CopyBindings::new(
                repo_id,
                BTreeMap::from([
                    ("primary".into(), ApprovedBackend::encrypted(&NoIo)),
                    ("recovery".into(), ApprovedBackend::encrypted(&NoIo)),
                ]),
            )
            .unwrap();
            // This fixture is explicitly non-sensitive. Encrypted-only grants
            // must refuse it, even with correct identifiers and repo identity.
            assert!(transfer_copies(&lease, &bindings, 1).is_err());
            let unchanged = lease.load().unwrap();
            assert_eq!(unchanged.phase(), Phase::Prepared);
            assert!(unchanged.failure().is_none());
            assert!(lease.payload_snapshot().is_ok());
        }
        let foreign = CopyBindings::new(
            "b".repeat(64),
            BTreeMap::from([
                (
                    "primary".into(),
                    ApprovedBackend::for_security(&NoIo, vec![Encryption::None]).unwrap(),
                ),
                (
                    "recovery".into(),
                    ApprovedBackend::for_security(&NoIo, vec![Encryption::None]).unwrap(),
                ),
            ]),
        )
        .unwrap();
        assert!(transfer_copies(&lease, &foreign, 1).is_err());
        assert!(ApprovedBackend::for_security(&NoIo, vec![]).is_err());
        assert!(
            ApprovedBackend::for_security(&NoIo, vec![Encryption::None, Encryption::None]).is_err()
        );
        assert!(CopyBindings::new("PRIVATE-SECRET".into(), BTreeMap::new()).is_err());
        assert!(CopyBindings::new(
            "a".repeat(64),
            BTreeMap::from([(
                "https://PRIVATE-SECRET".into(),
                ApprovedBackend::encrypted(&NoIo)
            ),])
        )
        .is_err());
    }

    #[test]
    fn apparent_upload_success_without_readback_is_not_a_receipt() {
        struct Unverified;
        impl ImmutableBackend for Unverified {
            fn put(&self, input: &mut dyn Read) -> Result<Fingerprint> {
                let mut bytes = Vec::new();
                input.read_to_end(&mut bytes)?;
                Fingerprint::new(format!("{:x}", Sha256::digest(&bytes)), bytes.len() as u64)
            }
            fn get_verified(&self, _: &Fingerprint, _: &mut dyn Write) -> Result<()> {
                bail!(BackendFailure::Integrity)
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let lease = journal.lease(job.id()).unwrap();
        let backend = Unverified;
        let backends = approved(BTreeMap::from([
            ("primary".into(), &backend as &dyn ImmutableBackend),
            ("recovery".into(), &backend as &dyn ImmutableBackend),
        ]));
        assert!(transfer_copies(&lease, &backends, 1).is_err());
        assert_eq!(lease.load().unwrap().phase(), Phase::Uploading);
        assert_eq!(
            lease.load().unwrap().failure(),
            Some(FailureCode::Integrity)
        );
    }
    #[test]
    fn transient_readback_failure_retains_exact_payload_and_obeys_backoff() {
        use crate::storage_core::reference::Fingerprint;
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Flaky {
            local: LocalBackend,
            fail_readback: AtomicBool,
        }
        impl ImmutableBackend for Flaky {
            fn put(&self, input: &mut dyn Read) -> Result<Fingerprint> {
                self.local.put(input)
            }
            fn get_verified(&self, identity: &Fingerprint, output: &mut dyn Write) -> Result<()> {
                if self.fail_readback.load(Ordering::Relaxed) {
                    return Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into());
                }
                self.local.get_verified(identity, output)
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let lease = journal.lease(job.id()).unwrap();
        let backend = Flaky {
            local: LocalBackend::open(&temp.path().join("objects"), 1024).unwrap(),
            fail_readback: AtomicBool::new(true),
        };
        let backends = approved(BTreeMap::from([
            ("primary".into(), &backend as &dyn ImmutableBackend),
            ("recovery".into(), &backend as &dyn ImmutableBackend),
        ]));
        assert!(transfer_copies(&lease, &backends, 1).is_err());
        assert_eq!(
            lease.load().unwrap().failure(),
            Some(FailureCode::Transient)
        );
        assert!(lease.payload_snapshot().is_ok());
        backend.fail_readback.store(false, Ordering::Relaxed);
        assert!(transfer_copies(&lease, &backends, 30).is_err());
        assert_eq!(
            transfer_copies(&lease, &backends, 31).unwrap().phase(),
            Phase::ReadyToStage
        );
    }
    #[test]
    fn changed_snapshot_is_rejected_before_immutable_backend_publication() {
        struct Changed {
            local: LocalBackend,
            path: std::path::PathBuf,
            replacement: Vec<u8>,
        }
        impl ImmutableBackend for Changed {
            fn put(&self, input: &mut dyn Read) -> Result<Fingerprint> {
                std::fs::write(&self.path, &self.replacement)?;
                self.local.put(input)
            }
            fn get_verified(&self, id: &Fingerprint, output: &mut dyn Write) -> Result<()> {
                self.local.get_verified(id, output)
            }
        }
        for replacement in [
            vec![b'z'; b"approved retained representation".len()],
            b"short".to_vec(),
            vec![b'z'; 100],
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (journal, job) = fixture(temp.path());
            let source = std::fs::read(
                temp.path()
                    .join("journal")
                    .join("a".repeat(64))
                    .join(format!("{}.source", job.id())),
            )
            .unwrap();
            let root = temp.path().join("objects");
            let backend = Changed {
                local: LocalBackend::open(&root, 1024).unwrap(),
                path: temp
                    .path()
                    .join("journal")
                    .join("a".repeat(64))
                    .join(format!("{}.payload", job.id())),
                replacement,
            };
            let bindings = approved(BTreeMap::from([
                ("primary".into(), &backend as &dyn ImmutableBackend),
                ("recovery".into(), &backend as &dyn ImmutableBackend),
            ]));
            let lease = journal.lease(job.id()).unwrap();
            assert!(transfer_copies(&lease, &bindings, 1).is_err());
            assert_eq!(
                lease.load().unwrap().failure(),
                Some(FailureCode::Integrity)
            );
            assert_eq!(
                std::fs::read_dir(&root)
                    .unwrap()
                    .filter(|entry| !entry
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .as_encoded_bytes()
                        .starts_with(b"."))
                    .count(),
                0
            );
            assert_eq!(
                std::fs::read(backend.path.with_extension("source")).unwrap(),
                source
            );
        }
    }

    #[test]
    fn selected_input_requires_exact_eof_even_for_empty_payloads() {
        let empty = Fingerprint::new(format!("{:x}", Sha256::digest(b"")), 0).unwrap();
        let mut input = &b""[..];
        let mut selected = SelectedInput::new(&mut input, &empty);
        assert_eq!(selected.read(&mut []).unwrap(), 0);
        assert!(!selected.finished);
        assert_eq!(selected.read(&mut [0]).unwrap(), 0);
        assert!(selected.finished);
        let mut input = &b"extra"[..];
        let mut selected = SelectedInput::new(&mut input, &empty);
        assert!(selected.read(&mut [0; 64]).is_err());
        assert!(selected.read(&mut [0; 64]).is_err());
        assert!(!selected.finished);
    }
    #[test]
    fn incomplete_consumption_cannot_produce_a_copy_receipt() {
        struct Partial(Fingerprint);
        impl ImmutableBackend for Partial {
            fn put(&self, input: &mut dyn Read) -> Result<Fingerprint> {
                input.read_exact(&mut [0; 1])?;
                Ok(self.0.clone())
            }
            fn get_verified(&self, _: &Fingerprint, _: &mut dyn Write) -> Result<()> {
                panic!("unfinished upload must not reach readback")
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let backend = Partial(job.payload().unwrap().clone());
        let bindings = approved(BTreeMap::from([
            ("primary".into(), &backend as &dyn ImmutableBackend),
            ("recovery".into(), &backend as &dyn ImmutableBackend),
        ]));
        let lease = journal.lease(job.id()).unwrap();
        assert!(transfer_copies(&lease, &bindings, 1).is_err());
        assert_eq!(
            lease.load().unwrap().failure(),
            Some(FailureCode::Integrity)
        );
    }

    #[test]
    fn changed_snapshot_cannot_reach_s3_transport() {
        use crate::storage_core::s3::{ConditionalPut, S3Backend, S3Transport};
        use std::cell::Cell;
        use std::fs::File;
        struct Transport(Cell<usize>);
        impl S3Transport for Transport {
            fn put_if_absent(&self, _: &Fingerprint, _: File) -> Result<ConditionalPut> {
                self.0.set(self.0.get() + 1);
                bail!("unexpected remote publication")
            }
            fn get(&self, _: &Fingerprint) -> Result<Box<dyn Read>> {
                panic!("unexpected readback")
            }
        }
        struct Changed<'a> {
            backend: S3Backend<&'a Transport>,
            path: std::path::PathBuf,
        }
        // Borrowed transports keep the witness counter available after transfer.
        impl S3Transport for &Transport {
            fn put_if_absent(&self, id: &Fingerprint, file: File) -> Result<ConditionalPut> {
                (*self).put_if_absent(id, file)
            }
            fn get(&self, id: &Fingerprint) -> Result<Box<dyn Read>> {
                (*self).get(id)
            }
        }
        impl ImmutableBackend for Changed<'_> {
            fn put(&self, input: &mut dyn Read) -> Result<Fingerprint> {
                std::fs::write(
                    &self.path,
                    vec![b'z'; b"approved retained representation".len()],
                )?;
                self.backend.put(input)
            }
            fn get_verified(&self, id: &Fingerprint, output: &mut dyn Write) -> Result<()> {
                self.backend.get_verified(id, output)
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let (journal, job) = fixture(temp.path());
        let transport = Transport(Cell::new(0));
        let backend = Changed {
            backend: S3Backend::new(&transport, 1024).unwrap(),
            path: temp
                .path()
                .join("journal")
                .join("a".repeat(64))
                .join(format!("{}.payload", job.id())),
        };
        let bindings = approved(BTreeMap::from([
            ("primary".into(), &backend as &dyn ImmutableBackend),
            ("recovery".into(), &backend as &dyn ImmutableBackend),
        ]));
        let lease = journal.lease(job.id()).unwrap();
        assert!(transfer_copies(&lease, &bindings, 1).is_err());
        assert_eq!(
            lease.load().unwrap().failure(),
            Some(FailureCode::Integrity)
        );
        assert_eq!(transport.0.get(), 0);
    }
}
