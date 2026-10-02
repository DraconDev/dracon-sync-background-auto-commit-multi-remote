use super::*;

fn policy() -> StoragePolicy {
    toml::from_str(
        r#"
enabled = true
[backends.archive]
type = "local"
root = "/operator/archive"
[[rules]]
paths = ["assets/keep.png"]
placement = "git"
[[rules]]
paths = ["assets/**"]
placement = "external"
min_bytes = 20
backend = "archive"
security = "warden-encrypted"
[[rules]]
paths = ["renders/**"]
placement = "external"
backend = "archive"
security = "warden-encrypted"
"#,
    )
    .unwrap()
}

#[test]
fn rule_order_threshold_and_no_universal_media_policy() {
    let compiled = CompiledPolicy::new(policy()).unwrap();
    assert_eq!(
        compiled.decide(Path::new("assets/keep.png"), 200).placement,
        Placement::Git
    );
    assert_eq!(
        compiled.decide(Path::new("assets/test.png"), 19).placement,
        Placement::Git
    );
    assert_eq!(
        compiled.decide(Path::new("assets/test.png"), 20).rule,
        Some(1)
    );
    assert_eq!(
        compiled
            .decide(Path::new("assets/deep/test.png"), 21)
            .placement,
        Placement::External
    );
    assert_eq!(
        compiled.decide(Path::new("renders/frame.png"), 1).placement,
        Placement::External
    );
    assert_eq!(
        compiled.decide(Path::new("unmatched.mp4"), 1000).placement,
        Placement::Git
    );
    let one = compile_pattern("assets/*").unwrap();
    assert!(!one.is_match("assets/deep/test.png"));
}

#[test]
fn default_disabled_explicit_false_and_complete_rule_replacement() {
    let global = policy();
    let disabled: StorageOverride = toml::from_str("enabled = false").unwrap();
    let effective = effective_policy(&global, Some(&disabled));
    assert!(!effective.enabled);
    assert_eq!(effective.backends.len(), 1);
    assert_eq!(
        CompiledPolicy::new(effective)
            .unwrap()
            .decide(Path::new("renders/frame.png"), 500)
            .placement,
        Placement::Git
    );
    let replacement: StorageOverride = toml::from_str("rules = []").unwrap();
    assert!(effective_policy(&global, Some(&replacement))
        .rules
        .is_empty());
    assert_eq!(effective_policy(&global, None).rules.len(), 3);
    assert!(!StoragePolicy::default().enabled);
    let old: SyncPolicy = toml::from_str("").unwrap();
    assert!(!old.storage.enabled);
}

#[test]
fn reject_unapproved_backend_and_repo_backend_injection() {
    let mut global = policy();
    global.rules[1].backend = Some("attacker".into());
    assert!(validate_policy(&global)
        .unwrap_err()
        .to_string()
        .contains("unapproved"));
    assert!(toml::from_str::<StorageOverride>(
        r#"
[backends.evil]
type = "s3"
endpoint = "https://evil.invalid"
"#
    )
    .is_err());
    assert!(toml::from_str::<RepoPolicyOverride>(
        r#"
[storage.backends.evil]
type = "local"
root = "/tmp/evil"
"#
    )
    .is_err());
}

#[test]
fn reject_invalid_rules_and_secret_bearing_endpoints() {
    for pattern in ["", "/etc/**", "../**", ".git/**", "a/.git/**", "a\\b", "["] {
        assert!(compile_pattern(pattern).is_err(), "{pattern:?}");
    }
    let mut global = policy();
    global.rules[1].security = None;
    assert!(validate_policy(&global).is_err());
    let mut global = policy();
    global.rules.push(global.rules[1].clone());
    assert!(validate_policy(&global).is_err());
    for endpoint in [
        "http://host.invalid",
        "https://user:secret@host.invalid",
        "https://host.invalid/?token=secret",
        "https://host.invalid/#secret",
    ] {
        let mut global = policy();
        global.backends.insert(
            "archive".into(),
            BackendBinding::S3 {
                endpoint: endpoint.into(),
                bucket: "assets".into(),
                credential_ref: "approved".into(),
                allowed_security: encrypted_only(),
            },
        );
        assert!(validate_policy(&global).is_err());
    }
}

