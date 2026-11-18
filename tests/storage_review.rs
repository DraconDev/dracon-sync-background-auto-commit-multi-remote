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
    let ciphertext_out = root.join("verified-manifest.age");
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
                "--ciphertext-out",
                ciphertext_out.to_str().unwrap(),
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
    // The transfer CLI intentionally opens only operator-provisioned backends.
    dracon_sync::storage_core::backend::LocalBackend::open(&root.join("archive"), 1024).unwrap();
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
    let (_, decoded) = store.load_prepared_manifest(prepared_metadata_id).unwrap();
    assert_eq!(decoded.enrollments().len(), 1);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        index_before
    );

    use sha2::{Digest, Sha256};
    let manifest_path = ".dracon/assets.manifest";
    std::fs::create_dir(work.join(".dracon")).unwrap();
    std::fs::copy(&ciphertext_out, work.join(manifest_path)).unwrap();
    let attributes = b"art.bin filter=dracon-storage -text\n";
    std::fs::write(work.join(".gitattributes"), attributes).unwrap();
    let digest_attrs = format!("{:x}", Sha256::digest(attributes));
    let tx_root = root.join("transactions");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&tx_root)
        .unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_dracon-sync"))
        .canonicalize()
        .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let driver = [
        executable.to_str().unwrap(),
        "storage",
        "filter-clean",
        "--repo",
        work.to_str().unwrap(),
        "--repo-id",
        &id,
        "--journal-root",
        journal.to_str().unwrap(),
        "--metadata-root",
        metadata.to_str().unwrap(),
        "--manifest-path",
        manifest_path,
    ]
    .into_iter()
    .map(quote)
    .collect::<Vec<_>>()
    .join(" ")
        + " -- %f";
    let mut config = repo
        .config()
        .unwrap()
        .open_level(git2::ConfigLevel::Local)
        .unwrap();
    config
        .set_str("filter.dracon-storage.clean", &driver)
        .unwrap();
    config
        .set_bool("filter.dracon-storage.required", true)
        .unwrap();
    let result = sync(
        &[
            "storage",
            "stage-enrollment",
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
            "--metadata-id",
            prepared_metadata_id,
            "--manifest-path",
            manifest_path,
            "--transaction-root",
            tx_root.to_str().unwrap(),
            "--attributes-sha256",
            &digest_attrs,
            "--job-id",
            job_id,
        ],
        &[],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read(work.join("art.bin")).unwrap(),
        b"working latest"
    );
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

    use std::io::Write;
    use std::process::Stdio;
    let clean = |input: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dracon-sync"))
            .args(["storage", "filter-clean"])
            .args(cold_args)
            .args([
                "--restore-root",
                restore_root.to_str().unwrap(),
                "--",
                "art.bin",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };
    let unchanged = clean(b"working latest");
    assert!(
        unchanged.status.success(),
        "{}",
        String::from_utf8_lossy(&unchanged.stderr)
    );
    assert_eq!(unchanged.stdout, pointer);
    let edited = clean(b"working edited");
    assert!(!edited.status.success());
    assert!(edited.stdout.is_empty());
    std::fs::write(restored, b"corrupt cache!").unwrap();
    let corrupt = clean(b"working latest");
    assert!(!corrupt.status.success());
    assert!(corrupt.stdout.is_empty());
}
