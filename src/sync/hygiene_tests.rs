//! Production-path controls for hygiene: real sync_repo positives, admission of
//! large-file-only work, opt-out/dry-run scope, foreign-intent refusal and
//! writer-safety. Fixtures are disposable; no installed-binary or fleet claim.
use super::*;

fn native(repo: &Path, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
    use std::io::Write;
    let mut command = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(["--no-optional-locks", "-C"])
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = input {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn excluded() -> BTreeSet<String> {
    crate::policy::default_exclude_dir_names()
        .into_iter()
        .collect()
}

#[test]
fn artifact_component_classifies_build_output_names_for_production_hygiene() {
    let no_exclusions = BTreeSet::new();
    for (file, expected) in [
        ("app/.output/bundle.js", ".output"),
        ("app/generated/module.js", "generated"),
        ("app/build_output/a.bin", "build_output"),
        ("app/output-cache/a.bin", "output-cache"),
    ] {
        assert_eq!(
            hygiene::artifact_component(Path::new(file), &no_exclusions).as_deref(),
            Some(expected),
            "{file}"
        );
    }
    assert_eq!(
        hygiene::artifact_component(Path::new("app/src/main.rs"), &no_exclusions),
        None
    );
}

fn policy(cleanup: bool) -> SyncPolicy {
    toml::from_str(&format!(
        "auto_github_private=false\nauto_commit=true\nauto_pull=false\nauto_push=false\n\
         auto_bump_versions=false\nauto_resolve_unmerged=false\nbuild_artifact_cleanup={cleanup}\n\
         max_stage_file_bytes=1024\n\
         trusted_emails=['test@test']\ntrusted_authors=['test']\nremotes=[]\n"
    ))
    .unwrap()
}

/// Tracked artifact bytes, an untracked oversized file and ordinary work.
fn fixture(artifact: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    native(r, &["init", "--template=", "-q", "-b", "main"], None);
    native(r, &["config", "user.name", "test"], None);
    native(r, &["config", "user.email", "test@test"], None);
    native(r, &["config", "commit.gpgsign", "false"], None);
    let hooks = r.join(".git/fixture-hooks");
    std::fs::create_dir(&hooks).unwrap();
    native(
        r,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
        None,
    );
    std::fs::create_dir_all(r.join("dist")).unwrap();
    std::fs::write(r.join("dist/bundle.js"), b"artifact bytes\n").unwrap();
    std::fs::write(r.join(artifact), b"artifact bytes\n").unwrap();
    std::fs::write(r.join("note.txt"), b"baseline\n").unwrap();
    native(r, &["add", "-A", "--"], None);
    native(r, &["commit", "-q", "-m", "initial"], None);
    d
}

#[tokio::test]
async fn hygiene_retirement_refuses_committed_symlink_without_touching_target_or_index() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let p = policy(true);
    std::os::unix::fs::symlink("dist/", r.join(".gitignore")).unwrap();
    native(r, &["add", "--", ".gitignore"], None);
    native(r, &["commit", "-q", "-m", "symlinked ignore fixture"], None);
    let artifact = std::fs::read(r.join("dist/extra.bin")).unwrap();
    let inode = std::fs::symlink_metadata(r.join(".gitignore"))
        .unwrap()
        .ino();
    let baseline = staged_epoch::Epoch::capture(r, 15).await.unwrap();
    let staging = staged_epoch::Staging::new(r, baseline).await.unwrap();
    let _epoch = staging
        .finish(r, &p, &excluded(), true, false)
        .await
        .unwrap();
    native(r, &["commit", "-q", "-m", "fixture artifact deindex"], None);
    let index = std::fs::read(r.join(".git/index")).unwrap();
    assert!(native(r, &["ls-files", "--", "dist/extra.bin"], None).is_empty());
    assert!(
        super::retry(
            r,
            &p,
            &excluded(),
            true,
            Instant::now() + Duration::from_secs(15)
        )
        .await
        .is_err(),
        "symlink text must not establish committed ignore rules"
    );
    assert!(super::has_pending(r).unwrap());
    assert_eq!(
        std::fs::read_link(r.join(".gitignore")).unwrap(),
        PathBuf::from("dist/")
    );
    assert_eq!(
        std::fs::symlink_metadata(r.join(".gitignore"))
            .unwrap()
            .ino(),
        inode
    );
    assert_eq!(std::fs::read(r.join("dist/extra.bin")).unwrap(), artifact);
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
}

#[tokio::test]
async fn hygiene_retirement_leading_space_is_not_an_effective_artifact_rule() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let p = policy(true);
    std::fs::write(r.join(".gitignore"), b" dist/\n").unwrap();
    native(r, &["add", "--", ".gitignore"], None);
    native(
        r,
        &["commit", "-q", "-m", "leading-space ignore fixture"],
        None,
    );
    let ignored = |path: &str| {
        let mut cmd = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        let out = cmd
            .args([
                "--no-optional-locks",
                "-C",
                r.to_str().unwrap(),
                "check-ignore",
                "--no-index",
                "--quiet",
                "--",
                path,
            ])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            matches!(out.status.code(), Some(0 | 1)),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.status.success()
    };
    std::fs::write(r.join("dist/new"), b"probe retained\n").unwrap();
    assert!(
        !ignored("dist/new"),
        "the original leading-space rule does not match dist"
    );
    let baseline = staged_epoch::Epoch::capture(r, 15).await.unwrap();
    let staging = staged_epoch::Staging::new(r, baseline).await.unwrap();
    let _epoch = staging
        .finish(r, &p, &excluded(), true, false)
        .await
        .unwrap();
    native(r, &["commit", "-q", "-m", "fixture artifact deindex"], None);
    super::retry(
        r,
        &p,
        &excluded(),
        true,
        Instant::now() + Duration::from_secs(15),
    )
    .await
    .unwrap();
    assert!(
        super::has_pending(r).unwrap(),
        "uncommitted canonical correction cannot retire intent"
    );
    let text = std::fs::read_to_string(r.join(".gitignore")).unwrap();
    assert!(
        text.starts_with(" dist/\n"),
        "preserve the operator's original line"
    );
    assert!(
        text.lines().any(|line| line == "dist/"),
        "append the actual exact rule"
    );
    assert!(ignored("dist/new"));
    native(r, &["add", "--", ".gitignore"], None);
    native(r, &["commit", "-q", "-m", "fixture corrected ignore"], None);
    super::retry(
        r,
        &p,
        &excluded(),
        true,
        Instant::now() + Duration::from_secs(15),
    )
    .await
    .unwrap();
    assert!(!super::has_pending(r).unwrap());
    assert_eq!(
        std::fs::read(r.join("dist/new")).unwrap(),
        b"probe retained\n"
    );
}

