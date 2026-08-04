//! File staging and path management — unstage, restore, blob detection.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

/// Unstage paths that match excluded directory patterns.
/// Returns the count of unstaged files.
pub(crate) async fn unstage_excluded_paths(
    repo: &Path,
    excluded_dir_names: &BTreeSet<String>,
) -> Result<usize> {
    let staged = super::staged_paths(repo).await?;
    let mut to_unstage = Vec::new();
    for path in staged {
        if !super::is_safe_git_path(&path) {
            eprintln!(
                "⚠️ skipping unsafe path {} in {}",
                path.display(),
                repo.display()
            );
            continue;
        }
        if is_excluded_change_path(&path, excluded_dir_names) {
            to_unstage.push(path);
        }
    }
    if to_unstage.is_empty() {
        return Ok(0);
    }
    for chunk in to_unstage.chunks(50) {
        let mut cmd = crate::policy::tokio_git_command();
        cmd.args(["reset", "-q", "HEAD", "--"])
            .current_dir(repo)
            .kill_on_drop(true);
        for path in chunk {
            cmd.arg(path);
        }
        // CHANGED 2026-07-21 (v0.112.33, audit M13/F2.4): require
        // exit 0 — the previous `.status().await?` ignored non-zero
        // exits (index.lock contention, pathspec errors) and the
        // caller's count claimed the paths were unstaged anyway.
        let status = cmd.status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "git reset HEAD -- ({} paths) failed in {}: exit {}",
                chunk.len(),
                repo.display(),
                status
            ));
        }
    }
    Ok(to_unstage.len())
}

/// Unstage files that exceed the max file size threshold.
/// Returns the count of unstaged files.
pub(crate) async fn unstage_oversized_paths(repo: &Path, max_bytes: u64) -> Result<usize> {
    let staged = super::staged_paths(repo).await?;
    let mut to_unstage = Vec::new();
    for path in staged {
        if !super::is_safe_git_path(&path) {
            eprintln!(
                "⚠️ skipping unsafe path {} in {}",
                path.display(),
                repo.display()
            );
            continue;
        }
        let full = repo.join(&path);
        if let Ok(meta) = tokio::fs::metadata(&full).await {
            if meta.len() > max_bytes {
                to_unstage.push(path);
            }
        }
    }
    if to_unstage.is_empty() {
        return Ok(0);
    }
    for chunk in to_unstage.chunks(50) {
        let mut cmd = crate::policy::tokio_git_command();
        cmd.args(["reset", "-q", "HEAD", "--"])
            .current_dir(repo)
            .kill_on_drop(true);
        for path in chunk {
            cmd.arg(path);
        }
        // CHANGED 2026-07-21 (v0.112.33, audit M13/F2.4): require
        // exit 0 (same rationale as `unstage_excluded_paths`).
        let status = cmd.status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "git reset HEAD -- ({} oversized paths) failed in {}: exit {}",
                chunk.len(),
                repo.display(),
                status
            ));
        }
    }
    Ok(to_unstage.len())
}

