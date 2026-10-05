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
            // R4-SC-10: :(literal) keeps glob metacharacters in
            // filenames from acting as pathspecs (over-unstaging).
            cmd.arg(super::literal_pathspec(path));
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

/// Size the staged (index) blobs for `paths` via one
/// `git cat-file --batch-check` call per chunk. Returns `(path, bytes)`
/// with `None` for entries that cannot be measured. Chunked at 1000
/// paths so the stdin payload stays under the 64 KiB pipe (deadlock
/// avoidance, same rationale as `blob_size_sum`).
async fn staged_blob_sizes(
    repo: &Path,
    shas: &[(std::path::PathBuf, String)],
) -> Vec<(std::path::PathBuf, Option<u64>)> {
    use tokio::io::AsyncWriteExt;
    let mut out = Vec::with_capacity(shas.len());
    for chunk in shas.chunks(1000) {
        let input: String = chunk.iter().map(|(_, sha)| format!("{sha}\n")).collect();
        let mut cmd = crate::policy::tokio_git_command();
        cmd.args([
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ])
        .current_dir(repo)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => {
                out.extend(chunk.iter().map(|(p, _)| (p.clone(), None)));
                continue;
            }
        };
        // Bounded write (< 64 KiB): safe to write fully, then read.
        let write_ok = if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).await.is_ok()
        } else {
            false
        };
        drop(child.stdin.take());
        let output = child.wait_with_output().await;
        let stdout = output.map(|o| o.stdout).unwrap_or_default();
        let lines: Vec<&str> = std::str::from_utf8(&stdout).unwrap_or("").lines().collect();
        for (i, (path, _)) in chunk.iter().enumerate() {
            let size = if !write_ok {
                None
            } else {
                lines.get(i).and_then(|line| {
                    let mut parts = line.split_whitespace();
                    let _name = parts.next()?;
                    let ty = parts.next()?;
                    if ty == "missing" {
                        return None;
                    }
                    parts.next()?.parse::<u64>().ok()
                })
            };
            out.push((path.clone(), size));
        }
    }
    out
}