#[tokio::test]
async fn hygiene_durable_retry_survives_production_commit_and_process_exit_with_writer_refusal() {
    const ROLE: &str = "DRACON_HYGIENE_DURABLE_REPO";
    if let Some(path) = std::env::var_os(ROLE) {
        let r = PathBuf::from(path);
        let mut p = policy(true);
        p.max_stage_file_bytes = 16 * 1024;
        let mut names = excluded();
        names.insert("custom-build-cache".into());
        let _writer = std::fs::OpenOptions::new()
            .write(true)
            .open(r.join(".gitignore"))
            .unwrap();
        std::fs::write(r.join("note.txt"), b"eligible real production edit\n").unwrap();
        let result = sync_repo(&r, &p, &names, 0, None, false, None)
            .await
            .unwrap();
        assert!(
            matches!(result, SyncOutcome::StagedPending),
            "refused metadata must remain pending: {result:?}"
        );
        assert!(native(&r, &["ls-files", "--", "custom-build-cache/a.bin"], None).is_empty());
        assert!(super::has_pending(&r).unwrap());
        // This process actually exits: subsequent retry cannot use Epoch memory.
        std::process::exit(0);
    }
    let d = fixture("dist/extra.bin");
    let r = d.path();
    std::fs::create_dir(r.join("custom-build-cache")).unwrap();
    std::fs::write(r.join("custom-build-cache/a.bin"), b"artifact retained\n").unwrap();
    native(r, &["add", "--", "custom-build-cache/a.bin"], None);
    native(r, &["commit", "-q", "-m", "custom tracked artifact"], None);
    let mut p = policy(false);
    p.max_stage_file_bytes = 16 * 1024;
    let mut names = excluded();
    names.insert("custom-build-cache".into());
    std::fs::write(r.join("note.txt"), b"warm-up\n").unwrap();
    for _ in 0..4 {
        sync_repo(r, &p, &names, 0, None, false, None)
            .await
            .unwrap();
        if native(r, &["status", "--porcelain"], None).is_empty() {
            break;
        }
    }
    // Existing ignored metadata must be stable before the actual writer control.
    if !r.join(".gitignore").exists() {
        std::fs::write(r.join(".gitignore"), b"# original\n").unwrap();
        native(r, &["add", "--", ".gitignore"], None);
        native(r, &["commit", "-q", "-m", "ignore baseline"], None);
    }
    assert!(!std::fs::read_to_string(r.join(".gitignore"))
        .unwrap()
        .contains("custom-build-cache/"));
    let inode = std::fs::metadata(r.join(".gitignore")).unwrap().ino();
    let output=tokio::time::timeout(Duration::from_secs(40),tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact","sync::hygiene::tests::hygiene_durable_retry_survives_production_commit_and_process_exit_with_writer_refusal","--nocapture"])
        .env(ROLE,r).kill_on_drop(true).output()).await.unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::metadata(r.join(".gitignore")).unwrap().ino(),
        inode
    );
    assert!(super::has_pending(r).unwrap());
    let mut text = std::fs::read_to_string(r.join(".gitignore")).unwrap();
    text.push_str("\n# newer canonical operator edit\n");
    std::fs::write(r.join(".gitignore"), text).unwrap();
    p.build_artifact_cleanup = true;
    for _ in 0..4 {
        sync_repo(r, &p, &names, 0, None, false, None)
            .await
            .unwrap();
        if !super::has_pending(r).unwrap() {
            break;
        }
    }
    assert!(
        !super::has_pending(r).unwrap(),
        "committed metadata must retire actual durable intent"
    );
    let committed = native(r, &["show", "HEAD:.gitignore"], None);
    let committed = String::from_utf8(committed).unwrap();
    assert!(
        committed.contains("custom-build-cache/\n")
            && committed.contains("# newer canonical operator edit\n")
    );
    assert_eq!(
        std::fs::read(r.join("custom-build-cache/a.bin")).unwrap(),
        b"artifact retained\n"
    );
}

