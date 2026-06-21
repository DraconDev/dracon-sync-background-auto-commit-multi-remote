//! Release pipeline: git tagging, GitHub releases, and package registry publishing.
//!
//! After a version bump in `sync_repo`, this module handles:
//! - Creating git tags (`v{version}`) for every bump
//! - Creating GitHub Releases for major bumps via `gh release create`
//! - Publishing to configured registries (crates.io, npm, PyPI)

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::bump::{extract_version_from_cargo, extract_version_from_json};
use crate::git::{
    gh_cmd, git_ssh_hardening, load_secret, run_git_with_timeout, run_git_with_timeout_env,
};
use crate::policy::{PublishRegistry, SyncPolicy};

/// Result of a release pipeline step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReleaseStep {
    /// Tag was created and pushed.
    TagCreated(String),
    /// GitHub Release was created.
    GitHubReleaseCreated(String),
    /// Package was published to a registry.
    Published { registry: String, version: String },
    /// Nix flake PR was created.
    NixFlakePRCreated(String),
    /// Step was skipped (already exists, disabled, etc.).
    Skipped(String),
    /// Step failed but did not block the pipeline.
    Failed { step: String, error: String },
}

/// Check if a git tag already exists in the repo.
pub(crate) async fn tag_exists(repo: &Path, tag: &str) -> Result<bool> {
    let repo = repo.to_path_buf();
    let tag_owned = tag.to_string();
    let result = tokio::task::spawn_blocking(move || {
        crate::git::git_cmd()
            .args(["tag", "--list", &tag_owned])
            .current_dir(&repo)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .context("failed to run git tag --list")
    })
    .await
    .context("spawn_blocking for tag_exists")??;
    Ok(String::from_utf8_lossy(&result.stdout).trim() == tag)
}

/// Create a git tag for the given version and push it.
pub(crate) async fn create_and_push_tag(repo: &Path, version: &str) -> Result<ReleaseStep> {
    let tag = format!("v{version}");

    if tag_exists(repo, &tag).await? {
        return Ok(ReleaseStep::Skipped(format!("tag {tag} already exists")));
    }

    // Create annotated tag
    run_git_with_timeout(
        repo,
        &["tag", "-a", &tag, "-m", &format!("Release {tag}")],
        30,
        "tag-create",
    )
    .await?;

    // Push tag
    match run_git_with_timeout_env(
        repo,
        &["push", "origin", &tag],
        120,
        "tag-push",
        &[
            ("GIT_SSH_COMMAND", &git_ssh_hardening()),
            ("GIT_TERMINAL_PROMPT", "0"),
        ],
    )
    .await
    {
        Ok(_) => {
            eprintln!("🏷️  Created and pushed tag {tag}");
            Ok(ReleaseStep::TagCreated(tag))
        }
        Err(e) => {
            // Tag was created locally but push failed — delete local tag to avoid inconsistency
            let _ = run_git_with_timeout(repo, &["tag", "-d", &tag], 10, "tag-delete").await;
            Ok(ReleaseStep::Failed {
                step: format!("push tag {tag}"),
                error: e.to_string(),
            })
        }
    }
}

