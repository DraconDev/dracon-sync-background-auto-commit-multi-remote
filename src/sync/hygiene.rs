//! Hygiene reconciliation for ordinary staging: large-file ignore rules and
//! build-artifact de-indexing. Detection and authorization are side-effect free;
//! canonical ignore bytes go through the leased exchange helper, and index
//! changes only ever touch the PRIVATE candidate.
use super::staged_epoch;
use super::*;
use crate::git::staged_intent::{CachedIdentity, StagedIntentSnapshot};
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::Instant;

pub(super) fn artifact_component(path: &Path, excluded: &BTreeSet<String>) -> Option<String> {
    path.components().find_map(|component| {
        let name = component.as_os_str().to_str()?;
        (crate::exclude::is_excluded_dir_name(name, excluded)
            || crate::exclude::is_build_output_dir_name(name))
        .then(|| name.to_owned())
    })
}

/// Authorized tracking removals: the path is gone from the candidate, is still
/// present with an unchanged identity in captured HEAD, and still resolves into
/// a build-artifact component under the CURRENT policy.
///
/// Both the private staging closure and the commit transaction re-run this, so a
/// later policy change, HEAD advance or re-added payload revokes the exception.
pub(super) fn authorized(
    changed: &BTreeMap<Vec<u8>, Option<CachedIdentity>>,
    head: &BTreeMap<Vec<u8>, CachedIdentity>,
    excluded: &BTreeSet<String>,
) -> Result<BTreeSet<Vec<u8>>> {
    let mut authorized = BTreeSet::new();
    for (raw, entry) in changed {
        if entry.is_some() {
            continue;
        }
        let path = staged_epoch::path_from_git_bytes(raw)?;
        if artifact_component(&path, excluded).is_none() {
            continue;
        }
        if head.get(raw).is_some() {
            authorized.insert(raw.clone());
        }
    }
    Ok(authorized)
}

/// Above-threshold untracked regular files: ignored, never committed or deleted.
async fn large_untracked(
    repo: &Path,
    policy: &SyncPolicy,
    excluded: &BTreeSet<String>,
    deadline: Instant,
) -> Result<Vec<String>> {
    let output = staged_epoch::git(
        repo,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
    )
    .await?;
    let mut patterns = Vec::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
    {
        ensure!(
            Instant::now() < deadline,
            "large-untracked observation deadline expired; operator files retained"
        );
        let path = staged_epoch::path_from_git_bytes(raw)?;
        // A regular single-link file only: directories, symlinks and FIFOs are
        // never adopted, and the rule must be an exact representable name.
        let Ok(meta) = std::fs::symlink_metadata(repo.join(&path)) else {
            continue;
        };
        if meta.is_file() && meta.nlink() == 1 && meta.len() > policy.max_stage_file_bytes {
            patterns.push(crate::exclude::hygiene::literal_file_pattern(&path)?);
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
    deadline: Instant,
) -> Result<Option<Plan>> {
    let mut patterns = large_untracked(repo, policy, excluded, deadline).await?;
    ensure!(
        baseline.changed.is_empty(),
        "hygiene requires an entry cycle without staged intent"
    );
    let mut deleted = BTreeMap::new();
    if cleanup {
        let mut components = BTreeSet::new();
        for (raw, entry) in &baseline.entries {
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

/// Persist required ignore rules once. Absent rules defer the whole plan: the
/// rendered bytes are evidence, not authorization to commit around them.
pub(super) async fn write_ignore(repo: &Path, plan: &Plan, deadline: Instant) -> Result<()> {
    if plan.patterns.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let (applied, rendered) =
            super::hygiene_apply::apply(repo, &plan.patterns, deadline).await?;
        ensure!(
            applied || rendered.is_none(),
            "generated ignore deferred; operator bytes and staged work preserved"
        );
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (repo, deadline);
        anyhow::bail!("generated ignore requires atomic no-replace/exchange support; work retained")
    }
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
        .args(["rm", "--cached", "-q", "--ignore-unmatch", "-z", "--stdin"]);
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
