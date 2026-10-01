use super::*;

fn fingerprint(character: char, bytes: u64) -> Fingerprint {
    Fingerprint::new(character.to_string().repeat(64), bytes).unwrap()
}

fn spec() -> JobSpec {
    JobSpec {
        repo_id: "a".repeat(64),
        path_hex: encode_relative_path(b"assets/private.bin").unwrap(),
        source: fingerprint('b', 100),
        policy_sha256: "c".repeat(64),
        primary: "primary".into(),
        required_copies: vec!["primary".into(), "recovery".into()],
        required_git_targets: vec!["github".into(), "gitlab".into()],
        encryption: Encryption::WardenAge,
    }
}

fn fixture() -> (tempfile::TempDir, Journal, Job) {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::open(
        &temp.path().join("journal"),
        &spec().repo_id,
        Limits::default(),
    )
    .unwrap();
    let job = journal.create(spec()).unwrap();
    (temp, journal, job)
}

fn prepare(lease: &JobLease, job: &mut Job) {
    job.record_capture(job.spec.source.clone()).unwrap();
    lease.save(job).unwrap();
    job.record_prepared(fingerprint('d', 200)).unwrap();
    lease.save(job).unwrap();
    job.begin_upload(1).unwrap();
    lease.save(job).unwrap();
}

#[test]
fn exact_version_identity_separates_source_policy_path_and_repository() {
    let base = Job::new(spec()).unwrap();
    for variant in 0..4 {
        let mut input = spec();
        match variant {
            0 => input.source = fingerprint('d', 100),
            1 => input.policy_sha256 = "d".repeat(64),
            2 => input.path_hex = encode_relative_path(b"other.bin").unwrap(),
            _ => input.repo_id = "d".repeat(64),
        }
        assert_ne!(base.id(), Job::new(input).unwrap().id());
    }
    let debug = format!("{base:?}");
    assert!(!debug.contains(base.spec.source.sha256()));
    assert!(!debug.contains("private.bin"));
}

#[test]
fn required_copies_and_git_destinations_gate_preservation() {
    let (temp, journal, mut job) = fixture();
    let lease = journal.lease(job.id()).unwrap();
    assert!(job.record_prepared(fingerprint('d', 200)).is_err());
    assert!(job.record_capture(fingerprint('e', 100)).is_err());
    prepare(&lease, &mut job);
    assert!(job
        .record_copy("unapproved", fingerprint('d', 200), 1)
        .is_err());
    assert!(job
        .record_copy("primary", fingerprint('e', 200), 1)
        .is_err());
    job.record_copy("primary", fingerprint('d', 200), 1)
        .unwrap();
    lease.save(&mut job).unwrap();
    assert_eq!(job.phase(), Phase::PrimaryVerified);
    let pointer = Pointer::new(fingerprint('d', 200)).unwrap();
    assert!(job.record_staged(&pointer).is_err());
    job.record_copy("recovery", fingerprint('d', 200), 2)
        .unwrap();
    lease.save(&mut job).unwrap();
    assert_eq!(job.phase(), Phase::ReadyToStage);
    assert!(job
        .record_staged(&Pointer::new(fingerprint('e', 200)).unwrap())
        .is_err());
    job.record_staged(&pointer).unwrap();
    lease.save(&mut job).unwrap();
    let commit = "1".repeat(40);
    job.record_commit(commit.clone()).unwrap();
    lease.save(&mut job).unwrap();
    assert!(job.record_git_push("github", &"2".repeat(40)).is_err());
    job.record_git_push("github", &commit).unwrap();
    lease.save(&mut job).unwrap();
    assert_eq!(job.phase(), Phase::Committed);
    job.record_git_push("gitlab", &commit).unwrap();
    lease.save(&mut job).unwrap();
    assert_eq!(job.phase(), Phase::Preserved);
    assert!(job.cancel().is_err());
    let summary = Journal::inspect(&temp.path().join("journal"), &spec().repo_id).unwrap();
    assert_eq!(summary.recorded_preserved, 1);
    assert_eq!(summary.pending_source_bytes, 0);
    job.note_failure(FailureCode::Integrity, None).unwrap();
    lease.save(&mut job).unwrap();
    assert_eq!(
        Journal::inspect(&temp.path().join("journal"), &spec().repo_id)
            .unwrap()
            .recorded_preserved,
        0
    );
}

