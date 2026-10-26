//! Production-path controls for hygiene: real sync_repo positives, admission of
//! large-file-only work, opt-out/dry-run scope, foreign-intent refusal and
//! writer-safety. Fixtures are disposable; no installed-binary or fleet claim.
use super::*;
use std::os::unix::fs::PermissionsExt;

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

fn policy(cleanup: bool) -> SyncPolicy {
    toml::from_str(&format!(
        "auto_github_private=false\nauto_commit=true\nauto_pull=false\nauto_push=false\n\
         auto_bump_versions=false\nauto_resolve_unmerged=false\nbuild_artifact_cleanup={cleanup}\n\
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
async fn hygiene_production_sync_untracks_artifacts_ignores_large_and_keeps_bytes() {
    let d = fixture("dist/extra.bin");
    let r = d.path();
    let artifact = std::fs::read(r.join("dist/extra.bin")).unwrap();
    let large = r.join("huge.iso");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(4 * 1024 * 1024)
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
    assert!(std::fs::read_to_string(r.join("huge.iso"))
        .unwrap()
        .is_empty());
    assert!(
        std::fs::metadata(&large).unwrap().len() == 4 * 1024 * 1024,
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
    writer.set_len(4 * 1024 * 1024).unwrap();
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
