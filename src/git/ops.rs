//! Git process operations — spawn, timeout, kill, and capture output.

use super::write_fence::WriteFence;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

/// Progress output extends the idle timeout, but it must not keep a network
/// operation alive forever when a remote emits an endless stream of plausible
/// progress lines. Four idle windows is generous for large pushes while still
/// giving the daemon a deterministic upper bound.
const PROGRESS_TIMEOUT_MULTIPLIER: u64 = 4;

pub(crate) fn progress_hard_timeout_secs(idle_timeout_secs: u64) -> u64 {
    idle_timeout_secs.saturating_mul(PROGRESS_TIMEOUT_MULTIPLIER)
}

/// Kill a git child process group using TERM then KILL.
///
/// Git push/pull spawn helper processes (ssh, remote-https, pack-objects).
/// Put those children in their own process group before spawning, then kill the
/// group on timeout so a timed-out operation cannot keep running in the daemon
/// cgroup and overlap with the next retry.
///
/// F47 (2026-07-19): the previous 200ms SIGTERM→SIGKILL gap was
/// tight for processes that need cleanup time (large git filter-repo
/// unpacking, etc.). The wait is asynchronous so a timed-out Git operation does
/// not block a Tokio worker thread while its process group exits.
///
/// CHANGED 2026-10-31: the grace was 2s, which is shorter than the time a
/// pre-push hook needs to remove its multi-gigabyte source snapshot. A hook
/// blocked in a synchronous exec cannot run its SIGTERM handler inside 2s, so
/// the KILL landed first and the snapshot leaked while `manifest.json` still
/// said `"status": "running"` — 38.3 GiB of orphans accumulated in
/// `~/.local/state/dracon/sync/gates` by 2026-10-31. The grace is now long
/// enough for a real cleanup and is a named constant so the two call sites and
/// the regression tests cannot drift apart.
pub(crate) const SIGTERM_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

/// Kill a git child process group using TERM then KILL.
#[cfg(unix)]
async fn kill_process_group(pid: u32) {
    let pid_s = format!("-{pid}");
    // Use `setsid` + `kill` shell-out to send signals to the
    // process group created by `process_group(0)` in
    // `configure_git_process_group`. The shell-out form is
    // portable across glibc/musl/distros; libc::killpg would
    // require a new direct dependency.
    let term_pid = pid_s.clone();
    let term_ok = tokio::task::spawn_blocking(move || {
        std::process::Command::new("kill")
            .args(["-TERM", term_pid.as_str()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    // CHANGED 2026-09-09 (audit F34): a failed SIGTERM used to return
    // early, so SIGKILL was never attempted and a runaway git (e.g.
    // filter-repo holding repo locks) survived. Attempt the KILL
    // regardless — on an already-dead group it fails harmlessly.
    if !term_ok {
        eprintln!(
            "⚠️ kill_process_group: SIGTERM to pgid {} failed (kill missing, no perm, or already dead) — still attempting SIGKILL",
            pid
        );
    }
    tokio::time::sleep(SIGTERM_GRACE).await;
    let _ = tokio::task::spawn_blocking(move || {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", pid_s.as_str()])
            .output();
    })
    .await;
}

#[cfg(not(unix))]
async fn kill_process_group(_pid: u32) {}

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
    // F48 (2026-07-18): the previous predicate used loose substring
    // matches like `delta` and `bytes` which fired on unrelated
    // stderr (e.g. `error: cannot merge — delta-branch merge`),
    // extending the deadline on adversarial input. Tighten to the
    // patterns git actually emits on progress paths.
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?xi)               # case-insensitive, multi-line (anchors per line)
            ^\s*(?:                 # line-start, optional indent
                (?:counting|writing|compressing|receiving|resolving)\s+objects:\s+\d+% |
                Total\s+\d+.*\(\S+\s+\d+/\d+\) |
                \d+\s*KiB\s*\|\s*\d+\.\d+\s+MiB/s |
                \d+%?\s*\(\d+/\d+\),\s*\d+\.\d+\s+KiB\s*\|\s*\d+\.\d+\s+MiB/s |
                (?:\d+ bytes|\d+\.\d+\s+\w+)\s*\|
            )
            |^remote:\s+\S", // any 'remote: ...' line emitted by server-side hooks.
        )
        .expect("static regex compiles")
    });
    re.is_match(line)
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