/// Detect large blobs ahead of the current position.
pub(crate) async fn detect_large_blobs_ahead(
    repo: &Path,
    min_bytes: u64,
) -> Result<Vec<(u64, String)>> {
    let r = repo.to_path_buf();
    let display = r.display().to_string();
    tokio::time::timeout(
        Duration::from_secs(60),
        tokio::task::spawn_blocking(move || -> Result<Vec<(u64, String)>> {
            let rev_list = crate::policy::std_git_command()
                .args(["rev-list", "--objects", "@{u}..HEAD"])
                .current_dir(&r)
                .output()
                .with_context(|| format!("failed rev-list in {}", r.display()))?;
            if !rev_list.status.success() {
                return Ok(Vec::new());
            }
            let mut cat_file_cmd = crate::policy::std_git_command();
            cat_file_cmd
                .args([
                    "cat-file",
                    "--batch-check=%(objectname) %(objecttype) %(objectsize) %(rest)",
                ])
                .current_dir(&r)
                .stdout(std::process::Stdio::piped());
            // CHANGED 2026-07-26 (v0.113.2, audit SYNC-H7): the
            // pre-fix code piped cat-file's stdin and wrote the
            // ENTIRE rev-list output into it BEFORE
            // `wait_with_output()` started draining stdout. With
            // thousands of objects ahead, cat-file's 64 KiB stdout
            // pipe fills (nobody reading), it stops reading stdin,
            // and the parent's `write_all` blocks forever — a
            // deadlock the 60s tokio timeout cannot cancel
            // (spawn_blocking thread + child leaked every repair
            // cycle), after which the caller's
            // `.unwrap_or_default()` silently disabled the 100 MiB
            // blob guard for exactly the repos that need it. Feed
            // stdin from a temp FILE instead — no pipe, no
            // deadlock. (Same incident class as the mod.rs
            // "CRITICAL deadlock avoidance" fix; that pattern was
            // never applied here.) NOTE: `tempfile` is a dev-only
            // dependency in this crate — use a std-only temp file
            // with a Drop-guard cleanup.
            let tmp_path = std::env::temp_dir().join(format!(
                "dracon-sync-blob-stdin-{}-{}.txt",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::write(&tmp_path, &rev_list.stdout)
                .with_context(|| format!("failed to write stdin tmpfile in {}", r.display()))?;
            struct StdinTmpCleanup(std::path::PathBuf);
            impl Drop for StdinTmpCleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_file(&self.0);
                }
            }
            let _tmp_cleanup = StdinTmpCleanup(tmp_path.clone());
            let stdin_fd = std::fs::File::open(&tmp_path)
                .with_context(|| format!("failed to reopen stdin tmpfile in {}", r.display()))?;
            let cat_file = cat_file_cmd
                .stdin(std::process::Stdio::from(stdin_fd))
                .spawn()
                .with_context(|| format!("failed cat-file in {}", r.display()))?;
            let output = cat_file.wait_with_output()?;
            if !output.status.success() {
                return Ok(Vec::new());
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut out: Vec<(u64, String)> = stdout
                .lines()
                .filter_map(|line| {
                    let mut parts = line.split_whitespace();
                    let _oid = parts.next()?;
                    let obj_type = parts.next()?;
                    let size_str = parts.next()?;
                    let path = parts.next()?;
                    if obj_type == "blob" {
                        let size = size_str.parse::<u64>().ok()?;
                        if size > min_bytes {
                            return Some((size, path.to_string()));
                        }
                    }
                    None
                })
                .collect();
            out.sort_by_key(|a| a.0);
            Ok(out)
        }),
    )
    .await
    .with_context(|| format!("timed out in detect_large_blobs_ahead for {}", display))?
    .with_context(|| format!("detect_large_blobs_ahead timed out (>60s) for {}", display))?
}

/// Get the top-level directory name from a path.
pub(crate) fn top_level_dir(path: &str) -> Option<String> {
    path.split('/').next().map(|s| s.to_string())
}

/// ADDED 2026-07-26 (v0.113.3, audit SYNC-H6): outcome of a real
/// history rewrite. Replaces the pre-fix `Option<String>` (a backup
/// BRANCH name — see the SYNC-H6 comment on `rewrite_ahead_paths`).
#[derive(Debug, Clone)]
pub(crate) struct RewriteOutcome {
    /// Path of the `git bundle` backup of the pre-rewrite HEAD.
    /// A bundle is not a ref, so filter-repo cannot rewrite or
    /// delete it (the pre-fix backup branch was rewritten along with
    /// everything else, preserving nothing).
    pub bundle_path: String,
    /// (full ref name, expected pre-rewrite sha) for the
    /// post-rewrite force push lease, captured from the pre-rewrite
    /// upstream tracking ref BEFORE filter-repo deleted `origin`.
    pub lease: Option<(String, String)>,
}

/// Rewrite ahead paths using git filter-repo or filter-branch.
/// Returns Some(RewriteOutcome) when history actually changed,
/// None if no paths to rewrite or the rewrite was a no-op.
///
/// F31 (2026-07-19): after a successful rewrite, check whether the
/// resulting HEAD actually differs from the backup branch. If the
/// rewrite was a no-op (e.g. the path glob didn't match anything
/// committed ahead of the remote), delete the backup branch to
/// avoid littering `git branch` output with empty `backup/pre-sync-*`
/// branches. The function signature is preserved: callers see
/// `Some(backup)` only when the rewrite actually changed history.
///
/// CHANGED 2026-07-26 (v0.113.3, audit SYNC-H6 — the F31 no-op
/// check made real rewrites indistinguishable from no-ops):
/// `git filter-repo --invert-paths --force` rewrites ALL refs,
/// including the `backup/pre-sync-*` branch created two statements
/// earlier — so the "backup" preserved nothing, the backup tree
/// ALWAYS equalled the rewritten HEAD tree, `rewrite_was_noop_
/// then_cleanup` reported every REAL rewrite as a no-op (deleting
/// the backup and returning None → caller never pushed), and
/// filter-repo also deleted the `origin` remote, so the next
/// cycle's auto-pull-on-reject merged the PRE-REWRITE history
/// back in — the >100 MiB blob returned to local history and was
/// pushed to all mirrors. The repair silently un-did itself.
/// Reproduced live during the audit. Now:
///  1. the backup is a `git bundle` FILE (not a ref) — filter-repo
///     cannot touch it;
///  2. filter-repo is limited to `--refs HEAD` (only the current
///     branch is rewritten);
///  3. the no-op check compares pre/post-rewrite HEAD SHAS, not
///     backup-tree vs HEAD-tree;
///  4. the pre-rewrite origin URL and upstream sha are captured
///     BEFORE the rewrite; origin is re-added afterwards (the
///     caller force-pushes with a lease anchored to that sha).
pub(crate) fn rewrite_ahead_paths(
    repo: &Path,
    paths_to_remove: &[String],
    backup_prefix: &str,
) -> Result<Option<RewriteOutcome>> {
    if paths_to_remove.is_empty() {
        return Ok(None);
    }

    // ADDED 2026-07-23 (v0.112.39, prevention #56): object-
    // completeness pre-flight. A history rewrite (filter-repo /
    // filter-branch) must not run on a damaged gitdir — if objects
    // referenced by main's history are MISSING from the object
    // store, the rewrite would produce (or preserve) history
    // referencing objects that don't exist anywhere. NOTE: this is
    // a cheap guard for a hypothetical class — the deathrun
    // investigation (2026-07-23) initially suspected the auto-repair
    // had broken history, but the corrected probe showed 0 missing
    // objects (a probe artifact). The guard is kept as cheap
    // insurance: if a genuinely damaged gitdir ever appears, we
    // refuse to rewrite it and alert instead of making it worse.
    let history = crate::report::probe_history(repo);
    if history.failed || history.missing_objects > 0 {
        let detail = if history.failed {
            "history probe failed (invalid HEAD/ref or timeout)".to_string()
        } else {
            format!(
                "{} objects referenced by main's history are missing from the object store",
                history.missing_objects
            )
        };
        return Err(anyhow::anyhow!(
            "refusing history rewrite in {}: {} (damaged gitdir) — restore from the forge or orphan-cutover first (backup not created)",
            repo.display(),
            detail
        ));
    }

    // Capture pre-rewrite state BEFORE filter-repo can destroy it:
    // HEAD sha (no-op check), origin URL (filter-repo DELETES the
    // origin remote), and the upstream lease anchor for the
    // post-rewrite force push.
    let pre_head = git_rev_parse(repo, "HEAD").ok_or_else(|| {
        anyhow::anyhow!("cannot resolve HEAD in {} — refusing rewrite", repo.display())
    })?;
    let origin_url = git_config_get(repo, "remote.origin.url");
    let lease: Option<(String, String)> =
        match (super::branch::current_branch(repo), git_rev_parse(repo, "@{u}")) {
            (Some(branch), Some(upstream_sha)) => {
                Some((format!("refs/heads/{}", branch), upstream_sha))
            }
            _ => None,
        };

    // Bundle backup (a FILE, not a ref) — immune to the rewrite.
    let bundle_name = format!(
        "{}-{}.bundle",
        backup_prefix.replace('/', "-"),
        crate::policy::timestamp_secs()
    );
    let bundle_dir = super::path_gitdir(repo).unwrap_or_else(|| repo.join(".git"));
    let bundle_path = bundle_dir.join(&bundle_name);
    let bundle_str = bundle_path.to_string_lossy().to_string();
    let create_backup = crate::policy::std_git_command()
        .args(["bundle", "create", &bundle_str, "HEAD"])
        .current_dir(repo)
        .status()
        .with_context(|| format!("failed backup bundle in {}", repo.display()))?;
    if !create_backup.success() {
        return Err(anyhow::anyhow!(
            "failed to create backup bundle {} in {}",
            bundle_str,
            repo.display()
        ));
    }

    let finish = |repo: &Path| -> Result<Option<RewriteOutcome>> {
        // Restore the origin remote if the rewrite deleted it
        // (filter-repo does this by design; filter-branch does not).
        if let Some(url) = &origin_url {
            if git_config_get(repo, "remote.origin.url").is_none() {
                let readd = crate::policy::std_git_command()
                    .args(["remote", "add", "origin", url])
                    .current_dir(repo)
                    .status();
                match readd {
                    Ok(s) if s.success() => {
                        eprintln!(
                            "🔧 re-added origin remote in {} (filter-repo removes it)",
                            repo.display()
                        );
                    }
                    _ => {
                        eprintln!(
                            "⚠️ failed to re-add origin remote in {} — restore manually: git remote add origin {}",
                            repo.display(),
                            url
                        );
                    }
                }
            }
        }
        // No-op check: pre vs post HEAD SHA (the pre-fix tree
        // compare against the rewritten backup was ALWAYS equal).
        let post_head = git_rev_parse(repo, "HEAD");
        if post_head.as_deref() == Some(pre_head.as_str()) {
            let _ = std::fs::remove_file(&bundle_path);
            return Ok(None);
        }
        Ok(Some(RewriteOutcome {
            bundle_path: bundle_str.clone(),
            lease: lease.clone(),
        }))
    };

    // Try git-filter-repo first (preferred, faster, actively maintained)
    let filter_repo_available = crate::policy::std_git_command()
        .args(["filter-repo", "--version"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if filter_repo_available {
        let mut args: Vec<String> = vec![
            "filter-repo".to_string(),
            "--invert-paths".to_string(),
            "--force".to_string(),
        ];
        for path in paths_to_remove {
            args.push("--path".to_string());
            args.push(path.clone());
        }
        // SYNC-H6: limit the rewrite to the current branch — the
        // pre-fix invocation rewrote ALL refs (including its own
        // backup branch).
        args.push("--refs".to_string());
        args.push("HEAD".to_string());
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let rewrite = crate::policy::std_git_command()
            .args(&args_ref)
            .current_dir(repo)
            .status()
            .with_context(|| format!("failed filter-repo in {}", repo.display()))?;
        if !rewrite.success() {
            return Err(anyhow::anyhow!(
                "filter-repo failed in {} (backup bundle: {})",
                repo.display(),
                bundle_str
            ));
        }
        return finish(repo);
    }

    let filter_branch_available = crate::policy::std_git_command()
        .args(["filter-branch", "--version"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if filter_branch_available {
        let args = build_filter_branch_args(paths_to_remove);
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let rewrite = crate::policy::std_git_command()
            .args(&args_ref)
            .current_dir(repo)
            .status()
            .with_context(|| format!("failed filter-branch in {}", repo.display()))?;
        if !rewrite.success() {
            return Err(anyhow::anyhow!(
                "filter-branch failed in {} (backup bundle: {})",
                repo.display(),
                bundle_str
            ));
        }
        return finish(repo);
    }

    Err(anyhow::anyhow!(
        "Neither git-filter-repo nor git-filter-branch available in {}. Install git-filter-repo (pip install git-filter-repo) or git-filter-branch to rewrite history (backup bundle: {})",
        repo.display(),
        bundle_str
    ))
}

/// Build the `git filter-branch` argv for the fallback rewrite path.
///
/// FIXED 2026-07-21 (v0.112.33, audit M12/F2.2): the previous argv
/// appended `paths_to_remove` as bare positional entries AFTER the
/// `--index-filter` string and before `--`. Two independent
/// breakages: (1) the index-filter command (`git rm -r --cached
/// --ignore-unmatch` with NO pathspec) dies with "fatal: No pathspec
/// was given" on every commit; (2) filter-branch forwards trailing
/// positionals to `git rev-list`, where `assets/big.mp4` is parsed
/// as a REVISION and dies with "bad revision". The fallback could
/// never succeed. The filter is now a single shell-quoted string
/// (paths inside the command), followed by `--` and an explicit
/// `--all` rev range (parity with the filter-repo arm, which also
/// rewrites all refs). Extracted as a pure function so the argv
/// shape is unit-testable without env shims.
fn build_filter_branch_args(paths_to_remove: &[String]) -> Vec<String> {
    let quoted: Vec<String> = paths_to_remove
        .iter()
        .map(|p| format!("'{}'", p.replace('\'', "'\\''")))
        .collect();
    let filter_expr = format!(
        "git rm -r --cached --ignore-unmatch -- {}",
        quoted.join(" ")
    );
    vec![
        "filter-branch".to_string(),
        "--force".to_string(),
        "--index-filter".to_string(),
        filter_expr,
        "--".to_string(),
        "--all".to_string(),
    ]
}

/// REMOVED 2026-07-26 (v0.113.3, audit SYNC-H6):
/// `rewrite_was_noop_then_cleanup` compared the backup branch's tree
/// against HEAD's tree — but filter-repo rewrote the backup branch
/// identically to HEAD, so the trees were ALWAYS equal and every
/// real rewrite was misreported as a no-op. Replaced by the
/// pre/post HEAD-sha compare inside `rewrite_ahead_paths`.
///
/// ADDED 2026-07-26 (v0.113.3): `git rev-parse <rev>` → trimmed sha.
fn git_rev_parse(repo: &Path, rev: &str) -> Option<String> {
    let out = crate::policy::std_git_command()
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

/// ADDED 2026-07-26 (v0.113.3): `git config --get <key>` → value.
fn git_config_get(repo: &Path, key: &str) -> Option<String> {
    let out = crate::policy::std_git_command()
        .args(["config", "--get", key])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Restore paths from the index to the working tree.
pub(crate) async fn restore_paths(repo: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    // F32 (2026-07-18): each path must be a valid git path (no
    // `..`, no absolute path, no NUL) before we hand it to git. The
    // sibling `unstage_paths` function already gates on this helper;
    // restore_paths did not.
    for p in paths {
        if !super::is_safe_git_path(std::path::Path::new(p)) {
            anyhow::bail!("restore_paths: refusing unsafe path '{}'", p);
        }
    }
    let mut args = vec![
        "restore".to_string(),
        "--staged".to_string(),
        "--worktree".to_string(),
        "--".to_string(),
    ];
    args.extend(paths.iter().cloned());
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    if super::run_git_with_timeout(repo, &args_ref, 30, "restore")
        .await
        .is_ok()
    {
        return Ok(());
    }

    let mut reset: Vec<String> = Vec::new();
    reset.push("reset".to_string());
    reset.push("HEAD".to_string());
    reset.push("--".to_string());
    reset.extend(paths.iter().cloned());
    let reset_ref: Vec<&str> = reset.iter().map(|s| s.as_str()).collect();
    if let Err(e) = super::run_git_with_timeout(repo, &reset_ref, 30, "reset").await {
        eprintln!("⚠️ git reset fallback failed for {}: {}", repo.display(), e);
        return Err(anyhow::anyhow!(
            "restore failed: git restore failed and reset fallback also failed: {}",
            e
        ));
    }
    for path in paths {
        let checkout_args = ["checkout", "--", path];
        if let Err(e) = super::run_git_with_timeout(repo, &checkout_args, 30, "checkout").await {
            eprintln!(
                "⚠️ git checkout failed for {} in {}: {}",
                path,
                repo.display(),
                e
            );
        }
    }
    Ok(())
}

fn is_excluded_change_path(path: &Path, excluded_dir_names: &BTreeSet<String>) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|c| excluded_dir_names.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::create_test_repo;

    /// F31 (2026-07-19): `rewrite_ahead_paths` must delete the backup
    /// branch when the rewrite was a no-op (HEAD tree == backup tree).
    #[test]
    fn test_f31_noop_rewrite_deletes_backup_branch() {
        if !crate::git::ops::filter_repo_available_for_tests() {
            eprintln!("filter-repo not installed; skipping");
            return;
        }
        let repo = create_test_repo();
        let pre = crate::policy::std_git_command()
            .args(["rev-parse", "HEAD^{tree}"])
            .current_dir(repo.as_path())
            .output()
            .expect("rev-parse");
        let pre_hash = String::from_utf8_lossy(&pre.stdout).trim().to_string();

        // Empty paths_to_remove means rewrite_ahead_paths short-circuits to Ok(None).
        let r = rewrite_ahead_paths(repo.as_path(), &[], "test/backup");
        assert!(r.is_ok());
        assert!(r.unwrap().is_none());

        // Now test with a path that doesn't match anything in HEAD.
        // The commit tree won't change; backup should be deleted.
        let r2 = rewrite_ahead_paths(
            repo.as_path(),
            &["nonexistent/should/not/match.xyz".to_string()],
            "test/backup",
        );
        assert!(r2.is_ok());
        assert!(r2.unwrap().is_none());

        // Verify no backup refs AND no leftover bundle files (the
        // no-op path removes the bundle).
        let branches = crate::policy::std_git_command()
            .args(["branch", "--list"])
            .current_dir(repo.as_path())
            .output()
            .expect("git branch");
        let stdout = String::from_utf8_lossy(&branches.stdout);
        assert!(
            !stdout.contains("test/backup-"),
            "expected no backup branches after no-op rewrite; got: {}",
            stdout
        );
        let bundles = std::fs::read_dir(repo.as_path().join(".git"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bundle"))
            .count();
        assert_eq!(bundles, 0, "no-op rewrite must not leave bundle files");

        // HEAD tree unchanged.
        let post = crate::policy::std_git_command()
            .args(["rev-parse", "HEAD^{tree}"])
            .current_dir(repo.as_path())
            .output()
            .expect("rev-parse");
        let post_hash = String::from_utf8_lossy(&post.stdout).trim().to_string();
        assert_eq!(pre_hash, post_hash);
    }

    /// ADDED 2026-07-26 (v0.113.3, audit SYNC-H6): a REAL rewrite
    /// must (a) return Some(outcome) — the pre-fix code misreported
    /// every real rewrite as a no-op because filter-repo rewrote the
    /// backup branch along with HEAD, (b) leave a bundle containing
    /// the PRE-rewrite HEAD, (c) preserve/re-add the origin remote
    /// (filter-repo deletes it), (d) capture the force-push lease
    /// anchor from the pre-rewrite upstream, and (e) rewrite ONLY
    /// HEAD (--refs HEAD), leaving other branches alone.
    #[test]
    fn test_real_rewrite_returns_outcome_with_bundle_and_lease() {
        if !crate::git::ops::filter_repo_available_for_tests() {
            eprintln!("filter-repo not installed; skipping");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@test"],
            vec!["config", "user.name", "test"],
        ] {
            let s = crate::policy::std_git_command()
                .args(&args)
                .current_dir(&repo_path)
                .status()
                .unwrap();
            assert!(s.success(), "git {:?} failed", args);
        }
        // Commit a large-blob stand-in and a normal file.
        std::fs::create_dir_all(repo_path.join("assets")).unwrap();
        std::fs::write(repo_path.join("assets/big.bin"), vec![7u8; 2048]).unwrap();
        std::fs::write(repo_path.join("keep.txt"), "keep\n").unwrap();
        for args in [vec!["add", "-A"], vec!["commit", "-q", "-m", "c1"]] {
            let s = crate::policy::std_git_command()
                .args(&args)
                .current_dir(&repo_path)
                .status()
                .unwrap();
            assert!(s.success(), "git {:?} failed", args);
        }
        // Origin (a bare sibling) + upstream tracking.
        let bare = tmp.path().join("origin.git");
        let s = crate::policy::std_git_command()
            .args(["init", "-q", "--bare"])
            .arg(&bare)
            .status()
            .unwrap();
        assert!(s.success());
        for args in [
            vec!["remote", "add", "origin", bare.to_str().unwrap()],
            vec!["config", "branch.main.remote", "origin"],
            vec!["config", "branch.main.merge", "refs/heads/main"],
        ] {
            let s = crate::policy::std_git_command()
                .args(&args)
                .current_dir(&repo_path)
                .status()
                .unwrap();
            assert!(s.success(), "git {:?} failed", args);
        }
        // Simulate the already-pushed state: set the remote-tracking
        // ref directly (a real push would trip the global warden
        // test-identity pre-push guard on this test-identity repo).
        let s = crate::policy::std_git_command()
            .args(["update-ref", "refs/remotes/origin/main", "HEAD"])
            .current_dir(&repo_path)
            .status()
            .unwrap();
        assert!(s.success());
        // A side branch that must SURVIVE the rewrite untouched.
        let s = crate::policy::std_git_command()
            .args(["branch", "side"])
            .current_dir(&repo_path)
            .status()
            .unwrap();
        assert!(s.success());
        let side_pre = git_rev_parse(&repo_path, "side").unwrap();
        let pre_head = git_rev_parse(&repo_path, "HEAD").unwrap();

        let r = rewrite_ahead_paths(&repo_path, &["assets".to_string()], "backup/test");
        let outcome = r
            .expect("rewrite must succeed")
            .expect("a REAL rewrite must return Some(outcome) — SYNC-H6 regression");

        // HEAD changed; the path is gone from history.
        let post_head = git_rev_parse(&repo_path, "HEAD").unwrap();
        assert_ne!(pre_head, post_head);
        // Bundle exists and contains the PRE-rewrite HEAD.
        assert!(std::path::Path::new(&outcome.bundle_path).exists());
        let verify = crate::policy::std_git_command()
            .args(["bundle", "verify", &outcome.bundle_path])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let verify_out = String::from_utf8_lossy(&verify.stderr).to_string()
            + &String::from_utf8_lossy(&verify.stdout);
        assert!(
            verify_out.contains(&pre_head),
            "bundle must contain pre-rewrite HEAD {}; got: {}",
            pre_head,
            verify_out
        );
        // Origin remote preserved/re-added (filter-repo deletes it).
        assert!(git_config_get(&repo_path, "remote.origin.url").is_some());
        // Lease anchor = pre-rewrite upstream sha.
        let (lease_ref, lease_sha) = outcome.lease.expect("lease must be captured");
        assert_eq!(lease_ref, "refs/heads/main");
        assert_eq!(lease_sha, pre_head);
        // Side branch untouched by the rewrite (--refs HEAD).
        assert_eq!(git_rev_parse(&repo_path, "side").unwrap(), side_pre);
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M12/F2.2): pins the
    /// filter-branch fallback argv shape — paths must be INSIDE the
    /// single quoted `--index-filter` string (never bare positionals,
    /// which filter-branch forwards to `git rev-list` where a path
    /// like `assets/big.mp4` dies as a "bad revision"), followed by
    /// `--` and an explicit `--all` rev range.
    #[test]
    fn test_build_filter_branch_args_shape() {
        let args = build_filter_branch_args(&[
            "assets/big.mp4".to_string(),
            "docs/my file.pdf".to_string(),
        ]);
        assert_eq!(args[0], "filter-branch");
        assert_eq!(args[1], "--force");
        assert_eq!(args[2], "--index-filter");
        let filter = &args[3];
        assert!(
            filter.starts_with("git rm -r --cached --ignore-unmatch -- "),
            "index-filter must contain the pathspec inside the command: {}",
            filter
        );
        assert!(filter.contains("'assets/big.mp4'"));
        // Space-containing path is single-quoted so the shell keeps
        // it as ONE argument.
        assert!(filter.contains("'docs/my file.pdf'"));
        // No bare positional paths between the filter string and `--`.
        assert_eq!(args[4], "--");
        assert_eq!(args[5], "--all");
        assert_eq!(args.len(), 6);
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M12/F2.2): a path with an
    /// embedded single quote is escaped (`'\''`) so the shell can't
    /// break out of the quoted string.
    #[test]
    fn test_build_filter_branch_args_escapes_single_quotes() {
        let args = build_filter_branch_args(&["we'ird.bin".to_string()]);
        assert!(args[3].contains("'we'\\''ird.bin'"), "got: {}", args[3]);
    }
}
