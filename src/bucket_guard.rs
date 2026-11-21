//! Forward-only bucket guard for the daemon auto-commit path.
//!
//! Background (goal 20261009193116-1ess3v, 2026-10-09): the platform
//! `.githooks/pre-commit` bucket guard runs the staged forward-only check
//! (`BUCKET_STRATEGY_GUARD_FORWARD_ONLY`) for human commits, but daemon
//! auto-commits are sealed through libgit2 (`dracon-git` `GitService::commit`),
//! which never executes hooks. The CLI path runs the configured hook chain
//! (`git commit`, no `--no-verify`) and the daemon invokes the same chain
//! explicitly before a sealed commit, but this in-process check remains the
//! guarantee that survives a repository with no hook installed at all.
//! That is how hellhunter `f5d55b5` (27 deleted protected assets) and
//! deathrun `30352ba` (4 deleted webp backdrops) were committed silently and
//! only detonated at push time, wedging both repos 20+ commits deep.
//!
//! This module runs the same staged check the hook runs, from inside the
//! daemon, between staging and committing. Repos without a discoverable
//! guard script are unaffected (the check is layout-gated, not policy
//! gated). Violations — and guard infrastructure failures — block the commit
//! with the index intact, mirroring the hook's fail-closed semantics.
//!
//! CHANGED 2026-11-21 (forward-only demotion): one refusal class no longer
//! blocks an UNATTENDED commit. The forward-only policy is correct for a
//! human — `git commit` stopping on a suspected accidental asset deletion is
//! the point of it — but it is a permanent wedge for the daemon, because the
//! check re-runs against the same staged tree (and later the same unpushed
//! range) on every attempt. One intentional retirement then blocks the commit
//! carrying it and every change queued behind it, and the repository can
//! never converge. Observed live: dracon-platform sat wedged from 20:27 on
//! 2026-11-21 with one superseded cover JPEG staged while 20+ unrelated files
//! waited behind it.
//!
//! The demotion is narrow and evidence-preserving: only
//! `BUCKET_STRATEGY_GUARD_FORWARD_ONLY` may proceed, only when the wrapper
//! honoured `DRACON_BUCKET_GUARD_MODE=warn`, and only as a RECORDED warning —
//! see [`GuardDemotion`]. Every other refusal code still fails closed here,
//! and the guard's own default (no mode set) is untouched, so an interactive
//! `git commit` still refuses.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Basename the hook wrapper looks for while walking checkout ancestry.
const GUARD_SCRIPT_RELATIVE: &str = "web/scripts/bucket-strategy-guard.sh";

/// The environment variable the guard wrapper reads to demote a
/// forward-only refusal (ADDED 2026-11-21). The wrapper and its demoter
/// (`bucket-guard-demote.mjs`) own the demotion decision; the daemon only
/// opts in and reads the verdict.
const GUARD_MODE_ENV: &str = "DRACON_BUCKET_GUARD_MODE";

/// The only mode value the wrapper demotes under. Anything else — including a
/// misspelling — keeps the historical hard stop.
const GUARD_MODE_PRESENT: (&str, &str) = (GUARD_MODE_ENV, "warn");

/// Prefix of the machine-readable demotion line the demoter emits on stderr.
const DEMOTION_PREFIX: &str = "bucket-guard-warning: ";

/// A demoted forward-only refusal: the guard refused, warn mode admitted the
/// commit, and the caller must RECORD what was admitted rather than treat the
/// pass as clean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuardDemotion {
    /// The original refusal code (`BUCKET_STRATEGY_GUARD_FORWARD_ONLY`).
    pub code: String,
    /// Violation classes reported by the guard (`staged`, `delete`, ...).
    pub operations: Vec<String>,
    /// The affected paths/refs, in the guard's order.
    pub paths: Vec<String>,
}

