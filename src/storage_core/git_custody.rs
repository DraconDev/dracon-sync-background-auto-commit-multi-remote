//! Bounded preservation of an exact indexed Git blob for enrollment review.
//!
//! A successful copy is custody evidence, not enrollment, upload or staging
//! authority. The caller owns the private destination and the repository binding.

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::time::Instant;

use super::reference::Fingerprint;

fn command(repo: &Path, executable: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(executable);
    // Repository discovery, command injection and object replacement must not
    // redirect the explicit repository or manufacture a different stored blob.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(repo)
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn settle(child: &mut Child) {
    // Only our read-only child is cancelled. Tokio retains reaping ownership
    // if the kernel cannot finish shutdown within this short cleanup grace.
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
}

async fn small_output(
    mut command: Command,
    input: Option<&[u8]>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    ensure!(Instant::now() < deadline, "Git custody deadline expired");
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().context("cannot start Git custody probe")?;
    let result = tokio::time::timeout_at(deadline, async {
        if let Some(input) = input {
            let mut stdin = child.stdin.take().context("missing Git custody stdin")?;
            stdin.write_all(input).await?;
            stdin.shutdown().await?;
        }
        let mut stdout = child.stdout.take().context("missing Git custody stdout")?;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 128];
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            ensure!(bytes.len() + count <= 512, "oversized Git custody metadata");
            bytes.extend_from_slice(&buffer[..count]);
        }
        ensure!(child.wait().await?.success(), "Git custody probe failed");
        Ok::<_, anyhow::Error>(bytes)
    })
    .await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        error => {
            settle(&mut child).await;
            error.context("Git custody deadline expired")?
        }
    }
}

/// Copy one exact Git blob into an empty private regular file, without filters,
/// refs/index changes, lazy fetch, or unbounded stdout buffering. Verify both
/// its Git object ID and its independent SHA-256 before returning a rewound,
/// synchronized destination. Failed partial copies grant no publication rights.
///
/// `repo` and `git` are explicit operator-selected absolute paths. The caller
/// must bind the owning repository and retain the resulting private artifact.
pub async fn preserve_git_blob(
    repo: &Path,
    git: &Path,
    oid: &str,
    destination: &mut File,
    max_bytes: u64,
    timeout: Duration,
) -> Result<Fingerprint> {
    ensure!(
        repo.is_absolute() && repo.is_dir() && git.is_absolute() && git.is_file(),
        "absolute Git custody bindings required"
    );
    ensure!(
        matches!(oid.len(), 40 | 64)
            && oid.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid Git custody object ID"
    );
    ensure!(max_bytes > 0 && !timeout.is_zero(), "positive custody bounds required");
    let metadata = destination.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() == 0 && destination.stream_position()? == 0,
        "Git custody destination must be an empty regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.nlink() == 1
                && metadata.mode() & 0o077 == 0,
            "Git custody destination must be private and singly owned"
        );
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .context("invalid Git custody deadline")?;
    let input = format!("{oid}\n");
    let header = small_output(
        command(repo, git, &["cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"]),
        Some(input.as_bytes()),
        deadline,
    )
    .await?;
    let header = std::str::from_utf8(&header).context("invalid Git custody metadata")?;
    let expected_prefix = format!("{oid} blob ");
    let decimal = header
        .strip_prefix(&expected_prefix)
        .and_then(|s| s.strip_suffix('\n'))
        .context("Git custody source is not an inspectable blob")?;
    ensure!(
        !decimal.is_empty()
            && decimal.bytes().all(|b| b.is_ascii_digit())
            && (decimal == "0" || !decimal.starts_with('0')),
        "invalid Git custody byte count"
    );
    let expected: u64 = decimal.parse().context("invalid Git custody byte count")?;
    ensure!(expected <= max_bytes, "Git custody capacity exceeded");
    ensure!(Instant::now() < deadline, "Git custody deadline expired");
    let mut child = command(repo, git, &["cat-file", "blob", oid])
        .spawn()
        .context("cannot start Git custody copy")?;
    let result = tokio::time::timeout_at(deadline, async {
        let mut stdout = child.stdout.take().context("missing Git custody stream")?;
        let mut buffer = [0u8; 64 * 1024];
        let mut bytes = 0u64;
        let mut sha = Sha256::new();
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            bytes = bytes.checked_add(count as u64).context("custody byte overflow")?;
            ensure!(bytes <= expected, "Git custody source exceeded its declared size");
            destination.write_all(&buffer[..count])?;
            sha.update(&buffer[..count]);
        }
        ensure!(child.wait().await?.success(), "Git custody copy failed");
        ensure!(bytes == expected, "Git custody source was truncated");
        Ok::<_, anyhow::Error>(Fingerprint::new(format!("{:x}", sha.finalize()), bytes)?)
    })
    .await;
    let fingerprint = match result {
        Ok(Ok(fingerprint)) => fingerprint,
        error => {
            settle(&mut child).await;
            return error.context("Git custody deadline expired")?;
        }
    };
    destination.sync_all()?;
    destination.rewind()?;
    // hash-object without -w performs no object database mutation. This also
    // verifies packed/loose content against the selected OID before enrollment.
    let mut hash = command(repo, git, &["hash-object", "--stdin"]);
    hash.stdin(Stdio::from(destination.try_clone()?));
    let actual = small_output(hash, None, deadline).await?;
    ensure!(actual == format!("{oid}\n").as_bytes(), "Git custody object ID mismatch");
    destination.rewind()?;
    Ok(fingerprint)
}
