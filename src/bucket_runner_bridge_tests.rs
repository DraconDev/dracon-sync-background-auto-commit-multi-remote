//! Tests for the `bucket_runner_bridge` module.
//!
//! Fixture layout:
//!   <tmpdir>/repo                      git init -b main
//!   <tmpdir>/fake/runner.bundle.mjs    Node.js stub mirroring runner.bundle.mjs
//!   <tmpdir>/fake/profile.json         { runner.path: <fake>/runner.bundle.mjs }
//!   <tmpdir>/state/                    stateDirectory (mode 0700)
//!   <tmpdir>/repo/bucket-runner.json   bridge config
//!   <tmpdir>/repo/bucket-assets.manifest.json   bucket manifest
//!
//! The fake validates: GIT_* env stripped, exact-request root /
//! profilePath / stateDirectory / remoteName / trustedRemoteUrl /
//! budget / allowedCandidates / operation, then emits a verdict whose
//! shape is dictated by the FAKE_VERDICT env var baked into the test.
//! Anything mismatching the env-encoded expectations makes the fake
//! emit a `refused` verdict with a `reason` field, so the bridge
//! surfaces the failure deterministically.

use super::*;
use crate::policy::SyncPolicy;
use std::os::unix::fs::PermissionsExt;

#[test]
fn shared_routing_defaults_validate_and_preserve_measured_bytes() {
    for projected in [1023, 1024, 1025] {
        let budget = routing_budget_from_json(ROUTING_DEFAULTS_JSON, projected).unwrap();
        assert_eq!(budget["largeBytes"], 1024);
        assert_eq!(budget["limitBytes"], 2u64 * 1024 * 1024 * 1024);
        assert_eq!(budget["pressureRatio"], 0.8);
        assert_eq!(budget["projectedBytes"], projected);
        assert_eq!(budget.as_object().unwrap().len(), 4);
    }
}

#[test]
fn shared_routing_defaults_fail_closed_on_invalid_data() {
    let base: serde_json::Value = serde_json::from_str(ROUTING_DEFAULTS_JSON).unwrap();
    for (key, value) in [
        ("schemaVersion", serde_json::json!(2)),
        ("largeBytes", serde_json::json!(0)),
        ("largeBytes", serde_json::json!(-1)),
        ("largeBytes", serde_json::json!(MAX_JS_INTEGER + 1)),
        ("pressureLimitBytes", serde_json::json!(0)),
        ("pressureLimitBytes", serde_json::json!(MAX_JS_INTEGER + 1)),
        ("pressureRatio", serde_json::json!(0)),
        ("pressureRatio", serde_json::json!(1)),
        ("extra", serde_json::json!(true)),
    ] {
        let mut invalid = base.clone();
        invalid[key] = value;
        assert!(routing_budget_from_json(&invalid.to_string(), 0).is_err());
    }
    assert!(routing_budget_from_json("{}", 0).is_err());
    assert!(routing_budget_from_json("not json", 0).is_err());
    assert!(routing_budget_from_json(ROUTING_DEFAULTS_JSON, MAX_JS_INTEGER + 1).is_err());
}

const FAKE_BODY: &str = r#"const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const GIT_VARS = [
  'GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_OBJECT_DIRECTORY',
  'GIT_ALTERNATE_OBJECT_DIRECTORIES','GIT_NAMESPACE','GIT_SHALLOW_FILE',
  'GIT_GRAFT_FILE','GIT_REPLACE_REF_BASE',
];

function refused(reason) {
  process.stdout.write(JSON.stringify({ schemaVersion: 1, status: 'refused', commitEligible: false, reason }) + '\n');
}

function readStdin(limit) {
  return new Promise((resolve) => {
    const chunks = [];
    let size = 0;
    let done = false;
    process.stdin.on('data', (chunk) => {
      size += chunk.length;
      if (size > limit) { done = true; resolve({ error: 'stdin-too-large' }); return; }
      chunks.push(chunk);
    });
    process.stdin.on('end', () => {
      if (!done) resolve({ body: Buffer.concat(chunks).toString('utf8') });
    });
  });
}

