//! Exercise the operator review/capture boundary through the real CLI.
#![cfg(unix)]

use dracon_sync::storage_core::journal::{Journal, Limits, Phase};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sync(arguments: &[&str], paths: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dracon-sync"))
        .args(arguments)
        .args(paths)
        .output()
        .unwrap()
}

#[test]
fn reviewed_capture_preserves_all_versions_and_refuses_unreviewed_or_changed_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let work = root.join("repo");
    let repo = git2::Repository::init(&work).unwrap();
    let id = "a".repeat(64);
    repo.config()
        .unwrap()
        .set_str("dracon.storageRepoId", &id)
        .unwrap();
    std::fs::write(work.join("art.bin"), b"committed original").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("art.bin")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Enrollment Fixture", "fixture@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "baseline", &tree, &[])
        .unwrap();
    std::fs::write(work.join("art.bin"), b"staged intermediate").unwrap();
    index.add_path(Path::new("art.bin")).unwrap();
    index.write().unwrap();
    std::fs::write(work.join("art.bin"), b"working latest").unwrap();
    let index_before = std::fs::read(repo.path().join("index")).unwrap();
    let policy = root.join("policy.toml");
    std::fs::write(&policy, format!(
        "[storage]\nenabled = true\n[storage.backends.archive]\ntype = \"local\"\nroot = {:?}\nallowed_security = [\"non-sensitive\"]\n[[storage.rules]]\npaths = [\"*.bin\"]\nplacement = \"external\"\nbackend = \"archive\"\nsecurity = \"non-sensitive\"\n", root.join("archive").to_str().unwrap()
    )).unwrap();
    let output = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let git = PathBuf::from(std::str::from_utf8(&output.stdout).unwrap().trim())
        .canonicalize()
        .unwrap();
    let custody = root.join("custody");
    let journal = root.join("journal");
    let prepare = || {
        sync(
            &[
                "storage",
                "prepare-enrollment-review",
                "--repo",
                work.to_str().unwrap(),
                "--repo-id",
                &id,
                "--custody-root",
                custody.to_str().unwrap(),
                "--git",
                git.to_str().unwrap(),
                "--json",
                "--",
                "art.bin",
            ],
            &[],
        )
    };
    // A held native operation lease prevents review preparation without waiting.
    let owner = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(repo.path().join("dracon-sync.operation-lease"))
        .unwrap();
    owner.try_lock().unwrap();
    let busy = prepare();
    assert!(!busy.status.success());
    assert!(String::from_utf8_lossy(&busy.stderr).contains("repository is busy"));
    assert!(!custody.exists());
    drop(owner);
    let prepared = prepare();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(receipt["preserved_versions"], 3);
    let review = receipt["manifest_path"].as_str().unwrap();
    let digest = receipt["review_sha256"].as_str().unwrap();
    let capture = |review_args: &[&str], path: &str| {
        let mut args = vec![
            "storage",
            "capture",
            "--repo",
            work.to_str().unwrap(),
            "--repo-id",
            &id,
            "--journal-root",
            journal.to_str().unwrap(),
            "--policy",
            policy.to_str().unwrap(),
            "--git-target",
            "github",
            "--json",
        ];
        args.extend_from_slice(review_args);
        args.extend_from_slice(&["--", path]);
        sync(&args, &[])
    };
    assert!(!capture(&[], "art.bin").status.success());
    assert!(!capture(&["--review-manifest", review], "art.bin")
        .status
        .success());
    let bad_digest = "b".repeat(64);
    assert!(!capture(
        &["--review-manifest", review, "--review-sha256", &bad_digest],
        "art.bin"
    )
    .status
    .success());
    assert!(!journal.exists());
    std::fs::write(work.join("other.bin"), b"working latest").unwrap();
    let approved = ["--review-manifest", review, "--review-sha256", digest];
    assert!(!capture(&approved, "other.bin").status.success());
    assert!(!journal.exists());
    let captured = capture(&approved, "art.bin");
    assert!(
        captured.status.success(),
        "{}",
        String::from_utf8_lossy(&captured.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&captured.stdout).unwrap();
    let jobs = Journal::open(&journal, &id, Limits::default()).unwrap();
    let lease = jobs.lease(result["job_id"].as_str().unwrap()).unwrap();
    assert_eq!(lease.load().unwrap().phase(), Phase::Captured);
    drop(lease);
    std::fs::write(work.join("art.bin"), b"new operator edit").unwrap();
    assert!(!capture(&approved, "art.bin").status.success());
    assert_eq!(
        std::fs::read(work.join("art.bin")).unwrap(),
        b"new operator edit"
    );
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        index_before
    );
    assert_eq!(Journal::inspect(&journal, &id).unwrap().records, 1);
    assert!(!root.join("archive").exists(), "capture must not upload");
}
