//! Bridge to `scripts/bucket-runner/runner.bundle.mjs`.
//!
//! The legacy `bucket_assets` workflow runs an inline Node script per
//! repo. The bucket-runner bundle is the third-party SDK that performs
//! the same verified-reference work for repositories that opt in via a
//! tracked `bucket-runner.json`. The bridge contract:
//!
//! - Absent or `enabled = false` `bucket-runner.json` → `Ok(None)` and
//!   the existing `bucket_assets::prepare` path stays in charge.
//! - Present + enabled → spawn `node <bundle-path>` with a bounded JSON
//!   request on stdin, read the bounded verdict JSON on stdout, parse
//!   the referenced manifest from the worktree, and return a
//!   `bucket_assets::Prepared` so downstream `verify_staged` and
//!   `staged_paths` checks at `sync.rs:4580` and `sync.rs:5598` work
//!   without further changes.
//!
//! All GIT_* repository-discovery env vars are stripped before spawn
//! (mirrors `bucket_guard::run_guard`) so the bundle measures THIS repo
//! from its explicit root rather than inheriting the daemon's (or a
//! worktree's) `GIT_DIR`. stdout/stderr from the bundle are never
//! logged: third-party SDK diagnostics must never become
//! credential-bearing daemon logs.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;

use crate::bucket_assets::Prepared;
use crate::policy::SyncPolicy;

mod outgoing;

/// Tracked config basename. Lives at the repo root only.
const CONFIG_RELATIVE: &str = "bucket-runner.json";

/// Hard I/O caps. Mirror `LIMIT2` inside `runner.bundle.mjs`.
const MAX_IO_BYTES: usize = 2 * 1024 * 1024;

/// Cap on auto-discovered candidate paths. Matches the bundle's
/// `allowedCandidates.length <= 2e4` guard.
const MAX_CANDIDATES: usize = 20000;

/// Floor on the bundle spawn timeout. Even when the operator lowers
/// `stage_op_timeout_secs`, a publish cycle that talks to S3 needs a
/// generous safety margin so transient network stalls do not abort a
/// mid-upload commit.
const MIN_BUNDLE_TIMEOUT_SECS: u64 = 60;

/// Default policy is shared with the JS runner, never supplied by repo config.
const ROUTING_DEFAULTS_JSON: &str = include_str!("../scripts/bucket-runner/routing-defaults.json");
const MAX_JS_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoutingDefaults {
    schema_version: u32,
    large_bytes: u64,
    pressure_ratio: f64,
    pressure_limit_bytes: u64,
}

fn routing_budget_from_json(defaults: &str, projected_bytes: u64) -> Result<serde_json::Value> {
    let policy: RoutingDefaults =
        serde_json::from_str(defaults).context("invalid shared routing policy")?;
    if policy.schema_version != 1
        || policy.large_bytes == 0
        || policy.large_bytes > MAX_JS_INTEGER
        || policy.pressure_limit_bytes == 0
        || policy.pressure_limit_bytes > MAX_JS_INTEGER
        || projected_bytes > MAX_JS_INTEGER
        || !policy.pressure_ratio.is_finite()
        || policy.pressure_ratio <= 0.0
        || policy.pressure_ratio >= 1.0
    {
        bail!("invalid shared routing policy or projected bytes");
    }
    Ok(serde_json::json!({
        "projectedBytes": projected_bytes,
        "limitBytes": policy.pressure_limit_bytes,
        "largeBytes": policy.large_bytes,
        "pressureRatio": policy.pressure_ratio,
    }))
}

/// Default manifest path inside the repo when the config omits one.
fn default_manifest_path() -> String {
    "bucket-assets.manifest.json".to_owned()
}

/// Media extensions accepted by the bundle's object-key regex
/// (`/^[a-f0-9]{64}\.(?:…)$/` in `runner.bundle.mjs`). Anything outside
/// this list is filtered out during `allowedCandidates` discovery so the
/// SDK never receives a candidate it cannot verify.
const MEDIA_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "avif", "gif", "svg", "mp3", "wav", "ogg", "mp4", "webm", "glb",
    "gltf",
];