#[tokio::test]
async fn hygiene_durable_uncommitted_candidate_recovers_only_exact_owned_tree_after_restart() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let mut p = policy(true);
    p.max_stage_file_bytes = 16 * 1024;
    let baseline = staged_epoch::Epoch::capture(r, 20).await.unwrap();
    let staging = staged_epoch::Staging::new(r, baseline).await.unwrap();
    let owned = staging
        .finish(r, &p, &excluded(), true, false)
        .await
        .unwrap();
    drop(owned);
    assert!(super::has_pending(r).unwrap());
    // A different cached payload must NOT inherit deletion authority.
    std::fs::write(r.join("foreign.txt"), b"foreign cached intent\n").unwrap();
    native(r, &["add", "--", "foreign.txt"], None);
    let foreign = StagedIntentSnapshot::capture(r, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    assert!(
        super::recover(r, &foreign, Instant::now() + Duration::from_secs(10))
            .await
            .unwrap()
            .is_empty()
    );
    native(r, &["restore", "--staged", "--", "foreign.txt"], None);
    // Preserve that unrelated worktree edit; exact original cached tree can resume.
    let head = native(r, &["rev-parse", "HEAD"], None);
    sync_repo(r, &p, &excluded(), 0, None, false, None)
        .await
        .unwrap();
    assert_ne!(
        native(r, &["rev-parse", "HEAD"], None),
        head,
        "recorded own deletion must not deadlock behind metadata retry"
    );
    assert!(native(r, &["ls-files", "--", "dist/extra.bin"], None).is_empty());
    assert_eq!(
        std::fs::read(r.join("foreign.txt")).unwrap(),
        b"foreign cached intent\n"
    );
}