/// Create a GitHub Release for a major version bump using `gh release create`.
pub(crate) async fn create_github_release(repo: &Path, tag: &str) -> Result<ReleaseStep> {
    let repo_name = extract_repo_name(repo)?;

    let repo_name_check = repo_name.clone();
    let tag_check = tag.to_string();
    let check = tokio::task::spawn_blocking(move || {
        gh_cmd()
            .args(["release", "view", &tag_check, "--repo", &repo_name_check])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    })
    .await;

    match check {
        Ok(Ok(status)) if status.success() => {
            return Ok(ReleaseStep::Skipped(format!(
                "GitHub release {tag} already exists"
            )));
        }
        _ => {}
    }

    let repo_name_create = repo_name.clone();
    let tag_create = tag.to_string();
    let result = tokio::task::spawn_blocking(move || {
        gh_cmd()
            .args([
                "release",
                "create",
                &tag_create,
                "--repo",
                &repo_name_create,
                "--title",
                &tag_create,
                "--notes",
                &format!("Release {tag_create}"),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            eprintln!("🚀 Created GitHub release {tag} for {repo_name}");
            Ok(ReleaseStep::GitHubReleaseCreated(tag.to_string()))
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ReleaseStep::Failed {
                step: format!("GitHub release {tag}"),
                error: stderr.trim().to_string(),
            })
        }
        _ => Ok(ReleaseStep::Failed {
            step: format!("GitHub release {tag}"),
            error: "gh release create failed".to_string(),
        }),
    }
}

/// Extract the repository name (owner/repo) from the git remote URL.
fn extract_repo_name(repo: &Path) -> Result<String> {
    let output = crate::git::git_cmd()
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .context("failed to get origin URL")?;

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Extract owner/repo from SSH or HTTPS URL
    // git@github.com:Owner/Repo.git -> Owner/Repo
    // ssh://git@github.com:22/Owner/Repo.git -> Owner/Repo
    // https://github.com/Owner/Repo.git -> Owner/Repo
    let repo_name = if url.starts_with("ssh://") {
        // ssh://git@host:port/Owner/Repo.git
        url.trim_start_matches("ssh://")
            .trim_end_matches(".git")
            .split('/')
            .skip(1) // skip the "git@host:port" part
            .collect::<Vec<_>>()
            .join("/")
    } else if url.starts_with("git@") {
        url.strip_prefix("git@")
            .and_then(|s| s.split_once(':'))
            .map(|(_, path)| path.trim_end_matches(".git").to_string())
            .unwrap_or_else(|| url.clone())
    } else if url.starts_with("https://") {
        // https://github.com/Owner/Repo.git -> Owner/Repo
        // Strip protocol and host to get owner/repo
        url.trim_start_matches("https://")
            .trim_end_matches(".git")
            .split('/')
            .skip(1) // skip the "github.com" part
            .collect::<Vec<_>>()
            .join("/")
    } else {
        url.clone()
    };

    Ok(repo_name)
}

/// Check if a version already exists on a package registry.
pub(crate) async fn version_exists_on_registry(
    registry: PublishRegistry,
    package_name: &str,
    version: &str,
) -> Result<bool> {
    match registry {
        PublishRegistry::CratesIo => {
            let url = format!("https://crates.io/api/v1/crates/{package_name}/{version}");
            let output = Command::new("curl")
                .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &url])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output();

            match output {
                Ok(out) => {
                    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    Ok(code == "200")
                }
                Err(_) => Ok(false), // Assume not found on network error
            }
        }
        PublishRegistry::Npm => {
            let url = format!("https://registry.npmjs.org/{package_name}/{version}");
            let output = Command::new("curl")
                .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &url])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output();

            match output {
                Ok(out) => {
                    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    Ok(code == "200")
                }
                Err(_) => Ok(false),
            }
        }
        PublishRegistry::Pypi => {
            let url = format!("https://pypi.org/pypi/{package_name}/{version}/json");
            let output = Command::new("curl")
                .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &url])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output();

            match output {
                Ok(out) => {
                    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    Ok(code == "200")
                }
                Err(_) => Ok(false),
            }
        }
    }
}