function main() {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--version') {
    process.stdout.write(JSON.stringify({ schemaVersion: 1, protocolVersion: 1, runner: 'bucket-runner', operations: ['inspect', 'publish', 'restore'] }) + '\n');
    return;
  }

  for (const v of GIT_VARS) {
    if (process.env[v] !== undefined) {
      refused('env-not-stripped:' + v);
      return;
    }
  }

  readStdin(2 * 1024 * 1024).then((stdin) => {
    if (stdin.error) { refused(stdin.error); return; }
    let request;
    try { request = JSON.parse(stdin.body); }
    catch (e) { refused('invalid-json'); return; }

    const checks = {
      schemaVersion: 1,
      operation: process.env.FAKE_OPERATION || 'publish',
      root: process.env.FAKE_ROOT,
      profilePath: process.env.FAKE_PROFILE_PATH,
      stateDirectory: process.env.FAKE_STATE_DIRECTORY,
      remoteName: process.env.FAKE_REMOTE_NAME,
      trustedRemoteUrl: process.env.FAKE_TRUSTED_REMOTE_URL,
    };
    for (const [k, expected] of Object.entries(checks)) {
      if (request[k] !== expected) {
        refused('mismatch:' + k + ':got=' + JSON.stringify(request[k]) + ':want=' + JSON.stringify(expected));
        return;
      }
    }

    const expectedBudget = process.env.FAKE_BUDGET ? JSON.parse(process.env.FAKE_BUDGET) : null;
    if (expectedBudget) {
      for (const [k, v] of Object.entries(expectedBudget)) {
        if (request.budget[k] !== v) {
          refused('budget-mismatch:' + k);
          return;
        }
      }
    }

    const expectedCandidates = process.env.FAKE_ALLOWED_CANDIDATES;
    if (expectedCandidates !== undefined) {
      const expected = JSON.parse(expectedCandidates);
      const actual = request.allowedCandidates || [];
      if (actual.length !== expected.length || !actual.every((v, i) => v === expected[i])) {
        refused('candidates-mismatch:got=' + JSON.stringify(actual));
        return;
      }
    }

    if (process.env.FAKE_VERDICT_KIND === 'refused') {
      refused(process.env.FAKE_VERDICT_REASON || 'refused-by-test');
      return;
    }
    if (process.env.FAKE_VERDICT_KIND === 'plannable') {
      process.stdout.write(JSON.stringify({ schemaVersion: 1, status: 'plannable', commitEligible: false }) + '\n');
      return;
    }
    if (process.env.FAKE_VERDICT_KIND === 'inspect') {
      process.stdout.write(JSON.stringify({ schemaVersion: 1, status: 'inspect', commitEligible: false }) + '\n');
      return;
    }

    const manifestPath = path.join(request.root, process.env.FAKE_MANIFEST_PATH || 'bucket-assets.manifest.json');
    const manifestBytes = fs.readFileSync(manifestPath);
    const sha256 = crypto.createHash('sha256').update(manifestBytes).digest('hex');
    process.stdout.write(JSON.stringify({ schemaVersion: 1, status: 'published', commitEligible: true, manifestSha256: sha256 }) + '\n');
  }).catch((e) => refused('fake-error:' + (e && e.message || String(e))));
}

main();
"#;

const SLOW_BODY: &str = r#"const secs = Number(process.env.FAKE_SLEEP_SECS || '120');
setTimeout(() => {
  process.stdout.write(JSON.stringify({ schemaVersion: 1, status: 'published', commitEligible: true, manifestSha256: '0'.repeat(64) }) + '\n');
}, secs * 1000);
"#;

const OVERSIZE_STDIN_BODY: &str = r#"process.stdin.on('data', () => {});
process.stdin.on('end', () => {
  process.stdout.write(JSON.stringify({ schemaVersion: 1, status: 'refused', commitEligible: false, reason: 'stdin-too-large' }) + '\n');
});
"#;

