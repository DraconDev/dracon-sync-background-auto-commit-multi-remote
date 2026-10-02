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
            }
            Placement::Git => {
                if rule.backend.is_some() || rule.security.is_some() {
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
            reason: if self.policy.enabled {
                "no matching storage rule"
            } else {
                "external storage disabled"
            }
            .into(),
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum StorageCommand {
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
    use std::io::Write;
    use std::process::Stdio;
    let mut input = Vec::new();
    for path in paths {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            input.extend_from_slice(path.as_os_str().as_bytes());
        }
        #[cfg(not(unix))]
        input.extend_from_slice(
            path.to_str()
                .context("unsupported path encoding")?
                .as_bytes(),
        );
        input.push(0);
    }
    let mut command = crate::policy::std_git_command();
    command.current_dir(repo).env("GIT_OPTIONAL_LOCKS", "0");
    if let Some(index) = index {
        command
            .env("GIT_INDEX_FILE", index)
            .arg("--literal-pathspecs");
    }
    command.arg("check-attr");
    if index.is_some() {
        command.arg("--cached");
    }
    let mut child = command
        .args(["-z", "--stdin", "filter"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .context("missing attribute inventory stdin")?;
    // Drain stdout concurrently; large inventories otherwise deadlock full pipes.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output();
    let written = writer
        .join()
        .map_err(|_| anyhow::anyhow!("attribute input thread failed"))?;
    let output = output?;
    written?;
    if !output.status.success() {
        bail!("attribute inventory failed");
    }
    let pieces: Vec<&[u8]> = output.stdout.split(|b| *b == 0).collect();
    let pieces = if pieces.last() == Some(&&b""[..]) {
        &pieces[..pieces.len() - 1]
    } else {
        &pieces[..]
    };
    if pieces.len() % 3 != 0 {
        bail!("invalid attribute inventory response");
    }
    let mut result = BTreeMap::new();
    for triple in pieces.chunks_exact(3) {
        if triple[1] != b"filter" {
            bail!("unexpected attribute inventory response");
        }
        result.insert(
            path_from_bytes(triple[0])?,
            std::str::from_utf8(triple[2])
                .context("unsupported filter encoding")?
                .to_owned(),
        );
    }
    if result.len() != paths.len() || !paths.iter().all(|path| result.contains_key(path)) {
        bail!("incomplete attribute inventory response");
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

pub(crate) fn run(command: &StorageCommand) -> Result<()> {
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
        StorageCommand::Status { .. }
        | StorageCommand::FilterClean { .. }
        | StorageCommand::VerifyIndex { .. } => {
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
                "Storage policy valid. Read-only planning; transfers are not implemented yet."
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
    let index_path = std::env::var_os("GIT_INDEX_FILE")
        .map(PathBuf::from)
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
