//! Upload primitives, not a substitute for the daemon coordinator regressions.
use super::*;
use std::time::Duration;

async fn git(repo: &Path, args: &[&str]) -> String {
    let child =
        super::super::spawn_git_command_cancellable(repo, args, "publication-fixture").unwrap();
    let (status, stdout, stderr) = tokio::time::timeout(
        Duration::from_secs(30),
        super::super::run_git_captured_output(child, repo, "publication-fixture"),
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

#[test]
fn publication_target_accepts_full_object_ids_but_not_moving_or_unsafe_sources() {
    for width in [40, 64] {
        let oid = "A".repeat(width);
        let target = PublicationTarget::new(&oid, "topic/safe").unwrap();
        assert_eq!(
            target.refspec(),
            format!("{}:refs/heads/topic/safe", oid.to_lowercase())
        );
        assert_eq!(target.branch(), "topic/safe");
        assert_eq!(target.object_id(), oid.to_lowercase());
        assert!(PublicationTarget::new(&"0".repeat(width), "main").is_err());
    }
    for source in ["HEAD", "main", "+HEAD", "", "HEAD~1", "a:refs/heads/other"] {
        assert!(PublicationTarget::new(source, "main").is_err(), "{source}");
    }
    let oid = "a".repeat(40);
    for branch in [
        "", "-option", "../main", "a:other", "a\nb", "a.lock", "a//b", "a@{b",
    ] {
        assert!(PublicationTarget::new(&oid, branch).is_err(), "{branch:?}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn pinned_origin_and_mirror_publish_the_same_commit_after_head_advances() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path().join("repo");
    let origin = fixture.path().join("origin.git");
    let mirror = fixture.path().join("mirror.git");
    let hooks = fixture.path().join("hooks");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&hooks).unwrap();
    git(
        fixture.path(),
        &["init", "--bare", origin.to_str().unwrap()],
    )
    .await;
    git(
        fixture.path(),
        &["init", "--bare", mirror.to_str().unwrap()],
    )
    .await;
    git(&repo, &["init", "-b", "main"]).await;
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    )
    .await;
    git(
        &repo,
        &["remote", "add", "mirror", mirror.to_str().unwrap()],
    )
    .await;
    std::fs::write(repo.join("file"), "first").unwrap();
    git(&repo, &["add", "--", "file"]).await;
    // Use the configured identity; fixtures do not manufacture authors.
    git(&repo, &["commit", "-m", "first publication fixture"]).await;
    let first = git(&repo, &["rev-parse", "HEAD"]).await;
    let target = PublicationTarget::new(&first, "main").unwrap();
    let hook = hooks.join("pre-push");
    std::fs::write(&hook, "#!/bin/sh\nset -eu\ntouch ../entered\nwhile ! test -e ../release; do sleep 0.02; done\nprintf x >> ../hook-count\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &repo,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    )
    .await;
    let _publisher = super::super::sync_lease::try_acquire_publication(&repo)
        .unwrap()
        .unwrap();
    let push = push_with_retries_for_target(&repo, 20, 1, "pinned-fixture", Some(&target));
    tokio::pin!(push);
    let entered = async {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !fixture.path().join("entered").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("pre-push did not start");
    };
    tokio::select! {
        result = &mut push => panic!("push ended before hook release: {result:?}"),
        () = entered => {}
    }
    assert!(super::super::sync_lease::try_acquire_publication(&repo)
        .unwrap()
        .is_none());
    let local = super::super::sync_lease::try_acquire(&repo)
        .unwrap()
        .unwrap();
    std::fs::write(repo.join("file"), "second").unwrap();
    git(&repo, &["add", "--", "file"]).await;
    git(
        &repo,
        &["commit", "-m", "new commit while earlier hook blocks"],
    )
    .await;
    let latest = git(&repo, &["rev-parse", "HEAD"]).await;
    assert_ne!(first, latest);
    assert!(!fixture.path().join("release").exists());
    drop(local);
    std::fs::write(fixture.path().join("release"), "release owned fixture hook").unwrap();
    tokio::time::timeout(Duration::from_secs(25), &mut push)
        .await
        .unwrap()
        .unwrap();
    super::super::multi_remote::push_to_named_remote_for_target(
        &repo,
        "mirror",
        20,
        1,
        false,
        Some(&target),
    )
    .await
    .unwrap();
    for remote in [&origin, &mirror] {
        assert_eq!(git(remote, &["rev-parse", "refs/heads/main"]).await, first);
    }
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).await, latest);
    let newest = PublicationTarget::new(&latest, "main").unwrap();
    let (origin_result, mirror_result) = tokio::join!(
        push_with_retries_for_target(&repo, 20, 1, "follow-up-fixture", Some(&newest)),
        super::super::multi_remote::push_to_named_remote_for_target(
            &repo,
            "mirror",
            20,
            1,
            false,
            Some(&newest),
        ),
    );
    origin_result.unwrap();
    mirror_result.unwrap();
    for remote in [&origin, &mirror] {
        assert_eq!(git(remote, &["rev-parse", "refs/heads/main"]).await, latest);
    }
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("hook-count")).unwrap(),
        "xxxx"
    );
}

#[tokio::test]
async fn rejected_pinned_upload_does_not_pull_merge_or_change_local_edits() {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path().join("repo");
    let bare = fixture.path().join("remote.git");
    let other = fixture.path().join("other");
    std::fs::create_dir(&repo).unwrap();
    git(fixture.path(), &["init", "--bare", bare.to_str().unwrap()]).await;
    git(&repo, &["init", "-b", "main"]).await;
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]).await;
    std::fs::write(repo.join("base"), "base").unwrap();
    git(&repo, &["add", "--", "base"]).await;
    git(&repo, &["commit", "-m", "shared base fixture"]).await;
    let base = git(&repo, &["rev-parse", "HEAD"]).await;
    let base_target = PublicationTarget::new(&base, "main").unwrap();
    push_with_retries_for_target(&repo, 20, 1, "initial-fixture", Some(&base_target))
        .await
        .unwrap();
    git(
        fixture.path(),
        &[
            "clone",
            "--branch",
            "main",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    )
    .await;
    std::fs::write(other.join("remote-only"), "remote change").unwrap();
    git(&other, &["add", "--", "remote-only"]).await;
    git(&other, &["commit", "-m", "divergent remote fixture"]).await;
    git(&other, &["push", "origin", "HEAD:refs/heads/main"]).await;
    let remote_tip = git(&bare, &["rev-parse", "refs/heads/main"]).await;
    std::fs::write(repo.join("local-only"), "local change").unwrap();
    git(&repo, &["add", "--", "local-only"]).await;
    git(&repo, &["commit", "-m", "divergent local fixture"]).await;
    std::fs::write(repo.join("uncommitted"), "preserve local edits").unwrap();
    let head = git(&repo, &["rev-parse", "HEAD"]).await;
    let status = git(&repo, &["status", "--porcelain"]).await;
    let target = PublicationTarget::new(&head, "main").unwrap();
    assert!(
        push_with_retries_for_target(&repo, 20, 3, "rejected-pinned-fixture", Some(&target))
            .await
            .is_err()
    );
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).await, head);
    assert_eq!(git(&repo, &["status", "--porcelain"]).await, status);
    assert_eq!(
        std::fs::read_to_string(repo.join("uncommitted")).unwrap(),
        "preserve local edits"
    );
    assert!(!repo.join("remote-only").exists());
    assert!(!repo.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/main"]).await,
        remote_tip
    );
}
