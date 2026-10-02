//! Durable preparation/copy execution for already captured exact versions.

use super::backend::BackendFailure;
use super::bindings::CopyBindings;
use super::journal::{Encryption, Job, JobLease, Phase};
use super::security::{require_age_header, WardenAdapter};
use anyhow::{bail, Result};

/// Advance a selected captured job to verified copies without Git/working edits.
/// Caller must resolve operator grants and approve the captured job/contract.
/// Run from a blocking worker with an async reactor available for Warden I/O;
/// network adapters must not block an async runtime worker. Retries retain the
/// exact approved representation and never select a different source/version.
pub async fn advance(
    lease: &JobLease,
    backends: &CopyBindings<'_>,
    warden: Option<&WardenAdapter>,
    now: u64,
) -> Result<Job> {
    let job = lease.load()?;
    backends.validate_job(job.spec())?;
    if now == 0
        || !job.retry_eligible(now)
        || !matches!(
            job.phase(),
            Phase::Captured
                | Phase::Prepared
                | Phase::Uploading
                | Phase::PrimaryVerified
                | Phase::ReadyToStage
        )
    {
        bail!("selected job is not eligible for preparation/transfer");
    }
    lease.source_snapshot()?;
    if job.phase() == Phase::Captured {
        match job.spec().encryption {
            Encryption::None => {
                let mut source = lease.source_snapshot()?;
                lease.retain_payload(&mut source, &job.spec().source)?;
            }
            Encryption::WardenAge => {
                warden
                    .ok_or(BackendFailure::Security)?
                    .prepare(lease, now)
                    .await?;
            }
        }
    }
    if job.spec().encryption == Encryption::WardenAge {
        let mut payload = lease.payload_snapshot()?;
        require_age_header(&mut payload).map_err(|_| BackendFailure::Security)?;
    }
    super::transfer::transfer_copies(lease, backends, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_core::backend::LocalBackend;
    use crate::storage_core::bindings::ApprovedBackend;
    use crate::storage_core::journal::{encode_relative_path, JobSpec, Journal, Limits};
    use crate::storage_core::reference::Fingerprint;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    #[cfg(unix)]
    #[tokio::test]
    async fn encrypted_preparation_copies_only_approved_representation() {
        use crate::storage_core::backend::ImmutableBackend;
        use std::os::unix::fs::PermissionsExt;
        for approved in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let (journal, id) = fixture(temp.path(), Encryption::WardenAge);
            let repo = temp.path().join("repo");
            std::fs::create_dir(&repo).unwrap();
            git2::Repository::init(&repo).unwrap();
            let executable = temp.path().join("warden-fixture");
            // Synthetic executable verifies worker/SDK composition, not encryption.
            let script = if approved {
                "#!/bin/sh\ncat >/dev/null\nprintf 'age-encryption.org/v1\\nopaque fixture\\n'\n"
            } else {
                "#!/bin/sh\ncat\n"
            };
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let adapter = WardenAdapter::new(
                &executable,
                &repo,
                &"a".repeat(64),
                std::time::Duration::from_secs(5),
            )
            .unwrap();
            let backend = LocalBackend::open(&temp.path().join("objects"), 1024).unwrap();
            let bindings = CopyBindings::new(
                "a".repeat(64),
                BTreeMap::from([(
                    "primary".into(),
                    ApprovedBackend::for_security(&backend, vec![Encryption::WardenAge]).unwrap(),
                )]),
            )
            .unwrap();
            let lease = journal.lease(&id).unwrap();
            let outcome = advance(&lease, &bindings, Some(&adapter), 1).await;
            if approved {
                let ready = outcome.unwrap();
                let identity = ready.payload().unwrap().clone();
                let mut bytes = Vec::new();
                backend.get_verified(&identity, &mut bytes).unwrap();
                assert_eq!(bytes, b"age-encryption.org/v1\nopaque fixture\n");
                std::fs::write(&executable, "#!/bin/sh\nexit 71\n").unwrap();
                assert_eq!(
                    advance(&lease, &bindings, Some(&adapter), 2)
                        .await
                        .unwrap()
                        .payload(),
                    Some(&identity)
                );
            } else {
                assert!(outcome.is_err());
                assert_eq!(lease.load().unwrap().phase(), Phase::Captured);
                assert!(lease.load().unwrap().payload().is_none());
                assert!(!temp
                    .path()
                    .join("objects")
                    .join(lease.load().unwrap().spec().source.sha256())
                    .exists());
            }
        }
    }
    fn fixture(path: &std::path::Path, encryption: Encryption) -> (Journal, String) {
        let bytes = b"captured exact version";
        let journal =
            Journal::open(&path.join("journal"), &"a".repeat(64), Limits::default()).unwrap();
        let job = journal
            .create(JobSpec {
                repo_id: "a".repeat(64),
                path_hex: encode_relative_path(b"asset.bin").unwrap(),
                source: Fingerprint::new(
                    format!("{:x}", Sha256::digest(bytes)),
                    bytes.len() as u64,
                )
                .unwrap(),
                policy_sha256: "b".repeat(64),
                primary: "primary".into(),
                required_copies: vec!["primary".into()],
                required_git_targets: vec!["github".into()],
                encryption,
            })
            .unwrap();
        journal
            .lease(job.id())
            .unwrap()
            .capture_snapshot(&mut &bytes[..])
            .unwrap();
        (journal, job.id().into())
    }
    #[tokio::test]
    async fn captured_version_is_prepared_copied_and_retry_keeps_identity() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, id) = fixture(temp.path(), Encryption::None);
        let backend = LocalBackend::open(&temp.path().join("objects"), 1024).unwrap();
        let bindings = CopyBindings::new(
            "a".repeat(64),
            BTreeMap::from([(
                "primary".into(),
                ApprovedBackend::for_security(&backend, vec![Encryption::None]).unwrap(),
            )]),
        )
        .unwrap();
        let lease = journal.lease(&id).unwrap();
        let ready = advance(&lease, &bindings, None, 1).await.unwrap();
        assert_eq!(ready.phase(), Phase::ReadyToStage);
        assert_eq!(
            advance(&lease, &bindings, None, 2).await.unwrap().payload(),
            ready.payload()
        );
    }
    #[tokio::test]
    async fn missing_warden_and_forbidden_class_do_not_prepare_or_upload() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, id) = fixture(temp.path(), Encryption::WardenAge);
        let root = temp.path().join("objects");
        let backend = LocalBackend::open(&root, 1024).unwrap();
        let lease = journal.lease(&id).unwrap();
        for grant in [vec![Encryption::None], vec![Encryption::WardenAge]] {
            let bindings = CopyBindings::new(
                "a".repeat(64),
                BTreeMap::from([(
                    "primary".into(),
                    ApprovedBackend::for_security(&backend, grant).unwrap(),
                )]),
            )
            .unwrap();
            assert!(advance(&lease, &bindings, None, 1).await.is_err());
            assert_eq!(lease.load().unwrap().phase(), Phase::Captured);
            assert!(lease.load().unwrap().payload().is_none());
        }
        assert_eq!(
            std::fs::read_dir(root)
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
    }
}
