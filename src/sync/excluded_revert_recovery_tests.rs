use super::*;

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let out = crate::git::git_cmd()
        .arg("--no-optional-locks")
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
async fn fixture() -> (tempfile::TempDir, PathBuf, Vec<RestoreTarget>) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["config", "user.name", "test"]);
    git(r, &["config", "user.email", "test@test"]);
    let hooks = r.join(".git/empty-hooks");
    std::fs::create_dir(&hooks).unwrap();
    git(r, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    for name in ["one.log", "two.log"] {
        std::fs::write(r.join(name), format!("HEAD {name}\n")).unwrap();
    }
    git(r, &["add", "--", "one.log", "two.log"]);
    git(r, &["commit", "-q", "-m", "initial"]);
    let pending = tempfile::Builder::new()
        .prefix("dracon-excluded-recovery-")
        .tempdir_in(r.join(".git"))
        .unwrap()
        .keep();
    let tree = pending.join("files");
    std::fs::create_dir(&tree).unwrap();
    let mut targets = Vec::new();
    for name in ["one.log", "two.log"] {
        std::fs::write(tree.join(name), std::fs::read(r.join(name)).unwrap()).unwrap();
        std::fs::write(r.join(name), format!("operator {name}\n")).unwrap();
        targets.push(RestoreTarget {
            path: name.as_bytes().to_vec(),
            before: Location::open(r, Path::new(name))
                .unwrap()
                .fingerprint()
                .await
                .unwrap(),
            desired: Location::open(&tree, Path::new(name))
                .unwrap()
                .fingerprint()
                .await
                .unwrap()
                .unwrap(),
        });
    }
    (d, pending, targets)
}
fn request(repo: &Path, pending: &Path, targets: Vec<RestoreTarget>) -> PathBuf {
    let req = RestoreRequest {
        source_id: env!("DRACON_EPOCH_SOURCE_ID").into(),
        repo: repo.as_os_str().as_bytes().to_vec(),
        head: String::from_utf8(git(repo, &["rev-parse", "HEAD"]))
            .unwrap()
            .trim()
            .into(),
        targets,
    };
    let path = pending.join("recovery.json");
    std::fs::write(&path, serde_json::to_vec(&req).unwrap()).unwrap();
    path
}
fn exchange(repo: &Path, pending: &Path, name: &str) {
    Location::open(repo, Path::new(name))
        .unwrap()
        .exchange(
            &Location::open(&pending.join("files"), Path::new(name)).unwrap(),
            libc::RENAME_EXCHANGE,
        )
        .unwrap();
}
#[tokio::test]
async fn excluded_recovery_partial_transaction_settles_without_replaying_or_resetting_user_work() {
    let (d, pending, targets) = fixture().await;
    let r = d.path();
    request(r, &pending, targets);
    exchange(r, &pending, "one.log");
    std::fs::write(r.join("one.log"), b"later canonical producer\n").unwrap();
    let index = std::fs::read(r.join(".git/index")).unwrap();
    assert!(super::super::has_pending_excluded_recovery(r).unwrap());
    assert!(settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap());
    assert!(!super::super::has_pending_excluded_recovery(r).unwrap());
    assert_eq!(
        std::fs::read(r.join("one.log")).unwrap(),
        b"later canonical producer\n"
    );
    assert_eq!(
        std::fs::read(r.join("two.log")).unwrap(),
        b"operator two.log\n"
    );
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
    let archived = r
        .join(".git/dracon-excluded-archive")
        .join(pending.file_name().unwrap());
    assert_eq!(
        std::fs::read(archived.join("files/one.log")).unwrap(),
        b"operator one.log\n"
    );
    assert_eq!(
        std::fs::read(archived.join("files/two.log")).unwrap(),
        b"HEAD two.log\n"
    );
}
#[tokio::test]
async fn excluded_recovery_uncertain_displaced_bytes_block_a_raw_clean_repo_even_without_optin() {
    let (d, pending, targets) = fixture().await;
    let r = d.path();
    request(r, &pending, targets);
    exchange(r, &pending, "one.log");
    exchange(r, &pending, "two.log");
    assert!(git(r, &["status", "--porcelain=v1", "--untracked-files=all"]).is_empty());
    std::fs::write(
        pending.join("files/one.log"),
        b"unknown later displaced bytes",
    )
    .unwrap();
    let index = std::fs::read(r.join(".git/index")).unwrap();
    assert!(!settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap());
    assert!(super::super::has_pending_excluded_recovery(r).unwrap());
    let policy:SyncPolicy=toml::from_str("auto_github_private=false\nauto_commit=true\nauto_pull=false\nauto_push=false\nauto_bump_versions=false\ntrusted_emails=['test@test']\ntrusted_authors=['test']\nremotes=[]\n").unwrap();
    assert_eq!(
        super::super::sync_repo(r, &policy, &BTreeSet::new(), 0, None, false, None)
            .await
            .unwrap(),
        SyncOutcome::Blocked
    );
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(pending.join("files/one.log")).unwrap(),
        b"unknown later displaced bytes"
    );
    assert!(git(r, &["status", "--porcelain=v1", "--untracked-files=all"]).is_empty());
    let hint = crate::report::repo_hint(
        &["INTENTIONAL_NO_UPSTREAM".into(), "EXCLUDED_RECOVERY".into()],
        false,
        true,
    );
    assert!(hint.contains("recovery unresolved") && hint.contains("retain canonical"));
}
#[tokio::test]
async fn excluded_recovery_foreign_lock_prevents_settlement_and_remains_exact() {
    let (d, pending, targets) = fixture().await;
    let r = d.path();
    request(r, &pending, targets);
    std::fs::write(r.join(".git/index.lock"), b"foreign live operation").unwrap();
    assert!(!settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap());
    assert!(pending.exists());
    assert_eq!(
        std::fs::read(r.join(".git/index.lock")).unwrap(),
        b"foreign live operation"
    );
}
#[tokio::test]
async fn excluded_recovery_cancel_while_helper_holds_real_leases_releases_writer_and_never_restores_late(
) {
    let (d, pending, mut targets) = fixture().await;
    let r = d.path();
    let large = std::fs::OpenOptions::new()
        .write(true)
        .open(r.join("one.log"))
        .unwrap();
    large.set_len(512 * 1024 * 1024).unwrap();
    drop(large);
    targets[0].before = Location::open(r, Path::new("one.log"))
        .unwrap()
        .fingerprint()
        .await
        .unwrap();
    let req = request(r, &pending, targets);
    let meta = std::fs::metadata(r.join("one.log")).unwrap();
    let expected = format!(
        "{:02x}:{:02x}:{}",
        libc::major(meta.dev()),
        libc::minor(meta.dev()),
        meta.ino()
    );
    let executable = std::env::var_os("DRACON_EPOCH_TEST_HELPER").expect("pinned matching helper");
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--dracon-excluded-restore-helper")
        .arg(req)
        .arg(std::process::id().to_string())
        .env("TOKIO_WORKER_THREADS", "2");
    let running = tokio::spawn(async move {
        crate::git::run_command_captured_output(command, 30, "lease cancellation witness", None)
            .await
    });
    let start = Instant::now();
    let mut observed = false;
    while start.elapsed() < Duration::from_secs(8) {
        let locks = std::fs::read_to_string("/proc/locks").unwrap();
        if locks.lines().any(|line| {
            line.contains("LEASE")
                && line.contains("ACTIVE")
                && line.contains("READ")
                && line.split_whitespace().any(|field| field == expected)
        }) {
            observed = true;
            break;
        }
        if running.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(
        observed,
        "the actual helper must hold a kernel lease before the cancellation claim"
    );
    running.abort();
    let _ = running.await;
    let mut writer = None;
    use std::os::unix::fs::OpenOptionsExt;
    for _ in 0..80 {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(r.join("one.log"))
        {
            Ok(file) => {
                writer = Some(file);
                break;
            }
            Err(error) if error.raw_os_error() == Some(libc::EAGAIN) => {
                tokio::time::sleep(Duration::from_millis(25)).await
            }
            Err(error) => panic!("writer reopen failed: {error}"),
        }
    }
    let mut writer =
        writer.expect("cancelled helper must release its real lease within finite bounds");
    writer.write_all(b"LATER").unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut prefix = [0u8; 5];
    use std::io::Read;
    File::open(r.join("one.log"))
        .unwrap()
        .read_exact(&mut prefix)
        .unwrap();
    assert_eq!(
        &prefix, b"LATER",
        "no detached restore may overwrite later producer bytes"
    );
    assert_eq!(
        std::fs::metadata(r.join("one.log")).unwrap().ino(),
        meta.ino()
    );
    assert!(super::super::has_pending_excluded_recovery(r).unwrap());
    assert!(settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap());
    assert!(!super::super::has_pending_excluded_recovery(r).unwrap());
}
