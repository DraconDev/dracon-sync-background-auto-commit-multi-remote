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

    // Restore the reviewed working bytes to qualify the next boundary. The
    // test adapter exercises the Warden subprocess protocol with fixture bytes.
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::write(work.join("art.bin"), b"working latest").unwrap();
    let warden = root.join("warden-fixture");
    std::fs::write(
        &warden,
        "#!/bin/sh\ncase \"$1\" in\nstorage-encrypt) printf 'age-encryption.org/v1\\n'; cat ;;\nstorage-decrypt) tail -n +2 ;;\n*) exit 99 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&warden, std::fs::Permissions::from_mode(0o700)).unwrap();
    let metadata = root.join("metadata");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&metadata)
        .unwrap();
    let job_id = result["job_id"].as_str().unwrap();
    let manifest = || {
        sync(
            &[
                "storage",
                "prepare-enrollment-manifest",
                "--repo",
                work.to_str().unwrap(),
                "--repo-id",
                &id,
                "--review-manifest",
                review,
                "--review-sha256",
                digest,
                "--journal-root",
                journal.to_str().unwrap(),
                "--metadata-root",
                metadata.to_str().unwrap(),
                "--metadata-policy-sha256",
                &bad_digest,
                "--warden",
                warden.to_str().unwrap(),
                "--job-id",
                job_id,
            ],
            &[],
        )
    };
    let early = manifest();
    assert!(!early.status.success());
    assert!(String::from_utf8_lossy(&early.stderr).contains("verified copies"));
    assert!(!metadata.join(&id).exists());
    let advanced = sync(
        &[
            "storage",
            "advance-job",
            "--repo",
            work.to_str().unwrap(),
            "--repo-id",
            &id,
            "--journal-root",
            journal.to_str().unwrap(),
            "--job-id",
            job_id,
            "--policy",
            policy.to_str().unwrap(),
        ],
        &[],
    );
    assert!(
        advanced.status.success(),
        "{}",
        String::from_utf8_lossy(&advanced.stderr)
    );
    let prepared = manifest();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let manifest_receipt: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(manifest_receipt["verified_paths"], 1);
    assert_eq!(manifest_receipt["git_changed"], false);
    let prepared_metadata_id = manifest_receipt["metadata_id"].as_str().unwrap();
    let store =
        dracon_sync::storage_core::metadata::MetadataStore::open(&metadata, &id, Limits::default())
            .unwrap();
    let (protected, decoded) = store.load_prepared_manifest(prepared_metadata_id).unwrap();
    assert_eq!(decoded.enrollments().len(), 1);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        index_before
    );

    use dracon_sync::storage_core::{
        enrollment_review::{approve_review_digest, ReviewLimits},
        index::IndexTransaction,
        staging::StageBundle,
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let approval = runtime
        .block_on(approve_review_digest(
            &work,
            &id,
            Path::new(review),
            digest,
            ReviewLimits {
                max_blob_bytes: 1024,
                max_total_bytes: 4096,
                max_retained_bytes: 128 * 1024,
                timeout: std::time::Duration::from_secs(30),
            },
        ))
        .unwrap();
    let lease = jobs.lease(job_id).unwrap();
    let bundle = StageBundle::build(&store, &protected, &decoded, vec![&lease]).unwrap();
    let manifest_path = ".dracon/assets.manifest";
    let tx = IndexTransaction::open(
        &work,
        &id,
        &root.join("transactions"),
        manifest_path.as_bytes(),
    )
    .unwrap();
    tx.stage_reviewed(&tx.snapshot().unwrap(), &bundle, &approval)
        .unwrap();
    // Commit inside the disposable fixture, then recover without producer state.
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "fixture enrollment",
        &tree,
        &[&parent],
    )
    .unwrap();
    let cold = root.join("cold");
    let clone = git2::Repository::clone(work.to_str().unwrap(), &cold).unwrap();
    clone
        .config()
        .unwrap()
        .set_str("dracon.storageRepoId", &id)
        .unwrap();
    let pointer = std::fs::read(cold.join("art.bin")).unwrap();
    assert!(dracon_sync::storage_core::reference::Pointer::parse(&pointer).is_ok());
    let cold_metadata = root.join("cold-metadata");
    let cold_args = [
        "--repo",
        cold.to_str().unwrap(),
        "--repo-id",
        &id,
        "--metadata-root",
        cold_metadata.to_str().unwrap(),
        "--manifest-path",
        manifest_path,
    ];
    let mut args = vec!["storage", "import-manifest"];
    args.extend(cold_args);
    args.extend([
        "--policy-sha256",
        &bad_digest,
        "--warden",
        warden.to_str().unwrap(),
    ]);
    let imported = sync(&args, &[]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let restore_root = root.join("cold-restore");
    let mut args = vec!["storage", "restore-asset"];
    args.extend(cold_args);
    args.extend([
        "--path",
        "art.bin",
        "--policy",
        policy.to_str().unwrap(),
        "--restore-root",
        restore_root.to_str().unwrap(),
    ]);
    let recovered = sync(&args, &[]);
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let output = String::from_utf8(recovered.stdout).unwrap();
    let restored = output
        .lines()
        .find_map(|line| line.strip_prefix("Verified recovery: 14 bytes at "))
        .unwrap();
    assert_eq!(std::fs::read(restored).unwrap(), b"working latest");
    assert_eq!(std::fs::read(cold.join("art.bin")).unwrap(), pointer);
}