/// Extract the package name from the repo's manifest file.
pub(crate) fn extract_package_name(repo: &Path, registry: PublishRegistry) -> Result<String> {
    match registry {
        PublishRegistry::CratesIo => {
            let cargo_toml =
                std::fs::read_to_string(repo.join("Cargo.toml")).context("no Cargo.toml found")?;
            // Simple parse: look for `name = "..."` in [package] section
            for line in cargo_toml.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name") {
                    if let Some(name) = trimmed.split('=').nth(1) {
                        let name = name.trim().trim_matches('"').trim();
                        if !name.is_empty() && !name.starts_with("workspace") {
                            return Ok(name.to_string());
                        }
                    }
                }
            }
            bail!("could not find package name in Cargo.toml")
        }
        PublishRegistry::Npm => {
            let pkg_json = std::fs::read_to_string(repo.join("package.json"))
                .context("no package.json found")?;
            // Simple parse: look for "name" field
            if let Some(name_line) = pkg_json.lines().find(|l| l.trim().starts_with("\"name\"")) {
                if let Some(name) = name_line.split(':').nth(1) {
                    let name = name.trim().trim_end_matches(',').trim_matches('"').trim();
                    if !name.is_empty() {
                        return Ok(name.to_string());
                    }
                }
            }
            bail!("could not find package name in package.json")
        }
        PublishRegistry::Pypi => {
            // Try pyproject.toml first, then setup.py
            let pyproject = repo.join("pyproject.toml");
            if pyproject.exists() {
                let content = std::fs::read_to_string(&pyproject)?;
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("name") && trimmed.contains('=') {
                        if let Some(name) = trimmed.split('=').nth(1) {
                            let name = name.trim().trim_matches('"').trim();
                            if !name.is_empty() {
                                return Ok(name.to_string());
                            }
                        }
                    }
                }
            }
            bail!("could not find package name in pyproject.toml")
        }
    }
}

/// Publish a package to the configured registry.
pub(crate) async fn publish_to_registry(
    repo: &Path,
    registry: PublishRegistry,
    token_env: &str,
    timeout_secs: u64,
) -> Result<ReleaseStep> {
    let token = match load_secret(token_env) {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Ok(ReleaseStep::Skipped(format!(
                "no token found for {token_env}"
            )));
        }
    };

    match registry {
        PublishRegistry::CratesIo => publish_crates_io(repo, &token, timeout_secs).await,
        PublishRegistry::Npm => publish_npm(repo, &token, timeout_secs).await,
        PublishRegistry::Pypi => publish_pypi(repo, &token, timeout_secs).await,
    }
}

async fn publish_crates_io(repo: &Path, token: &str, _timeout_secs: u64) -> Result<ReleaseStep> {
    // Dry run first
    let dry_run = Command::new("cargo")
        .args(["publish", "--dry-run"])
        .env("CARGO_REGISTRY_TOKEN", token)
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match dry_run {
        Ok(output) if output.status.success() => {
            // Real publish
            let result = Command::new("cargo")
                .args(["publish"])
                .env("CARGO_REGISTRY_TOKEN", token)
                .current_dir(repo)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();

            match result {
                Ok(out) if out.status.success() => {
                    let version =
                        read_cargo_version(repo).unwrap_or_else(|_| "unknown".to_string());
                    eprintln!("📦 Published to crates.io: v{version}");
                    Ok(ReleaseStep::Published {
                        registry: "crates-io".to_string(),
                        version,
                    })
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    // Check if it's "already uploaded" — not an error
                    if stderr.contains("already uploaded") || stderr.contains("already exists") {
                        return Ok(ReleaseStep::Skipped(
                            "already published to crates.io".to_string(),
                        ));
                    }
                    Ok(ReleaseStep::Failed {
                        step: "cargo publish".to_string(),
                        error: stderr.trim().to_string(),
                    })
                }
                Err(e) => Ok(ReleaseStep::Failed {
                    step: "cargo publish".to_string(),
                    error: e.to_string(),
                }),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ReleaseStep::Failed {
                step: "cargo publish --dry-run".to_string(),
                error: stderr.trim().to_string(),
            })
        }
        Err(e) => Ok(ReleaseStep::Failed {
            step: "cargo publish --dry-run".to_string(),
            error: e.to_string(),
        }),
    }
}

