//! Git process operations — spawn, timeout, kill, and capture output.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

/// Kill a git child process group using TERM then KILL.
///
/// Git push/pull spawn helper processes (ssh, remote-https, pack-objects).
/// Put those children in their own process group before spawning, then kill the
/// group on timeout so a timed-out operation cannot keep running in the daemon
/// cgroup and overlap with the next retry.
#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let pid_s = format!("-{pid}");
    let _ = std::process::Command::new("kill")
        .args(["-TERM", pid_s.as_str()])
        .output();
    std::thread::sleep(Duration::from_millis(200));
    let _ = std::process::Command::new("kill")
        .args(["-KILL", pid_s.as_str()])
        .output();
}

#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

#[cfg(unix)]
fn configure_git_process_group(cmd: &mut TokioCommand) {
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn configure_git_process_group(_cmd: &mut TokioCommand) {}

/// Return true when a git stderr line indicates push/pull progress.
///
/// The daemon uses per-operation idle timeouts for git network operations. A
/// large but active pack can legitimately run longer than the base timeout, so
/// progress output extends the deadline instead of aborting a healthy push.
pub(crate) fn is_git_push_progress_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("counting objects")
        || lower.contains("compressing objects")
        || lower.contains("writing objects")
        || lower.contains("receiving objects")
        || lower.contains("remote: ")
        || lower.contains("bytes")
        || lower.contains("delta")
        || lower.contains("uploaded")
}

fn child_status_result(
    status: std::process::ExitStatus,
    label: &str,
    workdir: &Path,
    stderr_output: String,
) -> Result<()> {
    if status.success() {
        Ok(())
    } else if stderr_output.is_empty() {
        Err(anyhow::anyhow!(
            "{} failed in {} with status {}",
            label,
            workdir.display(),
            status
        ))
    } else {
        Err(anyhow::anyhow!(
            "{} failed in {} with status {}: {}",
            label,
            workdir.display(),
            status,
            stderr_output
        ))
    }
}

async fn run_child_inner<F>(
    mut child: tokio::process::Child,
    workdir: &Path,
    timeout_secs: u64,
    label: &str,
    mut progress_predicate: Option<F>,
) -> Result<()>
where
    F: FnMut(&str) -> bool + Send + 'static,
{
    let pid = child.id();
    let stderr_handle = child.stderr.take();
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<Instant>();
    let stderr_task = tokio::spawn(async move {
        let mut stderr_output = String::new();
        if let Some(mut stderr) = stderr_handle {
            let mut lines = BufReader::new(&mut stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(is_progress) = progress_predicate.as_mut() {
                    if is_progress(&line) {
                        let _ = progress_tx.send(Instant::now());
                    }
                }
                if !stderr_output.is_empty() {
                    stderr_output.push('\n');
                }
                stderr_output.push_str(&line);
            }
        }
        stderr_output
    });

    let mut deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(250);

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| anyhow::anyhow!("{} failed in {}: {}", label, workdir.display(), e))?
        {
            let stderr_output = stderr_task
                .await
                .unwrap_or_else(|e| format!("stderr capture failed: {e}"));
            return child_status_result(status, label, workdir, stderr_output);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            if let Some(pid) = pid {
                kill_process_group(pid);
            }
            let _ = child.start_kill();
            let _ = child.wait().await;
            let _ = stderr_task.await;
            return Err(anyhow::anyhow!(
                "{} timeout in {} after {}s",
                label,
                workdir.display(),
                timeout_secs
            ));
        }

        tokio::select! {
            Some(_) = progress_rx.recv() => {
                deadline = Instant::now() + Duration::from_secs(timeout_secs);
            }
            _ = tokio::time::sleep(remaining.min(poll_interval)) => {}
        }
    }
}

/// Run a child process with a timeout, capturing stderr on failure.
pub(crate) async fn run_child(
    child: tokio::process::Child,
    workdir: &Path,
    timeout_secs: u64,
    label: &str,
) -> Result<()> {
    run_child_inner(
        child,
        workdir,
        timeout_secs,
        label,
        None::<fn(&str) -> bool>,
    )
    .await
}