/// Status values returned by the bundle for which a verdict is
/// non-refusing. `refused`, `plannable`, and `unsupported` always fail
/// closed (no commit permitted).
const STATUS_REFUSED: &str = "refused";
const STATUS_PLANNABLE: &str = "plannable";
const STATUS_UNSUPPORTED: &str = "unsupported";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    schema_version: u32,
    enabled: bool,
    profile_path: String,
    state_directory: String,
    remote_name: String,
    trusted_remote_url: String,
    asset_root: String,
    #[serde(default = "default_manifest_path")]
    manifest_path: String,
    #[serde(default)]
    allowed_candidates: Vec<String>,
}

#[derive(Deserialize)]
struct Profile {
    runner: ProfileRunner,
}

#[derive(Deserialize)]
struct ProfileRunner {
    path: String,
}

/// Reject control chars, leading dashes, absolute paths, and `..`
/// traversal in any repo-relative string. Mirrors `bucket_assets::safe_relative`
/// but adds a length cap (`<= 4096`) matching the bundle's `safe3` predicate.
fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['\0', '\\', ':'])
        && !path.starts_with('-')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Walk `repo / relative` and refuse symlinks at every component. The
/// manifest and profile live in the worktree, never the index, so
/// `regular_in_repo` must reject any path that has been replaced with a
/// symlink since the last clean checkout.
fn regular_in_repo(repo: &Path, relative: &str) -> Result<PathBuf> {
    if !safe_relative(relative) {
        bail!("unsafe bucket runner path");
    }
    let mut current = repo.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        if std::fs::symlink_metadata(&current)?
            .file_type()
            .is_symlink()
        {
            bail!("bucket runner path refuses symlinks");
        }
    }
    if !current.is_file() {
        bail!("bucket runner path requires a regular file");
    }
    Ok(current)
}

/// State directory is `absolute + dir + owned by current euid + mode 0700`.
/// This matches `directory3` inside `runner.bundle.mjs` so a config typo
/// is caught locally instead of producing a `request-or-authority-refused`
/// verdict from the SDK after a wasted spawn.
fn validate_state_directory(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    if !p.is_absolute() {
        bail!("stateDirectory must be absolute");
    }
    let metadata = std::fs::symlink_metadata(p)
        .with_context(|| format!("stateDirectory {path} not accessible"))?;
    if metadata.file_type().is_symlink() {
        bail!("stateDirectory refuses symlinks");
    }
    if !metadata.is_dir() {
        bail!("stateDirectory must be a directory");
    }
    let euid = unsafe { libc::geteuid() };
    if metadata.uid() != euid {
        bail!("stateDirectory owner must match current euid");
    }
    if metadata.permissions().mode() & 0o777 != 0o700 {
        bail!("stateDirectory mode must be 0700");
    }
    Ok(p.to_path_buf())
}

fn validate_profile_path(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    if !p.is_absolute() {
        bail!("profilePath must be absolute");
    }
    if !p.is_file() {
        bail!("profilePath must reference a regular file");
    }
    Ok(p.to_path_buf())
}

/// Mirrors `validateRequest`'s `trustedRemoteUrl` predicate inside the
/// bundle (`length > 0 && length <= 4096 && !/^[-]|[\s\x00-\x1f\x7f]/`).
fn trusted_remote_url_ok(url: &str) -> bool {
    !url.is_empty()
        && url.len() <= 4096
        && !url.starts_with('-')
        && !url
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control() || b == 0x7f)
}

/// Compile the `^[A-Za-z0-9_-]{1,80}$` remote-name check once.
fn remote_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Resolve the bridge config from the repo root. `Ok(None)` if the
/// config is absent (the bridge is opt-in). Any other I/O or parse
/// failure propagates so the daemon surfaces a clear refusal rather
/// than silently falling back to the legacy workflow.
fn discover_config(repo: &Path) -> Result<Option<PathBuf>> {
    match regular_in_repo(repo, CONFIG_RELATIVE) {
        Ok(path) => Ok(Some(path)),
        Err(error) => match error.downcast_ref::<std::io::Error>() {
            Some(io) if io.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(error),
        },
    }
}