fn git_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    assert!(crate::test_helpers::test_git_cmd()
        .args(["-c", "init.templateDir=", "init", "--quiet"])
        .arg(dir.path())
        .status()
        .unwrap()
        .success());
    dir
}

fn git_fixture_command(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = crate::test_helpers::test_git_cmd()
        .current_dir(repo)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn real_git_inventory_is_read_only_ignores_payload_filters_and_reports_migration() {
    let dir = git_fixture();
    let repo = dir.path();
    std::fs::create_dir(repo.join("assets")).unwrap();
    std::fs::write(repo.join("assets/tracked.png"), [7u8; 30]).unwrap();
    std::fs::write(repo.join("ignored.mp4"), [9u8; 40]).unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored.mp4\n").unwrap();
    git_fixture_command(repo, &["add", "--", "assets/tracked.png", ".gitignore"]);
    // A planner must not invoke even a required filter, or need a network/warden process.
    std::fs::write(repo.join(".gitattributes"), "* filter=never-run\n").unwrap();
    git_fixture_command(repo, &["config", "filter.never-run.clean", "exit 99"]);
    git_fixture_command(repo, &["config", "filter.never-run.required", "true"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let compiled = CompiledPolicy::new(policy()).unwrap();
    let global = crate::policy::test_sync_policy();
    let plan = inventory(repo, &global, &RepoPolicyOverride::default(), &compiled).unwrap();
    assert!(!plan.files.iter().any(|file| file.path == "ignored.mp4"));
    let asset = plan
        .files
        .iter()
        .find(|file| file.path == "assets/tracked.png")
        .unwrap();
    assert!(asset.tracked);
    assert_eq!(asset.filter.as_deref(), Some("never-run"));
    assert!(asset
        .concerns
        .iter()
        .any(|c| c.contains("existing Git filter")));
    assert_eq!(asset.decision.placement, Placement::External);
    assert!(asset.concerns.iter().any(|c| c.contains("migration")));
    assert_eq!(plan.proposed_external_bytes, 30);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(repo.join("assets/tracked.png")).unwrap(),
        vec![7; 30]
    );
}

#[test]
fn exclusions_and_large_unmatched_files_remain_visible() {
    let dir = git_fixture();
    std::fs::create_dir(dir.path().join("renders")).unwrap();
    std::fs::write(dir.path().join("renders/frame.png"), [7u8; 30]).unwrap();
    std::fs::write(dir.path().join("big.json"), [7u8; 30]).unwrap();
    let mut global = crate::policy::test_sync_policy();
    global.max_stage_file_bytes = 20;
    let local = RepoPolicyOverride {
        auto_commit_exclude_patterns: Some(vec!["renders/**".into()]),
        ..Default::default()
    };
    let plan = inventory(
        dir.path(),
        &global,
        &local,
        &CompiledPolicy::new(policy()).unwrap(),
    )
    .unwrap();
    assert_eq!(plan.proposed_external_bytes, 0);
    assert!(plan
        .files
        .iter()
        .find(|f| f.path == "renders/frame.png")
        .unwrap()
        .concerns
        .iter()
        .any(|c| c.contains("excluded")));
    assert!(plan
        .files
        .iter()
        .find(|f| f.path == "big.json")
        .unwrap()
        .concerns
        .iter()
        .any(|c| c.contains("staging limit")));
}

#[cfg(unix)]
#[test]
fn symlinks_and_non_utf8_paths_are_not_silently_followed_or_conflated() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let dir = git_fixture();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    symlink("/etc/passwd", dir.path().join("assets/link")).unwrap();
    let odd = std::ffi::OsStr::from_bytes(b"assets/odd-\xff");
    std::fs::write(dir.path().join(odd), [7u8; 30]).unwrap();
    let plan = inventory(
        dir.path(),
        &crate::policy::test_sync_policy(),
        &RepoPolicyOverride::default(),
        &CompiledPolicy::new(policy()).unwrap(),
    )
    .unwrap();
    let link = plan.files.iter().find(|f| f.path == "assets/link").unwrap();
    assert!(link.bytes.is_none());
    assert!(link.concerns.iter().any(|c| c.contains("symlink")));
    let odd = plan
        .files
        .iter()
        .find(|f| f.path_bytes_hex.is_some())
        .unwrap();
    assert!(odd.path_bytes_hex.as_ref().unwrap().ends_with("ff"));
    assert_eq!(plan.proposed_external_bytes, 30);
}

#[test]
fn malformed_repo_storage_policy_is_a_hard_error_for_planning() {
    let dir = git_fixture();
    std::fs::create_dir(dir.path().join(".dracon")).unwrap();
    std::fs::write(
        dir.path().join(".dracon/dracon-sync.toml"),
        "[storage]\nenabled = 'typo'\n",
    )
    .unwrap();
    let operator = dir.path().join("operator.toml");
    std::fs::write(&operator, "").unwrap();
    assert!(load_configuration(dir.path(), Some(&operator)).is_err());
}

#[test]
fn repository_cannot_downgrade_operator_backend_encryption() {
    let global = policy();
    let mut local = StorageOverride {
        rules: Some(global.rules.clone()),
        ..Default::default()
    };
    local.rules.as_mut().unwrap()[1].security = Some(Security::NonSensitive);
    assert!(validate_policy(&effective_policy(&global, Some(&local))).is_err());
    let mut approved = global;
    if let BackendBinding::Local {
        allowed_security, ..
    } = approved.backends.get_mut("archive").unwrap()
    {
        allowed_security.push(Security::NonSensitive);
    }
    assert!(validate_policy(&effective_policy(&approved, Some(&local))).is_ok());
}

#[cfg(unix)]
#[test]
fn repo_policy_symlinks_cannot_read_outside_repository() {
    let dir = git_fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("dracon-sync.toml"), "").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join(".dracon")).unwrap();
    let operator = dir.path().join("operator.toml");
    std::fs::write(&operator, "").unwrap();
    assert!(load_configuration(dir.path(), Some(&operator)).is_err());
}

