//! Repository status checks — origin, upstream, conflict state, readiness.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::current_branch;

/// RAII guard that acquires `.git/index.lock` using the same protocol git uses.
///
/// Git commands (checkout, add, reset, etc.) hold this lock while modifying
/// the working tree. By acquiring it too, we guarantee mutual exclusion with
/// any in-flight git operation. If the lock is held, we skip; if we hold it,
/// git's checkout waits for us.
///
/// This is the definitive fix for the clone race: during `git clone`, checkout
/// holds index.lock. Our `ensure_standard_files` / `publish_repo_pubkey`
/// write files to the working tree. Without the lock, these appear before
/// checkout completes → "Untracked working tree file would be overwritten by
/// merge." With the lock, either git holds it (we skip) or we hold it
/// (git's checkout waits until we're done).
pub(crate) struct IndexLock {
    path: PathBuf,
    // Keep the inode alive so deletion/recreation cannot reuse its identity.
    file: std::fs::File,
}

/// Return the index-lock path Git uses for a checkout.
///
/// A normal checkout stores the lock below `<repo>/.git`, while a linked
/// worktree or nested submodule exposes `.git` as a pointer file. Resolving
/// the pointer here keeps startup cleanup and `IndexLock` on the same path
/// instead of letting one caller inspect a non-existent `<repo>/.git/index.lock`.
pub(crate) fn index_lock_path(repo: &Path) -> PathBuf {
    crate::git::path_gitdir(repo)
        .map(|gitdir| gitdir.join("index.lock"))
        .unwrap_or_else(|| repo.join(".git").join("index.lock"))
}

impl IndexLock {
    /// Try to acquire `.git/index.lock` for a repo.
    /// Returns Ok(lock) if acquired, Err if another process holds it.
    /// Uses `O_EXCL` (create_new) for atomic creation — no TOCTOU race.
    #[cfg(not(unix))]
    pub(crate) fn acquire(_repo: &Path) -> Result<Self, String> {
        Err("index lock acquisition unsupported: ownership-safe cleanup requires Unix inode identities".into())
    }

    #[cfg(unix)]
    pub(crate) fn acquire(repo: &Path) -> Result<Self, String> {
        // CHANGED 2026-07-21 (v0.112.33, audit M16/F2.7): resolve
        // the REAL gitdir first — for linked worktrees and nested
        // submodules (the canonical architecture since 2026-07-02),
        // `<repo>/.git` is a FILE, so creating
        // `<repo>/.git/index.lock` failed with ENOTDIR and
        // `ensure_standard_files` was silently skipped EVERY cycle
        // for every submodule (with a misleading "lock contention"
        // debug message). For a submodule the lock belongs at
        // `<parent>/.git/modules/<name>/index.lock` — where git
        // itself takes it.
        Self::acquire_path(index_lock_path(repo), repo)
    }

    /// Caller resolved this exact path with a bounded owned Git command.
    #[cfg(unix)]
    pub(crate) fn acquire_path(path: PathBuf, repo: &Path) -> Result<Self, String> {
        match Self::try_acquire_path(path) {
            Ok(lock) => Ok(lock),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
                "index.lock held by another git operation, skipping {}",
                repo.display()
            )),
            Err(e) => Err(format!(
                "failed to create index.lock for {}: {}",
                repo.display(),
                e
            )),
        }
    }
}

impl IndexLock {
    /// Preserve the actual I/O kind for callers that distinguish expected
    /// contention from inspection/permission failures. Never parse error text.
    pub(crate) fn try_acquire_path(path: PathBuf) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            Ok(Self { path, file })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "ownership-safe index locks require Unix",
            ))
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn acquire_path(_path: PathBuf, _repo: &Path) -> Result<Self, String> {
        Err("ownership-safe index locks require Unix".into())
    }

    /// Atomically install a caller-owned private index while retaining the
    /// standard Git lock protocol. Never called unless full baseline identities
    /// and parent have been compared under this owned lock.
    ///
    /// The caller's private index is usually produced by `git read-tree`, which
    /// writes every entry WITHOUT stat data. Installing it verbatim forces the
    /// next `git status` to re-run the clean filter over every tracked file; in
    /// a warden-filtered repo that cost measured 147-274s per inspection and
    /// blew the inspection deadline (2026-10-26 fleet-idle repair). So the stat
    /// cache of the CURRENT index is carried over for every entry whose
    /// (mode, oid) the install leaves unchanged. Carrying stat data is safe
    /// exactly because it is only reused when mode and object id still match:
    /// Git re-validates size/mtime/ctime before trusting it, so a stale entry
    /// can only ever cost an extra hash, never report wrong content.
    pub(crate) async fn install_index(
        &mut self,
        repo: &Path,
        source: &Path,
        index: &Path,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        use std::io::{Read, Write};
        anyhow::ensure!(self.is_owned(), "index lock replaced before install");
        let source = std::fs::File::open(source)?;
        anyhow::ensure!(
            source.metadata()?.is_file() && source.metadata()?.len() <= 64 * 1024 * 1024,
            "private index exceeds installation budget"
        );
        let mut bytes = Vec::new();
        source.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "private index grew past budget"
        );
        let read_previous = || -> Result<(std::fs::Metadata, Vec<u8>)> {
            let file = std::fs::File::open(index)?;
            let metadata = file.metadata()?;
            anyhow::ensure!(
                metadata.is_file() && metadata.len() <= 64 * 1024 * 1024,
                "actual index exceeds installation budget"
            );
            let mut bytes = Vec::new();
            file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() <= 64 * 1024 * 1024,
                "actual index grew past budget"
            );
            Ok((metadata, bytes))
        };
        let previous = match read_previous() {
            Ok(previous) => Some(previous),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(error) => return Err(error),
        };
        let (merged, previous_mtime) = if let Some((metadata, previous)) = previous {
            let mtime = metadata.modified()?;
            let merged = tokio::time::timeout_at(
                deadline,
                carry_over_stat_data(repo, index, &previous, &bytes, mtime),
            )
            .await
            .context("stat-cache merge deadline expired; actual index preserved")?
            .unwrap_or(bytes);
            anyhow::ensure!(
                read_previous()?.1 == previous,
                "actual index changed during stat merge; preserved"
            );
            (merged, Some(mtime))
        } else {
            // A clean unborn repository has no actual index yet. There is no
            // cache to transfer; keep the proven candidate byte-for-byte.
            anyhow::ensure!(
                !index.try_exists()?,
                "actual index appeared during installation"
            );
            (bytes, None)
        };
        anyhow::ensure!(
            tokio::time::Instant::now() < deadline,
            "index installation deadline expired"
        );
        anyhow::ensure!(self.is_owned(), "index lock replaced during stat merge");
        self.file.write_all(&merged)?;
        // Preserve the old racy-clean boundary too. Advancing the index mtime
        // without Git's racy-entry smudging can hide a same-size concurrent edit.
        if let Some(mtime) = previous_mtime {
            self.file
                .set_times(std::fs::FileTimes::new().set_modified(mtime))?;
        }
        self.file.sync_all()?;
        anyhow::ensure!(self.is_owned(), "index lock replaced during install");
        std::fs::rename(&self.path, index)?;
        Ok(())
    }

    /// Detect replacement by a non-cooperating writer; this is not an atomic
    /// compare-and-unlink primitive against hostile filesystem mutation.
    pub(crate) fn is_owned(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(owned), Ok(current)) =
                (self.file.metadata(), std::fs::symlink_metadata(&self.path))
            {
                return (owned.dev(), owned.ino()) == (current.dev(), current.ino());
            }
        }
        false
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        if self.is_owned() {
            let _ = std::fs::remove_file(&self.path);
        }
        // Without a stable filesystem identity, conservatively retain the lock.
    }
}

