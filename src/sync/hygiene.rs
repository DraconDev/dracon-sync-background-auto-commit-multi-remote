//! Hygiene reconciliation for ordinary staging: large-file ignore rules and
//! build-artifact de-indexing. Detection and authorization are side-effect free;
//! canonical ignore bytes go through the leased exchange helper, and index
//! changes only ever touch the PRIVATE candidate.
use super::staged_epoch;
use super::*;
use crate::git::staged_intent::{CachedIdentity, StagedIntentSnapshot};
use anyhow::ensure;
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::Instant;

#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;

pub(super) fn artifact_component(path: &Path, excluded: &BTreeSet<String>) -> Option<String> {
    path.parent()?.components().find_map(|component| {
        let name = component.as_os_str().to_str()?;
        (crate::exclude::is_excluded_dir_name(name, excluded)
            || crate::exclude::is_build_output_dir_name(name))
        .then(|| name.to_owned())
    })
}

/// Above-threshold untracked regular files: ignored, never committed or deleted.
async fn large_untracked(
    repo: &Path,
    policy: &SyncPolicy,
    excluded: &BTreeSet<String>,
    baseline: &StagedIntentSnapshot,
    admitted: &[PathBuf],
    deadline: Instant,
) -> Result<Vec<String>> {
    // Only paths admitted by the production partition can be reconciled.
    // A fresh global ls-files scan would absorb auto-stage opt-outs and private
    // materialized assets that the caller deliberately excluded.
    if !policy.auto_stage_untracked {
        return Ok(Vec::new());
    }
    let overrides = load_repo_override(repo);
    let excludes = crate::policy::effective_auto_commit_excludes(policy, &overrides);
    let mut patterns = Vec::new();
    for path in admitted {
        ensure!(
            Instant::now() < deadline,
            "large-untracked observation deadline expired; operator files retained"
        );
        if baseline
            .entries
            .contains_key(path.as_os_str().as_encoded_bytes())
            || crate::exclude::matches_untracked_exclude(
                repo,
                path,
                &policy.untracked_exclude_patterns,
            )
            || !crate::exclude::should_stage_entry_with_blob_size(
                repo,
                &dracon_git::types::DiffFile::new(path.clone(), dracon_git::types::FileStatus::Added),
                0,
                excluded,
                &policy.exclude_file_patterns,
                policy.max_stage_file_bytes,
                &excludes,
            )
        {
            continue;
        }
        // Recheck WITHOUT following aliases; uncertainty is not successful work.
        let meta = match std::fs::symlink_metadata(repo.join(path)) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let single_link = {
            #[cfg(target_os = "linux")]
            {
                meta.nlink() == 1
            }
            #[cfg(not(target_os = "linux"))]
            {
                true
            }
        };
        if meta.is_file() && meta.len() > policy.max_stage_file_bytes {
            ensure!(
                single_link,
                "large-file ignore refuses an aliased operator file"
            );
            ensure!(
                path != Path::new(".gitignore") && path != Path::new(".gitattributes"),
                "managed metadata exceeds size policy; refusing to hide it"
            );
            patterns.push(crate::exclude::hygiene::literal_file_pattern(path)?);
        }
    }
    Ok(patterns)
}

/// Verified work for one ordinary (no-entry-intent) staging cycle.
pub(super) struct Plan {
    /// Raw tracked path -> captured HEAD identity justifying removal.
    pub(super) deleted: BTreeMap<Vec<u8>, String>,
    /// Positive ignore rules required for the remaining eligible work.
    pub(super) patterns: Vec<String>,
}

/// Detect hygiene work. Returns `None` when nothing is eligible or when operator
/// staging exists; a foreign staged delta is never reconciled around.
pub(super) async fn plan(
    repo: &Path,
    policy: &SyncPolicy,
    excluded: &BTreeSet<String>,
    baseline: &StagedIntentSnapshot,
    cleanup: bool,
    admitted_large: &[PathBuf],
    deadline: Instant,
) -> Result<Option<Plan>> {
    let mut patterns =
        large_untracked(repo, policy, excluded, baseline, admitted_large, deadline).await?;
    ensure!(
        baseline.changed.is_empty(),
        "hygiene requires an entry cycle without staged intent"
    );
    let mut deleted = BTreeMap::new();
    if cleanup {
        let mut components = BTreeSet::new();
        for (raw, entry) in &baseline.entries {
            if entry.mode == 0o160000 {
                continue;
            } // never de-index a checkout
            if let Some(name) =
                artifact_component(&staged_epoch::path_from_git_bytes(raw)?, excluded)
            {
                deleted.insert(raw.clone(), entry.oid.clone());
                components.insert(name);
            }
        }
        for name in components {
            patterns.push(crate::exclude::hygiene::literal_directory_pattern(&name)?);
        }
    }
    if deleted.is_empty() && patterns.is_empty() {
        return Ok(None);
    }
    Ok(Some(Plan { deleted, patterns }))
}

/// Remove authorized artifact paths from the PRIVATE index only.
pub(super) async fn deindex(
    repo: &Path,
    index: &Path,
    plan: &Plan,
    deadline: Instant,
) -> Result<()> {
    if plan.deleted.is_empty() {
        return Ok(());
    }
    let mut input = Vec::new();
    for raw in plan.deleted.keys() {
        ensure!(
            Instant::now() < deadline,
            "artifact de-index deadline expired; actual index untouched"
        );
        ensure!(
            crate::git::is_safe_git_path(&staged_epoch::path_from_git_bytes(raw)?),
            "unsafe artifact path"
        );
        input.extend_from_slice(b":(literal)");
        input.extend_from_slice(raw);
        input.push(0);
    }
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(repo)
        .env("GIT_INDEX_FILE", index)
        // Exact NUL-separated paths: no pathspec expansion, no argv growth.
        .args([
            "rm",
            "--cached",
            "-q",
            "--ignore-unmatch",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ]);
    let output = crate::git::run_command_captured_output(
        command.into_tokio(),
        3600,
        "private artifact de-index",
        Some(&input),
    )
    .await?;
    ensure!(
        output.status.success(),
        "private artifact de-index refused: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

/// Independent re-verification before publication: every authorized removal must
/// still correspond to an unchanged HEAD identity inside an artifact component.
/// A HEAD advance or policy change revokes the exemption instead of widening it.
pub(super) async fn verify_deletions(
    repo: &Path,
    excluded: &BTreeSet<String>,
    deleted: &BTreeMap<Vec<u8>, String>,
    deadline: Instant,
) -> Result<()> {
    if deleted.is_empty() {
        return Ok(());
    }
    let tree =
        staged_epoch::git(repo, &["ls-tree", "-r", "-z", "--full-tree", "HEAD"], None).await?;
    let head: BTreeMap<Vec<u8>, CachedIdentity> =
        crate::git::staged_intent::parse_entries(&tree.stdout, false, deadline)?;
    for (raw, oid) in deleted {
        ensure!(
            Instant::now() < deadline,
            "artifact authorization deadline expired"
        );
        ensure!(
            artifact_component(&staged_epoch::path_from_git_bytes(raw)?, excluded).is_some(),
            "artifact authorization no longer applies to this path"
        );
        ensure!(
            head.get(raw).is_some_and(|entry| entry.oid == *oid),
            "artifact path changed in HEAD; removal not authorized"
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "hygiene_tests.rs"]
mod tests;