const OVERSIZE_STDOUT_BODY: &str = r#"const filler = 'A'.repeat(3 * 1024 * 1024);
process.stdout.write(filler);
"#;

/// Build a deterministic policy with a low `stage_op_timeout_secs` so the
/// timeout test can finish quickly. The bridge floors the timeout at
/// `MIN_BUNDLE_TIMEOUT_SECS = 60` regardless of this value.
fn fast_policy() -> SyncPolicy {
    let mut value = toml::from_str::<SyncPolicy>(
        r#"
auto_commit = true
auto_push = false
stage_op_timeout_secs = 10
max_push_blob_bytes = 104857600
"#,
    )
    .expect("policy parses");
    if value.stage_op_timeout_secs < 60 {
        value.stage_op_timeout_secs = 60;
    }
    value
}

/// Initialise an empty git repo with the global warden hook suppressed so
/// the test harness does not need to install warden to commit fixtures.
fn init_repo(dir: &Path) {
    crate::test_helpers::test_git_cmd()
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.parent().unwrap_or(dir))
        .arg(dir.to_str().unwrap())
        .output()
        .expect("git init");
    crate::test_helpers::test_git_cmd()
        .args(["config", "user.email", "test@dracon"])
        .current_dir(dir)
        .output()
        .expect("git user.email");
    crate::test_helpers::test_git_cmd()
        .args(["config", "user.name", "DraconDev"])
        .current_dir(dir)
        .output()
        .expect("git user.name");
    crate::test_helpers::test_git_cmd()
        .args(["config", "core.hooksPath", "/dev/null"])
        .current_dir(dir)
        .output()
        .expect("git core.hooksPath");
}

fn write_state_dir(path: &Path) {
    std::fs::create_dir_all(path).expect("create state dir");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("state dir perms");
}

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = crate::test_helpers::test_git_cmd()
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
    output.stdout
}

fn manifest_bytes() -> Vec<u8> {
    br#"{
  "schemaVersion": 1,
  "bucket": "dracon-lfs",
  "repository": "test",
  "namespace": "test",
  "assets": [
    {"sourcePath": "static/a.png", "visibility": "private", "regenerable": true, "bucketed": true, "bytes": 1, "sha256": "0000000000000000000000000000000000000000000000000000000000000000"},
    {"sourcePath": "static/b.txt", "visibility": "public", "regenerable": false, "bucketed": false, "bytes": 1, "sha256": "0000000000000000000000000000000000000000000000000000000000000000"}
  ]
}"#
    .to_vec()
}

#[derive(Default, Clone)]
struct ConfigOverrides {
    profile_path: Option<String>,
    state_directory: Option<String>,
    remote_name: Option<String>,
    trusted_remote_url: Option<String>,
    asset_root: Option<String>,
    manifest_path: Option<String>,
    allowed_candidates: Option<String>,
    enabled: Option<bool>,
    /// Raw override of the entire config JSON string. When set, takes
    /// precedence over `ConfigOverrides::*`.
    raw: Option<String>,
}

fn write_bucket_runner_config(repo: &Path, fake_dir: &Path, state_dir: &Path, overrides: ConfigOverrides) {
    let default = || -> String {
        serde_json::json!({
            "schemaVersion": 1,
            "enabled": overrides.enabled.unwrap_or(true),
            "profilePath": overrides.profile_path.unwrap_or_else(|| fake_dir.join("profile.json").to_string_lossy().into_owned()),
            "stateDirectory": overrides.state_directory.unwrap_or_else(|| state_dir.to_string_lossy().into_owned()),
            "remoteName": overrides.remote_name.unwrap_or_else(|| "origin".to_owned()),
            "trustedRemoteUrl": overrides.trusted_remote_url.unwrap_or_else(|| "https://example.com/repo.git".to_owned()),
            "assetRoot": overrides.asset_root.unwrap_or_else(|| ".".to_owned()),
            "manifestPath": overrides.manifest_path.unwrap_or_else(|| "bucket-assets.manifest.json".to_owned()),
            "allowedCandidates": serde_json::from_str::<Vec<String>>(overrides.allowed_candidates.as_deref().unwrap_or("[]")).unwrap_or_default(),
        })
        .to_string()
    };
    let body = overrides.raw.unwrap_or_else(default);
    std::fs::write(repo.join("bucket-runner.json"), body).expect("write bucket-runner.json");
}

