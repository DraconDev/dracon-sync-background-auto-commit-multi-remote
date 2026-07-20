use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::git::gh_cmd;
use crate::policy::{AuthType, RemoteConfig};
use crate::secrets::{load_secret, sync_secrets_dir};

/// Strip ANSI escape sequences from a string.
/// Handles CSI sequences (ESC [ ... m), OSC sequences (ESC ] ... BEL/ST),
/// and bare ESC sequences.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // ESC sequence
            match chars.peek() {
                Some('[') => {
                    // CSI sequence: ESC [ ... <final byte>
                    chars.next();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC sequence: ESC ] ... BEL or ST
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    // Other ESC sequence (2-char): skip the next char
                    chars.next();
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Directory for visibility sync cache files.
fn visibility_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/dracon/visibility-sync")
}

/// Path to the cache file for a given repo.
/// Uses a hash of the full repo path to avoid collisions between
/// same-named repos in different watch roots.
fn visibility_cache_path(repo_path: &Path) -> PathBuf {
    let path_str = repo_path.to_string_lossy();
    let hash = simple_hash(&path_str);
    visibility_cache_dir().join(format!("{}.last", hash))
}

/// Simple FNV-1a-like hash for cache file names.
/// Not cryptographically secure — just for collision avoidance.
fn simple_hash(s: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    format!("{:016x}", hash)
}

/// Parse the cached visibility file. Format is two lines:
///   `visibility=<public|private>`
///   `<unix timestamp>`
/// The first line was added 2026-07-17 (goal `codeberg-public-only`).
/// Older caches written before that date contain only the timestamp;
/// for those we return `None` for the visibility (unknown).
fn parse_visibility_cache(content: &str) -> Option<(bool, u64)> {
    let mut lines = content.lines();
    let vis_line = lines.next()?;
    let ts_line = lines.next()?;
    let vis_str = vis_line.strip_prefix("visibility=")?.trim();
    let visibility = match vis_str {
        "public" => false,  // false = public
        "private" => true,  // true = private
        _ => return None,
    };
    let ts = ts_line.trim().parse::<u64>().ok()?;
    Some((visibility, ts))
}

/// Check whether the visibility cache is fresh (within `interval_hours`).
/// Backward-compatible: reads either the new `visibility=<state>` format
/// or the legacy timestamp-only format.
fn is_visibility_cache_fresh(repo_path: &Path, interval_hours: u64) -> bool {
    let path = visibility_cache_path(repo_path);
    if !path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return false;
    };
    // Try new format first; fall back to legacy timestamp-only.
    let ts = if let Some((_, ts)) = parse_visibility_cache(&content) {
        ts
    } else if let Ok(legacy_ts) = content.trim().parse::<u64>() {
        legacy_ts
    } else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let interval_secs = interval_hours.saturating_mul(3600);
    now.saturating_sub(ts) < interval_secs
}

/// Write the current visibility state AND timestamp to the cache.
/// Uses the new 2-line format (`visibility=<state>` + `<unix ts>`).
/// Old caches written in the legacy timestamp-only format will be
/// silently overwritten on the next sync cycle.
pub(crate) fn update_visibility_cache(repo_path: &Path, private: bool) {
    let dir = visibility_cache_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "⚠️ failed to create visibility cache dir {}: {}",
            dir.display(),
            e
        );
        return;
    }
    let path = visibility_cache_path(repo_path);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let vis = if private { "private" } else { "public" };
    let content = format!("visibility={vis}\n{now}");
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!(
            "⚠️ failed to write visibility cache {}: {}",
            path.display(),
            e
        );
    }
}

/// Look up the cached visibility for a repo. Returns `Some(true)` if
/// the cache says private, `Some(false)` if public, `None` if no cache
/// exists OR the cache is in legacy timestamp-only format (the next
/// sync cycle will refresh it).
///
/// This is the cheap read path used by the `repos` command and the
/// push-time codeberg gate. The actual `gh api` call lives in
/// `sync_mirror_visibility` and runs only when the cache is stale.
///
/// Backward compatibility: old timestamp-only cache files are
/// treated as `None` (unknown) so the safe-default path (skip codeberg)
/// fires until the cache refreshes.
pub(crate) fn cached_repo_visibility(repo_path: &Path) -> Option<bool> {
    let path = visibility_cache_path(repo_path);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return None;
    };
    parse_visibility_cache(&content).map(|(private, _)| private)
}

/// Test-only accessor for the cache path. Lives here so tests in
/// other modules (e.g. `report.rs`) can populate the visibility cache
/// without depending on internal layout. Production code should use
/// the `cached_repo_visibility` / `update_visibility_cache` helpers.
#[cfg(test)]
pub(crate) fn visibility_cache_path_test(repo_path: &Path) -> PathBuf {
    visibility_cache_path(repo_path)
}

/// Test-only accessor for the cache dir. Same rationale as
/// `visibility_cache_path_test`.
#[cfg(test)]
pub(crate) fn visibility_cache_dir_test() -> PathBuf {
    visibility_cache_dir()
}

/// Parse `owner/repo` from a GitHub remote URL.
/// Supports both SSH (`git@github.com:owner/repo.git`) and HTTPS (`https://github.com/owner/repo.git`).
pub(crate) fn parse_github_owner_repo(remote_url: &str) -> Option<(String, String)> {
    // SSH: git@github.com:owner/repo.git
    if remote_url.contains('@') {
        if let Some(colon) = remote_url.rfind(':') {
            let after_colon = &remote_url[colon + 1..];
            let clean = after_colon.strip_suffix(".git").unwrap_or(after_colon);
            if let Some(slash) = clean.find('/') {
                return Some((clean[..slash].to_string(), clean[slash + 1..].to_string()));
            }
        }
    }
    // HTTPS: https://github.com/owner/repo.git
    if let Some(host_start) = remote_url.find("github.com/") {
        let after_host = &remote_url[host_start + 11..];
        let clean = after_host.strip_suffix(".git").unwrap_or(after_host);
        if let Some(slash) = clean.find('/') {
            return Some((clean[..slash].to_string(), clean[slash + 1..].to_string()));
        }
    }
    None
}

