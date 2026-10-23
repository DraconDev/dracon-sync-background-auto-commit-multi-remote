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
        let remote = root.path().join("remote.git");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&hooks).unwrap();
        git(root.path(), &["init", "--bare", remote.to_str().unwrap()]).await;
        git(&repo, &["init", "-b", "main"]).await;
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )
        .await;
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

    fn pre_push(&self, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.hooks.join("pre-push");
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn pushed_refs(&self) -> Vec<String> {
        let out = std::process::Command::new(crate::policy::git_binary())
            .args(["for-each-ref", "--format=%(refname)"])
            .current_dir(&self.root.path().join("remote.git"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
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
async fn owned_commit_keeps_the_daemons_commit_hook_policy_and_worktree_edits() {
    // The daemon deliberately does NOT run `.git/hooks/pre-commit` on auto
    // commits: each commit is already gated in-process (bucket forward-only
    // guard, immutable-storage transaction, exclusions, size limits) and PUSH
    // hooks still run on every publication. A per-repo commit hook must
    // therefore neither block an auto commit nor be reported as having run.
    let fixture = Fixture::new().await;
    fixture.hook("#!/bin/sh\ntouch ../precommit-ran\nexit 1\n");
    let before = git(&fixture.repo, &["rev-parse", "HEAD"]).await;
    commit_staged_owned(&fixture.repo, "daemon auto commit", 10)
        .await
        .unwrap();
    assert!(
        !fixture.root.path().join("precommit-ran").exists(),
        "auto commits must not execute commit hooks"
    );
    assert_ne!(git(&fixture.repo, &["rev-parse", "HEAD"]).await, before);
    // The commit captures the staged snapshot; the unstaged edit is neither
    // included in it nor destroyed by committing.
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
    let bare = fixture.root.path().join("remote.git");
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
async fn cancelled_or_timed_out_publication_kills_owned_hook_descendants() {
    // Cancellation ownership, proven through the production publication entry:
    // the deadline and an external drop must both tear down the owned process
    // group (Git + hook + its helper), never leave an orphan, and never record
    // a transport failure for work that was merely interrupted.
    for deadline in [false, true] {
        let fixture = Fixture::new().await;
        fixture.pre_push(
            "#!/bin/sh\nset -eu\nsleep 300 &\nchild=$!\nprintf '%s %s\\n' \"$$\" \"$child\" > ../hook-pids\nwait\n",
        );
        let repo = fixture.repo.clone();
        let push_repo = repo.clone();
        let pids_file = fixture.root.path().join("hook-pids");
        let before = git(&repo, &["rev-parse", "HEAD"]).await;
        let mut push = Box::pin(async move {
            let policy: SyncPolicy = toml::from_str(&format!(
                "push_op_timeout_secs = {}\npush_retries = 0\n",
                if deadline { 2 } else { 60 }
            ))
            .unwrap();
            let mut plan =
                crate::sync::publication::plan_publication(&push_repo, &policy, true, None, 1)
                    .await
                    .unwrap();
            crate::git::push_resources::push_phase(upload_publication(
                &push_repo, &policy, &mut plan, false,
            ))
            .await
        });
        tokio::select! {
            result = &mut push => panic!("publication ended before its hook started: {result:?}"),
            () = async {
                tokio::time::timeout(Duration::from_secs(10), async {
                    while !std::fs::read_to_string(&pids_file).is_ok_and(|text| {
                        text.ends_with('\n') && text.split_whitespace().count() == 2
                    }) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }).await.expect("pre-push hook did not start");
            } => {}
        }
        let pids: Vec<u32> = std::fs::read_to_string(&pids_file)
            .unwrap()
            .split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 2, "fixture must identify hook and descendant");
        if deadline {
            // The operation deadline must settle the attempt by itself instead
            // of hanging. The typed error shape is the attempt's own concern;
            // this asserts completion within a bounded window.
            let _ = tokio::time::timeout(Duration::from_secs(30), &mut push)
                .await
                .expect("publication ignored its own operation deadline");
        }
        drop(push);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !pids.iter().all(|pid| no_longer_running(*pid)) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned publication descendants survived cancellation");
        // Interrupted work is not converged state: HEAD is untouched and the
        // bare remote never received the pinned commit.
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]).await, before);
        assert_eq!(fixture.pushed_refs(), Vec::<String>::new());
    }
}