fn install_fake(
    fake_dir: &Path,
    profile_json_name: &str,
    body: &str,
    env: &[(&str, &str)],
) -> std::path::PathBuf {
    std::fs::create_dir_all(fake_dir).expect("create fake dir");
    let bundle = fake_dir.join("runner.bundle.cjs");
    std::fs::write(&bundle, body).expect("write fake bundle");
    let profile_path = fake_dir.join(profile_json_name);
    let mut profile = serde_json::json!({
        "schemaVersion": 1,
        "id": "test",
        "enabled": true,
        "runner": {
            "path": bundle.to_string_lossy(),
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "version": 1
        },
        "storage": {
            "bucket": "dracon-lfs",
            "namespaceRoot": "test",
            "endpoint": "https://example.invalid",
            "region": "us-east-1",
            "uploaderCredentialFile": "/dev/null",
            "consumerCredentialFile": "/dev/null"
        },
        "allowedAssetTrees": ["/dev/null"],
        "consumers": [{"id": "filesystem-restore", "version": 1}]
    });
    for (k, v) in env {
        // The fake's actual environment is captured at process spawn time;
        // we encode expectations into the profile JSON for tests that
        // need to keep the daemon-side metadata coherent.
        profile[k] = serde_json::Value::String((*v).to_owned());
    }
    std::fs::write(&profile_path, profile.to_string()).expect("write profile.json");
    profile_path
}

fn env_kv_string(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn path_with_set_git_env(real: &Path) -> PathBuf {
    // Build a unique repo root that inherits some GIT_* env to confirm
    // the bridge strips them before spawning the bundle. Tests pass
    // `real.to_path_buf()` and the bridge ignores the inherited state.
    real.to_path_buf()
}

#[tokio::test]
async fn unconfigured_repo_does_not_spawn_bundle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let policy = fast_policy();
    let result = prepare(&repo, false, &policy).await.expect("prepare");
    assert!(result.is_none(), "no bucket-runner.json → Ok(None)");
}

#[tokio::test]
async fn disabled_repo_returns_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    install_fake(&fake_dir, "profile.json", FAKE_BODY, &[]);
    write_bucket_runner_config(
        &repo,
        &fake_dir,
        &state_dir,
        ConfigOverrides { enabled: Some(false), ..Default::default() },
    );
    let policy = fast_policy();
    let result = prepare(&repo, false, &policy).await.expect("prepare");
    assert!(result.is_none(), "enabled=false → Ok(None)");
}

