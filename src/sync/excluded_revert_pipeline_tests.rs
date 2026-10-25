use super::*;
use std::os::unix::fs::PermissionsExt;

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let out = crate::git::git_cmd()
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn setup() -> (tempfile::TempDir, SyncPolicy) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["config", "user.name", "test"]);
    git(r, &["config", "user.email", "test@test"]);
    let hooks = r.join(".git/local-test-hooks");
    std::fs::create_dir(&hooks).unwrap();
    git(r, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    std::fs::create_dir(r.join(".dracon")).unwrap();
    std::fs::write(
        r.join(".dracon/dracon-sync.toml"),
        b"revert_excluded_to_head = true\n",
    )
    .unwrap();
    std::fs::write(r.join("excluded.log"), b"head\n").unwrap();
    std::fs::write(r.join("normal.txt"), b"one\n").unwrap();
    git(
        r,
        &[
            "add",
            "--",
            "excluded.log",
            "normal.txt",
            ".dracon/dracon-sync.toml",
        ],
    );
    git(r, &["commit", "-q", "-m", "initial"]);
    let p: SyncPolicy = toml::from_str("auto_github_private=false\nauto_commit=true\nauto_pull=false\nauto_push=false\nauto_bump_versions=false\ntrusted_emails=['test@test']\ntrusted_authors=['test']\nauto_commit_exclude_patterns=['*.log']\n").unwrap();
    (d, p)
}
#[tokio::test]
async fn excluded_restore_only_excluded_work_needs_no_commit_and_dry_run_preserves_bytes() {
    let (d, p) = setup();
    let r = d.path();
    let head = git(r, &["rev-parse", "HEAD"]);
    std::fs::write(r.join("excluded.log"), b"operator work\n").unwrap();
    assert!(sync_repo(r, &p, &BTreeSet::new(), 0, None, true, None)
        .await
        .is_ok());
    assert_eq!(
        std::fs::read(r.join("excluded.log")).unwrap(),
        b"operator work\n"
    );
    assert_eq!(git(r, &["rev-parse", "HEAD"]), head);
    assert!(sync_repo(r, &p, &BTreeSet::new(), 0, None, false, None)
        .await
        .is_ok());
    assert_eq!(std::fs::read(r.join("excluded.log")).unwrap(), b"head\n");
    assert_eq!(
        git(r, &["rev-parse", "HEAD"]),
        head,
        "reverting excluded work alone must not manufacture a commit"
    );
}
#[tokio::test]
async fn excluded_restore_never_resets_later_pre_commit_producer_bytes() {
    let (d, p) = setup();
    let r = d.path();
    std::fs::write(r.join("excluded.log"), b"before epoch\n").unwrap();
    std::fs::write(r.join("normal.txt"), b"two\n").unwrap();
    let hook = r.join(".git/local-test-hooks/pre-commit");
    std::fs::write(
        &hook,
        b"#!/bin/sh\nprintf 'later hook bytes\\n' > excluded.log\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let result = sync_repo(r, &p, &BTreeSet::new(), 0, None, false, None).await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(git(r, &["show", "HEAD:normal.txt"]), b"two\n");
    assert_eq!(git(r, &["show", "HEAD:excluded.log"]), b"head\n");
    assert_eq!(
        std::fs::read(r.join("excluded.log")).unwrap(),
        b"later hook bytes\n",
        "post-hook work must NOT be reset after commit/publication"
    );
    assert!(
        hook.is_file(),
        "producer not stopped to manufacture a clean result"
    );
}