/// Measure staged-blob sizes for `paths` via `ls-files -s` +
/// `cat-file --batch-check` (M4 primitive, extracted 2026-10-03 for
/// reuse by the bootstrap sweep — R3-M1). Returns measured bytes per
/// path; `None` marks entries that could not be measured (fail-closed
/// candidates — the caller unstages them). Paths absent from the index
/// (staged deletions) are omitted: removals shrink the repo and skip
/// the size gate.
pub(crate) async fn staged_blob_sizes_for(
    repo: &Path,
    paths: Vec<std::path::PathBuf>,
) -> Result<std::collections::BTreeMap<std::path::PathBuf, Option<u64>>> {
    let mut candidates: Vec<std::path::PathBuf> = paths
        .into_iter()
        .filter(|path| {
            if !super::is_safe_git_path(path) {
                eprintln!(
                    "⚠️ skipping unsafe path {} in {}",
                    path.display(),
                    repo.display()
                );
                return false;
            }
            true
        })
        .collect();
    candidates.sort();
    // Map index entries (sha per path) via ls-files; :(literal) keeps
    // glob metacharacters in filenames from acting as pathspecs.
    let mut indexed: Vec<(std::path::PathBuf, String)> = Vec::new();
    let mut skipped: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    for chunk in candidates.chunks(500) {
        let mut cmd = crate::policy::tokio_git_command();
        cmd.args(["ls-files", "-s", "-z", "--"])
            .current_dir(repo)
            .kill_on_drop(true);
        for path in chunk {
            cmd.arg(super::literal_pathspec(path));
        }
        let output = cmd
            .output()
            .await
            .with_context(|| format!("git ls-files -s failed in {}", repo.display()))?;
        if !output.status.success() {
            // Fail closed: without the index listing we cannot prove
            // any entry is small — unstage the whole chunk.
            eprintln!(
                "⚠️ git ls-files -s failed in {}; unstaging {} staged path(s) blind",
                repo.display(),
                chunk.len()
            );
            for path in chunk {
                indexed.push((path.clone(), String::new()));
            }
            continue;
        }
        // Record format: "<mode> <sha> <stage>\t<path>\0" (raw bytes).
        let mut seen: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        for record in output.stdout.split(|b| *b == 0) {
            if record.is_empty() {
                continue;
            }
            let Some(tab) = record.iter().position(|b| *b == b'\t') else {
                continue;
            };
            let (meta, name) = (&record[..tab], &record[tab + 1..]);
            let mut meta_parts = meta.split(|b| *b == b' ');
            let mode = meta_parts.next();
            // FIX 2026-10-05 (hotfix 0.113.95): gitlinks (mode 160000)
            // stage a 40-hex nested pointer, never blob content — there
            // is nothing to size-gate, and the nested SHA never resolves
            // in the parent store (cat-file reports missing). Measuring
            // them fails closed and unstages EVERY gitlink each cycle
            // (fleet-wide parent-pointer freeze from the 0.113.94
            // deploy). Skip like deletions: pointer swaps add no bytes.
            // (Must join `skipped`, not just skip `indexed` — otherwise
            // the path lands in `blind` below and is unstaged anyway.)
            if mode == Some(b"160000".as_slice()) {
                skipped.insert(std::path::PathBuf::from(
                    String::from_utf8_lossy(name).into_owned(),
                ));
                continue;
            }
            let Some(sha) = meta_parts.next() else {
                continue;
            };
            if sha.len() != 40 && sha.len() != 64 {
                continue;
            }
            seen.insert(name.to_vec());
            indexed.push((
                std::path::PathBuf::from(String::from_utf8_lossy(name).into_owned()),
                String::from_utf8_lossy(sha).into_owned(),
            ));
        }
        // Any chunk path missing from the listing is either a staged
        // deletion (absent by design — commits a removal, never new
        // bytes, so it skips the size gate) or an index race that
        // already resolved itself (nothing staged — nothing to unstage).
        // Both outcomes skip; only measured-or-blind paths proceed.
        for path in chunk {
            let key = path.as_os_str().as_encoded_bytes();
            if seen.contains(key) {
                continue;
            }
            skipped.insert(path.clone());
        }
    }
    let mut sizes: std::collections::BTreeMap<std::path::PathBuf, Option<u64>> =
        std::collections::BTreeMap::new();
    // Empty-sha sentinels from a failed ls-files chunk fail closed below.
    let measurable: Vec<(std::path::PathBuf, String)> = indexed
        .into_iter()
        .filter(|(_, sha)| !sha.is_empty())
        .collect();
    let blind: Vec<std::path::PathBuf> = {
        let measured: std::collections::BTreeSet<_> =
            measurable.iter().map(|(p, _)| p.clone()).collect();
        candidates
            .iter()
            .filter(|p| !measured.contains(*p) && !skipped.contains(*p))
            .cloned()
            .collect()
    };
    for path in blind {
        sizes.insert(path, None);
    }
    for (path, size) in staged_blob_sizes(repo, &measurable).await {
        sizes.insert(path, size);
    }
    Ok(sizes)
}

