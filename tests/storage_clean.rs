//! Real Git required-filter checks using isolated local storage and synthetic metadata.
#![cfg(unix)]

use dracon_sync::storage_core::{
    backend::LocalBackend,
    bindings::{ApprovedBackend, CopyBindings},
    journal::{encode_relative_path, Encryption, JobSpec, Journal, Limits, Phase},
    manifest::{Enrollment, Manifest},
    metadata::MetadataStore,
    reference::{Fingerprint, Pointer},
    security::WardenAdapter,
    transfer::transfer_copies,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
    journal: Journal,
    job: String,
    pointer: Pointer,
    args: Vec<String>,
}

fn git(repo: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        cmd.env_remove(name);
    }
    cmd.current_dir(repo).args(args).output().unwrap()
}

fn quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\\''"))
}

async fn fixture() -> Fixture {
    fixture_path(b"asset [version].bin").await
}

async fn fixture_path(asset_path: &[u8]) -> Fixture {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    assert!(git(&repo, &["init", "--quiet"]).status.success());
    assert!(git(
        &repo,
        &["config", "--local", "dracon.storageRepoId", &"a".repeat(64)]
    )
    .status
    .success());
    let bytes = b"approved private source content for the isolated clean fixture";
    std::fs::write(
        repo.join(std::ffi::OsString::from_vec(asset_path.to_vec())),
        bytes,
    )
    .unwrap();
    let fingerprint =
        Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
    let spec = JobSpec {
        repo_id: "a".repeat(64),
        path_hex: encode_relative_path(asset_path).unwrap(),
        source: fingerprint.clone(),
        policy_sha256: "b".repeat(64),
        primary: "primary".into(),
        required_copies: vec!["primary".into(), "recovery".into()],
        required_git_targets: vec!["github".into()],
        encryption: Encryption::None,
    };
    let journal_root = root.join("journal");
    let journal = Journal::open(&journal_root, &spec.repo_id, Limits::default()).unwrap();
    let job = journal.create(spec.clone()).unwrap();
    {
        let lease = journal.lease(job.id()).unwrap();
        lease.capture_snapshot(&mut &bytes[..]).unwrap();
        lease.retain_payload(&mut &bytes[..], &fingerprint).unwrap();
        let primary = LocalBackend::open(&root.join("primary"), 1024).unwrap();
        let recovery = LocalBackend::open(&root.join("recovery"), 1024).unwrap();
        let grants = CopyBindings::new(
            spec.repo_id.clone(),
            BTreeMap::from([
                (
                    "primary".into(),
                    ApprovedBackend::for_security(&primary, vec![Encryption::None]).unwrap(),
                ),
                (
                    "recovery".into(),
                    ApprovedBackend::for_security(&recovery, vec![Encryption::None]).unwrap(),
                ),
            ]),
        )
        .unwrap();
        transfer_copies(&lease, &grants, 1).unwrap();
    }
    let manifest = Manifest::new(
        spec.repo_id.clone(),
        vec![Enrollment {
            path_hex: spec.path_hex,
            contract_sha256: spec.policy_sha256,
            primary: spec.primary,
            required_copies: spec.required_copies,
            encryption: spec.encryption,
            payload: Some(fingerprint.clone()),
        }],
    )
    .unwrap();
    let binary = root.join("warden-fixture");
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf 'age-encryption.org/v1\\n'\ncat\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let adapter =
        WardenAdapter::new(&binary, &repo, manifest.repo_id(), Duration::from_secs(5)).unwrap();
    let metadata_root = root.join("metadata");
    let store = MetadataStore::open(&metadata_root, manifest.repo_id(), Limits::default()).unwrap();
    let prepared = store
        .prepare(&manifest, &"b".repeat(64), &adapter, 1)
        .await
        .unwrap();
    // Prepare the matched metadata entry before the required clean driver can
    // publish any asset pointer. This is test-only index setup, not auto enrollment.
    let mut ciphertext = Vec::new();
    use std::io::Read;
    store
        .open_prepared(&prepared)
        .unwrap()
        .read_to_end(&mut ciphertext)
        .unwrap();
    std::fs::create_dir(repo.join(".dracon")).unwrap();
    std::fs::write(repo.join(".dracon/assets.manifest"), ciphertext).unwrap();
    assert!(git(&repo, &["add", "--", ".dracon/assets.manifest"])
        .status
        .success());
    let args = vec![
        "storage".into(),
        "filter-clean".into(),
        "--repo".into(),
        repo.to_str().unwrap().into(),
        "--repo-id".into(),
        spec.repo_id,
        "--journal-root".into(),
        journal_root.to_str().unwrap().into(),
        "--metadata-root".into(),
        metadata_root.to_str().unwrap().into(),
        "--manifest-path".into(),
        ".dracon/assets.manifest".into(),
    ];
    let driver = std::iter::once(env!("CARGO_BIN_EXE_dracon-sync").to_owned())
        .chain(args.iter().cloned())
        .map(|argument| quote(&argument))
        .collect::<Vec<_>>()
        .join(" ")
        + " -- %f";
    assert!(git(
        &repo,
        &["config", "--local", "filter.dracon-storage.clean", &driver]
    )
    .status
    .success());
    assert!(git(
        &repo,
        &[
            "config",
            "--local",
            "filter.dracon-storage.required",
            "true"
        ]
    )
    .status
    .success());
    std::fs::write(
        repo.join(".gitattributes"),
        "*.bin filter=dracon-storage -text -ident\n",
    )
    .unwrap();
    assert!(git(&repo, &["add", "--", ".gitattributes"])
        .status
        .success());
    Fixture {
        temp,
        repo,
        journal,
        job: job.id().into(),
        pointer: Pointer::new(fingerprint).unwrap(),
        args,
    }
}

