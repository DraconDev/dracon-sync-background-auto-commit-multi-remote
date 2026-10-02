use super::*;
use crate::storage_core::backend::{ImmutableBackend, LocalBackend};
use crate::storage_core::bindings::ApprovedBackend;
use crate::storage_core::journal::encode_relative_path;
use crate::storage_core::manifest::Enrollment;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

struct Fixture {
    temp: tempfile::TempDir,
    metadata: MetadataStore,
    prepared: PreparedMetadata,
    manifest: Manifest,
    backend: LocalBackend,
    warden: WardenAdapter,
}

async fn fixture(encryption: Encryption) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let binary = temp.path().join("warden-fixture");
    std::fs::write(&binary, "#!/bin/sh\ncase \"$1\" in\nstorage-encrypt) printf 'age-encryption.org/v1\\n'; cat ;;\nstorage-decrypt) tail -n +2 ;;\n*) exit 99 ;;\nesac\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let warden =
        WardenAdapter::new(&binary, &repo, &"a".repeat(64), Duration::from_secs(5)).unwrap();
    let backend = LocalBackend::open(&temp.path().join("objects"), 1024).unwrap();
    let mut bytes = Vec::new();
    if encryption == Encryption::WardenAge {
        bytes.extend_from_slice(b"age-encryption.org/v1\n");
    }
    bytes.extend_from_slice(b"exact original bytes");
    let payload = backend.put(&mut bytes.as_slice()).unwrap();
    let manifest = Manifest::new(
        "a".repeat(64),
        vec![Enrollment {
            path_hex: encode_relative_path(b"private-\xff title.bin").unwrap(),
            contract_sha256: "b".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into(), "recovery".into()],
            encryption,
            payload: Some(payload),
        }],
    )
    .unwrap();
    let metadata = MetadataStore::open(
        &temp.path().join("metadata"),
        manifest.repo_id(),
        Limits::default(),
    )
    .unwrap();
    let prepared = metadata
        .prepare(&manifest, &"b".repeat(64), &warden, 1)
        .await
        .unwrap();
    Fixture {
        temp,
        metadata,
        prepared,
        manifest,
        backend,
        warden,
    }
}