async fn publish_npm(repo: &Path, token: &str, _timeout_secs: u64) -> Result<ReleaseStep> {
    // Dry run first
    let dry_run = Command::new("npm")
        .args(["publish", "--dry-run"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match dry_run {
        Ok(output) if output.status.success() => {
            // Real publish
            let result = Command::new("npm")
                .args(["publish"])
                .env("NPM_TOKEN", token)
                .current_dir(repo)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();

            match result {
                Ok(out) if out.status.success() => {
                    let version = read_npm_version(repo).unwrap_or_else(|_| "unknown".to_string());
                    eprintln!("📦 Published to npm: v{version}");
                    Ok(ReleaseStep::Published {
                        registry: "npm".to_string(),
                        version,
                    })
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if stderr.contains("already published") || stderr.contains("409") {
                        return Ok(ReleaseStep::Skipped("already published to npm".to_string()));
                    }
                    Ok(ReleaseStep::Failed {
                        step: "npm publish".to_string(),
                        error: stderr.trim().to_string(),
                    })
                }
                Err(e) => Ok(ReleaseStep::Failed {
                    step: "npm publish".to_string(),
                    error: e.to_string(),
                }),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ReleaseStep::Failed {
                step: "npm publish --dry-run".to_string(),
                error: stderr.trim().to_string(),
            })
        }
        Err(e) => Ok(ReleaseStep::Failed {
            step: "npm publish --dry-run".to_string(),
            error: e.to_string(),
        }),
    }
}

async fn publish_pypi(repo: &Path, token: &str, _timeout_secs: u64) -> Result<ReleaseStep> {
    // Build sdist and wheel first
    let build = Command::new("python")
        .args(["-m", "build"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match build {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ReleaseStep::Failed {
                step: "python -m build".to_string(),
                error: stderr.trim().to_string(),
            });
        }
        Err(e) => {
            return Ok(ReleaseStep::Failed {
                step: "python -m build".to_string(),
                error: e.to_string(),
            });
        }
    }

    // Upload with twine
    let _dist_dir = repo.join("dist");
    let result = Command::new("twine")
        .args(["upload", "dist/*"])
        .env("TWINE_USERNAME", "__token__")
        .env("TWINE_PASSWORD", token)
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(out) if out.status.success() => {
            let version = read_pypi_version(repo).unwrap_or_else(|_| "unknown".to_string());
            eprintln!("📦 Published to PyPI: v{version}");
            Ok(ReleaseStep::Published {
                registry: "pypi".to_string(),
                version,
            })
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("already exists") || stderr.contains("409") {
                return Ok(ReleaseStep::Skipped(
                    "already published to PyPI".to_string(),
                ));
            }
            Ok(ReleaseStep::Failed {
                step: "twine upload".to_string(),
                error: stderr.trim().to_string(),
            })
        }
        Err(e) => Ok(ReleaseStep::Failed {
            step: "twine upload".to_string(),
            error: e.to_string(),
        }),
    }
}

/// Read the current version from Cargo.toml.
fn read_cargo_version(repo: &Path) -> Result<String> {
    let content = std::fs::read_to_string(repo.join("Cargo.toml"))?;
    extract_version_from_cargo(&content)
        .ok_or_else(|| anyhow::anyhow!("could not find version in Cargo.toml"))
}

/// Read the current version from package.json.
fn read_npm_version(repo: &Path) -> Result<String> {
    let content = std::fs::read_to_string(repo.join("package.json"))?;
    extract_version_from_json(&content, "version")
        .ok_or_else(|| anyhow::anyhow!("could not find version in package.json"))
}

/// Read the current version from pyproject.toml.
fn read_pypi_version(repo: &Path) -> Result<String> {
    let pyproject = repo.join("pyproject.toml");
    if pyproject.exists() {
        let content = std::fs::read_to_string(&pyproject)?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("version") && trimmed.contains('=') {
                if let Some(ver) = trimmed.split('=').nth(1) {
                    let ver = ver.trim().trim_matches('"').trim();
                    if !ver.is_empty() {
                        return Ok(ver.to_string());
                    }
                }
            }
        }
    }
    bail!("could not find version in pyproject.toml")
}