#[test]
fn history_inventory_counts_unique_reachable_blobs_not_current_tree_bytes() {
    let dir = git_fixture();
    let repo = dir.path();
    std::fs::write(repo.join("version.txt"), b"first-version").unwrap();
    git_fixture_command(repo, &["add", "--", "version.txt"]);
    git_fixture_command(
        repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "-m",
            "first",
        ],
    );
    std::fs::write(repo.join("version.txt"), b"second-version").unwrap();
    git_fixture_command(repo, &["add", "--", "version.txt"]);
    git_fixture_command(
        repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "-m",
            "second",
        ],
    );
    let history = history_inventory(repo).unwrap();
    assert_eq!(history.reachable_blob_count, 2);
    assert_eq!(history.reachable_raw_blob_bytes, 27);
    assert!(history.git_object_database_bytes > 0);
    assert!(history.scope.contains("not push bytes"));
}

#[test]
fn history_inventory_handles_unborn_repository() {
    let dir = git_fixture();
    let history = history_inventory(dir.path()).unwrap();
    assert_eq!(history.reachable_blob_count, 0);
    assert_eq!(history.reachable_raw_blob_bytes, 0);
}

#[test]
fn batched_attribute_inventory_drains_large_pipes_and_keeps_literal_names() {
    let dir = git_fixture();
    std::fs::write(dir.path().join(".gitattributes"), "* filter=prepared\n").unwrap();
    let mut paths = BTreeSet::new();
    for index in 0..3000 {
        paths.insert(PathBuf::from(format!(
            "assets/{index:04}-{}.png",
            "x".repeat(120)
        )));
    }
    paths.insert(PathBuf::from("assets/tab\tand\nnewline.png"));
    let result = inventory_filters(dir.path(), &paths).unwrap();
    assert_eq!(result.len(), paths.len());
    assert!(result.values().all(|filter| filter == "prepared"));
    assert!(result.contains_key(Path::new("assets/tab\tand\nnewline.png")));
}

