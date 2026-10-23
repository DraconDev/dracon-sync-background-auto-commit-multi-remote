//! End-to-end behavioural regressions for independent local progress and
//! owned publication. These drive the production `sync_repo` entry, real Git
//! hooks, real bare remotes, and real commits — not a hand-rolled push call.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new(crate::policy::git_binary())
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    bare: PathBuf,
}

impl Fixture {
    /// `blocked` installs a real pre-push hook that parks until released.
    fn new(name: &str, blocked: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join(name);
        let bare = root.path().join(format!("{name}-remote.git"));
        std::fs::create_dir(&repo).unwrap();
        git(
            root.path(),
            &["init", "--bare", "--quiet", bare.to_str().unwrap()],
        );
        git(&repo, &["init", "--quiet", "-b", "main"]);
        // Inherit the configured operator identity: fixtures must never
        // manufacture an author or override the repository identity.
        git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        let hooks = repo.join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        git(
            &repo,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        if blocked {
            let hook = hooks.join("pre-push");
            std::fs::write(
                &hook,
                "#!/bin/sh\nset -eu\ntouch ../hook-entered\nwhile [ ! -e ../hook-release ]; do sleep 0.01; done\n",
            )
            .unwrap();
            std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(repo.join("data"), "initial\n").unwrap();
        git(&repo, &["add", "--", "data"]);
        git(&repo, &["commit", "--quiet", "-m", "fixture baseline"]);
        Self { root, repo, bare }
    }

    fn remote_tip(&self) -> String {
        let out = std::process::Command::new(crate::policy::git_binary())
            .args(["rev-parse", "--verify", "refs/heads/main"])
            .current_dir(&self.bare)
            .output()
            .unwrap();
        if out.status.success() {
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        } else {
            String::new()
        }
    }

    fn released(&self) {
        std::fs::write(self.root.path().join("hook-release"), "go").unwrap();
    }

    async fn wait_for(&self, rel: &str, within: u64) {
        let path = self.root.path().join(rel);
        tokio::time::timeout(Duration::from_secs(within), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{} never appeared", path.display()));
    }

    /// One production sync pass: commit everything and publish it.
    fn sync(&self, local: &std::sync::Arc<tokio::sync::Semaphore>) -> SyncGuard {
        let repo = self.repo.clone();
        let local = local.clone();
        SyncGuard(Some(tokio::spawn(async move {
            let policy: SyncPolicy = toml::from_str("").unwrap();
            let permit = local.clone().acquire_owned().await.unwrap();
            // The production entry owns admission: it takes publication
            // ownership when free, or commits behind the current publisher.
            crate::git::push_resources::with_local_work(local, permit, async move {
                sync_repo_with_ahead_since(
                    &repo,
                    &policy,
                    &std::collections::BTreeSet::new(),
                    120,
                    None,
                    false,
                    None,
                    None,
                    false,
                )
                .await
                .unwrap()
            })
            .await
        })))
    }
}

struct SyncGuard(Option<tokio::task::JoinHandle<SyncOutcome>>);
impl Drop for SyncGuard {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}
impl SyncGuard {
    async fn join(mut self, within: u64) -> SyncOutcome {
        let outcome = tokio::time::timeout(Duration::from_secs(within), self.0.as_mut().unwrap())
            .await
            .expect("sync did not settle within its bounded budget");
        self.0.take();
        outcome.unwrap()
    }
}

/// The core contract: a blocked publication in repository A must not stop
/// repository B from committing AND publishing, and it must not stop a SECOND
/// real commit in A from landing while A's upload is still blocked. Once the
/// hook is released, A publishes its then-current (newest) tip.
#[tokio::test]
async fn blocked_publication_does_not_stall_cross_repo_or_same_repo_commits() {
    let a = Fixture::new("a", true);
    let b = Fixture::new("b", false);
    let local = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let a_baseline = git(&a.repo, &["rev-parse", "HEAD"]);
    let b_baseline = git(&b.repo, &["rev-parse", "HEAD"]);

    // A publishes a first commit, then its hook parks the upload.
    std::fs::write(a.repo.join("data"), "A first commit\n").unwrap();
    let first = a.sync(&local);
    a.wait_for("hook-entered", 20).await;
    let a_pinned = git(&a.repo, &["rev-parse", "HEAD"]);
    assert_ne!(a_pinned, a_baseline);
    // The pin happened under local ownership and the upload is genuinely
    // blocked: nothing has reached the bare remote yet.
    assert_eq!(a.remote_tip(), "");

    // B must be able to commit and publish while A's hook is still parked.
    std::fs::write(b.repo.join("data"), "B concurrent commit\n").unwrap();
    let b_outcome = b.sync(&local).join(60).await;
    let b_tip = git(&b.repo, &["rev-parse", "HEAD"]);
    assert_ne!(b_tip, b_baseline, "B must commit during A's blocked push");
    assert_eq!(
        b.remote_tip(),
        b_tip,
        "B must publish its newest tip while A is blocked (outcome {b_outcome:?})"
    );

    // A SECOND real commit in A while the first upload is still blocked. The
    // production refresh path must commit it WITHOUT waiting for release.
    std::fs::write(a.repo.join("second"), "A second commit during push\n").unwrap();
    let mut refresh = a.sync(&local);
    let a_second = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if let Ok(text) = std::process::Command::new(crate::policy::git_binary())
                .args(["log", "-1", "--format=%H", "--", "second"])
                .current_dir(&a.repo)
                .output()
            {
                let sha = String::from_utf8_lossy(&text.stdout).trim().to_owned();
                if !sha.is_empty() {
                    return sha;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a second commit in A was blocked by A's own in-flight publication");
    assert_ne!(a_second, a_pinned);
    assert!(
        a.remote_tip().is_empty(),
        "remote received A before hook release"
    );

    // Release the blocked hook: the pinned round completes; A's newest tip is
    // published by the following round (fresh dispatch in production, the
    // second sync pass here), never by silently retargeting the pinned one.
    a.released();
    let first_outcome = first.join(90).await;
    assert!(matches!(
        first_outcome,
        SyncOutcome::Synced | SyncOutcome::PushFailed | SyncOutcome::PushPaused
    ));
    let refresh_outcome = refresh.join(90).await;
    assert_eq!(
        a.remote_tip(),
        a_pinned,
        "the released round publishes its pinned tip"
    );
    let final_outcome = a.sync(&local).join(90).await;
    let newest = git(&a.repo, &["rev-parse", "HEAD"]);
    eprintln!(
        "DIAG pinned={a_pinned} second={a_second} newest={newest} remote={} log={}",
        a.remote_tip(),
        git(&a.repo, &["log", "--format=%H %s", "-4"])
    );
    assert_eq!(newest, a_second);
    assert_eq!(
        a.remote_tip(),
        newest,
        "follow-up publication must advance the remote to A's newest tip \
         (refresh {refresh_outcome:?}, final {final_outcome:?})"
    );
}

/// A repository with an active publication owner still commits new edits, and
/// reports that state instead of claiming a successful update.
#[tokio::test]
async fn secondary_owner_commits_and_reports_pending_publication() {
    let a = Fixture::new("pending", false);
    let local = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let before = git(&a.repo, &["rev-parse", "HEAD"]);
    let owner = crate::git::sync_lease::try_acquire_publication(&a.repo)
        .unwrap()
        .unwrap();
    std::fs::write(
        a.repo.join("data"),
        "edited while another publisher owns it\n",
    )
    .unwrap();
    let policy: SyncPolicy = toml::from_str("").unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(60),
        sync_repo_with_ahead_since(
            &a.repo,
            &policy,
            &std::collections::BTreeSet::new(),
            120,
            None,
            false,
            None,
            None,
            false,
        ),
    )
    .await
    .expect("a busy publisher must never block a local commit")
    .unwrap();
    assert_eq!(outcome, SyncOutcome::PublicationPending);
    assert_ne!(git(&a.repo, &["rev-parse", "HEAD"]), before);
    assert_eq!(a.remote_tip(), "", "no competing publisher may push");
    assert!(crate::git::sync_lease::try_acquire_publication(&a.repo)
        .unwrap()
        .is_none());
    drop(owner);
    let _ = local;
}
