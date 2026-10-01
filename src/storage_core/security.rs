//! Bounded Warden subprocess composition for exact captured source versions.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use super::backend::BackendFailure;
use super::journal::{Encryption, FailureCode, Job, JobLease, Phase};
use super::reference::{validate_sha256, Fingerprint};

/// Operator-selected Warden executable and owning repository, never manifest commands.
pub struct WardenAdapter {
    executable: PathBuf,
    repo: PathBuf,
    repo_id: String,
    timeout: Duration,
    identity_home: Option<PathBuf>,
}

impl WardenAdapter {
    /// Bind an absolute existing executable/repo and a positive processing deadline.
    /// Callers must authorize these bindings and establish the repo's stable identity.
    /// Every preparation/recovery checks that identity against the leased job.
    pub fn new(
        executable: &Path,
        repo: &Path,
        repo_id: &str,
        timeout: Duration,
    ) -> Result<Self> {
        validate_sha256(repo_id)?;
        if !executable.is_absolute() || !repo.is_absolute() || timeout.is_zero() {
            bail!("absolute operator bindings and positive Warden deadline required");
        }
        let executable = executable
            .canonicalize()
            .context("Warden executable unavailable")?;
        let repo = repo.canonicalize().context("owning repo unavailable")?;
        if !executable.is_file() || !repo.is_dir() {
            bail!("invalid Warden executable or owning repo binding");
        }
        Ok(Self {
            executable,
            repo,
            repo_id: repo_id.into(),
            timeout,
            identity_home: None,
        })
    }

    /// Explicitly select an operator identity home, without changing process-global HOME.
    /// Ambient machine-key overrides are removed when this option is used.
    pub fn with_identity_home(mut self, home: &Path) -> Result<Self> {
        if !home.is_absolute() || !home.is_dir() {
            bail!("absolute existing identity home required");
        }
        self.identity_home = Some(home.canonicalize()?);
        Ok(self)
    }

    /// Encrypt a captured version, durably approve/publish it, or recover its saved output.
    /// No working-tree file, Git index, backend, key creation, or recipient override is used.
    pub async fn prepare(&self, lease: &JobLease, now: u64) -> Result<Job> {
        let mut job = lease.load()?;
        // Reject a wrong binding before reading source bytes, invoking Warden or
        // recovering approved output. Do not poison a different repo's job.
        if job.spec().repo_id != self.repo_id {
            bail!("Warden repository binding does not match the job");
        }
        if job.spec().encryption != Encryption::WardenAge
            || !matches!(job.phase(), Phase::Captured | Phase::Prepared)
            || now == 0
            || !job.retry_eligible(now)
        {
            bail!("encrypted job is not eligible for security preparation");
        }
        match lease.recover_security_output() {
            Ok(Some(mut recovered)) => {
                let mut payload = lease.payload_snapshot()?;
                if require_age_header(&mut payload).is_err() {
                    return fail(lease, &mut recovered, FailureCode::Security, now);
                }
                return Ok(recovered);
            }
            Ok(None) => {}
            Err(_) => return fail(lease, &mut job, FailureCode::Integrity, now),
        }
        let source = match lease.source_snapshot() {
            Ok(source) => source,
            Err(_) => return fail(lease, &mut job, FailureCode::Integrity, now),
        };
        let mut spool = match lease.security_spool() {
            Ok(spool) => spool,
            Err(error) => return fail(lease, &mut job, super::transfer::classify(&error), now),
        };
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .arg("storage-encrypt")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--max-bytes")
            .arg(job.spec().source.bytes().max(1).to_string())
            .current_dir(&self.repo)
            .stdin(Stdio::from(source))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(home) = &self.identity_home {
            command.env("HOME", home).env_remove("ARCANE_MACHINE_KEY");
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return fail(lease, &mut job, FailureCode::Security, now),
        };
        let result = tokio::time::timeout(self.timeout, async {
            let mut output = child.stdout.take().context("missing Warden output pipe")?;
            let identity = stream_output(&mut output, &mut spool.file, spool.capacity).await?;
            if !child.wait().await?.success() {
                bail!("Warden did not approve output");
            }
            require_age_header(&mut spool.file)?;
            Ok::<_, anyhow::Error>(identity)
        })
        .await;
        let identity = match result {
            Ok(Ok(identity)) => identity,
            outcome => {
                let _ = child.kill().await;
                let code = match outcome {
                    Err(_) => FailureCode::Transient,
                    Ok(Err(error)) if error.downcast_ref::<BackendFailure>().is_some() => {
                        super::transfer::classify(&error)
                    }
                    _ => FailureCode::Security,
                };
                return fail(lease, &mut job, code, now);
            }
        };
        // Exit success, valid protocol header and bounded output precede approval.
        if lease.source_snapshot().is_err() {
            return fail(lease, &mut job, FailureCode::Integrity, now);
        }
        lease.approve_security_output(spool, identity)
    }
}