#[tokio::test]
async fn real_git_add_uses_only_exact_prepared_reference_and_preserves_edits() {
    let f = fixture().await;
    let source = std::fs::read(f.repo.join("asset [version].bin")).unwrap();
    let result = git(&f.repo, &["add", "--", "asset [version].bin"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let staged = git(&f.repo, &["show", ":asset [version].bin"]);
    assert_eq!(staged.stdout, f.pointer.encode());
    assert_eq!(
        std::fs::read(f.repo.join("asset [version].bin")).unwrap(),
        source
    );
    let mut changed = source.clone();
    changed[0] ^= 1;
    std::fs::write(f.repo.join("asset [version].bin"), &changed).unwrap();
    let index = std::fs::read(f.repo.join(".git/index")).unwrap();
    let rejected = git(&f.repo, &["add", "--", "asset [version].bin"]);
    assert!(!rejected.status.success());
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(f.repo.join("asset [version].bin")).unwrap(),
        changed
    );
    assert!(!String::from_utf8_lossy(&rejected.stderr)
        .contains(&String::from_utf8_lossy(&source).to_string()));
    std::fs::write(f.repo.join("asset [version].bin"), f.pointer.encode()).unwrap();
    assert!(git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(
        f.journal.lease(&f.job).unwrap().load().unwrap().phase(),
        Phase::ReadyToStage
    );
}

#[tokio::test]
async fn wrong_path_failed_job_and_foreign_repository_cannot_fall_back_to_raw() {
    let f = fixture().await;
    std::fs::write(f.repo.join("other.bin"), b"unprepared ordinary content").unwrap();
    assert!(!git(&f.repo, &["add", "--", "other.bin"]).status.success());
    assert!(git(&f.repo, &["ls-files", "--", "other.bin"])
        .stdout
        .is_empty());
    assert!(git(
        &f.repo,
        &["config", "--local", "dracon.storageRepoId", &"c".repeat(64)]
    )
    .status
    .success());
    assert!(!git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert!(git(
        &f.repo,
        &["config", "--local", "dracon.storageRepoId", &"a".repeat(64)]
    )
    .status
    .success());
    let lease = f.journal.lease(&f.job).unwrap();
    let mut job = lease.load().unwrap();
    job.note_failure(
        dracon_sync::storage_core::journal::FailureCode::Integrity,
        None,
    )
    .unwrap();
    lease.save(&mut job).unwrap();
    drop(lease);
    assert!(!git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    let output = Command::new(env!("CARGO_BIN_EXE_dracon-sync"))
        .args(&f.args)
        .args(["--", "asset [version].bin"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(f.temp.path().join("journal").exists());
}

#[tokio::test]
async fn missing_metadata_and_alternate_index_cannot_stage_an_unrestorable_pointer() {
    let f = fixture().await;
    let index = std::fs::read(f.repo.join(".git/index")).unwrap();
    let alternate = f.temp.path().join("alternate-index");
    let empty = Command::new("git")
        .current_dir(&f.repo)
        .env("GIT_INDEX_FILE", &alternate)
        .args(["read-tree", "--empty"])
        .output()
        .unwrap();
    assert!(empty.status.success());
    let before = std::fs::read(&alternate).unwrap();
    let rejected = Command::new("git")
        .current_dir(&f.repo)
        .env("GIT_INDEX_FILE", &alternate)
        .args(["add", "--", "asset [version].bin"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(std::fs::read(&alternate).unwrap(), before);
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), index);
    std::fs::copy(f.repo.join(".git/index"), &alternate).unwrap();
    let accepted = Command::new("git")
        .current_dir(&f.repo)
        .env("GIT_INDEX_FILE", &alternate)
        .args(["add", "--", "asset [version].bin"])
        .output()
        .unwrap();
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), index);
    let metadata = std::fs::read(f.repo.join(".dracon/assets.manifest")).unwrap();
    std::fs::write(
        f.repo.join(".dracon/assets.manifest"),
        b"age-encryption.org/v1\nwrong prepared metadata",
    )
    .unwrap();
    assert!(git(&f.repo, &["add", "--", ".dracon/assets.manifest"])
        .status
        .success());
    let wrong_index = std::fs::read(f.repo.join(".git/index")).unwrap();
    assert!(!git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(
        std::fs::read(f.repo.join(".git/index")).unwrap(),
        wrong_index
    );
    std::fs::write(f.repo.join(".dracon/assets.manifest"), metadata).unwrap();
    assert!(git(&f.repo, &["add", "--", ".dracon/assets.manifest"])
        .status
        .success());
    assert!(git(
        &f.repo,
        &["rm", "--cached", "--", ".dracon/assets.manifest"]
    )
    .status
    .success());
    let before = std::fs::read(f.repo.join(".git/index")).unwrap();
    assert!(!git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), before);
    assert!(f.repo.join("asset [version].bin").exists());
}

#[tokio::test]
async fn real_git_clean_preserves_non_utf8_path_identity() {
    use std::os::unix::ffi::OsStringExt;
    let raw = b"asset [raw] \xff.bin";
    let f = fixture_path(raw).await;
    let path = PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec()));
    let source = std::fs::read(f.repo.join(&path)).unwrap();
    let result = Command::new("git")
        .current_dir(&f.repo)
        .args(["add", "--"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let repo = git2::Repository::open(&f.repo).unwrap();
    let index = repo.index().unwrap();
    let entry = index.get_path(&path, 0).unwrap();
    assert_eq!(
        repo.find_blob(entry.id).unwrap().content(),
        f.pointer.encode()
    );
    assert_eq!(std::fs::read(f.repo.join(&path)).unwrap(), source);
}

fn enrolled(job: &dracon_sync::storage_core::journal::Job) -> Enrollment {
    let spec = job.spec();
    let mut copies = spec.required_copies.clone();
    copies.sort();
    Enrollment {
        path_hex: spec.path_hex.clone(),
        contract_sha256: spec.policy_sha256.clone(),
        primary: spec.primary.clone(),
        required_copies: copies,
        encryption: spec.encryption,
        payload: job.payload().cloned(),
    }
}

fn record_version(f: &Fixture, path: &[u8], bytes: &[u8]) -> Enrollment {
    use std::os::unix::ffi::OsStringExt;
    std::fs::write(
        f.repo.join(std::ffi::OsString::from_vec(path.to_vec())),
        bytes,
    )
    .unwrap();
    let fp = Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
    let spec = JobSpec {
        repo_id: "a".repeat(64),
        path_hex: encode_relative_path(path).unwrap(),
        source: fp.clone(),
        policy_sha256: "b".repeat(64),
        primary: "primary".into(),
        required_copies: vec!["primary".into(), "recovery".into()],
        required_git_targets: vec!["github".into()],
        encryption: Encryption::None,
    };
    let job = f.journal.create(spec).unwrap();
    let lease = f.journal.lease(job.id()).unwrap();
    lease.capture_snapshot(&mut &bytes[..]).unwrap();
    lease.retain_payload(&mut &bytes[..], &fp).unwrap();
    let primary = LocalBackend::open(&f.temp.path().join("primary"), 1024).unwrap();
    let recovery = LocalBackend::open(&f.temp.path().join("recovery"), 1024).unwrap();
    let grants = CopyBindings::new(
        "a".repeat(64),
        BTreeMap::from([
            (
                "primary".into(),
                ApprovedBackend::for_security(&primary, vec![Encryption::None]).unwrap(),
            ),
            (
                "recovery".into(),
                ApprovedBackend::for_security(&recovery, vec![Encryption::None]).unwrap(),
            ),
        ]),
    )
    .unwrap();
    transfer_copies(&lease, &grants, 1).unwrap();
    enrolled(&lease.load().unwrap())
}

async fn index_manifest(f: &Fixture, entries: Vec<Enrollment>) {
    use std::io::Read;
    let manifest = Manifest::new("a".repeat(64), entries).unwrap();
    let store = MetadataStore::open(
        &f.temp.path().join("metadata"),
        manifest.repo_id(),
        Limits::default(),
    )
    .unwrap();
    let adapter = WardenAdapter::new(
        &f.temp.path().join("warden-fixture"),
        &f.repo,
        manifest.repo_id(),
        Duration::from_secs(5),
    )
    .unwrap();
    let prepared = store
        .prepare(&manifest, &"b".repeat(64), &adapter, 1)
        .await
        .unwrap();
    let mut bytes = Vec::new();
    store
        .open_prepared(&prepared)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    std::fs::write(f.repo.join(".dracon/assets.manifest"), bytes).unwrap();
    assert!(git(&f.repo, &["add", "--", ".dracon/assets.manifest"])
        .status
        .success());
}

#[tokio::test]
async fn one_repository_driver_selects_multiple_paths_and_exact_historical_versions() {
    let f = fixture().await;
    let driver = git(&f.repo, &["config", "--get", "filter.dracon-storage.clean"]).stdout;
    let old_source = std::fs::read(f.repo.join("asset [version].bin")).unwrap();
    let old_metadata = std::fs::read(f.repo.join(".dracon/assets.manifest")).unwrap();
    let old = enrolled(&f.journal.lease(&f.job).unwrap().load().unwrap());
    let second = record_version(&f, b"second.bin", b"independently prepared second asset");
    index_manifest(&f, vec![old, second.clone()]).await;
    let first_add = git(&f.repo, &["add", "--", "asset [version].bin", "second.bin"]);
    assert!(
        first_add.status.success(),
        "{}",
        String::from_utf8_lossy(&first_add.stderr)
    );
    assert_eq!(
        git(&f.repo, &["show", ":second.bin"]).stdout,
        Pointer::new(second.payload.clone().unwrap())
            .unwrap()
            .encode()
    );
    let newer = record_version(
        &f,
        b"asset [version].bin",
        b"newer prepared first asset version",
    );
    index_manifest(&f, vec![newer.clone(), second]).await;
    assert!(git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(
        git(&f.repo, &["show", ":asset [version].bin"]).stdout,
        Pointer::new(newer.payload.unwrap()).unwrap().encode()
    );
    assert_eq!(
        git(&f.repo, &["config", "--get", "filter.dracon-storage.clean"]).stdout,
        driver
    );
    // An older indexed manifest selects its older local version, even with
    // newer prepared sources retained. It must not silently bless newer bytes.
    std::fs::write(f.repo.join(".dracon/assets.manifest"), old_metadata).unwrap();
    assert!(git(&f.repo, &["add", "--", ".dracon/assets.manifest"])
        .status
        .success());
    let before = std::fs::read(f.repo.join(".git/index")).unwrap();
    assert!(!git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), before);
    std::fs::write(f.repo.join("asset [version].bin"), old_source).unwrap();
    assert!(git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(
        git(&f.repo, &["show", ":asset [version].bin"]).stdout,
        f.pointer.encode()
    );
    assert!(verify_index(&f).status.success());
    assert!(!git(&f.repo, &["add", "--renormalize", "--", "second.bin"])
        .status
        .success());
}

#[tokio::test]
async fn busy_selected_job_fails_without_selecting_or_creating_another_version() {
    let f = fixture().await;
    let lease = f.journal.lease(&f.job).unwrap();
    let before = std::fs::read(f.repo.join(".git/index")).unwrap();
    assert!(!git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), before);
    assert_eq!(lease.load().unwrap().phase(), Phase::ReadyToStage);
    drop(lease);
    assert!(git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
}

fn verify_index(f: &Fixture) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dracon-sync"))
        .args(["storage", "verify-index", "--repo"])
        .arg(&f.repo)
        .args(["--repo-id", &"a".repeat(64), "--metadata-root"])
        .arg(f.temp.path().join("metadata"))
        .args(["--manifest-path", ".dracon/assets.manifest"])
        .output()
        .unwrap()
}

#[tokio::test]
async fn index_validation_catches_raw_and_missing_references_without_running_clean() {
    let f = fixture().await;
    assert!(!verify_index(&f).status.success());
    assert!(git(&f.repo, &["add", "--", "asset [version].bin"])
        .status
        .success());
    assert!(verify_index(&f).status.success());
    let repo = git2::Repository::open(&f.repo).unwrap();
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(Path::new("asset [version].bin"), 0).unwrap();
    entry.id = repo.blob(b"raw source bypassing filter").unwrap();
    index.add(&entry).unwrap();
    index.write().unwrap();
    let before = std::fs::read(f.repo.join(".git/index")).unwrap();
    assert!(!verify_index(&f).status.success());
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), before);
    index.remove_path(Path::new("asset [version].bin")).unwrap();
    index.write().unwrap();
    assert!(!verify_index(&f).status.success());
}