#[test]
fn parent_inventory_does_not_follow_nested_standalone_repository() {
    let parent = git_fixture();
    let nested = parent.path().join("child");
    std::fs::create_dir(&nested).unwrap();
    git_fixture_command(&nested, &["-c", "init.templateDir=", "init", "--quiet"]);
    std::fs::write(nested.join("asset.png"), [7u8; 30]).unwrap();
    let plan = inventory(
        parent.path(),
        &crate::policy::test_sync_policy(),
        &RepoPolicyOverride::default(),
        &CompiledPolicy::new(policy()).unwrap(),
    )
    .unwrap();
    assert!(!plan
        .files
        .iter()
        .any(|file| file.path.ends_with("asset.png")));
    assert!(plan.files.iter().any(|file| file
        .concerns
        .iter()
        .any(|concern| concern.contains("nested repository"))));
}

#[test]
fn journal_status_is_read_only_and_redacts_source_metadata() {
    use dracon_sync::storage_core::journal::{
        encode_relative_path, Encryption, JobSpec, Journal, Limits,
    };
    use dracon_sync::storage_core::reference::Fingerprint;
    let repo = git_fixture();
    let state = tempfile::tempdir().unwrap();
    let id = "a".repeat(64);
    let report = journal_status(repo.path(), Some(state.path()), Some(&id)).unwrap();
    assert!(!report.summary.initialized);
    assert!(!state.path().join("storage-journal").exists());
    let root = state.path().join("storage-journal");
    let journal = Journal::open(&root, &id, Limits::default()).unwrap();
    journal
        .create(JobSpec {
            repo_id: id.clone(),
            path_hex: encode_relative_path(b"secret-session-name.bin").unwrap(),
            source: Fingerprint::new("b".repeat(64), 200).unwrap(),
            policy_sha256: "c".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into()],
            required_git_targets: vec!["github".into()],
            encryption: Encryption::WardenAge,
        })
        .unwrap();
    let report = journal_status(repo.path(), Some(state.path()), Some(&id)).unwrap();
    assert_eq!(report.summary.records, 1);
    assert_eq!(report.summary.pending_source_bytes, 200);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("secret-session-name"));
    assert!(!json.contains(&"b".repeat(64)));
    assert!(!report.live_backend_verified);
    assert!(!report.transfers_available);
}

#[cfg(unix)]
async fn guarded_fixture() -> tempfile::TempDir {
    use dracon_sync::storage_core::{
        journal::{encode_relative_path, Encryption, Limits},
        manifest::{Enrollment, Manifest},
        metadata::MetadataStore,
        reference::{Fingerprint, Pointer},
        security::WardenAdapter,
    };
    use std::os::unix::fs::PermissionsExt;
    let dir = git_fixture();
    let repo = dir.path();
    let id = "a".repeat(64);
    git_fixture_command(repo, &["config", "dracon.storageRepoId", &id]);
    git_fixture_command(repo, &["config", "user.name", "DraconDev"]);
    git_fixture_command(repo, &["config", "user.email", "dracsharp@gmail.com"]);
    let payload = Fingerprint::new("b".repeat(64), 42).unwrap();
    let manifest = Manifest::new(
        id.clone(),
        vec![Enrollment {
            path_hex: encode_relative_path(b"asset.bin").unwrap(),
            contract_sha256: "c".repeat(64),
            primary: "archive".into(),
            required_copies: vec!["archive".into()],
            encryption: Encryption::None,
            payload: Some(payload.clone()),
        }],
    )
    .unwrap();
    let binary = repo.join(".git/synthetic-warden");
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf 'age-encryption.org/v1\\n'\ncat\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let adapter =
        WardenAdapter::new(&binary, repo, &id, std::time::Duration::from_secs(5)).unwrap();
    let root = repo.join(".git/metadata");
    let store = MetadataStore::open(&root, &id, Limits::default()).unwrap();
    let prepared = store
        .prepare(&manifest, &"c".repeat(64), &adapter, 1)
        .await
        .unwrap();
    use std::io::Read;
    let mut bytes = Vec::new();
    store
        .open_prepared(&prepared)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    std::fs::write(repo.join("assets.manifest"), bytes).unwrap();
    std::fs::write(
        repo.join("asset.bin"),
        Pointer::new(payload).unwrap().encode(),
    )
    .unwrap();
    std::fs::write(
        repo.join(".gitattributes"),
        "*.bin filter=dracon-storage -text -ident\n",
    )
    .unwrap();
    git_fixture_command(repo, &["config", "filter.dracon-storage.clean", "cat"]);
    git_fixture_command(repo, &["config", "filter.dracon-storage.required", "true"]);
    git_fixture_command(
        repo,
        &[
            "add",
            "--",
            "assets.manifest",
            "asset.bin",
            ".gitattributes",
        ],
    );
    dir
}

