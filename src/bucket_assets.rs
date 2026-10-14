//! Explicit per-checkout private asset workflow. No configuration => no work.
//! Upload runs before diff/staging; the final index must contain precisely the
//! verified manifest and no materialized asset changes. Existing Git baselines
//! are retained, never deleted/restored in the worktree by this module.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    schema_version: u32,
    enabled: bool,
    manifest: String,
    asset_root: String,
    workflow_script: String,
    #[serde(default)]
    uploader_credentials: Option<String>,
    #[serde(default)]
    consumer_credentials: Option<String>,
    #[serde(default)]
    large_bytes: Option<u64>,
    #[serde(default)]
    pressure_ratio: Option<f64>,
}

pub(crate) struct Prepared {
    manifest_path: PathBuf,
    verified_manifest: Vec<u8>,
    pub(crate) materialized_paths: HashSet<PathBuf>,
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\0', '\\', ':'])
        && !path.starts_with('-')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn regular_in_repo(repo: &Path, relative: &str) -> Result<PathBuf> {
    if !safe_relative(relative) {
        bail!("unsafe private asset workflow path");
    }
    let mut current = repo.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        if std::fs::symlink_metadata(&current)?
            .file_type()
            .is_symlink()
        {
            bail!("private asset workflow refuses symlinks");
        }
    }
    if !current.is_file() {
        bail!("private asset workflow requires a regular file");
    }
    Ok(current)
}

fn parse_materialized(bytes: &[u8], asset_root: &str) -> Result<HashSet<PathBuf>> {
    if asset_root != "." && !safe_relative(asset_root) {
        bail!("unsafe asset root");
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("invalid private manifest JSON")?;
    if value["schemaVersion"] != 1 || value["bucket"] != "dracon-lfs" {
        bail!("private manifest storage binding refused");
    }
    let assets = value["assets"]
        .as_array()
        .context("private manifest has no asset entries")?;
    let mut paths = HashSet::new();
    for asset in assets {
        if asset["visibility"] != "private" || asset["regenerable"] != true {
            bail!("private manifest eligibility refused");
        }
        let source = asset["sourcePath"]
            .as_str()
            .context("missing private source path")?;
        if !safe_relative(source) {
            bail!("unsafe private source path");
        }
        if let Some(bucketed) = asset.get("bucketed") {
            if !bucketed.is_boolean() {
                bail!("invalid bucketed flag");
            }
        }
        if asset["bucketed"] == true {
            let path = if asset_root == "." {
                PathBuf::from(source)
            } else {
                Path::new(asset_root).join(source)
            };
            if !paths.insert(path) {
                bail!("duplicate private materialized path");
            }
        }
    }
    Ok(paths)
}

async fn output(
    mut command: tokio::process::Command,
    timeout_secs: u64,
) -> Result<std::process::Output> {
    command.kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(timeout_secs.max(1)), command.output())
        .await
        .context("private bucket workflow timed out; no commit permitted")?
        .context("private bucket workflow process failed")
}

pub(crate) async fn prepare(
    repo: &Path,
    dry_run: bool,
    timeout_secs: u64,
) -> Result<Option<Prepared>> {
    let config_path = repo.join(".dracon/bucket-assets.json");
    match std::fs::symlink_metadata(&config_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let config_path = regular_in_repo(repo, ".dracon/bucket-assets.json")?;
    let config: Config = serde_json::from_slice(&std::fs::read(&config_path)?)
        .context("invalid private bucket workflow configuration")?;
    if config.schema_version != 1 {
        bail!("unsupported private bucket workflow configuration");
    }
    if !config.enabled {
        return Ok(None);
    }
    let manifest = regular_in_repo(repo, &config.manifest)?;
    let script = regular_in_repo(repo, &config.workflow_script)?;
    if config.asset_root != "." && !safe_relative(&config.asset_root) {
        bail!("unsafe private asset root");
    }
    if config
        .uploader_credentials
        .as_deref()
        .is_none_or(|v| !Path::new(v).is_absolute())
        || config
            .consumer_credentials
            .as_deref()
            .is_none_or(|v| !Path::new(v).is_absolute())
    {
        bail!("explicit scoped credential locations required");
    }
    if config.large_bytes == Some(0)
        || config
            .pressure_ratio
            .is_some_and(|v| !v.is_finite() || v <= 0.0 || v >= 1.0)
    {
        bail!("invalid private bucket routing threshold");
    }
    if !dry_run {
        let mut command = tokio::process::Command::new("node");
        command
            .arg(script)
            .arg("upload")
            .arg(&config_path)
            .arg(repo)
            .current_dir(repo);
        let result = output(command, timeout_secs).await?;
        // Intentionally do not echo captured stderr/stdout: third-party SDK
        // diagnostics must never become daemon credential-bearing logs.
        if !result.status.success() {
            bail!("private bucket upload refused; staged/worktree content retained");
        }
        let verdict: serde_json::Value = serde_json::from_slice(&result.stdout)
            .context("private workflow returned an invalid verdict")?;
        if verdict["ok"] != true {
            bail!("private workflow did not verify publication");
        }
    }
    let bytes = std::fs::read(manifest)?;
    let materialized_paths = parse_materialized(&bytes, &config.asset_root)?;
    Ok(Some(Prepared {
        manifest_path: PathBuf::from(config.manifest),
        verified_manifest: bytes,
        materialized_paths,
    }))
}

impl Prepared {
    pub(crate) async fn verify_staged(&self, repo: &Path, timeout_secs: u64) -> Result<()> {
        let mut command = tokio::process::Command::from(crate::git::git_cmd().into_std());
        command
            .args([
                "show",
                &format!(":{}", self.manifest_path.to_string_lossy()),
            ])
            .current_dir(repo);
        let manifest = output(command, timeout_secs).await?;
        if !manifest.status.success() || manifest.stdout != self.verified_manifest {
            bail!("staged private manifest differs from upload-verified references");
        }
        for path in crate::git::staged_paths(repo).await? {
            if self.materialized_paths.contains(&path) || path.starts_with(".bucket-cache") {
                bail!("materialized private asset/cache was staged; refusing raw-blob commit");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malicious_paths_and_ineligible_manifests_are_refused() {
        for path in [
            "../escape",
            "/absolute",
            "a/../b",
            "a//b",
            "a\\b",
            "-option",
            "a:b",
        ] {
            assert!(!safe_relative(path));
        }
        assert!(safe_relative("web/games/static/favicon.png"));
        let bytes = br#"{"schemaVersion":1,"bucket":"dracon-lfs","assets":[{"sourcePath":"static/a.png","visibility":"private","regenerable":true,"bucketed":true}]}"#;
        assert!(parse_materialized(bytes, "web/games")
            .unwrap()
            .contains(Path::new("web/games/static/a.png")));
        assert!(parse_materialized(bytes, "../outside").is_err());
        assert!(parse_materialized(
            &String::from_utf8_lossy(bytes)
                .replace("private", "public")
                .into_bytes(),
            "."
        )
        .is_err());
    }

    #[tokio::test]
    async fn unconfigured_and_disabled_repos_do_not_invoke_upload() {
        let dir = tempfile::tempdir().unwrap();
        assert!(prepare(dir.path(), false, 1).await.unwrap().is_none());
        std::fs::create_dir(dir.path().join(".dracon")).unwrap();
        std::fs::write(dir.path().join(".dracon/bucket-assets.json"), br#"{"schemaVersion":1,"enabled":false,"manifest":"not-present","assetRoot":".","workflowScript":"not-present"}"#).unwrap();
        assert!(prepare(dir.path(), false, 1).await.unwrap().is_none());
    }
}
