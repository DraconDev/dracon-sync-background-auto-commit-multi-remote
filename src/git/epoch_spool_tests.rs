use super::*;
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn command(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .current_dir(repo);
    cmd
}
fn git(repo: &Path, args: &[&str]) -> Output {
    let out = command(repo).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "Git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn indexed_repo(path: &Path, label: &str) -> (String, String) {
    fs::create_dir(path).unwrap();
    git(path, &["init", "-q", "--template="]);
    fs::write(path.join("path with spaces"), label.as_bytes()).unwrap();
    git(path, &["add", "--", "path with spaces"]);
    let blob = String::from_utf8(git(path, &["rev-parse", ":path with spaces"]).stdout)
        .unwrap()
        .trim()
        .to_owned();
    let tree = String::from_utf8(git(path, &["write-tree"]).stdout)
        .unwrap()
        .trim()
        .to_owned();
    (blob, tree)
}
fn pack(repo: &Path, spool: &Path, label: &str, objects: &[String]) {
    let mut child = command(repo)
        .arg("pack-objects")
        .arg(spool.join(label))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all((objects.join("\n") + "\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "pack fixture failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    spool: PathBuf,
    unique: Vec<String>,
}
fn fixture(mixed: bool, sealed: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("main");
    let (blob, tree) = indexed_repo(&repo, "baseline bytes");
    let spool = repo.join(".git/dracon-staging-fixture");
    fs::create_dir(&spool).unwrap();
    pack(&repo, &spool, "retained-main", &[blob, tree]);
    let mut unique = Vec::new();
    let index = if mixed {
        let foreign = root.path().join("foreign");
        let (blob, tree) = indexed_repo(&foreign, "unique bytes not in main");
        unique = vec![blob, tree];
        pack(&foreign, &spool, "retained-foreign", &unique);
        foreign.join(".git/index")
    } else {
        repo.join(".git/index")
    };
    fs::copy(index, spool.join("index")).unwrap();
    fs::write(spool.join("notes"), b"metadata\0\xff").unwrap();
    if sealed {
        let mut owner = SpoolLease::acquire(&spool).unwrap();
        owner.settle_empty_fixture().unwrap();
    }
    Fixture {
        _root: root,
        repo,
        spool,
        unique,
    }
}
fn backdate(spool: &Path) {
    fs::File::open(spool)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
}
async fn attempt(f: &Fixture, fault: reclaim::Fault) -> anyhow::Result<recovery::DurableReceipt> {
    reclaim::attempt(&f.repo, &f.spool, 30, fault).await
}

#[tokio::test]
async fn duplicate_spool_is_reaped() {
    let f = fixture(false, true);
    let index = fs::read(f.spool.join("index")).unwrap();
    let receipt = attempt(&f, reclaim::Fault::None).await.unwrap();
    assert!(!f.spool.exists());
    assert_eq!(
        receipt.unique_objects, 0,
        "classification must distinguish duplicates from mixed payloads"
    );
    assert!(receipt.rescue_pack.is_none());
    assert_eq!(
        fs::read(receipt.record.join("metadata/index")).unwrap(),
        index
    );
    assert_eq!(
        fs::read(receipt.record.join("metadata/notes")).unwrap(),
        b"metadata\0\xff"
    );
    assert!(receipt.record.join("reclaimed.json").is_file());
    git(&f.repo, &["gc", "--prune=now"]);
    git(&receipt.record.join("store.git"), &["gc", "--prune=now"]);
    recovery::verify_receipt(&receipt.record, &receipt.manifest_sha256, 10)
        .await
        .unwrap();
}
#[tokio::test]
async fn spool_with_unique_objects_is_rescued_then_reaped() {
    let f = fixture(true, true);
    let index = fs::read(f.spool.join("index")).unwrap();
    for object in &f.unique {
        assert!(!command(&f.repo)
            .args(["cat-file", "-e", object])
            .status()
            .unwrap()
            .success());
    }
    let receipt = attempt(&f, reclaim::Fault::None).await.unwrap();
    assert!(!f.spool.exists());
    assert_eq!(receipt.unique_objects, f.unique.len());
    assert_eq!(
        fs::read(receipt.record.join("metadata/index")).unwrap(),
        index
    );
    let rescue = receipt.rescue_pack.as_ref().unwrap();
    assert!(rescue.is_file() && rescue.with_extension("idx").is_file());
    let standalone = f._root.path().join("standalone");
    fs::create_dir(&standalone).unwrap();
    git(&standalone, &["init", "-q", "--bare", "--template="]);
    fs::copy(rescue, standalone.join("objects/pack/pack-check.pack")).unwrap();
    fs::copy(
        rescue.with_extension("idx"),
        standalone.join("objects/pack/pack-check.idx"),
    )
    .unwrap();
    for object in &f.unique {
        git(&standalone, &["cat-file", "-p", object]);
    }
    let saved = f._root.path().join("saved-record");
    fs::rename(&receipt.record, &saved).unwrap();
    fs::rename(&f.repo, f._root.path().join("main-unavailable")).unwrap();
    recovery::verify_receipt(&saved, &receipt.manifest_sha256, 10)
        .await
        .unwrap();
}
#[tokio::test]
async fn post_write_rescue_verification_failure_preserves_original() {
    let f = fixture(true, true);
    let before = fs::read(f.spool.join("index")).unwrap();
    let packs: Vec<_> = fs::read_dir(&f.spool)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pack"))
        .map(|p| (p.clone(), fs::read(p).unwrap()))
        .collect();
    let result = attempt(&f, reclaim::Fault::CorruptUnique).await;
    assert!(
        result.is_err(),
        "post-write corruption must fail verification, not grant deletion"
    );
    assert!(f.spool.exists());
    assert_eq!(fs::read(f.spool.join("index")).unwrap(), before);
    for (path, bytes) in packs {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[tokio::test]
async fn crash_boundaries_and_ledger_failure_preserve_source() {
    for fault in [
        reclaim::Fault::AfterReceipt,
        reclaim::Fault::FailLedger,
        reclaim::Fault::AfterIntent,
    ] {
        let f = fixture(true, true);
        let before = fs::read(f.spool.join("index")).unwrap();
        assert!(attempt(&f, fault).await.is_err());
        assert_eq!(fs::read(f.spool.join("index")).unwrap(), before);
        let records: Vec<_> = fs::read_dir(f.repo.join(".git/dracon-rescue"))
            .unwrap()
            .filter_map(|e| {
                let p = e.unwrap().path();
                p.join("receipt").exists().then_some(p)
            })
            .collect();
        assert_eq!(
            records.len(),
            1,
            "durable recovery must survive pre-removal failure"
        );
        assert!(!records[0].join("reclaimed.json").exists());
    }
}
#[tokio::test]
async fn changed_source_after_receipt_is_not_removed() {
    let f = fixture(false, true);
    assert!(attempt(&f, reclaim::Fault::ChangeSource).await.is_err());
    assert_eq!(
        fs::read(f.spool.join("notes")).unwrap(),
        b"changed after receipt"
    );
    assert!(f.spool.join("index").is_file());
}
#[tokio::test]
async fn preserve_marked_spool_is_never_deleted() {
    for marker in ["preserve-rejected", "original-index"] {
        let f = fixture(false, true);
        fs::write(f.spool.join(marker), b"deliberate evidence").unwrap();
        backdate(&f.spool);
        assert!(attempt(&f, reclaim::Fault::None).await.is_err());
        assert!(!reclaim_spool_if_lossless(&f.repo, &f.spool, 30).await);
        assert_eq!(reap_abandoned_spools(&f.repo, 1, 30).await, 0);
        assert_eq!(sweep_epoch_spools(&f.repo, 1, true).await, (0, 0));
        assert_eq!(
            fs::read(f.spool.join(marker)).unwrap(),
            b"deliberate evidence"
        );
    }
}
#[tokio::test]
async fn packless_valid_index_is_recovered_but_invalid_or_unknown_layout_is_preserved() {
    for invalid in [false, true] {
        let f = fixture(false, true);
        for entry in fs::read_dir(&f.spool).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "pack" || e == "idx") {
                fs::remove_file(path).unwrap();
            }
        }
        if invalid {
            fs::write(f.spool.join("index"), b"not an index").unwrap();
        }
        let result = attempt(&f, reclaim::Fault::None).await;
        assert_eq!(result.is_ok(), !invalid);
        assert_eq!(f.spool.exists(), invalid);
    }
    let f = fixture(false, true);
    fs::create_dir(f.spool.join("unknown-objects")).unwrap();
    assert!(attempt(&f, reclaim::Fault::None).await.is_err());
    assert!(f.spool.exists());
}
#[tokio::test]
async fn unlocked_legacy_spool_is_not_assumed_abandoned() {
    let f = fixture(false, false);
    let lease = SpoolLease::acquire(&f.spool).unwrap();
    drop(lease);
    backdate(&f.spool);
    assert!(attempt(&f, reclaim::Fault::None).await.is_err());
    assert!(!f.spool.join("reclaim-settled.json").exists());
    assert!(f.spool.join("index").is_file());
}
#[tokio::test]
async fn concurrent_reapers_remove_only_once() {
    let f = fixture(false, true);
    let (a, b) = tokio::join!(
        attempt(&f, reclaim::Fault::None),
        attempt(&f, reclaim::Fault::None)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(!f.spool.exists());
}
#[test]
fn child_holds_live_owner_probe() {
    let Some(path) = std::env::var_os("DRACON_RECLAIM_LIVE_PROBE") else {
        return;
    };
    let path = PathBuf::from(path);
    let _owner = if std::env::var_os("DRACON_RECLAIM_PROBE_IS_REAPER").is_some() {
        (
            None,
            Some(ownership::ReclaimOwner::acquire(path.parent().unwrap(), &path).unwrap()),
        )
    } else {
        (Some(SpoolLease::acquire(&path).unwrap()), None)
    };
    let ready = path.parent().unwrap().join("live-ready");
    fs::write(ready, b"held").unwrap();
    let start = std::time::Instant::now();
    while !path.parent().unwrap().join("live-exit").exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(15));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
async fn old_but_live_owner_cannot_be_reclaimed() {
    let f = fixture(false, false);
    let mut child = Child(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::epoch_spool::tests::child_holds_live_owner_probe",
            ])
            .env("DRACON_RECLAIM_LIVE_PROBE", &f.spool)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ready = f.spool.parent().unwrap().join("live-ready");
    let start = std::time::Instant::now();
    while !ready.exists() {
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    backdate(&f.spool);
    assert!(attempt(&f, reclaim::Fault::None).await.is_err());
    assert!(f.spool.join("index").is_file());
    fs::write(f.spool.parent().unwrap().join("live-exit"), b"exit").unwrap();
    assert!(child.0.wait().unwrap().success());
    assert!(
        attempt(&f, reclaim::Fault::None).await.is_err(),
        "exit without a settled marker remains uncertain"
    );
}
#[tokio::test]
async fn another_process_reclaimer_holds_source_through_proof() {
    let f = fixture(false, true);
    let mut child = Child(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::epoch_spool::tests::child_holds_live_owner_probe",
            ])
            .env("DRACON_RECLAIM_LIVE_PROBE", &f.spool)
            .env("DRACON_RECLAIM_PROBE_IS_REAPER", "1")
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ready = f.spool.parent().unwrap().join("live-ready");
    let start = std::time::Instant::now();
    while !ready.exists() {
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(attempt(&f, reclaim::Fault::None).await.is_err());
    assert!(f.spool.join("index").exists());
    fs::write(f.spool.parent().unwrap().join("live-exit"), b"exit").unwrap();
    assert!(child.0.wait().unwrap().success());
    attempt(&f, reclaim::Fault::None).await.unwrap();
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn rescue_storage_failure_preserves_original_bytes() {
    let f = fixture(true, true);
    let index = fs::read(f.spool.join("index")).unwrap();
    fs::write(f.repo.join(".git/dracon-rescue"), b"not a directory").unwrap();
    assert!(attempt(&f, reclaim::Fault::None).await.is_err());
    assert_eq!(fs::read(f.spool.join("index")).unwrap(), index);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn qualification_interlock_preserves_valid_recovery_spool() {
    let f = fixture(false, true);
    assert_eq!(
        reclaim_spool_if_lossless(&f.repo, &f.spool, 30).await,
        LOSSLESS_RECLAIM_QUALIFIED
    );
    assert_eq!(f.spool.exists(), !LOSSLESS_RECLAIM_QUALIFIED);
}
#[tokio::test]
async fn fresh_spool_is_protected_by_the_age_floor() {
    let f = fixture(false, true);
    assert_eq!(reap_abandoned_spools(&f.repo, 3600, 30).await, 0);
    assert!(f.spool.exists());
    assert!(spool_candidates(
        f.spool.parent().unwrap(),
        std::time::Duration::from_secs(3600)
    )
    .is_empty());
}
#[tokio::test]
async fn sweep_apply_still_refuses_age_only_deletion() {
    let f = fixture(false, true);
    backdate(&f.spool);
    assert_eq!(sweep_epoch_spools(&f.repo, 1, true).await, (0, 0));
    assert!(f.spool.exists());
    let (count, bytes) = sweep_epoch_spools(&f.repo, 1, false).await;
    assert_eq!(count, 1);
    assert!(bytes > 0);
}