/// One staged entry: `(mode, object id, path)`. Stage 0 only — a conflicted
/// index is never merged.
type StageEntry = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Carry the previous index's stat cache onto the index being installed.
///
/// `read-tree`-derived indexes carry no stat data, so installing one verbatim
/// makes the next `git status` re-run the clean filter for every tracked file
/// (measured 147-274s in a warden-filtered repo, past the inspection deadline).
///
/// The merge is done with Git's own plumbing, never by patching index bytes:
/// start from the CURRENT index (which has a warm stat cache), then apply only
/// the delta this install introduces via `update-index --index-info`. Untouched
/// paths are never rewritten, so they keep their stat data; paths whose
/// (mode, oid) changed or that were added lose it and cost one hash next time,
/// which is exactly correct. `--index-info` cannot change an object's content,
/// so this can only affect stat bookkeeping.
///
/// Returns `None` when the merge cannot be produced or verified, and the caller
/// then installs the private index byte-for-byte (the previous behaviour).
async fn carry_over_stat_data(
    repo: &Path,
    index: &Path,
    previous: &[u8],
    wanted: &[u8],
    previous_mtime: std::time::SystemTime,
) -> Option<Vec<u8>> {
    let workspace = tempfile::Builder::new()
        .prefix("dracon-stat-")
        .tempdir_in(index.parent()?)
        .ok()?;
    let private = workspace.path().join("private.index");
    std::fs::write(&private, wanted).ok()?;
    let before_path = workspace.path().join("before.index");
    std::fs::write(&before_path, previous).ok()?;
    let before = stat_index_listing(repo, &before_path).await?;
    let after = stat_index_listing(repo, &private).await?;
    let merged = workspace.path().join("merged.index");
    std::fs::write(&merged, previous).ok()?;
    std::fs::File::open(&merged)
        .ok()?
        .set_times(std::fs::FileTimes::new().set_modified(previous_mtime))
        .ok()?;
    let delta = stat_index_delta(&before, &after);
    if !delta.is_empty() {
        let mut command = stat_index_command(repo, &merged);
        command.args(["update-index", "-z", "--index-info"]);
        let output = super::run_command_captured_output(
            command,
            60,
            "private stat-cache delta",
            Some(&delta),
        )
        .await
        .ok()?;
        if !output.status.success() {
            return None;
        }
    }
    let merged_entries = stat_index_listing(repo, &merged).await?;
    if merged_entries != after {
        return None;
    }
    std::fs::read(&merged).ok()
}

fn stat_index_command(repo: &Path, index: &Path) -> tokio::process::Command {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(repo)
        .env("GIT_INDEX_FILE", index)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0");
    command.into_tokio()
}

async fn stat_index_listing(repo: &Path, index: &Path) -> Option<Vec<StageEntry>> {
    let mut command = stat_index_command(repo, index);
    command.args(["ls-files", "--stage", "-z"]);
    let output =
        super::run_command_captured_output(command, 60, "private stat-cache listing", None)
            .await
            .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_stage_entries(&output.stdout)
}

// Path-keyed lookups avoid two quadratic scans on large (87k+ path) repos.
fn stat_index_delta(before: &[StageEntry], after: &[StageEntry]) -> Vec<u8> {
    use std::collections::BTreeMap;
    let old: BTreeMap<_, _> = before.iter().map(|(m, o, p)| (p, (m, o))).collect();
    let new: BTreeMap<_, _> = after.iter().map(|(m, o, p)| (p, (m, o))).collect();
    let mut delta = Vec::new();
    for (path, (mode, oid)) in &new {
        if old.get(path) == Some(&(*mode, *oid)) {
            continue;
        }
        delta.extend_from_slice(mode);
        delta.push(b' ');
        delta.extend_from_slice(oid);
        delta.push(b'\t');
        delta.extend_from_slice(path);
        delta.push(0);
    }
    let width = before
        .first()
        .or(after.first())
        .map_or(40, |(_, oid, _)| oid.len());
    for path in old.keys().filter(|path| !new.contains_key(*path)) {
        delta.extend_from_slice(b"0 ");
        delta.extend(std::iter::repeat_n(b'0', width));
        delta.push(b'\t');
        delta.extend_from_slice(path);
        delta.push(0);
    }
    delta
}