/// Parse the manifest and build the same set of materialized paths the
/// legacy workflow produces. `.bucket-cache` is always suppressed so the
/// staged-tree check at `sync.rs:5598` never lets a generated cache
/// directory leak into a commit.
fn parse_materialized(bytes: &[u8], asset_root: &str) -> Result<HashSet<PathBuf>> {
    if asset_root != "." && !safe_relative(asset_root) {
        bail!("unsafe assetRoot");
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("invalid manifest JSON")?;
    if value["schemaVersion"] != 1 || value["bucket"] != "dracon-lfs" {
        bail!("manifest binding refused");
    }
    let assets = value["assets"]
        .as_array()
        .context("manifest has no asset entries")?;
    let mut paths = HashSet::new();
    for asset in assets {
        if let Some(bucketed) = asset.get("bucketed") {
            if !bucketed.is_boolean() {
                bail!("invalid bucketed flag");
            }
        }
        if asset["bucketed"] != true {
            continue;
        }
        let source = asset["sourcePath"]
            .as_str()
            .context("missing manifest source path")?;
        if !safe_relative(source) {
            bail!("unsafe manifest source path");
        }
        // Runner requests use repository root; manifest sources are already
        // repo-relative even when assetRoot narrows discovery to a subtree.
        let path = PathBuf::from(source);
        if !paths.insert(path) {
            bail!("duplicate materialized path");
        }
    }
    paths.insert(PathBuf::from(".bucket-cache"));
    Ok(paths)
}

/// Discover tracked and new Git-visible media, using the same operator
/// staging exclusions. Keep paths repo-relative, including non-dot roots.
async fn discover_candidates(
    repo: &Path,
    asset_root: &str,
    policy: &SyncPolicy,
) -> Result<Vec<String>> {
    if asset_root != "." && !safe_relative(asset_root) {
        bail!("unsafe assetRoot");
    }
    let (tracked, untracked) = outgoing::candidate_lists(repo).await?;
    let repo_override = crate::policy::load_repo_override(repo);
    if repo_override.override_parse_error.is_some() {
        bail!("invalid candidate policy");
    }
    let excludes = crate::policy::effective_auto_commit_excludes(policy, &repo_override);
    let dirs = policy.exclude_dir_names.iter().cloned().collect();
    let prefix = (asset_root != ".").then(|| format!("{asset_root}/"));
    let mut candidates = std::collections::BTreeSet::new();
    let mut scanned = 0;
    for (bytes, is_untracked) in [(&tracked, false), (&untracked, true)] {
        for raw in bytes.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            scanned += 1;
            if scanned > MAX_CANDIDATES {
                bail!("candidate scan exceeds limit");
            }
            let line = std::str::from_utf8(raw).context("non-UTF8 candidate")?;
            if is_untracked
                && (!policy.auto_stage_untracked
                    || crate::exclude::matches_untracked_exclude(
                        repo,
                        Path::new(line),
                        &policy.untracked_exclude_patterns,
                    ))
            {
                continue;
            }
            if prefix
                .as_ref()
                .is_some_and(|prefix| !line.starts_with(prefix))
                || !safe_relative(line)
            {
                continue;
            }
            let ext = match Path::new(line).extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !MEDIA_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            let path = repo.join(line);
            let meta = std::fs::symlink_metadata(&path).context("candidate vanished")?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                continue;
            }
            let mut ancestor = path.parent();
            let mut nested = false;
            while let Some(parent) = ancestor {
                if parent == repo {
                    break;
                }
                if parent.join(".git").exists()
                    || std::fs::symlink_metadata(parent)?.file_type().is_symlink()
                {
                    nested = true;
                    break;
                }
                ancestor = parent.parent();
            }
            if nested {
                continue;
            }
            let entry = dracon_git::types::DiffFile::new(
                PathBuf::from(line),
                dracon_git::types::FileStatus::Added,
            );
            if crate::exclude::should_stage_entry_with_blob_size(
                repo,
                &entry,
                meta.len(),
                &dirs,
                &policy.exclude_file_patterns,
                policy.max_stage_file_bytes,
                &excludes,
            ) {
                candidates.insert(line.to_owned());
            }
        }
    }
    Ok(candidates.into_iter().collect())
}

/// Strip repository-discovery env vars and force kill-on-drop. The set
/// mirrors `bucket_guard::run_guard` so the bundle and the daemon's
/// existing guards agree on what an "ambient" repo looks like.
fn strip_git_env(cmd: &mut Command) {
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_SHALLOW_FILE",
        "GIT_GRAFT_FILE",
        "GIT_REPLACE_REF_BASE",
    ] {
        cmd.env_remove(var);
    }
    cmd.kill_on_drop(true);
}