#[test]
fn restart_preserves_upload_evidence_and_retry_backoff() {
    let (temp, journal, mut job) = fixture();
    let lease = journal.lease(job.id()).unwrap();
    prepare(&lease, &mut job);
    job.record_copy("primary", fingerprint('d', 200), 1)
        .unwrap();
    lease.save(&mut job).unwrap();
    job.note_failure(FailureCode::Transient, Some(100)).unwrap();
    lease.save(&mut job).unwrap();
    drop(lease);
    drop(journal);
    let reopened = Journal::open(
        &temp.path().join("journal"),
        &spec().repo_id,
        Limits::default(),
    )
    .unwrap();
    let lease = reopened.lease(job.id()).unwrap();
    let mut restored = lease.load().unwrap();
    assert_eq!(restored.phase(), Phase::PrimaryVerified);
    assert!(restored.begin_upload(99).is_err());
    restored.begin_upload(100).unwrap();
    assert_eq!(restored.phase(), Phase::PrimaryVerified);
    restored
        .note_failure(FailureCode::Credentials, None)
        .unwrap();
    assert!(restored.begin_upload(101).is_err());
    restored.clear_failure();
    restored.begin_upload(101).unwrap();
    restored
        .record_copy("recovery", fingerprint('d', 200), 102)
        .unwrap();
    lease.save(&mut restored).unwrap();
    assert_eq!(restored.phase(), Phase::ReadyToStage);
}

#[test]
fn exclusive_leases_and_revision_checks_refuse_stale_updates() {
    let (_temp, journal, mut job) = fixture();
    let lease = journal.lease(job.id()).unwrap();
    assert!(journal.lease(job.id()).is_err());
    let mut stale = job.clone();
    job.record_capture(job.spec.source.clone()).unwrap();
    lease.save(&mut job).unwrap();
    stale.record_capture(stale.spec.source.clone()).unwrap();
    assert!(lease.save(&mut stale).is_err());
    drop(lease);
    assert!(journal.lease(job.id()).is_ok());
}

#[test]
fn cancelled_and_changed_sources_do_not_overwrite_old_jobs() {
    let (_temp, journal, mut old) = fixture();
    let lease = journal.lease(old.id()).unwrap();
    old.record_capture(old.spec.source.clone()).unwrap();
    lease.save(&mut old).unwrap();
    old.cancel().unwrap();
    lease.save(&mut old).unwrap();
    let mut next_spec = spec();
    next_spec.source = fingerprint('f', 101);
    let next = journal.create(next_spec).unwrap();
    assert_ne!(old.id(), next.id());
    assert_eq!(lease.load().unwrap().phase(), Phase::Cancelled);
    assert!(old.record_prepared(fingerprint('d', 200)).is_err());
}

#[test]
fn forged_regression_and_plaintext_encryption_are_rejected() {
    let (_temp, journal, mut job) = fixture();
    let lease = journal.lease(job.id()).unwrap();
    job.record_capture(job.spec.source.clone()).unwrap();
    lease.save(&mut job).unwrap();
    assert!(job.record_prepared(job.spec.source.clone()).is_err());
    let mut forged = lease.load().unwrap();
    forged.phase = Phase::PendingCapture;
    forged.capture = None;
    assert!(lease.save(&mut forged).is_err());
    job.record_prepared(fingerprint('d', 200)).unwrap();
    job.begin_upload(1).unwrap();
    assert!(
        lease.save(&mut job).is_err(),
        "must not skip durable prepared evidence"
    );
}

#[test]
fn corrupt_unknown_or_unsafe_records_are_retained_and_not_green() {
    let (temp, journal, job) = fixture();
    let path = journal.directory.join(format!("{}.json", job.id()));
    let lease = journal.lease(job.id()).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut decoded: serde_json::Value = serde_json::from_slice(&original).unwrap();
    decoded["schema_version"] = 999.into();
    std::fs::write(&path, serde_json::to_vec(&decoded).unwrap()).unwrap();
    assert!(lease.load().is_err());
    assert!(journal.create(spec()).is_err());
    let summary = Journal::inspect(&temp.path().join("journal"), &spec().repo_id).unwrap();
    assert_eq!(summary.invalid_records, 1);
    assert_eq!(summary.recorded_preserved, 0);
    assert!(path.is_file());
    std::fs::write(&path, b"private-token-with-invalid-json").unwrap();
    assert!(!format!("{:?}", lease.load().unwrap_err()).contains("private-token"));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"private-token-with-invalid-json"
    );
}

