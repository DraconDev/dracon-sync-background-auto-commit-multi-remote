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