#[cfg(unix)]
#[tokio::test]
async fn configured_storage_commits_verified_tree_and_preserves_rejected_index() {
    let dir = guarded_fixture().await;
    let repo = dir.path();
    let id = "a".repeat(64);
    let root = repo.join(".git/metadata");
    assert!(commit_configured_storage(repo, "unbound").is_err());
    setup_guard(repo, &id, &root, Path::new("assets.manifest")).unwrap();
    setup_guard(repo, &id, &root, Path::new("assets.manifest")).unwrap();
    assert!(verify_configured_index(repo, false).unwrap());
    let initial_index = std::fs::read(repo.join(".git/index")).unwrap();
    assert!(commit_configured_storage(repo, "guarded root").unwrap());
    let repository = git2::Repository::open(repo).unwrap();
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 0);
    assert_eq!(head.author().name().unwrap(), "DraconDev");
    let pointer = git_fixture_command(repo, &["show", "HEAD:asset.bin"]);
    assert_eq!(pointer, std::fs::read(repo.join("asset.bin")).unwrap());
    assert_eq!(
        std::fs::read(repo.join(".git/index")).unwrap(),
        initial_index
    );
    assert!(!repo.join(".git/index.lock").exists());
    // libgit2 callers bypass hooks: rejection must happen within the commit API.
    std::fs::write(repo.join("asset.bin"), b"raw accidental asset").unwrap();
    git_fixture_command(repo, &["add", "--", "asset.bin"]);
    let rejected_index = std::fs::read(repo.join(".git/index")).unwrap();
    assert!(commit_configured_storage(repo, "must reject raw bytes").is_err());
    assert_eq!(repository.head().unwrap().target(), Some(head.id()));
    assert_eq!(
        std::fs::read(repo.join(".git/index")).unwrap(),
        rejected_index
    );
    assert_eq!(
        std::fs::read(repo.join("asset.bin")).unwrap(),
        b"raw accidental asset"
    );
    assert!(!repo.join(".git/index.lock").exists());
}