#[test]
fn record_budget_and_read_only_status_never_delete_or_create_state() {
    let temp = tempfile::tempdir().unwrap();
    let absent = temp.path().join("absent");
    assert!(
        !Journal::inspect(&absent, &spec().repo_id)
            .unwrap()
            .initialized
    );
    assert!(!absent.exists());
    let journal = Journal::open(
        &temp.path().join("journal"),
        &spec().repo_id,
        Limits {
            max_records: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let old = journal.create(spec()).unwrap();
    assert_eq!(journal.create(spec()).unwrap().id(), old.id());
    let mut second = spec();
    second.source = fingerprint('e', 101);
    assert!(journal.create(second).is_err());
    assert_eq!(
        Journal::inspect(&temp.path().join("journal"), &spec().repo_id)
            .unwrap()
            .records,
        1
    );
    assert_eq!(
        journal.lease(old.id()).unwrap().load().unwrap().phase(),
        Phase::PendingCapture
    );
}

#[test]
fn relative_paths_and_required_sets_are_validated() {
    for path in [
        b"".as_slice(),
        b"/absolute",
        b"../escape",
        b"a/../escape",
        b"a/.git/config",
        b"a/.GIT/config",
        b"a//b",
        b"a\\b",
        b"C:relative",
        b"nul\0file",
    ] {
        assert!(encode_relative_path(path).is_err());
    }
    let odd = encode_relative_path(b"assets/non-utf8-\xff").unwrap();
    assert!(validate_path_hex(&odd).is_ok());
    let mut input = spec();
    input.required_copies = vec!["recovery".into()];
    assert!(input.validate().is_err());
    input = spec();
    input.required_copies.push("primary".into());
    assert!(input.validate().is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_hard_links_and_public_permissions_fail_closed() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (temp, journal, job) = fixture();
    let record = journal.directory.join(format!("{}.json", job.id()));
    let outside = temp.path().join("outside");
    std::fs::rename(&record, &outside).unwrap();
    symlink(&outside, &record).unwrap();
    assert!(journal.lease(job.id()).unwrap().load().is_err());
    std::fs::remove_file(&record).unwrap();
    std::fs::hard_link(&outside, &record).unwrap();
    assert!(journal.lease(job.id()).unwrap().load().is_err());
    std::fs::remove_file(&record).unwrap();
    std::fs::rename(&outside, &record).unwrap();
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(journal.lease(job.id()).unwrap().load().is_err());
    let missing_link = temp.path().join("missing-link");
    symlink(temp.path().join("missing-target"), &missing_link).unwrap();
    assert!(Journal::inspect(&missing_link, &spec().repo_id).is_err());
}

#[test]
fn process_death_releases_lease_and_atomic_write_has_old_or_new_record() {
    for phase in ["before-publish", "after-publish"] {
        let (temp, journal, job) = fixture();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage_core::journal::tests::crash_child",
                "--ignored",
            ])
            .env("DRACON_STORAGE_CRASH_ROOT", temp.path().join("journal"))
            .env("DRACON_STORAGE_CRASH_JOB", job.id())
            .env("DRACON_STORAGE_CRASH_POINT", phase)
            .output()
            .unwrap()
            .status;
        assert_eq!(status.code(), Some(73));
        let lease = journal.lease(job.id()).unwrap();
        let recovered = lease.load().unwrap();
        assert_eq!(
            recovered.phase(),
            if phase == "before-publish" {
                Phase::PendingCapture
            } else {
                Phase::Captured
            }
        );
        assert_eq!(recovered.spec.source, job.spec.source);
        assert_eq!(
            Journal::inspect(&temp.path().join("journal"), &spec().repo_id)
                .unwrap()
                .invalid_records,
            0
        );
    }
}

#[test]
#[ignore = "subprocess helper, invoked by the crash recovery test"]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os("DRACON_STORAGE_CRASH_ROOT").unwrap());
    let id = std::env::var("DRACON_STORAGE_CRASH_JOB").unwrap();
    let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
    let lease = journal.lease(&id).unwrap();
    let mut job = lease.load().unwrap();
    job.record_capture(job.spec.source.clone()).unwrap();
    lease.save(&mut job).unwrap();
    panic!("crash injection did not fire");
}

fn real_source_spec(bytes: &[u8]) -> JobSpec {
    let mut input = spec();
    input.source =
        Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap();
    input
}

#[test]
fn source_snapshots_resume_partial_capture_and_reuse_exact_completed_version() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("journal");
    let data = b"the exact source version captured before upload";
    let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
    let pending = journal.create(real_source_spec(data)).unwrap();
    let lease = journal.lease(pending.id()).unwrap();
    // Simulate a process dying after a prefix reached disk, with its lease released.
    let capture = journal.directory.join(format!("{}.capture", pending.id()));
    let mut file = open_private(&capture, true, true).unwrap();
    file.write_all(&data[..7]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let captured = lease.capture_snapshot(&mut &data[..]).unwrap();
    assert_eq!(captured.phase(), Phase::Captured);
    assert!(!capture.exists());
    let mut actual = Vec::new();
    lease
        .source_snapshot()
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, data);
    // A complete captured version remains available even after the live file changes.
    lease
        .capture_snapshot(&mut &b"newer unrelated working tree bytes"[..])
        .unwrap();
    let mut again = Vec::new();
    lease
        .source_snapshot()
        .unwrap()
        .read_to_end(&mut again)
        .unwrap();
    assert_eq!(again, data);
}