/// Parse `git ls-files --stage -z` into sorted `(mode, oid, path)` triples.
fn parse_stage_entries(raw: &[u8]) -> Option<Vec<StageEntry>> {
    let mut entries = Vec::new();
    for record in raw.split(|byte| *byte == 0).filter(|r| !r.is_empty()) {
        let tab = record.iter().position(|byte| *byte == b'\t')?;
        let header = &record[..tab];
        let mut fields = header.split(|byte| *byte == b' ');
        let mode = fields.next()?.to_vec();
        let oid = fields.next()?.to_vec();
        let stage = fields.next()?;
        if fields.next().is_some() || stage != b"0" {
            return None;
        }
        entries.push((mode, oid, record[tab + 1..].to_vec()));
    }
    entries.sort();
    Some(entries)
}

/// Check whether an `origin` remote exists via config or git CLI.
pub(crate) fn has_origin_remote(repo: &Path) -> bool {
    let config_path = repo.join(".git").join("config");
    if let Ok(config) = std::fs::read_to_string(&config_path) {
        if config
            .lines()
            .any(|line| line.trim() == "[remote \"origin\"]")
        {
            return true;
        }
        // FIXED 2026-10-03 (audit R4-SC-12): a config-parse miss is NOT
        // a verdict — trailing comments, casing, and include-based
        // remotes all read as absent. Fall through to the git CLI,
        // which resolves them natively, instead of returning false.
    }
    crate::policy::std_git_command()
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check whether the current branch has a configured upstream.
pub(crate) fn has_tracking_upstream(repo: &Path) -> bool {
    let config_path = repo.join(".git").join("config");
    if let Ok(config) = std::fs::read_to_string(&config_path) {
        if let Some(branch) = current_branch(repo) {
            let section = format!("[branch \"{}\"]", branch);
            if let Some(pos) = config.find(&section) {
                let after = &config[pos + section.len()..];
                let next_section = after.find('[').unwrap_or(after.len());
                let branch_config = &after[..next_section];
                return branch_config.contains("remote = ") && branch_config.contains("merge = ");
            }
        }
        return false;
    }
    // Config file not readable (worktree, symlink, etc.) —
    // fall back to git subprocess which handles these cases natively.
    crate::policy::std_git_command()
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// CHANGED 2026-07-26 (v0.113.2, audit SYNC-H8): resolve the REAL
// gitdir via `path_gitdir` — for nested-on-`main` submodules and
// linked worktrees, `<repo>/.git` is a FILE (gitdir pointer), so
// `<repo>/.git/MERGE_HEAD` was ENOTDIR and these helpers ALWAYS
// returned false: the daemon staged/committed/pushed straight
// through an operator's in-progress conflicted merge in all 10
// nested game repos, publishing conflict markers. Same bug class
// as the v0.112.33 IndexLock fix below.
fn state_path_exists(repo: &Path, name: &str) -> bool {
    crate::git::path_gitdir(repo)
        .map(|gitdir| gitdir.join(name))
        .unwrap_or_else(|| repo.join(".git").join(name))
        .exists()
}

/// Whether a rebase operation is in progress.
pub(crate) fn is_rebase_in_progress(repo: &Path) -> bool {
    state_path_exists(repo, "rebase-merge") || state_path_exists(repo, "rebase-apply")
}

/// Whether a merge operation is in progress.
pub(crate) fn is_merge_in_progress(repo: &Path) -> bool {
    state_path_exists(repo, "MERGE_HEAD")
}

/// Whether a cherry-pick operation is in progress.
pub(crate) fn is_cherry_pick_in_progress(repo: &Path) -> bool {
    state_path_exists(repo, "CHERRY_PICK_HEAD")
}

/// Whether a revert operation is in progress (R4-SC-11).
pub(crate) fn is_revert_in_progress(repo: &Path) -> bool {
    state_path_exists(repo, "REVERT_HEAD")
}

/// Whether a bisect session is active (R4-SC-11).
pub(crate) fn is_bisect_in_progress(repo: &Path) -> bool {
    state_path_exists(repo, "BISECT_LOG")
}

/// Check if a repository is ready for operations (has valid HEAD with commits).
pub(crate) fn is_repo_ready(repo: &Path) -> bool {
    // The repo is a "linked worktree" if `<repo>/.git` is a file
    // (a `gitdir: ...` pointer), not a directory. For worktrees,
    // we can't read `<repo>/.git/HEAD` directly (that path is
    // the .git file, not a directory), so we use `git rev-parse
    // HEAD` from the worktree itself, which works for both
    // regular repos and worktrees.
    let dot_git = repo.join(".git");
    if !dot_git.exists() {
        return false;
    }
    if dot_git.is_dir() {
        // Regular repo: HEAD is at `<repo>/.git/HEAD`.
        let head = dot_git.join("HEAD");
        if !head.exists() {
            return false;
        }
        if let Ok(content) = std::fs::read_to_string(&head) {
            if content.trim().is_empty() {
                return false;
            }
        } else {
            return false;
        }
    }
    // dot_git is a file (worktree) or a dir (regular). Either
    // way, `git rev-parse HEAD` works. Use it to verify HEAD
    // resolves to a real commit.
    let output = super::git_cmd()
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok();
    match output {
        Some(o) => {
            if !o.status.success() {
                return false;
            }
            let hash = String::from_utf8_lossy(&o.stdout).trim().to_string();
            !hash.is_empty()
        }
        None => false,
    }
}

/// ADDED 2026-07-21 (v0.112.30): whether the repo is a *stable* empty
/// repository — `git init` completed (HEAD is a symbolic ref to an
/// unborn branch, `.git` is a real directory) and no git operation is
/// in flight. This is the discriminator between "operator just ran
/// `git init` and hasn't committed yet" (safe to auto-commit a root
/// commit) and "mid-clone" (MUST NOT touch — the daemon would
/// otherwise `git add` a half-checked-out working tree).
///
/// Signals checked:
/// 1. `.git` is a real directory (skip worktree-file pointers — a
///    worktree of an unborn branch is an edge case we leave to the
///    operator).
/// 2. `.git/HEAD` contains `ref: refs/...` (symbolic ref — the state
///    `git init` leaves behind). A detached HEAD with no commits is
///    not a normal init state; skip.
/// 3. No `*.lock` files directly in `.git/` — catches `index.lock`
///    (checkout in progress), `HEAD.lock`, `packed-refs.lock`,
///    `shallow.lock`, `FETCH_HEAD.lock` (fetch writing refs).
/// 4. No `objects/pack/tmp_pack_*` — catches an in-progress clone/fetch
///    download (the pack is written to a tmp file, then renamed).
///
/// The window this does NOT cover (between fetch completing and
/// `refs/heads/<branch>` being written during clone) is closed by the
/// fact that git writes the branch ref atomically with the other refs
/// BEFORE checkout begins — so `git rev-parse HEAD` (checked by the
/// caller via `is_repo_ready`) already succeeds in that window, and
/// the `index.lock` check covers the checkout phase.
pub(crate) fn is_stable_empty_repo(repo: &Path) -> bool {
    let dot_git = repo.join(".git");
    if !dot_git.is_dir() {
        return false;
    }
    let head = match std::fs::read_to_string(dot_git.join("HEAD")) {
        Ok(h) => h,
        Err(_) => return false,
    };
    if !head.trim_start().starts_with("ref: refs/") {
        return false;
    }
    if let Ok(entries) = std::fs::read_dir(&dot_git) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(".lock") {
                return false;
            }
        }
    }
    let pack_dir = dot_git.join("objects").join("pack");
    if let Ok(entries) = std::fs::read_dir(&pack_dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("tmp_pack_") {
                return false;
            }
        }
    }
    true
}