impl GuardDemotion {
    /// One-line description for the journal, alert ledger and row hint.
    pub(crate) fn summary(&self) -> String {
        let what = if self.paths.is_empty() {
            "un-named path".to_owned()
        } else {
            self.paths.join(", ")
        };
        format!(
            "{} demoted to a warning ({}) — {what} retired under policy; bytes remain in history",
            self.code,
            if self.operations.is_empty() {
                "unspecified".to_owned()
            } else {
                self.operations.join("+")
            }
        )
    }

    /// The verdict's own `errors` lines, which name each offending path.
    pub(crate) fn paths_label(&self) -> String {
        if self.paths.is_empty() {
            String::new()
        } else {
            self.paths.join("; ")
        }
    }
}

/// Parse the demoter's machine-readable warning out of the guard's stderr.
///
/// The line is JSON after a stable prefix, so a warning is never inferred from
/// prose. `None` means the wrapper did NOT demote (either no mode was set, the
/// refusal was not forward-only, or the wrapper predates the mode) — the
/// caller then keeps its ordinary hard-stop behaviour.
pub(crate) fn demotion_from_stderr(stderr: &str) -> Option<GuardDemotion> {
    let line = stderr
        .lines()
        .find(|line| line.starts_with(DEMOTION_PREFIX))?;
    let value: serde_json::Value =
        serde_json::from_str(line.strip_prefix(DEMOTION_PREFIX)?.trim()).ok()?;
    let code = value.get("code")?.as_str()?.to_owned();
    let strings = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(GuardDemotion {
        code,
        operations: strings("operations"),
        paths: strings("paths"),
    })
}

/// Basename of the per-repo demotion record (gitdir-relative).
///
/// A demotion is a one-shot event: once the commit carrying the retirement
/// lands, the staged tree is clean and the guard stops refusing. Without a
/// persisted record the row hint would be gone before the operator could ever
/// read it, and a second unreported deletion would look like the first. The
/// record is therefore written beside the repository's own Git metadata (not
/// in the worktree, so it is never committed) and surfaced by the report while
/// it is fresh.
const DEMOTION_RECORD: &str = "dracon-bucket-guard-demotion.json";

/// How long a demotion record stays reportable. The retirement itself is
/// permanent (it is in history); this window only governs how long the table
/// keeps pointing at it, so a warning cannot become permanent furniture.
pub(crate) const DEMOTION_REPORT_WINDOW_SECS: u64 = 24 * 60 * 60;

fn demotion_record_path(repo: &Path) -> Option<PathBuf> {
    repo.join(".git")
        .exists()
        .then(|| crate::git::path_gitdir(repo))
        .flatten()
        .map(|gitdir| gitdir.join(DEMOTION_RECORD))
}

/// Persist the latest demotion so the report can name it.
///
/// Failure is diagnostic only: the commit already happened, so a record that
/// cannot be written must not turn an admitted retirement into a failed sync.
pub(crate) fn record_demotion(repo: &Path, demotion: &GuardDemotion) {
    let Some(path) = demotion_record_path(repo) else {
        return;
    };
    let record = serde_json::json!({
        "ts_unix": crate::policy::timestamp_secs(),
        "code": demotion.code,
        "operations": demotion.operations,
        "paths": demotion.paths,
    });
    let bytes = match serde_json::to_vec_pretty(&record) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("⚠️ cannot serialize bucket-guard demotion record: {error}");
            return;
        }
    };
    if let Err(error) = std::fs::write(&path, bytes) {
        eprintln!(
            "⚠️ cannot write bucket-guard demotion record {}: {error}",
            path.display()
        );
    }
}

