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
use super::journal::{Encryption, FailureCode, Job, JobLease};
use super::reference::Fingerprint;

/// Operator-selected Warden executable and owning repository, never manifest commands.
pub struct WardenAdapter {
    executable: PathBuf,
    repo: PathBuf,
    timeout: Duration,
    identity_home: Option<PathBuf>,
}

impl WardenAdapter {
    /// Bind an absolute existing executable/repo and a positive processing deadline.
    /// Callers must authorize these bindings and match the repo to the job's identity.
    pub fn new(executable: &Path, repo: &Path, timeout: Duration) -> Result<Self> {
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
        if job.spec().encryption != Encryption::WardenAge || now == 0 || !job.retry_eligible(now) {
            bail!("encrypted job is not eligible for security preparation");
        }
        match lease.recover_security_output() {
            Ok(Some(recovered)) => return Ok(recovered),
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