/// ADDED 2026-07-21 (v0.112.30): whether the current branch has an
/// upstream configured (`branch.<name>.remote` + `branch.<name>.merge`)
/// but the corresponding remote-tracking ref
/// (`refs/remotes/<remote>/<branch>`) does NOT exist. This is the
/// "never pushed" (or "remote branch deleted") state: libgit2's
/// ahead/behind computation returns 0 because there is nothing to
/// compare against, which previously hid the fact that EVERY commit on
/// HEAD was unpushed — the daemon's `has_local_or_pending_work` check
/// then treated the repo as fully synced and skipped it forever.
pub(crate) fn upstream_tracking_ref_missing(repo: &Path) -> bool {
    let Some(branch) = current_branch(repo) else {
        return false;
    };
    let output = crate::policy::std_git_command()
        .args(["config", "--get", &format!("branch.{}.remote", branch)])
        .current_dir(repo)
        .output();
    let remote = match output {
        Ok(o) if o.status.success() => {
            let r = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if r.is_empty() {
                return false;
            }
            r
        }
        _ => return false,
    };
    // Sanitize: remote names come from git config; refuse anything that
    // could escape the refs/remotes/ namespace.
    if remote.contains("..") || remote.contains('/') || remote.starts_with('.') {
        return false;
    }
    let tracking_ref = format!("refs/remotes/{}/{}", remote, branch);
    crate::policy::std_git_command()
        .args(["rev-parse", "--verify", "-q", &tracking_ref])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| !s.success())
        .unwrap_or(false)
}

/// ADDED 2026-07-21 (v0.112.31, audit H7/F1.4): total commits
/// reachable from HEAD. Used as the ahead-count fallback when no
/// remote-tracking ref exists anywhere (never pushed): every commit
/// is definitionally unpushed.
pub(crate) fn count_all_head_commits(repo: &Path) -> u64 {
    try_count_all_head_commits(repo).unwrap_or(0)
}

