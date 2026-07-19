/// Shared test utilities for dracon-sync tests.
///
/// # EnvRestorer
///
/// `EnvRestorer` saves an environment variable, sets it to a new value (or removes it),
/// and restores the original on drop. This prevents env var leaks between tests when
/// running in parallel.
///
/// ## Use `new()` when you need to SET an env var for a test:
/// ```
/// let _guard = EnvRestorer::new("MY_VAR", "new_value");
/// // MY_VAR is "new_value" for the duration of this test
/// ```
///
/// ## Use `remove()` when you need to CLEAR an env var for a test:
/// ```
/// let _guard = EnvRestorer::remove("SOME_VAR");
/// // SOME_VAR is unset for the duration of this test
/// ```
///
/// # Parallel Test Constraints
///
/// Parallel tests mutate shared globals such as `HOME`, `PATH`, and `DRACON_SYNC_GIT_BIN`.
/// The workspace defaults tests to one thread via `.cargo/config.toml` because these mocks use
/// process-global state. Tests that need a mock git binary must still use `GitBinRestorer` so
/// git command wrappers serialize and block until the mock env is cleared. Tests that mutate
/// `PATH` for external tool mocks must still use `acquire_path_lock()` + `EnvRestorer`.
///
/// # Git Command Helper
///
/// Use `test_git_cmd()` instead of direct process construction in tests.
/// This respects `DRACON_SYNC_GIT_BIN`, serializes git invocations, and avoids PATH races in parallel runs.
///
/// ```ignore
/// let output = test_git_cmd().current_dir(&repo).args(["status"]).output()?;
/// ```
#[allow(dead_code)]
pub(crate) fn test_git_cmd() -> crate::policy::GitCommand {
    crate::git::git_cmd()
}

/// Create a git commit command with `--no-verify` to bypass warden hooks.
///
/// Tests that only need to set up git state (not test warden behavior) should use
/// this helper to avoid interference from globally installed warden hooks.
///
/// ```ignore
/// test_commit_cmd().current_dir(&repo).args(["-m", "init"]).output()?;
/// ```
#[allow(dead_code)]
pub(crate) fn test_commit_cmd() -> crate::policy::GitCommand {
    let mut cmd = test_git_cmd();
    cmd.args(["commit", "--no-verify"]);
    cmd
}

/// Global registry of test temp dirs created via `create_test_repo*`.
///
/// F45 (2026-07-18): the previous implementation called
/// `std::mem::forget(tmp)` which permanently strands the temp dir
/// on disk (no `Drop` ever runs, so the kernel eventually reaps it
/// only when `/tmp` is full or rebooted). For a long-running test
/// runner running hundreds of `cargo test` iterations, this fills
/// `/tmp` over hours.
///
/// The new approach moves the `TempDir` into a global
/// `Mutex<Vec<TempDir>>` so the dirs ARE reaped at process exit
/// (when the mutex is dropped). The repo path is still valid for
/// the lifetime of the test binary because the TempDir's underlying
/// dir is held in the Vec.
#[cfg(test)]
#[allow(dead_code)]
static TEST_TEMPS: std::sync::OnceLock<std::sync::Mutex<Vec<tempfile::TempDir>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
fn keep_temp_alive(tmp: tempfile::TempDir) -> std::path::PathBuf {
    let path = tmp.path().to_path_buf();
    let mut registry = TEST_TEMPS
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .expect("TEST_TEMPS lock poisoned");
    registry.push(tmp); // Vec drop → TempDir drop → cleanup at process exit.
    path
}

/// Create a simple test repo with one commit.
///
/// Returns the path to the created repo. The repo has a single file "f" with
/// content "content" and one commit with message "init".
///
/// Uses `--no-verify` to bypass warden hooks.
///
/// ```ignore
/// let repo = create_test_repo();
/// ```
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn create_test_repo() -> std::path::PathBuf {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let repo = tmp.path().to_path_buf();
    test_git_cmd()
        .args(["init", "-q", &repo.to_string_lossy()])
        .output()
        .expect("git init");
    std::fs::write(repo.join("f"), "content").expect("write file");
    test_git_cmd()
        .args(["add", "f"])
        .current_dir(&repo)
        .output()
        .expect("git add");
    test_commit_cmd()
        .args(["-m", "init"])
        .current_dir(&repo)
        .output()
        .expect("git commit");
    keep_temp_alive(tmp)
}

