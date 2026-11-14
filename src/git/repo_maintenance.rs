//! Per-repository exclusion between pushes and object-store maintenance.
//!
//! ADDED 2026-11-14 (dracon-sync 0.113.100). A `git push` reads the
//! repository's pack list once, early, then runs the pre-push hook. When it
//! finally builds the pack it checks which remote tips it already has with a
//! QUICK object lookup that does not re-read that pack list. If an auto-gc
//! `git repack -d` runs during the hook, it moves the freshly committed loose
//! objects (including the commit the remote already has) into a new pack and
//! deletes the loose files. The push then cannot find the remote's tip, treats
//! it as unknown, and uploads the whole history.
//!
//! Live evidence: browser-extensions-shared (7.6 GiB of packs, ~3-minute
//! pre-push gates) had three one-commit pushes rejected by GitHub with "pack
//! exceeds maximum allowed size (2.00 GiB)". Each overlapped an incremental
//! repack; every long push without one sent a few MiB. Reproduced locally with
//! a bare remote: a one-commit push sent 57 MiB (the full history) with a
//! repack in its hook window and 239 bytes without.
//!
//! The rule: maintenance never starts while a push is in flight for the same
//! repository (it is skipped and retried on a later probe), and a push that
//! arrives during maintenance waits for it (a repack is bounded by its own
//! timeout and normally takes seconds).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Default)]
struct RepoActivity {
    pushes: usize,
    maintenance: bool,
}

fn registry() -> &'static Mutex<HashMap<PathBuf, RepoActivity>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, RepoActivity>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(repo: &Path) -> PathBuf {
    std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf())
}

fn with_registry<T>(work: impl FnOnce(&mut HashMap<PathBuf, RepoActivity>) -> T) -> T {
    let mut map = registry().lock().unwrap_or_else(|poison| poison.into_inner());
    work(&mut map)
}

fn release(map: &mut HashMap<PathBuf, RepoActivity>, key: &Path, push: bool) {
    if let Some(activity) = map.get_mut(key) {
        if push {
            activity.pushes = activity.pushes.saturating_sub(1);
        } else {
            activity.maintenance = false;
        }
        if activity.pushes == 0 && !activity.maintenance {
            map.remove(key);
        }
    }
}

/// Held for the whole lifetime of one push command (hook included).
#[must_use]
pub(crate) struct PushActivity {
    key: PathBuf,
}

impl Drop for PushActivity {
    fn drop(&mut self) {
        with_registry(|map| release(map, &self.key, true));
    }
}

/// Held for the whole lifetime of one gc/repack.
#[must_use]
pub(crate) struct MaintenanceActivity {
    key: PathBuf,
}

impl Drop for MaintenanceActivity {
    fn drop(&mut self) {
        with_registry(|map| release(map, &self.key, false));
    }
}

const PUSH_WAIT_POLL: Duration = Duration::from_millis(200);

/// Register a push, first waiting out any maintenance already running on the
/// same repository.
pub(crate) async fn begin_push(repo: &Path) -> PushActivity {
    let key = key(repo);
    let mut announced = false;
    loop {
        let admitted = with_registry(|map| {
            let activity = map.entry(key.clone()).or_default();
            if activity.maintenance {
                false
            } else {
                activity.pushes += 1;
                true
            }
        });
        if admitted {
            return PushActivity { key };
        }
        if !announced {
            announced = true;
            eprintln!(
                "⏳ {} push waiting for repository maintenance (gc/repack) to finish",
                repo.display()
            );
        }
        tokio::time::sleep(PUSH_WAIT_POLL).await;
    }
}