/// Whether HEAD is unborn (symbolic ref to a branch with no commits
/// yet — the state a fresh `git init` leaves behind). Used to tell a
/// legitimate 0-commit count from a corrupt/broken repo when
/// `rev-list --count HEAD` fails: unborn → Ok(0), anything else → Err.
fn is_unborn_head(repo: &Path) -> bool {
    let symbolic = crate::policy::std_git_command()
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !symbolic {
        return false;
    }
    !crate::policy::std_git_command()
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .current_dir(repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Fallible HEAD-commit count. ADDED 2026-10-02 (audit L2): the
/// backstop gate must FAIL CLOSED when the count is unknowable, so
/// `count_ahead_commits` uses this instead of the 0-defaulting
/// wrapper — a status error surfaces instead of silently presenting
/// as "0 ahead" and disarming the backstop. An UNBORN head (fresh
/// `git init`, no commits) still counts Ok(0): zero commits is a
/// legitimate answer, not a status error.
pub(crate) fn try_count_all_head_commits(repo: &Path) -> Result<u64> {
    let output = crate::policy::std_git_command()
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(repo)
        .output()
        .with_context(|| {
            format!(
                "rev-list --count HEAD failed to spawn for {}",
                repo.display()
            )
        })?;
    if !output.status.success() {
        if is_unborn_head(repo) {
            return Ok(0);
        }
        anyhow::bail!(
            "rev-list --count HEAD exited {} for {}",
            output.status,
            repo.display()
        );
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .with_context(|| {
            format!(
                "rev-list --count HEAD printed no number for {}",
                repo.display()
            )
        })
}

/// Return the known mirror tracking refs for the current branch, followed by
/// the legacy `main` refs when the checkout is on another branch. Older
/// clones can retain a `main` tracking ref after a branch rename, while
/// feature/master checkouts should still be evaluated against their own
/// branch first.
fn known_mirror_tracking_refs(repo: &Path) -> Vec<String> {
    let branch = crate::git::current_branch(repo)
        .filter(|branch| branch != "HEAD" && crate::git::is_safe_branch_name(branch))
        .unwrap_or_else(|| "main".to_string());
    let mut branches = vec![branch.clone()];
    if branch != "main" {
        branches.push("main".to_string());
    }
    ["github", "gitlab", "codeberg"]
        .into_iter()
        .flat_map(|remote| {
            branches
                .iter()
                .map(move |branch| format!("refs/remotes/{remote}/{branch}"))
        })
        .collect()
}

/// ADDED 2026-07-21 (v0.112.31, audit H7/F1.4): whether ANY known
/// mirror remote-tracking ref exists locally. Local-only check.
/// Distinguishes "count is 0 because synced with a mirror" from
/// "count is 0 because nothing was ever pushed from this clone" —
/// `count_unpushed_vs_mirrors` returns 0 for both.
pub(crate) fn any_mirror_tracking_ref_exists(repo: &Path) -> bool {
    known_mirror_tracking_refs(repo).iter().any(|r| {
        crate::policy::std_git_command()
            .args(["rev-parse", "--verify", "-q", r.as_str()])
            .current_dir(repo)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// Count unpushed commits against the first available mirror tracking ref.
/// For repos without an upstream tracking branch (mirror-only repos like
/// `.dracon`), `git status` reports `ahead = 0` even when there ARE local
/// commits that haven't been pushed to any remote. This function checks the
/// current branch (and a legacy `main` ref when applicable) to find the
/// actual unpushed count.
pub(crate) fn count_unpushed_vs_mirrors(repo: &Path) -> u64 {
    for mirror_ref in known_mirror_tracking_refs(repo) {
        let output = crate::policy::std_git_command()
            .args(["rev-list", "--count", &format!("{}..HEAD", mirror_ref)])
            .current_dir(repo)
            .output();
        if let Ok(o) = output {
            if o.status.success() {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let count: u64 = stdout.trim().parse().unwrap_or(0);
                if count > 0 {
                    return count;
                }
            }
        }
    }
    0
}

/// Count local commits that can be fast-forward-pushed to a mirror whose
/// tracking ref is behind HEAD. Unlike `count_unpushed_vs_mirrors`, this
/// excludes divergent/ahead mirrors: those require reconciliation and must
/// not cause the daemon to retry a push on every clean cycle.
pub(crate) fn count_pushable_unpushed_vs_mirrors(repo: &Path) -> u64 {
    let mut max_count = 0;
    for mirror_ref in known_mirror_tracking_refs(repo) {
        let ancestor = crate::policy::std_git_command()
            .args(["merge-base", "--is-ancestor", &mirror_ref, "HEAD"])
            .current_dir(repo)
            .output();
        if !ancestor.is_ok_and(|output| output.status.success()) {
            continue;
        }
        let output = crate::policy::std_git_command()
            .args(["rev-list", "--count", &format!("{}..HEAD", mirror_ref)])
            .current_dir(repo)
            .output();
        if let Ok(o) = output {
            if o.status.success() {
                let count: u64 = String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse()
                    .unwrap_or(0);
                max_count = max_count.max(count);
            }
        }
    }
    max_count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: `git init -q -b main <path>` + local user config.
    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let status = crate::policy::std_git_command()
            .args(["init", "-q", "-b", "main"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
        for (k, v) in [("user.email", "test@test"), ("user.name", "test")] {
            let status = crate::policy::std_git_command()
                .args(["config", k, v])
                .current_dir(path)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    fn commit_file(path: &Path, name: &str, msg: &str) {
        std::fs::write(path.join(name), "content\n").unwrap();
        for args in [
            vec!["add", name],
            vec!["commit", "--no-verify", "-q", "-m", msg],
        ] {
            let status = crate::policy::std_git_command()
                .args(&args)
                .current_dir(path)
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        }
    }

    // ---- conflict-state helpers (SYNC-H8, v0.113.2) ----

    /// Regression test for SYNC-H8: for a nested submodule / linked
    /// worktree layout, `<repo>/.git` is a FILE (`gitdir: <path>`),
    /// so conflict-state files must be looked up in the REAL gitdir.
    /// Pre-fix these helpers probed `<repo>/.git/MERGE_HEAD` (ENOTDIR)
    /// and always returned false.
    #[test]
    fn test_conflict_state_detected_through_gitfile() {
        let tmp = tempfile::tempdir().unwrap();
        let real_gitdir = tmp.path().join("modules").join("sub");
        std::fs::create_dir_all(&real_gitdir).unwrap();
        let nested = tmp.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join(".git"),
            format!("gitdir: {}", real_gitdir.display()),
        )
        .unwrap();

        assert!(!is_merge_in_progress(&nested));
        assert!(!is_rebase_in_progress(&nested));
        assert!(!is_cherry_pick_in_progress(&nested));
        assert!(!is_revert_in_progress(&nested));
        assert!(!is_bisect_in_progress(&nested));

        std::fs::write(real_gitdir.join("MERGE_HEAD"), "abc\n").unwrap();
        assert!(
            is_merge_in_progress(&nested),
            "MERGE_HEAD in the real gitdir must be detected through the gitfile"
        );
        std::fs::remove_file(real_gitdir.join("MERGE_HEAD")).unwrap();

        std::fs::create_dir_all(real_gitdir.join("rebase-merge")).unwrap();
        assert!(is_rebase_in_progress(&nested));
        std::fs::remove_dir_all(real_gitdir.join("rebase-merge")).unwrap();

        std::fs::write(real_gitdir.join("CHERRY_PICK_HEAD"), "abc\n").unwrap();
        assert!(is_cherry_pick_in_progress(&nested));
        std::fs::remove_file(real_gitdir.join("CHERRY_PICK_HEAD")).unwrap();

        // ADDED 2026-10-03 (audit R4-SC-11): revert/bisect markers
        // resolve through the gitfile to the real gitdir too.
        std::fs::write(real_gitdir.join("REVERT_HEAD"), "abc\n").unwrap();
        assert!(is_revert_in_progress(&nested));
        std::fs::remove_file(real_gitdir.join("REVERT_HEAD")).unwrap();

        std::fs::write(real_gitdir.join("BISECT_LOG"), "abc\n").unwrap();
        assert!(is_bisect_in_progress(&nested));
    }

    /// Plain-repo layout (real `.git/` directory) still works.
    #[test]
    fn test_conflict_state_plain_repo_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        assert!(!is_merge_in_progress(&repo));
        std::fs::write(repo.join(".git/MERGE_HEAD"), "abc\n").unwrap();
        assert!(is_merge_in_progress(&repo));
    }

    // ---- is_stable_empty_repo ----

    #[test]
    fn test_is_stable_empty_repo_fresh_init() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        assert!(
            is_stable_empty_repo(&repo),
            "fresh `git init` repo with symref HEAD must be stable-empty"
        );
    }

    #[test]
    fn test_is_stable_empty_repo_index_lock_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        std::fs::write(repo.join(".git/index.lock"), "").unwrap();
        assert!(
            !is_stable_empty_repo(&repo),
            "index.lock (mid-checkout) must block the empty-repo bootstrap"
        );
    }

    #[test]
    fn test_is_stable_empty_repo_tmp_pack_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        std::fs::write(repo.join(".git/objects/pack/tmp_pack_abc123"), "").unwrap();
        assert!(
            !is_stable_empty_repo(&repo),
            "tmp_pack_* (mid-clone fetch) must block the empty-repo bootstrap"
        );
    }

    #[test]
    fn test_is_stable_empty_repo_head_lock_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        std::fs::write(repo.join(".git/HEAD.lock"), "").unwrap();
        assert!(
            !is_stable_empty_repo(&repo),
            "any *.lock in .git root must block the bootstrap"
        );
    }

    #[test]
    fn test_is_stable_empty_repo_detached_head_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        let status = crate::policy::std_git_command()
            .args(["checkout", "-q", "--detach", "HEAD"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            !is_stable_empty_repo(&repo),
            "detached HEAD (raw sha, not `ref:`) is not the git-init state"
        );
    }

    // ---- upstream_tracking_ref_missing ----

    #[test]
    fn test_upstream_tracking_ref_missing_no_config() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        assert!(
            !upstream_tracking_ref_missing(&repo),
            "no upstream configured → not 'missing' (nothing to miss)"
        );
    }

    /// Regression test for R4-SC-12: a config-parse miss must fall
    /// back to `git remote get-url origin`. A trailing comment on the
    /// section line defeats the exact-match parse, and an include-based
    /// remote has no section in .git/config at all — but git resolves
    /// both. Pre-fix both fixtures returned false (skipped pulls,
    /// spurious ensure_origin_remote attempts).
    #[test]
    fn test_has_origin_remote_falls_back_on_parse_miss() {
        // Trailing-comment section line: parse misses, CLI resolves.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let status = crate::policy::std_git_command()
            .args(["remote", "add", "origin", "https://example.com/x.git"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(has_origin_remote(&repo));
        let config_path = repo.join(".git").join("config");
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(text.lines().any(|l| l.trim() == "[remote \"origin\"]"));
        let commented = text.replacen(
            "[remote \"origin\"]",
            "[remote \"origin\"] ; trailing comment",
            1,
        );
        std::fs::write(&config_path, commented).unwrap();
        assert!(
            has_origin_remote(&repo),
            "trailing comment must not read as absent"
        );

        // Include-based remote: no section in .git/config at all.
        let tmp2 = tempfile::tempdir().unwrap();
        let repo2 = tmp2.path().join("repo");
        init_repo(&repo2);
        let extra = tmp2.path().join("extra.inc");
        std::fs::write(
            &extra,
            "[remote \"origin\"]\n\turl = https://example.com/y.git\n",
        )
        .unwrap();
        let status = crate::policy::std_git_command()
            .args(["config", "include.path", extra.to_str().unwrap()])
            .current_dir(&repo2)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            has_origin_remote(&repo2),
            "include-based origin must resolve via CLI fallback"
        );

        // Negative control: no origin anywhere → still false.
        let tmp3 = tempfile::tempdir().unwrap();
        let repo3 = tmp3.path().join("repo");
        init_repo(&repo3);
        assert!(!has_origin_remote(&repo3));
    }

    #[test]
    fn test_upstream_tracking_ref_missing_config_without_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        // Configure upstream like configure_publish_upstream_if_missing
        // does, but never push/fetch so refs/remotes/origin/main is absent.
        for (k, v) in [
            ("branch.main.remote", "origin"),
            ("branch.main.merge", "refs/heads/main"),
        ] {
            let status = crate::policy::std_git_command()
                .args(["config", k, v])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        assert!(
            upstream_tracking_ref_missing(&repo),
            "configured upstream with no remote-tracking ref = never pushed"
        );
    }

    #[test]
    fn test_upstream_tracking_ref_missing_ref_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        for (k, v) in [
            ("branch.main.remote", "origin"),
            ("branch.main.merge", "refs/heads/main"),
        ] {
            let status = crate::policy::std_git_command()
                .args(["config", k, v])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        // Simulate a pushed state: create the remote-tracking ref.
        let status = crate::policy::std_git_command()
            .args(["update-ref", "refs/remotes/origin/main", "HEAD"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            !upstream_tracking_ref_missing(&repo),
            "remote-tracking ref present → not missing"
        );
    }

    // ---- count_all_head_commits ----

    #[test]
    fn test_count_all_head_commits_counts_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        assert_eq!(count_all_head_commits(&repo), 0, "no commits → 0");
        commit_file(&repo, "a.txt", "first");
        commit_file(&repo, "b.txt", "second");
        assert_eq!(count_all_head_commits(&repo), 2, "two commits → 2");
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M16/F2.7): IndexLock on a
    /// nested-submodule-style repo (`.git` is a FILE pointing at a
    /// shared gitdir) must create the lock in the REAL gitdir — the
    /// pre-fix code hardcoded `<repo>/.git/index.lock`, which failed
    /// with ENOTDIR on every submodule, so `ensure_standard_files`
    /// was silently skipped every cycle.
    #[test]
    fn test_index_lock_resolves_real_gitdir_for_submodule() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        let nested = parent.join("sub");
        let real_gitdir = parent.join(".git").join("modules").join("sub");
        std::fs::create_dir_all(&real_gitdir).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        // `.git` FILE with a relative gitdir pointer (submodule layout).
        std::fs::write(nested.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();

        assert_eq!(
            index_lock_path(&nested),
            real_gitdir.join("index.lock"),
            "startup cleanup and IndexLock must resolve the same submodule lock path"
        );
        let lock = IndexLock::acquire(&nested).expect("acquire must succeed");
        assert!(
            real_gitdir.join("index.lock").exists(),
            "lock must be created in the real gitdir"
        );
        assert!(
            !nested.join(".git").join("index.lock").exists(),
            "lock must NOT be created under the .git file"
        );
        drop(lock);
        assert!(
            !real_gitdir.join("index.lock").exists(),
            "lock must be released on drop"
        );
    }

    /// A linked worktree's `.git` file points to a per-worktree gitdir
    /// below the primary checkout's `worktrees/` directory. Startup cleanup
    /// must inspect that resolved directory, where Git creates the index lock.
    #[test]
    fn test_index_lock_path_resolves_linked_worktree_gitdir() {
        let tmp = tempfile::tempdir().unwrap();
        let primary_gitdir = tmp.path().join("primary").join(".git");
        let worktree = tmp.path().join("linked");
        let worktree_gitdir = primary_gitdir.join("worktrees").join("linked");
        std::fs::create_dir_all(&worktree_gitdir).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", worktree_gitdir.display()),
        )
        .unwrap();

        assert_eq!(
            index_lock_path(&worktree),
            worktree_gitdir.join("index.lock"),
            "linked worktree lock must be resolved below its per-worktree gitdir"
        );
    }

    // ---- any_mirror_tracking_ref_exists (v0.112.31, audit H7/F1.4) ----

    #[test]
    fn test_any_mirror_tracking_ref_exists_false_for_fresh_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        assert!(
            !any_mirror_tracking_ref_exists(&repo),
            "no mirror refs → nothing was ever pushed from this clone"
        );
    }

    #[test]
    fn test_any_mirror_tracking_ref_exists_true_with_mirror_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        let status = crate::policy::std_git_command()
            .args(["update-ref", "refs/remotes/gitlab/main", "HEAD"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(any_mirror_tracking_ref_exists(&repo));
    }

    #[test]
    fn test_mirror_tracking_helpers_follow_non_main_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        let status = crate::policy::std_git_command()
            .args(["checkout", "-q", "-b", "feature/audit"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        let status = crate::policy::std_git_command()
            .args(["update-ref", "refs/remotes/gitlab/feature/audit", "HEAD"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(any_mirror_tracking_ref_exists(&repo));

        commit_file(&repo, "b.txt", "local ahead");
        assert_eq!(count_unpushed_vs_mirrors(&repo), 1);
        assert_eq!(count_pushable_unpushed_vs_mirrors(&repo), 1);
    }

    #[test]
    fn test_count_pushable_unpushed_mirrors_excludes_divergence() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "init");
        let first = crate::policy::std_git_command()
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let first = String::from_utf8_lossy(&first.stdout).trim().to_string();
        crate::policy::std_git_command()
            .args(["update-ref", "refs/remotes/gitlab/main", &first])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert_eq!(count_pushable_unpushed_vs_mirrors(&repo), 0);

        commit_file(&repo, "b.txt", "local ahead");
        assert_eq!(count_pushable_unpushed_vs_mirrors(&repo), 1);

        // Point the mirror at a different root. A divergent mirror is not a
        // fast-forward push candidate and must not trigger retry churn. Make
        // the unrelated root in this repository so the ref points at an
        // object Git can inspect (rather than a dangling test ref).
        let tree = crate::policy::std_git_command()
            .args(["write-tree"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let tree = String::from_utf8_lossy(&tree.stdout).trim().to_string();
        let foreign = crate::policy::std_git_command()
            .args(["commit-tree", &tree, "-m", "foreign"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(foreign.status.success());
        let foreign = String::from_utf8_lossy(&foreign.stdout).trim().to_string();
        crate::policy::std_git_command()
            .args(["update-ref", "refs/remotes/gitlab/main", &foreign])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert_eq!(count_pushable_unpushed_vs_mirrors(&repo), 0);
    }
}

#[cfg(all(test, unix))]
#[tokio::test]
async fn installing_an_epoch_index_keeps_the_stat_cache_of_unchanged_entries() {
    // Reproduced defect (2026-10-26): epochs install a `read-tree` index that
    // carries NO stat data, so the next `git status` re-runs the clean filter
    // for every tracked file. In a warden-filtered repo that cost 147-274s and
    // blew the status-inspection deadline. Installing must now carry stat
    // data across for entries whose (mode, oid) are unchanged.
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    run(&["init", "-q", "--template=", "-b", "main"]);
    run(&["config", "user.email", "test@test"]);
    run(&["config", "user.name", "test"]);
    run(&["config", "commit.gpgsign", "false"]);
    for name in ["one.txt", "two.txt", "three.txt"] {
        std::fs::write(repo.join(name), format!("{name}\n")).unwrap();
    }
    run(&["add", "--", "one.txt", "two.txt", "three.txt"]);
    run(&["commit", "-q", "-m", "fixture"]);
    let index = repo.join(".git/index");
    let zeroed = |index: &Path| -> usize {
        let out = std::process::Command::new("git")
            .args(["ls-files", "--debug"])
            .current_dir(repo)
            .env("GIT_INDEX_FILE", index)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.trim() == "ctime: 0:0")
            .count()
    };
    // Warm the stat cache the way a normal checkout does.
    run(&["update-index", "--refresh"]);
    assert_eq!(zeroed(&index), 0, "fixture must start warm");
    // Build a read-tree index (stat-less), exactly like the epoch path does.
    let tree = String::from_utf8_lossy(&run(&["rev-parse", "HEAD^{tree}"]).stdout)
        .trim()
        .to_string();
    let private = dir.path().join("private.index");
    let out = std::process::Command::new("git")
        .args(["read-tree", &tree])
        .current_dir(repo)
        .env("GIT_INDEX_FILE", &private)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        zeroed(&private),
        3,
        "control: read-tree actually loses stat cache"
    );
    // A cached stat is not proof of clean content when its mtime meets the
    // index's racy-clean boundary. Preserve that boundary through the merge.
    run(&["config", "core.trustctime", "false"]);
    let tracked_time = std::fs::metadata(repo.join("one.txt"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::open(&index)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(tracked_time))
        .unwrap();
    std::fs::write(repo.join("one.txt"), "NEW.txt\n").unwrap(); // same size
    std::fs::File::open(repo.join("one.txt"))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(tracked_time))
        .unwrap();
    // Negative control: copying warm stats but advancing the index's mtime
    // reproduces a false-clean result. This changes only a disposable index.
    let bad_cache = repo.join("bad-cache.index");
    std::fs::copy(&index, &bad_cache).unwrap();
    std::fs::File::open(&bad_cache).unwrap().set_times(
        std::fs::FileTimes::new().set_modified(tracked_time + std::time::Duration::from_secs(2)),
    ).unwrap();
    let bad = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=no"])
        .current_dir(repo).env("GIT_INDEX_FILE", &bad_cache)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .output().unwrap();
    assert!(bad.status.success());
    assert!(!String::from_utf8_lossy(&bad.stdout).contains("one.txt"),
        "negative control must reproduce the unsafe cache's hidden edit");
    let before_cancel = std::fs::read(&index).unwrap();
    let mut expired = IndexLock::try_acquire_path(index.with_extension("lock")).unwrap();
    assert!(expired
        .install_index(repo, &private, &index, tokio::time::Instant::now())
        .await
        .is_err());
    assert_eq!(
        std::fs::read(&index).unwrap(),
        before_cancel,
        "expired merge must not install"
    );
    drop(expired);
    let mut lock = IndexLock::try_acquire_path(index.with_extension("lock")).unwrap();
    lock.install_index(
        repo,
        &private,
        &index,
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .unwrap();
    drop(lock);
    assert_eq!(
        zeroed(&index),
        0,
        "install must preserve stat data for unchanged (mode, oid) entries"
    );
    let racy_status = run(&["status", "--porcelain=v1"]);
    assert!(
        String::from_utf8_lossy(&racy_status.stdout).contains("one.txt"),
        "same-size racy-clean edit must remain visible after installing cached stats"
    );
    // The installed index must still describe exactly the same tree.
    let staged = String::from_utf8_lossy(&run(&["write-tree"]).stdout)
        .trim()
        .to_string();
    assert_eq!(staged, tree);
    // And a genuine content change must still be visible afterwards.
    std::fs::write(repo.join("two.txt"), "changed\n").unwrap();
    let status = String::from_utf8_lossy(&run(&["status", "--porcelain=v1"]).stdout).to_string();
    assert!(
        status.contains("two.txt"),
        "content change must remain visible: {status}"
    );
    // Exercise the nonempty delta too, including mode change, removal, and
    // a raw newline path. Existing concurrent worktree bytes must survive.
    let oid = String::from_utf8_lossy(&run(&["rev-parse", "HEAD:three.txt"]).stdout)
        .trim()
        .to_string();
    let candidate = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_INDEX_FILE", &private)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    };
    candidate(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100755,{oid},one.txt"),
    ]);
    candidate(&["update-index", "--force-remove", "two.txt"]);
    candidate(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{oid},new\nname"),
    ]);
    let expected_tree = candidate(&["write-tree"]);
    let mut lock = IndexLock::try_acquire_path(index.with_extension("lock")).unwrap();
    lock.install_index(
        repo,
        &private,
        &index,
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .unwrap();
    drop(lock);
    assert_eq!(
        run(&["write-tree"]).stdout,
        expected_tree,
        "staged intent must match the candidate exactly"
    );
    assert_eq!(std::fs::read(repo.join("one.txt")).unwrap(), b"NEW.txt\n");
    assert_eq!(std::fs::read(repo.join("two.txt")).unwrap(), b"changed\n");
}

#[cfg(test)]
#[test]
fn stat_delta_handles_large_inventory_and_sha256_without_rescanning_every_path() {
    // A representative large inventory, not a permanent fleet-count pin.
    let before: Vec<StageEntry> = (0..100_000)
        .map(|i| {
            (
                b"100644".to_vec(),
                vec![b'a'; 64],
                format!("dir/{i:06}").into_bytes(),
            )
        })
        .collect();
    let mut after = before.clone();
    after[17].0 = b"100755".to_vec();
    after[31].1 = vec![b'b'; 64];
    after.pop();
    after.push((b"100644".to_vec(), vec![b'c'; 64], b"new\tfile".to_vec()));
    let start = std::time::Instant::now();
    let delta = stat_index_delta(&before, &after);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "delta exhausted a staging-scale CPU budget"
    );
    let changes = parse_stage_entries(&delta);
    // index-info has no stage column: inspect its four NUL-delimited records.
    assert!(changes.is_none());
    let records: Vec<_> = delta.split(|b| *b == 0).filter(|r| !r.is_empty()).collect();
    assert_eq!(records.len(), 4);
    assert!(records
        .iter()
        .any(|r| r.starts_with(b"100755 ") && r.ends_with(b"dir/000017")));
    assert!(records.iter().any(|r| r.ends_with(b"new\tfile")));
    let mut deletion = b"0 ".to_vec();
    deletion.extend_from_slice(&[b'0'; 64]);
    deletion.extend_from_slice(b"\tdir/099999");
    assert!(
        records.contains(&deletion.as_slice()),
        "SHA256 deletion must use 64 zeroes"
    );
    assert_eq!(before[17].0, b"100644", "baseline intent remains immutable");
}

#[cfg(all(test, not(unix)))]
#[test]
fn index_lock_unsupported_platform_refuses_before_filesystem_access() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let error = IndexLock::acquire(repo.path()).err().unwrap();
    assert!(error.contains("unsupported"));
    assert!(!repo.path().join(".git/index.lock").exists());
}