async fn stream_output(
    input: &mut tokio::process::ChildStdout,
    output: &mut File,
    capacity: u64,
) -> Result<Fingerprint> {
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(BackendFailure::Capacity)?;
        if total > capacity {
            bail!(BackendFailure::Capacity);
        }
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    Fingerprint::new(format!("{:x}", digest.finalize()), total)
}

fn require_age_header(file: &mut File) -> Result<()> {
    let mut header = [0u8; 22];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut header)?;
    if &header != b"age-encryption.org/v1\n" {
        bail!("unsupported security output representation");
    }
    Ok(())
}

fn fail(lease: &JobLease, job: &mut Job, code: FailureCode, now: u64) -> Result<Job> {
    job.note_failure(
        code,
        (code == FailureCode::Transient).then(|| now.saturating_add(30)),
    )?;
    lease.save(job)?;
    bail!("security preparation failed: {code:?}")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::storage_core::journal::{encode_relative_path, JobSpec, Journal, Limits};
    use std::os::unix::fs::PermissionsExt;

    fn fixture(temp: &Path, script: &str, limits: Limits) -> (Journal, Job, WardenAdapter) {
        let repo = temp.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let binary = temp.join("warden-fixture");
        std::fs::write(&binary, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bytes = b"selected private source";
        let spec = JobSpec {
            repo_id: "a".repeat(64),
            path_hex: encode_relative_path(b"private.bin").unwrap(),
            source: Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64)
                .unwrap(),
            policy_sha256: "c".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into()],
            required_git_targets: vec!["github".into()],
            encryption: Encryption::WardenAge,
        };
        let journal = Journal::open(&temp.join("journal"), &spec.repo_id, limits).unwrap();
        let job = journal.create(spec).unwrap();
        journal
            .lease(job.id())
            .unwrap()
            .capture_snapshot(&mut &bytes[..])
            .unwrap();
        let adapter =
            WardenAdapter::new(&binary, &repo, &"a".repeat(64), Duration::from_secs(5)).unwrap();
        (journal, job, adapter)
    }

    // Synthetic executables test subprocess/publication mechanics only. Real
    // encryption and recipient trust are checked by the operational test below.
    const APPROVED: &str =
        "cat >/dev/null\nprintf 'age-encryption.org/v1\\napproved opaque fixture\\n'";

    #[tokio::test]
    async fn foreign_repo_binding_cannot_encrypt_or_adopt_a_prepared_job() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, job, owning_adapter) = fixture(temp.path(), APPROVED, Limits::default());
        let foreign_adapter = WardenAdapter::new(
            &temp.path().join("warden-fixture"),
            &temp.path().join("repo"),
            &"b".repeat(64),
            Duration::from_secs(5),
        )
        .unwrap();
        let lease = journal.lease(job.id()).unwrap();
        assert!(foreign_adapter.prepare(&lease, 1).await.is_err());
        let captured = lease.load().unwrap();
        assert_eq!(captured.phase(), Phase::Captured);
        assert!(captured.failure().is_none());
        assert!(captured.prepared_candidate().is_none());
        assert!(lease.payload_snapshot().is_err());
        let prepared = owning_adapter.prepare(&lease, 2).await.unwrap();
        let before = lease.load().unwrap();
        assert!(foreign_adapter.prepare(&lease, 3).await.is_err());
        let after = lease.load().unwrap();
        assert_eq!(after.phase(), Phase::Prepared);
        assert_eq!(before.payload(), after.payload());
        assert_eq!(after.payload(), prepared.payload());
        assert!(after.failure().is_none());
    }

    #[tokio::test]
    async fn success_publishes_once_and_restart_does_not_spawn_again() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, job, adapter) = fixture(temp.path(), APPROVED, Limits::default());
        let lease = journal.lease(job.id()).unwrap();
        let prepared = adapter.prepare(&lease, 1).await.unwrap();
        assert_eq!(prepared.phase(), Phase::Prepared);
        let identity = prepared.payload().unwrap().clone();
        drop(lease);
        // Invocation after preparation must not run this failing replacement.
        std::fs::write(temp.path().join("warden-fixture"), "#!/bin/sh\nexit 71\n").unwrap();
        let lease = journal.lease(job.id()).unwrap();
        assert_eq!(
            adapter.prepare(&lease, 2).await.unwrap().payload(),
            Some(&identity)
        );
        assert!(lease.source_snapshot().is_ok());
        assert!(lease.payload_snapshot().is_ok());
    }

    #[tokio::test]
    async fn failed_exit_and_plaintext_stdout_cannot_approve_a_payload() {
        for script in ["cat\n", "printf 'age-encryption.org/v1\\npartial\\n'\nprintf 'private-diagnostic-sentinel' >&2\nexit 9"] {
            let temp = tempfile::tempdir().unwrap();
            let (journal, job, adapter) = fixture(temp.path(), script, Limits::default());
            let lease = journal.lease(job.id()).unwrap();
            let error = adapter.prepare(&lease, 1).await.unwrap_err().to_string();
            assert!(!error.contains("private-diagnostic-sentinel"));
            let failed = lease.load().unwrap();
            assert_eq!(failed.phase(), Phase::Captured);
            assert_eq!(failed.failure(), Some(FailureCode::Security));
            assert!(failed.prepared_candidate().is_none());
            assert!(lease.payload_snapshot().is_err());
            assert!(lease.source_snapshot().is_ok());
        }
    }

    #[tokio::test]
    async fn deadline_kills_child_and_retry_respects_backoff() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, job, mut adapter) = fixture(temp.path(), "exec sleep 10", Limits::default());
        adapter.timeout = Duration::from_millis(100);
        let lease = journal.lease(job.id()).unwrap();
        let started = std::time::Instant::now();
        assert!(adapter.prepare(&lease, 1).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(
            lease.load().unwrap().failure(),
            Some(FailureCode::Transient)
        );
        std::fs::write(
            temp.path().join("warden-fixture"),
            format!("#!/bin/sh\n{APPROVED}\n"),
        )
        .unwrap();
        adapter.timeout = Duration::from_secs(5);
        assert!(adapter.prepare(&lease, 30).await.is_err());
        assert_eq!(
            adapter.prepare(&lease, 31).await.unwrap().phase(),
            Phase::Prepared
        );
    }

    #[tokio::test]
    async fn oversized_stdout_and_unapproved_orphan_do_not_escape_capacity() {
        let temp = tempfile::tempdir().unwrap();
        let limits = Limits {
            max_payload_bytes: 8,
            max_retained_payload_bytes: 16,
            ..Limits::default()
        };
        let (journal, job, adapter) = fixture(temp.path(), APPROVED, limits);
        let lease = journal.lease(job.id()).unwrap();
        assert!(adapter.prepare(&lease, 1).await.is_err());
        assert_eq!(lease.load().unwrap().failure(), Some(FailureCode::Capacity));
        assert!(lease.load().unwrap().prepared_candidate().is_none());
        assert!(lease.source_snapshot().is_ok());
        // A process may disappear before selecting an approved candidate. A
        // private unapproved spool is reset only after the source reverifies.
        let temp2 = tempfile::tempdir().unwrap();
        let (journal, job, adapter) = fixture(temp2.path(), APPROVED, Limits::default());
        let lease = journal.lease(job.id()).unwrap();
        let mut spool = lease.security_spool().unwrap();
        spool
            .file
            .write_all(b"unfinished unapproved transform")
            .unwrap();
        spool.file.sync_all().unwrap();
        drop(spool);
        assert_eq!(
            adapter.prepare(&lease, 1).await.unwrap().phase(),
            Phase::Prepared
        );
    }

    #[tokio::test]
    async fn approval_and_publication_survive_process_death_without_reencrypting() {
        for phase in ["after-security-approval", "after-security-publish"] {
            let temp = tempfile::tempdir().unwrap();
            let (journal, job, adapter) = fixture(temp.path(), APPROVED, Limits::default());
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "storage_core::security::tests::security_crash_child",
                    "--ignored",
                ])
                .env("DRACON_STORAGE_CRASH_ROOT", temp.path())
                .env("DRACON_STORAGE_CRASH_JOB", job.id())
                .env("DRACON_STORAGE_CRASH_POINT", phase)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(73));
            std::fs::write(temp.path().join("warden-fixture"), "#!/bin/sh\nexit 71\n").unwrap();
            let lease = journal.lease(job.id()).unwrap();
            let expected = lease.load().unwrap().prepared_candidate().unwrap().clone();
            assert_eq!(
                adapter.prepare(&lease, 2).await.unwrap().payload(),
                Some(&expected)
            );
            assert!(lease.payload_snapshot().is_ok());
        }
    }

    #[tokio::test]
    #[ignore = "subprocess helper, invoked by security crash recovery test"]
    async fn security_crash_child() {
        let root = PathBuf::from(std::env::var_os("DRACON_STORAGE_CRASH_ROOT").unwrap());
        let id = std::env::var("DRACON_STORAGE_CRASH_JOB").unwrap();
        let journal =
            Journal::open(&root.join("journal"), &"a".repeat(64), Limits::default()).unwrap();
        let adapter = WardenAdapter::new(
            &root.join("warden-fixture"),
            &root.join("repo"),
            Duration::from_secs(5),
        )
        .unwrap();
        adapter
            .prepare(&journal.lease(&id).unwrap(), 1)
            .await
            .unwrap();
        panic!("security crash injection did not fire");
    }

    #[tokio::test]
    async fn completed_unapproved_output_after_crash_is_not_adopted() {
        let temp = tempfile::tempdir().unwrap();
        let (journal, job, adapter) = fixture(temp.path(), APPROVED, Limits::default());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage_core::security::tests::security_crash_child",
                "--ignored",
            ])
            .env("DRACON_STORAGE_CRASH_ROOT", temp.path())
            .env("DRACON_STORAGE_CRASH_JOB", job.id())
            .env("DRACON_STORAGE_CRASH_POINT", "before-security-approval")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(73));
        let lease = journal.lease(job.id()).unwrap();
        assert!(lease.load().unwrap().prepared_candidate().is_none());
        std::fs::write(temp.path().join("warden-fixture"), "#!/bin/sh\nexit 71\n").unwrap();
        assert!(adapter.prepare(&lease, 2).await.is_err());
        assert_eq!(lease.load().unwrap().phase(), Phase::Captured);
        assert!(lease.load().unwrap().payload().is_none());
        assert!(lease.source_snapshot().is_ok());
    }

    #[tokio::test]
    async fn bad_recovery_representation_records_failure_on_the_latest_revision() {
        use std::os::unix::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let (journal, job, adapter) = fixture(temp.path(), "exit 71", Limits::default());
        let lease = journal.lease(job.id()).unwrap();
        let bytes = b"incorrect approved representation fixture";
        let identity =
            Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
        let mut captured = lease.load().unwrap();
        captured.select_prepared_payload(identity).unwrap();
        lease.save(&mut captured).unwrap();
        let path = temp
            .path()
            .join("journal")
            .join("a".repeat(64))
            .join(format!("{}.security-output", job.id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(adapter.prepare(&lease, 2).await.is_err());
        assert_eq!(lease.load().unwrap().failure(), Some(FailureCode::Security));
        assert!(lease.source_snapshot().is_ok());
        assert!(lease.payload_snapshot().is_ok());
    }

    #[tokio::test]
    #[ignore = "operational check: requires age-keygen and DRACON_STORAGE_TEST_WARDEN source-build binary"]
    async fn real_warden_large_payload_copies_and_cold_restore_use_fixture_keys() {
        use crate::storage_core::backend::{ImmutableBackend, LocalBackend};
        use crate::storage_core::transfer::transfer_copies;
        use std::collections::BTreeMap;
        let binary = PathBuf::from(
            std::env::var_os("DRACON_STORAGE_TEST_WARDEN")
                .expect("source-built Warden binary required"),
        );
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(home.join(".dracon/keys")).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let keys = home.join(".dracon/keys/identity.age");
        assert!(std::process::Command::new("age-keygen")
            .arg("-o")
            .arg(&keys)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600)).unwrap();
        let chunk = [0x39u8; 64 * 1024];
        let bytes = 101 * 1024 * 1024u64;
        let mut hash = Sha256::new();
        for _ in 0..(bytes / chunk.len() as u64) {
            hash.update(chunk);
        }
        let source = Fingerprint::new(format!("{:x}", hash.finalize()), bytes).unwrap();
        let spec = JobSpec {
            repo_id: "a".repeat(64),
            path_hex: encode_relative_path(b"large-private.bin").unwrap(),
            source: source.clone(),
            policy_sha256: "c".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into(), "recovery".into()],
            required_git_targets: vec!["github".into()],
            encryption: Encryption::WardenAge,
        };
        let journal = Journal::open(
            &temp.path().join("journal"),
            &spec.repo_id,
            Limits::default(),
        )
        .unwrap();
        let job = journal.create(spec).unwrap();
        struct Synthetic {
            remaining: u64,
            largest_read: usize,
        }
        impl Read for Synthetic {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                self.largest_read = self.largest_read.max(output.len());
                let count = output.len().min(self.remaining as usize);
                output[..count].fill(0x39);
                self.remaining -= count as u64;
                Ok(count)
            }
        }
        let lease = journal.lease(job.id()).unwrap();
        let mut synthetic = Synthetic {
            remaining: bytes,
            largest_read: 0,
        };
        lease.capture_snapshot(&mut synthetic).unwrap();
        assert!(synthetic.largest_read <= 64 * 1024);
        let adapter = WardenAdapter::new(&binary, &repo, Duration::from_secs(180))
            .unwrap()
            .with_identity_home(&home)
            .unwrap();
        let prepared = adapter.prepare(&lease, 1).await.unwrap();
        let identity = prepared.payload().unwrap().clone();
        assert_ne!(identity.sha256(), source.sha256());
        let primary_root = temp.path().join("primary");
        let recovery_root = temp.path().join("recovery");
        let primary = LocalBackend::open(&primary_root, bytes * 2).unwrap();
        let recovery = LocalBackend::open(&recovery_root, bytes * 2).unwrap();
        let backends = BTreeMap::from([
            ("primary".into(), &primary as &dyn ImmutableBackend),
            ("recovery".into(), &recovery as &dyn ImmutableBackend),
        ]);
        assert_eq!(
            transfer_copies(&lease, &backends, 2).unwrap().phase(),
            Phase::ReadyToStage
        );
        drop(lease);
        drop(journal);
        drop(backends);
        drop(primary);
        drop(recovery);
        // Cold restoration opens only the recovery backend and independent
        // fixture keys, with no local capture/preparation records required.
        let cold = LocalBackend::open_existing(&recovery_root, bytes * 2).unwrap();
        let ciphertext = temp.path().join("cold-ciphertext");
        let mut options = std::fs::OpenOptions::new();
        use std::os::unix::fs::OpenOptionsExt;
        let mut saved = options
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&ciphertext)
            .unwrap();
        cold.get_verified(&identity, &mut saved).unwrap();
        saved.seek(SeekFrom::Start(0)).unwrap();
        let plaintext = temp.path().join("restored-source");
        let output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&plaintext)
            .unwrap();
        assert!(std::process::Command::new(&binary)
            .arg("storage-decrypt")
            .arg("--repo")
            .arg(&repo)
            .arg("--max-bytes")
            .arg(bytes.to_string())
            .env("HOME", &home)
            .env_remove("ARCANE_MACHINE_KEY")
            .stdin(Stdio::from(saved))
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
        let mut restored = File::open(&plaintext).unwrap();
        let mut actual_hash = Sha256::new();
        let mut actual_bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = restored.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            actual_hash.update(&buffer[..count]);
            actual_bytes += count as u64;
        }
        assert_eq!(actual_bytes, bytes);
        assert_eq!(format!("{:x}", actual_hash.finalize()), source.sha256());
        assert!(!repo.join(".gitattributes").exists());
        assert!(!repo.join(".arcane").exists());
    }
}
