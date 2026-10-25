use super::*;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let out = crate::git::git_cmd()
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.name", "test"]);
    git(d.path(), &["config", "user.email", "test@test"]);
    let empty = d.path().join(".git/initial-empty-hooks");
    std::fs::create_dir(&empty).unwrap();
    git(
        d.path(),
        &["config", "core.hooksPath", empty.to_str().unwrap()],
    );
    std::fs::write(d.path().join("excluded.log"), b"head\0binary\xff\n").unwrap();
    std::fs::write(d.path().join("normal.txt"), b"one\n").unwrap();
    git(d.path(), &["add", "--", "excluded.log", "normal.txt"]);
    git(d.path(), &["commit", "-q", "-m", "initial"]);
    d
}
fn paths() -> Vec<dracon_git::types::DiffFile> {
    vec![dracon_git::types::DiffFile::new(
        PathBuf::from("excluded.log"),
        dracon_git::types::FileStatus::Modified,
    )]
}
async fn restore(repo: &Path) -> Result<bool> {
    super::excluded_revert::restore(
        repo,
        &paths(),
        tokio::time::Instant::now() + Duration::from_secs(15),
    )
    .await
}
fn recovery_contains(repo: &Path, bytes: &[u8]) -> bool {
    std::fs::read_dir(repo.join(".git"))
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("dracon-excluded-recovery-")
                && std::fs::read(e.path().join("files/excluded.log")).is_ok_and(|b| b == bytes)
        })
}
#[tokio::test]
async fn excluded_restore_keeps_binary_recovery_and_does_not_touch_index_or_untracked() {
    let d = fixture();
    let repo = d.path();
    std::fs::write(repo.join("excluded.log"), b"operator\0bytes\xfe").unwrap();
    std::fs::write(repo.join("unknown.log"), b"untracked user work").unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let mut entries = paths();
    entries.push(dracon_git::types::DiffFile::new(
        PathBuf::from("unknown.log"),
        dracon_git::types::FileStatus::Added,
    ));
    assert!(super::excluded_revert::restore(
        repo,
        &entries,
        tokio::time::Instant::now() + Duration::from_secs(15)
    )
    .await
    .unwrap());
    assert_eq!(
        std::fs::read(repo.join("excluded.log")).unwrap(),
        b"head\0binary\xff\n"
    );
    assert_eq!(
        std::fs::read(repo.join("unknown.log")).unwrap(),
        b"untracked user work"
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(recovery_contains(repo, b"operator\0bytes\xfe"));
    std::fs::remove_file(repo.join("excluded.log")).unwrap();
    assert!(
        restore(repo).await.unwrap(),
        "a known deletion is restored without overwriting a newly created file"
    );
    assert_eq!(
        std::fs::read(repo.join("excluded.log")).unwrap(),
        b"head\0binary\xff\n"
    );
}
#[tokio::test]
async fn excluded_restore_defers_active_writer_without_relocating_its_future_bytes() {
    let d = fixture();
    let repo = d.path();
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(repo.join("excluded.log"))
        .unwrap();
    assert!(
        !restore(repo).await.unwrap(),
        "kernel writer lease must refuse a live writable FD"
    );
    writer.seek(SeekFrom::Start(0)).unwrap();
    writer.write_all(b"LATER").unwrap();
    writer.sync_all().unwrap();
    assert!(
        std::fs::read(repo.join("excluded.log"))
            .unwrap()
            .starts_with(b"LATER"),
        "the producer's FD must still address the actual working file, not a hidden backup"
    );
    drop(writer);
    let later = std::fs::read(repo.join("excluded.log")).unwrap();
    assert!(restore(repo).await.unwrap());
    assert!(recovery_contains(repo, &later));
}
#[tokio::test]
async fn excluded_restore_foreign_lock_is_not_removed_or_used_as_hygiene_ownership() {
    let d = fixture(); let repo = d.path();
    std::fs::write(repo.join("excluded.log"), b"operator bytes").unwrap();
    std::fs::write(repo.join(".git/index.lock"), b"foreign owner").unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    assert!(!restore(repo).await.unwrap());
    assert_eq!(std::fs::read(repo.join(".git/index.lock")).unwrap(), b"foreign owner");
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join("excluded.log")).unwrap(), b"operator bytes");
}
#[tokio::test]
async fn excluded_restore_deadline_kills_required_filter_descendants_without_mutation() {
    let d = fixture(); let repo = d.path();
    std::fs::write(repo.join("excluded.log"), b"operator bytes").unwrap();
    std::fs::write(repo.join(".gitattributes"), b"excluded.log filter=restoreprobe\n").unwrap();
    git(repo, &["config", "filter.restoreprobe.required", "true"]);
    git(repo, &["config", "filter.restoreprobe.clean", "cat"]);
    let script = repo.join(".git/hanging-smudge");
    let pidfile = repo.join(".git/owned-filter-pid");
    std::fs::write(&script, format!("#!/bin/sh\nsleep 60 &\necho $! > '{}'\nwait\n", pidfile.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(repo, &["config", "filter.restoreprobe.smudge", script.to_str().unwrap()]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let start = tokio::time::Instant::now();
    let result = super::excluded_revert::restore(repo, &paths(), start+Duration::from_secs(3)).await;
    assert!(result.is_err(), "required filter cannot outlive the absolute deadline");
    assert!(start.elapsed()<Duration::from_secs(8));
    let pid: u32 = std::fs::read_to_string(&pidfile).expect("hanging filter actually started").trim().parse().unwrap();
    for _ in 0..40 {
        let live = std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s|
            !s.rsplit_once(')').unwrap().1.trim_start().starts_with('Z'));
        if !live { break; }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(!std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s|
        !s.rsplit_once(')').unwrap().1.trim_start().starts_with('Z')), "owned descendant still running");
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join("excluded.log")).unwrap(), b"operator bytes");
    assert!(!repo.join(".git/index.lock").exists());
}
#[tokio::test]
async fn excluded_restore_preserves_foreign_staged_intent_exactly() {
    let d = fixture();
    let repo = d.path();
    std::fs::write(repo.join("excluded.log"), b"keep local").unwrap();
    std::fs::write(repo.join("normal.txt"), b"foreign staged bytes").unwrap();
    git(repo, &["add", "--", "normal.txt"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    assert!(!restore(repo).await.unwrap());
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(git(repo, &["show", ":normal.txt"]), b"foreign staged bytes");
    assert_eq!(
        std::fs::read(repo.join("excluded.log")).unwrap(),
        b"keep local"
    );
}
#[tokio::test]
async fn excluded_restore_required_smudge_refusal_and_later_writer_are_not_overwritten() {
    let d = fixture();
    let repo = d.path();
    std::fs::write(repo.join("excluded.log"), b"before filter").unwrap();
    std::fs::write(
        repo.join(".gitattributes"),
        b"excluded.log filter=restoreprobe\n",
    )
    .unwrap();
    git(repo, &["config", "filter.restoreprobe.required", "true"]);
    git(repo, &["config", "filter.restoreprobe.clean", "cat"]);
    git(repo, &["config", "filter.restoreprobe.smudge", "false"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    assert!(
        restore(repo).await.is_err(),
        "meaningful required-smudge failure is not healthy hygiene"
    );
    assert_eq!(
        std::fs::read(repo.join("excluded.log")).unwrap(),
        b"before filter"
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    let script = repo.join(".git/smudge-producer");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat\nprintf 'later producer' > '{}'\n",
            repo.join("excluded.log").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        repo,
        &[
            "config",
            "filter.restoreprobe.smudge",
            script.to_str().unwrap(),
        ],
    );
    assert!(
        !restore(repo).await.unwrap(),
        "a filter's later working bytes were not captured ownership"
    );
    assert_eq!(
        std::fs::read(repo.join("excluded.log")).unwrap(),
        b"later producer"
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
}