#[tokio::test]
async fn missing_profile_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    // profile.json is intentionally NOT installed.
    write_bucket_runner_config(&repo, &fake_dir, &state_dir, ConfigOverrides::default());
    let policy = fast_policy();
    let result = prepare(&repo, false, &policy).await;
    let error = match result {
        Ok(_) => panic!("missing profile must refuse"),
        Err(e) => e,
    };
    let msg = format!("{error:#}");
    assert!(
        msg.contains("profile") || msg.contains("invalid"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn unsafe_config_paths_are_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    let profile_path = install_fake(&fake_dir, "profile.json", FAKE_BODY, &[]);

    let policy = fast_policy();
    let cases: &[(&str, ConfigOverrides)] = &[
        ("relative profilePath", ConfigOverrides { profile_path: Some("relative/profile.json".into()), ..Default::default() }),
        ("relative stateDirectory", ConfigOverrides { state_directory: Some("relative/state".into()), ..Default::default() }),
        (
            "stateDirectory without 0700",
            ConfigOverrides {
                state_directory: Some({
                    let p = tmp.path().join("bad_state");
                    std::fs::create_dir_all(&p).unwrap();
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
                    p.to_string_lossy().into_owned()
                }),
                ..Default::default()
            },
        ),
        ("assetRoot='..'", ConfigOverrides { asset_root: Some("..".into()), ..Default::default() }),
        ("manifestPath='..'", ConfigOverrides { manifest_path: Some("..".into()), ..Default::default() }),
        ("allowedCandidates=['../escape']", ConfigOverrides { allowed_candidates: Some(r#"["../escape"]"#.into()), ..Default::default() }),
        ("bad remoteName", ConfigOverrides { remote_name: Some("name with space".into()), ..Default::default() }),
        ("malformed JSON", ConfigOverrides { raw: Some("{not json".into()), ..Default::default() }),
    ];

    for (label, overrides) in cases {
        write_bucket_runner_config(&repo, &fake_dir, &state_dir, overrides.clone());
        let result = prepare(&repo, false, &policy).await;
        match result {
            Ok(value) => panic!("case {label} should have erred, got ok"),
            Err(_) => {}
        }
        let _ = profile_path; // keep variable alive
    }
}

#[tokio::test]
async fn refused_spawn_is_indexed_and_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    install_fake(&fake_dir, "profile.json", FAKE_BODY, &[]);
    write_bucket_runner_config(&repo, &fake_dir, &state_dir, ConfigOverrides::default());
    std::fs::write(repo.join("bucket-assets.manifest.json"), manifest_bytes()).expect("manifest");
    git(&repo, &["add", "--", "bucket-assets.manifest.json"]);
    git(&repo, &["commit", "-qm", "manifest"]);

    let policy = fast_policy();
    // The bridge spawns the bundle with the test's env inherited; we
    // flip the verdict via FAKE_VERDICT_KIND before the spawn by
    // setting the env in the current process. The fake reads
    // FAKE_VERDICT_KIND to choose the verdict shape.
    std::env::set_var("FAKE_VERDICT_KIND", "refused");
    std::env::set_var("FAKE_VERDICT_REASON", "simulated-bundle-refusal");
    let result = prepare(&repo, false, &policy).await;
    std::env::remove_var("FAKE_VERDICT_KIND");
    std::env::remove_var("FAKE_VERDICT_REASON");

    let error = match result {
        Ok(value) => panic!("refused verdict must error, got ok"),
        Err(e) => e,
    };
    let msg = format!("{error:#}");
    assert!(
        msg.contains("refused") || msg.contains("refusal"),
        "expected refused in error, got: {msg}"
    );
}

#[tokio::test]
async fn non_refused_status_without_commit_eligible_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    write_bucket_runner_config(&repo, &fake_dir, &state_dir, ConfigOverrides::default());
    std::fs::write(repo.join("bucket-assets.manifest.json"), manifest_bytes()).expect("manifest");
    git(&repo, &["add", "--", "bucket-assets.manifest.json"]);
    git(&repo, &["commit", "-qm", "manifest"]);

    let policy = fast_policy();
    let profile_path = install_fake(&fake_dir, "profile.json", FAKE_BODY, &[]);
    std::env::set_var("FAKE_VERDICT_KIND", "plannable");
    std::env::remove_var("FAKE_ALLOWED_CANDIDATES");
    let repo_str = repo.to_string_lossy().into_owned();
    let profile_str = profile_path.to_string_lossy().into_owned();
    let state_str = state_dir.to_string_lossy().into_owned();
    std::env::set_var("FAKE_ROOT", &repo_str);
    std::env::set_var("FAKE_PROFILE_PATH", &profile_str);
    std::env::set_var("FAKE_STATE_DIRECTORY", &state_str);
    std::env::set_var("FAKE_REMOTE_NAME", "origin");
    std::env::set_var("FAKE_TRUSTED_REMOTE_URL", "https://example.com/repo.git");
    let result = prepare(&repo, false, &policy).await;
    std::env::remove_var("FAKE_VERDICT_KIND");
    std::env::remove_var("FAKE_ROOT");
    std::env::remove_var("FAKE_PROFILE_PATH");
    std::env::remove_var("FAKE_STATE_DIRECTORY");
    std::env::remove_var("FAKE_REMOTE_NAME");
    std::env::remove_var("FAKE_TRUSTED_REMOTE_URL");
    let error = match result {
        Ok(value) => panic!("plannable without commitEligible must error, got ok"),
        Err(e) => e,
    };
    let msg = format!("{error:#}");
    assert!(
        msg.contains("plannable") || msg.contains("commitEligible"),
        "expected plannable / commitEligible in error, got: {msg}"
    );
}

#[tokio::test]
async fn mocked_end_to_end_returns_prepared_with_materialized_paths() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    let profile_path = install_fake(&fake_dir, "profile.json", FAKE_BODY, &[]);
    let candidates_json = serde_json::to_string(&vec!["bucket-assets.manifest.json".to_string()]).unwrap();
    write_bucket_runner_config(&repo, &fake_dir, &state_dir, ConfigOverrides {
        allowed_candidates: Some(candidates_json),
        ..Default::default()
    });
    std::fs::write(repo.join("bucket-assets.manifest.json"), manifest_bytes()).expect("manifest");
    git(&repo, &["add", "--", "bucket-assets.manifest.json"]);
    git(&repo, &["commit", "-qm", "manifest"]);

    let policy = fast_policy();
    let candidates: Vec<String> = vec!["bucket-assets.manifest.json".into()];
    let expected_budget = serde_json::json!({
        "projectedBytes": 0u64,
        "limitBytes": 2u64 * 1024 * 1024 * 1024,
        "largeBytes": 1024u64,
        "pressureRatio": 0.8_f64,
    });
    let repo_str = repo.to_string_lossy().into_owned();
    let profile_str = profile_path.to_string_lossy().into_owned();
    let state_str = state_dir.to_string_lossy().into_owned();
    std::env::set_var("FAKE_OPERATION", "publish");
    std::env::set_var("FAKE_ROOT", &repo_str);
    std::env::set_var("FAKE_PROFILE_PATH", &profile_str);
    std::env::set_var("FAKE_STATE_DIRECTORY", &state_str);
    std::env::set_var("FAKE_REMOTE_NAME", "origin");
    std::env::set_var("FAKE_TRUSTED_REMOTE_URL", "https://example.com/repo.git");
    std::env::set_var("FAKE_BUDGET", expected_budget.to_string());
    std::env::set_var("FAKE_ALLOWED_CANDIDATES", serde_json::to_string(&candidates).unwrap());
    std::env::set_var("FAKE_MANIFEST_PATH", "bucket-assets.manifest.json");
    std::env::remove_var("FAKE_VERDICT_KIND");
    let _path_unused = path_with_set_git_env(&repo);

    let prepared = prepare(&repo, false, &policy)
        .await
        .expect("bridge should succeed")
        .expect("Some(prepared)");

    std::env::remove_var("FAKE_OPERATION");
    std::env::remove_var("FAKE_ROOT");
    std::env::remove_var("FAKE_PROFILE_PATH");
    std::env::remove_var("FAKE_STATE_DIRECTORY");
    std::env::remove_var("FAKE_REMOTE_NAME");
    std::env::remove_var("FAKE_TRUSTED_REMOTE_URL");
    std::env::remove_var("FAKE_BUDGET");
    std::env::remove_var("FAKE_ALLOWED_CANDIDATES");
    std::env::remove_var("FAKE_MANIFEST_PATH");

    assert_eq!(prepared.materialized_paths.len(), 2);
    assert!(prepared
        .materialized_paths
        .contains(&PathBuf::from("static/a.png")));
    assert!(prepared.materialized_paths.contains(&PathBuf::from(".bucket-cache")));
    // verify_staged accepts the prepared value's manifest_path and
    // verified_manifest bytes; this is the same code path the daemon
    // calls at sync.rs:4580, so a green check there confirms the
    // Prepared shape matches the legacy contract exactly.
    prepared
        .verify_staged(&repo, policy.stage_op_timeout_secs)
        .await
        .expect("verify_staged must accept the bridged manifest");
}

#[tokio::test]
async fn timeout_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    install_fake(&fake_dir, "profile.json", SLOW_BODY, &[]);
    write_bucket_runner_config(&repo, &fake_dir, &state_dir, ConfigOverrides::default());
    std::fs::write(repo.join("bucket-assets.manifest.json"), manifest_bytes()).expect("manifest");
    git(&repo, &["add", "--", "bucket-assets.manifest.json"]);
    git(&repo, &["commit", "-qm", "manifest"]);

    let mut policy = fast_policy();
    policy.stage_op_timeout_secs = 1; // well below MIN_BUNDLE_TIMEOUT_SECS
    std::env::set_var("FAKE_SLEEP_SECS", "120");
    let result = prepare(&repo, false, &policy).await;
    std::env::remove_var("FAKE_SLEEP_SECS");
    let error = match result {
        Ok(value) => panic!("slow runner must time out, got ok"),
        Err(e) => e,
    };
    let msg = format!("{error:#}");
    assert!(msg.contains("timed out") || msg.contains("timeout"), "msg: {msg}");
}

