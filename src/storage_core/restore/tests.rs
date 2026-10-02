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