#[test]
fn ordinary_repo_has_no_storage_commit_or_guard_requirement() {
    let dir = git_fixture();
    assert!(!verify_configured_index(dir.path(), false).unwrap());
    assert!(!commit_configured_storage(dir.path(), "ordinary").unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn cold_clone_and_staged_attribute_removal_cannot_disable_storage_guard() {
    let source = guarded_fixture().await;
    setup_guard(source.path(), &"a".repeat(64), &source.path().join(".git/metadata"), Path::new("assets.manifest")).unwrap();
    assert!(commit_configured_storage(source.path(), "storage source").unwrap());
    let destination = tempfile::tempdir().unwrap();
    let clone = destination.path().join("cold");
    let repository = git2::Repository::clone(source.path().to_str().unwrap(), &clone).unwrap();
    let local = repository.config().unwrap().open_level(git2::ConfigLevel::Local).unwrap();
    assert!(local.get_entry(GUARD_VERSION_KEY).is_err());
    assert!(local.get_entry("filter.dracon-storage.clean").is_err());
    let head = repository.head().unwrap().target();
    let index = std::fs::read(repository.path().join("index")).unwrap();
    assert!(verify_configured_index(&clone, false).is_err());
    assert!(commit_configured_storage(&clone, "unbound cold clone").is_err());
    assert_eq!(std::fs::read(repository.path().join("index")).unwrap(), index);
    // Removing both declarations and the working pointer cannot erase HEAD's
    // preservation contract or permit a new raw-file commit.
    git_fixture_command(&clone, &["update-index", "--force-remove", "--", ".gitattributes", "asset.bin"]);
    std::fs::write(clone.join("asset.bin"), b"hydrated bytes must stay out of Git").unwrap();
    assert!(verify_configured_index(&clone, false).is_err());
    assert!(commit_configured_storage(&clone, "removed attributes").is_err());
    assert_eq!(repository.head().unwrap().target(), head);
    assert_eq!(std::fs::read(clone.join("asset.bin")).unwrap(), b"hydrated bytes must stay out of Git");
    assert!(!repository.path().join("index.lock").exists());
}

#[test]
fn staged_nested_macro_and_quoted_storage_declarations_require_binding() {
    for declaration in [
        &b"[attr]preserved filter=dracon-storage -text\n*.bin preserved\n"[..],
        &b"\"private \\"name\\".bin\" filter=dracon-storage\r\n"[..],
        &b"private-\xff.bin filter=dracon-storage\n"[..],
        &b"*.absent filter=dracon-storage\n"[..],
    ] {
        let dir = git_fixture();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/.gitattributes"), declaration).unwrap();
        git_fixture_command(dir.path(), &["add", "--", "nested/.gitattributes"]);
        assert!(verify_configured_index(dir.path(), false).is_err());
        assert!(commit_configured_storage(dir.path(), "unbound declaration").is_err());
    }
}

#[test]
fn comments_and_storage_named_patterns_do_not_activate_guard() {
    let dir = git_fixture();
    std::fs::write(dir.path().join(".gitattributes"), b"# *.bin filter=dracon-storage\n  # example filter=dracon-storage\n\"filter=dracon-storage\" -text\n*.bin filter=other\n").unwrap();
    git_fixture_command(dir.path(), &["add", "--", ".gitattributes"]);
    assert!(!verify_configured_index(dir.path(), false).unwrap());
    assert!(!commit_configured_storage(dir.path(), "ordinary attributes").unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn storage_bootstrap_cannot_bypass_guard_with_no_verify() {
    for valid in [true, false] {
        let dir = guarded_fixture().await;
        let repo = dir.path();
        setup_guard(
            repo,
            &"a".repeat(64),
            &repo.join(".git/metadata"),
            Path::new("assets.manifest"),
        )
        .unwrap();
        if !valid {
            std::fs::write(repo.join("asset.bin"), b"unverified raw bytes").unwrap();
            git_fixture_command(repo, &["add", "--", "asset.bin"]);
        }
        // Bootstrap has work to stage in addition to the previously staged pair.
        std::fs::write(repo.join("README.md"), "bootstrap fixture\n").unwrap();
        let policy: crate::policy::SyncPolicy = toml::from_str(
            r#"
auto_commit = true
auto_push = false
auto_pull = false
auto_bump_versions = false
trusted_emails = ["dracsharp@gmail.com"]
trusted_authors = ["DraconDev"]
"#,
        )
        .unwrap();
        let result =
            crate::sync::bootstrap_empty_repo_commit(repo, &policy, &BTreeSet::new(), false)
                .await
                .unwrap();
        assert_eq!(result, valid);
        let repository = git2::Repository::open(repo).unwrap();
        assert_eq!(repository.head().is_ok(), valid);
        assert_eq!(
            std::fs::read(repo.join("README.md")).unwrap(),
            b"bootstrap fixture\n"
        );
        if !valid {
            assert_eq!(
                std::fs::read(repo.join("asset.bin")).unwrap(),
                b"unverified raw bytes"
            );
        }
        assert!(!repo.join(".git/index.lock").exists());
    }
}

#[test]
fn attribute_response_rejects_duplicates_excess_fields_and_large_values() {
    let paths = BTreeSet::from([PathBuf::from("a.bin"), PathBuf::from("b.bin")]);
    let good = b"a.bin\0filter\0dracon-storage\0b.bin\0filter\0unspecified\0";
    assert_eq!(parse_attribute_response(good, &paths).unwrap().len(), 2);
    for response in [
        &b"a.bin\0filter\0dracon-storage"[..],
        &b"a.bin\0filter\0dracon-storage\0a.bin\0filter\0unspecified\0"[..],
        &b"other.bin\0filter\0dracon-storage\0b.bin\0filter\0unspecified\0"[..],
        &b"a.bin\0other\0dracon-storage\0b.bin\0filter\0unspecified\0"[..],
    ] {
        assert!(parse_attribute_response(response, &paths).is_err());
    }
    let mut excess = good.to_vec();
    excess.extend_from_slice(b"c.bin\0filter\0unspecified\0");
    assert!(parse_attribute_response(&excess, &paths).is_err());
    let mut oversized = b"a.bin\0filter\0".to_vec();
    oversized.extend_from_slice(&[b'x'; 1025]);
    oversized.extend_from_slice(b"\0b.bin\0filter\0unspecified\0");
    assert!(parse_attribute_response(&oversized, &paths).is_err());
    assert!(parse_attribute_response(&[], &BTreeSet::new())
        .unwrap()
        .is_empty());
}

#[test]
fn real_attribute_query_drains_full_pipes_and_enforces_input_budget() {
    let dir = git_fixture();
    std::fs::write(
        dir.path().join(".gitattributes"),
        "*.bin filter=dracon-storage\n",
    )
    .unwrap();
    let paths = (0..12_000)
        .map(|n| PathBuf::from(format!("file-{n:05}.bin")))
        .collect();
    let filters = inventory_filters(dir.path(), &paths).unwrap();
    assert_eq!(filters.len(), 12_000);
    assert!(filters.values().all(|value| value == "dracon-storage"));
    let oversized = BTreeSet::from([PathBuf::from("x".repeat(16 * 1024 * 1024))]);
    let error = inventory_filters(dir.path(), &oversized).unwrap_err();
    assert!(error.to_string().contains("input budget"));
    let too_many = (0..100_001)
        .map(|n| PathBuf::from(format!("p{n}")))
        .collect();
    let error = inventory_filters(dir.path(), &too_many).unwrap_err();
    assert!(error.to_string().contains("path budget"));
}

#[cfg(unix)]
fn attribute_test_command(script: &str, repo: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("sh");
    command
        .current_dir(repo)
        .args(["-c", script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    command
}

#[cfg(unix)]
#[test]
fn attribute_query_caps_output_and_deadlines_blocked_input() {
    let temp = tempfile::tempdir().unwrap();
    let flooding =
        attribute_test_command("while :; do printf 'oversized-output'; done", temp.path());
    let error = bounded_attribute_query(
        flooding,
        Vec::new(),
        1024,
        std::time::Duration::from_secs(2),
    )
    .unwrap_err();
    assert!(error.to_string().contains("output budget"), "{error}");
    let blocked = attribute_test_command("exec sleep 60", temp.path());
    let started = std::time::Instant::now();
    let error = bounded_attribute_query(
        blocked,
        vec![b'x'; 1024 * 1024],
        1024,
        std::time::Duration::from_millis(250),
    )
    .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[cfg(target_os = "linux")]
#[test]
fn attribute_query_kills_descendants_holding_stdout_after_parent_exit() {
    let temp = tempfile::tempdir().unwrap();
    let command = attribute_test_command(
        "sh -c 'echo $$ > descendant.pid; exec sleep 60' & exit 0",
        temp.path(),
    );
    let error =
        bounded_attribute_query(command, Vec::new(), 1024, std::time::Duration::from_secs(1))
            .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    let pid = std::fs::read_to_string(temp.path().join("descendant.pid")).unwrap();
    // SIGKILL delivery/reaping is asynchronous; wait briefly for death rather
    // than assuming the child has received its signal before this thread runs.
    for _ in 0..100 {
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()));
        if stat.is_err() || stat.unwrap().split_once(") ").unwrap().1.starts_with('Z') {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("attribute-query descendant survived cancellation");
}