/// Whether `repo` carries a demotion record fresh enough to report, and what
/// it says. `Err` is reserved for a record that exists but cannot be read, so
/// an unreadable record is never mistaken for "nothing happened".
pub(crate) fn pending_demotion(repo: &Path) -> anyhow::Result<Option<GuardDemotion>> {
    let Some(path) = demotion_record_path(repo) else {
        return Ok(None);
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        anyhow::anyhow!(
            "contains a bucket-guard demotion record that is not readable JSON: {error}"
        )
    })?;
    let ts_unix = value
        .get("ts_unix")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("bucket-guard demotion record has no timestamp"))?;
    let now = crate::policy::timestamp_secs();
    // A clock that jumped backwards must not make a record permanent: treat
    // a future timestamp as fresh only for the same window.
    let age = now.saturating_sub(ts_unix);
    if ts_unix > now && ts_unix - now > DEMOTION_REPORT_WINDOW_SECS {
        return Ok(None);
    }
    if age > DEMOTION_REPORT_WINDOW_SECS {
        return Ok(None);
    }
    let code = value
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("bucket-guard demotion record has no code"))?
        .to_owned();
    let strings = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(Some(GuardDemotion {
        code,
        operations: strings("operations"),
        paths: strings("paths"),
    }))
}

/// Walk `repo` and its ancestors for an executable guard script, mirroring
/// the `find_guard` function in `.githooks/pre-commit`. Returns `None` when
/// the repo is not bucket-managed.
fn discover_guard(repo: &Path, env_override: Option<&str>) -> Option<PathBuf> {
    if let Some(candidate) = env_override {
        let path = PathBuf::from(candidate);
        if path.is_file() && is_executable(&path) {
            return Some(path);
        }
        // An explicit override pointing nowhere is a configuration error,
        // not a skip: fail closed at the call site via the sentinel below.
        return Some(PathBuf::from(format!("\0missing-override:{candidate}")));
    }
    let mut current = repo.to_path_buf();
    loop {
        let candidate = current.join(GUARD_SCRIPT_RELATIVE);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
        current = current.parent()?.to_path_buf();
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Evaluate the pushable range exactly as the pre-push hook would: the commits
/// the next push of the current branch would newly publish (local tip vs the
/// upstream tip). Returns `None` when the repo is not bucket-managed or the
/// range is unresolvable (no branch/upstream) — the caller falls back to
/// summary-based routing. A live `Some` verdict is always current, unlike
/// stuck-ledger summaries that may record a stale or misclassified failure.
pub(crate) struct RangeVerdict {
    pub ok: bool,
    pub code: String,
    pub errors: Vec<String>,
    /// `Some` when warn mode demoted a forward-only refusal: the push may
    /// proceed, but the caller records it instead of treating this as a live
    /// violation (ADDED 2026-11-21).
    pub demoted: Option<GuardDemotion>,
}

pub(crate) async fn check_push_range(
    repo: &Path,
    timeout_secs: u64,
    env_override: Option<&str>,
) -> Result<Option<RangeVerdict>> {
    let guard = match discover_guard(repo, env_override) {
        None => return Ok(None),
        Some(path) => path,
    };
    let guard_display = guard.display().to_string();
    if guard_display.starts_with("\0missing-override:") {
        return Ok(None);
    }
    // Unresolvable range (not a repo, detached HEAD, no upstream) is not
    // an error: the caller falls back to summary-based routing.
    let branch = match git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) if !branch.is_empty() && branch != "HEAD" => branch,
        _ => return Ok(None),
    };
    let upstream = match git_output(repo, &["rev-parse", "@{u}"]) {
        Ok(upstream) if !upstream.is_empty() && !upstream.starts_with("0000000") => upstream,
        _ => return Ok(None),
    };
    let output = run_guard(
        &guard,
        repo,
        &[
            "--ref",
            &branch,
            "--remote-tip",
            &upstream,
            "--no-size",
            "--json",
        ],
        timeout_secs,
        true,
    )
    .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let value: serde_json::Value = serde_json::from_str(&stdout).with_context(|| {
        format!(
            "bucket guard range check produced unparseable output (exit {})",
            output.status
        )
    })?;
    let ok = value.get("ok") == Some(&serde_json::Value::Bool(true));
    let code = value
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or("BUCKET_STRATEGY_GUARD_UNKNOWN")
        .to_owned();
    let errors: Vec<String> = value
        .get("errors")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(RangeVerdict {
        ok,
        code,
        errors,
        demoted: demotion_from_stderr(&stderr),
    }))
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| format!("git {} failed to spawn", args.join(" ")))?;
    if !output.status.success() {
        anyhow::bail!("git {} failed", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn run_guard(
    guard: &Path,
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    demote: bool,
) -> Result<std::process::Output> {
    let guard_display = guard.display().to_string();
    let mut cmd = tokio::process::Command::new(guard);
    cmd.args(args).current_dir(repo);
    // Opt in to the wrapper's forward-only demotion. Set only here: the
    // daemon is the one caller that may proceed through that refusal, and
    // every interactive invocation keeps the wrapper's default hard stop.
    if demote {
        cmd.env(GUARD_MODE_PRESENT.0, GUARD_MODE_PRESENT.1);
    } else {
        cmd.env_remove(GUARD_MODE_ENV);
    }
    // Mirror the hook: strip repository-discovery overrides so the guard
    // measures this repo from its explicit root rather than inheriting the
    // daemon's (or a worktree's) GIT_* environment.
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_SHALLOW_FILE",
        "GIT_GRAFT_FILE",
        "GIT_REPLACE_REF_BASE",
    ] {
        cmd.env_remove(var);
    }
    crate::git::run_command_captured_output(cmd, timeout_secs, "bucket guard", None)
        .await
        .with_context(|| format!("bucket guard failed: {guard_display}"))
}

/// Run the staged forward-only check for `repo`.
///
/// * No discoverable guard script → `Ok(None)` (repo is not bucket-managed).
/// * Guard refuses and warn mode demoted it → `Ok(Some(GuardDemotion))`: the
///   commit may proceed, but the caller MUST record the demotion.
/// * Guard refuses without a demotion → `Err` carrying the violation code and
///   paths; the caller must leave the index intact and skip the commit.
/// * Guard infrastructure failure (missing override target, spawn failure,
///   timeout, unparseable output) → `Err` (fail closed, mirroring the hook).
pub(crate) async fn check_staged_forward_only(
    repo: &Path,
    timeout_secs: u64,
    env_override: Option<&str>,
) -> Result<Option<GuardDemotion>> {
    let guard = match discover_guard(repo, env_override) {
        None => return Ok(None),
        Some(path) => path,
    };
    let guard_display = guard.display().to_string();
    if guard_display.starts_with("\0missing-override:") {
        anyhow::bail!(
            "bucket guard override points nowhere: {}",
            guard_display.trim_start_matches('\0')
        );
    }
    let output = run_guard(
        &guard,
        repo,
        &["--staged", "--no-size", "--json"],
        timeout_secs,
        true,
    )
    .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: Option<serde_json::Value> = serde_json::from_str(&stdout).ok();
    match parsed {
        Some(value) if value.get("ok") == Some(&serde_json::Value::Bool(true)) => {
            // A demotion is an admitted refusal, never a clean pass: return it
            // so the caller records it. A genuinely clean verdict has no
            // demotion line and yields `None`.
            Ok(demotion_from_stderr(&stderr))
        }
        Some(value) => {
            let code = value
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("BUCKET_STRATEGY_GUARD_UNKNOWN");
            let mut errors: Vec<String> = value
                .get("errors")
                .and_then(|e| e.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if errors.is_empty() {
                errors.push(stdout.trim().chars().take(400).collect());
            }
            errors.truncate(10);
            anyhow::bail!(
                "bucket guard refused staged tree ({}): {}",
                code,
                errors.join("; ")
            );
        }
        None => {
            let stderr_tail: String = String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(400)
                .collect();
            anyhow::bail!(
                "bucket guard produced unparseable output (exit {}): {}",
                output.status,
                stderr_tail
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn stub_guard(dir: &Path, name: &str, body: &str) -> PathBuf {
        let root = dir.join("proj");
        let script_dir = root.join("web/scripts");
        std::fs::create_dir_all(&script_dir).unwrap();
        let script = script_dir.join("bucket-strategy-guard.sh");
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        root.join(name)
    }

    #[tokio::test]
    async fn no_guard_script_skips_check() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(check_staged_forward_only(tmp.path(), 10, None)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn guard_ok_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = stub_guard(
            tmp.path(),
            "repo",
            "#!/bin/sh\necho '{\"ok\": true, \"code\": \"OK\"}'\n",
        );
        std::fs::create_dir_all(&repo).unwrap();
        assert!(check_staged_forward_only(&repo, 10, None).await.is_ok());
    }

    #[tokio::test]
    async fn guard_violation_blocks_with_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = stub_guard(
            tmp.path(),
            "repo",
            "#!/bin/sh\necho '{\"ok\": false, \"code\": \"BUCKET_STRATEGY_GUARD_FORWARD_ONLY\", \"errors\": [\"staged: static/art/a.png\"]}'\nexit 1\n",
        );
        std::fs::create_dir_all(&repo).unwrap();
        let err = check_staged_forward_only(&repo, 10, None)
            .await
            .unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("BUCKET_STRATEGY_GUARD_FORWARD_ONLY"), "{msg}");
        assert!(msg.contains("static/art/a.png"), "{msg}");
    }

    #[tokio::test]
    async fn guard_garbage_output_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = stub_guard(tmp.path(), "repo", "#!/bin/sh\necho 'not json'\nexit 1\n");
        std::fs::create_dir_all(&repo).unwrap();
        assert!(check_staged_forward_only(&repo, 10, None).await.is_err());
    }

    #[tokio::test]
    async fn push_range_reports_live_violations() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("proj");
        let script_dir = proj.join("web/scripts");
        std::fs::create_dir_all(&script_dir).unwrap();
        let script = script_dir.join("bucket-strategy-guard.sh");
        // Stub asserts it was invoked with the push-range argv (branch + a
        // full upstream sha), then reports a violation like the real guard
        // would for unpublished deletes.
        std::fs::write(
            &script,
            "#!/bin/sh\ncase \" $* \" in *\" --ref main \"*) ;; *) exit 3;; esac\ntip=\"\"; prev=\"\"\nfor a in \"$@\"; do if [ \"$prev\" = \"--remote-tip\" ]; then tip=\"$a\"; fi; prev=\"$a\"; done\ncase \"$tip\" in ????????*) ;; *) exit 4;; esac\necho '{\"ok\": false, \"code\": \"BUCKET_STRATEGY_GUARD_FORWARD_ONLY\", \"errors\": [\"commit: static/a.png\"]}'\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let repo = proj.join("repo");
        // Genuine upstream via a local bare remote (synthetic tracking refs
        // are rejected by rev-parse @{u}); the stub only asserts argv shape.
        let upstream = proj.join("upstream.git");
        std::process::Command::new("git")
            .args(["init", "-q", "--bare", upstream.to_str().unwrap()])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                repo.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["checkout", "-qb", "main"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("f"), "x").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(&repo)
            .status()
            .unwrap();
        // Publish so @{u} resolves (file protocol, offline); the stub
        // reports violations regardless of the true range content.
        std::process::Command::new("git")
            .args(["push", "-q", "-u", "origin", "main"])
            .current_dir(&repo)
            .status()
            .unwrap();
        let verdict = check_push_range(&repo, 10, None)
            .await
            .unwrap()
            .expect("managed repo");
        assert!(!verdict.ok);
        assert_eq!(verdict.code, "BUCKET_STRATEGY_GUARD_FORWARD_ONLY");
        assert!(verdict.errors.iter().any(|e| e.contains("static/a.png")));
    }

    #[tokio::test]
    async fn push_range_none_without_upstream() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = stub_guard(tmp.path(), "repo", "#!/bin/sh\nexit 0\n");
        std::fs::create_dir_all(&repo).unwrap();
        // Not a git repo at all: range unresolvable -> None (summary fallback).
        assert!(check_push_range(&repo, 10, None).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_env_override_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let err = check_staged_forward_only(tmp.path(), 10, Some("/nonexistent/guard.sh"))
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("points nowhere"));
    }
}