/// Claim the repository for maintenance, or `None` while any push is in
/// flight for it.
pub(crate) fn try_begin_maintenance(repo: &Path) -> Option<MaintenanceActivity> {
    let key = key(repo);
    with_registry(|map| {
        let activity = map.entry(key.clone()).or_default();
        if activity.pushes > 0 || activity.maintenance {
            None
        } else {
            activity.maintenance = true;
            Some(MaintenanceActivity { key })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_repo(name: &str) -> PathBuf {
        PathBuf::from(format!(
            "/nonexistent/repo-maintenance-test-{name}-{}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn maintenance_is_refused_while_a_push_is_in_flight() {
        let repo = unique_repo("refused");
        let push = begin_push(&repo).await;
        assert!(try_begin_maintenance(&repo).is_none());
        drop(push);
        let maintenance = try_begin_maintenance(&repo);
        assert!(maintenance.is_some());
        drop(maintenance);
        assert!(with_registry(|map| !map.contains_key(&key(&repo))));
    }

    #[tokio::test]
    async fn concurrent_pushes_all_block_maintenance_until_the_last_ends() {
        let repo = unique_repo("concurrent");
        let first = begin_push(&repo).await;
        let second = begin_push(&repo).await;
        drop(first);
        assert!(try_begin_maintenance(&repo).is_none());
        drop(second);
        assert!(try_begin_maintenance(&repo).is_some());
    }

    #[tokio::test]
    async fn push_waits_for_running_maintenance() {
        let repo = unique_repo("waits");
        let maintenance = try_begin_maintenance(&repo).expect("free repo");
        let waiter = {
            let repo = repo.clone();
            tokio::spawn(async move { begin_push(&repo).await })
        };
        tokio::time::sleep(PUSH_WAIT_POLL * 3).await;
        assert!(!waiter.is_finished(), "push must not start during maintenance");
        drop(maintenance);
        let push = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("push admitted after maintenance")
            .expect("task");
        assert!(try_begin_maintenance(&repo).is_none());
        drop(push);
    }

    #[tokio::test]
    async fn other_repositories_are_unaffected() {
        let busy = unique_repo("busy");
        let idle = unique_repo("idle");
        let _push = begin_push(&busy).await;
        assert!(try_begin_maintenance(&idle).is_some());
    }

    /// The mechanism this module exists for, end to end against a local bare
    /// remote: a repack inside the pre-push hook window turns a one-commit
    /// push into a full-history upload. Keeps the evidence executable: if a
    /// future Git stops doing this, this test says the guard is no longer
    /// load-bearing.
    #[test]
    fn repack_during_pre_push_hook_inflates_the_pushed_pack() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |cwd: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            out
        };
        git(root, &["init", "-q", "--bare", "remote.git"]);
        git(root, &["init", "-q", "-b", "main", "work"]);
        let work = root.join("work");
        for (k, v) in [("user.email", "t@t"), ("user.name", "t"), ("gc.auto", "0")] {
            git(&work, &["config", k, v]);
        }
        // Packed history large enough to dominate the measurement.
        for i in 0..2u64 {
            // xorshift64: incompressible, so the packed size tracks the raw size.
            let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ (i + 1);
            let bytes: Vec<u8> = (0..4_000_000)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 32) as u8
                })
                .collect();
            std::fs::write(work.join(format!("big{i}")), bytes).unwrap();
            git(&work, &["add", "-A"]);
            git(&work, &["commit", "-qm", "big"]);
        }
        git(&work, &["repack", "-adq"]);
        git(&work, &["remote", "add", "origin", "../remote.git"]);
        git(&work, &["push", "-q", "origin", "main"]);
        // The remote's tip is a LOOSE commit locally, as after a fresh daemon commit.
        std::fs::write(work.join("small1"), "a").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-qm", "s1"]);
        git(&work, &["push", "-q", "origin", "main"]);

        let pushed_bytes = |repack_in_hook: bool, n: u32| -> u64 {
            std::fs::write(work.join(format!("next{n}")), format!("{n}")).unwrap();
            git(&work, &["add", "-A"]);
            git(&work, &["commit", "-qm", "next"]);
            let hooks = work.join(".git/hooks");
            std::fs::create_dir_all(&hooks).unwrap();
            let hook = hooks.join("pre-push");
            if repack_in_hook {
                std::fs::write(&hook, "#!/bin/sh\ngit repack -d -l -q\n").unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
            } else {
                let _ = std::fs::remove_file(&hook);
            }
            let out = git(&work, &["push", "--progress", "origin", "main"]);
            let stderr = String::from_utf8_lossy(&out.stderr).replace('\r', "\n");
            let line = stderr
                .lines()
                .rev()
                .find(|l| l.starts_with("Writing objects: 100%"))
                .unwrap_or("")
                .to_string();
            // "Writing objects: 100% (n/n), 7.63 MiB | …" → bytes
            let size = line.split(", ").nth(1).unwrap_or("0 bytes");
            let mut parts = size.split_whitespace();
            let value: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
            let scale = match parts.next().unwrap_or("bytes") {
                "KiB" => 1024.0,
                "MiB" => 1048576.0,
                "GiB" => 1073741824.0,
                _ => 1.0,
            };
            (value * scale) as u64
        };
        let clean = pushed_bytes(false, 1);
        let raced = pushed_bytes(true, 2);
        assert!(clean < 64 * 1024, "control push sent {clean} bytes");
        assert!(
            raced > 4 * 1024 * 1024,
            "a repack in the hook window should resend history; sent {raced} bytes"
        );
    }
}
