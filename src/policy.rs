use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::Mutex;
use tokio::process::Command as TokioCommand;

pub(crate) static GIT_COMMAND_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct GitCommand {
    inner: StdCommand,
}

impl GitCommand {
    pub(crate) fn new() -> Self {
        // Poisoned means a previous git-command thread panicked while holding
        // the lock; continuing would risk overlapping git operations.
        let _command_guard = GIT_COMMAND_LOCK.lock().expect("git command lock poisoned");
        Self {
            inner: StdCommand::new(git_binary()),
        }
    }
}

impl Deref for GitCommand {
    type Target = StdCommand;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for GitCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub(crate) struct TokioGitCommand {
    inner: TokioCommand,
}

impl TokioGitCommand {
    pub(crate) fn new() -> Self {
        // Poisoned means a previous git-command thread panicked while holding
        // the lock; continuing would risk overlapping git operations.
        let _command_guard = GIT_COMMAND_LOCK.lock().expect("git command lock poisoned");
        Self {
            inner: TokioCommand::new(git_binary()),
        }
    }
}

impl Deref for TokioGitCommand {
    type Target = TokioCommand;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TokioGitCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub(crate) const DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct StandardFileConfig {
    pub(crate) source: String,
    pub(crate) target: String,
    #[serde(default)]
    pub(crate) overwrite: bool,
}

impl StandardFileConfig {
    pub(crate) fn source_path(&self, base_dir: &Path) -> PathBuf {
        let expanded = expand_tilde(&self.source);
        if expanded.is_absolute() {
            expanded
        } else {
            base_dir.join(&expanded)
        }
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix('~') {
        dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum StandardFilesEntry {
    Short(String),
    Full(StandardFileConfig),
}

impl From<String> for StandardFileConfig {
    fn from(name: String) -> Self {
        StandardFileConfig {
            source: format!("templates/{}", name),
            target: name,
            overwrite: false,
        }
    }
}

fn deserialize_standard_files<'de, D>(deserializer: D) -> Result<Vec<StandardFileConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Vec<StandardFilesEntry> = Deserialize::deserialize(deserializer)?;
    Ok(raw.into_iter().map(|e| e.into_config()).collect())
}

impl StandardFilesEntry {
    fn into_config(self) -> StandardFileConfig {
        match self {
            StandardFilesEntry::Short(name) => name.into(),
            StandardFilesEntry::Full(cfg) => cfg,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RemoteConfig {
    pub(crate) name: String,
    pub(crate) push_url: String,
    #[serde(default)]
    pub(crate) auto_create: bool,
    #[serde(default)]
    pub(crate) auto_create_account: String,
    #[serde(default = "default_auth_type")]
    pub(crate) auth_type: AuthType,
    #[serde(default = "default_priority")]
    pub(crate) priority: u32,
    #[serde(default)]
    pub(crate) api_endpoint: Option<String>,
    #[serde(default)]
    pub(crate) auto_create_token_var: Option<String>,
    /// Optional per-remote name mapping for repos that need sanitization.
    /// Key = local repo basename, Value = remote project name.
    /// Example: { ".dracon" = "dracon-home" } maps .dracon → dracon-home on this remote.
    #[serde(default)]
    pub(crate) repo_name_map: std::collections::HashMap<String, String>,
    /// If true, when a push to this remote fails with non-fast-forward, the daemon
    /// will diagnose divergence. If the remote is purely behind (0 commits ahead
    /// of local), it force-pushes with --force-with-lease. If the remote has
    /// commits local lacks (divergent), the repo is marked CONCERN instead.
    #[serde(default)]
    pub(crate) force_push_when_behind: bool,
}

impl RemoteConfig {
    pub(crate) fn resolve_push_url(&self, repo_name: &str) -> String {
        let resolved_name = self
            .repo_name_map
            .get(repo_name)
            .map(|s| s.as_str())
            .unwrap_or(repo_name);
        let url = self.push_url.replace("{repo}", resolved_name);
        url.replace("{account}", &self.auto_create_account)
    }

    pub(crate) fn resolve_repo_name(&self, repo_name: &str) -> String {
        self.repo_name_map
            .get(repo_name)
            .cloned()
            .unwrap_or_else(|| repo_name.to_string())
    }
}

fn default_auth_type() -> AuthType {
    AuthType::GitHub
}

fn default_priority() -> u32 {
    50
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AuthType {
    #[default]
    GitHub,
    GitLab,
    Codeberg,
    Generic,
}

impl AuthType {
    /// Auto-detect auth type from a push URL.
    /// Returns the detected type, or `GitHub` (default) if no match.
    pub(crate) fn from_push_url(url: &str) -> Self {
        let lower = url.to_ascii_lowercase();
        if lower.contains("gitlab") {
            Self::GitLab
        } else if lower.contains("codeberg") {
            Self::Codeberg
        } else if lower.contains("github") {
            Self::GitHub
        } else {
            Self::Generic
        }
    }
}

impl RemoteConfig {
    /// Returns the effective auth type: explicitly set if non-default,
    /// otherwise auto-detected from the push_url.
    pub(crate) fn effective_auth_type(&self) -> AuthType {
        if self.auth_type != AuthType::GitHub {
            return self.auth_type;
        }
        AuthType::from_push_url(&self.push_url)
    }

    /// Returns the account name: explicitly set if non-empty,
    /// otherwise extracted from the push_url.
    pub(crate) fn resolve_account(&self) -> String {
        if !self.auto_create_account.is_empty() {
            return self.auto_create_account.clone();
        }
        // Extract account from push_url patterns:
        // SSH:   git@host:account/repo.git
        // HTTPS: https://host/account/repo.git
        let url = &self.push_url;
        if url.contains('@') {
            // SSH: git@host:account/{repo}.git → extract account before {repo}
            if let Some(colon) = url.rfind(':') {
                let after_colon = &url[colon + 1..];
                if let Some(slash) = after_colon.find('/') {
                    return after_colon[..slash].to_string();
                }
            }
        } else if url.starts_with("http://") || url.starts_with("https://") {
            // HTTPS: https://host/account/{repo}.git
            if let Some(double_slash) = url.find("://") {
                let after_proto = &url[double_slash + 3..];
                if let Some(slash) = after_proto.find('/') {
                    let after_host = &after_proto[slash + 1..];
                    if let Some(slash) = after_host.find('/') {
                        return after_host[..slash].to_string();
                    }
                }
            }
        }
        self.auto_create_account.clone()
    }
}

fn deserialize_remotes_or_extra<'de, D>(deserializer: D) -> Result<Vec<RemoteConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RemotesOrExtra {
        New(Vec<RemoteConfig>),
        Legacy(Vec<String>),
    }

    let raw = RemotesOrExtra::deserialize(deserializer)?;
    match raw {
        RemotesOrExtra::New(configs) => Ok(configs),
        RemotesOrExtra::Legacy(names) => {
            let defaults = vec![
                RemoteConfig {
                    name: "github".to_string(),
                    push_url: "https://github.com/{account}/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: "DraconDev".to_string(),
                    auth_type: AuthType::GitHub,
                    priority: 50,
                    api_endpoint: None,
                    auto_create_token_var: None,
                    repo_name_map: Default::default(),
                    force_push_when_behind: false,
                },
                RemoteConfig {
                    name: "gitlab".to_string(),
                    push_url: "git@gitlab.com:{account}/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: "DraconDev".to_string(),
                    auth_type: AuthType::GitLab,
                    priority: 50,
                    api_endpoint: None,
                    auto_create_token_var: None,
                    repo_name_map: Default::default(),
                    force_push_when_behind: false,
                },
                RemoteConfig {
                    name: "codeberg".to_string(),
                    push_url: "git@codeberg.org:{account}/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: "dracondev".to_string(),
                    auth_type: AuthType::Codeberg,
                    priority: 50,
                    api_endpoint: Some("https://codeberg.org/api/v1/repos".to_string()),
                    auto_create_token_var: None,
                    repo_name_map: Default::default(),
                    force_push_when_behind: false,
                },
            ];

            let filtered: Vec<RemoteConfig> = defaults
                .into_iter()
                .filter(|d| names.contains(&d.name))
                .map(|mut d| {
                    d.auto_create = true;
                    d
                })
                .collect();
            Ok(filtered)
        }
    }
}

pub(crate) fn git_binary() -> PathBuf {
    if let Ok(custom) = std::env::var("DRACON_SYNC_GIT_BIN") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    for candidate in ["/run/current-system/sw/bin/git", "/usr/bin/git", "/bin/git"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return path;
        }
    }

    PathBuf::from("git")
}

pub(crate) fn std_git_command() -> GitCommand {
    GitCommand::new()
}

pub(crate) fn tokio_git_command() -> TokioGitCommand {
    TokioGitCommand::new()
}

pub(crate) fn timestamp_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Default, Deserialize, Clone)]
pub(crate) struct SyncPolicy {
    #[serde(default)]
    pub(crate) system_repo: String,
    #[serde(default = "default_pulse_interval")]
    pub(crate) pulse_interval_secs: u64,
    #[serde(default = "default_inactivity_push_delay_secs")]
    pub(crate) inactivity_push_delay_secs: u64,
    #[serde(default = "default_true")]
    pub(crate) auto_commit: bool,
    /// If true, bump patch versions before an auto-commit (best-effort).
    /// Applies to common files when present at repo root:
    /// - Rust: `Cargo.toml` (and keep `Cargo.lock` aligned for root package)
    /// - Node/TS: `package.json` (and align `package-lock.json` root `version` when applicable)
    /// - Generic: `VERSION`
    #[serde(default = "default_true")]
    pub(crate) auto_bump_versions: bool,
    #[serde(default = "default_true")]
    pub(crate) auto_pull: bool,
    #[serde(default = "default_true")]
    pub(crate) auto_push: bool,
    #[serde(default)]
    pub(crate) backup_policy: String,
    #[serde(default)]
    pub(crate) backup_dir: String,
    #[serde(default)]
    pub(crate) exclude_repos: Vec<String>,
    #[serde(default)]
    pub(crate) exclude_dir_names: Vec<String>,
    #[serde(default = "default_exclude_file_patterns")]
    pub(crate) exclude_file_patterns: Vec<String>,
    /// If true, the daemon will stage newly-created untracked working
    /// files on the next sync cycle. Defaults to `true` so brand-new
    /// working files (research docs, source files, etc.) get
    /// committed promptly. Excluded by `untracked_exclude_patterns`.
    #[serde(default = "default_true")]
    pub(crate) auto_stage_untracked: bool,
    /// Glob patterns for untracked files that should NOT be
    /// auto-staged by `auto_stage_untracked`. Defaults to common
    /// safe patterns: user notes, scratch files, audit/research
    /// evidence, and dotfile/scratch dirs. Edit in
    /// `dracon-sync.toml` to extend.
    #[serde(default = "default_untracked_exclude_patterns")]
    pub(crate) untracked_exclude_patterns: Vec<String>,
    /// Glob patterns for TRACKED files that should NOT be auto-
    /// staged by the daemon. Unlike `untracked_exclude_patterns`
    /// (which only applies to newly-added files), this list applies
    /// to ANY file the daemon considers staging — including
    /// modifications to already-tracked files.
    ///
    /// Use case: a repo's `web/test-results/` directory has 372
    /// Playwright screenshots that are force-tracked by the
    /// `.gitignore` allowlist (`!*.png`). Playwright regenerates
    /// these on every test run, and the daemon auto-commits them
    /// — creating a moving target the daemon can never push.
    /// Setting `auto_commit_exclude_patterns = ["**/test-results/**"]`
    /// in the repo's `.dracon/dracon-sync.toml` tells the daemon
    /// to skip those files entirely (manual `git add` still works).
    ///
    /// Defaults to empty: this is an opt-in per-repo mechanism. The
    /// global `untracked_exclude_patterns` still applies to new
    /// files; this only filters modifications to tracked files.
    #[serde(default)]
    pub(crate) auto_commit_exclude_patterns: Vec<String>,
    #[serde(default = "default_true")]
    pub(crate) auto_repair_concerns: bool,
    #[serde(default = "default_true")]
    pub(crate) auto_repair_warns: bool,
    #[serde(default = "default_true")]
    pub(crate) auto_rewrite_large_blobs: bool,
    #[serde(default)]
    pub(crate) watch_roots: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_remotes_or_extra")]
    pub(crate) remotes: Vec<RemoteConfig>,
    #[serde(default)]
    pub(crate) auto_github_private: bool,
    #[serde(default = "default_github_account")]
    pub(crate) auto_github_private_account: String,
    #[serde(default = "default_max_stage_file_bytes")]
    pub(crate) max_stage_file_bytes: u64,
    /// Maximum number of files to stage in a single commit batch.
    /// When a repo has more untracked files than this limit, the daemon
    /// commits them in multiple smaller batches (each ≤ this size) to
    /// avoid lock contention and large commit overhead.
    /// Default: 100 (matches goal mqli43u6-tg3lcf requirement of 50-100).
    #[serde(default = "default_max_stage_batch_files")]
    pub(crate) max_stage_batch_files: usize,