#[tokio::test]
async fn private_recovery_verifies_plain_and_encrypted_versions_and_preserves_edits() {
    for encryption in [Encryption::None, Encryption::WardenAge] {
        let f = fixture(encryption).await;
        let root = f.temp.path().join("restored");
        let store = RestoreStore::open(&root, f.manifest.repo_id(), Limits::default()).unwrap();
        let binding = RestoreBinding::new(
            f.manifest.repo_id().into(),
            "recovery".into(),
            ApprovedBackend::for_security(&f.backend, vec![encryption]).unwrap(),
        )
        .unwrap();
        let path = &f.manifest.enrollments()[0].path_hex;
        let result = store
            .recover(
                &f.metadata,
                &f.prepared,
                &f.manifest,
                path,
                &binding,
                Some(&f.warden),
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(result.path()).unwrap(),
            b"exact original bytes"
        );
        assert_eq!(result.bytes(), 20);
        assert_eq!(
            std::fs::metadata(result.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let again = store
            .recover(
                &f.metadata,
                &f.prepared,
                &f.manifest,
                path,
                &binding,
                Some(&f.warden),
            )
            .await
            .unwrap();
        assert_eq!(again.path(), result.path());
        std::fs::write(result.path(), b"operator edit preserved").unwrap();
        assert!(store
            .recover(
                &f.metadata,
                &f.prepared,
                &f.manifest,
                path,
                &binding,
                Some(&f.warden)
            )
            .await
            .is_err());
        assert_eq!(
            std::fs::read(result.path()).unwrap(),
            b"operator edit preserved"
        );
    }
}

#[tokio::test]
async fn version_and_retention_limits_preserve_previous_recovery_and_tombstones_refuse() {
    let f = fixture(Encryption::None).await;
    let binding = RestoreBinding::new(
        f.manifest.repo_id().into(),
        "recovery".into(),
        ApprovedBackend::for_security(&f.backend, vec![Encryption::None]).unwrap(),
    )
    .unwrap();
    let path = &f.manifest.enrollments()[0].path_hex;
    let mut next = f.manifest.enrollments()[0].clone();
    next.payload = Some(
        f.backend
            .put(&mut b"different next bytes".as_slice())
            .unwrap(),
    );
    let next_manifest = Manifest::new(f.manifest.repo_id().into(), vec![next.clone()]).unwrap();
    let next_prepared = f
        .metadata
        .prepare(&next_manifest, &"b".repeat(64), &f.warden, 2)
        .await
        .unwrap();
    next.payload = None;
    let deleted = Manifest::new(f.manifest.repo_id().into(), vec![next]).unwrap();
    let deleted_prepared = f
        .metadata
        .prepare(&deleted, &"b".repeat(64), &f.warden, 3)
        .await
        .unwrap();
    for (name, max_records, retained) in [("versions", 1, 40), ("bytes", 3, 20)] {
        let store = RestoreStore::open(
            &f.temp.path().join(name),
            f.manifest.repo_id(),
            Limits {
                max_records,
                max_snapshot_bytes: 20,
                max_retained_snapshot_bytes: retained,
                ..Limits::default()
            },
        )
        .unwrap();
        let first = store
            .recover(&f.metadata, &f.prepared, &f.manifest, path, &binding, None)
            .await
            .unwrap();
        assert!(store
            .recover(
                &f.metadata,
                &next_prepared,
                &next_manifest,
                path,
                &binding,
                None
            )
            .await
            .is_err());
        assert!(store
            .recover(
                &f.metadata,
                &deleted_prepared,
                &deleted,
                path,
                &binding,
                None
            )
            .await
            .is_err());
        assert_eq!(
            std::fs::read(first.path()).unwrap(),
            b"exact original bytes"
        );
        let again = store
            .recover(&f.metadata, &f.prepared, &f.manifest, path, &binding, None)
            .await
            .unwrap();
        assert_eq!(again.path(), first.path());
        assert_eq!(
            std::fs::read_dir(&store.directory)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "source"))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn wrong_grants_missing_keys_and_failed_authenticated_output_never_publish() {
    let f = fixture(Encryption::WardenAge).await;
    let store = RestoreStore::open(
        &f.temp.path().join("restored"),
        f.manifest.repo_id(),
        Limits::default(),
    )
    .unwrap();
    let path = &f.manifest.enrollments()[0].path_hex;
    for (repo, copy, class) in [
        ("c".repeat(64), "recovery", Encryption::WardenAge),
        ("a".repeat(64), "foreign", Encryption::WardenAge),
        ("a".repeat(64), "recovery", Encryption::None),
    ] {
        let binding = RestoreBinding::new(
            repo,
            copy.into(),
            ApprovedBackend::for_security(&f.backend, vec![class]).unwrap(),
        )
        .unwrap();
        assert!(store
            .recover(
                &f.metadata,
                &f.prepared,
                &f.manifest,
                path,
                &binding,
                Some(&f.warden)
            )
            .await
            .is_err());
    }
    let binding = RestoreBinding::new(
        f.manifest.repo_id().into(),
        "recovery".into(),
        ApprovedBackend::encrypted(&f.backend),
    )
    .unwrap();
    assert!(store
        .recover(&f.metadata, &f.prepared, &f.manifest, path, &binding, None)
        .await
        .is_err());
    std::fs::write(
        f.temp.path().join("warden-fixture"),
        "#!/bin/sh\nprintf 'unauthenticated prefix'; exit 9\n",
    )
    .unwrap();
    assert!(store
        .recover(
            &f.metadata,
            &f.prepared,
            &f.manifest,
            path,
            &binding,
            Some(&f.warden)
        )
        .await
        .is_err());
    assert!(!std::fs::read_dir(&store.directory)
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "source")));
}

#[tokio::test]
async fn faulty_backend_and_byte_budgets_cannot_publish_unverified_output() {
    struct LyingBackend;
    impl ImmutableBackend for LyingBackend {
        fn put(&self, _: &mut dyn Read) -> Result<Fingerprint> {
            unreachable!()
        }
        fn get_verified(&self, _: &Fingerprint, out: &mut dyn Write) -> Result<()> {
            out.write_all(b"wrong bytes")?;
            Ok(())
        }
    }
    let f = fixture(Encryption::None).await;
    let store = RestoreStore::open(
        &f.temp.path().join("restored"),
        f.manifest.repo_id(),
        Limits::default(),
    )
    .unwrap();
    let binding = RestoreBinding::new(
        f.manifest.repo_id().into(),
        "recovery".into(),
        ApprovedBackend::for_security(&LyingBackend, vec![Encryption::None]).unwrap(),
    )
    .unwrap();
    let path = &f.manifest.enrollments()[0].path_hex;
    assert!(store
        .recover(&f.metadata, &f.prepared, &f.manifest, path, &binding, None)
        .await
        .is_err());
    let bounded = RestoreStore::open(
        &f.temp.path().join("bounded"),
        f.manifest.repo_id(),
        Limits {
            max_snapshot_bytes: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let binding = RestoreBinding::new(
        f.manifest.repo_id().into(),
        "recovery".into(),
        ApprovedBackend::for_security(&f.backend, vec![Encryption::None]).unwrap(),
    )
    .unwrap();
    assert!(bounded
        .recover(&f.metadata, &f.prepared, &f.manifest, path, &binding, None)
        .await
        .is_err());
    assert!(!std::fs::read_dir(&bounded.directory)
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "source")));
}

#[test]
fn publication_race_preserves_operator_destination_and_verified_capture() {
    struct RaceInput {
        input: std::io::Cursor<Vec<u8>>,
        destination: PathBuf,
    }
    impl Read for RaceInput {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = self.input.read(bytes)?;
            if count == 0 {
                std::fs::write(&self.destination, b"operator file created during capture")?;
            }
            Ok(count)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let id = "a".repeat(64);
    let destination = temp.path().join(format!("{id}.source"));
    let bytes = b"verified recovered content";
    let source =
        Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
    let mut input = RaceInput {
        input: std::io::Cursor::new(bytes.to_vec()),
        destination: destination.clone(),
    };
    assert!(journal::retain_snapshot(
        temp.path(),
        &id,
        SnapshotKind::Source,
        Limits::default(),
        &mut input,
        &source
    )
    .is_err());
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"operator file created during capture"
    );
    assert_eq!(
        std::fs::read(temp.path().join(format!("{id}.capture"))).unwrap(),
        bytes
    );
}

#[tokio::test]
#[ignore = "requires age-keygen and DRACON_STORAGE_TEST_WARDEN source-built binary"]
async fn real_cold_recovery_restores_large_encrypted_asset_without_original_source() {
    use std::process::{Command, Stdio};
    let binary = PathBuf::from(std::env::var_os("DRACON_STORAGE_TEST_WARDEN").unwrap());
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("producer");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let home = temp.path().join("keys-home");
    std::fs::create_dir_all(home.join(".dracon/keys")).unwrap();
    let keys = home.join(".dracon/keys/identity.age");
    assert!(Command::new("age-keygen")
        .args(["-o"])
        .arg(&keys)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600)).unwrap();
    let source_path = temp.path().join("original-source");
    let mut source = journal::open_private(&source_path, true, true).unwrap();
    let bytes = 101 * 1024 * 1024u64;
    let chunk = [0x39u8; 64 * 1024];
    let mut hash = Sha256::new();
    for _ in 0..bytes / chunk.len() as u64 {
        source.write_all(&chunk).unwrap();
        hash.update(chunk);
    }
    let expected = Fingerprint::new(format!("{:x}", hash.finalize()), bytes).unwrap();
    source.seek(SeekFrom::Start(0)).unwrap();
    let cipher_path = temp.path().join("original-ciphertext");
    let mut cipher = journal::open_private(&cipher_path, true, true).unwrap();
    assert!(Command::new(&binary)
        .args(["storage-encrypt", "--repo"])
        .arg(&repo)
        .args(["--max-bytes", &bytes.to_string()])
        .env("HOME", &home)
        .env_remove("ARCANE_MACHINE_KEY")
        .stdin(Stdio::from(source))
        .stdout(Stdio::from(cipher.try_clone().unwrap()))
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    cipher.seek(SeekFrom::Start(0)).unwrap();
    let object_root = temp.path().join("recovery-objects");
    let backend = LocalBackend::open(&object_root, bytes * 2).unwrap();
    let payload = backend.put(&mut cipher).unwrap();
    let manifest = Manifest::new(
        "a".repeat(64),
        vec![Enrollment {
            path_hex: encode_relative_path(b"assets/private-video.mp4").unwrap(),
            contract_sha256: "b".repeat(64),
            primary: "recovery".into(),
            required_copies: vec!["recovery".into()],
            encryption: Encryption::WardenAge,
            payload: Some(payload.clone()),
        }],
    )
    .unwrap();
    let original_metadata = temp.path().join("original-metadata");
    let metadata =
        MetadataStore::open(&original_metadata, manifest.repo_id(), Limits::default()).unwrap();
    let adapter = WardenAdapter::new(&binary, &repo, manifest.repo_id(), Duration::from_secs(180))
        .unwrap()
        .with_identity_home(&home)
        .unwrap();
    let prepared = metadata
        .prepare(&manifest, &"b".repeat(64), &adapter, 1)
        .await
        .unwrap();
    let mut protected = Vec::new();
    metadata
        .open_prepared(&prepared)
        .unwrap()
        .read_to_end(&mut protected)
        .unwrap();
    let metadata_payload = prepared.payload().clone();
    drop(metadata);
    drop(cipher);
    drop(backend);
    // Remove only test-owned originals. Cold recovery has immutable objects,
    // committed metadata bytes and independently retained authorized keys.
    std::fs::remove_file(source_path).unwrap();
    std::fs::remove_file(cipher_path).unwrap();
    std::fs::remove_dir_all(original_metadata).unwrap();
    let moved = temp.path().join("cold-checkout");
    std::fs::create_dir_all(moved.join(".git")).unwrap();
    let warden = WardenAdapter::new(
        &binary,
        &moved,
        manifest.repo_id(),
        Duration::from_secs(180),
    )
    .unwrap()
    .with_identity_home(&home)
    .unwrap();
    let metadata = MetadataStore::open(
        &temp.path().join("cold-metadata"),
        manifest.repo_id(),
        Limits::default(),
    )
    .unwrap();
    let (prepared, decoded) = metadata
        .import(
            &mut protected.as_slice(),
            &metadata_payload,
            &"b".repeat(64),
            &warden,
        )
        .await
        .unwrap();
    let backend = LocalBackend::open_existing(&object_root, bytes * 2).unwrap();
    let grant = RestoreBinding::new(
        manifest.repo_id().into(),
        "recovery".into(),
        ApprovedBackend::encrypted(&backend),
    )
    .unwrap();
    let store = RestoreStore::open(
        &temp.path().join("restored"),
        manifest.repo_id(),
        Limits::default(),
    )
    .unwrap();
    let restored = store
        .recover(
            &metadata,
            &prepared,
            &decoded,
            &manifest.enrollments()[0].path_hex,
            &grant,
            Some(&warden),
        )
        .await
        .unwrap();
    assert_eq!(restored.bytes(), bytes);
    journal::verify_snapshot(restored.path(), &expected).unwrap();
    assert!(!moved.join("assets/private-video.mp4").exists());
    #[cfg(target_os = "linux")]
    {
        use git2::{IndexEntry, IndexTime};
        // The actual-key recovery proof can publish to an isolated checkout;
        // the filesystem fixture has no original source/journal/cache to use.
        let repository = git2::Repository::init(&moved).unwrap();
        repository
            .config()
            .unwrap()
            .set_str("dracon.storageRepoId", manifest.repo_id())
            .unwrap();
        std::fs::create_dir(moved.join("assets")).unwrap();
        let pointer = crate::storage_core::reference::Pointer::new(payload)
            .unwrap()
            .encode();
        std::fs::write(moved.join("assets/private-video.mp4"), &pointer).unwrap();
        let mut index = repository.index().unwrap();
        for (path, data) in [
            (b"assets/private-video.mp4".as_slice(), pointer.as_slice()),
            (b"assets.manifest".as_slice(), protected.as_slice()),
        ] {
            index
                .add(&IndexEntry {
                    ctime: IndexTime::new(0, 0),
                    mtime: IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100644,
                    uid: 0,
                    gid: 0,
                    file_size: data.len() as u32,
                    id: repository.blob(data).unwrap(),
                    flags: 0,
                    flags_extended: 0,
                    path: path.to_vec(),
                })
                .unwrap();
        }
        index.write().unwrap();
        let before = std::fs::read(repository.path().join("index")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("DraconDev", "dracsharp@gmail.com").unwrap();
        let commit = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "cold encrypted hydration fixture",
                &tree,
                &[],
            )
            .unwrap();
        let hydrated = crate::storage_core::hydration::HydrationStore::open(
            &temp.path().join("hydration"),
            manifest.repo_id(),
            Limits::default(),
        )
        .unwrap()
        .hydrate(
            &repository,
            &restored,
            Path::new("assets.manifest"),
            commit,
            |repo| {
                crate::storage_core::index::verify_manifest_entries(repo, &repo.index()?, &decoded)
            },
        )
        .unwrap();
        journal::verify_snapshot(hydrated.path(), &expected).unwrap();
        assert_eq!(std::fs::read(hydrated.backup().unwrap()).unwrap(), pointer);
        assert_eq!(
            std::fs::read(repository.path().join("index")).unwrap(),
            before
        );
        assert!(!repository.path().join("index.lock").exists());
    }
}
