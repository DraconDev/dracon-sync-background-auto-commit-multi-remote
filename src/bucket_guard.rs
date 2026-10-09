//! Forward-only bucket guard for the daemon auto-commit path.
//!
//! Background (goal 20261009193116-1ess3v, 2026-10-09): the platform
//! `.githooks/pre-commit` bucket guard runs the staged forward-only check
//! (`BUCKET_STRATEGY_GUARD_FORWARD_ONLY`) for human commits, but daemon
//! auto-commits go through libgit2 (`dracon-git` `GitService::commit`),
//! which never executes hooks — the CLI fallback even passes `--no-verify`.
//! That is how hellhunter `f5d55b5` (27 deleted protected assets) and
//! deathrun `30352ba` (4 deleted webp backdrops) were committed silently and
//! only detonated at push time, wedging both repos 20+ commits deep.
//!
//! This module runs the same staged check the hook runs, from inside the
//! daemon, between staging and committing. Repos without a discoverable
//! guard script are unaffected (the check is layout-gated, not policy
//! gated). Violations — and guard infrastructure failures — block the commit
//! with the index intact, mirroring the hook's fail-closed semantics.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Basename the hook wrapper looks for while walking checkout ancestry.
const GUARD_SCRIPT_RELATIVE: &str = "web/scripts/bucket-strategy-guard.sh";

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
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return None,
        }
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

/// Run the staged forward-only check for `repo`.
///
/// * No discoverable guard script → `Ok(())` (repo is not bucket-managed).
/// * Guard reports `ok: false` → `Err` carrying the violation code and paths;
///   the caller must leave the index intact and skip the commit.
/// * Guard infrastructure failure (missing override target, spawn failure,
///   timeout, unparseable output) → `Err` (fail closed, mirroring the hook).
pub(crate) async fn check_staged_forward_only(
    repo: &Path,
    timeout_secs: u64,
    env_override: Option<&str>,
) -> Result<()> {
    let guard = match discover_guard(repo, env_override) {
        None => return Ok(()),
        Some(path) => path,
    };
    let guard_display = guard.display().to_string();
    if guard_display.starts_with("\0missing-override:") {
        anyhow::bail!(
            "bucket guard override points nowhere: {}",
            guard_display.trim_start_matches('\0')
        );
    }
    let mut cmd = tokio::process::Command::new(&guard);
    cmd.args(["--staged", "--no-size", "--json"])
        .current_dir(repo);
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
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs.max(1)),
        cmd.output(),
    )
    .await
    .with_context(|| format!("bucket guard timed out after {timeout_secs}s: {guard_display}"))?
    .with_context(|| format!("bucket guard failed to spawn: {guard_display}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Option<serde_json::Value> = serde_json::from_str(&stdout).ok();
    match parsed {
        Some(value) if value.get("ok") == Some(&serde_json::Value::Bool(true)) => Ok(()),
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
            let stderr_tail: String =
                String::from_utf8_lossy(&output.stderr).trim().chars().take(400).collect();
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
        assert!(check_staged_forward_only(tmp.path(), 10, None).await.is_ok());
    }

    #[tokio::test]
    async fn guard_ok_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = stub_guard(tmp.path(), "repo", "#!/bin/sh\necho '{\"ok\": true, \"code\": \"OK\"}'\n");
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
        let err = check_staged_forward_only(&repo, 10, None).await.unwrap_err();
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
    async fn missing_env_override_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let err = check_staged_forward_only(tmp.path(), 10, Some("/nonexistent/guard.sh"))
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("points nowhere"));
    }
}
