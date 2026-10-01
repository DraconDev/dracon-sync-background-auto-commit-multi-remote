//! External-storage planning. All operations in this milestone are read-only.

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
    },
    S3 {
        endpoint: String,
        bucket: String,
        credential_ref: String,
    },
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
            BackendBinding::Local { root } => {
                if !root.is_absolute() {
                    bail!("local backend {name:?} root must be absolute");
                }
            }
            BackendBinding::S3 {
                endpoint,
                bucket,
                credential_ref,
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
                if rule.security.is_none() {
                    bail!("storage rule {index} requires explicit security classification");
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
    /// Inventory owned Git paths and explain proposed placement; no upload/staging.
    Plan {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Operator policy; omitted uses existing policy if present, otherwise defaults.
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        json: bool,
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
    eligibility: &'static str,
    proposed_git_bytes: u64,
    proposed_external_bytes: u64,
    files: Vec<FilePlan>,
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
    let exclusions = crate::exclude::excluded_dir_names_set(global);
    let auto_exclusions = local
        .auto_commit_exclude_patterns
        .as_ref()
        .unwrap_or(&global.auto_commit_exclude_patterns);
    let mut plan = Plan {
        schema_version: 1, mode: "read-only-placement-preview", repository: repo.to_owned(),
        storage_enabled: policy.policy.enabled,
        eligibility: "placement preview only; ownership, secret classification, filter compatibility and enrollment must pass before transfer",
        proposed_git_bytes: 0, proposed_external_bytes: 0, files: Vec::new(),
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
            bytes,
            decision,
            concerns,
        });
    }
    Ok(plan)
}

pub(crate) fn run(command: &StorageCommand) -> Result<()> {
    let (repo, policy_path, json) = match command {
        StorageCommand::Plan { repo, policy, json }
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
    let plan = inventory(&repo, &global, &local, &compiled)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        println!(
            "Read-only placement preview for {} (no uploads or index changes)",
            repo.display()
        );
        println!("Proposed working-tree bytes: Git {}, external {}. These are not history or push-size estimates.", plan.proposed_git_bytes, plan.proposed_external_bytes);
        println!("{}", plan.eligibility);
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