/// Run the full release pipeline after a version bump.
///
/// Steps (each gated independently):
/// 1. Create and push git tag — gated on `repo_auto_tag`
/// 2. Create GitHub Release for major bumps — gated on `repo_auto_release`
/// 3. Publish to configured registries — gated on `repo_publish_targets`
/// 4. Create Nix flake PR — gated on `nix_auto_update` and presence of `flake.nix`
pub(crate) async fn run_release_pipeline(
    repo: &Path,
    _old_version: &str,
    new_version: &str,
    bump_level: &str, // "major", "minor", "patch"
    policy: &SyncPolicy,
    repo_auto_tag: bool,
    repo_auto_release: bool,
    repo_publish_targets: &[String],
    repo_nix_auto_update: bool,
) -> Vec<ReleaseStep> {
    let mut steps = Vec::new();

    // Step 1: Create and push tag (gated on auto_tag)
    if repo_auto_tag {
        match create_and_push_tag(repo, new_version).await {
            Ok(step) => steps.push(step),
            Err(e) => steps.push(ReleaseStep::Failed {
                step: "create tag".to_string(),
                error: e.to_string(),
            }),
        }
    }

    // Step 2: GitHub Release for major bumps (gated on auto_release)
    if repo_auto_release && bump_level == "major" {
        let tag = format!("v{new_version}");
        match create_github_release(repo, &tag).await {
            Ok(step) => steps.push(step),
            Err(e) => steps.push(ReleaseStep::Failed {
                step: "GitHub release".to_string(),
                error: e.to_string(),
            }),
        }
    }

    // Step 3: Publish to configured registries (gated on auto_publish + per-repo targets)
    if policy.auto_publish {
        for target in &policy.publish_targets {
            if !repo_publish_targets.contains(&target.name) {
                continue;
            }

            // Check if version already exists on registry
            match extract_package_name(repo, target.registry) {
                Ok(pkg_name) => {
                    match version_exists_on_registry(target.registry, &pkg_name, new_version).await
                    {
                        Ok(true) => {
                            steps.push(ReleaseStep::Skipped(format!(
                                "v{new_version} already on {}",
                                target.registry.as_str()
                            )));
                        }
                        Ok(false) => {
                            match publish_to_registry(
                                repo,
                                target.registry,
                                &target.token_secret,
                                target.publish_timeout_secs,
                            )
                            .await
                            {
                                Ok(step) => steps.push(step),
                                Err(e) => steps.push(ReleaseStep::Failed {
                                    step: format!("publish to {}", target.registry.as_str()),
                                    error: e.to_string(),
                                }),
                            }
                        }
                        Err(e) => steps.push(ReleaseStep::Failed {
                            step: format!(
                                "check {} registry for {pkg_name}",
                                target.registry.as_str()
                            ),
                            error: e.to_string(),
                        }),
                    }
                }
                Err(e) => steps.push(ReleaseStep::Skipped(format!(
                    "no package name for {}: {e}",
                    target.registry.as_str()
                ))),
            }
        }
    }

    // Step 4: Nix flake PR (gated on nix_auto_update and presence of flake.nix)
    if repo_nix_auto_update && crate::nix::has_flake_nix(repo) {
        match crate::nix::update_flake_version(repo, new_version) {
            Ok(true) => match crate::nix::create_flake_pr(repo, new_version).await {
                Ok(step) => steps.push(step),
                Err(e) => steps.push(ReleaseStep::Failed {
                    step: "nix flake pr".to_string(),
                    error: e.to_string(),
                }),
            },
            Ok(false) => {
                steps.push(ReleaseStep::Skipped(
                    "flake.nix version already up to date".to_string(),
                ));
            }
            Err(e) => steps.push(ReleaseStep::Failed {
                step: "update flake.nix version".to_string(),
                error: e.to_string(),
            }),
        }
    }

    steps
}