/// Unstage files that exceed the max file size threshold.
/// Returns the count of unstaged files.
pub(crate) async fn unstage_oversized_paths(repo: &Path, max_bytes: u64) -> Result<usize> {
    let staged = super::staged_paths(repo).await?;
    // FIX (audit M4, 2026-10-02): size the STAGED blob (index), not the
    // worktree file. Statting the worktree lets stage-large-then-truncate
    // commit a >max blob past the gate (TOCTOU), and stat errors failed
    // open. Paths absent from the index are staged deletions (or already
    // gone): deletions shrink the repo and need no gate, so only
    // present index entries are measured. Unmeasurable entries fail
    // closed (unstaged) rather than committing blind.
    let sizes = staged_blob_sizes_for(repo, staged.into_iter().collect()).await?;
    let mut to_unstage = Vec::new();
    for (path, size) in &sizes {
        match size {
            Some(n) if *n > max_bytes => to_unstage.push(path.clone()),
            Some(_) => {}
            None => {
                eprintln!(
                    "⚠️ cannot measure staged blob for {} in {}; unstaging (fail closed)",
                    path.display(),
                    repo.display()
                );
                to_unstage.push(path.clone());
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
            // R4-SC-10: :(literal) — see `unstage_excluded_paths`.
            cmd.arg(super::literal_pathspec(path));
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
                // FIXED 2026-10-03 (audit R4-SC-14): the old
                // `Ok(vec![])` silently disabled the >100 MiB rewrite
                // guard for exactly the repos that need it (no
                // upstream, transient rev-list errors). Propagate —
                // the caller decides (loud incident vs expected skip).
                // Deliberately no whole-branch fallback: it would flag
                // already-published blobs and the caller would rewrite
                // published history to remove them.
                return Err(anyhow::anyhow!(
                    "rev-list --objects @{{u}}..HEAD failed in {}: {}",
                    r.display(),
                    String::from_utf8_lossy(&rev_list.stderr).trim()
                ));
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
            //
            // FIXED 2026-09-27 (audit F89): the pre-fix name was
            // `pid` + nanoseconds and the file was created with
            // `std::fs::write` (O_CREAT|O_TRUNC, follows symlinks,
            // mode from umask) in the world-writable `temp_dir()`. A
            // local attacker who won the name race got an arbitrary
            // file truncated/overwritten as the daemon user. It is now
            // created with `create_new(true)` (O_EXCL|O_CREAT) and
            // mode 0600, retried on a name collision, and opened for
            // writing through the same handle the reader later uses.
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt as _;
            let tmp_dir = std::env::temp_dir();
            let mut tmp_path = std::path::PathBuf::new();
            let mut stdin_file = None;
            for attempt in 0..8u32 {
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let candidate = tmp_dir.join(format!(
                    "dracon-sync-blob-stdin-{}-{}-{}.txt",
                    std::process::id(),
                    nonce,
                    attempt
                ));
                match std::fs::OpenOptions::new()
                    // REGRESSION FIXED 2026-09-27 (audit rework round 3):
                    // the F89 fix opened this handle WRITE-ONLY and handed
                    // the raw fd to `Stdio::from(...)` for the
                    // `git cat-file --batch-check` child. Stdio::from(File)
                    // is a raw fd hand-off, NOT a reopen — so the child
                    // inherited an O_WRONLY fd 0 and could never read the
                    // object list. cat-file exits 0 with no output, the
                    // code took the silent `Ok(Vec::new())` path, and
                    // `detect_large_blobs_ahead` always reported nothing:
                    // the 100 MiB push guard was silently dead. Measured on
                    // this repo: the write-only sequence yielded 0 records
                    // where the previous read-only reopen yielded 6440.
                    // The handle must be READ-AND-WRITE: we write the
                    // rev-list through it, then the child reads it.
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&candidate)
                {
                    Ok(handle) => {
                        tmp_path = candidate;
                        stdin_file = Some(handle);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => {
                        return Err(e).with_context(|| {
                            format!("failed to create stdin tmpfile in {}", r.display())
                        });
                    }
                }
            }
            let mut stdin_file = stdin_file
                .with_context(|| format!("failed to create stdin tmpfile in {}", r.display()))?;
            stdin_file
                .write_all(&rev_list.stdout)
                .with_context(|| format!("failed to write stdin tmpfile in {}", r.display()))?;
            stdin_file
                .flush()
                .with_context(|| format!("failed to flush stdin tmpfile in {}", r.display()))?;
            // REWIND before handing the fd to the child. The write above
            // advanced the file offset, and `Stdio::from(File)` hands the
            // child the SAME descriptor, which shares that offset — so the
            // child would start reading at EOF and see nothing. The
            // pre-F89 code called `File::open(...)` again, which created a
            // fresh descriptor at offset 0; replacing that reopen with a
            // bare O_EXCL create lost the rewind and silently disabled the
            // guard. Seeking to 0 keeps the O_EXCL + 0600 hardening AND
            // restores the rewind.
            use std::io::Seek as _;
            stdin_file
                .seek(std::io::SeekFrom::Start(0))
                .with_context(|| format!("failed to rewind stdin tmpfile in {}", r.display()))?;
            struct StdinTmpCleanup(std::path::PathBuf);
            impl Drop for StdinTmpCleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_file(&self.0);
                }
            }
            let _tmp_cleanup = StdinTmpCleanup(tmp_path.clone());
            let stdin_fd = stdin_file;
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
                .filter_map(|line| parse_large_blob_record(line, min_bytes))
                .collect();
            out.sort_by_key(|a| a.0);
            Ok(out)
        }),
    )
    .await
    .with_context(|| format!("timed out in detect_large_blobs_ahead for {}", display))?
    .with_context(|| format!("detect_large_blobs_ahead timed out (>60s) for {}", display))?
}

