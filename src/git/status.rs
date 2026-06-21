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
    let head = repo.join(".git").join("HEAD");
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