/// Probe the bundle's `--version` exit. Used for `dry_run` so the daemon
/// can confirm the bundle is installed and runnable without spending the
/// cost of a full publish cycle.
async fn run_version(bundle: &Path) -> Result<()> {
    let mut cmd = Command::new("node");
    cmd.arg(bundle).arg("--version");
    strip_git_env(&mut cmd);
    let output = crate::git::run_command_captured_output(
        cmd,
        MIN_BUNDLE_TIMEOUT_SECS,
        "bundle --version",
        None,
    )
    .await
    .context("bundle --version failed")?;
    if !output.status.success() {
        bail!("bundle --version refused");
    }
    // Never echo the captured stdout/stderr: third-party SDK diagnostics
    // must never become daemon credential-bearing logs.
    let verdict: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("bundle --version invalid JSON")?;
    if verdict["schemaVersion"] != 1 {
        bail!("bundle --version schema mismatch");
    }
    Ok(())
}

/// Pipe `request` to the bundle's stdin, capture stdout with a hard
/// 2 MiB cap, and return the parsed verdict. The bundle's own
/// `LIMIT2` cap already shrinks oversized output to a `refused`
/// verdict, but we re-check defensively.
async fn spawn_bundle(
    bundle: &Path,
    request: &[u8],
    timeout_secs: u64,
) -> Result<serde_json::Value> {
    let mut cmd = Command::new("node");
    cmd.arg(bundle);
    strip_git_env(&mut cmd);
    let output =
        crate::git::run_command_captured_output(cmd, timeout_secs, "bucket bundle", Some(request))
            .await
            .context("bundle execution failed")?;
    if output.stdout.len() > MAX_IO_BYTES {
        bail!("bundle stdout exceeded 2 MiB");
    }
    // Never echo captured stdout/stderr: third-party SDK diagnostics
    // must never become daemon credential-bearing logs.
    if !output.status.success() {
        bail!("bundle refused");
    }
    serde_json::from_slice(&output.stdout).context("invalid bundle verdict")
}