#[tokio::test]
async fn hygiene_durable_policy_optout_and_symlink_registry_refuse_without_mutation() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let p = policy(true);
    let baseline = staged_epoch::Epoch::capture(r, 15).await.unwrap();
    let staging = staged_epoch::Staging::new(r, baseline).await.unwrap();
    let _owned = staging
        .finish(r, &p, &excluded(), true, false)
        .await
        .unwrap();
    let index = std::fs::read(r.join(".git/index")).unwrap();
    assert!(super::retry(
        r,
        &p,
        &excluded(),
        false,
        Instant::now() + Duration::from_secs(10)
    )
    .await
    .is_err());
    assert!(super::has_pending(r).unwrap());
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
    assert!(!r.join(".gitignore").exists());
    let other = fixture("dist/extra.bin");
    let canary = tempfile::tempdir().unwrap();
    std::fs::write(canary.path().join("canary"), b"foreign bytes").unwrap();
    std::os::unix::fs::symlink(
        canary.path(),
        other.path().join(".git/dracon-hygiene-intents"),
    )
    .unwrap();
    assert!(super::has_pending(other.path()).is_err());
    assert_eq!(
        std::fs::read(canary.path().join("canary")).unwrap(),
        b"foreign bytes"
    );
}

#[tokio::test]
async fn hygiene_durable_malformed_record_is_inspection_failure_not_idle_or_permission_to_write() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let p = policy(true);
    let baseline = staged_epoch::Epoch::capture(r, 15).await.unwrap();
    let staging = staged_epoch::Staging::new(r, baseline).await.unwrap();
    let _owned = staging
        .finish(r, &p, &excluded(), true, false)
        .await
        .unwrap();
    let root = r.join(".git/dracon-hygiene-intents");
    let directory = std::fs::read_dir(&root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(directory.join("intent.json"), b"{invalid JSON\n").unwrap();
    let index = std::fs::read(r.join(".git/index")).unwrap();
    let artifact = std::fs::read(r.join("dist/extra.bin")).unwrap();
    assert!(super::has_pending(r).is_err());
    assert!(super::retry(
        r,
        &p,
        &excluded(),
        true,
        Instant::now() + Duration::from_secs(10)
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(r.join("dist/extra.bin")).unwrap(), artifact);
    assert!(!r.join(".gitignore").exists());
}

#[tokio::test]
async fn hygiene_large_only_production_cycle_is_admitted_after_clean_warmup() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let mut p = policy(false);
    p.max_stage_file_bytes = 16 * 1024;
    // Finish hardening and prior ordinary work BEFORE the qualifying cycle.
    std::fs::write(r.join("note.txt"), b"warm-up ordinary edit\n").unwrap();
    for _ in 0..4 {
        let result = sync_repo(r, &p, &excluded(), 0, None, false, None).await;
        assert!(result.is_ok(), "warm-up failed: {result:?}");
        if native(r, &["status", "--porcelain", "--untracked-files=all"], None).is_empty() {
            break;
        }
    }
    assert!(
        native(r, &["status", "--porcelain", "--untracked-files=all"], None).is_empty(),
        "qualifying cycle requires clean prior work"
    );
    let head = native(r, &["rev-parse", "HEAD"], None);
    let large = r.join("huge-only.iso");
    let content = vec![b'L'; 64 * 1024];
    std::fs::write(&large, &content).unwrap();
    let inode = std::fs::metadata(&large).unwrap().ino();
    assert_eq!(
        native(r, &["status", "--porcelain", "--untracked-files=all"], None),
        b"?? huge-only.iso\n"
    );
    let result = sync_repo(r, &p, &excluded(), 0, None, false, None)
        .await
        .unwrap();
    assert!(
        matches!(result, SyncOutcome::StagedPending),
        "new ignore metadata must owe follow-up: {result:?}"
    );
    assert!(std::fs::read_to_string(r.join(".gitignore"))
        .unwrap()
        .contains("/huge-only.iso\n"));
    assert_eq!(std::fs::read(&large).unwrap(), content);
    assert_eq!(std::fs::metadata(&large).unwrap().ino(), inode);
    assert_eq!(
        native(r, &["rev-parse", "HEAD"], None),
        head,
        "large-only reconciliation must not manufacture a commit"
    );
}

