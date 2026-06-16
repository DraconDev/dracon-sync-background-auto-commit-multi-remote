use crate::policy::{RepoPolicyOverride, SyncPolicy};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn ensure_standard_files(
    repo: &Path,
    policy: &SyncPolicy,
    repo_override: &RepoPolicyOverride,
    policy_base_dir: Option<&Path>,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    if policy.standard_files.is_empty() {
        return Ok(vec![]);
    }

    let sync_base = policy_base_dir
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(".dracon/utilities/sync")));

    let Some(base) = sync_base else {
        anyhow::bail!("cannot resolve standard files base dir: no policy path and no home dir");
    };

    let mut copied = Vec::new();

    for cfg in &policy.standard_files {
        if repo_override.skip_standard_files.contains(&cfg.target) {
            continue;
        }

        let target_path = repo.join(&cfg.target);

        if target_path.exists() && !cfg.overwrite {
            continue;
        }

        let source_path = cfg.source_path(&base);

        if !source_path.exists() {
            eprintln!(
                "⚠️ standard file template missing: {} (tried {})",
                cfg.target,
                source_path.display()
            );
            continue;
        }

        if dry_run {
            println!(
                "📝 Would copy standard file: {} -> {}",
                source_path.display(),
                target_path.display()
            );
            copied.push(target_path);
            continue;
        }

        if target_path.exists() && cfg.overwrite {
            if target_path.is_dir() {
                std::fs::remove_dir_all(&target_path).with_context(|| {
                    format!("failed to remove existing directory {}", cfg.target)
                })?;
            } else {
                std::fs::remove_file(&target_path)
                    .with_context(|| format!("failed to remove existing {}", cfg.target))?;
            }
        }

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }

        std::fs::copy(&source_path, &target_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source_path.display(),
                target_path.display()
            )
        })?;

        copied.push(target_path);
    }

    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::StandardFileConfig;
    use tempfile::TempDir;

    fn make_policy(standard_files: Vec<StandardFileConfig>) -> SyncPolicy {
        SyncPolicy {
            standard_files,
            ..Default::default()
        }
    }

    fn make_override(skip: Vec<String>) -> RepoPolicyOverride {
        RepoPolicyOverride {
            skip_standard_files: skip,
            ..Default::default()
        }
    }

    #[test]
    fn test_copies_file_when_missing() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "AGPL"
        );
    }

    #[test]
    fn test_skips_when_target_exists() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        std::fs::write(repo_dir.join("LICENSE"), "EXISTING").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert!(copied.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "EXISTING"
        );
    }

    #[test]
    fn test_overwrites_when_configured() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "NEW_AGPL").unwrap();
        std::fs::write(repo_dir.join("LICENSE"), "OLD_LICENSE").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: true,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "NEW_AGPL"
        );
    }

    #[test]
    fn test_skips_from_repo_override() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("CUSTOM.md"), "custom content").unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/CUSTOM.md".to_string(),
            target: "CUSTOM.md".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec!["CUSTOM.md".to_string()]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert!(copied.is_empty());
        assert!(!repo_dir.join("CUSTOM.md").exists());
    }

    #[test]
    fn test_warns_when_template_missing() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/NONEXISTENT".to_string(),
            target: "NONEXISTENT.txt".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dry_run_does_not_copy_files() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), true);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert!(
            !repo_dir.join("LICENSE").exists(),
            "dry-run must not write files"
        );
    }

    #[test]
    fn test_subdirectory_target_creates_parent() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "docs/LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("docs/LICENSE")).unwrap(),
            "AGPL"
        );
    }

    #[test]
    fn test_overwrite_directory_target() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        std::fs::create_dir(repo_dir.join("LICENSE")).unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: true,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        assert!(repo_dir.join("LICENSE").is_file());
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "AGPL"
        );
    }

    #[test]
    fn test_absolute_source_path() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let abs_template = dir.path().join("custom_license.txt");
        std::fs::write(&abs_template, "CUSTOM").unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: abs_template.to_string_lossy().to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "CUSTOM"
        );
    }

    #[test]
    fn test_funding_yml_in_dot_github_subdir() {
        // GitHub discovers FUNDING.yml at .github/FUNDING.yml. The standard
        // files flow must allow long-form entries that target subdirectories
        // like .github/ while pulling the source from templates/FUNDING.yml.
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("FUNDING.yml"), "github: []\n").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/FUNDING.yml".to_string(),
            target: ".github/FUNDING.yml".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert!(repo_dir.join(".github/FUNDING.yml").exists());
        assert_eq!(
            std::fs::read_to_string(repo_dir.join(".github/FUNDING.yml")).unwrap(),
            "github: []\n"
        );
    }

    #[test]
    fn test_funding_yml_skip_standard_files_optout() {
        // Per-repo skip_standard_files must opt out FUNDING.yml cleanly.
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("FUNDING.yml"), "github: []\n").unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/FUNDING.yml".to_string(),
            target: ".github/FUNDING.yml".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![".github/FUNDING.yml".to_string()]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert!(copied.is_empty());
        assert!(!repo_dir.join(".github/FUNDING.yml").exists());
    }

    #[test]
    fn test_short_form_source_resolution() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPLv3").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "AGPLv3"
        );
    }
}