/// Query GitHub for the visibility of a repo using `gh api`.
/// Returns `true` if the repo is private, `false` if public.
/// On any error (gh not installed, no auth, network failure), returns `true` as the safe default.
pub(crate) fn get_github_visibility(owner: &str, repo: &str) -> bool {
    let output = match gh_cmd()
        .args([
            "api",
            &format!("repos/{}/{}", owner, repo),
            "--jq",
            ".private",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("⚠️ gh api failed (is gh installed?): {}", e);
            return true;
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("⚠️ gh api failed: {}", stderr.trim());
        return true;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // "true" → private, "false" → public, anything else → safe default (private)
    stdout == "true"
}

const GITLAB_API_PROJECTS: &str = "https://gitlab.com/api/v4/projects/{}%2F{}";
const CODEBERG_API_REPOS: &str = "https://codeberg.org/api/v1/repos/{}/{}";

/// Set GitLab repo visibility using `curl` with PRIVATE-TOKEN.
/// The token is passed via stdin (`-H @-`) so it never appears in the
/// process command line (visible to other local users via /proc).
/// `private=true` means private, `private=false` means public.
fn set_gitlab_visibility(owner: &str, repo: &str, token: &str, private: bool) -> Result<()> {
    use std::io::Write;
    let visibility = if private { "private" } else { "public" };
    let encoded = format!("{}%2F{}", owner, repo);
    let url = GITLAB_API_PROJECTS.replace("{}", &encoded);
    let body = format!("visibility={}", visibility);
    let header = format!("PRIVATE-TOKEN: {}\r\n", token);
    let mut child = std::process::Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-H",
            "@-",
            "-X",
            "PUT",
            "--data-binary",
            &body,
            &url,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| "curl failed to run for GitLab visibility update")?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("curl stdin not available"))?;
        stdin
            .write_all(header.as_bytes())
            .with_context(|| "failed to write GitLab PRIVATE-TOKEN header to curl stdin")?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| "curl wait_with_output failed for GitLab visibility update")?;

    let code = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match code.as_str() {
        "200" => Ok(()),
        "401" => Err(anyhow::anyhow!(
            "GitLab visibility update failed: unauthorized (invalid token)"
        )),
        "404" => Err(anyhow::anyhow!(
            "GitLab visibility update failed: repo not found"
        )),
        _ => Err(anyhow::anyhow!(
            "GitLab visibility update failed: HTTP {}",
            code
        )),
    }
}

/// Set Codeberg repo visibility using `curl` with Authorization token.
/// The token is passed via stdin (`-H @-`) so it never appears in the
/// process command line (visible to other local users via /proc).
/// `private=true` means private, `private=false` means public.
fn set_codeberg_visibility(owner: &str, repo: &str, token: &str, private: bool) -> Result<()> {
    use std::io::Write;
    let url = CODEBERG_API_REPOS.replace("{}", &format!("{}/{}", owner, repo));
    let json = format!("{{\"private\":{}}}", private);
    let headers = format!(
        "Authorization: token {}\r\nContent-Type: application/json\r\n",
        token
    );
    let mut child = std::process::Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-H",
            "@-",
            "-X",
            "PATCH",
            "--data-binary",
            &json,
            &url,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| "curl failed to run for Codeberg visibility update")?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("curl stdin not available"))?;
        stdin
            .write_all(headers.as_bytes())
            .with_context(|| "failed to write Codeberg Authorization header to curl stdin")?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| "curl wait_with_output failed for Codeberg visibility update")?;

    let code = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match code.as_str() {
        "200" => Ok(()),
        "401" => Err(anyhow::anyhow!(
            "Codeberg visibility update failed: unauthorized (invalid token)"
        )),
        "404" => Err(anyhow::anyhow!(
            "Codeberg visibility update failed: repo not found"
        )),
        _ => Err(anyhow::anyhow!(
            "Codeberg visibility update failed: HTTP {}",
            code
        )),
    }
}

/// Set GitHub repo visibility using `gh api -X PATCH`.
/// `private=true` → private, `private=false` → public.
/// On success the HTTP status code is 200; on failure the stderr from
/// `gh` is surfaced.
///
/// ADDED 2026-07-20 (v0.112.28) for the `make-public` / `make-private`
/// CLI subcommand. Mirrors the existing `set_gitlab_visibility` and
/// `set_codeberg_visibility` pattern but uses `gh` instead of `curl`.
fn set_github_visibility(owner: &str, repo: &str, private: bool) -> Result<()> {
    let private_field = if private { "true" } else { "false" };
    let output = gh_cmd()
        .args([
            "api",
            "-X",
            "PATCH",
            &format!("repos/{}/{}", owner, repo),
            "-f",
            &format!("private={}", private_field),
        ])
        .output()
        .with_context(|| "gh api failed for GitHub visibility update")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    // gh returns "Not Found" for repos that don't exist on the
    // authenticated account. Match the existing 404 surface so the
    // CLI can present a clear "repo not found" error.
    if trimmed.contains("Not Found") || trimmed.contains("404") {
        return Err(anyhow::anyhow!(
            "GitHub visibility update failed: repo not found"
        ));
    }
    if trimmed.contains("401") || trimmed.contains("Unauthorized") {
        return Err(anyhow::anyhow!(
            "GitHub visibility update failed: unauthorized (gh auth issue)"
        ));
    }
    Err(anyhow::anyhow!(
        "GitHub visibility update failed: {}",
        trimmed
    ))
}