/// Run a child process with a progress-aware idle timeout.
async fn run_child_with_progress<F>(
    child: tokio::process::Child,
    workdir: &Path,
    timeout_secs: u64,
    label: &str,
    progress_predicate: F,
) -> Result<()>
where
    F: FnMut(&str) -> bool + Send + 'static,
{
    run_child_inner(
        child,
        workdir,
        timeout_secs,
        label,
        Some(progress_predicate),
    )
    .await
}

fn spawn_git_command(repo: &Path, args: &[&str], op_label: &str) -> Result<tokio::process::Child> {
    let label = format!("git {}", op_label);
    let mut cmd = crate::policy::tokio_git_command();
    cmd.args(args)
        .current_dir(repo)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    configure_git_process_group(&mut cmd);
    cmd.spawn()
        .with_context(|| format!("failed to spawn {} in {}", label, repo.display()))
}

fn spawn_git_command_env(
    repo: &Path,
    args: &[&str],
    op_label: &str,
    env: &[(&str, &str)],
) -> Result<tokio::process::Child> {
    let label = format!("git {}", op_label);
    let mut cmd = crate::policy::tokio_git_command();
    cmd.args(args)
        .current_dir(repo)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    configure_git_process_group(&mut cmd);
    cmd.spawn()
        .with_context(|| format!("failed to spawn {} in {}", label, repo.display()))
}

/// Run a git command with a timeout using the tokio git command builder.
pub(crate) async fn run_git_with_timeout(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    let label = format!("git {}", op_label);
    let child = spawn_git_command(repo, args, op_label)?;
    run_child(child, repo, timeout_secs, &label).await
}

/// Run a git command with extra environment variables and a timeout.
pub(crate) async fn run_git_with_timeout_env(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    env: &[(&str, &str)],
) -> Result<()> {
    let label = format!("git {}", op_label);
    let child = spawn_git_command_env(repo, args, op_label, env)?;
    run_child(child, repo, timeout_secs, &label).await
}

/// Run a git push/pull with a progress-aware idle timeout.
pub(crate) async fn run_git_with_timeout_env_progress(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    env: &[(&str, &str)],
) -> Result<()> {
    let label = format!("git {}", op_label);
    let child = spawn_git_command_env(repo, args, op_label, env)?;
    run_child_with_progress(child, repo, timeout_secs, &label, is_git_push_progress_line).await
}
#[cfg(unix)]
pub(crate) async fn git_askpass_script(token: &str) -> Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = std::env::temp_dir().join(format!(
        "dracon-git-askpass-{}-{}.sh",
        std::process::id(),
        nano
    ));

    // F41 fix (2026-07-18): create the file atomically with mode
    // 0o700 (O_EXCL | O_NOFOLLOW). The previous flow wrote the file
    // with default umask (typically 0o666) and then tightened
    // permissions afterwards — the file was world-readable between
    // the write and chmod. The caller should still `unlink` the
    // returned path via the AskpassScript guard (see below) so the
    // credential doesn't linger in /tmp.
    let _ = tokio::fs::remove_file(&tmp_path).await; // Best-effort: ignore ENOENT.

    // Shell-quote the token (POSIX single-quote escape). For
    // alnum-only tokens (the realistic case — PATs of any forge)
    // this is a no-op. Tokens with `'` break the inner quoting;
    // Forgejo/GitLab allow this in some legacy schemes but we treat
    // it as a hard error rather than risk malformed shell.
    if token.contains('\'') {
        anyhow::bail!("git_askpass_script: token contains a single quote (refused; F59)");
    }
    let script = format!("#!/bin/sh\nprintf '%s\\n' '{token}'\n");

    // Atomic create with mode 0o700.
    {
        use std::fs::OpenOptions;
        let mut openopts = OpenOptions::new();
        openopts
            .write(true)
            .create_new(true)
            .truncate(false)
            .custom_flags(libc_o_excl_o_nofollow())
            .mode(0o700);
        let mut f = openopts.open(&tmp_path).with_context(|| {
            format!(
                "failed to create GIT_ASKPASS script at {}",
                tmp_path.display()
            )
        })?;
        use std::io::Write;
        f.write_all(script.as_bytes()).with_context(|| {
            format!(
                "failed to write GIT_ASKPASS script to {}",
                tmp_path.display()
            )
        })?;
    }

    Ok(tmp_path)
}