/// Parse one `cat-file --batch-check` record while preserving spaces in the
/// path returned via `%(rest)`. Splitting all fields on whitespace truncated
/// `models/my large.bin` to `models/my`, which could hide a large blob from
/// the rewrite guard.
fn parse_large_blob_record(line: &str, min_bytes: u64) -> Option<(u64, String)> {
    let mut fields = line.splitn(4, ' ');
    let _oid = fields.next()?;
    let obj_type = fields.next()?;
    let size_str = fields.next()?;
    let path = fields.next()?.to_string();
    if obj_type != "blob" || path.is_empty() {
        return None;
    }
    let size = size_str.parse::<u64>().ok()?;
    (size > min_bytes).then_some((size, path))
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
        anyhow::anyhow!(
            "cannot resolve HEAD in {} — refusing rewrite",
            repo.display()
        )
    })?;
    let origin_url = git_config_get(repo, "remote.origin.url");
    let lease: Option<(String, String)> = match (
        super::branch::current_branch(repo),
        git_rev_parse(repo, "@{u}"),
    ) {
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
/// `--all` rev range. Extracted as a pure function so the argv
/// shape is unit-testable without env shims.
///
/// FIXED 2026-09-27 (audit F88): the rev range was left as `--all`
/// under a comment claiming "parity with the filter-repo arm, which
/// also rewrites all refs". That stopped being true at SYNC-H6
/// (v0.113.3), which limited the filter-repo arm to `--refs HEAD`.
/// The caller only force-pushes the CURRENT branch, so on the
/// fallback path every other local branch was silently rewritten and
/// never published — it diverged from its remote with no incident
/// recorded anywhere. Both arms now rewrite exactly the same rev as
/// the caller will publish: HEAD.
fn build_filter_branch_args(paths_to_remove: &[String]) -> Vec<String> {
    // R4-SC-10: :(literal) inside the shell quotes — the index-filter's
    // `git rm` would otherwise read glob metacharacters as pathspecs
    // (over-removal from rewritten history).
    let quoted: Vec<String> = paths_to_remove
        .iter()
        .map(|p| format!("':(literal){}'", p.replace('\'', "'\\''")))
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
        // Same rev as the filter-repo arm's `--refs HEAD`. NEVER `--all`:
        // the caller force-pushes only the current branch, so rewriting
        // every ref would silently diverge the unpublished ones.
        "HEAD".to_string(),
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
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
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
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
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
    // R4-SC-10: :(literal) on every pathspec below — glob
    // metacharacters in filenames must not act as pathspecs.
    let literal: Vec<String> = super::literal_pathspecs(paths);
    let mut args = vec![
        "restore".to_string(),
        "--staged".to_string(),
        "--worktree".to_string(),
        "--".to_string(),
    ];
    args.extend(literal.iter().cloned());
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
    reset.extend(literal.iter().cloned());
    let reset_ref: Vec<&str> = reset.iter().map(|s| s.as_str()).collect();
    if let Err(e) = super::run_git_with_timeout(repo, &reset_ref, 30, "reset").await {
        eprintln!("⚠️ git reset fallback failed for {}: {}", repo.display(), e);
        return Err(anyhow::anyhow!(
            "restore failed: git restore failed and reset fallback also failed: {}",
            e
        ));
    }
    for path in &literal {
        let checkout_args = ["checkout", "--", path.as_str()];
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
    use crate::test_helpers::{create_test_repo, test_commit_cmd, test_git_cmd};

    /// Gitlinks stage a 40-hex pointer, never blob content: the size
    /// gate must exempt them. FIX 2026-10-05 (hotfix 0.113.95): the
    /// R4-SC-05 post-stage sweep ran the blob gate after gitlink
    /// staging; nested SHAs never resolve in the parent store, so
    /// EVERY gitlink fleet-wide was unstaged each cycle (fail closed)
    /// and no parent pointer advanced from the 0.113.94 deploy until
    /// this fix.
    #[tokio::test]
    async fn test_unstage_oversized_paths_exempts_gitlinks() {
        let repo = create_test_repo();
        // Nested SHA absent from this store, exactly like production.
        let nested_sha = "7c04c6393d44b64bea706e6ecad2d9c166b95320";
        let out = test_git_cmd()
            .args([
                "update-index",
                "--cacheinfo",
                &format!("160000,{nested_sha},nested"),
            ])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "cacheinfo staging must succeed");

        let n = unstage_oversized_paths(&repo, 1024).await.unwrap();

        assert_eq!(n, 0, "gitlink must survive the size gate");
        let listed = test_git_cmd()
            .args(["ls-files", "-s", "nested"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout);
        assert!(
            listed.starts_with("160000"),
            "gitlink must stay staged, index: {listed}"
        );
    }

    #[tokio::test]
    async fn test_unstage_oversized_paths_measures_staged_blob_not_worktree() {
        // FIX (audit M4, 2026-10-02): stage-large-then-truncate must not
        // bypass the gate — the staged blob is measured, not the
        // worktree file. Small staged files stay; staged deletions of
        // large files stay (removals shrink the repo).
        let repo = create_test_repo();
        // Case 3 setup first (its commit must not sweep the other cases):
        // staged deletion of a large file stays staged.
        std::fs::write(repo.join("gone.bin"), vec![b'y'; 2048]).unwrap();
        test_git_cmd()
            .args(["add", "gone.bin"])
            .current_dir(&repo)
            .output()
            .unwrap();
        test_commit_cmd()
            .args(["-m", "add gone"])
            .current_dir(&repo)
            .output()
            .unwrap();
        test_git_cmd()
            .args(["rm", "-q", "gone.bin"])
            .current_dir(&repo)
            .output()
            .unwrap();
        // Case 1: 2 KiB staged blob, worktree truncated after staging.
        std::fs::write(repo.join("big.bin"), vec![b'x'; 2048]).unwrap();
        test_git_cmd()
            .args(["add", "big.bin"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("big.bin"), b"tiny").unwrap();
        // Case 2: small staged file stays staged.
        std::fs::write(repo.join("small.txt"), b"small\n").unwrap();
        test_git_cmd()
            .args(["add", "small.txt"])
            .current_dir(&repo)
            .output()
            .unwrap();

        let n = unstage_oversized_paths(&repo, 1024).await.unwrap();
        assert_eq!(
            n, 1,
            "only the large staged blob must be unstaged (deletion + small file stay)"
        );
        let cached = test_git_cmd()
            .args(["diff", "--cached", "--name-only"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let cached = String::from_utf8_lossy(&cached.stdout);
        assert!(
            !cached.contains("big.bin"),
            "large blob must be unstaged, cached: {cached}"
        );
        assert!(
            cached.contains("small.txt"),
            "small file must stay staged, cached: {cached}"
        );
        assert!(
            cached.contains("gone.bin"),
            "staged deletion must stay staged, cached: {cached}"
        );
    }

    #[tokio::test]
    async fn test_unstage_oversized_paths_quotes_glob_metachars() {
        // ADDED 2026-10-03 (audit R4-SC-10): the reset pathspec must
        // be :(literal)-quoted — an unquoted `big[0].bin` pathspec
        // also matches the staged sibling `big0.bin` (over-unstaging).
        let repo = create_test_repo();
        std::fs::write(repo.join("big[0].bin"), vec![b'x'; 2048]).unwrap();
        std::fs::write(repo.join("big0.bin"), b"small\n").unwrap();
        test_git_cmd()
            .args(["add", "--", "big[0].bin", "big0.bin"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let n = unstage_oversized_paths(&repo, 1024).await.unwrap();
        assert_eq!(n, 1, "only the large blob must be unstaged");
        let cached = test_git_cmd()
            .args(["diff", "--cached", "--name-only"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let cached = String::from_utf8_lossy(&cached.stdout);
        assert!(
            !cached.contains("big[0].bin"),
            "large blob must be unstaged, cached: {cached}"
        );
        assert!(
            cached.contains("big0.bin"),
            "glob-matching sibling must STAY staged, cached: {cached}"
        );
    }

    #[test]
    fn large_blob_record_preserves_spaces_in_path() {
        assert_eq!(
            parse_large_blob_record(
                "0123456789abcdef blob 123456 models/my large model.onnx",
                100_000
            ),
            Some((123456, "models/my large model.onnx".to_string()))
        );
        assert_eq!(
            parse_large_blob_record("0123456789abcdef blob 99 models/small.bin", 100),
            None
        );
    }

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
            "a[0].bin".to_string(),
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
        // CHANGED 2026-10-03 (audit R4-SC-10): every path carries
        // :(literal) so glob metacharacters can't act as pathspecs.
        assert!(filter.contains("':(literal)assets/big.mp4'"));
        // Space-containing path is single-quoted so the shell keeps
        // it as ONE argument.
        assert!(filter.contains("':(literal)docs/my file.pdf'"));
        // Glob-metachar path is literal-quoted (no over-removal).
        assert!(filter.contains("':(literal)a[0].bin'"));
        // No bare positional paths between the filter string and `--`.
        assert_eq!(args[4], "--");
        // FIXED 2026-09-27 (audit F88): the rev range is `HEAD`, matching
        // the filter-repo arm's `--refs HEAD` (SYNC-H6). It must NEVER be
        // `--all`: the caller force-pushes only the current branch, so
        // rewriting every ref would silently diverge the unpublished ones
        // from their remotes with no incident recorded.
        assert_eq!(args[5], "HEAD");
        assert_ne!(args[5], "--all", "filter-branch must not rewrite all refs");
        assert_eq!(args.len(), 6);
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M12/F2.2): a path with an
    /// embedded single quote is escaped (`'\''`) so the shell can't
    /// break out of the quoted string.
    #[test]
    fn test_build_filter_branch_args_escapes_single_quotes() {
        let args = build_filter_branch_args(&["we'ird.bin".to_string()]);
        // CHANGED 2026-10-03 (audit R4-SC-10): :(literal) prefixes the
        // shell-escaped path (quoting and literal-magic compose).
        assert!(
            args[3].contains("':(literal)we'\\''ird.bin'"),
            "got: {}",
            args[3]
        );
    }
}

/// REGRESSION (audit rework round 3): the F89 fix opened the cat-file
/// stdin temp file WRITE-ONLY and handed the raw fd to `Stdio::from`,
/// so `git cat-file --batch-check` inherited an O_WRONLY fd 0, read
/// nothing, and `detect_large_blobs_ahead` silently returned an empty
/// vec on every call — disabling the large-blob rewrite guard at
/// `report.rs:7670` while every test stayed green because nothing
/// covered this function.
///
/// This test builds a repo with an upstream (`@{u}` must resolve for
/// `rev-list --objects @{u}..HEAD`), commits a blob above the
/// threshold, and asserts the blob is actually reported. With the
/// write-only handle it reports 0 records and fails.
#[cfg(test)]
mod detect_large_blobs_ahead_regression {
    use super::*;
    use crate::test_helpers::{create_test_repo_with_remote, test_commit_cmd, test_git_cmd};

    #[tokio::test]
    async fn reports_a_blob_committed_ahead_of_upstream() {
        let (repo, _bare) = create_test_repo_with_remote();
        // Publish the initial commit so @{u} resolves.
        test_git_cmd()
            .args(["push", "-q", "-u", "origin", "HEAD"])
            .current_dir(&repo)
            .output()
            .expect("push init");

        // A blob comfortably above the 1 MiB threshold used here.
        let big = vec![b'x'; 2 * 1024 * 1024];
        std::fs::write(repo.join("big.bin"), &big).expect("write big blob");
        test_git_cmd()
            .args(["add", "big.bin"])
            .current_dir(&repo)
            .output()
            .expect("git add big");
        test_commit_cmd()
            .args(["-m", "add big blob"])
            .current_dir(&repo)
            .output()
            .expect("git commit big");

        let found = detect_large_blobs_ahead(&repo, 1024 * 1024)
            .await
            .expect("detect_large_blobs_ahead");

        assert!(
            !found.is_empty(),
            "detect_large_blobs_ahead returned no records for a 2 MiB blob \
             committed ahead of upstream — the cat-file stdin handle is not \
             readable, so the large-blob guard is silently dead"
        );
        let (size, path) = &found[0];
        assert!(
            *size >= 2 * 1024 * 1024,
            "reported size {} is smaller than the committed blob",
            size
        );
        assert_eq!(path, "big.bin");
    }

    /// Regression test for R4-SC-14: a rev-list failure (here: no
    /// upstream, so `@{u}` cannot resolve) must propagate as Err, not
    /// `Ok(vec![])`. The old empty-vec silently disabled the >100 MiB
    /// rewrite guard for exactly the repos that need it; the caller
    /// now decides (loud incident vs expected skip).
    #[tokio::test]
    async fn rev_list_failure_propagates_instead_of_empty() {
        let (repo, _bare) = create_test_repo_with_remote();
        // Deliberately NOT pushing: no upstream → @{u} unresolvable.
        let err = detect_large_blobs_ahead(&repo, 1024 * 1024)
            .await
            .expect_err("no-upstream rev-list must fail closed, not Ok(empty)");
        assert!(
            err.to_string().contains("rev-list"),
            "error must name the failed measure, got: {err:#}"
        );
    }
}