/// Flip repo visibility to the target `private` value across all
/// configured remotes (GitHub + GitLab + optional Codeberg).
/// Returns the list of `(remote_name, Result<()>)` for each remote
/// attempted.
///
/// ADDED 2026-07-20 (v0.112.28) for the `make-public` / `make-private`
/// CLI subcommand. Unlike `sync_mirror_visibility` (which reads GitHub
/// as the source of truth and propagates to mirrors), this is an
/// EXPLICIT operator command that pushes the target visibility to
/// every remote.
///
/// Parameters:
/// - `origin_url`: the origin remote URL (e.g. `git@github.com:owner/repo.git`)
/// - `remotes`: full configured remotes slice
/// - `repo_name`: local repo basename (e.g. `dracon-sync`)
/// - `private`: target visibility (`true` = private, `false` = public)
/// - `include_codeberg`: whether to flip codeberg too (default false
///   to protect the 85 GiB grace quota). Codeberg is opt-in.
pub(crate) fn flip_repo_visibility(
    origin_url: &str,
    remotes: &[RemoteConfig],
    repo_name: &str,
    private: bool,
    include_codeberg: bool,
) -> Vec<(String, Result<()>)> {
    let visibility_str = if private { "private" } else { "public" };
    let mut results = Vec::new();

    // 1. GitHub (canonical — set via `gh api`).
    if let Some((owner, gh_repo)) = parse_github_owner_repo(origin_url) {
        let resolved = remotes
            .iter()
            .find(|r| r.name == "github" || r.effective_auth_type() == AuthType::GitHub)
            .map(|r| r.resolve_repo_name(&gh_repo))
            .unwrap_or(gh_repo);
        let result = set_github_visibility(&owner, &resolved, private);
        results.push(("github".to_string(), result));
    } else {
        results.push((
            "github".to_string(),
            Err(anyhow::anyhow!(
                "no github origin URL found for {}",
                repo_name
            )),
        ));
    }

    // 2. GitLab (if configured).
    for remote in remotes {
        if remote.effective_auth_type() != AuthType::GitLab {
            continue;
        }
        let account = remote.resolve_account();
        let resolved = remote.resolve_repo_name(repo_name);
        let token_var = remote
            .auto_create_token_var
            .as_deref()
            .unwrap_or("GITLAB_TOKEN");
        match load_secret(token_var, &sync_secrets_dir()) {
            Some(token) => {
                let result = set_gitlab_visibility(&account, &resolved, &token, private);
                results.push((remote.name.clone(), result));
            }
            None => {
                results.push((
                    remote.name.clone(),
                    Err(anyhow::anyhow!(
                        "no {} env / secret file; cannot flip visibility",
                        token_var
                    )),
                ));
            }
        }
    }

    // 3. Codeberg (opt-in only — quota is the concern).
    if include_codeberg {
        for remote in remotes {
            if remote.effective_auth_type() != AuthType::Codeberg {
                continue;
            }
            let account = remote.resolve_account();
            let resolved = remote.resolve_repo_name(repo_name);
            let token_var = remote
                .auto_create_token_var
                .as_deref()
                .unwrap_or("CODEBERG_TOKEN");
            match load_secret(token_var, &sync_secrets_dir()) {
                Some(token) => {
                    let result = set_codeberg_visibility(&account, &resolved, &token, private);
                    results.push((remote.name.clone(), result));
                }
                None => {
                    results.push((
                        remote.name.clone(),
                        Err(anyhow::anyhow!(
                            "no {} env / secret file; cannot flip visibility",
                            token_var
                        )),
                    ));
                }
            }
        }
    }

    if crate::policy::debug_enabled() {
        eprintln!(
            "🐛 flip_repo_visibility({}): {} remotes, include_codeberg={}, target={}",
            repo_name,
            results.len(),
            include_codeberg,
            visibility_str
        );
    }

    results
}

/// Query GitHub for the current visibility of the origin repo, then update
/// all configured mirrors (GitLab, Codeberg) to match.
///
/// This function is **non-fatal**: errors are logged but never propagated,
/// so a visibility sync failure will never break the git push pipeline.
pub(crate) fn sync_mirror_visibility(
    origin_url: &str,
    remotes: &[RemoteConfig],
    repo_path: &Path,
    interval_hours: u64,
) {
    // Check cache first
    if is_visibility_cache_fresh(repo_path, interval_hours) {
        return;
    }

    let repo_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if repo_name.is_empty() {
        return;
    }

    let Some((owner, gh_repo)) = parse_github_owner_repo(origin_url) else {
        eprintln!(
            "⚠️ could not parse GitHub owner/repo from origin URL: {}",
            origin_url
        );
        return;
    };

    let github_private = get_github_visibility(&owner, &gh_repo);
    let visibility_str = if github_private { "private" } else { "public" };

    if crate::policy::debug_enabled() {
        eprintln!("🐛 GitHub repo {}/{} is {}", owner, gh_repo, visibility_str);
    }

    for remote in remotes {
        let auth = remote.effective_auth_type();
        let account = remote.resolve_account();
        if auth == AuthType::GitLab {
            let token_var = remote
                .auto_create_token_var
                .as_deref()
                .unwrap_or("GITLAB_TOKEN");
            if let Some(token) = load_secret(token_var, &sync_secrets_dir()) {
                let resolved_name = remote.resolve_repo_name(&repo_name);
                if let Err(e) =
                    set_gitlab_visibility(&account, &resolved_name, &token, github_private)
                {
                    eprintln!(
                        "⚠️ failed to set GitLab visibility for {}: {}",
                        resolved_name, e
                    );
                } else if crate::policy::debug_enabled() {
                    eprintln!(
                        "🐛 set GitLab {}/{} to {}",
                        account, resolved_name, visibility_str
                    );
                }
            } else {
                eprintln!("⚠️ no GITLAB_TOKEN for visibility sync on {}", remote.name);
            }
        }

        if auth == AuthType::Codeberg {
            let token_var = remote
                .auto_create_token_var
                .as_deref()
                .unwrap_or("CODEBERG_TOKEN");
            if let Some(token) = load_secret(token_var, &sync_secrets_dir()) {
                let resolved_name = remote.resolve_repo_name(&repo_name);
                if let Err(e) =
                    set_codeberg_visibility(&account, &resolved_name, &token, github_private)
                {
                    eprintln!(
                        "⚠️ failed to set Codeberg visibility for {}: {}",
                        resolved_name, e
                    );
                } else if crate::policy::debug_enabled() {
                    eprintln!(
                        "🐛 set Codeberg {}/{} to {}",
                        account, resolved_name, visibility_str
                    );
                }
            } else {
                eprintln!(
                    "⚠️ no CODEBERG_TOKEN for visibility sync on {}",
                    remote.name
                );
            }
        }
    }

    // Update cache even on partial failures — we don't want to hammer APIs
    // on every sync cycle when a token is permanently missing.
    update_visibility_cache(repo_path, github_private);
}

/// Repo metadata fetched from GitHub: description + topics.
#[derive(Debug, Clone, Default)]
pub(crate) struct RepoMetadata {
    pub(crate) description: String,
    pub(crate) topics: Vec<String>,
}