#[test]
fn wrong_capture_prefix_and_corrupt_snapshot_fail_without_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::open(
        &temp.path().join("journal"),
        &spec().repo_id,
        Limits::default(),
    )
    .unwrap();
    let data = b"immutable source";
    let pending = journal.create(real_source_spec(data)).unwrap();
    let lease = journal.lease(pending.id()).unwrap();
    let capture = journal.directory.join(format!("{}.capture", pending.id()));
    let mut file = open_private(&capture, true, true).unwrap();
    file.write_all(b"wrong").unwrap();
    drop(file);
    assert!(lease.capture_snapshot(&mut &data[..]).is_err());
    assert_eq!(std::fs::read(&capture).unwrap(), b"wrong");
    assert_eq!(lease.load().unwrap().phase(), Phase::PendingCapture);
    // Explicit test maintenance of its own invalid spool, never production cleanup.
    std::fs::remove_file(&capture).unwrap();
    lease.capture_snapshot(&mut &data[..]).unwrap();
    let source = journal.directory.join(format!("{}.source", pending.id()));
    std::fs::write(&source, b"corrupted source").unwrap();
    assert!(lease.source_snapshot().is_err());
    assert!(lease.capture_snapshot(&mut &data[..]).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), b"corrupted source");
}

#[test]
fn aggregate_capture_budget_refuses_new_work_without_deleting_retained_versions() {
    let temp = tempfile::tempdir().unwrap();
    let limits = Limits {
        max_snapshot_bytes: 20,
        max_retained_snapshot_bytes: 20,
        ..Limits::default()
    };
    let journal = Journal::open(&temp.path().join("journal"), &spec().repo_id, limits).unwrap();
    let first = journal.create(real_source_spec(b"first snapshot")).unwrap();
    let lease = journal.lease(first.id()).unwrap();
    lease.capture_snapshot(&mut &b"first snapshot"[..]).unwrap();
    let second = journal
        .create(real_source_spec(b"second snapshot"))
        .unwrap();
    let second_lease = journal.lease(second.id()).unwrap();
    assert!(second_lease
        .capture_snapshot(&mut &b"second snapshot"[..])
        .is_err());
    assert!(lease.source_snapshot().is_ok());
    assert_eq!(second_lease.load().unwrap().phase(), Phase::PendingCapture);
}

#[test]
fn complete_capture_recovers_after_death_before_or_after_snapshot_publish() {
    for phase in ["before-snapshot-publish", "after-snapshot-publish"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("journal");
        let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
        let job = journal.create(real_source_spec(b"crash snapshot")).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage_core::journal::tests::snapshot_crash_child",
                "--ignored",
            ])
            .env("DRACON_STORAGE_CRASH_ROOT", &root)
            .env("DRACON_STORAGE_CRASH_JOB", job.id())
            .env("DRACON_STORAGE_CRASH_POINT", phase)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(73));
        let lease = journal.lease(job.id()).unwrap();
        // No live source read is necessary when the complete saved capture verifies.
        let recovered = lease.capture_snapshot(&mut &b""[..]).unwrap();
        assert_eq!(recovered.phase(), Phase::Captured);
        let mut restored = Vec::new();
        lease
            .source_snapshot()
            .unwrap()
            .read_to_end(&mut restored)
            .unwrap();
        assert_eq!(restored, b"crash snapshot");
    }
}

#[test]
#[ignore = "subprocess helper, invoked by snapshot crash recovery test"]
fn snapshot_crash_child() {
    let root = PathBuf::from(std::env::var_os("DRACON_STORAGE_CRASH_ROOT").unwrap());
    let id = std::env::var("DRACON_STORAGE_CRASH_JOB").unwrap();
    let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
    journal
        .lease(&id)
        .unwrap()
        .capture_snapshot(&mut &b"crash snapshot"[..])
        .unwrap();
    panic!("snapshot crash injection did not fire");
}

fn digest_bytes(bytes: &[u8]) -> Fingerprint {
    Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap()
}

