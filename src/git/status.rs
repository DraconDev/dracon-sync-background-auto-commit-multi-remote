//! Repository status checks — origin, upstream, conflict state, readiness.

use std::path::{Path, PathBuf};

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
    /// True if we successfully created the lock (our responsibility to clean up).
    held: bool,
}

impl IndexLock {
    /// Try to acquire `.git/index.lock` for a repo.
    /// Returns Ok(lock) if acquired, Err if another process holds it.
    /// Uses `O_EXCL` (create_new) for atomic creation — no TOCTOU race.
    pub(crate) fn acquire(repo: &Path) -> Result<Self, String> {
        let path = repo.join(".git").join("index.lock");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL — fails if file exists
            .open(&path)
        {
            Ok(_file) => Ok(Self { path, held: true }),
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

impl Drop for IndexLock {
    fn drop(&mut self) {
        if self.held {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Check whether an `origin` remote exists via config or git CLI.
pub(crate) fn has_origin_remote(repo: &Path) -> bool {
    let config_path = repo.join(".git").join("config");
    if let Ok(config) = std::fs::read_to_string(&config_path) {
        return config
            .lines()
            .any(|line| line.trim() == "[remote \"origin\"]");
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

/// Whether a rebase operation is in progress.
pub(crate) fn is_rebase_in_progress(repo: &Path) -> bool {
    repo.join(".git").join("rebase-merge").exists()
        || repo.join(".git").join("rebase-apply").exists()
}

/// Whether a merge operation is in progress.
pub(crate) fn is_merge_in_progress(repo: &Path) -> bool {
    repo.join(".git").join("MERGE_HEAD").exists()
}

/// Whether a cherry-pick operation is in progress.
pub(crate) fn is_cherry_pick_in_progress(repo: &Path) -> bool {
    repo.join(".git").join("CHERRY_PICK_HEAD").exists()
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
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("tmp_pack_")
            {
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

/// ADDED 2026-07-21 (v0.112.30): total commits reachable from HEAD.
/// Used as the ahead-count fallback when no remote-tracking ref exists
/// anywhere (never pushed): every commit is definitionally unpushed.
pub(crate) fn count_all_head_commits(repo: &Path) -> u64 {
    let output = crate::policy::std_git_command()
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(repo)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        _ => 0,
    }
}

/// Count unpushed commits against the first available mirror tracking ref.
/// For repos without an upstream tracking branch (mirror-only repos like
/// `.dracon`), `git status` reports `ahead = 0` even when there ARE local
/// commits that haven't been pushed to any remote. This function checks
/// against known mirror tracking refs (`remotes/github/main`,
/// `remotes/gitlab/main`, `remotes/codeberg/main`) to find the actual
/// unpushed count.
pub(crate) fn count_unpushed_vs_mirrors(repo: &Path) -> u64 {
    let known_mirror_refs = [
        "refs/remotes/github/main",
        "refs/remotes/gitlab/main",
        "refs/remotes/codeberg/main",
    ];
    for mirror_ref in &known_mirror_refs {
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
