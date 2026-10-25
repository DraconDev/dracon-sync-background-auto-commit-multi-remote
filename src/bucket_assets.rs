//! Explicit per-checkout private asset workflow. No configuration => no work.
//! Upload runs before diff/staging; the final index must contain precisely the
//! verified manifest and no materialized asset changes. Existing Git baselines
//! are retained, never deleted/restored in the worktree by this module.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

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
    asset_root: PathBuf,
    output_ceiling: Option<u64>,
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
    command: tokio::process::Command,
    timeout_secs: u64,
) -> Result<std::process::Output> {
    crate::git::run_command_captured_output(command, timeout_secs, "private bucket workflow", None)
        .await
        .context("private bucket workflow failed; no commit permitted")
}

/// Candidate locations, in order. A TRACKED root config keeps the pilot
/// reproducible from a fresh checkout; the ignored `.dracon/` path stays an
/// operator-local override. Neither exists by default.
const CONFIG_CANDIDATES: [&str; 2] = ["bucket-assets.json", ".dracon/bucket-assets.json"];

fn discover_config(repo: &Path) -> Result<Option<PathBuf>> {
    for relative in CONFIG_CANDIDATES {
        match regular_in_repo(repo, relative) {
            Ok(path) => return Ok(Some(path)),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub(crate) async fn prepare(
    repo: &Path,
    dry_run: bool,
    timeout_secs: u64,
) -> Result<Option<Prepared>> {
    let Some(config_path) = discover_config(repo)? else {
        return Ok(None);
    };
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
    let mut verified_digest = None;
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
        let digest = verdict["manifestSha256"]
            .as_str()
            .context("workflow omitted upload-bound manifest digest")?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            bail!("invalid upload-bound manifest digest");
        }
        verified_digest = Some(digest.to_owned());
    }
    let bytes = std::fs::read(manifest)?;
    if let Some(expected) = verified_digest {
        use sha2::{Digest, Sha256};
        if format!("{:x}", Sha256::digest(&bytes)) != expected {
            bail!("manifest changed after upload verification; no commit permitted");
        }
    }
    let materialized_paths = parse_materialized(&bytes, &config.asset_root)?;
    Ok(Some(Prepared {
        manifest_path: PathBuf::from(config.manifest),
        verified_manifest: bytes,
        asset_root: PathBuf::from(config.asset_root),
        output_ceiling: None,
        materialized_paths,
    }))
}

impl Prepared {
    /// Construct a `Prepared` from its parts. Used by sibling modules
    /// (e.g. `bucket_runner_bridge`) that produce equivalent verified
    /// references from a different workflow. The fields stay private so
    /// downstream call sites continue to rely on the same invariants
    /// (sha256-verified manifest, suppressed materialized paths,
    /// `.bucket-cache` exclusion); this constructor is the only
    /// external construction path.
    pub(crate) fn from_parts(
        manifest_path: PathBuf,
        verified_manifest: Vec<u8>,
        materialized_paths: HashSet<PathBuf>,
        asset_root: PathBuf,
    ) -> Self {
        Self {
            manifest_path,
            verified_manifest,
            asset_root,
            output_ceiling: None,
            materialized_paths,
        }
    }

    pub(crate) fn with_output_ceiling(mut self, ceiling: u64) -> Self {
        self.output_ceiling = Some(ceiling);
        self
    }

    /// Missing materializations in a sparse/fresh consumer are not new work.
    /// Present changed sources must remain pending even if they are Gitignored.
    pub(crate) async fn has_materialization_followup(&self, repo: &Path) -> Result<bool> {
        let mut present = Vec::new();
        for path in &self.materialized_paths {
            match std::fs::symlink_metadata(repo.join(path)) {
                Ok(_) => present.push(path.clone()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.has_untracked_followup(repo, &present).await
    }

    /// Git still reports raw untracked materializations; do not hide or rewrite
    /// that state. For THIS eligible-work result only, exclude exactly the
    /// already-declared runtime lock and sources whose current bytes still match
    /// the upload-verified manifest. Unknown paths/changed bytes remain pending.
    /// The caller's absolute epoch deadline owns this streaming inspection.
    pub(crate) async fn has_untracked_followup(
        &self,
        repo: &Path,
        untracked: &[PathBuf],
    ) -> Result<bool> {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncReadExt;
        let manifest: serde_json::Value = serde_json::from_slice(&self.verified_manifest)?;
        let mut sources = std::collections::HashMap::new();
        for asset in manifest["assets"]
            .as_array()
            .context("verified manifest assets missing")?
        {
            if asset["bucketed"] != true {
                continue;
            }
            let source = asset["sourcePath"]
                .as_str()
                .context("missing verified source path")?;
            if !safe_relative(source) {
                bail!("unsafe verified source path");
            }
            let source = if self.asset_root == Path::new(".") {
                PathBuf::from(source)
            } else {
                self.asset_root.join(source)
            };
            sources.insert(source, asset);
        }
        let runtime_lock = PathBuf::from(format!("{}.lock", self.manifest_path.display()));
        for path in untracked {
            if !self.materialized_paths.contains(path) {
                return Ok(true);
            }
            if path == &runtime_lock {
                continue;
            } // already-reserved SDK lock, not an asset/name-prefix waiver
            let Some(asset) = sources.get(path) else {
                return Ok(true);
            };
            let Some(expected_bytes) = asset["bytes"].as_u64() else {
                return Ok(true);
            };
            let Some(expected_sha) = asset["sha256"].as_str().filter(|sha| {
                sha.len() == 64
                    && sha
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }) else {
                return Ok(true);
            };
            let relative = path.to_str().context("nonportable verified source path")?;
            let full = regular_in_repo(repo, relative)?;
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            let file = options.open(&full)?;
            let before = file.metadata()?;
            if !before.is_file() || before.len() != expected_bytes {
                return Ok(true);
            }
            let mut file = tokio::fs::File::from_std(file);
            let mut digest = Sha256::new();
            let mut read = 0u64;
            // Heap-backed: embedding 64KiB in the nested epoch/scheduler
            // future can overflow the normal test/worker stack when moved.
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                read = read
                    .checked_add(count as u64)
                    .context("materialization byte overflow")?;
                if read > expected_bytes {
                    return Ok(true);
                }
                digest.update(&buffer[..count]);
            }
            let after = std::fs::symlink_metadata(&full)?;
            if !after.is_file()
                || after.len() != before.len()
                || after.modified()? != before.modified()?
            {
                return Ok(true);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if after.dev() != before.dev() || after.ino() != before.ino() {
                    return Ok(true);
                }
            }
            if read != expected_bytes || format!("{:x}", digest.finalize()) != expected_sha {
                return Ok(true);
            }
        }
        Ok(false)
    }

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
        let staged_paths = crate::git::staged_paths(repo).await?;
        if let Some(ceiling) = self.output_ceiling {
            // Backstop the same staged-blob ceiling used by normal hygiene:
            // routing reservations must never authorize larger clean output.
            for size in
                crate::git::staged_blob_sizes_for(repo, staged_paths.iter().cloned().collect())
                    .await?
                    .values()
            {
                if !matches!(size, Some(bytes) if *bytes <= ceiling) {
                    bail!("staged output exceeds accounting ceiling or cannot be measured");
                }
            }
        }
        for path in staged_paths {
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

    #[tokio::test]
    async fn followup_accounts_for_exact_verified_bytes_without_hiding_foreign_work() {
        use sha2::{Digest, Sha256};
        for root in [".", "public"] {
            let dir = tempfile::tempdir().unwrap();
            let relative = if root == "." {
                PathBuf::from("asset.bin")
            } else {
                PathBuf::from("public/asset.bin")
            };
            std::fs::create_dir_all(dir.path().join(&relative).parent().unwrap()).unwrap();
            std::fs::write(dir.path().join(&relative), b"old").unwrap();
            let manifest = serde_json::json!({"assets":[{"sourcePath":"asset.bin",
                "bucketed":true,"bytes":3,"sha256":format!("{:x}", Sha256::digest(b"old"))}]});
            let lock = PathBuf::from("manifest.json.lock");
            std::fs::write(dir.path().join(&lock), b"reserved SDK lock").unwrap();
            let prepared = Prepared::from_parts(
                PathBuf::from("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
                HashSet::from([relative.clone(), lock.clone()]),
                PathBuf::from(root),
            );
            let paths = vec![relative.clone(), lock];
            assert!(!prepared
                .has_untracked_followup(dir.path(), &paths)
                .await
                .unwrap());
            std::fs::write(dir.path().join(&relative), b"new").unwrap();
            assert!(
                prepared
                    .has_untracked_followup(dir.path(), &paths)
                    .await
                    .unwrap(),
                "same-size later source bytes are real pending private work"
            );
            std::fs::write(dir.path().join(&relative), b"old").unwrap();
            let mut foreign = paths.clone();
            foreign.push(PathBuf::from("public/unknown-child.bin"));
            assert!(
                prepared
                    .has_untracked_followup(dir.path(), &foreign)
                    .await
                    .unwrap(),
                "a known directory or SDK enrollment must not waive an unknown child"
            );
            assert_eq!(std::fs::read(dir.path().join(&relative)).unwrap(), b"old");
            std::fs::remove_file(dir.path().join(&relative)).unwrap();
            assert!(
                prepared
                    .has_untracked_followup(dir.path(), &paths)
                    .await
                    .is_err(),
                "inspection failure cannot become no-work"
            );
            #[cfg(unix)]
            {
                std::fs::write(dir.path().join("other"), b"old").unwrap();
                std::os::unix::fs::symlink(dir.path().join("other"), dir.path().join(&relative))
                    .unwrap();
                assert!(prepared.has_untracked_followup(dir.path(), &paths).await.is_err(),
                    "even identical bytes through a substituted symlink are not verified materialization");
            }
        }
    }

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
        // A TRACKED root opt-in is discovered, and unsafe manifests/roots are
        // refused before any workflow child process is spawned.
        std::fs::write(
            dir.path().join("bucket-assets.json"),
            br#"{"schemaVersion":1,"enabled":true,"manifest":"../outside.json","assetRoot":"..","workflowScript":"../outside.mjs"}"#,
        )
        .unwrap();
        assert!(prepare(dir.path(), false, 5).await.is_err());
    }

    fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
        let output = crate::git::git_cmd()
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(output.status.success(), "fixture git operation failed");
        output.stdout
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q", "-b", "main"]);
        git(repo, &["config", "user.name", "DraconDev"]);
        git(repo, &["config", "user.email", "dracsharp@gmail.com"]);
        // Test isolation: this fixture is not warden-managed, and the real
        // warden harden pass would rewrite .gitignore hygiene blocks. Setting
        // the local filter marks it as already hardened so the daemon skips
        // that external write entirely.
        git(repo, &["config", "filter.dracon.clean", "cat"]);
        // The daemon now runs the configured commit hook chain, so the ambient
        // global warden pre-commit hook applies here. Neutralize it LOCALLY, the
        // same way any repo out of warden's scope does: this fixture exercises
        // the daemon's staging/commit path, not warden's policy, and its
        // synthetic `.gitattributes` is deliberately not warden-hardened.
        // An explicit empty hooks path is used (not /dev/null) so `git` reports
        // "no hook" rather than "cannot run hook".
        let empty_hooks = repo.join(".git/empty-hooks");
        std::fs::create_dir_all(&empty_hooks).unwrap();
        git(
            repo,
            &["config", "core.hooksPath", empty_hooks.to_str().unwrap()],
        );
        std::fs::create_dir(repo.join(".dracon")).unwrap();
        std::fs::create_dir(repo.join("static")).unwrap();
        std::fs::write(repo.join("static/a.png"), "old tracked baseline").unwrap();
        std::fs::write(repo.join(".gitignore"), "/.bucket-cache/\n").unwrap();
        std::fs::write(repo.join("bucket-assets.json"), br#"{"schemaVersion":1,"enabled":true,"manifest":"manifest.json","assetRoot":".","workflowScript":"workflow.mjs","uploaderCredentials":"/synthetic/uploader.env","consumerCredentials":"/synthetic/consumer.env"}"#).unwrap();
        std::fs::write(repo.join("manifest.json"), br#"{"schemaVersion":1,"bucket":"dracon-lfs","repository":"games","namespace":"games","assets":[{"id":"fixture","sourcePath":"static/a.png","logicalPath":"brand/a.png","objectKey":"games/{sha256}.{ext}","sha256":"0000000000000000000000000000000000000000000000000000000000000000","bytes":20,"visibility":"private","regenerable":true,"generator":{"command":"node generate.mjs","inputs":[]}}]}"#).unwrap();
        std::fs::write(repo.join("bucket-assets.json"), br#"{"schemaVersion":1,"enabled":true,"manifest":"manifest.json","assetRoot":".","workflowScript":"workflow.mjs","uploaderCredentials":"/synthetic/uploader.env","consumerCredentials":"/synthetic/consumer.env"}"#).unwrap();
        // Controlled fake storage layer. The actual daemon staging/commit path
        // remains real; live SDK/storage qualification is a separate gate.
        std::fs::write(repo.join("workflow.mjs"), r#"import fs from 'node:fs';
import crypto from 'node:crypto';
if(fs.existsSync('.git/refuse-upload')) process.exit(1);
const before=fs.readFileSync('manifest.json','utf8');
const manifest=JSON.parse(before), asset=manifest.assets[0], body=fs.readFileSync(asset.sourcePath);
const sha=crypto.createHash('sha256').update(body).digest('hex');
fs.writeFileSync('.git/upload-verified',sha);
if(asset.sha256!==sha || !asset.bucketed){asset.sha256=sha;asset.bytes=body.length;asset.bucketed=true;fs.writeFileSync('manifest.json',JSON.stringify(manifest));}
const manifestSha256=crypto.createHash('sha256').update(fs.readFileSync('manifest.json')).digest('hex');
if(fs.existsSync('.git/tamper-after-verification')){asset.sha256='f'.repeat(64);fs.writeFileSync('manifest.json',JSON.stringify(manifest));}
console.log(JSON.stringify({ok:true,manifestSha256}));
"#).unwrap();
        git(
            repo,
            &[
                "add",
                "--",
                ".gitignore",
                "bucket-assets.json",
                "workflow.mjs",
                "manifest.json",
                "static/a.png",
            ],
        );
        git(repo, &["commit", "--no-verify", "-qm", "fixture baseline"]);
        dir
    }

    #[tokio::test]
    async fn staged_accounting_ceiling_measures_clean_output_not_plaintext() {
        if !crate::test_helpers::node_available() {
            {
                eprintln!("skip: node is not available in this environment");
                return;
            }
        }
        let dir = fixture();
        let repo = dir.path();
        let prepared = prepare(repo, false, 30)
            .await
            .unwrap()
            .unwrap()
            .with_output_ceiling(4096);
        git(repo, &["add", "--", "manifest.json"]);
        prepared.verify_staged(repo, 10).await.unwrap();
        std::fs::write(repo.join(".gitattributes"), "expanded.txt filter=expand\n").unwrap();
        git(
            repo,
            &[
                "config",
                "filter.expand.clean",
                "cat; head -c 8192 /dev/zero",
            ],
        );
        std::fs::write(repo.join("expanded.txt"), "x").unwrap();
        // This is ordinary authorized staging, never an accounting probe.
        git(repo, &["add", "--", "expanded.txt"]);
        let error = prepared.verify_staged(repo, 10).await.unwrap_err();
        assert!(error.to_string().contains("accounting ceiling"));
        assert_eq!(std::fs::read(repo.join("expanded.txt")).unwrap(), b"x");
    }

    #[tokio::test]
    async fn post_upload_capture_tampering_is_refused() {
        if !crate::test_helpers::node_available() {
            {
                eprintln!("skip: node is not available in this environment");
                return;
            }
        }
        let dir = fixture();
        std::fs::write(dir.path().join(".git/tamper-after-verification"), "tamper").unwrap();
        let error = prepare(dir.path(), false, 30).await.err().unwrap();
        assert!(error
            .to_string()
            .contains("changed after upload verification"));
        assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), b"1\n");
    }

    #[tokio::test]
    async fn staged_reference_drift_and_raw_materialization_are_refused() {
        if !crate::test_helpers::node_available() {
            {
                eprintln!("skip: node is not available in this environment");
                return;
            }
        }
        let dir = fixture();
        let repo = dir.path();
        let prepared = prepare(repo, false, 30).await.unwrap().unwrap();
        git(repo, &["add", "--", "manifest.json"]);
        prepared.verify_staged(repo, 10).await.unwrap();
        std::fs::write(repo.join("static/a.png"), "changed raw asset").unwrap();
        git(repo, &["add", "--", "static/a.png"]);
        assert!(prepared.verify_staged(repo, 10).await.is_err());
        git(repo, &["restore", "--staged", "--", "static/a.png"]);
        std::fs::write(repo.join("manifest.json"), "{}").unwrap();
        git(repo, &["add", "--", "manifest.json"]);
        assert!(prepared.verify_staged(repo, 10).await.is_err());
        assert_eq!(
            std::fs::read_to_string(repo.join("static/a.png")).unwrap(),
            "changed raw asset"
        );
    }

    #[tokio::test]
    async fn daemon_commits_verified_reference_not_raw_bytes_and_recovers_from_failure() {
        if !crate::test_helpers::node_available() {
            {
                eprintln!("skip: node is not available in this environment");
                return;
            }
        }
        let dir = fixture();
        let repo = dir.path();
        let policy: crate::policy::SyncPolicy = toml::from_str(
            r#"
auto_github_private = false
auto_commit = true
auto_pull = false
auto_push = false
auto_bump_versions = false
trusted_emails = ["dracsharp@gmail.com"]
trusted_authors = ["DraconDev"]
"#,
        )
        .unwrap();
        let count = |repo: &Path| {
            String::from_utf8(git(repo, &["rev-list", "--count", "HEAD"]))
                .unwrap()
                .trim()
                .parse::<usize>()
                .unwrap()
        };
        std::fs::write(repo.join("static/a.png"), "first generated version").unwrap();
        let result =
            crate::sync::sync_repo(repo, &policy, &Default::default(), 0, None, false, None).await;
        assert!(result.is_ok(), "daemon fixture failed: {result:?}");
        assert_eq!(
            count(repo),
            2,
            "verified reference must be exactly one commit"
        );
        assert!(repo.join(".git/upload-verified").exists());
        assert_eq!(
            git(repo, &["show", "HEAD:static/a.png"]),
            b"old tracked baseline"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&git(repo, &["show", "HEAD:manifest.json"])).unwrap();
        assert_eq!(
            manifest["assets"][0]["sha256"].as_str().unwrap(),
            std::fs::read_to_string(repo.join(".git/upload-verified")).unwrap()
        );
        // Materialization is still modified against its old baseline, but a
        // second daemon pass must not create another blob/reference commit.
        crate::sync::sync_repo(repo, &policy, &Default::default(), 0, None, false, None)
            .await
            .unwrap();
        assert_eq!(count(repo), 2);
        std::fs::write(repo.join("static/a.png"), "next generated version").unwrap();
        std::fs::write(repo.join(".git/refuse-upload"), "mock storage outage").unwrap();
        let result =
            crate::sync::sync_repo(repo, &policy, &Default::default(), 0, None, false, None)
                .await
                .unwrap();
        assert!(matches!(result, crate::sync::SyncOutcome::Blocked));
        assert_eq!(count(repo), 2);
        assert_eq!(
            std::fs::read_to_string(repo.join("static/a.png")).unwrap(),
            "next generated version"
        );
        std::fs::remove_file(repo.join(".git/refuse-upload")).unwrap();
        crate::sync::sync_repo(repo, &policy, &Default::default(), 0, None, false, None)
            .await
            .unwrap();
        assert_eq!(count(repo), 3);
        assert_eq!(
            git(repo, &["show", "HEAD:static/a.png"]),
            b"old tracked baseline"
        );
    }
}
