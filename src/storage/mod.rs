//! External-storage planning and explicit networkless prepared clean transformation.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use globset::{GlobBuilder, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::policy::{RepoPolicyOverride, SyncPolicy};

/// Operator-owned bindings; repositories cannot supply endpoints or credentials.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum BackendBinding {
    Local {
        root: PathBuf,
        #[serde(default = "encrypted_only")]
        allowed_security: Vec<Security>,
    },
    S3 {
        endpoint: String,
        bucket: String,
        credential_ref: String,
        #[serde(default)]
        region: Option<String>,
        #[serde(default)]
        prefix: String,
        #[serde(default = "encrypted_only")]
        allowed_security: Vec<Security>,
    },
}

fn encrypted_only() -> Vec<Security> {
    vec![Security::WardenEncrypted]
}

impl BackendBinding {
    fn allowed_security(&self) -> &[Security] {
        match self {
            Self::Local {
                allowed_security, ..
            }
            | Self::S3 {
                allowed_security, ..
            } => allowed_security,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Placement {
    Git,
    External,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Security {
    NonSensitive,
    WardenEncrypted,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageRule {
    pub(crate) paths: Vec<String>,
    pub(crate) placement: Placement,
    #[serde(default)]
    pub(crate) min_bytes: Option<u64>,
    #[serde(default)]
    pub(crate) backend: Option<String>,
    /// Additional required logical copies; primary is always required too.
    #[serde(default)]
    pub(crate) required_copies: Vec<String>,
    #[serde(default)]
    pub(crate) security: Option<Security>,
}

/// An opt-in policy. No universal media threshold or extension list.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoragePolicy {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) backends: BTreeMap<String, BackendBinding>,
    #[serde(default)]
    pub(crate) rules: Vec<StorageRule>,
}

/// Overrides deliberately cannot define a backend or grant public publication.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageOverride {
    pub(crate) enabled: Option<bool>,
    pub(crate) rules: Option<Vec<StorageRule>>,
}

pub(crate) fn effective_policy(
    global: &StoragePolicy,
    repo: Option<&StorageOverride>,
) -> StoragePolicy {
    let mut resolved = global.clone();
    if let Some(repo) = repo {
        resolved.enabled = repo.enabled.unwrap_or(global.enabled);
        if let Some(rules) = &repo.rules {
            resolved.rules = rules.clone();
        }
    }
    resolved
}

fn compile_pattern(pattern: &str) -> Result<GlobMatcher> {
    if pattern.is_empty()
        || pattern.starts_with('/')
        || pattern.contains('\\')
        || pattern
            .split('/')
            .any(|part| part == ".." || part == ".git")
    {
        bail!("storage path pattern must be relative and exclude Git internals: {pattern:?}");
    }
    Ok(GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .with_context(|| format!("invalid storage pattern {pattern:?}"))?
        .compile_matcher())
}

/// Strict validation uses only configuration, without reading credentials/network.
pub(crate) fn validate_policy(policy: &StoragePolicy) -> Result<()> {
    for (name, backend) in &policy.backends {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            bail!("backend identifiers must contain only letters, digits, '-' or '_'");
        }
        match backend {
            BackendBinding::Local { root, .. } => {
                if !root.is_absolute() {
                    bail!("local backend {name:?} root must be absolute");
                }
            }
            BackendBinding::S3 {
                endpoint,
                bucket,
                credential_ref,
                ..
            } => {
                let url = reqwest::Url::parse(endpoint).context("invalid S3 endpoint")?;
                if url.scheme() != "https"
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    bail!("S3 backend {name:?} requires HTTPS without embedded credentials, query or fragment");
                }
                if bucket.is_empty() || credential_ref.is_empty() {
                    bail!("S3 backend {name:?} needs a bucket and operator credential reference");
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    for (index, rule) in policy.rules.iter().enumerate() {
        if rule.paths.is_empty() {
            bail!("storage rule {index} has no paths");
        }
        match rule.placement {
            Placement::External => {
                let backend = rule
                    .backend
                    .as_ref()
                    .context("external rule requires a backend binding")?;
                if !policy.backends.contains_key(backend) {
                    bail!("storage rule {index} selects unapproved backend {backend:?}");
                }
                let security = rule
                    .security
                    .context("external rule requires explicit security classification")?;
                if !policy.backends[backend]
                    .allowed_security()
                    .contains(&security)
                {
                    bail!("storage rule {index} selects a security class not approved for backend {backend:?}");
                }
                let mut copies = BTreeSet::new();
                for copy in &rule.required_copies {
                    if !copies.insert(copy) || copy == backend {
                        bail!("storage rule {index} repeats a required copy");
                    }
                    if !policy
                        .backends
                        .get(copy)
                        .is_some_and(|b| b.allowed_security().contains(&security))
                    {
                        bail!("storage rule {index} requires an unapproved copy/security class");
                    }
                }
            }
            Placement::Git => {
                if rule.backend.is_some()
                    || rule.security.is_some()
                    || !rule.required_copies.is_empty()
                {
                    bail!("Git rule {index} cannot set external backend/security");
                }
            }
        }
        for pattern in &rule.paths {
            compile_pattern(pattern)?;
            if !seen.insert((pattern.clone(), rule.min_bytes)) {
                bail!("duplicate storage condition at rule {index}: {pattern:?}");
            }
        }
    }
    Ok(())
}

struct CompiledPolicy {
    policy: StoragePolicy,
    matchers: Vec<Vec<GlobMatcher>>,
}

#[derive(Debug, Serialize)]
struct Decision {
    placement: Placement,
    rule: Option<usize>,
    backend: Option<String>,
    security: Option<Security>,
    required_copies: Vec<String>,
    reason: String,
}

impl CompiledPolicy {
    fn new(policy: StoragePolicy) -> Result<Self> {
        validate_policy(&policy)?;
        let matchers = policy
            .rules
            .iter()
            .map(|rule| {
                rule.paths
                    .iter()
                    .map(|p| compile_pattern(p))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { policy, matchers })
    }

    fn decide(&self, path: &Path, bytes: u64) -> Decision {
        if self.policy.enabled {
            for (index, (rule, matchers)) in
                self.policy.rules.iter().zip(&self.matchers).enumerate()
            {
                if rule.min_bytes.is_none_or(|min| bytes >= min)
                    && matchers.iter().any(|matcher| matcher.is_match(path))
                {
                    return Decision {
                        placement: rule.placement,
                        rule: Some(index),
                        backend: rule.backend.clone(),
                        security: rule.security,
                        required_copies: rule
                            .backend
                            .iter()
                            .cloned()
                            .chain(rule.required_copies.iter().cloned())
                            .collect(),
                        reason: "first matching path and size condition; proposed placement only"
                            .into(),
                    };
                }
            }
        }
        Decision {
            placement: Placement::Git,
            rule: None,
            backend: None,
            security: None,
            required_copies: Vec::new(),
            reason: if self.policy.enabled {
                "no matching storage rule"
            } else {
                "external storage disabled"
            }
            .into(),
        }
    }
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct RecoveryOptions {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    repo_id: String,
    #[arg(long)]
    metadata_root: PathBuf,
    #[arg(long)]
    manifest_path: PathBuf,
    #[arg(long)]
    path: PathBuf,
    #[arg(long, default_value = "HEAD")]
    revision: String,
    /// Operator configuration; repository overrides cannot define adapters.
    #[arg(long)]
    policy: Option<PathBuf>,
    /// Select an approved required copy; defaults to the declared primary.
    #[arg(long)]
    backend: Option<String>,
    #[arg(long)]
    restore_root: PathBuf,
    #[arg(long)]
    warden: Option<PathBuf>,
    #[arg(long)]
    identity_home: Option<PathBuf>,
    /// Explicit private operator credential directory; required for S3 recovery.
    #[arg(long)]
    credentials_root: Option<PathBuf>,
    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,
    #[arg(long, default_value_t = 2 * 1024 * 1024 * 1024)]
    max_payload_bytes: u64,
    #[arg(long, default_value_t = 1024 * 1024 * 1024)]
    max_output_bytes: u64,
    #[arg(long, default_value_t = 4 * 1024 * 1024 * 1024)]
    max_retained_bytes: u64,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct ProbeOptions {
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long)]
    policy: Option<PathBuf>,
    #[arg(long)]
    backend: String,
    #[arg(long)]
    credentials_root: PathBuf,
    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AdvanceOptions {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    repo_id: String,
    #[arg(long)]
    journal_root: PathBuf,
    #[arg(long)]
    job_id: String,
    #[arg(long)]
    policy: Option<PathBuf>,
    #[arg(long)]
    credentials_root: Option<PathBuf>,
    #[arg(long)]
    warden: Option<PathBuf>,
    #[arg(long)]
    identity_home: Option<PathBuf>,
    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,
    #[arg(long, default_value_t = 1024 * 1024 * 1024)]
    max_snapshot_bytes: u64,
    #[arg(long, default_value_t = 2 * 1024 * 1024 * 1024)]
    max_payload_bytes: u64,
    #[arg(long, default_value_t = 4 * 1024 * 1024 * 1024)]
    max_retained_snapshot_bytes: u64,
    #[arg(long, default_value_t = 8 * 1024 * 1024 * 1024)]
    max_retained_payload_bytes: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum StorageCommand {
    /// Capture a policy-selected new path; no upload, enrollment or Git mutation.
    Capture {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        repo_id: String,
        #[arg(long)]
        journal_root: PathBuf,
        #[arg(long)]
        policy: Option<PathBuf>,
        /// Required logical Git destinations for later preservation acknowledgment.
        #[arg(long, required = true)]
        git_target: Vec<String>,
        #[arg(long, default_value_t = 1024 * 1024 * 1024)]
        max_snapshot_bytes: u64,
        #[arg(long, default_value_t = 4 * 1024 * 1024 * 1024)]
        max_retained_snapshot_bytes: u64,
        #[arg(long)]
        json: bool,
        path: PathBuf,
    },
    /// Prepare/copy one captured version; stops before staging, committing or pushing.
    AdvanceJob(Box<AdvanceOptions>),
    /// Probe S3 write refusal/readback; retains one synthetic 64-byte control object.
    ProbeBackend(Box<ProbeOptions>),
    /// Recover an exact committed asset into a private cache; checkout is unchanged.
    RestoreAsset(Box<RecoveryOptions>),
    /// Hydrate one checked-out asset using a private retained transaction (Linux).
    Hydrate {
        #[command(flatten)]
        recovery: Box<RecoveryOptions>,
        #[arg(long)]
        hydration_root: PathBuf,
        /// Resume a matching retained local transaction without fetching/decrypting.
        #[arg(long)]
        resume_local: bool,
    },
    /// Import authenticated committed metadata into a private cold-recovery cache.
    ImportManifest {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        repo_id: String,
        #[arg(long)]
        metadata_root: PathBuf,
        #[arg(long)]
        manifest_path: PathBuf,
        #[arg(long, default_value = "HEAD")]
        revision: String,
        /// Operator-approved metadata import policy digest; never read from Git.
        #[arg(long)]
        policy_sha256: String,
        /// Absolute operator-selected Warden binary; never read from the manifest.
        #[arg(long)]
        warden: PathBuf,
        #[arg(long)]
        identity_home: Option<PathBuf>,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
    },
    /// Bind already validated storage to a commit guard; no filter/hook installation.
    SetupGuard {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        repo_id: String,
        #[arg(long)]
        metadata_root: PathBuf,
        #[arg(long)]
        manifest_path: PathBuf,
    },
    /// Validate the actual index using explicit local guard bindings.
    VerifyConfiguredIndex {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Check indexed enrolled references against locally verified protected metadata.
    /// Independent of Git's stat cache; no backend availability claim.
    VerifyIndex {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        repo_id: String,
        #[arg(long)]
        metadata_root: PathBuf,
        #[arg(long)]
        manifest_path: PathBuf,
    },
    /// Networkless Git clean driver selecting the exact indexed manifest version.
    /// Writes only a verified canonical pointer to stdout; does not install filters.
    FilterClean {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        repo_id: String,
        #[arg(long)]
        journal_root: PathBuf,
        #[arg(long)]
        metadata_root: PathBuf,
        /// Reserved repository-relative protected metadata path in the Git index.
        #[arg(long)]
        manifest_path: PathBuf,
        /// Exact repository-relative Git path (normally supplied as Git's %f).
        path: PathBuf,
    },
    /// Inspect durable job evidence without creating state or contacting backends.
    Status {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// State base directory; storage-journal is appended.
        #[arg(long)]
        state_dir: Option<PathBuf>,
        /// Explicit operator-bound repository ID; otherwise read local Git config.
        #[arg(long)]
        repo_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Inventory owned Git paths and explain proposed placement; no upload/staging.
    Plan {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Operator policy; omitted uses existing policy if present, otherwise defaults.
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Scan reachable blob bytes and object-store disk usage (can be slow).
        #[arg(long)]
        history: bool,
    },
    /// Validate effective storage rules and operator backend bindings, without I/O to backends.
    Validate {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

fn git_read(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = crate::policy::std_git_command()
        .current_dir(repo)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .context("cannot run Git inventory")?;
    if !output.status.success() {
        // Never expose arbitrary Git stderr, which may contain remote URLs/credentials.
        bail!("Git inventory command failed ({})", output.status);
    }
    Ok(output.stdout)
}

fn root(repo: &Path) -> Result<PathBuf> {
    let raw = git_read(repo, &["rev-parse", "--show-toplevel"])?;
    let raw = raw.strip_suffix(b"\n").unwrap_or(&raw);
    path_from_bytes(raw)?
        .canonicalize()
        .context("cannot resolve repository root")
}

fn path_from_bytes(raw: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(raw)))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(
            std::str::from_utf8(raw).context("non-UTF8 path unsupported on this platform")?,
        ))
    }
}

fn load_configuration(
    repo: &Path,
    explicit: Option<&Path>,
) -> Result<(SyncPolicy, RepoPolicyOverride)> {
    let global_path = explicit
        .map(PathBuf::from)
        .or_else(|| crate::policy::resolve_policy_path().ok());
    let global = match global_path {
        Some(path) => SyncPolicy::load(&path)?,
        None => toml::from_str::<SyncPolicy>("")?,
    };
    let repo_path = repo.join(".dracon/dracon-sync.toml");
    for component in [repo.join(".dracon"), repo_path.clone()] {
        match std::fs::symlink_metadata(&component) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("repo policy must not follow symlinks")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("cannot inspect repo policy"),
        }
    }
    let local = match std::fs::read_to_string(&repo_path) {
        Ok(text) => toml::from_str(&text).context("invalid repo policy; planning refused")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RepoPolicyOverride::default(),
        Err(e) => return Err(e).context("cannot read repo policy"),
    };
    Ok((global, local))
}

#[derive(Debug, Serialize)]
struct FilePlan {
    path: String,
    /// Lossless path bytes for filenames which cannot be represented as UTF-8.
    path_bytes_hex: Option<String>,
    tracked: bool,
    filter: Option<String>,
    bytes: Option<u64>,
    decision: Decision,
    concerns: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Plan {
    schema_version: u32,
    mode: &'static str,
    repository: PathBuf,
    storage_enabled: bool,
    transfers_available: bool,
    eligibility: &'static str,
    proposed_git_bytes: u64,
    proposed_external_bytes: u64,
    files: Vec<FilePlan>,
    history: Option<HistoryInventory>,
}

fn inventory_filters(repo: &Path, paths: &BTreeSet<PathBuf>) -> Result<BTreeMap<PathBuf, String>> {
    inventory_filters_at(repo, paths, None)
}

fn inventory_filters_at(
    repo: &Path,
    paths: &BTreeSet<PathBuf>,
    index: Option<&Path>,
) -> Result<BTreeMap<PathBuf, String>> {
    use std::process::Stdio;
    const MAX_INPUT: usize = 16 * 1024 * 1024;
    const MAX_PATHS: usize = 100_000;
    if paths.len() > MAX_PATHS {
        bail!("attribute inventory path budget exceeded");
    }
    let mut input = Vec::new();
    for path in paths {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let bytes = path.as_os_str().as_bytes();
            if bytes.len() >= MAX_INPUT.saturating_sub(input.len()) {
                bail!("attribute inventory input budget exceeded");
            }
            input.extend_from_slice(bytes);
        }
        #[cfg(not(unix))]
        {
            let bytes = path
                .to_str()
                .context("unsupported path encoding")?
                .as_bytes();
            if bytes.len() >= MAX_INPUT.saturating_sub(input.len()) {
                bail!("attribute inventory input budget exceeded");
            }
            input.extend_from_slice(bytes);
        }
        input.push(0);
    }
    let mut command = crate::policy::std_git_command();
    command.current_dir(repo).env("GIT_OPTIONAL_LOCKS", "0");
    if let Some(index) = index {
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|name| {
                name.starts_with("GIT_CONFIG_KEY_") || name.starts_with("GIT_CONFIG_VALUE_")
            }) {
                command.env_remove(name);
            }
        }
        command
            .env("GIT_INDEX_FILE", index)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .arg("--literal-pathspecs");
    }
    command.arg("check-attr");
    if index.is_some() {
        command.arg("--cached");
    }
    command
        .args(["-z", "--stdin", "filter"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = bounded_attribute_query(
        command.into_std(),
        input,
        32 * 1024 * 1024,
        std::time::Duration::from_secs(30),
    )?;
    parse_attribute_response(&output, paths)
}

/// Use a dedicated runtime thread because callers include synchronous CLI code
/// running inside Tokio and daemon spawn_blocking workers. All pipe operations
/// share one deadline; no writer/reader thread can outlive a failed query.
fn bounded_attribute_query(
    mut command: std::process::Command,
    input: Vec<u8>,
    output_budget: usize,
    timeout: std::time::Duration,
) -> Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    std::thread::spawn(move || -> Result<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async move {
            let mut child = tokio::process::Command::from(command)
                .kill_on_drop(true)
                .spawn()?;
            let pid = child.id();
            let mut stdin = child.stdin.take().context("missing attribute stdin")?;
            let stdout = child.stdout.take().context("missing attribute stdout")?;
            let exchange = async {
                let write = async {
                    stdin.write_all(&input).await?;
                    drop(stdin);
                    Ok::<_, anyhow::Error>(())
                };
                let read = async {
                    let mut output = Vec::new();
                    stdout
                        .take(output_budget as u64 + 1)
                        .read_to_end(&mut output)
                        .await?;
                    if output.len() > output_budget {
                        bail!("attribute inventory output budget exceeded");
                    }
                    Ok::<_, anyhow::Error>(output)
                };
                let wait = async { Ok::<_, anyhow::Error>(child.wait().await?) };
                let (_, output, status) = tokio::try_join!(write, read, wait)?;
                if !status.success() {
                    bail!("attribute inventory failed");
                }
                Ok(output)
            };
            let result = match tokio::time::timeout(timeout, exchange).await {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!("attribute inventory timed out")),
            };
            if result.is_err() {
                #[cfg(unix)]
                if let Some(pid) = pid {
                    // The child was created in its own process group. Kill that
                    // group even if the parent exited while descendants held pipes.
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                #[cfg(not(unix))]
                let _ = pid;
                let _ = child.start_kill();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            }
            result
        })
    })
    .join()
    .map_err(|_| anyhow::anyhow!("attribute query thread failed"))?
}

fn parse_attribute_response(
    output: &[u8],
    paths: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, String>> {
    if !output.is_empty() && !output.ends_with(&[0]) {
        bail!("unterminated attribute inventory response");
    }
    let mut pieces = output.split(|b| *b == 0);
    let mut result = BTreeMap::new();
    for _ in 0..paths.len() {
        let path = pieces.next().context("missing attribute path")?;
        let attribute = pieces.next().context("missing attribute name")?;
        let value = pieces.next().context("missing attribute value")?;
        if attribute != b"filter" || value.len() > 1024 {
            bail!("unexpected or oversized attribute inventory response");
        }
        let path = path_from_bytes(path)?;
        if !paths.contains(&path) || result.contains_key(&path) {
            bail!("unexpected or duplicate attribute inventory path");
        }
        result.insert(
            path,
            std::str::from_utf8(value)
                .context("unsupported filter encoding")?
                .to_owned(),
        );
    }
    if pieces.next() != Some(&b""[..]) || pieces.next().is_some() {
        bail!("extra attribute inventory response");
    }

    Ok(result)
}

fn inventory(
    repo: &Path,
    global: &SyncPolicy,
    local: &RepoPolicyOverride,
    policy: &CompiledPolicy,
) -> Result<Plan> {
    let tracked: BTreeSet<PathBuf> = git_read(repo, &["ls-files", "--cached", "-z"])?
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(path_from_bytes)
        .collect::<Result<_>>()?;
    let all: BTreeSet<PathBuf> = git_read(
        repo,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?
    .split(|b| *b == 0)
    .filter(|p| !p.is_empty())
    .map(path_from_bytes)
    .collect::<Result<_>>()?;
    let filters = inventory_filters(repo, &all)?;
    let exclusions = crate::exclude::excluded_dir_names_set(global);
    let auto_exclusions = local
        .auto_commit_exclude_patterns
        .as_ref()
        .unwrap_or(&global.auto_commit_exclude_patterns);
    let mut plan = Plan {
        schema_version: 1, mode: "read-only-placement-preview", repository: repo.to_owned(),
        storage_enabled: policy.policy.enabled,
        transfers_available: false,
        eligibility: "placement preview only; ownership, secret classification, filter compatibility and enrollment must pass before transfer",
        proposed_git_bytes: 0, proposed_external_bytes: 0, files: Vec::new(), history: None,
    };
    for path in all {
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!("Git returned a non-relative inventory path");
        }
        let mut concerns = Vec::new();
        let mut current = repo.to_owned();
        let mut safe = true;
        for part in path.components() {
            current.push(part.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    safe = false;
                    concerns.push("symlink: never traverse for external preservation".into());
                    break;
                }
                Ok(meta) if meta.is_dir() && current.join(".git").exists() => {
                    safe = false;
                    concerns.push("nested repository: apply its own storage policy".into());
                    break;
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    safe = false;
                    concerns.push("missing from working tree".into());
                    break;
                }
                Err(e) => return Err(e).context("cannot inspect inventory path"),
            }
        }
        let bytes = if safe {
            let metadata = std::fs::symlink_metadata(&current)?;
            if metadata.is_file() {
                Some(metadata.len())
            } else {
                concerns.push("not a regular payload file".into());
                None
            }
        } else {
            None
        };
        let decision = policy.decide(&path, bytes.unwrap_or(0));
        let excluded = crate::exclude::is_excluded_change_path(&path, &exclusions)
            || crate::exclude::is_excluded_file(&path, &global.exclude_file_patterns)
            || crate::exclude::matches_untracked_exclude(repo, &path, auto_exclusions)
            || (!tracked.contains(&path)
                && crate::exclude::matches_untracked_exclude(
                    repo,
                    &path,
                    &global.untracked_exclude_patterns,
                ));
        if decision.placement == Placement::External
            && filters
                .get(&path)
                .is_some_and(|filter| filter != "unspecified" && filter != "unset")
        {
            concerns.push(
                "existing Git filter: compatibility must be verified before external enrollment"
                    .into(),
            );
        }
        if excluded {
            concerns
                .push("excluded by existing Sync policy; placement does not override this".into());
        }
        if let Some(bytes) = bytes {
            if decision.placement == Placement::Git && bytes > global.max_stage_file_bytes {
                concerns.push("above existing Git staging limit".into());
            }
            if decision.placement == Placement::External && tracked.contains(&path) {
                concerns.push(
                    "tracked Git content: reviewed forward migration required; history remains"
                        .into(),
                );
            }
            if !excluded {
                match decision.placement {
                    Placement::Git => {
                        plan.proposed_git_bytes = plan.proposed_git_bytes.saturating_add(bytes)
                    }
                    Placement::External => {
                        plan.proposed_external_bytes =
                            plan.proposed_external_bytes.saturating_add(bytes)
                    }
                }
            }
        }
        #[cfg(unix)]
        let path_bytes_hex = if path.to_str().is_none() {
            use std::os::unix::ffi::OsStrExt;
            Some(
                path.as_os_str()
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            )
        } else {
            None
        };
        #[cfg(not(unix))]
        let path_bytes_hex = None;
        plan.files.push(FilePlan {
            path: path.to_string_lossy().into_owned(),
            path_bytes_hex,
            tracked: tracked.contains(&path),
            filter: filters.get(&path).cloned(),
            bytes,
            decision,
            concerns,
        });
    }
    Ok(plan)
}

#[derive(Debug, Serialize)]
struct HistoryInventory {
    reachable_blob_count: u64,
    reachable_raw_blob_bytes: u64,
    git_object_database_bytes: u64,
    scope: &'static str,
}

fn history_inventory(repo: &Path) -> Result<HistoryInventory> {
    use std::io::BufRead;
    use std::process::Stdio;
    let mut rev_list = crate::policy::std_git_command()
        .current_dir(repo)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["rev-list", "--objects", "--all", "--no-object-names"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let input = rev_list
        .stdout
        .take()
        .context("missing object inventory pipe")?;
    let spawn = crate::policy::std_git_command()
        .current_dir(repo)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["cat-file", "--batch-check=%(objecttype) %(objectsize)"])
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut cat = match spawn {
        Ok(cat) => cat,
        Err(error) => {
            let _ = rev_list.kill();
            let _ = rev_list.wait();
            return Err(error.into());
        }
    };
    let mut result = HistoryInventory {
        reachable_blob_count: 0, reachable_raw_blob_bytes: 0, git_object_database_bytes: 0,
        scope: "all local refs; unique reachable raw blobs; database disk includes unreachable objects and excludes nested gitdirs; not push bytes",
    };
    let read_result = (|| -> Result<()> {
        for line in
            std::io::BufReader::new(cat.stdout.take().context("missing blob inventory pipe")?)
                .lines()
        {
            let line = line?;
            let (kind, size) = line
                .split_once(' ')
                .context("invalid object inventory result")?;
            let bytes: u64 = size
                .parse()
                .context("missing or invalid object inventory")?;
            if kind == "blob" {
                result.reachable_blob_count += 1;
                result.reachable_raw_blob_bytes = result
                    .reachable_raw_blob_bytes
                    .checked_add(bytes)
                    .context("history byte overflow")?;
            }
        }
        Ok(())
    })();
    if read_result.is_err() {
        let _ = cat.kill();
        let _ = rev_list.kill();
    }
    let cat_status = cat.wait()?;
    let rev_status = rev_list.wait()?;
    read_result?;
    if !cat_status.success() || !rev_status.success() {
        bail!("history inventory failed");
    }
    let raw = git_read(repo, &["count-objects", "-v"])?;
    for line in std::str::from_utf8(&raw)?.lines() {
        if let Some(kib) = line
            .strip_prefix("size: ")
            .or_else(|| line.strip_prefix("size-pack: "))
        {
            let bytes = kib
                .parse::<u64>()?
                .checked_mul(1024)
                .context("database byte overflow")?;
            result.git_object_database_bytes = result
                .git_object_database_bytes
                .checked_add(bytes)
                .context("database byte overflow")?;
        }
    }
    Ok(result)
}

#[derive(Debug, Serialize)]
struct JournalStatus {
    schema_version: u32,
    mode: &'static str,
    transfers_available: bool,
    live_backend_verified: bool,
    healthy_records: bool,
    summary: dracon_sync::storage_core::journal::Summary,
}

fn journal_status(
    repo: &Path,
    state_dir: Option<&Path>,
    repo_id: Option<&str>,
) -> Result<JournalStatus> {
    use dracon_sync::storage_core::journal::{Journal, Summary};
    let id = if let Some(id) = repo_id {
        Some(id.to_owned())
    } else {
        let output = crate::policy::std_git_command()
            .current_dir(repo)
            .args(["config", "--local", "--get", "dracon.storageRepoId"])
            .output()?;
        if output.status.success() {
            Some(
                std::str::from_utf8(&output.stdout)
                    .context("invalid local storage repo ID encoding")?
                    .trim()
                    .to_owned(),
            )
        } else if output.status.code() == Some(1) {
            None
        } else {
            bail!("cannot read local storage repo ID");
        }
    };
    let summary = if let Some(id) = id {
        let base = match state_dir {
            Some(path) => path.to_owned(),
            None => {
                match std::env::var_os("DRACON_SYNC_STATE_DIR").filter(|value| !value.is_empty()) {
                    Some(path) => PathBuf::from(path),
                    None => dirs::home_dir()
                        .context("home not found")?
                        .join(".dracon/utilities/sync"),
                }
            }
        };
        Journal::inspect(&base.join("storage-journal"), &id)?
    } else {
        Summary::default()
    };
    Ok(JournalStatus {
        schema_version: 1,
        mode: "read-only-journal-evidence",
        transfers_available: false,
        live_backend_verified: false,
        healthy_records: summary.invalid_records == 0 && summary.failed_records == 0,
        summary,
    })
}

pub(crate) async fn run(command: &StorageCommand) -> Result<()> {
    if matches!(command, StorageCommand::Capture { .. }) {
        let command = command.clone();
        return tokio::task::spawn_blocking(move || capture_job(&command))
            .await
            .map_err(|_| anyhow::anyhow!("capture worker failed"))?;
    }
    if let StorageCommand::AdvanceJob(options) = command {
        let options = options.clone();
        return tokio::task::spawn_blocking(move || {
            futures::executor::block_on(advance_job(&options))
        })
        .await
        .map_err(|_| anyhow::anyhow!("transfer worker failed"))?;
    }
    if let StorageCommand::ProbeBackend(options) = command {
        let options = options.clone();
        return tokio::task::spawn_blocking(move || probe_backend(&options))
            .await
            .map_err(|_| anyhow::anyhow!("backend probe worker failed"))?;
    }
    if matches!(
        command,
        StorageCommand::RestoreAsset(_) | StorageCommand::Hydrate { .. }
    ) {
        return restore_asset(command).await;
    }
    if matches!(command, StorageCommand::ImportManifest { .. }) {
        return import_manifest(command).await;
    }
    if let StorageCommand::SetupGuard {
        repo,
        repo_id,
        metadata_root,
        manifest_path,
    } = command
    {
        setup_guard(repo, repo_id, metadata_root, manifest_path)?;
        println!(
            "Storage commit guard bound; hook/filter installation and transfers were not enabled."
        );
        return Ok(());
    }
    if let StorageCommand::VerifyConfiguredIndex { repo } = command {
        if !verify_configured_index(repo, true)? {
            bail!("repository has no configured storage guard");
        }
        return Ok(());
    }
    if let StorageCommand::VerifyIndex {
        repo,
        repo_id,
        metadata_root,
        manifest_path,
    } = command
    {
        let indexed = indexed_manifest(repo, repo_id, metadata_root, manifest_path)?;
        dracon_sync::storage_core::index::verify_manifest_entries(
            &indexed.repo,
            &indexed.index,
            &indexed.manifest,
        )?;
        verify_storage_attributes(&indexed)?;
        println!("Indexed storage references and required attributes match protected metadata; backends were not checked.");
        return Ok(());
    }
    if matches!(command, StorageCommand::FilterClean { .. }) {
        return filter_clean(command);
    }
    if let StorageCommand::Status {
        repo,
        state_dir,
        repo_id,
        json,
    } = command
    {
        let report = journal_status(&root(repo)?, state_dir.as_deref(), repo_id.as_deref())?;
        if *json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("Local storage journal: initialized={}, valid={}, invalid={}, failures={}, recorded preserved={}",
                report.summary.initialized, report.summary.records, report.summary.invalid_records,
                report.summary.failed_records, report.summary.recorded_preserved);
            println!(
                "Recorded evidence only; backends were not checked and transfers are not enabled."
            );
        }
        if !report.healthy_records {
            bail!("storage journal has unresolved concerns; records were preserved");
        }
        return Ok(());
    }
    let (repo, policy_path, json) = match command {
        StorageCommand::Capture { .. }
        | StorageCommand::AdvanceJob(_)
        | StorageCommand::ProbeBackend(_)
        | StorageCommand::Status { .. }
        | StorageCommand::FilterClean { .. }
        | StorageCommand::VerifyIndex { .. }
        | StorageCommand::SetupGuard { .. }
        | StorageCommand::VerifyConfiguredIndex { .. }
        | StorageCommand::ImportManifest { .. }
        | StorageCommand::RestoreAsset(_)
        | StorageCommand::Hydrate { .. } => {
            unreachable!("local command handled before policy resolution")
        }
        StorageCommand::Plan {
            repo, policy, json, ..
        }
        | StorageCommand::Validate { repo, policy, json } => (repo, policy.as_deref(), *json),
    };
    let repo = root(repo)?;
    let (global, local) = load_configuration(&repo, policy_path)?;
    let compiled = CompiledPolicy::new(effective_policy(&global.storage, local.storage.as_ref()))?;
    if matches!(command, StorageCommand::Validate { .. }) {
        if json {
            println!(
                "{}",
                serde_json::json!({"schema_version":1,"valid":true,"mode":"read-only","storage_enabled":compiled.policy.enabled,"transfers_available":false})
            );
        } else {
            println!(
                "Storage policy valid. Read-only planning; automatic transfers are not enabled."
            );
        }
        return Ok(());
    }
    let mut plan = inventory(&repo, &global, &local, &compiled)?;
    if matches!(command, StorageCommand::Plan { history: true, .. }) {
        plan.history = Some(history_inventory(&repo)?);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        println!(
            "Read-only placement preview for {} (no uploads or index changes)",
            repo.display()
        );
        println!("Proposed working-tree bytes: Git {}, external {}. These are not history or push-size estimates.", plan.proposed_git_bytes, plan.proposed_external_bytes);
        println!("{}", plan.eligibility);
        if let Some(history) = &plan.history {
            println!(
                "Reachable raw blobs: {} bytes ({} blobs); own Git object database: {} bytes. {}",
                history.reachable_raw_blob_bytes,
                history.reachable_blob_count,
                history.git_object_database_bytes,
                history.scope
            );
        }
        for file in &plan.files {
            println!(
                "{:?}\t{}\t{:?}\trule {:?}\t{}",
                file.decision.placement,
                file.path.escape_debug(),
                file.bytes,
                file.decision.rule,
                file.concerns.join("; ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn resolve_s3(
    binding: &BackendBinding,
    credentials_root: Option<&Path>,
    timeout_secs: u64,
) -> Result<dracon_sync::storage_core::s3::http::SignedHttpTransport> {
    use dracon_sync::storage_core::s3::{
        credentials,
        http::{HttpConfig, SignedHttpTransport},
    };
    let BackendBinding::S3 {
        endpoint,
        bucket,
        credential_ref,
        region,
        prefix,
        ..
    } = binding
    else {
        bail!("selected backend is not an S3 binding");
    };
    let region = region
        .as_ref()
        .context("approved S3 binding requires an explicit signing region")?;
    let root =
        credentials_root.context("S3 operation requires an explicit operator credentials root")?;
    SignedHttpTransport::new(
        HttpConfig {
            endpoint: endpoint.clone(),
            bucket: bucket.clone(),
            region: region.clone(),
            prefix: prefix.clone(),
            timeout: std::time::Duration::from_secs(timeout_secs),
        },
        credentials::load(root, credential_ref)?,
    )
}

fn probe_backend(options: &ProbeOptions) -> Result<()> {
    let repo = root(&options.repo)?;
    let (global, local) = load_configuration(&repo, options.policy.as_deref())?;
    if local.owned == Some(false) {
        bail!("repository opted out of Sync ownership");
    }
    CompiledPolicy::new(global.storage.clone())?;
    let binding = global
        .storage
        .backends
        .get(&options.backend)
        .context("selected backend lacks an operator binding")?;
    let verified = resolve_s3(
        binding,
        Some(&options.credentials_root),
        options.timeout_secs,
    )?
    .verify_conditional_writes()?;
    if options.json {
        println!(
            "{}",
            serde_json::json!({"schema_version":1,"backend":options.backend,
            "competing_creates_verified":true,"conflicting_write_refused":true,"full_readback_verified":true,
            "probe_object_retained":true,"probe_object_bytes":verified.probe_object().bytes(),
            "checked_at_unix":verified.verified_at_unix(),"live_provider_certified":false})
        );
    } else {
        println!("Backend {}: competing creates, conflicting-write refusal and exact readback verified; 64-byte control object retained.", options.backend);
    }
    Ok(())
}

fn capture_job(command: &StorageCommand) -> Result<()> {
    use dracon_sync::storage_core::{
        capture::select_source,
        journal::{encode_relative_path, Encryption, JobSpec, Journal, Limits, Phase},
    };
    use sha2::{Digest, Sha256};
    let StorageCommand::Capture {
        repo,
        repo_id,
        journal_root,
        policy,
        git_target,
        max_snapshot_bytes,
        max_retained_snapshot_bytes,
        json,
        path,
    } = command
    else {
        unreachable!("capture matched")
    };
    let repo = root(repo)?;
    // Validate confinement before any Git query or source access.
    let path_hex = encode_relative_path(path.as_os_str().as_encoded_bytes())?;
    let repository = git2::Repository::open(&repo)?;
    if repository
        .config()?
        .open_level(git2::ConfigLevel::Local)?
        .get_string("dracon.storageRepoId")
        .ok()
        .as_deref()
        != Some(repo_id.as_str())
    {
        bail!("capture repository binding does not match");
    }
    let (global, local) = load_configuration(&repo, policy.as_deref())?;
    if local.owned == Some(false) {
        bail!("repository opted out of Sync ownership");
    }
    let compiled = CompiledPolicy::new(effective_policy(&global.storage, local.storage.as_ref()))?;
    if !compiled.policy.enabled {
        bail!("external storage is disabled");
    }
    let index = repository.index()?;
    if index.get_path(path, 0).is_some() {
        bail!("tracked content requires verified enrollment or reviewed forward migration");
    }
    match repository.head() {
        Ok(head) => match head.peel_to_tree()?.get_path(path) {
            Ok(_) => bail!("historically tracked content requires reviewed forward migration"),
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(error.into()),
        },
        Err(error)
            if matches!(
                error.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) => {}
        Err(error) => return Err(error.into()),
    }
    let exclusions = crate::exclude::excluded_dir_names_set(&global);
    let auto = local
        .auto_commit_exclude_patterns
        .as_ref()
        .unwrap_or(&global.auto_commit_exclude_patterns);
    if repository.is_path_ignored(path)?
        || crate::exclude::is_excluded_change_path(path, &exclusions)
        || crate::exclude::is_excluded_file(path, &global.exclude_file_patterns)
        || crate::exclude::matches_untracked_exclude(&repo, path, auto)
        || crate::exclude::matches_untracked_exclude(
            &repo,
            path,
            &global.untracked_exclude_patterns,
        )
    {
        bail!("source excluded by existing Git/Sync policy");
    }
    let filters = inventory_filters(&repo, &BTreeSet::from([path.clone()]))?;
    if filters
        .get(path)
        .is_some_and(|filter| filter != "unspecified" && filter != "unset")
    {
        bail!("existing Git filter requires verified composition before enrollment");
    }
    let (mut source, fingerprint) = select_source(&repo, path, *max_snapshot_bytes)?;
    let decision = compiled.decide(path, fingerprint.bytes());
    if decision.placement != Placement::External {
        bail!("source is not selected for external preservation");
    }
    let mut copies = decision.required_copies.clone();
    copies.sort();
    let mut targets = git_target.clone();
    targets.sort();
    // Hash only the portable declared contract, never credentials/backend locations.
    let contract = serde_json::json!({"version":1,"rule":compiled.policy.rules[decision.rule.context("selected rule missing")?], "required_git_targets":targets});
    let spec = JobSpec {
        repo_id: repo_id.clone(),
        path_hex,
        source: fingerprint,
        policy_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&contract)?)),
        primary: decision.backend.context("selected primary missing")?,
        required_copies: copies,
        required_git_targets: targets,
        encryption: match decision.security.context("selected security missing")? {
            Security::NonSensitive => Encryption::None,
            Security::WardenEncrypted => Encryption::WardenAge,
        },
    };
    spec.validate()?;
    let journal = Journal::open(
        journal_root,
        repo_id,
        Limits {
            max_snapshot_bytes: *max_snapshot_bytes,
            max_retained_snapshot_bytes: *max_retained_snapshot_bytes,
            ..Limits::default()
        },
    )?;
    let job = journal.create(spec)?;
    let lease = journal.lease(job.id())?;
    let captured = match job.phase() {
        Phase::PendingCapture | Phase::Captured => lease.capture_snapshot(&mut source)?,
        Phase::Cancelled => bail!("selected version was cancelled"),
        _ => {
            lease.source_snapshot()?;
            lease.load()?
        }
    };
    if *json {
        println!(
            "{}",
            serde_json::json!({"schema_version":1,"job_id":captured.id(),"source_captured":true,"phase":captured.phase(),"git_changed":false,"uploaded":false})
        );
    } else {
        println!(
            "Job {}: exact source captured; no upload or Git change.",
            captured.id()
        );
    }
    Ok(())
}

async fn advance_job(options: &AdvanceOptions) -> Result<()> {
    use dracon_sync::storage_core::{
        backend::{ImmutableBackend, LocalBackend},
        bindings::{ApprovedBackend, CopyBindings},
        journal::{Encryption, Journal, Limits, Phase},
        s3::S3Backend,
        security::WardenAdapter,
        worker,
    };
    let repo = root(&options.repo)?;
    let repository = git2::Repository::open(&repo)?;
    if repository
        .config()?
        .open_level(git2::ConfigLevel::Local)?
        .get_string("dracon.storageRepoId")
        .ok()
        .as_deref()
        != Some(options.repo_id.as_str())
    {
        bail!("transfer repository binding does not match");
    }
    let journal = Journal::open(
        &options.journal_root,
        &options.repo_id,
        Limits {
            max_snapshot_bytes: options.max_snapshot_bytes,
            max_payload_bytes: options.max_payload_bytes,
            max_retained_snapshot_bytes: options.max_retained_snapshot_bytes,
            max_retained_payload_bytes: options.max_retained_payload_bytes,
            ..Limits::default()
        },
    )?;
    let lease = journal.lease(&options.job_id)?;
    let job = lease.load()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)?
        .as_secs();
    if !job.retry_eligible(now)
        || !matches!(
            job.phase(),
            Phase::Captured
                | Phase::Prepared
                | Phase::Uploading
                | Phase::PrimaryVerified
                | Phase::ReadyToStage
        )
    {
        bail!("selected job is not eligible for preparation/transfer");
    }
    let (global, local) = load_configuration(&repo, options.policy.as_deref())?;
    if local.owned == Some(false) {
        bail!("repository opted out of Sync ownership");
    }
    CompiledPolicy::new(global.storage.clone())?;
    let class = match job.spec().encryption {
        Encryption::None => Security::NonSensitive,
        Encryption::WardenAge => Security::WardenEncrypted,
    };
    // Validate the entire required copy set before credential/network/backend I/O.
    for id in &job.spec().required_copies {
        let binding = global
            .storage
            .backends
            .get(id)
            .context("required copy lacks an operator binding")?;
        if !binding.allowed_security().contains(&class) {
            bail!("required copy lacks the approved security class");
        }
    }
    let mut adapter = options
        .warden
        .as_ref()
        .map(|binary| {
            WardenAdapter::new(
                binary,
                &repo,
                &options.repo_id,
                std::time::Duration::from_secs(options.timeout_secs),
            )
        })
        .transpose()?;
    if let Some(home) = &options.identity_home {
        adapter = Some(
            adapter
                .take()
                .context("identity home requires a Warden executable")?
                .with_identity_home(home)?,
        );
    }
    if job.phase() == Phase::Captured
        && job.spec().encryption == Encryption::WardenAge
        && adapter.is_none()
    {
        bail!("captured encrypted job requires an approved Warden adapter");
    }
    // Reject damaged retained inputs before even publishing a capability control.
    lease.source_snapshot()?;
    if job.phase() != Phase::Captured {
        lease.payload_snapshot()?;
    }
    let mut adapters: BTreeMap<String, Box<dyn ImmutableBackend>> = BTreeMap::new();
    for id in &job.spec().required_copies {
        let binding = &global.storage.backends[id];
        let backend: Box<dyn ImmutableBackend> = match binding {
            BackendBinding::Local { root, .. } => Box::new(LocalBackend::open_existing(
                root,
                options.max_payload_bytes,
            )?),
            BackendBinding::S3 { .. } => Box::new(S3Backend::new(
                resolve_s3(
                    binding,
                    options.credentials_root.as_deref(),
                    options.timeout_secs,
                )?
                .verify_conditional_writes()?,
                options.max_payload_bytes,
            )?),
        };
        adapters.insert(id.clone(), backend);
    }
    let bindings = CopyBindings::new(
        options.repo_id.clone(),
        adapters
            .iter()
            .map(|(id, backend)| {
                let classes = global.storage.backends[id]
                    .allowed_security()
                    .iter()
                    .map(|class| match class {
                        Security::NonSensitive => Encryption::None,
                        Security::WardenEncrypted => Encryption::WardenAge,
                    })
                    .collect();
                Ok((
                    id.clone(),
                    ApprovedBackend::for_security(backend.as_ref(), classes)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?,
    )?;
    let ready = worker::advance(&lease, &bindings, adapter.as_ref(), now).await?;
    if options.json {
        println!(
            "{}",
            serde_json::json!({"schema_version":1,"job_id":ready.id(),"phase":"ready-to-stage",
        "required_copies_verified":true,"payload_bytes":ready.payload().context("prepared payload missing")?.bytes(),"git_changed":false})
        );
    } else {
        println!(
            "Job {}: required copies verified; ready to stage. Git unchanged.",
            ready.id()
        );
    }
    Ok(())
}

async fn restore_asset(command: &StorageCommand) -> Result<()> {
    let command = command.clone();
    tokio::task::spawn_blocking(move || futures::executor::block_on(restore_asset_inner(&command)))
        .await
        .map_err(|_| anyhow::anyhow!("recovery worker failed"))?
}

async fn restore_asset_inner(command: &StorageCommand) -> Result<()> {
    let (options, hydration_root, resume_local) = match command {
        StorageCommand::RestoreAsset(options) => (options.as_ref(), None, false),
        StorageCommand::Hydrate {
            recovery,
            hydration_root,
            resume_local,
        } => (recovery.as_ref(), Some(hydration_root), *resume_local),
        _ => unreachable!("recovery command was matched"),
    };
    let RecoveryOptions {
        repo,
        repo_id,
        metadata_root,
        manifest_path,
        path,
        revision,
        policy,
        backend,
        restore_root,
        warden,
        identity_home,
        credentials_root,
        timeout_secs,
        max_payload_bytes,
        max_output_bytes,
        max_retained_bytes,
    } = options;
    use dracon_sync::storage_core::{
        backend::{ImmutableBackend, LocalBackend},
        bindings::{ApprovedBackend, RestoreBinding},
        journal::{Encryption, Limits},
        metadata::{MetadataStore, MAX_PROTECTED_MANIFEST_BYTES},
        reference::{validate_sha256, Fingerprint, Pointer},
        restore::RestoreStore,
        security::WardenAdapter,
    };
    use sha2::{Digest, Sha256};
    validate_sha256(repo_id)?;
    let repo = repo.canonicalize()?;
    let repository = git2::Repository::open(&repo)?;
    if repository
        .workdir()
        .context("recovery requires a checkout")?
        .canonicalize()?
        != repo
        || repository
            .config()?
            .open_level(git2::ConfigLevel::Local)?
            .get_string("dracon.storageRepoId")
            .ok()
            .as_deref()
            != Some(repo_id.as_str())
    {
        bail!("recovery repository binding does not match");
    }
    #[cfg(unix)]
    let path_hex = {
        use std::os::unix::ffi::OsStrExt;
        dracon_sync::storage_core::journal::encode_relative_path(
            manifest_path.as_os_str().as_bytes(),
        )?;
        dracon_sync::storage_core::journal::encode_relative_path(path.as_os_str().as_bytes())?
    };
    #[cfg(not(unix))]
    let path_hex = {
        dracon_sync::storage_core::journal::encode_relative_path(
            manifest_path
                .to_str()
                .context("unsupported manifest path")?
                .as_bytes(),
        )?;
        dracon_sync::storage_core::journal::encode_relative_path(
            path.to_str().context("unsupported asset path")?.as_bytes(),
        )?
    };
    let commit = repository.revparse_single(revision)?.peel_to_commit()?;
    if hydration_root.is_some() {
        #[cfg(not(target_os = "linux"))]
        bail!("working-file hydration currently requires Linux");
        if repository.head()?.peel_to_commit()?.id() != commit.id() {
            bail!("hydrate requires the selected checked-out commit; use restore-asset for historical recovery");
        }
        verify_hydration_binding(&repo, repo_id, metadata_root, manifest_path)?;
        if resume_local {
            #[cfg(target_os = "linux")]
            {
                let root = hydration_root.context("hydration root missing")?;
                let hydrated = dracon_sync::storage_core::hydration::HydrationStore::open(root, repo_id, Limits {
                    max_snapshot_bytes: *max_output_bytes,
                    max_retained_snapshot_bytes: *max_retained_bytes,
                    ..Limits::default()
                })?.resume(&repository, manifest_path, path, |_| {
                    verify_hydration_binding(&repo, repo_id, metadata_root, manifest_path)
                }).with_context(|| format!("local hydration resume refused; retained transaction files preserved under {}", root.display()))?;
                println!("Verified local hydration at {}", hydrated.path().display());
                if let Some(backup) = hydrated.backup() {
                    println!("Original retained at {}", backup.display());
                }
                return Ok(());
            }
        }
    }
    let tree = commit.tree()?;
    let entry = tree.get_path(manifest_path)?;
    let (bytes, kind) = repository.odb()?.read_header(entry.id())?;
    if entry.filemode() != 0o100644
        || kind != git2::ObjectType::Blob
        || bytes as u64 > MAX_PROTECTED_MANIFEST_BYTES
    {
        bail!("invalid committed metadata mode or byte budget");
    }
    let blob = repository.find_blob(entry.id())?;
    let payload = Fingerprint::new(
        format!("{:x}", Sha256::digest(blob.content())),
        bytes as u64,
    )?;
    let metadata = MetadataStore::open(metadata_root, repo_id, Limits::default())?;
    let (prepared, manifest) = metadata.load_prepared_payload(&payload)?;
    let enrolled = manifest
        .enrollment(&path_hex)
        .context("asset is not enrolled in this committed version")?;
    let reference = tree.get_path(path)?;
    let (bytes, kind) = repository.odb()?.read_header(reference.id())?;
    if !matches!(reference.filemode(), 0o100644 | 0o100755)
        || kind != git2::ObjectType::Blob
        || bytes > 1024
    {
        bail!("committed asset is not an ordinary bounded reference");
    }
    let pointer = Pointer::parse(repository.find_blob(reference.id())?.content())?;
    if !enrolled.matches_pointer(&pointer) {
        bail!("committed reference and manifest disagree");
    }
    let selected = backend.as_deref().unwrap_or(&enrolled.primary);
    if !enrolled.required_copies.iter().any(|id| id == selected) {
        bail!("selected copy is not required by this enrollment");
    }
    let (global, _) = load_configuration(&repo, policy.as_deref())?;
    CompiledPolicy::new(global.storage.clone())?;
    let binding = global
        .storage
        .backends
        .get(selected)
        .context("selected copy lacks an operator backend binding")?;
    let (backend_adapter, allowed_security): (Box<dyn ImmutableBackend>, _) = match binding {
        BackendBinding::Local {
            root,
            allowed_security,
        } => (
            Box::new(LocalBackend::open_existing(root, *max_payload_bytes)?),
            allowed_security,
        ),
        BackendBinding::S3 {
            allowed_security, ..
        } => {
            use dracon_sync::storage_core::s3::S3Backend;
            (
                Box::new(S3Backend::new(
                    resolve_s3(binding, credentials_root.as_deref(), *timeout_secs)?,
                    *max_payload_bytes,
                )?),
                allowed_security,
            )
        }
    };
    let granted = ApprovedBackend::for_security(
        backend_adapter.as_ref(),
        allowed_security
            .iter()
            .map(|class| match class {
                Security::NonSensitive => Encryption::None,
                Security::WardenEncrypted => Encryption::WardenAge,
            })
            .collect(),
    )?;
    let binding = RestoreBinding::new(repo_id.clone(), selected.into(), granted)?;
    if identity_home.is_some() && warden.is_none() {
        bail!("identity home requires a selected Warden executable");
    }
    let mut adapter = warden
        .as_ref()
        .map(|binary| {
            WardenAdapter::new(
                binary,
                &repo,
                repo_id,
                std::time::Duration::from_secs(*timeout_secs),
            )
        })
        .transpose()?;
    if let Some(home) = identity_home {
        adapter = Some(
            adapter
                .take()
                .context("selected Warden executable missing")?
                .with_identity_home(home)?,
        );
    }
    let restored = RestoreStore::open(
        restore_root,
        repo_id,
        Limits {
            max_payload_bytes: *max_payload_bytes,
            max_snapshot_bytes: *max_output_bytes,
            max_retained_snapshot_bytes: *max_retained_bytes,
            ..Limits::default()
        },
    )?
    .recover(
        &metadata,
        &prepared,
        &manifest,
        &path_hex,
        &binding,
        adapter.as_ref(),
    )
    .await?;
    if let Some(hydration_root) = hydration_root {
        #[cfg(target_os = "linux")]
        {
            let hydrated = dracon_sync::storage_core::hydration::HydrationStore::open(hydration_root, repo_id, Limits {
                max_snapshot_bytes: *max_output_bytes,
                max_retained_snapshot_bytes: *max_retained_bytes,
                ..Limits::default()
            })?.hydrate(&repository, &restored, manifest_path, commit.id(), |_| {
                verify_hydration_binding(&repo, repo_id, metadata_root, manifest_path)
            }).with_context(|| format!("hydration refused; retained recovery/transaction files preserved under {} and {}", restore_root.display(), hydration_root.display()))?;
            println!(
                "Verified hydration: {} bytes at {}",
                restored.bytes(),
                hydrated.path().display()
            );
            if let Some(backup) = hydrated.backup() {
                println!("Original retained at {}", backup.display());
            }
            return Ok(());
        }
    }
    println!(
        "Verified recovery: {} bytes at {}",
        restored.bytes(),
        restored.path().display()
    );
    Ok(())
}

fn verify_hydration_binding(
    repo: &Path,
    repo_id: &str,
    metadata_root: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let binding = configured_guard(repo, false)?
        .context("hydrate requires an explicitly configured storage guard")?;
    if binding.repo_id != repo_id
        || binding.metadata_root.canonicalize()? != metadata_root.canonicalize()?
        || binding.manifest_path != manifest_path
    {
        bail!("hydration arguments and configured storage guard disagree");
    }
    if !verify_configured_index(repo, false)? {
        bail!("hydrate requires the storage guard binding");
    }
    Ok(())
}

async fn import_manifest(command: &StorageCommand) -> Result<()> {
    let StorageCommand::ImportManifest {
        repo,
        repo_id,
        metadata_root,
        manifest_path,
        revision,
        policy_sha256,
        warden,
        identity_home,
        timeout_secs,
    } = command
    else {
        bail!("expected explicit import bindings");
    };
    use dracon_sync::storage_core::{
        journal::Limits,
        metadata::{MetadataStore, MAX_PROTECTED_MANIFEST_BYTES},
        reference::{validate_sha256, Fingerprint},
        security::WardenAdapter,
    };
    use sha2::{Digest, Sha256};
    validate_sha256(repo_id)?;
    validate_sha256(policy_sha256)?;
    let repo = repo.canonicalize()?;
    let repository = git2::Repository::open(&repo)?;
    if repository
        .workdir()
        .context("manifest import requires a checkout")?
        .canonicalize()?
        != repo
        || repository
            .config()?
            .open_level(git2::ConfigLevel::Local)?
            .get_string("dracon.storageRepoId")
            .ok()
            .as_deref()
            != Some(repo_id.as_str())
    {
        bail!("manifest import repository binding does not match");
    }
    if !metadata_root.is_absolute() {
        bail!("absolute private metadata root required");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        dracon_sync::storage_core::journal::encode_relative_path(
            manifest_path.as_os_str().as_bytes(),
        )?;
    }
    #[cfg(not(unix))]
    dracon_sync::storage_core::journal::encode_relative_path(
        manifest_path
            .to_str()
            .context("unsupported manifest path")?
            .as_bytes(),
    )?;
    let commit = repository.revparse_single(revision)?.peel_to_commit()?;
    let tree = commit.tree()?;
    let entry = tree.get_path(manifest_path)?;
    if entry.filemode() != 0o100644 {
        bail!("invalid committed metadata mode");
    }
    let (bytes, kind) = repository.odb()?.read_header(entry.id())?;
    if kind != git2::ObjectType::Blob || bytes as u64 > MAX_PROTECTED_MANIFEST_BYTES {
        bail!("committed metadata exceeds budget");
    }
    let blob = repository.find_blob(entry.id())?;
    let payload = Fingerprint::new(
        format!("{:x}", Sha256::digest(blob.content())),
        bytes as u64,
    )?;
    let mut adapter = WardenAdapter::new(
        warden,
        &repo,
        repo_id,
        std::time::Duration::from_secs(*timeout_secs),
    )?;
    if let Some(home) = identity_home {
        adapter = adapter.with_identity_home(home)?;
    }
    let store = MetadataStore::open(metadata_root, repo_id, Limits::default())?;
    let (_, manifest) = store
        .import(&mut blob.content(), &payload, policy_sha256, &adapter)
        .await?;
    #[cfg(unix)]
    let manifest_hex = {
        use std::os::unix::ffi::OsStrExt;
        dracon_sync::storage_core::journal::encode_relative_path(
            manifest_path.as_os_str().as_bytes(),
        )?
    };
    #[cfg(not(unix))]
    let manifest_hex = dracon_sync::storage_core::journal::encode_relative_path(
        manifest_path.to_str().unwrap().as_bytes(),
    )?;
    if manifest.enrollment(&manifest_hex).is_some() {
        bail!("metadata path cannot be an enrolled asset");
    }
    println!("Authenticated committed manifest imported into private cache; assets, filters, guard bindings and transfers were not changed.");
    Ok(())
}

fn filter_clean(command: &StorageCommand) -> Result<()> {
    let StorageCommand::FilterClean {
        repo,
        repo_id,
        journal_root,
        metadata_root,
        manifest_path,
        path,
    } = command
    else {
        bail!("expected explicit clean bindings");
    };
    use dracon_sync::storage_core::{
        clean::PreparedClean,
        journal::{Journal, Limits},
    };
    if !journal_root.is_absolute() || !journal_root.join(repo_id).is_dir() {
        bail!("clean driver requires an existing absolute private journal binding");
    }
    let indexed = indexed_manifest(repo, repo_id, metadata_root, manifest_path)?;
    #[cfg(unix)]
    let path_hex = {
        use std::os::unix::ffi::OsStrExt;
        dracon_sync::storage_core::journal::encode_relative_path(path.as_os_str().as_bytes())?
    };
    #[cfg(not(unix))]
    let path_hex = dracon_sync::storage_core::journal::encode_relative_path(
        path.to_str()
            .context("unsupported path encoding")?
            .as_bytes(),
    )?;
    let journal = Journal::open(journal_root, repo_id, Limits::default())?;
    let lease = journal.lease_enrolled(&indexed.manifest, &path_hex)?;
    let clean = PreparedClean::new(
        &indexed.store,
        &indexed.prepared,
        &indexed.manifest,
        &path_hex,
        &lease,
    )?;
    clean.clean(&mut std::io::stdin().lock(), &mut std::io::stdout().lock())
}

struct IndexedManifest {
    repo: git2::Repository,
    index: git2::Index,
    index_path: PathBuf,
    store: dracon_sync::storage_core::metadata::MetadataStore,
    prepared: dracon_sync::storage_core::metadata::PreparedMetadata,
    manifest: dracon_sync::storage_core::manifest::Manifest,
}

fn indexed_manifest(
    repo: &Path,
    repo_id: &str,
    metadata_root: &Path,
    manifest_path: &Path,
) -> Result<IndexedManifest> {
    indexed_manifest_at(repo, repo_id, metadata_root, manifest_path, None)
}

fn indexed_manifest_at(
    repo: &Path,
    repo_id: &str,
    metadata_root: &Path,
    manifest_path: &Path,
    explicit_index: Option<PathBuf>,
) -> Result<IndexedManifest> {
    use dracon_sync::storage_core::{journal::Limits, metadata::MetadataStore};
    dracon_sync::storage_core::reference::validate_sha256(repo_id)?;
    let repository = git2::Repository::open(repo)?;
    let workdir = repository
        .workdir()
        .context("clean driver requires a worktree")?;
    if workdir.canonicalize()? != repo.canonicalize()?
        || repository
            .config()?
            .open_level(git2::ConfigLevel::Local)?
            .get_string("dracon.storageRepoId")
            .ok()
            .as_deref()
            != Some(repo_id)
    {
        bail!("clean driver repository binding does not match");
    }
    if !metadata_root.is_absolute() || !metadata_root.join(repo_id).is_dir() {
        bail!("existing absolute private metadata binding required");
    }
    #[cfg(unix)]
    let manifest_hex = {
        use std::os::unix::ffi::OsStrExt;
        dracon_sync::storage_core::journal::encode_relative_path(
            manifest_path.as_os_str().as_bytes(),
        )?
    };
    #[cfg(not(unix))]
    let manifest_hex = dracon_sync::storage_core::journal::encode_relative_path(
        manifest_path
            .to_str()
            .context("unsupported path encoding")?
            .as_bytes(),
    )?;
    let index_path = explicit_index
        .or_else(|| std::env::var_os("GIT_INDEX_FILE").map(PathBuf::from))
        .unwrap_or_else(|| repository.path().join("index"));
    let index_path = if index_path.is_absolute() {
        index_path
    } else {
        std::env::current_dir()?.join(index_path)
    };
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(&index_path)?;
    let info = file.metadata()?;
    if !info.is_file() || info.len() > 64 * 1024 * 1024 {
        bail!("clean driver index unavailable or exceeds budget");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if info.uid() != unsafe { libc::geteuid() } {
            bail!("clean driver index owner mismatch");
        }
    }
    let index = git2::Index::open(&index_path)?;
    if index.has_conflicts() {
        bail!("clean driver requires a resolved index");
    }
    let entry = index
        .get_path(manifest_path, 0)
        .context("matching protected metadata must already be staged")?;
    if entry.mode != 0o100644 {
        bail!("invalid protected metadata index mode");
    }
    let (size, kind) = repository.odb()?.read_header(entry.id)?;
    if kind != git2::ObjectType::Blob
        || size as u64 > dracon_sync::storage_core::metadata::MAX_PROTECTED_MANIFEST_BYTES
    {
        bail!("protected metadata exceeds budget");
    }
    let blob = repository.find_blob(entry.id)?;
    use sha2::{Digest, Sha256};
    let payload = dracon_sync::storage_core::reference::Fingerprint::new(
        format!("{:x}", Sha256::digest(blob.content())),
        blob.size() as u64,
    )?;
    drop(blob);
    let store = MetadataStore::open(metadata_root, repo_id, Limits::default())?;
    let (prepared, manifest) = store.load_prepared_payload(&payload)?;
    if manifest.enrollment(&manifest_hex).is_some() {
        bail!("metadata path cannot be an enrolled asset");
    }
    Ok(IndexedManifest {
        repo: repository,
        index,
        index_path,
        store,
        prepared,
        manifest,
    })
}

fn verify_storage_attributes(indexed: &IndexedManifest) -> Result<()> {
    if indexed
        .repo
        .config()?
        .open_level(git2::ConfigLevel::Local)?
        .get_bool("filter.dracon-storage.required")
        .ok()
        != Some(true)
    {
        bail!("storage clean driver must be locally required");
    }
    let mut paths = BTreeSet::new();
    for entry in indexed.index.iter() {
        if matches!(entry.mode, 0o100644 | 0o100755) {
            paths.insert(path_from_bytes(&entry.path)?);
        }
    }
    let mut enrolled = BTreeSet::new();
    for entry in indexed.manifest.enrollments() {
        let path = path_from_bytes(&decode_manifest_path(&entry.path_hex)?)?;
        paths.insert(path.clone());
        enrolled.insert(path);
    }
    let root = indexed
        .repo
        .workdir()
        .context("storage verification requires a worktree")?;
    let filters = inventory_filters_at(root, &paths, Some(&indexed.index_path))?;
    for (path, filter) in filters {
        if (filter == "dracon-storage") != enrolled.contains(&path) {
            bail!("indexed storage attributes and enrollments disagree");
        }
    }
    Ok(())
}

fn decode_manifest_path(hex: &str) -> Result<Vec<u8>> {
    // The private manifest codec has already validated these confined hex paths.
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

const GUARD_VERSION_KEY: &str = "dracon.storageGuardVersion";
const GUARD_METADATA_KEY: &str = "dracon.storageMetadataRoot";
const GUARD_MANIFEST_KEY: &str = "dracon.storageManifestPath";
const GUARD_EXECUTABLE_KEY: &str = "dracon.storageSyncExecutable";

fn verify_guard_entries(indexed: &IndexedManifest) -> Result<()> {
    dracon_sync::storage_core::index::verify_manifest_entries(
        &indexed.repo,
        &indexed.index,
        &indexed.manifest,
    )?;
    verify_storage_attributes(indexed)
}

struct GuardBinding {
    repo_id: String,
    metadata_root: PathBuf,
    manifest_path: PathBuf,
}

/// Portable declarations survive cloning without any local filter settings.
/// A declaration reserves storage routing even if no current file matches it;
/// local overrides do not authorize dropping the preservation contract.
fn portable_storage_declaration(
    repository: &git2::Repository,
    honor_git_index: bool,
) -> Result<bool> {
    fn is_attributes(path: &[u8]) -> bool {
        path == b".gitattributes" || path.ends_with(b"/.gitattributes")
    }
    fn declares(bytes: &[u8]) -> bool {
        bytes.split(|byte| *byte == b'\n').any(|line| {
            let line = line.trim_ascii();
            if line.is_empty() || line.starts_with(b"#") {
                return false;
            }
            // Skip Git's pattern (possibly C-quoted); inspect attribute tokens,
            // not filenames or comments containing the driver's name.
            let mut offset = 0;
            let quoted = line[0] == b'"';
            if quoted {
                offset = 1;
            }
            while offset < line.len() {
                match line[offset] {
                    b'\\' if quoted => offset = (offset + 2).min(line.len()),
                    b'"' if quoted => {
                        offset += 1;
                        break;
                    }
                    byte if !quoted && byte.is_ascii_whitespace() => break,
                    _ => offset += 1,
                }
            }
            line[offset..]
                .split(|byte| byte.is_ascii_whitespace())
                .any(|token| token == b"filter=dracon-storage")
        })
    }
    let mut attribute_bytes = 0usize;
    let mut inspect = |oid: git2::Oid, mode: u32| -> Result<bool> {
        if mode != 0o100644 && mode != 0o100755 {
            return Ok(false);
        }
        let (size, kind) = repository.odb()?.read_header(oid)?;
        attribute_bytes = attribute_bytes
            .checked_add(size)
            .context("attribute byte count overflow")?;
        if kind != git2::ObjectType::Blob
            || size > 4 * 1024 * 1024
            || attribute_bytes > 16 * 1024 * 1024
        {
            bail!("portable storage attribute detection exceeds byte budget");
        }
        Ok(declares(repository.find_blob(oid)?.content()))
    };
    let selected = if honor_git_index {
        std::env::var_os("GIT_INDEX_FILE").map(PathBuf::from)
    } else {
        None
    };
    let index_path = selected.unwrap_or_else(|| repository.path().join("index"));
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    match options.open(&index_path) {
        Ok(file) => {
            let info = file.metadata()?;
            if !info.is_file() || info.len() > 64 * 1024 * 1024 {
                bail!("portable storage detection index unavailable or exceeds budget");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if info.uid() != unsafe { libc::geteuid() } {
                    bail!("portable storage detection index owner mismatch");
                }
            }
            for entry in git2::Index::open(&index_path)?.iter() {
                if is_attributes(&entry.path) && inspect(entry.id, entry.mode)? {
                    return Ok(true);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("cannot inspect portable storage index"),
    }
    let head = match repository.head() {
        Ok(head) => head.peel_to_tree()?,
        Err(error)
            if matches!(
                error.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            return Ok(false)
        }
        Err(error) => return Err(error).context("cannot inspect portable storage HEAD"),
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut outcome = Ok(false);
    let traversal = head.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
        if std::time::Instant::now() >= deadline {
            outcome = Err(anyhow::anyhow!(
                "portable storage HEAD detection deadline exceeded"
            ));
            return git2::TreeWalkResult::Abort;
        }
        if entry.name_bytes() == b".gitattributes" {
            match inspect(entry.id(), entry.filemode() as u32) {
                Ok(false) => {}
                result => {
                    outcome = result;
                    return git2::TreeWalkResult::Abort;
                }
            }
        }
        git2::TreeWalkResult::Ok
    });
    match outcome {
        Ok(false) => {
            traversal?;
            Ok(false)
        }
        result => result,
    }
}

/// Direct daemon and manual-hook entrypoint. No storage marker means no change.
/// The daemon passes false to inspect its actual libgit2 index, ignoring an
/// ambient alternate-index environment. Manual Git hooks pass true for Git's index.
fn configured_guard(repo: &Path, honor_git_index: bool) -> Result<Option<GuardBinding>> {
    let repository = git2::Repository::open(repo)?;
    let local = repository.config()?.open_level(git2::ConfigLevel::Local)?;
    let version = match local.get_string(GUARD_VERSION_KEY) {
        Ok(version) => Some(version),
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(error) => return Err(error).context("cannot read storage guard version"),
    };
    let effective = repository.config()?;
    let mut driver_present = false;
    for key in [
        "filter.dracon-storage.clean",
        "filter.dracon-storage.process",
        "filter.dracon-storage.required",
    ] {
        match effective.get_entry(key) {
            Ok(_) => driver_present = true,
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(error).context("cannot read storage driver setting"),
        }
    }
    if version.is_none() && !driver_present {
        if !portable_storage_declaration(&repository, honor_git_index)? {
            return Ok(None);
        }
        bail!("committed or staged storage attributes require an explicit version-1 guard binding");
    }
    if version.as_deref() != Some("1") {
        bail!("storage driver requires an explicit version-1 guard binding");
    }
    let repo_id = local
        .get_string("dracon.storageRepoId")
        .context("storage guard repo identity missing")?;
    let metadata_root = PathBuf::from(
        local
            .get_string(GUARD_METADATA_KEY)
            .context("storage guard metadata binding missing")?,
    );
    let manifest_path = PathBuf::from(
        local
            .get_string(GUARD_MANIFEST_KEY)
            .context("storage guard manifest binding missing")?,
    );
    let executable = PathBuf::from(
        local
            .get_string(GUARD_EXECUTABLE_KEY)
            .context("storage guard executable binding missing")?,
    );
    if !executable.is_absolute() || !executable.is_file() {
        bail!("storage guard executable unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if executable.metadata()?.permissions().mode() & 0o111 == 0 {
            bail!("storage guard executable is not executable");
        }
    }
    Ok(Some(GuardBinding {
        repo_id,
        metadata_root,
        manifest_path,
    }))
}

pub(crate) fn verify_configured_index(repo: &Path, honor_git_index: bool) -> Result<bool> {
    let Some(binding) = configured_guard(repo, honor_git_index)? else {
        return Ok(false);
    };
    let explicit_index = if honor_git_index {
        None
    } else {
        Some(git2::Repository::open(repo)?.path().join("index"))
    };
    let indexed = indexed_manifest_at(
        repo,
        &binding.repo_id,
        &binding.metadata_root,
        &binding.manifest_path,
        explicit_index,
    )?;
    verify_guard_entries(&indexed)?;
    Ok(true)
}

/// Commit the verified immutable tree under an owned Git index lock.
/// Returns false only for a repository without storage markers. No fallback
/// bypasses validation; callers retain the ordinary path solely for that case.
pub(crate) fn commit_configured_storage(repo: &Path, message: &str) -> Result<bool> {
    let Some(binding) = configured_guard(repo, false)? else {
        return Ok(false);
    };
    let repository = git2::Repository::open(repo)?;
    let _lock =
        dracon_sync::storage_core::index::CommitLock::acquire(&repository, &binding.repo_id)?;
    let mut indexed = indexed_manifest_at(
        repo,
        &binding.repo_id,
        &binding.metadata_root,
        &binding.manifest_path,
        Some(repository.path().join("index")),
    )?;
    verify_guard_entries(&indexed)?;
    let tree_id = indexed.index.write_tree_to(&repository)?;
    let tree = repository.find_tree(tree_id)?;
    let signature = repository.signature()?;
    let parent = match repository.head() {
        Ok(head) => Some(head.peel_to_commit()?),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
        Err(error) => return Err(error).context("cannot determine guarded commit parent"),
    };
    let parents: Vec<_> = parent.iter().collect();
    repository.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )?;
    Ok(true)
}

fn setup_guard(
    repo: &Path,
    repo_id: &str,
    metadata_root: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let indexed = indexed_manifest_at(
        repo,
        repo_id,
        metadata_root,
        manifest_path,
        Some(git2::Repository::open(repo)?.path().join("index")),
    )?;
    verify_guard_entries(&indexed)?;
    let executable = std::env::current_exe()?.canonicalize()?;
    let metadata_root = metadata_root.canonicalize()?;
    let values = [
        (
            GUARD_METADATA_KEY,
            metadata_root
                .to_str()
                .context("guard metadata binding must be UTF-8")?,
        ),
        (
            GUARD_MANIFEST_KEY,
            manifest_path
                .to_str()
                .context("guard manifest binding must be UTF-8")?,
        ),
        (
            GUARD_EXECUTABLE_KEY,
            executable
                .to_str()
                .context("guard executable binding must be UTF-8")?,
        ),
    ];
    let mut local = indexed
        .repo
        .config()?
        .open_level(git2::ConfigLevel::Local)?;
    match local.get_string(GUARD_VERSION_KEY) {
        Ok(version) if version != "1" => {
            bail!("unknown guard version requires explicit maintenance")
        }
        Ok(_) => {
            if values
                .iter()
                .any(|(key, value)| local.get_string(key).ok().as_deref() != Some(*value))
            {
                bail!("existing guard binding differs; explicit rebinding maintenance required");
            }
            return Ok(());
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(error).context("cannot read existing storage guard binding"),
    }
    // Publish the activation marker last. A crash cannot expose an enabled guard
    // with newly missing fields; an existing driver already fails closed meanwhile.
    for (key, value) in values {
        local.set_str(key, value)?;
    }
    local.set_str(GUARD_VERSION_KEY, "1")?;
    Ok(())
}