/// Bridge entry point. Returns `Ok(None)` when the repo is not
/// bridge-managed so `sync.rs` can fall through to
/// `bucket_assets::prepare` unchanged. When bridge-managed, returns
/// a fully populated `bucket_assets::Prepared` whose shape matches
/// the legacy workflow's output exactly.
pub(crate) async fn prepare(
    repo: &Path,
    dry_run: bool,
    policy: &SyncPolicy,
) -> Result<Option<Prepared>> {
    let Some(config_path) = discover_config(repo)? else {
        return Ok(None);
    };
    let config_bytes =
        std::fs::read(&config_path).with_context(|| format!("read {}", config_path.display()))?;
    let config: Config = serde_json::from_slice(&config_bytes)
        .with_context(|| format!("invalid {}", config_path.display()))?;
    if config.schema_version != 1 {
        bail!("unsupported bucket-runner.json schemaVersion");
    }
    if !config.enabled {
        return Ok(None);
    }

    let profile_path = validate_profile_path(&config.profile_path)?;
    let state_directory = validate_state_directory(&config.state_directory)?;
    if config.asset_root != "." && !safe_relative(&config.asset_root) {
        bail!("unsafe assetRoot");
    }
    if !safe_relative(&config.manifest_path) {
        bail!("unsafe manifestPath");
    }
    if !remote_name_ok(&config.remote_name) {
        bail!("remoteName must match /^[A-Za-z0-9_-]{{1,80}}$/");
    }
    if !trusted_remote_url_ok(&config.trusted_remote_url) {
        bail!("trustedRemoteUrl must be a safe non-empty string");
    }
    for entry in &config.allowed_candidates {
        if !safe_relative(entry) {
            bail!("unsafe allowedCandidates entry: {entry}");
        }
    }

    let profile_bytes = std::fs::read(&profile_path)
        .with_context(|| format!("read profile {}", profile_path.display()))?;
    let profile: Profile = serde_json::from_slice(&profile_bytes)
        .with_context(|| format!("invalid profile JSON {}", profile_path.display()))?;
    if !profile.runner.path.ends_with("/runner.bundle.mjs")
        && !profile.runner.path.ends_with("/runner.bundle.cjs")
    {
        bail!("profile.runner.path must end with /runner.bundle.mjs or /runner.bundle.cjs");
    }
    let bundle_path = PathBuf::from(&profile.runner.path);
    if !bundle_path.is_file() {
        bail!("bundle path not found: {}", bundle_path.display());
    }

    if dry_run {
        run_version(&bundle_path).await?;
        return Ok(None);
    }

    let allowed_candidates = if config.allowed_candidates.is_empty() {
        discover_candidates(repo, &config.asset_root, policy).await?
    } else {
        config.allowed_candidates.clone()
    };

    let measured_bytes = outgoing::measure(repo, policy)
        .await
        .context("bucket outgoing-byte accounting refused; no publish")?;
    // Publication may create/grow a previously clean or absent manifest.
    // Reserve its enforced output ceiling BEFORE the routing decision.
    let projected_bytes = measured_bytes
        .checked_add(policy.max_stage_file_bytes)
        .filter(|bytes| *bytes <= MAX_JS_INTEGER)
        .context("accounting-manifest-reserve-overflow")?;
    let request = serde_json::json!({
        "schemaVersion": 1u32,
        "operation": "publish",
        "root": repo.to_string_lossy(),
        "profilePath": profile_path.to_string_lossy(),
        "stateDirectory": state_directory.to_string_lossy(),
        "remoteName": config.remote_name,
        "trustedRemoteUrl": config.trusted_remote_url,
        // Measured read-only upper bound; failure above cannot fall back to
        // a zero budget or launch the upload-capable bundle.
        "budget": routing_budget_from_json(ROUTING_DEFAULTS_JSON, projected_bytes)?,
        "allowedCandidates": allowed_candidates,
    });
    let request_bytes = serde_json::to_vec(&request).context("request serialization")?;
    if request_bytes.len() > MAX_IO_BYTES {
        bail!("bucket-runner request exceeds 2 MiB");
    }

    let timeout_secs = policy.stage_op_timeout_secs.max(MIN_BUNDLE_TIMEOUT_SECS);
    let verdict = spawn_bundle(&bundle_path, &request_bytes, timeout_secs).await?;

    if verdict.get("schemaVersion") != Some(&serde_json::Value::from(1u64)) {
        bail!("bundle verdict schemaVersion mismatch");
    }
    let status = verdict["status"]
        .as_str()
        .context("bundle verdict missing status")?
        .to_owned();
    let commit_eligible = verdict["commitEligible"].as_bool().unwrap_or(false);
    if matches!(
        status.as_str(),
        STATUS_REFUSED | STATUS_PLANNABLE | STATUS_UNSUPPORTED
    ) {
        bail!("bucket-runner status refused: {status}");
    }
    if !commit_eligible {
        bail!("bucket-runner commitEligible=false; status={status}");
    }

    let manifest_sha256 = verdict["manifestSha256"]
        .as_str()
        .context("bundle verdict missing manifestSha256")?
        .to_owned();
    if manifest_sha256.len() != 64
        || !manifest_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        bail!("invalid manifest digest from bundle");
    }

    let manifest_full =
        regular_in_repo(repo, &config.manifest_path).context("read manifestPath from worktree")?;
    let manifest_bytes =
        std::fs::read(&manifest_full).context("read manifest bytes from worktree")?;

    use sha2::{Digest, Sha256};
    if format!("{:x}", Sha256::digest(&manifest_bytes)) != manifest_sha256 {
        bail!("manifest sha256 mismatch; no commit permitted");
    }

    let mut materialized_paths = parse_materialized(&manifest_bytes, &config.asset_root)?;
    materialized_paths.insert(PathBuf::from(format!("{}.lock", config.manifest_path)));
    Ok(Some(
        Prepared::from_parts(
            PathBuf::from(&config.manifest_path),
            manifest_bytes,
            materialized_paths,
        )
        .with_output_ceiling(policy.max_stage_file_bytes),
    ))
}

#[cfg(test)]
#[path = "bucket_runner_bridge_tests.rs"]
mod bucket_runner_bridge_tests;