#[tokio::test]
async fn hygiene_large_plan_cannot_expand_beyond_partition_or_override_opt_outs() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    for path in ["allowed.iso", "private.iso", "local.iso", "excluded.iso"] {
        std::fs::write(r.join(path), vec![b'X'; 4096]).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let baseline = StagedIntentSnapshot::capture(r, deadline).await.unwrap();
    let mut p = policy(false);
    p.untracked_exclude_patterns = vec!["local.iso".into()];
    p.auto_commit_exclude_patterns = vec!["excluded.iso".into()];
    let admitted = [
        PathBuf::from("allowed.iso"),
        PathBuf::from("local.iso"),
        PathBuf::from("excluded.iso"),
    ];
    let plan = super::plan(r, &p, &excluded(), &baseline, false, &admitted, deadline)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.patterns,
        vec!["/allowed.iso"],
        "private paths absent from partition and explicit exclusions must never be hidden"
    );
    p.auto_stage_untracked = false;
    assert!(
        super::plan(r, &p, &excluded(), &baseline, false, &admitted, deadline)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !r.join(".gitignore").exists(),
        "planning cannot change canonical metadata"
    );
    assert!(native(r, &["diff", "--cached", "--name-only"], None).is_empty());
}

#[tokio::test]
async fn hygiene_successful_ignore_write_reports_followup_until_metadata_is_staged() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    std::fs::write(r.join(".gitignore"), b"# operator-owned original\n").unwrap();
    native(r, &["add", "--", ".gitignore"], None);
    native(r, &["commit", "-q", "-m", "tracked ignore baseline"], None);
    let head = native(r, &["rev-parse", "HEAD"], None);
    let index = std::fs::read(r.join(".git/index")).unwrap();
    let result = super::super::apply_deferred_ignore(
        r,
        Some((
            vec!["cache/".into()],
            Instant::now() + Duration::from_secs(15),
        )),
    )
    .await
    .unwrap();
    assert!(
        result.is_some(),
        "actual canonical write still owes local staging/publication"
    );
    assert_eq!(native(r, &["diff", "--name-only"], None), b".gitignore\n");
    assert_eq!(native(r, &["rev-parse", "HEAD"], None), head);
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
    let bytes = std::fs::read_to_string(r.join(".gitignore")).unwrap();
    assert!(bytes.contains("# operator-owned original\n"));
    assert!(bytes.contains("cache/\n"));
    let no_change = super::super::apply_deferred_ignore(
        r,
        Some((
            vec!["cache/".into()],
            Instant::now() + Duration::from_secs(15),
        )),
    )
    .await
    .unwrap();
    assert!(
        no_change.is_none(),
        "idempotent rendering must not manufacture more work"
    );
}

#[tokio::test]
async fn hygiene_production_sync_untracks_artifacts_ignores_large_and_keeps_bytes() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let artifact = std::fs::read(r.join("dist/extra.bin")).unwrap();
    let large = r.join("huge.iso");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(4096)
        .unwrap();
    std::fs::write(r.join("note.txt"), b"operator edit\n").unwrap();
    let result = sync_repo(r, &policy(true), &excluded(), 0, None, false, None).await;
    assert!(result.is_ok(), "production hygiene sync failed: {result:?}");
    assert!(
        native(r, &["ls-files", "--", "dist/extra.bin"], None).is_empty(),
        "artifact must be de-indexed"
    );
    assert_eq!(
        std::fs::read(r.join("dist/extra.bin")).unwrap(),
        artifact,
        "artifact WORKTREE bytes are preserved"
    );
    assert!(
        std::fs::metadata(&large).unwrap().len() == 4096,
        "oversized file is ignored, never truncated or committed"
    );
    let ignore = std::fs::read_to_string(r.join(".gitignore")).unwrap();
    assert!(ignore.contains("/huge.iso"));
    assert!(ignore.contains("dist/"));
    let tracked = String::from_utf8(native(r, &["ls-files"], None)).unwrap();
    assert!(!tracked.contains("huge.iso"));
    assert!(tracked.contains("note.txt"));
    // The operator's ordinary edit was still committed by the same daemon path.
    assert_eq!(
        native(r, &["show", "HEAD:note.txt"], None),
        b"operator edit\n"
    );
}