    /// Auto-resolve unmerged index entries when the working tree
    /// matches HEAD (the "ours" side of the merge). When true, the
    /// daemon runs `git reset HEAD -- <unmerged-paths>` to clear
    /// the unmerge and proceeds with the commit. This is safe
    /// because the working tree content has already been verified
    /// to match HEAD byte-for-byte. Default: true (commit-all
    /// policy — never block on stale unmerged entries).
    /// ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44.
    #[serde(default = "default_auto_resolve_unmerged")]
    pub(crate) auto_resolve_unmerged: bool,

    /// Minimum seconds between push attempts to all remotes of a
    /// single repo. When the daemon makes multiple commits within
    /// this window, it accumulates them and pushes once at the end
    /// of the window. This eliminates per-commit push churn.
    /// Default: 30s.
    /// ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44.
    #[serde(default = "default_push_debounce_secs")]
    pub(crate) push_debounce_secs: u64,

    /// When the untracked-file count exceeds this threshold, the
    /// daemon emits a warning to the operator. Set to 0 to disable.
    /// Default: 500.
    /// ADDED 2026-06-21, goal 55db3bfc-4fc0-4650-8349-38da9e62bd44.
    #[serde(default = "default_untracked_warn_threshold")]
    pub(crate) untracked_warn_threshold: usize,

    #[serde(default = "default_pull_op_timeout_secs")]
    pub(crate) pull_op_timeout_secs: u64,
    #[serde(default = "default_push_op_timeout_secs")]
    pub(crate) push_op_timeout_secs: u64,
    #[serde(default = "default_repo_sync_timeout_secs")]
    pub(crate) repo_sync_timeout_secs: u64,
    /// Timeout for `git add` staging operations during a sync cycle.
    /// Repos with very large working trees (thousands of dirty paths) may
    /// need a higher value than the legacy 30s hardcoded default.
    /// Default: 60s.
    #[serde(default = "default_stage_op_timeout_secs")]
    pub(crate) stage_op_timeout_secs: u64,
    /// When `git add` times out for a repo, the daemon applies a per-repo
    /// cooldown of this many seconds before retrying. This prevents the
    /// incident ledger from being spammed every ~70s with the same timeout
    /// for repos whose working tree is too large to stage automatically.
    /// Default: 3600s (1 hour).
    #[serde(default = "default_stage_cooldown_secs")]
    pub(crate) stage_cooldown_secs: u64,
    #[serde(default = "default_push_retries")]
    pub(crate) push_retries: u32,
    #[serde(default = "default_repair_cooldown_secs")]
    pub(crate) repair_cooldown_secs: u64,
    #[serde(default = "default_max_push_blob_bytes")]
    pub(crate) max_push_blob_bytes: u64,
    #[serde(default = "default_sem_max_concurrent_sync")]
    pub(crate) sem_max_concurrent_sync: usize,
    #[serde(default = "default_incident_ledger_max_lines")]
    pub(crate) incident_ledger_max_lines: usize,
    #[serde(default = "default_incident_ledger_max_age_days")]
    pub(crate) incident_ledger_max_age_days: u64,
    #[serde(default)]
    pub(crate) webhook_url: Option<String>,
    #[serde(default = "default_alert_unpushed_threshold")]
    pub(crate) alert_unpushed_threshold: usize,
    /// When a repo has more than this many unpushed commits AND the push
    /// has been pending (ahead_since) for more than
    /// `auto_commit_backstop_min_age_secs`, the daemon stops auto-
    /// committing and logs the backstop. This prevents the daemon from
    /// creating a moving target while a push is failing. Set to 0 to
    /// disable the backstop entirely. The ACTIVITY column shows
    /// `⏸ backstop` for repos in this state so the operator can see
    /// the daemon is intentionally pausing.
    #[serde(default = "default_auto_commit_backstop_threshold")]
    pub(crate) auto_commit_backstop_threshold: usize,
    #[serde(default = "default_auto_commit_backstop_min_age_secs")]
    pub(crate) auto_commit_backstop_min_age_secs: u64,
    /// Number of consecutive push failures before the daemon
    /// stops auto-pushing a repo and surfaces a `🛑 push-stuck`
    /// state in the ACTIVITY column with the actual error
    /// message in the HINT. Default 5. Set to 0 to disable the
    /// retry budget (never give up; the existing
    /// `push_retries` still applies per-attempt).
    #[serde(default = "default_push_max_retries")]
    pub(crate) push_max_retries: u32,
    /// Safety guard rail: when true (default), the daemon skips
    /// auto-commit AND auto-push for repos classified as
    /// `Unowned` or `Unknown` by `ownership::detect_ownership`.
    /// This prevents the daemon from committing/pushing into a
    /// repo whose `origin` remote points to someone else's
    /// GitHub/GitLab account (e.g. `zerostack-reference`'s
    /// `gi-dellav/zerostack.git`) or whose HEAD author is a
    /// historical bad config (e.g. `dracon-ai-lib`'s
    /// `Dracon <dracon@void>`). Set to false to re-enable
    /// auto-handling for unowned repos (NOT recommended).
    /// Per-repo override: `RepoPolicyOverride.auto_skip_unowned`.
    #[serde(default = "default_true")]
    pub(crate) auto_skip_unowned: bool,
    /// Trusted git `user.email` values. A repo is classified as
    /// Owned when its local `git config user.email` matches one
    /// of these. Default: `["dracsharp@gmail.com"]`. This is the
    /// strongest ownership signal — a repo with the wrong
    /// `user.email` will be classified `Unowned` with reason
    /// `untrusted_email`.
    #[serde(default = "default_trusted_emails")]
    pub(crate) trusted_emails: Vec<String>,
    /// Trusted git author name values. A repo is classified as
    /// Owned when its HEAD commit author name matches one of
    /// these (e.g. when the operator's commit name was changed
    /// in the global git config but historical commits still
    /// have the old name). Default: `["DraconDev"]`.
    #[serde(default = "default_trusted_authors")]
    pub(crate) trusted_authors: Vec<String>,
    /// Trusted `origin` remote URL substrings. A repo is
    /// classified as Owned when its `origin` URL contains one of
    /// these (matched as a substring, e.g.
    /// `github.com/DraconDev`). Default: the three DraconDev
    /// hosts on GitHub, GitLab, and Codeberg.
    #[serde(default = "default_trusted_remote_hosts")]
    pub(crate) trusted_remote_hosts: Vec<String>,
    /// When a repo has been dirty continuously for longer than
    /// this many seconds, the daemon commits REGARDLESS of
    /// whether the fingerprint is still changing. Prevents the
    /// "⏸ stalled Xm" pileup the operator sees when many repos
    /// have stale dirty state from previous sessions. Default
    /// 60s. Set to 0 to disable (back to 5s fingerprint wait).
    #[serde(default = "default_settling_max_delay_secs")]
    pub(crate) settling_max_delay_secs: u64,
    /// Action to take when a dirty repo exceeds
    /// `settling_max_delay_secs`. `Commit` (default) force-
    /// commits the current state; `Warn` logs a warning but
    /// does not commit; `Ignore` does nothing.
    #[serde(default = "default_dirty_max_age_action")]
    pub(crate) dirty_max_age_action: DirtyMaxAgeAction,
    /// Minimum time between consecutive auto-commits for the
    /// same repo. Prevents thrashing when the operator is
    /// actively editing. Default 5s. Setting this too high will
    /// make the daemon appear to "stall" on dirty repos.
    #[serde(default = "default_min_commit_interval_secs")]
    pub(crate) min_commit_interval_secs: u64,