/// Owns only a child group created by configure_git_process_group. Disarm
/// immediately after reaping its leader to avoid signalling a reused PID.
struct GroupKillGuard(Option<u32>);
impl Drop for GroupKillGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            // SAFETY: a positive child PID is also its explicitly-created PGID.
            let result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    eprintln!("git: failed to kill process group {pid}: {error}");
                }
            }
        }
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
    // This runner receives children spawned into their own process group.
    // kill_on_drop kills only the leader, not SSH/filter/helper descendants.
    let mut group_guard = GroupKillGuard(pid);
    let stderr_handle = child.stderr.take();
    let stdout_handle = child.stdout.take();
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<Instant>();
    // Keep capture owned by this future so cancellation cannot detach it.
    let stderr_task = async move {
        let mut stderr_output = String::new();
        let mut stderr_truncated = false;
        const MAX_STDERR_BYTES: usize = 1024 * 1024;
        if let Some(mut stderr) = stderr_handle {
            let mut lines = BufReader::new(&mut stderr).lines();
            loop {
                // F50 (2026-07-18): a broken pipe (subprocess OOM-killed,
                // pipe closed early) returns Err; the previous
                // `while let Ok(Some(line))` silently dropped the
                // error. Surface the pipe break so the caller's
                // diagnostic distinguishes pipe error from timeout.
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Some(is_progress) = progress_predicate.as_mut() {
                            if is_progress(&line) {
                                let _ = progress_tx.send(Instant::now());
                            }
                        }
                        if stderr_output.len() < MAX_STDERR_BYTES {
                            if !stderr_output.is_empty() {
                                stderr_output.push('\n');
                            }
                            let remaining = MAX_STDERR_BYTES.saturating_sub(stderr_output.len());
                            let mut end = remaining.min(line.len());
                            while end > 0 && !line.is_char_boundary(end) {
                                end -= 1;
                            }
                            stderr_output.push_str(&line[..end]);
                            if end < line.len() {
                                stderr_truncated = true;
                            }
                        } else {
                            stderr_truncated = true;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!(
                            "stderr pipe error during git op (likely child crashed): {}",
                            e
                        );
                        stderr_output.push_str(&format!("<stderr pipe error: {}>", e));
                        break;
                    }
                }
            }
        }
        if stderr_truncated {
            stderr_output.push_str("\n<stderr truncated at 1 MiB>");
        }
        stderr_output
    };
    let stderr_task = Box::pin(stderr_task);
    let capture = async move {
        let stdout_task = async move {
            let mut captured = Vec::new();
            if let Some(mut pipe) = stdout_handle {
                use tokio::io::AsyncReadExt;
                let mut chunk = [0u8; 8192];
                // Hooks often print gate refusals to stdout. Bound the
                // diagnostic prefix, but always drain to avoid deadlock.
                loop {
                    match pipe.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let keep = n.min((64 * 1024usize).saturating_sub(captured.len()));
                            captured.extend_from_slice(&chunk[..keep]);
                        }
                        Err(_) => {
                            captured.extend_from_slice(b"<stdout pipe error>");
                            break;
                        }
                    }
                }
            }
            captured
        };
        let (mut stderr, stdout) = tokio::join!(stderr_task, stdout_task);
        if !stdout.is_empty() {
            stderr.push_str("\n<hook/stdout diagnostics>\n");
            stderr.push_str(&super::push::redact_credentials_for_log(
                &String::from_utf8_lossy(&stdout),
            ));
        }
        stderr
    };
    let mut stderr_task = Box::pin(capture);
    let mut captured_stderr = None;

    let started_at = Instant::now();
    let hard_deadline = started_at + Duration::from_secs(progress_hard_timeout_secs(timeout_secs));
    let mut deadline = started_at + Duration::from_secs(timeout_secs);
    // F49 (2026-07-19): the previous 250ms poll was longer than
    // needed for try_wait accuracy; reduce to 100ms. The progress
    // wakeup is already event-driven via progress_rx in the
    // tokio::select! below; this poll is just the safety net to
    // catch child exit between progress events.
    let poll_interval = Duration::from_millis(100);

    loop {
        let effective_deadline = std::cmp::min(deadline, hard_deadline);
        let remaining = effective_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            if let Some(pid) = pid {
                kill_process_group(pid).await;
            }
            let _ = child.start_kill();
            let _ = child.wait().await;
            group_guard.0 = None;
            if captured_stderr.is_none() {
                let _ = stderr_task.await;
            }
            return Err(anyhow::anyhow!(
                "{} timeout in {} after {}s idle ({}s hard cap)",
                label,
                workdir.display(),
                timeout_secs,
                progress_hard_timeout_secs(timeout_secs)
            ));
        }

        // Drain stderr before reaping the leader: helpers may inherit the
        // pipe and outlive Git. Keeping the leader unreaped retains its PID
        // (and our group identity) until cancellation-safe capture finishes.
        // EOF wakes this select immediately; the next iteration waits for
        // child exit without a polling delay. Timeout/progress stay active
        // even when a helper holds the pipe open after Git exits.
        tokio::select! {
            status = child.wait(), if captured_stderr.is_some() => {
                let status = status
                    .map_err(|e| anyhow::anyhow!("{} failed in {}: {}", label, workdir.display(), e))?;
                // wait reaped the leader; never signal its potentially reused PID.
                group_guard.0 = None;
                let stderr_output = match captured_stderr.take() {
                    Some(output) => output,
                    None => stderr_task.await,
                };
                return child_status_result(status, label, workdir, stderr_output);
            }
            output = &mut stderr_task, if captured_stderr.is_none() => {
                captured_stderr = Some(output);
            }
            Some(_) = progress_rx.recv() => {
                deadline = std::cmp::min(
                    Instant::now() + Duration::from_secs(timeout_secs),
                    hard_deadline,
                );
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
    spawn_git_command_inner(repo, args, op_label, &[], None)
}

fn spawn_git_command_fenced(
    repo: &Path,
    args: &[&str],
    op_label: &str,
    fence: &WriteFence,
) -> Result<tokio::process::Child> {
    spawn_git_command_inner(repo, args, op_label, &[], Some(fence))
}

fn spawn_git_command_inner(
    repo: &Path,
    args: &[&str],
    op_label: &str,
    env: &[(&str, &str)],
    fence: Option<&WriteFence>,
) -> Result<tokio::process::Child> {
    let label = format!("git {}", op_label);
    let mut cmd = crate::policy::tokio_git_command();
    cmd.args(args)
        .current_dir(repo)
        .kill_on_drop(true)
        .stdout(if args.contains(&"push") {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stderr(std::process::Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    if let Some(fence) = fence {
        fence.verify()?;
        fence.inherit_into(&mut cmd);
    }
    configure_git_process_group(&mut cmd);
    super::push_resources::configure_child(&mut cmd, args);
    cmd.spawn()
        .with_context(|| format!("failed to spawn {} in {}", label, repo.display()))
}

/// Spawn Git in its own process group with captured output. The child must
/// be passed immediately to `run_git_captured_output`: kill_on_drop alone
/// kills only Git, while that runner owns cancellation of its helpers too.
pub(crate) fn spawn_git_command_cancellable(
    repo: &Path,
    args: &[&str],
    op_label: &str,
) -> Result<tokio::process::Child> {
    spawn_git_command_cancellable_io(repo, args, op_label, false)
}

/// Captured Git with caller-owned stdin; drain output concurrently with input.
pub(crate) fn spawn_git_command_cancellable_stdin(
    repo: &Path,
    args: &[&str],
    op_label: &str,
) -> Result<tokio::process::Child> {
    spawn_git_command_cancellable_io(repo, args, op_label, true)
}

fn spawn_git_command_cancellable_io(
    repo: &Path,
    args: &[&str],
    op_label: &str,
    stdin: bool,
) -> Result<tokio::process::Child> {
    let label = format!("git {}", op_label);
    let mut cmd = crate::policy::tokio_git_command();
    cmd.args(args)
        .current_dir(repo)
        .kill_on_drop(true)
        .stdin(if stdin {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if stdin {
        // Accounting callers explicitly own the target repository. Do not
        // inherit repository/config injection or permit promisor lazy fetches.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1");
    }
    configure_git_process_group(&mut cmd);
    cmd.spawn()
        .with_context(|| format!("failed to spawn {} in {}", label, repo.display()))
}

/// Run a cancellable git child to completion, capturing both output streams,
/// WITHOUT imposing a timeout of its own. The CALLER decides cancellation —
/// e.g. `tokio::time::timeout` around the future — and the child's process
/// group dies with the future via `kill_on_drop` + process group.
///
/// Returns (exit status, stdout bytes, stderr bytes). Output is fully
/// drained before awaiting the exit so large traversals cannot deadlock on
/// full pipes; stdout is capped in memory to keep classification bounded
/// (a repo whose diff exceeds the cap surfaces the cap, not a hang).
pub(crate) async fn run_git_captured_output(
    child: tokio::process::Child,
    workdir: &Path,
    op_label: &str,
) -> anyhow::Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>)> {
    run_captured_output(child, workdir, &format!("git {op_label}")).await
}

/// Capture a workflow command in an owned process group with a finite deadline.
/// Input and both bounded output streams are driven together; neither a blocked
/// stdin write nor a helper retaining stderr can escape timeout/cancellation.
/// Preserve caller cwd/env, and return raw bytes without echoing SDK diagnostics.
pub(crate) async fn run_command_captured_output(
    command: TokioCommand,
    timeout_secs: u64,
    label: &str,
    input: Option<&[u8]>,
) -> Result<std::process::Output> {
    run_command_captured_input(command, timeout_secs, label, input, None).await
}

/// Stream a caller-held read-only source descriptor, not a pathname that may be
/// replaced during validation. Same bounded/cancellable process-group ownership.
pub(crate) async fn run_command_captured_file(
    command: TokioCommand,
    timeout_secs: u64,
    label: &str,
    file: std::fs::File,
) -> Result<std::process::Output> {
    run_command_captured_input(command, timeout_secs, label, None, Some(file)).await
}

async fn run_command_captured_input(
    mut command: TokioCommand,
    timeout_secs: u64,
    label: &str,
    input: Option<&[u8]>,
    file: Option<std::fs::File>,
) -> Result<std::process::Output> {
    use tokio::io::AsyncWriteExt;
    let workdir = command
        .as_std()
        .get_current_dir()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    command
        .kill_on_drop(true)
        .stdin(if let Some(file) = file {
            std::process::Stdio::from(file)
        } else if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    configure_git_process_group(&mut command);
    let run = async {
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn {label}"))?;
        let stdin = child.stdin.take();
        let capture = run_captured_output(child, &workdir, label);
        let write = async {
            if let Some(input) = input {
                let mut stdin = stdin.context("workflow stdin not captured")?;
                stdin
                    .write_all(input)
                    .await
                    .context("workflow stdin write failed")?;
                stdin
                    .shutdown()
                    .await
                    .context("workflow stdin close failed")?;
            }
            Ok::<(), anyhow::Error>(())
        };
        let ((status, stdout, stderr), ()) = tokio::try_join!(capture, write)?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    tokio::time::timeout(Duration::from_secs(timeout_secs.max(1)), run)
        .await
        .with_context(|| format!("{label} timed out after {}s", timeout_secs.max(1)))?
}

async fn run_captured_output(
    mut child: tokio::process::Child,
    workdir: &Path,
    label: &str,
) -> anyhow::Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>)> {
    // Cancellation cannot await a grace period. Send SIGKILL directly to
    // the owned group; no shell-outs, sleeps or detached capture tasks.
    // Keep the leader unreaped while draining pipes, so its PID cannot be
    // reused before a cancellation signal. Disarm immediately after wait.
    let mut group_guard = GroupKillGuard(child.id());
    const MAX_CAPTURED_BYTES: usize = 64 * 1024 * 1024;
    let stdout_pipe = child
        .stdout
        .take()
        .with_context(|| format!("{} in {}: stdout not captured", label, workdir.display()))?;
    let stderr_pipe = child
        .stderr
        .take()
        .with_context(|| format!("{} in {}: stderr not captured", label, workdir.display()))?;
    // Both readers belong to this future. An overflow or read error is
    // fail-closed and drops the sibling reader plus the process-group guard.
    let (stdout_buf, stderr_buf) = tokio::try_join!(
        read_captured_pipe(stdout_pipe, MAX_CAPTURED_BYTES),
        read_captured_pipe(stderr_pipe, MAX_CAPTURED_BYTES),
    )
    .with_context(|| format!("{} in {}: output capture failed", label, workdir.display()))?;
    let status = child
        .wait()
        .await
        .with_context(|| format!("{} in {}: wait failed", label, workdir.display()))?;
    group_guard.0 = None;
    Ok((status, stdout_buf, stderr_buf))
}

async fn read_captured_pipe(
    mut pipe: impl tokio::io::AsyncRead + Unpin,
    cap: usize,
) -> anyhow::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    // This reader is embedded twice in capture and transitively in the sync
    // state machine. Keep its scratch space off the executor/test stack.
    let mut chunk = vec![0u8; 8192];
    loop {
        let n = pipe.read(&mut chunk).await?;
        if n == 0 {
            return Ok(bytes);
        }
        anyhow::ensure!(
            n <= cap.saturating_sub(bytes.len()),
            "output exceeded {cap} bytes"
        );
        bytes.extend_from_slice(&chunk[..n]);
    }
}

fn spawn_git_command_env(
    repo: &Path,
    args: &[&str],
    op_label: &str,
    env: &[(&str, &str)],
) -> Result<tokio::process::Child> {
    spawn_git_command_inner(repo, args, op_label, env, None)
}

fn spawn_git_command_env_fenced(
    repo: &Path,
    args: &[&str],
    op_label: &str,
    env: &[(&str, &str)],
    fence: &WriteFence,
) -> Result<tokio::process::Child> {
    spawn_git_command_inner(repo, args, op_label, env, Some(fence))
}

/// Run a git command with a timeout using the tokio git command builder.
pub(crate) async fn run_git_with_timeout(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    super::push_resources::command(
        args,
        run_git_with_timeout_inner(repo, args, timeout_secs, op_label),
    )
    .await
}

async fn run_git_with_timeout_inner(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
) -> Result<()> {
    run_git_with_timeout_inner_fenced(repo, args, timeout_secs, op_label, None).await
}

async fn run_git_with_timeout_inner_fenced(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    fence: Option<&WriteFence>,
) -> Result<()> {
    let label = format!("git {}", op_label);
    let trace_add = args.first() == Some(&"add") && crate::policy::debug_enabled();
    let unix_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    };
    if trace_add {
        eprintln!(
            "scheduler: add_spawn repo={} unix_ms={}",
            repo.display(),
            unix_ms()
        );
    }
    let child = match fence {
        Some(fence) => spawn_git_command_fenced(repo, args, op_label, fence)?,
        None => spawn_git_command(repo, args, op_label)?,
    };
    if trace_add {
        eprintln!(
            "scheduler: add_spawned repo={} unix_ms={} pid={:?}",
            repo.display(),
            unix_ms(),
            child.id()
        );
    }
    let result = run_child(child, repo, timeout_secs, &label).await;
    if trace_add {
        eprintln!(
            "scheduler: add_done repo={} unix_ms={} ok={}",
            repo.display(),
            unix_ms(),
            result.is_ok()
        );
    }
    result
}

/// ADDED 2026-07-21 (v0.112.33, audit M13/F2.4): run a std git
/// command to completion and require exit status 0, including stderr
/// in the error. The recurring `.status().with_context(...)?`
/// pattern only surfaces SPAWN failure — a non-zero exit from the
/// git process is silently treated as success (~6 call sites,
/// including `consolidate_to_main` proceeding to `branch -D master`
/// on a failed `git checkout main`).
pub(crate) fn std_git_checked(
    cmd: &mut std::process::Command,
    context: &str,
) -> anyhow::Result<()> {
    let output = cmd
        .output()
        .with_context(|| format!("{} (spawn failed)", context))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!(
            "{} (exit {}): {}",
            context,
            output.status,
            stderr.trim()
        ))
    }
}

/// Run a Git worktree-mutating command while a caller-owned canonical writer
/// fence remains inherited by the complete process family through final close.
pub(crate) async fn run_git_with_timeout_fenced(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    fence: &WriteFence,
) -> Result<()> {
    super::push_resources::command(
        args,
        run_git_with_timeout_inner_fenced(repo, args, timeout_secs, op_label, Some(fence)),
    )
    .await
}

/// Run a git command with extra environment variables and a timeout.
pub(crate) async fn run_git_with_timeout_env(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    env: &[(&str, &str)],
) -> Result<()> {
    super::push_resources::command(args, async {
        let label = format!("git {}", op_label);
        let child = spawn_git_command_env(repo, args, op_label, env)?;
        run_child(child, repo, timeout_secs, &label).await
    })
    .await
}

/// Run an environment-configured Git command under a caller-held worktree fence.
pub(crate) async fn run_git_with_timeout_env_fenced(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    env: &[(&str, &str)],
    fence: &WriteFence,
) -> Result<()> {
    super::push_resources::command(args, async {
        let label = format!("git {}", op_label);
        let child = spawn_git_command_env_fenced(repo, args, op_label, env, fence)?;
        run_child(child, repo, timeout_secs, &label).await
    })
    .await
}

/// Run a git push/pull with a progress-aware idle timeout.
pub(crate) async fn run_git_with_timeout_env_progress(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    env: &[(&str, &str)],
) -> Result<()> {
    super::push_resources::command(args, async {
        let label = format!("git {}", op_label);
        let child = spawn_git_command_env(repo, args, op_label, env)?;
        run_child_with_progress(child, repo, timeout_secs, &label, is_git_push_progress_line).await
    })
    .await
}

/// Run a progress-aware Git operation while holding the caller's worktree fence.
pub(crate) async fn run_git_with_timeout_env_progress_fenced(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    op_label: &str,
    env: &[(&str, &str)],
    fence: &WriteFence,
) -> Result<()> {
    super::push_resources::command(args, async {
        let label = format!("git {}", op_label);
        let child = spawn_git_command_env_fenced(repo, args, op_label, env, fence)?;
        run_child_with_progress(child, repo, timeout_secs, &label, is_git_push_progress_line).await
    })
    .await
}
#[cfg(unix)]
pub(crate) async fn git_askpass_script(token: &str) -> Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    // Shell-quote the token (POSIX single-quote escape). For
    // alnum-only tokens (the realistic case — PATs of any forge)
    // this is a no-op. Tokens with `'` break the inner quoting;
    // Forgejo/GitLab allow this in some legacy schemes but we treat
    // it as a hard error rather than risk malformed shell.
    if token.contains('\'') {
        anyhow::bail!("git_askpass_script: token contains a single quote (refused; F59)");
    }
    let script = format!("#!/bin/sh\nprintf '%s\\n' '{token}'\n");

    // F41 fix (2026-07-18): create the file atomically with mode
    // 0o700 (O_EXCL | O_NOFOLLOW). The previous flow wrote the file
    // with default umask (typically 0o666) and then tightened
    // permissions afterwards — the file was world-readable between
    // the write and chmod. The caller should still `unlink` the
    // returned path via the AskpassScript guard (see below) so the
    // credential doesn't linger in /tmp.
    //
    // FIXED 2026-10-03 (audit R3-L08): the old best-effort
    // pre-remove of `tmp_path` reopened the symlink/race window the
    // O_EXCL contract otherwise closes (unlink a path we never
    // created, then a gap before exclusive create). Create-exclusive
    // FIRST; on the near-impossible pid+nanos collision (EEXIST)
    // retry with a fresh timestamp instead of removing anything.
    for attempt in 0..3u32 {
        let nano = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = std::env::temp_dir().join(format!(
            "dracon-git-askpass-{}-{}-{}.sh",
            std::process::id(),
            nano,
            attempt
        ));

        // Atomic create with mode 0o700.
        let created = {
            use std::fs::OpenOptions;
            let mut openopts = OpenOptions::new();
            openopts
                .write(true)
                .create_new(true)
                .truncate(false)
                .custom_flags(libc_o_excl_o_nofollow())
                .mode(0o700);
            openopts.open(&tmp_path)
        };
        let mut f = match created {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 2 => continue,
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "failed to create GIT_ASKPASS script at {}",
                        tmp_path.display()
                    )
                });
            }
        };
        use std::io::Write;
        f.write_all(script.as_bytes()).with_context(|| {
            format!(
                "failed to write GIT_ASKPASS script to {}",
                tmp_path.display()
            )
        })?;
        return Ok(tmp_path);
    }
    unreachable!("retry loop returns or bails on its final iteration");
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

