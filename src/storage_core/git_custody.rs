//! Bounded preservation of an exact indexed Git blob for enrollment review.
//!
//! A successful copy is custody evidence, not enrollment, upload or staging
//! authority. The caller owns the private destination and the repository binding.

use anyhow::{ensure, Context, Result};
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
            && oid
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid Git custody object ID"
    );
    ensure!(
        max_bytes > 0 && !timeout.is_zero(),
        "positive custody bounds required"
    );
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
        command(
            repo,
            git,
            &[
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
        ),
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
            bytes = bytes
                .checked_add(count as u64)
                .context("custody byte overflow")?;
            ensure!(
                bytes <= expected,
                "Git custody source exceeded its declared size"
            );
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
    ensure!(
        actual == format!("{oid}\n").as_bytes(),
        "Git custody object ID mismatch"
    );
    destination.rewind()?;
    Ok(fingerprint)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command as StdCommand;

    fn git() -> PathBuf {
        let output = StdCommand::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        PathBuf::from(std::str::from_utf8(&output.stdout).unwrap().trim())
            .canonicalize()
            .unwrap()
    }

    fn fixture(bytes: &[u8]) -> (tempfile::TempDir, String) {
        let root = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        let oid = repo.blob(bytes).unwrap().to_string();
        (root, oid)
    }

    #[tokio::test]
    async fn streams_and_rewinds_a_blob_larger_than_the_io_buffer() {
        let bytes: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let (repo, oid) = fixture(&bytes);
        let mut destination = tempfile::tempfile().unwrap();
        let fingerprint = preserve_git_blob(
            repo.path(),
            &git(),
            &oid,
            &mut destination,
            bytes.len() as u64,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(fingerprint.bytes(), bytes.len() as u64);
        assert_eq!(
            fingerprint.sha256(),
            format!("{:x}", Sha256::digest(&bytes))
        );
        let mut restored = Vec::new();
        destination.read_to_end(&mut restored).unwrap();
        assert_eq!(restored, bytes);
    }

    #[tokio::test]
    async fn packed_indexed_blob_is_preserved_without_materializing_the_worktree() {
        let bytes = b"an indexed art source whose working file is absent";
        let (root, oid) = fixture(bytes);
        let repo = git2::Repository::open(root.path()).unwrap();
        let blob = git2::Oid::from_str(&oid).unwrap();
        let mut builder = repo.treebuilder(None).unwrap();
        builder.insert("art.png", blob, 0o100644).unwrap();
        let tree = repo.find_tree(builder.write().unwrap()).unwrap();
        let signature = git2::Signature::now("Custody Fixture", "custody@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        for args in [vec!["repack", "-ad"], vec!["prune-packed"]] {
            let status = StdCommand::new(git())
                .current_dir(root.path())
                .args(["-c", "core.hooksPath=/dev/null"])
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }
        assert!(!root
            .path()
            .join(".git/objects")
            .join(&oid[..2])
            .join(&oid[2..])
            .exists());
        let mut destination = tempfile::tempfile().unwrap();
        preserve_git_blob(
            root.path(),
            &git(),
            &oid,
            &mut destination,
            1024,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        let mut preserved = Vec::new();
        destination.read_to_end(&mut preserved).unwrap();
        assert_eq!(preserved, bytes);
        assert!(!root.path().join("art.png").exists());
    }

    #[tokio::test]
    async fn capacity_refusal_happens_before_copying_payload_bytes() {
        let (repo, oid) = fixture(b"larger than the selected custody capacity");
        let mut destination = tempfile::tempfile().unwrap();
        assert!(preserve_git_blob(
            repo.path(),
            &git(),
            &oid,
            &mut destination,
            3,
            Duration::from_secs(5)
        )
        .await
        .is_err());
        assert_eq!(destination.metadata().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_tree_cannot_be_preserved_as_an_asset_blob() {
        let (root, _) = fixture(b"blob");
        let repo = git2::Repository::open(root.path()).unwrap();
        let oid = repo.treebuilder(None).unwrap().write().unwrap().to_string();
        let mut destination = tempfile::tempfile().unwrap();
        assert!(preserve_git_blob(
            root.path(),
            &git(),
            &oid,
            &mut destination,
            1024,
            Duration::from_secs(5)
        )
        .await
        .is_err());
        assert_eq!(destination.metadata().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn existing_custody_bytes_are_never_truncated_or_overwritten() {
        let (repo, oid) = fixture(b"new source");
        let mut destination = tempfile::tempfile().unwrap();
        destination.write_all(b"retained original").unwrap();
        destination.rewind().unwrap();
        assert!(preserve_git_blob(
            repo.path(),
            &git(),
            &oid,
            &mut destination,
            1024,
            Duration::from_secs(5)
        )
        .await
        .is_err());
        let mut retained = String::new();
        destination.read_to_string(&mut retained).unwrap();
        assert_eq!(retained, "retained original");
    }

    #[tokio::test]
    async fn nonprivate_destinations_and_zero_deadlines_are_refused() {
        let (repo, oid) = fixture(b"source");
        let mut destination = tempfile::tempfile().unwrap();
        destination
            .set_permissions(std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(preserve_git_blob(
            repo.path(),
            &git(),
            &oid,
            &mut destination,
            1024,
            Duration::from_secs(5)
        )
        .await
        .is_err());
        destination
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .unwrap();
        assert!(preserve_git_blob(
            repo.path(),
            &git(),
            &oid,
            &mut destination,
            1024,
            Duration::ZERO
        )
        .await
        .is_err());
        assert_eq!(destination.metadata().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_stalled_metadata_child_is_cancelled_at_the_deadline() {
        let (root, oid) = fixture(b"source");
        let executable = root.path().join("owned-git-fixture");
        std::fs::write(
            &executable,
            "#!/bin/sh\necho $$ > custody-child.pid\nexec sleep 30\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut destination = tempfile::tempfile().unwrap();
        assert!(preserve_git_blob(
            root.path(),
            &executable,
            &oid,
            &mut destination,
            1024,
            Duration::from_secs(1)
        )
        .await
        .is_err());
        let pid: i32 = std::fs::read_to_string(root.path().join("custody-child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "the owned child must be reaped"
        );
        assert_eq!(destination.metadata().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn malformed_ids_are_refused_before_any_git_process_runs() {
        let (root, _) = fixture(b"source");
        let executable = root.path().join("owned-git-fixture");
        std::fs::write(&executable, "#!/bin/sh\ntouch must-not-run\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut destination = tempfile::tempfile().unwrap();
        for oid in ["--batch", "../HEAD", "ABCD", &"f".repeat(41)] {
            assert!(preserve_git_blob(
                root.path(),
                &executable,
                oid,
                &mut destination,
                1024,
                Duration::from_secs(5)
            )
            .await
            .is_err());
        }
        assert!(!root.path().join("must-not-run").exists());
    }
}