    #[serde(default)]
    pub(crate) sync_visibility: bool,
    #[serde(default = "default_sync_visibility_interval_hours")]
    pub(crate) sync_visibility_interval_hours: u64,
    /// When true, sync repo description and topics from GitHub to mirror remotes.
    /// Uses the same interval as visibility sync.
    #[serde(default)]
    pub(crate) sync_metadata: bool,
    /// When true, version bumps automatically create a git tag (e.g. v0.2.0).
    /// Tags are cheap and reversible — default is true.
    /// Per-repo override via `auto_tag` in .dracon/dracon-sync.toml.
    #[serde(default = "default_true")]
    pub(crate) auto_tag: bool,
    /// When true, major version bumps create a GitHub Release.
    /// Releases are public-facing milestones — default is false.
    /// Per-repo override via `auto_release` in .dracon/dracon-sync.toml.
    #[serde(default)]
    pub(crate) auto_release: bool,
    /// Master toggle for auto-publishing to package registries after version bumps.
    /// Requires per-repo opt-in via `auto_publish` in `.dracon/dracon-sync.toml`.
    #[serde(default)]
    pub(crate) auto_publish: bool,
    /// Configured package registry publish targets.
    #[serde(default)]
    pub(crate) publish_targets: Vec<PublishTarget>,
    /// When true, version bumps in repos with a `flake.nix` create a PR updating
    /// the flake's version field (in addition to tagging and publishing).
    /// Default is false.
    #[serde(default)]
    pub(crate) nix_auto_update: bool,
    /// Standard files to ensure exist in every synced repository.
    /// Short form (filename string) resolves to `templates/{name}` in the sync config dir.
    /// Long form allows specifying explicit source/target paths and overwrite behavior.
    #[serde(default, deserialize_with = "deserialize_standard_files")]
    pub(crate) standard_files: Vec<StandardFileConfig>,
    /// Automatically copy standard files during the sync cycle.
    /// When true (default), standard files are auto-copied to repos during sync.
    /// When false, use `dracon-sync scaffold` to apply on demand.
    #[serde(default = "default_true")]
    pub(crate) standard_files_auto: bool,
    /// `dracon-sync repos` "active" threshold (in minutes). A repo is
    /// labelled `active` only when it is clean, in sync, and both commit
    /// and push are within this window. Default: 5 minutes.
    #[serde(default = "default_active_commit_minutes")]
    pub(crate) active_commit_minutes: u64,
    /// `dracon-sync repos` "committing" threshold (in minutes). A repo
    /// with unpushed commits, or whose last commit is between
    /// `active_commit_minutes` and this value, is labelled `committing`.
    /// Default: 60 minutes.
    #[serde(default = "default_committing_commit_minutes")]
    pub(crate) committing_commit_minutes: u64,
    /// `dracon-sync repos` "cold" threshold (in minutes). A repo whose
    /// last commit is older than this is labelled `cold`. Default:
    /// 1440 minutes (24 hours).
    #[serde(default = "default_cold_commit_minutes")]
    pub(crate) cold_commit_minutes: u64,
}

/// Package registry type for auto-publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PublishRegistry {
    #[default]
    CratesIo,
    Npm,
    Pypi,
}

impl PublishRegistry {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            PublishRegistry::CratesIo => "crates-io",
            PublishRegistry::Npm => "npm",
            PublishRegistry::Pypi => "pypi",
        }
    }
}

/// A configured package registry publish target.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PublishTarget {
    /// Human-readable name for this target (used in per-repo opt-in lists).
    pub(crate) name: String,
    /// Registry type (crates-io, npm, pypi).
    pub(crate) registry: PublishRegistry,
    /// Environment variable name or secret file key for the auth token.
    /// Loaded via `load_secret()` from env or `~/.dracon/utilities/sync/secrets/`.
    pub(crate) token_secret: String,
    /// Timeout for the publish command in seconds.
    #[serde(default = "default_publish_timeout_secs")]
    pub(crate) publish_timeout_secs: u64,
}

fn default_publish_timeout_secs() -> u64 {
    300
}

fn default_active_commit_minutes() -> u64 {
    5
}

fn default_committing_commit_minutes() -> u64 {
    60
}

