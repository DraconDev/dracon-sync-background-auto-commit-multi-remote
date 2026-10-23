//! Commit-boundary regressions: real hooks and staged/worktree preservation.
use super::*;
use std::time::Duration;

async fn git(repo: &Path, args: &[&str]) -> String {
    let child =
        crate::git::spawn_git_command_cancellable(repo, args, "owned-commit-fixture").unwrap();
    let (status, stdout, stderr) = tokio::time::timeout(
        Duration::from_secs(30),
        crate::git::run_git_captured_output(child, repo, "owned-commit-fixture"),
    )
    .await
    .expect("fixture Git deadline")
    .unwrap();
    assert!(
        status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&stderr)
    );
    String::from_utf8(stdout).unwrap().trim().to_owned()
}

#[cfg(unix)]
struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    hooks: PathBuf,
}

#[cfg(unix)]
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let hooks = root.path().join("hooks");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&hooks).unwrap();
        git(&repo, &["init", "-b", "main"]).await;
        std::fs::write(repo.join("file"), "first").unwrap();
        git(&repo, &["add", "--", "file"]).await;
        commit_staged_owned(&repo, "initial owned commit fixture", 20)
            .await
            .unwrap();
        git(
            &repo,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        )
        .await;
        std::fs::write(repo.join("file"), "staged content").unwrap();
        git(&repo, &["add", "--", "file"]).await;
        std::fs::write(repo.join("file"), "unstaged content").unwrap();
        Self { root, repo, hooks }
    }

    fn hook(&self, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.hooks.join("pre-commit");
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn rejecting_pre_commit_is_not_bypassed_and_edits_remain_distinct() {
    let fixture = Fixture::new().await;
    fixture.hook("#!/bin/sh\ntouch ../precommit-ran\nexit 1\n");
    let before = git(&fixture.repo, &["rev-parse", "HEAD"]).await;
    assert!(commit_staged_owned(&fixture.repo, "must be refused", 10)
        .await
        .is_err());
    assert!(fixture.root.path().join("precommit-ran").exists());
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]).await, before);
    assert_eq!(
        git(&fixture.repo, &["show", ":file"]).await,
        "staged content"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("file")).unwrap(),
        "unstaged content"
    );
    fixture.hook("#!/bin/sh\ntouch ../precommit-accepted\nexit 0\n");
    commit_staged_owned(&fixture.repo, "accepted staged content", 10)
        .await
        .unwrap();
    assert!(fixture.root.path().join("precommit-accepted").exists());
    assert_ne!(git(&fixture.repo, &["rev-parse", "HEAD"]).await, before);
    assert_eq!(
        git(&fixture.repo, &["show", "HEAD:file"]).await,
        "staged content"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("file")).unwrap(),
        "unstaged content"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn another_publication_owner_allows_a_real_commit_without_a_push_or_failure_reset() {
    let fixture = Fixture::new().await;
    fixture.hook("#!/bin/sh\ntouch ../pending-owner-precommit\nexit 0\n");
    let bare = fixture.root.path().join("remote.git");
    git(
        fixture.root.path(),
        &["init", "--bare", bare.to_str().unwrap()],
    )
    .await;
    git(
        &fixture.repo,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    )
    .await;
    let publisher = crate::git::sync_lease::try_acquire_publication(&fixture.repo)
        .unwrap()
        .unwrap();
    let before = git(&fixture.repo, &["rev-parse", "HEAD"]).await;
    let policy = SyncPolicy {
        auto_commit: true,
        auto_push: true,
        auto_pull: true,
        auto_bump_versions: false,
        auto_tag: false,
        standard_files_auto: false,
        build_artifact_cleanup: false,
        sync_metadata: false,
        sync_visibility: false,
        remotes: vec![],
        // Exercise the parsed production defaults, notably the nonzero
        // staging size guard; derived Default deliberately contains zeros.
        ..toml::from_str::<SyncPolicy>("").unwrap()
    };
    let mut failures = HashMap::from([(
        "origin".to_owned(),
        crate::daemon::RemoteFailInfo {
            consecutive: 2,
            last_error: "previous actual transport refusal".to_owned(),
            last_attempt_unix: 17,
        },
    )]);
    let previous_failures = failures.clone();
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        sync_repo_with_ahead_since(
            &fixture.repo,
            &policy,
            &BTreeSet::new(),
            60,
            Some(&mut failures),
            false,
            None,
            None,
            false,
        ),
    )
    .await
    .expect("publication admission blocked local progress")
    .unwrap();
    assert_eq!(outcome, SyncOutcome::PublicationPending);
    assert_ne!(git(&fixture.repo, &["rev-parse", "HEAD"]).await, before);
    assert_eq!(
        git(&fixture.repo, &["show", "HEAD:file"]).await,
        "unstaged content"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("file")).unwrap(),
        "unstaged content"
    );
    assert!(fixture.root.path().join("pending-owner-precommit").exists());
    assert!(git(&bare, &["for-each-ref", "--format=%(refname)"])
        .await
        .is_empty());
    assert_eq!(failures, previous_failures);
    assert!(
        crate::git::sync_lease::try_acquire_publication(&fixture.repo)
            .unwrap()
            .is_none()
    );
    drop(publisher);
}

#[cfg(unix)]
fn no_longer_running(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat
            .rsplit_once(") ")
            .is_some_and(|(_, rest)| rest.starts_with('Z')),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_and_deadline_stop_commit_hook_descendants_and_allow_retry() {
    for deadline in [false, true] {
        let fixture = Fixture::new().await;
        fixture.hook("#!/bin/sh\nsleep 300 &\nchild=$!\nprintf '%s %s\\n' \"$$\" \"$child\" > ../commit-hook-pids\nwait\n");
        let before = git(&fixture.repo, &["rev-parse", "HEAD"]).await;
        let mut commit = Box::pin(commit_staged_owned(
            &fixture.repo,
            "blocked commit",
            if deadline { 2 } else { 30 },
        ));
        let pids_file = fixture.root.path().join("commit-hook-pids");
        tokio::select! {
            result = &mut commit => panic!("commit ended before hook started: {result:?}"),
            () = async {
                tokio::time::timeout(Duration::from_secs(10), async {
                    while !std::fs::read_to_string(&pids_file).is_ok_and(|text| {
                        text.ends_with('\n') && text.split_whitespace().count() == 2
                    }) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }).await.expect("hook did not start");
            } => {}
        }
        let pids: Vec<u32> = std::fs::read_to_string(&pids_file)
            .unwrap()
            .split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 2, "fixture must identify hook and descendant");
        if deadline {
            assert!(tokio::time::timeout(Duration::from_secs(5), &mut commit)
                .await
                .expect("commit operation exceeded its deadline")
                .is_err());
        }
        drop(commit);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !pids.iter().all(|pid| no_longer_running(*pid)) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned hook descendants survived cancellation");
        assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]).await, before);
        assert_eq!(
            git(&fixture.repo, &["show", ":file"]).await,
            "staged content"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.repo.join("file")).unwrap(),
            "unstaged content"
        );
        fixture.hook("#!/bin/sh\nexit 0\n");
        commit_staged_owned(&fixture.repo, "retry after owned cancellation", 10)
            .await
            .unwrap();
        assert_eq!(
            git(&fixture.repo, &["show", "HEAD:file"]).await,
            "staged content"
        );
    }
}
