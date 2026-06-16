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
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = std::env::temp_dir().join(format!(
        "dracon-git-askpass-{}-{}.sh",
        std::process::id(),
        nano
    ));
    let escaped = token.replace('\'', "'\"'\"'");
    let script = format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", escaped);
    tokio::fs::write(&tmp_path, &script)
        .await
        .with_context(|| {
            format!(
                "failed to write GIT_ASKPASS script to {}",
                tmp_path.display()
            )
        })?;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = tokio::fs::metadata(&tmp_path).await?.permissions();
    perms.set_mode(0o700);
    tokio::fs::set_permissions(&tmp_path, perms).await?;
    Ok(tmp_path)
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
}