fn default_cold_commit_minutes() -> u64 {
    1440
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct RepoPolicyOverride {
    /// Optional per-repo override for `auto_bump_versions`.
    pub(crate) auto_bump_versions: Option<bool>,
    /// Per-repo override for auto-tagging. Defaults to true (inherited from global).
    /// Set to false to disable version tags for this repo.
    #[serde(default)]
    pub(crate) auto_tag: Option<bool>,
    /// Per-repo override for GitHub Releases on major bumps.
    /// Defaults to false (inherited from global auto_release).
    #[serde(default)]
    pub(crate) auto_release: Option<bool>,
    /// Per-repo list of publish target names to auto-publish to.
    /// Only used when global `auto_publish = true`.
    #[serde(default)]
    pub(crate) auto_publish: Vec<String>,
    /// Per-repo override for nix flake auto-update. Defaults to false (inherited from global).
    /// Set to true to enable auto-creating PRs for flake version updates.
    #[serde(default)]
    pub(crate) nix_auto_update: Option<bool>,
    /// Per-repo list of standard file targets to skip.
    /// Use the target filename (e.g., "LICENSE") not the source path.
    #[serde(default)]
    pub(crate) skip_standard_files: Vec<String>,
    /// When true, the daemon recognizes this repo as intentionally
    /// untracked by any remote (for example, a local working tree that
    /// mirrors a different public clone, or a legacy-isolated repo).
    /// With this flag set:
    ///
    /// - `NO_UPSTREAM` is replaced by the explicit `INTENTIONAL_NO_UPSTREAM`
    ///   flag in `repos` output, so the row is not classified as a
    ///   hidden concern.
    /// - `repair concerns` skips the repo entirely.
    /// - The auto-repair path never runs `git push -u origin HEAD`, so the
    ///   daemon will not attempt to wire the local branch to a remote the
    ///   operator has explicitly chosen to leave unconnected.
    /// - The hint for the row says
    ///   `"intentional legacy isolation, no upstream configured"`.
    ///
    /// Default: false. Set in `<repo>/.dracon/dracon-sync.toml`.
    #[serde(default)]
    pub(crate) intentional_no_upstream: bool,
    /// Per-repo override for `active_commit_minutes`. None means inherit
    /// the global value. See [`SyncPolicy::active_commit_minutes`].
    #[serde(default)]
    pub(crate) active_commit_minutes: Option<u64>,
    /// Per-repo override for `committing_commit_minutes`. None means
    /// inherit the global value.
    #[serde(default)]
    pub(crate) committing_commit_minutes: Option<u64>,
    /// Per-repo override for `cold_commit_minutes`. None means inherit
    /// the global value.
    #[serde(default)]
    pub(crate) cold_commit_minutes: Option<u64>,
    /// Per-repo list of glob patterns for TRACKED files the daemon
    /// should NOT auto-commit. See
    /// [`SyncPolicy::auto_commit_exclude_patterns`]. None means
    /// inherit the global value. Each entry is a glob
    /// (e.g. `"**/test-results/**"` or `"*.log"`).
    #[serde(default)]
    pub(crate) auto_commit_exclude_patterns: Option<Vec<String>>,
    /// Per-repo override for `auto_skip_unowned`. Some(true)
    /// forces the repo to be classified as Owned (the daemon
    /// will commit and push even if its origin or author isn't
    /// trusted). Some(false) forces Unowned (skip regardless of
    /// signals). None inherits the global policy.
    #[serde(default)]
    pub(crate) owned: Option<bool>,
    /// Per-repo override for `auto_skip_unowned`. Some(false)
    /// re-enables the daemon for a specific unowned repo (the
    /// operator has confirmed they want to push into it). Some
    /// (true) overrides to skip. None inherits the global
    /// policy.
    #[serde(default)]
    pub(crate) auto_skip_unowned: Option<bool>,
    /// Per-repo override for `settling_max_delay_secs`. None
    /// inherits the global value. See
    /// [`SyncPolicy::settling_max_delay_secs`].
    #[serde(default)]
    pub(crate) settling_max_delay_secs: Option<u64>,
    /// Per-repo override for `dirty_max_age_action`. None
    /// inherits the global value.
    #[serde(default)]
    pub(crate) dirty_max_age_action: Option<DirtyMaxAgeAction>,
    /// Per-repo list of remote names to skip when configuring
    /// and pushing mirrors. Each entry is a remote name as
    /// defined in the global `[[remotes]]` config (e.g. "gitlab").
    /// When a remote is in this list, the daemon will not
    /// add the remote to `.git/config` and will not attempt to
    /// push to it. Default: empty (no exclusion). Set in
    /// `<repo>/.dracon/dracon-sync.toml`.
    ///
    /// Added 2026-06-23 (goal mqqsyzyd-qkvna5) so a repo whose
    /// mirror is permanently unavailable (e.g. over a free-tier
    /// storage quota) can opt out of that mirror without
    /// affecting other repos that use it.
    #[serde(default)]
    pub(crate) exclude_remotes: Vec<String>,

}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_pulse_interval() -> u64 {
    1
}

pub(crate) fn default_inactivity_push_delay_secs() -> u64 {
    2
}

pub(crate) fn load_repo_override(repo: &Path) -> RepoPolicyOverride {
    let path = repo.join(".dracon").join("dracon-sync.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return RepoPolicyOverride::default();
    };
    toml::from_str(&content).unwrap_or_else(|e| {
        eprintln!("⚠️ failed to parse repo override {}: {}", path.display(), e);
        RepoPolicyOverride::default()
    })
}

pub(crate) fn default_exclude_dir_names() -> Vec<String> {
    [
        "target",
        "node_modules",
        ".cache",
        ".direnv",
        ".venv",
        "dist",
        "build",
        "archives",
        ".tmp-*",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Default exclude patterns for `exclude_file_patterns` (TRACKED file
/// modifications). Operator policy change 2026-06-15 (goal `9aaf0b08`):
/// "commit all unless we have a super good reason to leave it out like
/// over 100 megs". Logs and DB files are now committed (operator can
/// `git rm` them later if desired). Per-repo `auto_commit_exclude_patterns`
/// still applies for operator-defined per-repo exclusions.
pub(crate) fn default_exclude_file_patterns() -> Vec<String> {
    Vec::new()
}

/// Default exclude patterns for `untracked_exclude_patterns`. The daemon
/// will NOT auto-stage untracked files matching any of these patterns.
/// Operator policy change 2026-06-15 (goal `9aaf0b08`): "commit all
/// unless we have a super good reason to leave it out". Defaults are
/// now MINIMAL — only session-scratch patterns (super-good reasons to
/// keep untracked) remain. Patterns REMOVED in this change (now
/// committed by default):
///   - User notes (`**/note.md`, `**/notes.md`, `**/scratch.md`)
///   - Audit / evidence (`**/audit/**`, `**/evidence/**`, `**/screenshots/**`)
///   - Media files (`*.png`, `*.jpg`, `*.jpeg`, `*.gif`, `*.webp`,
///     `*.mp4`, `*.mov`)
///
/// Patterns KEPT (super-good reasons to stay untracked):
///   - Session scratch dirs (`**/scratch/**`, `**/scratch-*`, `**/scratch_*`)
///   - Temp dirs (`**/tmp/**`, `**/tmp-*`)
///   - Agent session scratch (`**/pi-tmp/**`, `**/.pi-tmp/**`,
///     `.demon/**`, `.sisyphus/**`, `.ralph/**`)
///   - Research scratch dirs (`**/research/scratch/**`)
///
/// Per-repo `auto_commit_exclude_patterns` is the operator's opt-in
/// mechanism to extend this list per-repo (e.g., Junk-Runner-bevy's
/// `**/test-results/**` exclusion).
pub(crate) fn default_untracked_exclude_patterns() -> Vec<String> {
    [
        // Session / agent scratch dirs — keep local
        "**/scratch/**",
        "**/scratch-*",
        "**/scratch_*",
        "**/tmp/**",
        "**/tmp-*",
        "**/pi-tmp/**",
        "**/.pi-tmp/**",
        "**/research/scratch/**",
        // Agent session state — never auto-stage
        ".demon/**",
        ".sisyphus/**",
        ".ralph/**",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

pub(crate) fn default_max_stage_file_bytes() -> u64 {
    100 * 1024 * 1024
}

fn default_max_stage_batch_files() -> usize {
    // CHANGED 2026-06-20: 100 -> 100000. The batch limit was splitting
    // large Playwright test runs into multiple commits, slowing sync.
    // With 100000, the daemon commits everything it can in one cycle.
    100000
}

pub(crate) fn default_auto_resolve_unmerged() -> bool {
    // Default: true. The daemon's commit-all policy should never
    // be blocked by a stale unmerged index. When the working tree
    // matches HEAD (verified byte-by-byte), the unmerge is safe to
    // reset.
    true
}

pub(crate) fn default_push_debounce_secs() -> u64 {
    // Default: 30s. Eliminates per-commit push churn while still
    // keeping the remotes up-to-date within a reasonable window.
    30
}

pub(crate) fn default_untracked_warn_threshold() -> usize {
    // Default: 500. Emits a warning when the untracked count
    // exceeds this. Set to 0 to disable.
    500
}

pub(crate) fn default_pull_op_timeout_secs() -> u64 {
    30
}

pub(crate) fn default_push_op_timeout_secs() -> u64 {
    300
}

pub(crate) fn default_repo_sync_timeout_secs() -> u64 {
    420
}

pub(crate) fn default_stage_op_timeout_secs() -> u64 {
    60
}

pub(crate) fn default_stage_cooldown_secs() -> u64 {
    3600
}

pub(crate) fn default_push_retries() -> u32 {
    3
}

pub(crate) fn default_repair_cooldown_secs() -> u64 {
    60
}

pub(crate) fn default_max_push_blob_bytes() -> u64 {
    DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES
}

pub(crate) fn default_sem_max_concurrent_sync() -> usize {
    4
}

pub(crate) fn default_incident_ledger_max_lines() -> usize {
    10_000
}

pub(crate) fn default_incident_ledger_max_age_days() -> u64 {
    30
}

fn default_github_account() -> String {
    "DraconDev".to_string()
}

fn default_alert_unpushed_threshold() -> usize {
    10
}

fn default_auto_commit_backstop_threshold() -> usize {
    20
}

fn default_auto_commit_backstop_min_age_secs() -> u64 {
    300
}

fn default_push_max_retries() -> u32 {
    5
}

pub(crate) fn default_trusted_emails() -> Vec<String> {
    vec!["dracsharp@gmail.com".to_string()]
}

pub(crate) fn default_trusted_authors() -> Vec<String> {
    vec!["DraconDev".to_string()]
}

pub(crate) fn default_trusted_remote_hosts() -> Vec<String> {
    vec![
        "github.com/DraconDev".to_string(),
        "gitlab.com/dracondev".to_string(),
        "codeberg.org/dracondev".to_string(),
    ]
}

pub(crate) fn default_settling_max_delay_secs() -> u64 {
    60
}

pub(crate) fn default_min_commit_interval_secs() -> u64 {
    5
}

pub(crate) fn default_dirty_max_age_action() -> DirtyMaxAgeAction {
    DirtyMaxAgeAction::Commit
}

/// What the daemon should do when a dirty repo has been dirty
/// continuously for longer than `settling_max_delay_secs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DirtyMaxAgeAction {
    /// Force-commit the current working tree state, regardless of
    /// fingerprint stability. Default.
    Commit,
    /// Log a warning to stderr but do NOT commit. The operator
    /// must intervene.
    Warn,
    /// Do nothing. Same as `Warn` but with no log line.
    Ignore,
}

impl Default for DirtyMaxAgeAction {
    fn default() -> Self {
        DirtyMaxAgeAction::Commit
    }
}

fn default_sync_visibility_interval_hours() -> u64 {
    24
}

impl SyncPolicy {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read policy {}", path.display()))?;
        let mut policy: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse policy {}", path.display()))?;
        if policy.exclude_dir_names.is_empty() {
            policy.exclude_dir_names = default_exclude_dir_names();
        }
        if policy.max_stage_file_bytes == 0 {
            policy.max_stage_file_bytes = default_max_stage_file_bytes();
        }
        if policy.pull_op_timeout_secs == 0 {
            policy.pull_op_timeout_secs = default_pull_op_timeout_secs();
        }
        if policy.push_op_timeout_secs == 0 {
            policy.push_op_timeout_secs = default_push_op_timeout_secs();
        }
        if policy.repo_sync_timeout_secs == 0 {
            policy.repo_sync_timeout_secs = default_repo_sync_timeout_secs();
        }
        if policy.stage_op_timeout_secs == 0 {
            policy.stage_op_timeout_secs = default_stage_op_timeout_secs();
        }
        if policy.push_retries == 0 {
            policy.push_retries = default_push_retries();
        }
        if policy.inactivity_push_delay_secs == 0 {
            policy.inactivity_push_delay_secs = default_inactivity_push_delay_secs();
        }
        if policy.repair_cooldown_secs == 0 {
            policy.repair_cooldown_secs = default_repair_cooldown_secs();
        }
        if policy.max_push_blob_bytes == 0 {
            policy.max_push_blob_bytes = default_max_push_blob_bytes();
        }
        if policy.incident_ledger_max_lines == 0 {
            policy.incident_ledger_max_lines = default_incident_ledger_max_lines();
        }
        if policy.incident_ledger_max_age_days == 0 {
            policy.incident_ledger_max_age_days = default_incident_ledger_max_age_days();
        }
        if policy.pull_op_timeout_secs < 5 {
            eprintln!(
                "⚠️ pull_op_timeout_secs {} below minimum 5s, adjusting",
                policy.pull_op_timeout_secs
            );
            policy.pull_op_timeout_secs = 5;
        }
        if policy.push_op_timeout_secs < 10 {
            eprintln!(
                "⚠️ push_op_timeout_secs {} below minimum 10s, adjusting",
                policy.push_op_timeout_secs
            );
            policy.push_op_timeout_secs = 10;
        }
        if policy.stage_op_timeout_secs < 10 {
            eprintln!(
                "⚠️ stage_op_timeout_secs {} below minimum 10s, adjusting",
                policy.stage_op_timeout_secs
            );
            policy.stage_op_timeout_secs = 10;
        }
        if policy.stage_cooldown_secs == 0 {
            policy.stage_cooldown_secs = default_stage_cooldown_secs();
        }
        policy.max_push_blob_bytes = policy
            .max_push_blob_bytes
            .clamp(1, DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES);
        policy.repo_sync_timeout_secs = policy.repo_sync_timeout_secs.max(
            policy
                .push_op_timeout_secs
                .saturating_add(30)
                .max(policy.pull_op_timeout_secs.saturating_add(30)),
        );
        Ok(policy)
    }

    pub(crate) fn watch_root_paths(&self) -> Vec<PathBuf> {
        self.watch_roots
            .iter()
            .map(PathBuf::from)
            .filter(|p| {
                if !p.exists() {
                    eprintln!("⚠️ watch root {} does not exist, skipping", p.display());
                    false
                } else {
                    true
                }
            })
            .collect()
    }
}

pub(crate) fn resolve_policy_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home not found")?;
    if let Ok(val) = std::env::var("DRACON_SYNC_POLICY") {
        return Ok(PathBuf::from(val));
    }
    let paths = [
        home.join(".dracon/utilities/sync/dracon-sync.toml"),
        home.join(".dracon/utilities/sync/config.toml"),
        home.join(".dracon/git/dracon-git.toml"),
    ];
    for path in &paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }
    anyhow::bail!("sync policy not found")
}

#[derive(Debug, Default)]
pub(crate) struct ValidateResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidateResult {
    fn error(&mut self, msg: String) {
        self.errors.push(msg);
    }

    fn warn(&mut self, msg: String) {
        self.warnings.push(msg);
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

pub(crate) fn validate_config(policy_path: &Path) -> ValidateResult {
    let mut result = ValidateResult::default();

    let content = match std::fs::read_to_string(policy_path) {
        Ok(c) => c,
        Err(e) => {
            result.error(format!(
                "cannot read policy {}: {}",
                policy_path.display(),
                e
            ));
            return result;
        }
    };

    let policy: SyncPolicy = match toml::from_str(&content) {
        Ok(p) => p,
        Err(e) => {
            result.error(format!("TOML parse error: {}", e));
            return result;
        }
    };

    for root in &policy.watch_roots {
        let path = Path::new(root);
        if !path.exists() {
            result.error(format!("watch root does not exist: {}", root));
        } else if !path.is_dir() {
            result.error(format!("watch root is not a directory: {}", root));
        }
    }

    if policy.watch_roots.is_empty() {
        result.error("no watch_roots defined (no directories will be synced)".to_string());
    }

    for (idx, remote) in policy.remotes.iter().enumerate() {
        if remote.push_url.is_empty() {
            result.error(format!(
                "remote[{}] '{}': push_url is empty",
                idx, remote.name
            ));
        }

        if remote.auto_create {
            // Empty auto_create_account is intentionally silent: resolve_account()
            // extracts the account from the push_url (e.g. git@host:account/{repo}.git
            // → account). This is a working default that has shipped for years.

            if let Some(token_var) = &remote.auto_create_token_var {
                if token_var.is_empty() {
                    result.error(format!(
                        "remote[{}] '{}': auto_create_token_var is set but empty",
                        idx, remote.name
                    ));
                } else if std::env::var(token_var).is_err() {
                    let secrets_dir = crate::secrets::sync_secrets_dir();
                    let secrets_path =
                        secrets_dir.join(format!("{}.env", token_var.to_lowercase()));
                    if !secrets_path.exists() {
                        result.warn(format!(
                            "remote[{}] '{}': auto_create_token_var '{}' not in env and no secret file at {}",
                            idx, remote.name, token_var, secrets_path.display()
                        ));
                    }
                }
            }

            if remote.effective_auth_type() == crate::policy::AuthType::Codeberg {
                if let Some(api_endpoint) = &remote.api_endpoint {
                    if api_endpoint.is_empty() {
                        result.error(format!(
                            "remote[{}] '{}': auth_type=codeberg but api_endpoint is empty",
                            idx, remote.name
                        ));
                    } else if !api_endpoint.starts_with("http://")
                        && !api_endpoint.starts_with("https://")
                    {
                        result.error(format!(
                            "remote[{}] '{}': api_endpoint '{}' is not a valid URL",
                            idx, remote.name, api_endpoint
                        ));
                    }
                } else {
                    // No api_endpoint set → fall back to the Codeberg default
                    // (https://codeberg.org). Working default, intentionally silent.
                }
            }
        } else if !remote.push_url.contains("{repo}") && !remote.push_url.contains("{account}") {
            result.warn(format!(
                "remote[{}] '{}': push_url '{}' has no {{repo}} or {{account}} placeholder — repo names will not be substituted",
                idx, remote.name, remote.push_url
            ));
        }

        for (local_name, remote_name) in &remote.repo_name_map {
            if local_name.is_empty() {
                result.error(format!(
                    "remote[{}] '{}': repo_name_map has empty local name (maps to '{}')",
                    idx, remote.name, remote_name
                ));
            }
            if remote_name.is_empty() {
                result.error(format!(
                    "remote[{}] '{}': repo_name_map local '{}' maps to empty remote name",
                    idx, remote.name, local_name
                ));
            }
            if local_name.contains('/') || local_name.contains('\\') {
                result.error(format!(
                    "remote[{}] '{}': repo_name_map local name '{}' is not a valid directory name",
                    idx, remote.name, local_name
                ));
            }
        }
    }

    if policy.remotes.is_empty() {
        result.warn("no remotes defined (push operations will have no destination)".to_string());
    }

    for (idx, pattern) in policy.exclude_dir_names.iter().enumerate() {
        if pattern.is_empty() {
            result.warn(format!("exclude_dir_names[{}] is empty string", idx));
        }
    }

    for (idx, pattern) in policy.exclude_file_patterns.iter().enumerate() {
        if pattern.is_empty() {
            result.warn(format!("exclude_file_patterns[{}] is empty string", idx));
        }
    }

    if policy.auto_github_private && policy.auto_github_private_account.is_empty() {
        result
            .error("auto_github_private=true but auto_github_private_account is empty".to_string());
    }

    if policy.pulse_interval_secs == 0 {
        result.error("pulse_interval_secs must be > 0".to_string());
    }

    if policy.push_retries == 0 {
        result.error("push_retries must be > 0".to_string());
    }

    if policy.max_stage_file_bytes == 0 {
        result.error("max_stage_file_bytes must be > 0".to_string());
    }

    if policy.stage_op_timeout_secs < 10 {
        result.warn(format!(
            "stage_op_timeout_secs {} below recommended minimum 10s",
            policy.stage_op_timeout_secs
        ));
    }
    if policy.stage_cooldown_secs < 60 {
        result.warn(format!(
            "stage_cooldown_secs {} below recommended minimum 60s; very short cooldowns can flood the incident ledger",
            policy.stage_cooldown_secs
        ));
    }
    if policy.pull_op_timeout_secs < 5 {
        result.warn(format!(
            "pull_op_timeout_secs {} below recommended minimum 5s",
            policy.pull_op_timeout_secs
        ));
    }
    if policy.push_op_timeout_secs < 10 {
        result.warn(format!(
            "push_op_timeout_secs {} below recommended minimum 10s",
            policy.push_op_timeout_secs
        ));
    }
    let min_repo_sync_timeout = policy
        .push_op_timeout_secs
        .saturating_add(30)
        .max(policy.pull_op_timeout_secs.saturating_add(30));
    if policy.repo_sync_timeout_secs < min_repo_sync_timeout {
        result.warn(format!(
            "repo_sync_timeout_secs {} below recommended minimum {}s (push/pull timeout + 30s safety margin)",
            policy.repo_sync_timeout_secs, min_repo_sync_timeout
        ));
    }
    if policy.inactivity_push_delay_secs == 0 {
        result.warn("inactivity_push_delay_secs = 0 means the daemon may commit partial changes before quiet time elapses".to_string());
    }
    if policy.repair_cooldown_secs < 10 {
        result.warn(format!(
            "repair_cooldown_secs {} below recommended minimum 10s; very short cooldowns can flood the incident ledger",
            policy.repair_cooldown_secs
        ));
    }
    if policy.incident_ledger_max_lines < 100 {
        result.warn(format!(
            "incident_ledger_max_lines {} below recommended minimum 100; recent-push-failure classification may lose context",
            policy.incident_ledger_max_lines
        ));
    }
    if policy.incident_ledger_max_age_days == 0 {
        result.warn(
            "incident_ledger_max_age_days = 0 disables age-based ledger retention".to_string(),
        );
    }

    if let Some(ref url) = policy.webhook_url {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            result.error(format!(
                "webhook_url '{}' is not a valid http/https URL",
                url
            ));
        }
    }

    check_toml_field_ordering(&content, &mut result);
    result
}

fn check_toml_field_ordering(content: &str, result: &mut ValidateResult) {
    let mut first_section_pos: Option<usize> = None;
    let mut pos = 0;
    let bytes = content.as_bytes();

    let mut in_table = false; // inside a [[remotes]] or [[...]] table entry

    while pos < bytes.len() {
        let line_start = pos;
        while pos < bytes.len() && bytes[pos] != b'\n' {
            pos += 1;
        }
        let line = &content[line_start..pos];
        let stripped = line.trim();

        if stripped.starts_with("[[") {
            // Enters a new table entry — fields inside are table-scoped, not top-level
            in_table = true;
            if first_section_pos.is_none() {
                first_section_pos = Some(line_start);
            }
        } else if stripped.starts_with('[') && !stripped.starts_with("[[") {
            // Single-bracket section like [repo_name_map], [extra_remotes]
            in_table = false;
            if first_section_pos.is_none() {
                first_section_pos = Some(line_start);
            }
        } else if !stripped.is_empty() && !stripped.starts_with('#') && stripped.contains('=') {
            if let Some(first_sec) = first_section_pos {
                // Only warn for top-level fields after a section header.
                // Fields inside [[remotes]] table entries are correctly placed.
                if line_start > first_sec && !in_table {
                    let (key, _) = stripped.split_once('=').unwrap_or((stripped, ""));
                    let key = key.trim();
                    if !key.starts_with('"')
                        && !key.starts_with('\'')
                        && !key.is_empty()
                        && !key.contains('{')
                        && !key.contains('.')
                    {
                        result.warn(format!(
                            "field '{}' appears after a section header -- top-level fields \
                             must be defined before any [section] or [[remotes]] block or \
                             they will be silently ignored by the TOML parser (they become \
                             table fields instead)",
                            key
                        ));
                    }
                }
            }
        }

        pos += 1;
    }
}

pub(crate) fn env_freeze_enabled() -> bool {
    matches!(
        std::env::var("DRACON_SYNC_FREEZE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub(crate) fn debug_enabled() -> bool {
    matches!(
        std::env::var("DRACON_SYNC_DEBUG")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub(crate) fn freeze_marker_paths(_policy_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // Freeze markers are intentionally kept out of git-tracked repos to avoid accidental
    // perpetual DIRTY states and surprise "sync frozen" incidents.
    //
    // Canonical locations:
    // - ~/.dracon/dracon-sync.freeze
    // - ~/.dracon/freeze/dracon-sync
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".dracon").join("dracon-sync.freeze"));
        paths.push(home.join(".dracon").join("freeze").join("dracon-sync"));
    }
    paths
}

/// Default freeze-marker TTL: 24 hours. Markers older than this are auto-cleared
/// and a warning is logged to prevent indefinite pause from a forgotten `pause`.
///
/// The 2026-06-04 incident (1h23m of stale freeze, 3 CONCERN repos) is the
/// motivating example. See `.dracon/project-state.md` for details.
pub(crate) const FREEZE_MARKER_TTL_SECS: u64 = 24 * 60 * 60;

/// If a freeze marker exists but is older than `FREEZE_MARKER_TTL_SECS`,
/// auto-clear it and log a warning. Returns `Some(reason)` if sync should
/// still be frozen (marker is fresh or no marker).
pub(crate) fn freeze_reason(policy_path: &Path) -> Option<String> {
    if env_freeze_enabled() {
        return Some("env DRACON_SYNC_FREEZE".to_string());
    }

    for marker in freeze_marker_paths(policy_path) {
        if marker.exists() {
            // Check TTL — auto-expire stale markers
            if let Ok(meta) = std::fs::metadata(&marker) {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = modified.elapsed() {
                        if age.as_secs() > FREEZE_MARKER_TTL_SECS {
                            eprintln!(
                                "⚠️ freeze marker at {} is stale ({:.0}h old, TTL {}s); auto-clearing to prevent indefinite pause",
                                marker.display(),
                                age.as_secs() as f64 / 3600.0,
                                FREEZE_MARKER_TTL_SECS
                            );
                            let _ = std::fs::remove_file(&marker);
                            continue;
                        }
                    }
                }
            }
            return Some(format!("marker {}", marker.display()));
        }
    }

    None
}

pub(crate) fn open_policy_in_editor(policy_path: &Path) -> Result<()> {
    let mut editors = Vec::new();
    if let Ok(visual) = std::env::var("VISUAL") {
        if !visual.trim().is_empty() {
            editors.push(visual);
        }
    }
    if let Ok(editor) = std::env::var("EDITOR") {
        if !editor.trim().is_empty() {
            editors.push(editor);
        }
    }
    for fallback in ["nvim", "vim", "nano", "vi"] {
        editors.push(fallback.to_string());
    }

    for editor in editors {
        match StdCommand::new(editor.trim()).arg(policy_path).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => {
                return Err(anyhow::anyhow!(
                    "editor exited non-zero ({}). policy: {}",
                    status,
                    policy_path.display()
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "failed to launch editor '{}' for {}: {}",
                    editor,
                    policy_path.display(),
                    e
                ));
            }
        }
    }

    Err(anyhow::anyhow!(
        "no editor available. set VISUAL or EDITOR to open {}",
        policy_path.display()
    ))
}

#[cfg(test)]
pub(crate) fn test_sync_policy() -> SyncPolicy {
    SyncPolicy {
        system_repo: String::new(),
        pulse_interval_secs: 1,
        inactivity_push_delay_secs: 5,
        auto_commit: true,
        auto_bump_versions: true,
        auto_pull: true,
        auto_push: true,
        backup_policy: String::new(),
        backup_dir: String::new(),
        exclude_repos: vec![],
        exclude_dir_names: vec![],
        exclude_file_patterns: vec![],
        auto_repair_concerns: true,
        auto_repair_warns: true,
        auto_rewrite_large_blobs: true,
        auto_stage_untracked: true,
        untracked_exclude_patterns: default_untracked_exclude_patterns(),
        auto_commit_exclude_patterns: Vec::new(),
        watch_roots: vec![],
        remotes: vec![],
        auto_github_private: false,
        auto_github_private_account: "DraconDev".to_string(),
        max_stage_file_bytes: 100 * 1024 * 1024,
        max_stage_batch_files: 100000,
        auto_resolve_unmerged: default_auto_resolve_unmerged(),
        push_debounce_secs: default_push_debounce_secs(),
        untracked_warn_threshold: default_untracked_warn_threshold(),
        pull_op_timeout_secs: 30,
        push_op_timeout_secs: 300,
        repo_sync_timeout_secs: 420,
        stage_op_timeout_secs: 60,
        stage_cooldown_secs: 3600,
        push_retries: 3,
        repair_cooldown_secs: 60,
        max_push_blob_bytes: 100 * 1024 * 1024,
        sem_max_concurrent_sync: default_sem_max_concurrent_sync(),
        incident_ledger_max_lines: 10_000,
        incident_ledger_max_age_days: 30,
        webhook_url: None,
        alert_unpushed_threshold: 10,
        auto_commit_backstop_threshold: 20,
        auto_commit_backstop_min_age_secs: 300,
        push_max_retries: 5,
        auto_skip_unowned: true,
        trusted_emails: default_trusted_emails(),
        trusted_authors: default_trusted_authors(),
        trusted_remote_hosts: default_trusted_remote_hosts(),
        settling_max_delay_secs: 60,
        dirty_max_age_action: DirtyMaxAgeAction::Commit,
        min_commit_interval_secs: 5,
        sync_visibility: false,
        sync_visibility_interval_hours: 24,
        sync_metadata: false,
        auto_tag: true,
        auto_release: false,
        auto_publish: false,
        publish_targets: vec![],
        nix_auto_update: false,
        standard_files: vec![],
        standard_files_auto: true,
        active_commit_minutes: 5,
        committing_commit_minutes: 60,
        cold_commit_minutes: 1440,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_exclude_dir_names() {
        let dirs = default_exclude_dir_names();
        assert!(dirs.contains(&"target".to_string()));
        assert!(dirs.contains(&"node_modules".to_string()));
        assert!(dirs.contains(&".cache".to_string()));
    }

    #[test]
    fn test_default_exclude_file_patterns() {
        // Goal 9aaf0b08 (2026-06-15): operator's "commit all unless
        // super-good reason" policy. Default is now empty list.
        // Logs/DBs are committed; operator can `git rm` them later.
        let patterns = default_exclude_file_patterns();
        assert!(
            patterns.is_empty(),
            "default_exclude_file_patterns should be empty under commit-all policy, got: {:?}",
            patterns
        );
    }

    #[test]
    fn test_default_untracked_exclude_patterns_is_commit_all_unless_scratch() {
        // Goal 9aaf0b08 (2026-06-15): operator's "commit all unless
        // super-good reason" policy. The new default keeps ONLY
        // session-scratch patterns; everything else (audit/, evidence/,
        // screenshots/, media, notes) is committed.
        let patterns = default_untracked_exclude_patterns();

        // Patterns that MUST be present (super-good reasons):
        for required in [
            "**/scratch/**",
            "**/scratch-*",
            "**/scratch_*",
            "**/tmp/**",
            "**/tmp-*",
            "**/pi-tmp/**",
            "**/.pi-tmp/**",
            ".demon/**",
            ".sisyphus/**",
            ".ralph/**",
        ] {
            assert!(
                patterns.contains(&required.to_string()),
                "default_untracked_exclude_patterns must contain `{}` (super-good reason), got: {:?}",
                required,
                patterns
            );
        }

        // Patterns that MUST NOT be present (operator wants committed):
        for forbidden in [
            "**/audit/**",     // intentional audit evidence
            "**/evidence/**",  // intentional evidence
            "**/screenshots/**", // intentional screenshots
            "*.png",           // media
            "*.jpg",
            "*.jpeg",
            "*.gif",
            "*.webp",
            "*.mp4",
            "*.mov",
            "**/note.md",      // notes
            "**/notes.md",
            "**/NOTE.md",
            "**/scratch.md",
        ] {
            assert!(
                !patterns.contains(&forbidden.to_string()),
                "default_untracked_exclude_patterns must NOT contain `{}` (operator wants committed), got: {:?}",
                forbidden,
                patterns
            );
        }
    }

    #[test]
    fn test_default_max_stage_file_bytes() {
        let bytes = default_max_stage_file_bytes();
        assert_eq!(bytes, 100 * 1024 * 1024);
    }

    #[test]
    fn test_default_pull_op_timeout_secs() {
        let secs = default_pull_op_timeout_secs();
        assert_eq!(secs, 30);
    }

    #[test]
    fn test_default_push_op_timeout_secs() {
        let secs = default_push_op_timeout_secs();
        assert_eq!(secs, 300);
    }

    #[test]
    fn test_default_repo_sync_timeout_secs() {
        let secs = default_repo_sync_timeout_secs();
        assert_eq!(secs, 420);
    }

    #[test]
    fn test_default_push_retries() {
        let retries = default_push_retries();
        assert_eq!(retries, 3);
    }

    #[test]
    fn test_default_repair_cooldown_secs() {
        let secs = default_repair_cooldown_secs();
        assert_eq!(secs, 60);
    }

    #[test]
    fn test_default_stage_op_timeout_secs() {
        let secs = default_stage_op_timeout_secs();
        assert_eq!(secs, 60);
        assert!(secs >= 10, "stage timeout must be at least 10s");
    }

    #[test]
    fn test_default_stage_cooldown_secs() {
        let secs = default_stage_cooldown_secs();
        // 1 hour default — long enough to stop the incident ledger from
        // being spammed every ~70s by repos whose working tree is too
        // large to stage within stage_op_timeout_secs.
        assert_eq!(secs, 3600);
    }

    #[test]
    fn test_default_max_push_blob_bytes() {
        let bytes = default_max_push_blob_bytes();
        assert_eq!(bytes, DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES);
    }

    #[test]
    fn test_default_incident_ledger_max_lines() {
        let lines = default_incident_ledger_max_lines();
        assert_eq!(lines, 10_000);
    }

    #[test]
    fn test_default_incident_ledger_max_age_days() {
        let days = default_incident_ledger_max_age_days();
        assert_eq!(days, 30);
    }

    #[test]
    fn test_debug_enabled() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_DEBUG", "");
        assert!(!debug_enabled());
    }

    #[test]
    fn test_default_true() {
        assert!(default_true());
    }

    #[test]
    fn test_default_pulse_interval() {
        assert_eq!(default_pulse_interval(), 1);
    }

    #[test]
    fn test_default_inactivity_push_delay_secs() {
        assert_eq!(default_inactivity_push_delay_secs(), 2);
    }

    #[test]
    fn test_default_trusted_emails() {
        let emails = default_trusted_emails();
        assert!(emails.contains(&"dracsharp@gmail.com".to_string()));
    }

    #[test]
    fn test_default_trusted_authors() {
        let authors = default_trusted_authors();
        assert!(authors.contains(&"DraconDev".to_string()));
    }

    #[test]
    fn test_default_trusted_remote_hosts_includes_three_platforms() {
        let hosts = default_trusted_remote_hosts();
        assert!(hosts.iter().any(|h| h.contains("github.com")));
        assert!(hosts.iter().any(|h| h.contains("gitlab.com")));
        assert!(hosts.iter().any(|h| h.contains("codeberg.org")));
    }

    #[test]
    fn test_default_settling_max_delay_secs_is_60() {
        assert_eq!(default_settling_max_delay_secs(), 60);
    }

    #[test]
    fn test_default_min_commit_interval_secs_is_5() {
        assert_eq!(default_min_commit_interval_secs(), 5);
    }

    #[test]
    fn test_default_dirty_max_age_action_is_commit() {
        assert_eq!(default_dirty_max_age_action(), DirtyMaxAgeAction::Commit);
    }

    #[test]
    fn test_dirty_max_age_action_default_is_commit() {
        assert_eq!(DirtyMaxAgeAction::default(), DirtyMaxAgeAction::Commit);
    }

    #[test]
    fn test_dirty_max_age_action_serde_kebab_case() {
        // Verify the serde rename_all = "kebab-case" works
        // both ways. The TOML is "commit" (or
        // "warn" or "ignore") and the Rust enum is
        // `Commit` / `Warn` / `Ignore`.
        #[derive(serde::Deserialize)]
        struct Wrap {
            action: DirtyMaxAgeAction,
        }
        let toml = "action = \"warn\"\n";
        let w: Wrap = toml::from_str(toml).expect("parse warn");
        assert_eq!(w.action, DirtyMaxAgeAction::Warn);

        let toml = "action = \"commit\"\n";
        let w: Wrap = toml::from_str(toml).expect("parse commit");
        assert_eq!(w.action, DirtyMaxAgeAction::Commit);

        let toml = "action = \"ignore\"\n";
        let w: Wrap = toml::from_str(toml).expect("parse ignore");
        assert_eq!(w.action, DirtyMaxAgeAction::Ignore);
    }

    #[test]
    fn test_test_sync_policy_has_new_fields() {
        // Regression: the test fixture must be kept in sync
        // with the SyncPolicy struct. If a new field is
        // added, the test fixture will fail to compile
        // (good — forces the author to think about the
        // default).
        let p = test_sync_policy();
        assert!(p.auto_skip_unowned);
        assert!(!p.trusted_emails.is_empty());
        assert!(!p.trusted_authors.is_empty());
        assert!(!p.trusted_remote_hosts.is_empty());
        assert_eq!(p.settling_max_delay_secs, 60);
        assert_eq!(p.dirty_max_age_action, DirtyMaxAgeAction::Commit);
        assert_eq!(p.min_commit_interval_secs, 5);
    }

    #[test]
    fn test_repo_override_parses_new_fields() {
        // Per-repo override TOML should parse the new
        // fields. Backward compat: missing fields
        // default to None / inherit global.
        let toml = r#"
owned = true
auto_skip_unowned = false
settling_max_delay_secs = 30
dirty_max_age_action = "warn"
"#;
        let parsed: RepoPolicyOverride =
            toml::from_str(toml).expect("parse override");
        assert_eq!(parsed.owned, Some(true));
        assert_eq!(parsed.auto_skip_unowned, Some(false));
        assert_eq!(parsed.settling_max_delay_secs, Some(30));
        assert_eq!(parsed.dirty_max_age_action, Some(DirtyMaxAgeAction::Warn));
    }

    #[test]
    fn test_repo_override_missing_new_fields_defaults_none() {
        // Backward compat: an old override TOML without
        // the new fields should parse with all new
        // fields as None.
        let toml = r#"
auto_bump_versions = false
"#;
        let parsed: RepoPolicyOverride =
            toml::from_str(toml).expect("parse old override");
        assert_eq!(parsed.owned, None);
        assert_eq!(parsed.auto_skip_unowned, None);
        assert_eq!(parsed.settling_max_delay_secs, None);
        assert_eq!(parsed.dirty_max_age_action, None);
    }

    #[test]
    fn test_default_sem_max_concurrent_sync_is_four() {
        assert_eq!(default_sem_max_concurrent_sync(), 4);
    }

    /// Regression test: the apply-phase deadline is derived from
    /// `pulse_interval_secs` so the daemon main loop is bounded
    /// against a slow push. The deadline is `pulse_interval_secs * 2`
    /// with a minimum of 2s.
    #[test]
    fn test_apply_deadline_derived_from_pulse_interval() {
        // Default pulse_interval_secs is 1; the deadline is 1*2 = 2.
        let p = SyncPolicy::default();
        let expected = (p.pulse_interval_secs.max(1) * 2) as u64;
        assert_eq!(expected, 2);
    }

    #[test]
    fn test_git_host_blob_limit() {
        assert_eq!(DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES, 100 * 1024 * 1024);
    }

    #[test]
    fn test_timestamp_secs_returns_reasonable_value() {
        let ts = timestamp_secs();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(ts > 0);
        assert!(ts <= now + 1);
    }

    #[test]
    fn test_sync_policy_defaults_produce_valid_values() {
        let policy = test_sync_policy();
        assert!(policy.pulse_interval_secs >= 1);
        assert!(policy.inactivity_push_delay_secs >= 1);
        assert!(policy.max_stage_file_bytes > 0);
        assert!(policy.pull_op_timeout_secs >= 5);
        assert!(policy.push_op_timeout_secs >= 10);
    }

    fn test_sync_policy() -> SyncPolicy {
        super::test_sync_policy()
    }

    #[test]
    fn test_repo_policy_override_default() {
        let override_default = crate::policy::RepoPolicyOverride::default();
        assert!(override_default.auto_bump_versions.is_none());
    }

    #[test]
    fn test_freeze_marker_paths() {
        let paths = freeze_marker_paths(std::path::Path::new("/fake/path.toml"));
        assert!(!paths.is_empty());
    }

    #[test]
    fn test_load_repo_override_nonexistent() {
        let repo = std::path::Path::new("/nonexistent/path/for/test");
        let override_ = load_repo_override(repo);
        assert!(override_.auto_bump_versions.is_none());
    }

    #[test]
    fn test_load_repo_override_intentional_no_upstream() {
        // A repo's per-repo `.dracon/dracon-sync.toml` setting
        // `intentional_no_upstream = true` must round-trip through
        // `load_repo_override`. Default when the file is absent must
        // remain `false` so the existing concern classification path
        // is unchanged for any repo that has not opted in.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join(".dracon")).unwrap();
        std::fs::write(
            repo.join(".dracon/dracon-sync.toml"),
            "intentional_no_upstream = true\n",
        )
        .unwrap();
        let override_ = load_repo_override(repo);
        assert!(override_.intentional_no_upstream);
    }

    #[test]
    fn test_load_repo_override_intentional_no_upstream_default_false() {
        // When the per-repo override file is absent, the field must
        // default to false so the new opt-in is non-breaking.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let override_ = load_repo_override(repo);
        assert!(!override_.intentional_no_upstream);
    }

    static POLICY_ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct VarGuard {
        var: String,
        original: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl VarGuard {
        fn set_temp(var: &str, value: &str) -> Self {
            let lock = POLICY_ENV_GUARD.lock().unwrap();
            let original = std::env::var(var).ok();
            if value.is_empty() {
                std::env::remove_var(var);
            } else {
                std::env::set_var(var, value);
            }
            Self {
                var: var.to_string(),
                original,
                _lock: lock,
            }
        }
    }
    impl Drop for VarGuard {
        fn drop(&mut self) {
            if let Some(orig) = self.original.take() {
                std::env::set_var(&self.var, orig);
            } else {
                std::env::remove_var(&self.var);
            }
        }
    }

    #[test]
    fn test_env_freeze_enabled_ignores_case() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "TRUE");
        assert!(env_freeze_enabled());
    }

    #[test]
    fn test_env_freeze_enabled_accepts_yes() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "yes");
        assert!(env_freeze_enabled());
    }

    #[test]
    fn test_env_freeze_enabled_accepts_on() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "on");
        assert!(env_freeze_enabled());
    }

    #[test]
    fn test_env_freeze_enabled_rejects_false() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "false");
        assert!(!env_freeze_enabled());
    }

    #[test]
    fn test_env_freeze_enabled_rejects_empty() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "");
        assert!(!env_freeze_enabled());
    }

    #[test]
    fn test_debug_enabled_accepts_1() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_DEBUG", "1");
        assert!(debug_enabled());
    }

    #[test]
    fn test_debug_enabled_rejects_empty() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_DEBUG", "");
        assert!(!debug_enabled());
    }

    #[test]
    fn test_freeze_reason_env_takes_precedence() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "1");
        let reason = freeze_reason(std::path::Path::new("/fake/policy.toml"));
        assert_eq!(reason, Some("env DRACON_SYNC_FREEZE".to_string()));
    }

    #[test]
    fn test_freeze_reason_none_when_not_frozen() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_FREEZE", "");
        let reason = freeze_reason(std::path::Path::new("/fake/policy.toml"));
        assert!(reason.is_none());
    }

    #[test]
    fn test_freeze_marker_paths_includes_dracondir() {
        let paths = freeze_marker_paths(std::path::Path::new("/fake.toml"));
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().contains(".dracon")));
        assert!(paths.iter().any(|p| p.to_string_lossy().contains("freeze")));
    }

    #[test]
    fn test_resolve_policy_path_env_override() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_POLICY", "/custom/policy.toml");
        let path = resolve_policy_path();
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), PathBuf::from("/custom/policy.toml"));
    }

    #[test]
    fn test_sync_policy_watch_roots_filters_nonexistent() {
        let policy = SyncPolicy {
            watch_roots: vec![
                "/nonexistent/path/one".to_string(),
                "/nonexistent/path/two".to_string(),
            ],
            ..test_sync_policy()
        };
        let roots = policy.watch_root_paths();
        assert!(roots.is_empty());
    }

    #[test]
    fn test_timestamp_secs_returns_increasing_values() {
        let ts1 = timestamp_secs();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ts2 = timestamp_secs();
        assert!(ts2 >= ts1);
    }

    #[test]
    fn test_resolve_push_url_template_substitution() {
        let config = RemoteConfig {
            name: "github".to_string(),
            push_url: "git@github.com:{account}/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "myorg".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        assert_eq!(
            config.resolve_push_url("my-repo"),
            "git@github.com:myorg/my-repo.git"
        );
    }

    #[test]
    fn test_resolve_push_url_no_template() {
        let config = RemoteConfig {
            name: "mirror".to_string(),
            push_url: "git@mirror.example.com:fixed/path.git".to_string(),
            auto_create: false,
            auto_create_account: "".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        assert_eq!(
            config.resolve_push_url("any-repo"),
            "git@mirror.example.com:fixed/path.git"
        );
    }

    #[test]
    fn test_resolve_push_url_account_only() {
        let config = RemoteConfig {
            name: "gitlab".to_string(),
            push_url: "git@gitlab.com:{account}/".to_string(),
            auto_create: false,
            auto_create_account: "testuser".to_string(),
            auth_type: AuthType::GitLab,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        assert_eq!(config.resolve_push_url("repo"), "git@gitlab.com:testuser/");
    }

    #[test]
    fn test_resolve_push_url_with_name_mapping() {
        let mut config = RemoteConfig {
            name: "gitlab".to_string(),
            push_url: "git@gitlab.com:{account}/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "myorg".to_string(),
            auth_type: AuthType::GitLab,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        config
            .repo_name_map
            .insert(".dracon".to_string(), "dracon-home".to_string());

        assert_eq!(
            config.resolve_push_url(".dracon"),
            "git@gitlab.com:myorg/dracon-home.git"
        );
        assert_eq!(
            config.resolve_push_url("other-repo"),
            "git@gitlab.com:myorg/other-repo.git"
        );
    }

    #[test]
    fn test_resolve_repo_name_with_mapping() {
        let mut config = RemoteConfig {
            name: "gitlab".to_string(),
            push_url: "git@gitlab.com:{account}/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "myorg".to_string(),
            auth_type: AuthType::GitLab,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        config
            .repo_name_map
            .insert(".dracon".to_string(), "dracon-home".to_string());

        assert_eq!(config.resolve_repo_name(".dracon"), "dracon-home");
        assert_eq!(config.resolve_repo_name("other-repo"), "other-repo");
    }

    #[test]
    fn test_resolve_repo_name_without_mapping() {
        let config = RemoteConfig {
            name: "github".to_string(),
            push_url: "https://github.com/{account}/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "myorg".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };

        assert_eq!(config.resolve_repo_name(".dracon"), ".dracon");
        assert_eq!(config.resolve_repo_name("my-repo"), "my-repo");
    }

    #[test]
    fn test_validate_config_valid_policy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
auto_commit = true
auto_pull = true
auto_push = true
auto_bump_versions = false
watch_roots = ["/tmp"]
remotes = []
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(
            result.is_valid(),
            "valid policy should pass: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_validate_config_warns_on_short_stage_cooldown_and_timeouts() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = ["/tmp"]
remotes = []
stage_op_timeout_secs = 5
stage_cooldown_secs = 10
pull_op_timeout_secs = 1
push_op_timeout_secs = 5
repo_sync_timeout_secs = 5
inactivity_push_delay_secs = 0
repair_cooldown_secs = 1
incident_ledger_max_lines = 10
incident_ledger_max_age_days = 0
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(result.is_valid(), "short values warn but remain valid");
        for expected in [
            "stage_op_timeout_secs 5",
            "stage_cooldown_secs 10",
            "pull_op_timeout_secs 1",
            "push_op_timeout_secs 5",
            "repo_sync_timeout_secs 5",
            "inactivity_push_delay_secs = 0",
            "repair_cooldown_secs 1",
            "incident_ledger_max_lines 10",
            "incident_ledger_max_age_days = 0",
        ] {
            assert!(
                result.warnings.iter().any(|w| w.contains(expected)),
                "missing warning for {expected}: {:?}",
                result.warnings
            );
        }
    }

    #[test]
    fn test_validate_config_missing_watch_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = ["/nonexistent/path/that/does/not/exist"]
remotes = []
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(!result.is_valid(), "missing watch root should fail");
        assert!(
            result.errors.iter().any(|e| e.contains("does not exist")),
            "should mention missing path"
        );
    }

    #[test]
    fn test_validate_config_invalid_webhook_url() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = ["/tmp"]
remotes = []
webhook_url = "ftp://invalid.example.com/hook"
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(!result.is_valid(), "non-http webhook URL should fail");
        assert!(
            result.errors.iter().any(|e| e.contains("webhook_url")),
            "should mention webhook_url"
        );
    }

    #[test]
    fn test_validate_config_empty_remote_push_url() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = ["/tmp"]
[[remotes]]
name = "test"
push_url = ""
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(!result.is_valid(), "empty push_url should fail");
    }

    #[test]
    fn test_validate_config_empty_auto_create_account_is_silent() {
        // Empty auto_create_account is silently accepted: resolve_account() extracts
        // the account from the push_url. The config is valid AND the warning is
        // intentionally suppressed (it was noise — see policy.rs).
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = ["/tmp"]
[[remotes]]
name = "github"
push_url = "https://github.com/{account}/{repo}.git"
auto_create = true
auto_create_account = ""
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(
            result.is_valid(),
            "auto_create=true with empty account is valid (account extracted from push_url)"
        );
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.contains("auto_create_account")),
            "empty auto_create_account should not warn (resolve_account() handles it), got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_validate_config_no_watch_roots_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = []
remotes = []
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(!result.is_valid(), "no watch_roots should fail");
        assert!(
            result.errors.iter().any(|e| e.contains("watch_roots")),
            "should mention watch_roots"
        );
    }

    #[test]
    fn test_validate_config_warns_on_no_remotes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let content = r#"
auto_github_private = false
watch_roots = ["/tmp"]
remotes = []
"#;
        std::fs::write(tmp.path().join("policy.toml"), content).unwrap();
        let result = validate_config(tmp.path().join("policy.toml").as_path());
        assert!(result.is_valid(), "no remotes is a warning not error");
        assert!(
            result.warnings.iter().any(|w| w.contains("no remotes")),
            "should warn about no remotes"
        );
    }

    #[test]
    fn test_standard_files_short_form() {
        let toml = r#"
standard_files = ["LICENSE"]
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.standard_files.len(), 1);
        assert_eq!(policy.standard_files[0].source, "templates/LICENSE");
        assert_eq!(policy.standard_files[0].target, "LICENSE");
        assert!(!policy.standard_files[0].overwrite);
    }

    #[test]
    fn test_standard_files_short_form_funding_yml() {
        // FUNDING.yml is the second default standard file. The short form must
        // resolve its source to templates/FUNDING.yml and target to the repo
        // root name. Repos that need the GitHub-required .github/ subdir must
        // use the long form.
        let toml = r#"
standard_files = ["LICENSE", "FUNDING.yml"]
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.standard_files.len(), 2);
        assert_eq!(policy.standard_files[1].source, "templates/FUNDING.yml");
        assert_eq!(policy.standard_files[1].target, "FUNDING.yml");
        assert!(!policy.standard_files[1].overwrite);
    }

    #[test]
    fn test_standard_files_funding_yml_github_subdir_long_form() {
        // GitHub discovers FUNDING.yml at .github/FUNDING.yml. The long form
        // is required to install the file in that subdirectory while still
        // pulling the template from templates/FUNDING.yml.
        let toml = r#"
[[standard_files]]
source = "templates/LICENSE"
target = "LICENSE"

[[standard_files]]
source = "templates/FUNDING.yml"
target = ".github/FUNDING.yml"
overwrite = false
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.standard_files.len(), 2);
        let funding = &policy.standard_files[1];
        assert_eq!(funding.source, "templates/FUNDING.yml");
        assert_eq!(funding.target, ".github/FUNDING.yml");
        assert!(!funding.overwrite);
    }

    #[test]
    fn test_standard_files_long_form() {
        let toml = r#"
[[standard_files]]
source = "templates/CUSTOM"
target = "NOTICE"
overwrite = true
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.standard_files.len(), 1);
        assert_eq!(policy.standard_files[0].source, "templates/CUSTOM");
        assert_eq!(policy.standard_files[0].target, "NOTICE");
        assert!(policy.standard_files[0].overwrite);
    }

    #[test]
    fn test_standard_files_mixed_form() {
        let toml = r#"
[[standard_files]]
source = "templates/LICENSE"
target = "LICENSE"

[[standard_files]]
source = "templates/CUSTOM"
target = "NOTICE"
overwrite = true
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.standard_files.len(), 2);
        assert_eq!(policy.standard_files[0].source, "templates/LICENSE");
        assert_eq!(policy.standard_files[0].target, "LICENSE");
        assert!(!policy.standard_files[0].overwrite);
        assert_eq!(policy.standard_files[1].source, "templates/CUSTOM");
        assert_eq!(policy.standard_files[1].target, "NOTICE");
        assert!(policy.standard_files[1].overwrite);
    }

    #[test]
    fn test_standard_files_empty() {
        let toml = r#"
standard_files = []
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert!(policy.standard_files.is_empty());
    }

    #[test]
    fn test_standard_files_default_empty() {
        let toml = r#"
pulse_interval_secs = 1
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert!(policy.standard_files.is_empty());
    }

    #[test]
    fn test_default_policy_does_not_force_funding_yml() {
        // FUNDING.yml is Dracon-specific. External users must opt in explicitly;
        // the default policy must not force it.
        let policy = SyncPolicy::default();
        assert!(policy.standard_files.is_empty());
        assert!(!policy
            .standard_files
            .iter()
            .any(|cfg| cfg.target == "FUNDING.yml" || cfg.target == ".github/FUNDING.yml"));
    }

    #[test]
    fn test_license_only_policy_does_not_force_funding_yml() {
        // A generic external user can keep the starter policy with only LICENSE.
        let toml = r#"
standard_files = ["LICENSE"]
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.standard_files.len(), 1);
        assert_eq!(policy.standard_files[0].target, "LICENSE");
        assert!(!policy
            .standard_files
            .iter()
            .any(|cfg| cfg.target == "FUNDING.yml" || cfg.target == ".github/FUNDING.yml"));
    }

    #[test]
    fn test_standard_files_auto_default_true() {
        let toml = r#"
pulse_interval_secs = 1
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert!(policy.standard_files_auto);
    }

    #[test]
    fn test_standard_files_auto_explicit_true() {
        let toml = r#"
pulse_interval_secs = 1
standard_files_auto = true
"#;
        let policy: SyncPolicy = toml::from_str(toml).unwrap();
        assert!(policy.standard_files_auto);
    }

    #[test]
    fn test_validate_toml_field_ordering_warns_on_post_section_fields() {
        use crate::policy::ValidateResult;
        let toml = r#"
pulse_interval_secs = 1

[sync]
auto_pull = true

[remote "origin"]
url = "git@github.com:foo/bar.git"
"#;
        let content = toml;
        let mut result = ValidateResult::default();
        check_toml_field_ordering(content, &mut result);
        assert!(
            result.warnings.iter().any(|w| {
                w.contains("'auto_pull' appears after a section header")
                    || w.contains("'url' appears after a section header")
            }),
            "expected warning about fields after section, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_validate_toml_field_ordering_ok_with_all_fields_before_sections() {
        use crate::policy::ValidateResult;
        let toml = r#"
pulse_interval_secs = 1
standard_files_auto = true
auto_github_private = false
"#;
        let content = toml;
        let mut result = ValidateResult::default();
        check_toml_field_ordering(content, &mut result);
        assert!(
            result.warnings.is_empty(),
            "expected no warnings, got: {:?}",
            result.warnings
        );
    }

    /// Goal 546d4f9c: durability check. The `dracon-sync.example.toml`
    /// must stay in sync with the code defaults. If a future change
    /// updates one but not the other, the operator's "commit all
    /// unless super-good reason" policy silently regresses on fresh
    /// installs. This test catches that drift.
    ///
    /// Date: 2026-06-15 (goal 9aaf0b08 / 546d4f9c).
    #[test]
    fn test_example_toml_matches_policy_defaults() {
        use std::path::PathBuf;
        // `dracon-sync.example.toml` lives at
        // <workspace>/dracon-sync/dracon-sync.example.toml.
        // CARGO_MANIFEST_DIR points at <workspace>/dracon-sync
        // for the dracon-sync crate, so the file is at
        // $CARGO_MANIFEST_DIR/dracon-sync.example.toml.
        let example_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("dracon-sync.example.toml");
        let content = std::fs::read_to_string(&example_path)
            .unwrap_or_else(|e| panic!(
                "could not read example config at {}: {}",
                example_path.display(), e
            ));
        // Parse the example config into a `SyncPolicy` so the
        // comparison exercises the same loader a real daemon uses.
        let example: SyncPolicy = toml::from_str(&content)
            .expect("dracon-sync.example.toml must parse as SyncPolicy");

        // 1. `exclude_file_patterns` in the example must be
        //    empty, matching the code default of
        //    `default_exclude_file_patterns() = Vec::new()`.
        //    Logs and DBs are committed by default under the
        //    operator's "commit all unless super-good reason"
        //    policy.
        let example_excluded_files = &example.exclude_file_patterns;
        assert!(
            example_excluded_files.is_empty(),
            "example.toml exclude_file_patterns must be empty \
             (commit logs/DBs by default), got: {:?}",
            example_excluded_files
        );
        assert_eq!(
            example_excluded_files,
            &default_exclude_file_patterns(),
            "example.toml exclude_file_patterns must match code \
             default (drift = silent regression)"
        );

        // 2. `untracked_exclude_patterns` in the example must
        //    match the code default
        //    `default_untracked_exclude_patterns()`. The example
        //    is the recommended config; if it diverges from the
        //    code default, fresh installs get a different policy
        //    than expected.
        let example_untracked = &example.untracked_exclude_patterns;
        let default_untracked = default_untracked_exclude_patterns();
        assert_eq!(
            example_untracked, &default_untracked,
            "example.toml untracked_exclude_patterns must match \
             code default (drift = silent regression on fresh \
             install).\n  example: {:?}\n  default: {:?}",
            example_untracked, default_untracked
        );

        // 3. `max_stage_file_bytes` in the example must equal the
        //    code default (100 MiB).
        let example_max = example.max_stage_file_bytes;
        assert_eq!(
            example_max, default_max_stage_file_bytes(),
            "example.toml max_stage_file_bytes must match code \
             default (drift = silent regression)"
        );
    }
}