/// Read the current version from the repo based on detected project type.
/// Returns (version, project_type) where project_type is "rust", "node", or "python".
pub(crate) fn detect_project_version(repo: &Path) -> Option<(String, &'static str)> {
    if repo.join("Cargo.toml").exists() {
        read_cargo_version(repo).ok().map(|v| (v, "rust"))
    } else if repo.join("package.json").exists() {
        read_npm_version(repo).ok().map(|v| (v, "node"))
    } else if repo.join("pyproject.toml").exists() {
        read_pypi_version(repo).ok().map(|v| (v, "python"))
    } else if repo.join("pubspec.yaml").exists() {
        // Flutter/Dart: read version from pubspec.yaml
        fs::read_to_string(repo.join("pubspec.yaml"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.trim().starts_with("version:"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|v| v.trim().to_string())
            })
            .map(|v| (v, "dart"))
    } else if repo.join("VERSION").exists() || repo.join("version.txt").exists() {
        // Plain text version files: single line with semver
        let path = if repo.join("VERSION").exists() {
            repo.join("VERSION")
        } else {
            repo.join("version.txt")
        };
        fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(|v| (v, "plain"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::EnvRestorer;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_github_release_uses_configured_pat_without_prompt() {
        let repo_dir = TempDir::new().unwrap();
        let repo = repo_dir.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/TestOwner/test-repo.git",
            ])
            .current_dir(&repo)
            .status()
            .unwrap();

        let tmp_home = TempDir::new().unwrap();
        let tmp_bin = TempDir::new().unwrap();
        let gh_mock = tmp_bin.path().join("gh");
        std::fs::write(
            &gh_mock,
            "#!/bin/sh
if [ -z \"$GH_TOKEN\" ]; then
  echo 'missing GH_TOKEN' >&2
  exit 20
fi
if [ \"$GH_PROMPT_DISABLED\" != \"1\" ]; then
  echo 'prompt not disabled' >&2
  exit 21
fi
if [ \"$1\" = \"release\" ] && [ \"$2\" = \"view\" ]; then
  exit 1
fi
if [ \"$1\" = \"release\" ] && [ \"$2\" = \"create\" ]; then
  echo 'created'
  exit 0
fi
echo \"unexpected args: $*\" >&2
exit 22
",
        )
        .unwrap();
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755)).unwrap();

        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        std::fs::write(
            secrets_dir.join("github.env"),
            "GH_TOKEN=ghp_test_token_for_release\n",
        )
        .unwrap();

        let _home_guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("GH_TOKEN");
        {
            let _lock = crate::git::acquire_path_lock();
        }
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!("{}:{}", tmp_bin.path().to_string_lossy(), orig_path),
        );

        let result = create_github_release(&repo, "v1.0.0").await;
        assert!(
            matches!(
                result,
                Ok(ReleaseStep::GitHubReleaseCreated(ref tag)) if tag == "v1.0.0"
            ),
            "expected GitHub release creation, got: {:?}",
            result
        );
    }

    #[test]
    fn test_extract_package_name_cargo() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let name = extract_package_name(dir.path(), PublishRegistry::CratesIo).unwrap();
        assert_eq!(name, "my-crate");
    }

    #[test]
    fn test_extract_package_name_npm() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            "{\n  \"name\": \"my-npm-pkg\",\n  \"version\": \"1.0.0\"\n}\n",
        )
        .unwrap();
        let name = extract_package_name(dir.path(), PublishRegistry::Npm).unwrap();
        assert_eq!(name, "my-npm-pkg");
    }

    #[test]
    fn test_extract_package_name_pypi() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"my-pypi-pkg\"\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        let name = extract_package_name(dir.path(), PublishRegistry::Pypi).unwrap();
        assert_eq!(name, "my-pypi-pkg");
    }

    #[test]
    fn test_read_cargo_version() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();
        assert_eq!(read_cargo_version(dir.path()).unwrap(), "1.2.3");
    }

    #[test]
    fn test_read_npm_version() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            "{\n  \"name\": \"test\",\n  \"version\": \"3.4.5\"\n}\n",
        )
        .unwrap();
        assert_eq!(read_npm_version(dir.path()).unwrap(), "3.4.5");
    }

    #[test]
    fn test_detect_project_version_rust() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let (ver, typ) = detect_project_version(dir.path()).unwrap();
        assert_eq!(ver, "0.1.0");
        assert_eq!(typ, "rust");
    }

    #[test]
    fn test_detect_project_version_unknown() {
        let dir = TempDir::new().unwrap();
        assert!(detect_project_version(dir.path()).is_none());
    }

    #[test]
    fn test_extract_repo_name_from_ssh_url() {
        let _dir = TempDir::new().unwrap();
        // We can't easily mock `git remote get-url`, so test the URL parsing logic directly
        let url = "git@github.com:DraconDev/dracon-utilities.git";
        let repo_name = if url.starts_with("git@") {
            url.strip_prefix("git@")
                .and_then(|s| s.split_once(':'))
                .map(|(_, path)| path.trim_end_matches(".git"))
                .unwrap_or(url)
        } else {
            url
        };
        assert_eq!(repo_name, "DraconDev/dracon-utilities");
    }

    #[test]
    fn test_extract_repo_name_from_https_url() {
        let url = "https://github.com/DraconDev/dracon-utilities.git";
        let repo_name = url
            .trim_start_matches("https://")
            .trim_end_matches(".git")
            .split('/')
            .skip(1)
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(repo_name, "DraconDev/dracon-utilities");
    }

    #[test]
    fn test_extract_repo_name_from_ssh_url_with_port() {
        let url = "ssh://git@github.com:22/DraconDev/dracon-utilities.git";
        let repo_name = url
            .trim_start_matches("ssh://")
            .trim_end_matches(".git")
            .split('/')
            .skip(1)
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(repo_name, "DraconDev/dracon-utilities");
    }

    #[test]
    fn test_release_step_skipped_display() {
        let step = ReleaseStep::Skipped("already exists".to_string());
        assert!(matches!(step, ReleaseStep::Skipped(_)));
    }

    #[test]
    fn test_publish_registry_default() {
        assert_eq!(PublishRegistry::default(), PublishRegistry::CratesIo);
    }

    #[test]
    fn test_publish_registry_as_str() {
        assert_eq!(PublishRegistry::CratesIo.as_str(), "crates-io");
        assert_eq!(PublishRegistry::Npm.as_str(), "npm");
        assert_eq!(PublishRegistry::Pypi.as_str(), "pypi");
    }

    #[tokio::test]
    async fn test_release_pipeline_tag_only_when_auto_tag_true() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.2.0\"\n",
        )
        .unwrap();

        // Init git repo so tag commands work
        crate::git::git_cmd()
            .args(["init"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["add", "-A"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "commit",
                "--no-verify",
                "-m",
                "init",
                "--author",
                "test <test@test.com>",
            ])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();

        let policy = crate::policy::test_sync_policy();
        // auto_tag = true, auto_release = false, publish targets = []
        let steps = run_release_pipeline(
            dir.path(),
            "0.1.0",
            "0.2.0",
            "minor",
            &policy,
            true,  // auto_tag
            false, // auto_release
            &[],   // no publish targets
            false, // nix_auto_update
        )
        .await;

        // Should have tag step — either TagCreated or Failed (push fails without origin)
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, ReleaseStep::TagCreated(_)))
                || steps.iter().any(
                    |s| matches!(s, ReleaseStep::Failed { step, .. } if step.contains("push tag"))
                ),
            "expected tag creation or push attempt, got: {:?}",
            steps
        );
        // Should NOT have a release step
        assert!(
            !steps
                .iter()
                .any(|s| matches!(s, ReleaseStep::GitHubReleaseCreated(_))),
            "should not create GitHub release when auto_release=false"
        );
    }

    #[tokio::test]
    async fn test_release_pipeline_no_tag_when_auto_tag_false() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.2.0\"\n",
        )
        .unwrap();

        crate::git::git_cmd()
            .args(["init"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["add", "-A"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "commit",
                "--no-verify",
                "-m",
                "init",
                "--author",
                "test <test@test.com>",
            ])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();

        let policy = crate::policy::test_sync_policy();
        // auto_tag = false, auto_release = false, publish targets = []
        let steps = run_release_pipeline(
            dir.path(),
            "0.1.0",
            "0.2.0",
            "minor",
            &policy,
            false, // auto_tag disabled
            false, // auto_release disabled
            &[],   // no publish targets
            false, // nix_auto_update
        )
        .await;

        // Nothing should happen
        assert!(
            steps.is_empty(),
            "expected no steps when all toggles off, got: {:?}",
            steps
        );
    }

    #[tokio::test]
    async fn test_release_pipeline_release_on_major_when_auto_release_true() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();

        crate::git::git_cmd()
            .args(["init"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["add", "-A"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "commit",
                "--no-verify",
                "-m",
                "init",
                "--author",
                "test <test@test.com>",
            ])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();

        let policy = crate::policy::test_sync_policy();
        let steps = run_release_pipeline(
            dir.path(),
            "0.2.0",
            "1.0.0",
            "major",
            &policy,
            true,  // auto_tag
            true,  // auto_release enabled
            &[],   // no publish targets
            false, // nix_auto_update
        )
        .await;

        // Should have a GitHub release step (Created, Failed, or Skipped if it already exists)
        // Note: In environments with gh authenticated, `gh release view` may succeed against
        // the real API, causing Skipped("already exists") instead of Failed.
        assert!(
            steps.iter().any(|s| matches!(s, ReleaseStep::GitHubReleaseCreated(_)))
                || steps.iter().any(|s| matches!(s, ReleaseStep::Failed { step, .. } if step.contains("GitHub release")))
                || steps.iter().any(|s| matches!(s, ReleaseStep::Skipped(msg) if msg.contains("GitHub release"))),
            "expected GitHub release step (created, failed, or skipped) on major bump with auto_release=true, got: {:?}", steps
        );
    }

    #[tokio::test]
    async fn test_release_pipeline_no_release_on_minor_even_if_auto_release_true() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.2.0\"\n",
        )
        .unwrap();

        crate::git::git_cmd()
            .args(["init"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args(["add", "-A"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        crate::git::git_cmd()
            .args([
                "commit",
                "--no-verify",
                "-m",
                "init",
                "--author",
                "test <test@test.com>",
            ])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();

        let policy = crate::policy::test_sync_policy();
        let steps = run_release_pipeline(
            dir.path(),
            "0.1.0",
            "0.2.0",
            "minor",
            &policy,
            true,  // auto_tag
            true,  // auto_release enabled
            &[],   // no publish targets
            false, // nix_auto_update
        )
        .await;

        // Should NOT have a GitHub release step (only major bumps get releases)
        assert!(
            !steps.iter().any(|s| matches!(s, ReleaseStep::GitHubReleaseCreated(_)))
                && !steps.iter().any(|s| matches!(s, ReleaseStep::Failed { step, .. } if step.contains("GitHub release"))),
            "minor bump should not trigger GitHub release even with auto_release=true"
        );
    }

    #[test]
    fn test_detect_project_version_version_txt() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("version.txt"), "1.2.3\n").unwrap();
        let result = detect_project_version(dir.path());
        assert_eq!(result, Some(("1.2.3".to_string(), "plain")));
    }

    #[test]
    fn test_detect_project_version_version_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("VERSION"), "2.0.0\n").unwrap();
        let result = detect_project_version(dir.path());
        assert_eq!(result, Some(("2.0.0".to_string(), "plain")));
    }

    #[test]
    fn test_detect_project_version_pubspec_yaml() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pubspec.yaml"),
            "name: my_app\nversion: 3.1.0\n",
        )
        .unwrap();
        let result = detect_project_version(dir.path());
        assert_eq!(result, Some(("3.1.0".to_string(), "dart")));
    }

    #[test]
    fn test_detect_project_version_cargo_takes_priority_over_version_txt() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("version.txt"), "9.9.9\n").unwrap();
        let result = detect_project_version(dir.path());
        assert_eq!(result, Some(("0.1.0".to_string(), "rust")));
    }
}