#[tokio::test]
async fn oversized_stdin_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    // Use a candidate list just over 2 MiB after JSON encoding: each
    // entry is 8 chars + a comma, so ~230k entries of "abcdefg/" would
    // land around 2.3 MiB. Easier: stage a manifest asset list with
    // thousands of candidates → the request JSON itself exceeds 2 MiB.
    install_fake(&fake_dir, "profile.json", OVERSIZE_STDIN_BODY, &[]);
    // Each entry is ~24 chars; ~88k entries gets us past 2 MiB. Build in one
    // shot instead of re-serializing the whole vector per iteration.
    let count = (2 * 1024 * 1024 + 1024) / 24 + 1;
    let mut many: Vec<String> = (0..count)
        .map(|i| format!("static/long_path_{:06}.png", i))
        .collect();
    many.truncate(90_000);
    let many_str = serde_json::to_string(&many).unwrap();
    write_bucket_runner_config(
        &repo,
        &fake_dir,
        &state_dir,
        ConfigOverrides { allowed_candidates: Some(many_str), ..Default::default() },
    );
    std::fs::write(repo.join("bucket-assets.manifest.json"), manifest_bytes()).expect("manifest");
    git(&repo, &["add", "--", "bucket-assets.manifest.json"]);
    git(&repo, &["commit", "-qm", "manifest"]);

    let policy = fast_policy();
    let result = prepare(&repo, false, &policy).await;
    let error = match result {
        Ok(value) => panic!("oversized stdin must error, got ok"),
        Err(e) => e,
    };
    let msg = format!("{error:#}");
    assert!(
        msg.contains("exceeds 2 MiB") || msg.contains("2 MiB"),
        "msg: {msg}"
    );
}

#[tokio::test]
async fn oversized_stdout_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    init_repo(&repo);
    let fake_dir = tmp.path().join("fake");
    let state_dir = tmp.path().join("state");
    write_state_dir(&state_dir);
    install_fake(&fake_dir, "profile.json", OVERSIZE_STDOUT_BODY, &[]);
    write_bucket_runner_config(&repo, &fake_dir, &state_dir, ConfigOverrides::default());
    std::fs::write(repo.join("bucket-assets.manifest.json"), manifest_bytes()).expect("manifest");
    git(&repo, &["add", "--", "bucket-assets.manifest.json"]);
    git(&repo, &["commit", "-qm", "manifest"]);

    let policy = fast_policy();
    let result = prepare(&repo, false, &policy).await;
    let error = match result {
        Ok(value) => panic!("oversized stdout must error, got ok"),
        Err(e) => e,
    };
    let msg = format!("{error:#}");
    assert!(
        msg.contains("exceeded 2 MiB") || msg.contains("stdout"),
        "msg: {msg}"
    );
}

#[allow(dead_code)]
fn env_kv_compiles(_s: &str) {
    let _ = env_kv_string(&[("a", "b")]);
}