#[test]
fn prepared_payload_is_retained_once_and_cannot_change_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("journal");
    let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
    let source = b"private exact source";
    // Opaque fixture bytes: authorization/encryption belong to the caller.
    let payload = b"approved opaque representation one";
    let expected = digest_bytes(payload);
    let job = journal.create(real_source_spec(source)).unwrap();
    let lease = journal.lease(job.id()).unwrap();
    assert!(lease.retain_payload(&mut &payload[..], &expected).is_err());
    lease.capture_snapshot(&mut &source[..]).unwrap();
    assert!(lease
        .retain_payload(&mut &source[..], &digest_bytes(source))
        .is_err());
    let prepared = lease.retain_payload(&mut &payload[..], &expected).unwrap();
    assert_eq!(prepared.phase(), Phase::Prepared);
    drop(lease);
    drop(journal);
    let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
    let lease = journal.lease(job.id()).unwrap();
    // Repeated preparation never substitutes new randomness/working tree bytes.
    lease
        .retain_payload(&mut &b"different new ciphertext"[..], &expected)
        .unwrap();
    assert!(lease
        .retain_payload(&mut &b"replacement"[..], &digest_bytes(b"replacement"))
        .is_err());
    let mut actual = Vec::new();
    lease
        .payload_snapshot()
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, payload);
    let path = journal.directory.join(format!("{}.payload", job.id()));
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(lease.payload_snapshot().is_err());
    assert!(lease.retain_payload(&mut &payload[..], &expected).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"corrupt");
}

#[test]
fn payload_spools_resume_matching_prefix_and_have_independent_byte_budget() {
    let temp = tempfile::tempdir().unwrap();
    let limits = Limits {
        max_payload_bytes: 20,
        max_retained_payload_bytes: 20,
        ..Limits::default()
    };
    let journal = Journal::open(&temp.path().join("journal"), &spec().repo_id, limits).unwrap();
    let source = b"source";
    let payload = b"opaque payload one";
    let first = journal.create(real_source_spec(source)).unwrap();
    let lease = journal.lease(first.id()).unwrap();
    lease.capture_snapshot(&mut &source[..]).unwrap();
    let spool = journal
        .directory
        .join(format!("{}.payload-capture", first.id()));
    let mut file = open_private(&spool, true, true).unwrap();
    file.write_all(&payload[..6]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    lease
        .retain_payload(&mut &payload[..], &digest_bytes(payload))
        .unwrap();
    assert!(!spool.exists());
    let second = journal
        .create(real_source_spec(b"different source"))
        .unwrap();
    let second_lease = journal.lease(second.id()).unwrap();
    second_lease
        .capture_snapshot(&mut &b"different source"[..])
        .unwrap();
    assert!(second_lease
        .retain_payload(&mut &payload[..], &digest_bytes(payload))
        .is_err());
    assert_eq!(second_lease.load().unwrap().phase(), Phase::Captured);
    assert!(lease.payload_snapshot().is_ok());
    assert!(second_lease.source_snapshot().is_ok());
}

#[test]
fn prepared_payload_recovers_process_death_without_repeating_security_processing() {
    for phase in [
        "before-payload-publish",
        "after-payload-publish",
        "before-publish",
        "after-publish",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("journal");
        let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
        let job = journal
            .create(real_source_spec(b"crash payload source"))
            .unwrap();
        journal
            .lease(job.id())
            .unwrap()
            .capture_snapshot(&mut &b"crash payload source"[..])
            .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage_core::journal::tests::payload_crash_child",
                "--ignored",
            ])
            .env("DRACON_STORAGE_CRASH_ROOT", &root)
            .env("DRACON_STORAGE_CRASH_JOB", job.id())
            .env("DRACON_STORAGE_CRASH_POINT", phase)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(73));
        let lease = journal.lease(job.id()).unwrap();
        let recovered = lease
            .retain_payload(&mut &b""[..], &digest_bytes(b"approved crash payload"))
            .unwrap();
        assert_eq!(recovered.phase(), Phase::Prepared);
        let mut actual = Vec::new();
        lease
            .payload_snapshot()
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, b"approved crash payload");
    }
}

#[test]
#[ignore = "subprocess helper, invoked by payload crash recovery test"]
fn payload_crash_child() {
    let root = PathBuf::from(std::env::var_os("DRACON_STORAGE_CRASH_ROOT").unwrap());
    let id = std::env::var("DRACON_STORAGE_CRASH_JOB").unwrap();
    let journal = Journal::open(&root, &spec().repo_id, Limits::default()).unwrap();
    journal
        .lease(&id)
        .unwrap()
        .retain_payload(
            &mut &b"approved crash payload"[..],
            &digest_bytes(b"approved crash payload"),
        )
        .unwrap();
    panic!("payload crash injection did not fire");
}