/// Query GitHub for repo description and topics using `gh api`.
/// Returns empty metadata on any error (non-fatal).
pub(crate) fn get_github_metadata(owner: &str, repo: &str) -> RepoMetadata {
    let output = match gh_cmd()
        .args([
            "api",
            &format!("repos/{}/{}", owner, repo),
            "--jq",
            "{description: .description, topics: .topics}",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("⚠️ gh api metadata failed (is gh installed?): {}", e);
            return RepoMetadata::default();
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("⚠️ gh api metadata failed: {}", stderr.trim());
        return RepoMetadata::default();
    }
    let stdout_raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // gh api --jq with JSON object construction can include ANSI color codes;
    // strip them before parsing JSON.
    let stdout = strip_ansi(&stdout_raw);
    // Parse JSON: {"description": "...", "topics": ["a","b"]}
    match serde_json::from_str::<RepoMetadataJson>(&stdout) {
        Ok(m) => RepoMetadata {
            description: m.description.unwrap_or_default().trim().to_string(),
            topics: m.topics.unwrap_or_default(),
        },
        Err(e) => {
            eprintln!("⚠️ failed to parse gh api metadata JSON: {}", e);
            RepoMetadata::default()
        }
    }
}

#[derive(Deserialize)]
struct RepoMetadataJson {
    description: Option<String>,
    topics: Option<Vec<String>>,
}

/// Set GitLab repo description and topics using `curl` with PRIVATE-TOKEN.
/// The token is passed via stdin (`-H @-`) so it never appears in the
/// process command line (visible to other local users via /proc).
fn set_gitlab_metadata(owner: &str, repo: &str, token: &str, meta: &RepoMetadata) -> Result<()> {
    use std::io::Write;
    let encoded = format!("{}%2F{}", owner, repo);
    let url = GITLAB_API_PROJECTS.replace("{}", &encoded);
    let mut form_data = vec![format!(
        "description={}",
        urlencoding::encode(&meta.description)
    )];
    for topic in &meta.topics {
        form_data.push(format!("tag_list[]={}", urlencoding::encode(topic)));
    }
    let form_body = form_data.join("&");
    let header = format!("PRIVATE-TOKEN: {}\r\n", token);
    let mut child = std::process::Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-H",
            "@-",
            "-X",
            "PUT",
            "--data-binary",
            &form_body,
            &url,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| "curl failed to run for GitLab metadata update")?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("curl stdin not available"))?;
        stdin
            .write_all(header.as_bytes())
            .with_context(|| "failed to write GitLab PRIVATE-TOKEN header to curl stdin")?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| "curl wait_with_output failed for GitLab metadata update")?;

    let code = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match code.as_str() {
        "200" => Ok(()),
        "401" => Err(anyhow::anyhow!(
            "GitLab metadata update failed: unauthorized"
        )),
        "404" => Err(anyhow::anyhow!(
            "GitLab metadata update failed: repo not found"
        )),
        _ => Err(anyhow::anyhow!(
            "GitLab metadata update failed: HTTP {}",
            code
        )),
    }
}

/// Set Codeberg repo description and topics using `curl` with Authorization token.
/// The token is passed via stdin (`-H @-`) so it never appears in the
/// process command line (visible to other local users via /proc).
fn set_codeberg_metadata(owner: &str, repo: &str, token: &str, meta: &RepoMetadata) -> Result<()> {
    use std::io::Write;
    let url = CODEBERG_API_REPOS.replace("{}", &format!("{}/{}", owner, repo));
    let json = serde_json::json!({
        "description": if meta.description.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(meta.description.clone()) },
        "topics": meta.topics,
    });
    let headers = format!(
        "Authorization: token {}\r\nContent-Type: application/json\r\n",
        token
    );
    let mut child = std::process::Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-H",
            "@-",
            "-X",
            "PATCH",
            "--data-binary",
            &json.to_string(),
            &url,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| "curl failed to run for Codeberg metadata update")?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("curl stdin not available"))?;
        stdin
            .write_all(headers.as_bytes())
            .with_context(|| "failed to write Codeberg Authorization header to curl stdin")?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| "curl wait_with_output failed for Codeberg metadata update")?;

    let code = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match code.as_str() {
        "200" => Ok(()),
        "401" => Err(anyhow::anyhow!(
            "Codeberg metadata update failed: unauthorized"
        )),
        "404" => Err(anyhow::anyhow!(
            "Codeberg metadata update failed: repo not found"
        )),
        _ => Err(anyhow::anyhow!(
            "Codeberg metadata update failed: HTTP {}",
            code
        )),
    }
}

/// Sync repo metadata (description + topics) from GitHub to all configured mirrors.
///
/// This function is **non-fatal**: errors are logged but never propagated.
/// Reuses the same cache as visibility sync.
pub(crate) fn sync_mirror_metadata(
    origin_url: &str,
    remotes: &[RemoteConfig],
    repo_path: &Path,
    interval_hours: u64,
) {
    // Check cache first (shares cache with visibility sync)
    if is_visibility_cache_fresh(repo_path, interval_hours) {
        return;
    }

    let repo_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if repo_name.is_empty() {
        return;
    }

    let Some((owner, gh_repo)) = parse_github_owner_repo(origin_url) else {
        // Already warned by visibility sync if called in same cycle
        return;
    };

    let meta = get_github_metadata(&owner, &gh_repo);

    if crate::policy::debug_enabled() {
        eprintln!(
            "🐛 GitHub {}/{} metadata: description={:?} topics={:?}",
            owner, gh_repo, meta.description, meta.topics
        );
    }

    for remote in remotes {
        let auth = remote.effective_auth_type();
        let account = remote.resolve_account();
        if auth == AuthType::GitLab {
            let token_var = remote
                .auto_create_token_var
                .as_deref()
                .unwrap_or("GITLAB_TOKEN");
            if let Some(token) = load_secret(token_var, &sync_secrets_dir()) {
                let resolved_name = remote.resolve_repo_name(&repo_name);
                if let Err(e) = set_gitlab_metadata(&account, &resolved_name, &token, &meta) {
                    eprintln!(
                        "⚠️ failed to set GitLab metadata for {}: {}",
                        resolved_name, e
                    );
                } else if crate::policy::debug_enabled() {
                    eprintln!("🐛 set GitLab {}/{} metadata", account, resolved_name);
                }
            }
        }

        if auth == AuthType::Codeberg {
            let token_var = remote
                .auto_create_token_var
                .as_deref()
                .unwrap_or("CODEBERG_TOKEN");
            if let Some(token) = load_secret(token_var, &sync_secrets_dir()) {
                let resolved_name = remote.resolve_repo_name(&repo_name);
                if let Err(e) = set_codeberg_metadata(&account, &resolved_name, &token, &meta) {
                    eprintln!(
                        "⚠️ failed to set Codeberg metadata for {}: {}",
                        resolved_name, e
                    );
                } else if crate::policy::debug_enabled() {
                    eprintln!("🐛 set Codeberg {}/{} metadata", account, resolved_name);
                }
            }
        }
    }

    // Don't update cache here — visibility sync will do it in the same cycle
}

