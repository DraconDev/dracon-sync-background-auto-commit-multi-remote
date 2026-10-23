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
        // Real watched repositories track their published branch; without
        // this, ahead/behind is undefined and the push gate cannot be
        // exercised. Identity is inherited, never overridden here.
        git(&repo, &["config", "branch.main.remote", "origin"]);
        git(&repo, &["config", "branch.main.merge", "refs/heads/main"]);
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

    /// Wait until this fixture's publication lease is free again, i.e. the
    /// previous round has fully torn down (a dropped task releases it).
    async fn publication_released(&self, within: u64) {
        tokio::time::timeout(Duration::from_secs(within), async {
            loop {
                let acquired = crate::git::sync_lease::try_acquire_publication(&self.repo)
                    .unwrap()
                    .is_some();
                if acquired {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("publication ownership was never released");
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
/// real commit in A from landing while A's upload is still blocked. A released
/// round publishes its own pin; bounded follow-up rounds converge the remote on
/// A's newest tip.
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
    b.publication_released(30).await;
    let b_published = b.remote_tip();
    assert_ne!(
        b_published, "",
        "B must publish while A's push hook is blocked (outcome {b_outcome:?})"
    );
    // A pinned round publishes ITS pin; a repo that keeps accepting edits may
    // have produced newer commits during that upload. Those are published by
    // the next round, so one follow-up dispatch must converge the remote on
    // B's newest tip.
    let b_tip = git(&b.repo, &["rev-parse", "HEAD"]);
    assert_ne!(b_tip, b_baseline, "B must commit during A's blocked push");
    if b_published != b_tip {
        let b_follow_up = b.sync(&local).join(60).await;
        b.publication_released(30).await;
        assert_eq!(
            b.remote_tip(),
            git(&b.repo, &["rev-parse", "HEAD"]),
            "a follow-up round must publish B's newest tip ({b_follow_up:?})"
        );
    }
    assert!(
        a.remote_tip().is_empty(),
        "A's remote must still be empty while its hook is parked"
    );

    // A SECOND real commit in A while the FIRST upload is still blocked. No
    // further dispatch happens: the only thing that can commit here is the
    // running worker's own local refresh, which is exactly what the contract
    // requires (the scheduler's in-flight reservation would skip a redispatch).
    std::fs::write(a.repo.join("second"), "A second commit during push\n").unwrap();
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
        "the remote must not receive A before its hook is released"
    );

    // Release the blocked hook: the PINNED round completes and publishes its
    // pinned tip — not silently retargeted at newer HEAD.
    a.released();
    let first_outcome = first.join(90).await;
    assert!(matches!(
        first_outcome,
        SyncOutcome::Synced | SyncOutcome::PushFailed | SyncOutcome::PushPaused
    ));
    assert_eq!(
        a.remote_tip(),
        a_pinned,
        "the released round publishes its own pinned tip"
    );
    // Follow-up rounds must eventually converge the remote on the newest tip.
    // A round publishes ITS pin, and A keeps accepting edits, so this is
    // bounded convergence rather than "the first follow-up round wins".
    let mut last = first_outcome;
    for _ in 0..4 {
        if a.remote_tip() == git(&a.repo, &["rev-parse", "HEAD"]) {
            break;
        }
        last = a.sync(&local).join(90).await;
        a.publication_released(30).await;
    }
    let newest = git(&a.repo, &["rev-parse", "HEAD"]);
    assert!(
        a_second == newest
            || git(
                &a.repo,
                &["merge-base", "--is-ancestor", &a_second, &newest]
            )
            .is_empty(),
        "the second commit built during the blocked push must survive"
    );
    assert_eq!(
        a.remote_tip(),
        newest,
        "publication must converge the remote on A's newest tip ({last:?})"
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

/// Durable rediscovery: an unpublished tip created by ANOTHER process (a
/// one-shot CLI owner that has already exited) must be picked up by the next
/// dispatch. Publication is not a queued in-memory request that dies with its
/// creator — the actual repository state is the request.
#[tokio::test]
async fn unpublished_tip_from_another_process_is_rediscovered() {
    let fixture = Fixture::new("rediscover", false);
    let local = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let baseline = git(&fixture.repo, &["rev-parse", "HEAD"]);
    // An external process commits and exits without ever publishing. Its
    // publication lease is released with it.
    std::fs::write(fixture.repo.join("data"), "external process edit\n").unwrap();
    git(&fixture.repo, &["add", "--", "data"]);
    git(
        &fixture.repo,
        &["commit", "--quiet", "-m", "external commit"],
    );
    let external_tip = git(&fixture.repo, &["rev-parse", "HEAD"]);
    assert_ne!(external_tip, baseline);
    assert_eq!(fixture.remote_tip(), "");
    // The daemon has no queued request for it; only inspection of real state.
    let outcome = fixture.sync(&local).join(90).await;
    fixture.publication_released(30).await;
    assert_eq!(
        fixture.remote_tip(),
        external_tip,
        "an unpublished tip from another process must be rediscovered and published ({outcome:?})"
    );
}