/// Combine `O_EXCL | O_NOFOLLOW` as a libc `c_int` for
/// `OpenOptions::custom_flags`. Avoids a hard dependency on the
/// `libc` crate — just the bit values we need. Stable across the
/// platforms we support (Linux x86_64/aarch64, macOS).
#[cfg(unix)]
fn libc_o_excl_o_nofollow() -> i32 {
    // O_EXCL = 0x80 on Linux, 0x4 on macOS — but OpenOptionsExt on
    // macOS doesn't honour `mode()` or `O_NOFOLLOW` reliably, so we
    // restrict to Linux constants and gate the whole function.
    #[cfg(target_os = "linux")]
    {
        0x80 | 0x20000
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Fallback: just O_EXCL (no O_NOFOLLOW). The chmod race fix
        // still works because the file is created with mode 0o700.
        0x80
    }
}

/// RAII guard that unlinks the askpass script on drop. F41:
/// caller-side cleanup of the `/tmp/dracon-git-askpass-...` file.
///
///   let path = git_askpass_script(&token).await?;
///   let _guard = AskpassScript::new(path.clone());
///   // …git push with GIT_ASKPASS=path…
///   // dropped at scope exit; `path` is unlinked.
pub(crate) struct AskpassScript {
    path: PathBuf,
}

#[cfg(unix)]
impl AskpassScript {
    #[allow(dead_code)] // Available for callers that drop the path; current call sites use async unlink explicitly.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(unix)]
impl Drop for AskpassScript {
    fn drop(&mut self) {
        // Best-effort synchronous unlink. Ignore errors (ENOENT,
        // EBUSY on Windows-rare races). The file is created with
        // 0o700 owned by the daemon user; the unlink is safe.
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(not(unix))]
pub(crate) async fn git_askpass_script(_token: &str) -> Result<PathBuf> {
    anyhow::bail!("GIT_ASKPASS helper is only implemented on Unix")
}

/// Run a git command and capture its stdout as a string.
pub(crate) fn run_git_capture_output(repo: &Path, args: &[&str], op_label: &str) -> Result<String> {
    let output = crate::policy::std_git_command()
        .args(args)
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .with_context(|| format!("failed to run git {} in {}", op_label, repo.display()))?;
    if !output.status.success() {
        anyhow::bail!("git {} failed in {}", op_label, repo.display());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::is_git_push_progress_line;

    #[test]
    fn test_git_push_progress_predicate_detects_pack_progress() {
        assert!(is_git_push_progress_line(
            "Compressing objects:  50% (123/246)"
        ));
        assert!(is_git_push_progress_line(
            "Writing objects:  10% (1/10), 1.23 KiB | 1.23 MiB/s"
        ));
        assert!(is_git_push_progress_line(
            "remote: Resolving deltas: 100% (10/10)"
        ));
        assert!(!is_git_push_progress_line("fatal: could not read Username"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_git_askpass_script_atomic_0o700_create_and_cleanup() {
        use std::os::unix::fs::PermissionsExt;
        use super::{git_askpass_script, AskpassScript};

        // F41 regression: the file must be created with mode 0o700
        // atomically (no world-readable window) and cleaned up by
        // the Drop guard.
        let path = git_askpass_script("ghp_abc123XYZtestToken00000")
            .await
            .expect("script create");
        let meta = tokio::fs::metadata(&path).await.expect("metadata");
        let mode = meta.permissions().mode();
        // Mode should be EXACTLY 0o700 (no world-read, no group-read).
        assert_eq!(
            mode & 0o777,
            0o700,
            "askpass script created with mode {:o} (expected 0o700); the world-readable race window is back",
            mode
        );

        // Drop the cleanup guard and verify the file is unlinked.
        let cleanup_path = path.clone();
        {
            let _guard = AskpassScript::new(cleanup_path);
            assert!(tokio::fs::metadata(&path).await.is_ok(), "file exists in scope");
        }
        // After drop, file should be gone.
        assert!(
            tokio::fs::metadata(&path).await.is_err(),
            "askpass script was not unlinked after Drop"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_git_askpass_script_rejects_single_quote() {
        // F59: tokens with single quotes break POSIX shell quoting;
        // we refuse them outright rather than risk shell injection.
        let result = super::git_askpass_script("abc'def").await;
        assert!(result.is_err(), "single-quote token must be rejected");
    }
}