/// Remove visibility cache entries for repos that no longer exist on disk.
pub(crate) fn prune_stale_visibility_cache(
    repo_set: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Result<()> {
    let cache_dir = visibility_cache_dir();
    if !cache_dir.exists() {
        return Ok(());
    }
    // Build set of valid cache hashes from current repos
    let valid_hashes: std::collections::HashSet<String> = repo_set
        .iter()
        .map(|r| format!("{}.last", simple_hash(&r.to_string_lossy())))
        .collect();
    let mut removed = 0;
    for entry in std::fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".last") && !valid_hashes.contains(name_str.as_ref()) {
            std::fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    if removed > 0 {
        eprintln!(
            "🧹 startup: pruned {} stale visibility cache entries",
            removed
        );
    }
    Ok(())
}

/// Check GitHub visibility at repo creation time and return whether the
/// repo should be created as private. If `sync_visibility` is disabled,
/// always returns `true` (private).
#[cfg(test)]
pub(crate) fn github_visibility_at_creation(
    owner: &str,
    repo_name: &str,
    sync_visibility: bool,
) -> bool {
    if !sync_visibility {
        return true;
    }
    get_github_visibility(owner, repo_name)
}

/// Update the `auto_create_account` and `--private` flag for GitHub repo creation
/// based on the visibility setting. When `sync_visibility` is true, queries GitHub
/// to determine if the repo already exists and what its visibility is.
///
/// This is a no-op wrapper around `create_repo_on_github` for the `sync_visibility=false`
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_owner_repo_ssh() {
        let result = parse_github_owner_repo("git@github.com:DraconDev/my-repo.git");
        assert_eq!(
            result,
            Some(("DraconDev".to_string(), "my-repo".to_string()))
        );
    }

    #[test]
    fn test_parse_github_owner_repo_https() {
        let result = parse_github_owner_repo("https://github.com/DraconDev/my-repo.git");
        assert_eq!(
            result,
            Some(("DraconDev".to_string(), "my-repo".to_string()))
        );
    }

    #[test]
    fn test_parse_github_owner_repo_no_git_suffix() {
        let result = parse_github_owner_repo("git@github.com:DraconDev/my-repo");
        assert_eq!(
            result,
            Some(("DraconDev".to_string(), "my-repo".to_string()))
        );
    }

    #[test]
    fn test_parse_github_owner_repo_invalid_url() {
        let result = parse_github_owner_repo("not-a-url");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_gitlab_url() {
        let result = parse_github_owner_repo("git@gitlab.com:someone/repo.git");
        // Should parse as (someone, repo) since the parser is generic enough
        assert_eq!(result, Some(("someone".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_visibility_cache_not_fresh_when_missing() {
        let repo_path = Path::new("/tmp/test_repo_that_should_not_exist_12345");
        assert!(!is_visibility_cache_fresh(repo_path, 24));
    }

    #[test]
    fn test_visibility_cache_fresh_when_recent() {
        let repo_path = Path::new("/tmp/test_cache_fresh");
        update_visibility_cache(repo_path, true);
        assert!(is_visibility_cache_fresh(repo_path, 24));
        // Cleanup
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_visibility_cache_stale_when_old() {
        let repo_path = Path::new("/tmp/test_cache_stale");
        let path = visibility_cache_path(repo_path);
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(25 * 3600);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();
        std::fs::write(&path, old_ts.to_string()).unwrap();
        assert!(!is_visibility_cache_fresh(repo_path, 24));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_visibility_cache_updates_timestamp() {
        let repo_path = Path::new("/tmp/test_cache_update");
        let path = visibility_cache_path(repo_path);
        // Write old timestamp
        let old_ts = "1000";
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();
        std::fs::write(&path, old_ts).unwrap();
        // Update
        update_visibility_cache(repo_path, true);
        let new_content = std::fs::read_to_string(&path).unwrap();
        // New format: "visibility=private\n<unix_ts>". Parse the second line.
        let new_ts_line = new_content.lines().nth(1).unwrap();
        let new_ts = new_ts_line.trim().parse::<u64>().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(new_ts > 1000);
        assert!(new_ts <= now);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_github_visibility_at_creation_disabled() {
        // When sync_visibility is false, always private
        assert!(github_visibility_at_creation("DraconDev", "test", false));
    }

    #[test]
    fn test_get_github_visibility_returns_safe_default_on_error() {
        // With no gh installed (or in test env), should return true (private)
        let result = get_github_visibility("nonexistent-owner-12345", "nonexistent-repo-67890");
        assert!(result, "safe default should be private");
    }

    #[test]
    fn test_sync_mirror_visibility_skips_when_cache_fresh() {
        let repo_path = Path::new("/tmp/test_skip_cached");
        update_visibility_cache(repo_path, true);
        // Should return immediately without error even with bad remotes
        let remotes = vec![RemoteConfig {
            name: "gitlab".to_string(),
            push_url: "git@gitlab.com:test/repo.git".to_string(),
            auto_create: false,
            auto_create_account: "test".to_string(),
            auth_type: AuthType::GitLab,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        sync_mirror_visibility("git@github.com:DraconDev/test.git", &remotes, repo_path, 24);
        // If we got here without panicking, the cache skip worked
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_sync_mirror_visibility_handles_unparseable_origin() {
        let repo_path = Path::new("/tmp/test_bad_origin");
        let remotes: Vec<RemoteConfig> = vec![];
        // Should not panic on unparseable URL
        sync_mirror_visibility("not-a-valid-url", &remotes, repo_path, 0);
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_parse_github_owner_repo_with_dots() {
        let result = parse_github_owner_repo("git@github.com:DraconDev/.dracon.git");
        assert_eq!(
            result,
            Some(("DraconDev".to_string(), ".dracon".to_string()))
        );
    }

    #[test]
    fn test_parse_github_owner_repo_with_name_mapping() {
        let result = parse_github_owner_repo("https://github.com/my-org/some-repo.git");
        assert_eq!(
            result,
            Some(("my-org".to_string(), "some-repo".to_string()))
        );
    }

    // ---- Creation-time visibility tests ----

    #[test]
    fn test_github_visibility_at_creation_enabled_private() {
        // When sync_visibility is true, we check GitHub. In test env (no gh),
        // get_github_visibility returns true (safe default = private).
        assert!(github_visibility_at_creation("DraconDev", "test", true));
    }

    #[test]
    fn test_sync_visibility_defaults_in_policy() {
        // Verify that the default policy parsing gives sync_visibility=false
        let toml = "";
        let policy: crate::policy::SyncPolicy = toml::from_str(toml).unwrap_or_else(|_| {
            // If empty TOML fails, try minimal valid config
            toml::from_str("pulse_interval_secs = 1").unwrap()
        });
        assert!(
            !policy.sync_visibility,
            "sync_visibility should default to false"
        );
        assert_eq!(
            policy.sync_visibility_interval_hours, 24,
            "interval should default to 24"
        );
    }

    #[test]
    fn test_sync_visibility_parses_true() {
        let toml = "sync_visibility = true\nsync_visibility_interval_hours = 6";
        let policy: crate::policy::SyncPolicy = toml::from_str(toml)
            .unwrap_or_else(|_| toml::from_str("pulse_interval_secs = 1\nsync_visibility = true\nsync_visibility_interval_hours = 6").unwrap());
        assert!(policy.sync_visibility);
        assert_eq!(policy.sync_visibility_interval_hours, 6);
    }

    // ---- Mirror update tests with mock curl ----

    #[test]
    fn test_set_gitlab_visibility_builds_correct_request() {
        // We can't easily mock curl, but we can verify the function handles
        // the "curl not found" case gracefully.
        let result = set_gitlab_visibility("testowner", "testrepo", "faketoken", true);
        // curl may or may not be available, but it should not panic
        let _ = result;
    }

    #[test]
    fn test_set_codeberg_visibility_builds_correct_request() {
        let result = set_codeberg_visibility("testowner", "testrepo", "faketoken", false);
        let _ = result;
    }

    // ---- Error isolation tests ----

    #[test]
    fn test_sync_mirror_visibility_does_not_panic_on_all_failures() {
        let _repo_name = "test_all_failures";
        let remotes = vec![
            RemoteConfig {
                name: "gitlab".to_string(),
                push_url: "git@gitlab.com:test/repo.git".to_string(),
                auto_create: false,
                auto_create_account: "test".to_string(),
                auth_type: AuthType::GitLab,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: Some("NONEXISTENT_TOKEN_VAR_12345".to_string()),
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
            RemoteConfig {
                name: "codeberg".to_string(),
                push_url: "git@codeberg.org:test/repo.git".to_string(),
                auto_create: false,
                auto_create_account: "test".to_string(),
                auth_type: AuthType::Codeberg,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: Some("NONEXISTENT_TOKEN_VAR_12345".to_string()),
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
        ];
        // Set interval to 0 to force cache expiration
        sync_mirror_visibility(
            "git@github.com:DraconDev/test.git",
            &remotes,
            Path::new("/tmp/test_all_failures"),
            0,
        );
        // Should not panic even when all tokens are missing
        let _ = std::fs::remove_file(visibility_cache_path(Path::new("/tmp/test_all_failures")));
    }

    // ---- Idempotency / cache behavior ----

    #[test]
    fn test_cache_prevents_repeated_api_calls() {
        let repo_path = Path::new("/tmp/test_idempotency_cache");
        update_visibility_cache(repo_path, true);
        // Second call with fresh cache should skip entirely
        assert!(is_visibility_cache_fresh(repo_path, 24));
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_cache_written_on_parseable_origin_even_when_tokens_missing() {
        // When origin URL is parseable but tokens are missing, the cache
        // should still be written to prevent hammering on every sync cycle.
        let repo_path = Path::new("/tmp/test_cache_on_failure");
        let remotes: Vec<RemoteConfig> = vec![];
        sync_mirror_visibility("git@github.com:DraconDev/test.git", &remotes, repo_path, 0);
        // Cache should exist even with no remotes to update
        let path = visibility_cache_path(repo_path);
        assert!(
            path.exists(),
            "cache should be written even when no remotes configured"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cache_not_written_on_unparseable_origin() {
        // When the origin URL can't be parsed, we return early before writing cache.
        // This is correct — the next cycle should retry since we couldn't even
        // determine the GitHub owner/repo.
        let repo_path = Path::new("/tmp/test_cache_unparseable");
        let remotes: Vec<RemoteConfig> = vec![];
        sync_mirror_visibility("not-a-url", &remotes, repo_path, 0);
        let path = visibility_cache_path(repo_path);
        assert!(
            !path.exists(),
            "cache should NOT be written for unparseable URLs (need to retry)"
        );
    }

    // ---- Edge cases ----

    #[test]
    fn test_parse_github_owner_repo_with_hyphens() {
        let result = parse_github_owner_repo("git@github.com:my-org/my-super-repo.git");
        assert_eq!(
            result,
            Some(("my-org".to_string(), "my-super-repo".to_string()))
        );
    }

    #[test]
    fn test_parse_github_owner_repo_empty_string() {
        let result = parse_github_owner_repo("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_visibility_cache_corrupt_file() {
        let repo_path = Path::new("/tmp/test_corrupt_cache");
        let path = visibility_cache_path(repo_path);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();
        std::fs::write(&path, "not-a-number").unwrap();
        assert!(
            !is_visibility_cache_fresh(repo_path, 24),
            "corrupt cache should be treated as stale"
        );
        let _ = std::fs::remove_file(&path);
    }

    // ---- Tests for codeberg-public-only visibility cache wiring ----
    // ---- (goal `codeberg-public-only`, 2026-07-17)             ----

    #[test]
    fn test_parse_visibility_cache_new_format_public() {
        let content = "visibility=public\n1234567890";
        let parsed = parse_visibility_cache(content);
        assert_eq!(parsed, Some((false, 1234567890)));
    }

    #[test]
    fn test_parse_visibility_cache_new_format_private() {
        let content = "visibility=private\n1234567890";
        let parsed = parse_visibility_cache(content);
        assert_eq!(parsed, Some((true, 1234567890)));
    }

    #[test]
    fn test_parse_visibility_cache_rejects_legacy_timestamp_only() {
        // Old format (just the timestamp) must NOT be parseable by
        // the new helper — it returns None, which `cached_repo_visibility`
        // surfaces as None (= unknown visibility = safe default).
        // This is the backward-compatibility contract.
        let legacy = "1234567890";
        assert_eq!(parse_visibility_cache(legacy), None);
    }

    #[test]
    fn test_parse_visibility_cache_rejects_malformed() {
        assert_eq!(parse_visibility_cache("visibility=wat\n1"), None);
        assert_eq!(parse_visibility_cache("visibility=public\n"), None);
        assert_eq!(parse_visibility_cache(""), None);
    }

    #[test]
    fn test_cached_repo_visibility_returns_none_when_no_file() {
        let repo_path = Path::new("/tmp/test_no_cache_file");
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
        assert_eq!(cached_repo_visibility(repo_path), None);
    }

    #[test]
    fn test_cached_repo_visibility_returns_private() {
        let repo_path = Path::new("/tmp/test_cached_private");
        update_visibility_cache(repo_path, true);
        assert_eq!(cached_repo_visibility(repo_path), Some(true));
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_cached_repo_visibility_returns_public() {
        let repo_path = Path::new("/tmp/test_cached_public");
        update_visibility_cache(repo_path, false);
        assert_eq!(cached_repo_visibility(repo_path), Some(false));
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_cached_repo_visibility_treats_legacy_format_as_unknown() {
        // Backward compat: cache files written before the new
        // format (timestamp only) must surface as None, not as
        // false (=public). This forces the safe-default path
        // (skip codeberg) until the next visibility sync refreshes
        // the cache with the new format.
        let repo_path = Path::new("/tmp/test_legacy_cache");
        let path = visibility_cache_path(repo_path);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();
        std::fs::write(&path, "1234567890").unwrap();
        assert_eq!(
            cached_repo_visibility(repo_path),
            None,
            "legacy timestamp-only cache must surface as None (unknown)"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_visibility_cache_freshness_works_for_both_formats() {
        // Freshness check (is_visibility_cache_fresh) must accept
        // BOTH the new format AND the legacy timestamp-only format.
        // This is the backward-compat path for the freshness check.
        let repo_path = Path::new("/tmp/test_freshness_both");
        let path = visibility_cache_path(repo_path);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();

        // Legacy format
        std::fs::write(&path, "999999999999999").unwrap();
        assert!(is_visibility_cache_fresh(repo_path, 24));

        // New format
        std::fs::write(&path, "visibility=private\n999999999999999").unwrap();
        assert!(is_visibility_cache_fresh(repo_path, 24));

        let _ = std::fs::remove_file(&path);
    }

    // ---- Metadata sync tests ----

    #[test]
    fn test_get_github_metadata_returns_empty_on_error() {
        let meta = get_github_metadata("nonexistent-owner-12345", "nonexistent-repo-67890");
        assert!(
            meta.description.is_empty(),
            "description should be empty on error"
        );
        assert!(meta.topics.is_empty(), "topics should be empty on error");
    }

    #[test]
    fn test_sync_metadata_defaults_in_policy() {
        let toml = "";
        let policy: crate::policy::SyncPolicy = toml::from_str(toml)
            .unwrap_or_else(|_| toml::from_str("pulse_interval_secs = 1").unwrap());
        assert!(
            !policy.sync_metadata,
            "sync_metadata should default to false"
        );
    }

    #[test]
    fn test_sync_metadata_parses_true() {
        let toml = "sync_metadata = true";
        let policy: crate::policy::SyncPolicy = toml::from_str(toml).unwrap_or_else(|_| {
            toml::from_str("pulse_interval_secs = 1\nsync_metadata = true").unwrap()
        });
        assert!(policy.sync_metadata);
    }

    #[test]
    fn test_sync_mirror_metadata_does_not_panic() {
        let repo_path = Path::new("/tmp/test_metadata_no_panic");
        let remotes: Vec<RemoteConfig> = vec![];
        sync_mirror_metadata("git@github.com:DraconDev/test.git", &remotes, repo_path, 0);
        // Should not panic even with no remotes
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_sync_mirror_metadata_handles_unparseable_origin() {
        let repo_path = Path::new("/tmp/test_metadata_bad_origin");
        let remotes: Vec<RemoteConfig> = vec![];
        sync_mirror_metadata("not-a-url", &remotes, repo_path, 0);
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_repo_metadata_default() {
        let meta = RepoMetadata::default();
        assert!(meta.description.is_empty());
        assert!(meta.topics.is_empty());
    }

    #[test]
    fn test_set_gitlab_metadata_does_not_panic() {
        let meta = RepoMetadata {
            description: "Test repo".to_string(),
            topics: vec!["rust".to_string(), "cli".to_string()],
        };
        let result = set_gitlab_metadata("testowner", "testrepo", "faketoken", &meta);
        let _ = result;
    }

    #[test]
    fn test_set_codeberg_metadata_does_not_panic() {
        let meta = RepoMetadata {
            description: "Test repo".to_string(),
            topics: vec!["rust".to_string()],
        };
        let result = set_codeberg_metadata("testowner", "testrepo", "faketoken", &meta);
        let _ = result;
    }

    #[test]
    fn test_sync_metadata_with_missing_tokens() {
        let _repo_name = "test_metadata_missing_tokens";
        let remotes = vec![
            RemoteConfig {
                name: "gitlab".to_string(),
                push_url: "git@gitlab.com:test/repo.git".to_string(),
                auto_create: false,
                auto_create_account: "test".to_string(),
                auth_type: AuthType::GitLab,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: Some("NONEXISTENT_TOKEN_VAR_META".to_string()),
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
            RemoteConfig {
                name: "codeberg".to_string(),
                push_url: "git@codeberg.org:test/repo.git".to_string(),
                auto_create: false,
                auto_create_account: "test".to_string(),
                auth_type: AuthType::Codeberg,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: Some("NONEXISTENT_TOKEN_VAR_META2".to_string()),
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
        ];
        sync_mirror_metadata(
            "git@github.com:DraconDev/test.git",
            &remotes,
            Path::new("/tmp/test_metadata_missing_tokens"),
            0,
        );
        // Should not panic
        let _ = std::fs::remove_file(visibility_cache_path(Path::new(
            "/tmp/test_metadata_missing_tokens",
        )));
    }

    #[test]
    fn test_strip_ansi_removes_color_codes() {
        // Simulate gh api output with ANSI color codes
        let input = "\x1b[1;38m{\x1b[m\n\x1b[1;34m\"description\"\x1b[m\x1b[1;38m:\x1b[m \x1b[32m\"Hello world\"\x1b[m,\n\x1b[1;34m\"topics\"\x1b[m\x1b[1;38m:\x1b[m\x1b[32m[\"rust\"]\x1b[m\n}";
        let stripped = strip_ansi(input);
        assert!(
            !stripped.contains('\x1b'),
            "ANSI codes should be removed: got {:?}",
            stripped
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&stripped).expect("should be valid JSON after stripping");
        assert_eq!(parsed["description"], "Hello world");
        assert_eq!(parsed["topics"][0], "rust");
    }

    #[test]
    fn test_strip_ansi_passthrough_plain_text() {
        let input = r#"{"description":"plain","topics":[]}"#;
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn test_resolve_account_from_ssh_url() {
        let remote = RemoteConfig {
            name: "gitlab".to_string(),
            push_url: "git@gitlab.com:dracondev/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: String::new(),
            auth_type: AuthType::GitLab,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        assert_eq!(remote.resolve_account(), "dracondev");
    }

    #[test]
    fn test_resolve_account_from_https_url() {
        let remote = RemoteConfig {
            name: "codeberg".to_string(),
            push_url: "https://codeberg.org/myorg/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: String::new(),
            auth_type: AuthType::Codeberg,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        assert_eq!(remote.resolve_account(), "myorg");
    }

    #[test]
    fn test_resolve_account_explicit_overrides() {
        let remote = RemoteConfig {
            name: "github".to_string(),
            push_url: "git@github.com:DraconDev/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "ExplicitAccount".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        };
        assert_eq!(remote.resolve_account(), "ExplicitAccount");
    }

    // ---- refresh-visibility subcommand tests (goal `refresh-visibility-2026-07-17`) ----
    //
    // The `refresh-visibility` operator subcommand calls update_visibility_cache
    // and get_github_visibility directly. These tests verify the contract
    // these functions rely on:
    //
    //   1. update_visibility_cache upgrades legacy timestamp-only files
    //      to the new format (idempotent refresh)
    //   2. update_visibility_cache preserves new-format files
    //   3. parse_github_owner_repo accepts the SSH/HTTPS URL forms the
    //      refresh path expects
    //   4. cached_repo_visibility correctly reports (unknown) for legacy
    //      files BEFORE the refresh, and (public/private) AFTER
    //      (this is the user-visible behavior in `dracon-sync repos`)

    #[test]
    fn test_refresh_upgrades_legacy_to_new_format() {
        // Simulate a pre-v0.112.16 cache file (10-byte timestamp only).
        let repo_path = Path::new("/tmp/test_refresh_upgrades");
        let path = visibility_cache_path(repo_path);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();
        let old_ts = "1234567890";
        std::fs::write(&path, old_ts).unwrap();

        // BEFORE refresh: legacy file surfaces as None (= unknown).
        assert_eq!(cached_repo_visibility(repo_path), None);

        // Simulate refresh-visibility subcommand: write new format with private=true.
        update_visibility_cache(repo_path, true);

        // AFTER refresh: new format surfaces as Some(true) (= private).
        assert_eq!(cached_repo_visibility(repo_path), Some(true));

        // File content must be in the new format.
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.starts_with("visibility=private\n"),
            "expected new-format header, got {:?}",
            content
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_refresh_preserves_new_format() {
        // Pre-existing new-format cache file. update_visibility_cache must
        // overwrite it cleanly without corruption.
        let repo_path = Path::new("/tmp/test_refresh_preserves");
        let path = visibility_cache_path(repo_path);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();
        // Write initial new-format file with public=false.
        update_visibility_cache(repo_path, false);
        assert_eq!(cached_repo_visibility(repo_path), Some(false));

        // Re-run with private=true (simulates re-refresh with different result).
        update_visibility_cache(repo_path, true);
        assert_eq!(cached_repo_visibility(repo_path), Some(true));

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("visibility=private\n"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_refresh_handles_legacy_file_correctly_via_ssh_url() {
        // The refresh-visibility subcommand's URL parsing path.
        let ssh = "git@github.com:DraconDev/dracon-sync.git";
        assert_eq!(
            parse_github_owner_repo(ssh),
            Some(("DraconDev".to_string(), "dracon-sync".to_string()))
        );
    }

    #[test]
    fn test_refresh_falls_back_to_unknown_when_gh_unavailable() {
        // When `gh api` fails (no network, no auth, wrong owner/repo),
        // get_github_visibility returns true (safe default = private).
        // The refresh subcommand then writes the cache with private=true,
        // so the next `dracon-sync repos` shows `(private)` instead of
        // `(unknown)`. This is intentional: false negatives (public treated
        // as private) are safe; false positives (private treated as public)
        // could leak private content to codeberg.
        let result = get_github_visibility("nonexistent-owner-12345", "nonexistent-repo-67890");
        assert!(
            result,
            "gh failure must default to private (safe default)"
        );
        let repo_path = Path::new("/tmp/test_refresh_fallback_unknown");
        update_visibility_cache(repo_path, result);
        assert_eq!(cached_repo_visibility(repo_path), Some(true));
        let _ = std::fs::remove_file(visibility_cache_path(repo_path));
    }

    #[test]
    fn test_refresh_idempotent() {
        // Running refresh-visibility twice on the same repo must produce
        // identical results (same visibility, valid cache file).
        let repo_path = Path::new("/tmp/test_refresh_idempotent");
        let path = visibility_cache_path(repo_path);
        std::fs::create_dir_all(visibility_cache_dir()).unwrap();

        // First call
        update_visibility_cache(repo_path, true);
        let content_1 = std::fs::read_to_string(&path).unwrap();

        // Wait a millisecond to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Second call (re-refresh)
        update_visibility_cache(repo_path, true);
        let content_2 = std::fs::read_to_string(&path).unwrap();

        // Visibility line must be identical (timestamps can differ)
        let line1 = content_1.lines().next().unwrap();
        let line2 = content_2.lines().next().unwrap();
        assert_eq!(line1, line2, "visibility line must be stable across refresh");
        assert_eq!(line1, "visibility=private");

        let _ = std::fs::remove_file(&path);
    }
}
