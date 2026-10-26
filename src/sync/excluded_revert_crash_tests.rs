use super::*;
use std::os::unix::fs::OpenOptionsExt;
use std::process::Stdio;

#[tokio::test]
async fn excluded_recovery_parent_crash_kills_actual_lease_holder_without_late_restores() {
    const ROLE: &str = "DRACON_EXCLUDED_PARENT_CRASH_HOLDER";
    if let Some(request) = std::env::var_os(ROLE) {
        // A real disposable parent, not a marker claiming that the grandchild
        // stopped. The outer witness independently observes kernel leases and
        // process state, then SIGKILLs THIS parent while the helper is working.
        let mut helper =
            tokio::process::Command::new(std::env::var_os("DRACON_EPOCH_TEST_HELPER").unwrap());
        let mut child = helper
            .args(["--dracon-excluded-restore-helper"])
            .arg(request)
            .arg(std::process::id().to_string())
            .env("TOKIO_WORKER_THREADS", "2")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        std::fs::write(
            std::env::var_os("DRACON_EXCLUDED_CHILD_PID_FILE").unwrap(),
            child.id().unwrap().to_string(),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_secs(45)).await;
        let _ = child.kill().await;
        panic!("outer parent-crash witness failed to terminate its owned holder");
    }
    let (d, pending, mut targets) = super::recovery_tests::fixture().await;
    let r = d.path();
    let large = OpenOptions::new()
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
    let request = super::recovery_tests::request(r, &pending, targets);
    let meta = std::fs::metadata(r.join("one.log")).unwrap();
    let inode = format!(
        "{:02x}:{:02x}:{}",
        libc::major(meta.dev()),
        libc::minor(meta.dev()),
        meta.ino()
    );
    let pid_file = pending.join("child-pid");
    let mut holder = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact","sync::excluded_revert::crash_tests::excluded_recovery_parent_crash_kills_actual_lease_holder_without_late_restores","--nocapture"])
        .env(ROLE,&request).env("DRACON_EXCLUDED_CHILD_PID_FILE",&pid_file)
        .env("TOKIO_WORKER_THREADS","2").stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true)
        .spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let helper_pid = loop {
        assert!(
            Instant::now() < deadline,
            "no actual helper lease observed before parent crash"
        );
        assert!(
            holder.try_wait().unwrap().is_none(),
            "owned helper parent exited before witness"
        );
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
        {
            let leased = std::fs::read_to_string("/proc/locks")
                .unwrap()
                .lines()
                .any(|line| {
                    let fields: Vec<_> = line.split_whitespace().collect();
                    fields.len() >= 6
                        && fields[1..4] == ["LEASE", "ACTIVE", "READ"]
                        && fields[4] == pid.to_string()
                        && fields[5] == inode
                });
            if leased {
                break pid;
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    let stat = std::fs::read_to_string(format!("/proc/{helper_pid}/stat")).unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    assert_eq!(
        fields[1].parse::<u32>().unwrap(),
        holder.id().unwrap(),
        "actual helper parent identity"
    );
    let start_time = fields[19].to_owned();
    holder.kill().await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let stopped = match std::fs::read_to_string(format!("/proc/{helper_pid}/stat")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => panic!("cannot inspect owned helper death: {e}"),
            Ok(stat) => {
                let fields: Vec<_> = stat
                    .rsplit_once(") ")
                    .unwrap()
                    .1
                    .split_whitespace()
                    .collect();
                fields[19] != start_time || fields[0] == "Z"
            }
        };
        if stopped {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "helper outlived its killed parent"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The crash may land on either side of exchange; do not manufacture a
    // before-exchange assertion from a lease observation alone. Both versions
    // must remain classifiable, and no late restore may replace this inode.
    let after_crash = std::fs::metadata(r.join("one.log")).unwrap();
    // Process state and kernel lease teardown are separate observations. Keep
    // the SAME three-second budget from parent death; no reset, grace sleep or
    // unbounded retry can conceal a surviving lease. Successful nonblocking
    // open is the actual release witness, not the zombie/PID marker above.
    let mut writer = loop {
        match OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(r.join("one.log"))
        {
            Ok(writer) => break writer,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline,
                    "parent death did not release the actual kernel lease within its original bound: {error}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => panic!("cannot observe lease release: {error}"),
        }
    };
    use std::io::{Read, Write};
    writer.write_all(b"LATER").unwrap();
    writer.sync_all().unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut prefix = [0; 5];
    File::open(r.join("one.log"))
        .unwrap()
        .read_exact(&mut prefix)
        .unwrap();
    assert_eq!(&prefix, b"LATER");
    assert_eq!(
        std::fs::metadata(r.join("one.log")).unwrap().ino(),
        after_crash.ino()
    );
    assert!(super::super::has_pending_excluded_recovery(r).unwrap());
    assert!(settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap());
    assert!(!super::super::has_pending_excluded_recovery(r).unwrap());
}

#[tokio::test]
async fn excluded_recovery_legacy_permission_repair_never_follows_archive_symlinks() {
    let (d, pending, targets) = super::recovery_tests::fixture().await;
    let r = d.path();
    super::recovery_tests::request(r, &pending, targets);
    assert!(settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap());
    let outside = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o755))
        .tempdir()
        .unwrap();
    std::fs::write(
        outside.path().join("canary.txt"),
        b"unrelated owned content",
    )
    .unwrap();
    let before = std::fs::metadata(outside.path()).unwrap().mode();
    std::os::unix::fs::symlink(
        outside.path(),
        r.join(".git/dracon-excluded-archive/dracon-excluded-recovery-symlink"),
    )
    .unwrap();
    assert!(super::super::has_pending_excluded_recovery(r).is_err());
    assert!(settle_pending(r, Instant::now() + Duration::from_secs(5))
        .await
        .is_err());
    assert_eq!(std::fs::metadata(outside.path()).unwrap().mode(), before);
    assert_eq!(
        std::fs::read(outside.path().join("canary.txt")).unwrap(),
        b"unrelated owned content"
    );
}
