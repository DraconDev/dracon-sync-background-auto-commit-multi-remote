//! Exact-payload copy execution, independent of Git staging and daemon scheduling.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

use super::backend::{BackendFailure, ImmutableBackend};
use super::journal::{FailureCode, Job, JobLease};

/// Upload/read back every required copy using operator-resolved backend adapters.
///
/// The caller must authorize the backend bindings and successfully complete
/// security processing before retaining the payload. No source file is read,
/// encrypted again, staged, committed or deleted here. Retries always use the
/// same private prepared snapshot and reverify every required copy, even when
/// historical receipts exist. This holds only the job lease, never a Git lock.
pub fn transfer_copies(
    lease: &JobLease,
    backends: &BTreeMap<String, &dyn ImmutableBackend>,
    now: u64,
) -> Result<Job> {
    let mut job = lease.load()?;
    if now == 0
        || backends.len() != job.spec().required_copies.len()
        || job
            .spec()
            .required_copies
            .iter()
            .any(|id| !backends.contains_key(id))
    {
        bail!("exact required backend bindings and a positive timestamp are required");
    }
    let expected = job
        .payload()
        .context("payload has not been prepared")?
        .clone();
    job.begin_upload(now)?;
    lease.save(&mut job)?;
    for id in job.spec().required_copies.clone() {
        let backend = backends[&id];
        let mut input = match lease.payload_snapshot() {
            Ok(input) => input,
            Err(_) => return fail(lease, &mut job, FailureCode::Integrity, now),
        };
        let actual = match backend.put(&mut input) {
            Ok(actual) => actual,
            Err(error) => return fail(lease, &mut job, classify(&error), now),
        };
        if actual != expected {
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

fn classify(error: &anyhow::Error) -> FailureCode {
    if let Some(kind) = error.downcast_ref::<BackendFailure>() {
        return match kind {
            BackendFailure::Capacity => FailureCode::Capacity,
            BackendFailure::Integrity => FailureCode::Integrity,
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
    use crate::storage_core::backend::LocalBackend;
    use crate::storage_core::journal::{
        encode_relative_path, Encryption, JobSpec, Journal, Limits, Phase,
    };
    use crate::storage_core::reference::Fingerprint;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

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
        let backends = BTreeMap::from([
            ("primary".into(), &primary as &dyn ImmutableBackend),
            ("recovery".into(), &recovery as &dyn ImmutableBackend),
        ]);
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
        assert!(transfer_copies(&lease, &BTreeMap::new(), 1).is_err());
        assert_eq!(lease.load().unwrap().phase(), Phase::Prepared);
        let primary = LocalBackend::open(&temp.path().join("primary"), 1024).unwrap();
        let recovery = LocalBackend::open(&temp.path().join("recovery"), 1).unwrap();
        let backends = BTreeMap::from([
            ("primary".into(), &primary as &dyn ImmutableBackend),
            ("recovery".into(), &recovery as &dyn ImmutableBackend),
        ]);
        assert!(transfer_copies(&lease, &backends, 2).is_err());
        let failed = lease.load().unwrap();
        assert_eq!(failed.phase(), Phase::PrimaryVerified);
        assert_eq!(failed.failure(), Some(FailureCode::Capacity));
        assert!(lease.payload_snapshot().is_ok());
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
        let backends = BTreeMap::from([
            ("primary".into(), &backend as &dyn ImmutableBackend),
            ("recovery".into(), &backend as &dyn ImmutableBackend),
        ]);
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
        let backends = BTreeMap::from([
            ("primary".into(), &backend as &dyn ImmutableBackend),
            ("recovery".into(), &backend as &dyn ImmutableBackend),
        ]);
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
}