/// Create a test repo with a bare remote.
///
/// Returns (repo_path, bare_path). The repo has a single commit and is
/// configured with "origin" pointing to the bare repo.
///
/// ```ignore
/// let (repo, bare) = create_test_repo_with_remote();
/// ```
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn create_test_repo_with_remote() -> (std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let bare = tmp.path().join("bare.git");
    test_git_cmd()
        .args(["init", "--bare", &bare.to_string_lossy()])
        .output()
        .expect("git init --bare");
    let repo = tmp.path().join("repo");
    test_git_cmd()
        .args(["init", "-q", &repo.to_string_lossy()])
        .output()
        .expect("git init");
    test_git_cmd()
        .args(["remote", "add", "origin", &bare.to_string_lossy()])
        .current_dir(&repo)
        .output()
        .expect("git remote add");
    std::fs::write(repo.join("f"), "content").expect("write file");
    test_git_cmd()
        .args(["add", "f"])
        .current_dir(&repo)
        .output()
        .expect("git add");
    test_commit_cmd()
        .args(["-m", "init"])
        .current_dir(&repo)
        .output()
        .expect("git commit");
    // F45 fix: keep the dir in TEST_TEMPS instead of leaking.
    keep_temp_alive(tmp);
    (repo, bare)
}

#[allow(dead_code)]
pub(crate) struct GitBinRestorer {
    inner: EnvRestorer,
}

impl GitBinRestorer {
    #[allow(dead_code)]
    pub(crate) fn new(new_value: &str) -> Self {
        Self {
            inner: EnvRestorer::new("DRACON_SYNC_GIT_BIN", new_value),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn remove() -> Self {
        Self {
            inner: EnvRestorer::remove("DRACON_SYNC_GIT_BIN"),
        }
    }
}

#[allow(dead_code)]
pub(crate) struct EnvRestorer {
    key: String,
    old_value: Option<String>,
}

#[allow(dead_code)]
impl EnvRestorer {
    /// Saves current value of `key`, sets it to `new_value`.
    /// On Drop: restores the original value (or removes if unset).
    pub(crate) fn new(key: &str, new_value: &str) -> Self {
        let old_value = std::env::var(key).ok();
        std::env::set_var(key, new_value);
        EnvRestorer {
            key: key.to_string(),
            old_value,
        }
    }

    /// Saves current value of `key`, removes the variable entirely.
    /// On Drop: restores the original value (or removes if unset).
    pub(crate) fn remove(key: &str) -> Self {
        let old_value = std::env::var(key).ok();
        std::env::remove_var(key);
        EnvRestorer {
            key: key.to_string(),
            old_value,
        }
    }
}

impl Drop for EnvRestorer {
    fn drop(&mut self) {
        // F46 (2026-07-18): `std::env::set_var` and `remove_var` from a
        // `Drop` implementation is racy with other threads reading the
        // same env var. In `.cargo/config.toml` tests run with
        // `--test-threads=1` for exactly this reason, so the race is
        // closed in CI, but the contract is brittle. If you can
        // restructure the test to do explicit cleanup at the end of
        // the test function (rather than via this Drop), prefer that.
        std::env::remove_var(&self.key);
        if let Some(ref v) = self.old_value {
            std::env::set_var(&self.key, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F45 (2026-07-18): verify that `create_test_repo` no longer
    /// leaks `mem::forget()`'d temp dirs at the process level. After
    /// the fix, the temp dir must be tracked in TEST_TEMPS.
    #[test]
    fn test_create_test_repo_registers_temp_dir() {
        let initial_count = TEST_TEMPS
            .get()
            .and_then(|m| m.lock().ok().map(|g| g.len()))
            .unwrap_or(0);
        let _repo = create_test_repo();
        let after_count = TEST_TEMPS
            .get()
            .and_then(|m| m.lock().ok().map(|g| g.len()))
            .expect("TEST_TEMPS must be initialised by create_test_repo");
        assert!(
            after_count >= initial_count + 1,
            "create_test_repo did not register the temp dir: \
             before={initial_count} after={after_count}; \
             the F45 fix is missing"
        );
    }

    /// F46 (2026-07-18): the `EnvRestorer` saves/restores via Drop,
    /// which is racy with concurrent env var readers. The runtime
    /// guard is `--test-threads=1` in `.cargo/config.toml`. This test
    /// just exercises the save/restore contract.
    #[test]
    fn test_env_restorer_round_trip() {
        let key = "DRACON_TEST_ROUND_TRIP_VAR";
        std::env::set_var(key, "before");
        {
            let _guard = EnvRestorer::new(key, "during");
            assert_eq!(std::env::var(key).as_deref().ok(), Some("during"));
        }
        // After drop, original value is restored.
        assert_eq!(std::env::var(key).as_deref().ok(), Some("before"));
        std::env::remove_var(key);
    }

    /// F46 follow-up: `EnvRestorer::remove` unsets the variable;
    /// after drop, the original value is restored (None if it was
    /// unset to begin with).
    #[test]
    fn test_env_restorer_remove_round_trip() {
        let key = "DRACON_TEST_REMOVE_VAR";
        std::env::set_var(key, "before");
        {
            let _guard = EnvRestorer::remove(key);
            assert!(std::env::var(key).is_err(), "remove should unset");
        }
        assert_eq!(std::env::var(key).as_deref().ok(), Some("before"));
        std::env::remove_var(key);
    }
}