#[tokio::test]
async fn hygiene_opt_out_and_dry_run_leave_tracking_and_ignore_untouched() {
    for (cleanup, dry) in [(false, false), (true, true)] {
        let d = fixture("dist/extra.bin");
        let r = d.path();
        let before = String::from_utf8(native(r, &["ls-files"], None)).unwrap();
        let result = sync_repo(r, &policy(cleanup), &excluded(), 0, None, dry, None).await;
        assert!(result.is_ok(), "{}", result.unwrap_err());
        assert_eq!(
            String::from_utf8(native(r, &["ls-files"], None)).unwrap(),
            before
        );
        assert!(
            !r.join(".gitignore").exists(),
            "dry-run/opt-out must not write ignore rules"
        );
    }
}

#[tokio::test]
async fn hygiene_never_absorbs_foreign_operator_staging() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    std::fs::write(r.join("note.txt"), b"operator staged\n").unwrap();
    native(r, &["add", "--", "note.txt"], None);
    let staged = native(r, &["rev-parse", ":note.txt"], None);
    let before = String::from_utf8(native(r, &["ls-files"], None)).unwrap();
    let _ = sync_repo(r, &policy(true), &excluded(), 0, None, false, None).await;
    assert_eq!(
        native(r, &["rev-parse", ":note.txt"], None),
        staged,
        "operator staged identity must survive"
    );
    assert!(
        String::from_utf8(native(r, &["ls-files"], None))
            .unwrap()
            .contains("dist/extra.bin"),
        "cleanup must not run around foreign staged intent"
    );
    assert!(before.contains("dist/extra.bin"));
}

#[tokio::test]
async fn hygiene_preserves_active_writer_and_untracked_large_bytes() {
    use std::io::Write;
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let large = r.join("huge.iso");
    let mut writer = std::fs::File::create(&large).unwrap();
    writer.set_len(4096).unwrap();
    let inode = std::fs::metadata(&large).unwrap().ino();
    let _ = sync_repo(r, &policy(true), &excluded(), 0, None, false, None).await;
    writer.write_all(b"LATE").unwrap();
    writer.sync_all().unwrap();
    assert_eq!(
        std::fs::metadata(&large).unwrap().ino(),
        inode,
        "an oversized operator file must not be relocated by an ignore rule"
    );
    assert!(std::fs::read(&large).unwrap().starts_with(b"LATE"));
}

#[tokio::test]
async fn hygiene_generated_ignore_waits_for_writer_fence_before_capture_and_recovers() {
    use std::io::Write;
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let path = r.join(".gitignore");
    std::fs::write(&path, b"# original\n").unwrap();
    let held = crate::git::write_fence::WriteFence::acquire_until(
        r, Instant::now() + Duration::from_secs(1),
    ).await.unwrap();
    let mut writer = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    let index = std::fs::read(r.join(".git/index")).unwrap();
    let blocked = super::super::hygiene_apply::apply(
        r, &["cache/".into()], Instant::now() + Duration::from_millis(30),
    ).await;
    assert!(blocked.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"# original\n");
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
    writer.write_all(b"# writer finished\n").unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    drop(held);
    let result = super::super::hygiene_apply::apply(
        r, &["cache/".into()], Instant::now() + Duration::from_secs(15),
    ).await.unwrap();
    assert!(result.0, "publication must recover after the accepted writer closes");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# writer finished\n"), "capture must include the completed writer's bytes");
    assert!(text.contains("cache/"));
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
}