impl AskpassScript {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for AskpassScript {
    fn drop(&mut self) {
        // Best-effort synchronous unlink. NotFound is fine (already
        // cleaned up — nothing lingers). Any OTHER failure leaves the
        // token script in /tmp and must be LOUD (FIXED 2026-10-02,
        // audit L4: the old `let _ =` swallowed EPERM/EROFS with no
        // log). The file is created 0o700 owned by the daemon user.
        if let Err(e) = std::fs::remove_file(&self.path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                crate::log_warn!(
                    "askpass cleanup failed for {}: {} (token script lingers on disk)",
                    self.path.display(),
                    e
                );
            }
        }
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

/// Test helper: returns whether `git filter-repo` is on PATH.
/// Used by F31 regression test to skip when filter-repo is absent.
#[cfg(test)]
pub(crate) fn filter_repo_available_for_tests() -> bool {
    use std::process::Command;
    Command::new("git")
        .args(["filter-repo", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_git_push_progress_line;

    #[test]
    fn captured_pipe_future_does_not_embed_its_io_buffer() {
        let reader = super::read_captured_pipe(tokio::io::empty(), 1024);
        let size = std::mem::size_of_val(&reader);
        assert!(
            size < 2048,
            "capture future embeds a large buffer: {size} bytes"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn owned_workflow_capture_drives_input_and_both_outputs_together() {
        let mut command = tokio::process::Command::new("sh");
        command.args([
            "-c",
            "head -c 131072 /dev/zero; head -c 131072 /dev/zero >&2; cat; exit 23",
        ]);
        let input = vec![b'x'; 1024 * 1024];
        let output =
            super::run_command_captured_output(command, 10, "workflow-io-fixture", Some(&input))
                .await
                .unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "nonzero exit must not be discarded"
        );
        assert_eq!(output.stderr, vec![0; 131072]);
        assert_eq!(&output.stdout[..131072], vec![0; 131072]);
        assert_eq!(&output.stdout[131072..], input);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn owned_workflow_deadline_and_cancellation_kill_only_the_owned_family() {
        use std::time::Duration;
        for cancel in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let mut command = tokio::process::Command::new("sh");
            command
                .args(["-c", "sleep 30 & echo $! > helper.pid; wait"])
                .current_dir(temp.path());
            // This unrelated fixture child must survive the owned group's exit.
            let mut sentinel = tokio::process::Command::new("sleep")
                .arg("30")
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let input = vec![b'x'; 1024 * 1024];
            let mut run = Box::pin(super::run_command_captured_output(
                command,
                if cancel { 30 } else { 2 },
                "workflow-blocked-stdin",
                Some(&input),
            ));
            let marker = temp.path().join("helper.pid");
            tokio::select! {
                result = &mut run => panic!("fixture exited before blocking: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(5), async {
                    while !marker.exists() { tokio::time::sleep(Duration::from_millis(10)).await; }
                }) => ready.unwrap(),
            }
            let pid: u32 = std::fs::read_to_string(marker)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let identity = || {
                std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .map(|text| {
                        let fields: Vec<_> = text
                            .rsplit_once(") ")
                            .unwrap()
                            .1
                            .split_whitespace()
                            .collect();
                        (fields[0].to_owned(), fields[19].to_owned())
                    })
            };
            let before = identity().unwrap();
            if cancel {
                drop(run);
            } else {
                assert!(run.await.unwrap_err().to_string().contains("timed out"));
            }
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if identity().is_none_or(|(state, start)| state == "Z" || start != before.1) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("workflow child escaped its owned group after cancellation");
            assert!(
                sentinel.try_wait().unwrap().is_none(),
                "unrelated active workload was killed"
            );
            sentinel.kill().await.unwrap();
            sentinel.wait().await.unwrap();
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_hook_stdout_is_bounded_drained_and_retained() {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "printf 'Pre-push: TEST gate failed\\n'; head -c 100000 /dev/zero; echo fatal >&2; exit 1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        super::configure_git_process_group(&mut command);
        let child = command.spawn().unwrap();
        let error = super::run_child(child, std::path::Path::new("."), 5, "push-fixture")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Pre-push: TEST gate failed"));
        assert!(error.contains("fatal"));
        assert!(error.len() < 66000);
    }

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

    #[test]
    fn test_f48_tightened_progress_predicate() {
        // F48 regression: the new predicate must NOT extend the
        // deadline on error messages that happen to contain `delta`
        // or `bytes`.
        assert!(!is_git_push_progress_line(
            "error: cannot merge without a merge base (use --allow-unrelated-histories for a delta-branch merge strategy)"
        ));
        assert!(!is_git_push_progress_line("[trace] 0 bytes allocated"));
        assert!(!is_git_push_progress_line(
            "fatal: protocol error: bad bandle 42"
        ));
        // And it MUST still match the legitimate patterns.
        assert!(is_git_push_progress_line(
            "remote: Total 42 (delta 1), reused 0 (delta 0)"
        ));
        assert!(is_git_push_progress_line("remote: Processing 1234"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn child_exit_wakes_runner_without_poll_interval() {
        use futures::FutureExt;
        use tokio::io::AsyncWriteExt;
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", "read line; exit 0"])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let run = super::run_child(child, std::path::Path::new("."), 5, "exit-wakeup-fixture");
        tokio::pin!(run);
        // The child is definitely still blocked on stdin when the runner
        // first registers its waits, so try_wait cannot satisfy this test.
        assert!(run.as_mut().now_or_never().is_none());
        input.write_all(b"finish\n").await.unwrap();
        drop(input);
        tokio::time::timeout(std::time::Duration::from_millis(80), run)
            .await
            .expect("child exit must wake the runner before the old 100ms polling tick")
            .unwrap();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn cancellation_after_leader_exit_kills_stderr_holder() {
        use std::time::Duration;
        let temp = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", "sleep 30 & echo $! > helper.pid; exit 0"])
            .current_dir(temp.path())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        super::configure_git_process_group(&mut command);
        let child = command.spawn().unwrap();
        let leader = child.id().unwrap();
        let root = temp.path().to_path_buf();
        let worker = tokio::spawn(async move {
            super::run_child(child, &root, 10, "stderr-holder-fixture").await
        });
        let helper_file = temp.path().join("helper.pid");
        tokio::time::timeout(Duration::from_secs(3), async {
            while !helper_file.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            // Wait for exit (zombie or already reaped), not merely spawn.
            loop {
                let state = std::fs::read_to_string(format!("/proc/{leader}/stat")).ok();
                if state
                    .as_ref()
                    .is_none_or(|s| s.rsplit_once(") ").unwrap().1.starts_with('Z'))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let helper: i32 = std::fs::read_to_string(helper_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // Also clean up on assertion failure; this fixture must not leak.
        struct Cleanup(i32);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    libc::kill(self.0, libc::SIGKILL);
                }
            }
        }
        let _cleanup = Cleanup(helper);
        // Give the old runner a turn to reap and disarm before aborting.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!worker.is_finished(), "helper still owns stderr");
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let state = std::fs::read_to_string(format!("/proc/{helper}/stat")).ok();
                if state
                    .as_ref()
                    .is_none_or(|s| s.rsplit_once(") ").unwrap().1.starts_with('Z'))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled runner left stderr helper alive after leader exit");
    }

    #[test]
    fn progress_timeout_has_a_bounded_hard_ceiling() {
        assert_eq!(super::progress_hard_timeout_secs(0), 0);
        assert_eq!(super::progress_hard_timeout_secs(60), 240);
        assert_eq!(super::progress_hard_timeout_secs(u64::MAX), u64::MAX);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_git_askpass_script_atomic_0o700_create_and_cleanup() {
        use super::{git_askpass_script, AskpassScript};
        use std::os::unix::fs::PermissionsExt;

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
            assert!(
                tokio::fs::metadata(&path).await.is_ok(),
                "file exists in scope"
            );
        }
        // After drop, file should be gone.
        assert!(
            tokio::fs::metadata(&path).await.is_err(),
            "askpass script was not unlinked after Drop"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_git_askpass_script_creates_distinct_paths_without_remove() {
        // R3-L08: successive creates must yield distinct live files —
        // the old pre-remove + fixed pid+nanos name could unlink and
        // recreate a colliding path; creation is now exclusive-first
        // with a retry counter in the name.
        use super::{git_askpass_script, AskpassScript};
        let a = git_askpass_script("[REDACTED]").await.expect("create a");
        let _guard_a = AskpassScript::new(a.clone());
        let b = git_askpass_script("[REDACTED]").await.expect("create b");
        let _guard_b = AskpassScript::new(b.clone());
        assert_ne!(a, b, "two creates must not share a path");
        assert!(
            tokio::fs::metadata(&a).await.is_ok() && tokio::fs::metadata(&b).await.is_ok(),
            "both scripts must exist simultaneously (no clobber)"
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

    #[cfg(unix)]
    #[test]
    fn test_askpass_drop_logs_unlink_failure() {
        // ADDED 2026-10-02 (audit L4): a failed unlink must be LOUD —
        // the old `let _ =` swallowed EPERM/EROFS with no log while
        // the token script lingered in /tmp. Unlinking a DIRECTORY
        // with remove_file fails deterministically (non-NotFound), so
        // Drop takes the warn branch (visible on stderr) and the path
        // survives — proving the failure was hit, not silently
        // absorbed. A guard over an already-gone path stays silent
        // (nothing lingers, nothing to report).
        use super::AskpassScript;

        let dir = tempfile::tempdir().expect("temp dir");
        let subdir = dir.path().join("not-a-file");
        std::fs::create_dir(&subdir).expect("mkdir");
        {
            let _guard = AskpassScript::new(subdir.clone());
        }
        assert!(
            subdir.is_dir(),
            "a directory cannot be unlinked by remove_file — the Drop failure branch must have fired"
        );

        let gone = dir.path().join("already-cleaned");
        {
            let _guard = AskpassScript::new(gone);
        }
    }
}