#[tokio::test]
async fn hygiene_generated_ignore_preserves_existing_writer_and_visible_recovery() {
    use std::io::Write;
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let path = r.join(".gitignore");
    std::fs::write(&path, b"operator original\n").unwrap();
    let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let before = std::fs::read(r.join(".git/index")).unwrap();
    let inode = writer.metadata().unwrap().ino();
    let result = super::super::hygiene_apply::apply(
        r,
        &["cache/".into()],
        Instant::now() + Duration::from_secs(15),
    )
    .await
    .unwrap();
    assert!(
        !result.0 && result.1.is_some(),
        "writer exclusion must defer actual application"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"operator original\n");
    writer.write_all(b"LATER").unwrap();
    writer.sync_all().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
    assert!(std::fs::read(&path).unwrap().starts_with(b"LATER"));
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), before);
    let archive = r.join(".git/dracon-excluded-archive");
    assert!(
        std::fs::read_dir(archive).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("dracon-excluded-recovery-")),
        "generated recovery must use the concern observer's recognized namespace"
    );
}

#[tokio::test]
async fn hygiene_generated_ignore_rejects_symlink_and_hardlink_without_modifying_targets() {
    use std::os::unix::fs::symlink;
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let canary = r.join("canary");
    std::fs::write(&canary, b"unrelated operator content\n").unwrap();
    let path = r.join(".gitignore");
    let before = std::fs::read(r.join(".git/index")).unwrap();
    for linked in [true, false] {
        if linked {
            symlink(&canary, &path).unwrap()
        } else {
            std::fs::hard_link(&canary, &path).unwrap()
        }
        assert!(super::super::hygiene_apply::apply(
            r,
            &["cache/".into()],
            Instant::now() + Duration::from_secs(10)
        )
        .await
        .is_err());
        assert_eq!(
            std::fs::read(&canary).unwrap(),
            b"unrelated operator content\n"
        );
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), before);
        std::fs::remove_file(&path).unwrap();
    }
}

#[tokio::test]
async fn hygiene_generated_ignore_never_adopts_intervening_content_as_old_baseline() {
    const ROLE: &str = "DRACON_HYGIENE_IGNORE_RACE_REPO";
    if let Some(repo) = std::env::var_os(ROLE) {
        let r = PathBuf::from(repo);
        let result = super::super::hygiene_apply::apply(
            &r,
            &["cache/".into()],
            Instant::now() + Duration::from_secs(15),
        )
        .await
        .unwrap();
        assert!(
            !result.0 && result.1.is_some(),
            "newer canonical bytes must REFUSE an older render"
        );
        assert_eq!(
            std::fs::read(r.join(".gitignore")).unwrap(),
            b"foreign newer content\n"
        );
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let d = fixture("dist/extra.bin");
    let r = d.path();
    std::fs::write(r.join(".gitignore"), b"observed original\n").unwrap();
    let before = std::fs::read(r.join(".git/index")).unwrap();
    let wrapper = r.join(".git/race-ignore-helper");
    std::fs::write(&wrapper,b"#!/bin/sh\nprintf 'foreign newer content\\n' > \"$DRACON_HYGIENE_IGNORE_RACE_REPO/.gitignore\"\nexec \"$DRACON_HYGIENE_REAL_HELPER\" \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output=tokio::time::timeout(Duration::from_secs(25),tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact","sync::hygiene::tests::hygiene_generated_ignore_never_adopts_intervening_content_as_old_baseline","--nocapture"])
        .env(ROLE,r).env("DRACON_EPOCH_TEST_HELPER",&wrapper)
        .env("DRACON_HYGIENE_REAL_HELPER",std::env::var_os("DRACON_EPOCH_TEST_HELPER").unwrap())
        .kill_on_drop(true).output()).await.unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), before);
    assert_eq!(
        std::fs::read(r.join(".gitignore")).unwrap(),
        b"foreign newer content\n"
    );
}
