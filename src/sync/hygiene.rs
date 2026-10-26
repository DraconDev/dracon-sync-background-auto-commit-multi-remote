//! Hygiene reconciliation for ordinary staging: artifact de-indexing and
//! large-file ignore rules. Detection is side-effect free; canonical ignore
//! bytes go through the leased exchange helper, index changes only touch the
//! PRIVATE candidate, and every authorized deletion is re-verified against HEAD.
use super::hygiene_apply;
use super::staged_epoch;
use super::*;
use crate::git::staged_intent::{CachedIdentity, StagedIntentSnapshot};
use tokio::time::Instant;

/// Exact, verified work for one ordinary staging cycle.
pub(super) struct Plan {
    /// Raw tracked path -> the HEAD object identity that authorizes its removal.
    pub(super) deleted: std::collections::BTreeMap<Vec<u8>, String>,
    /// Positive ignore rules to add; empty when only removals are needed.
    pub(super) patterns: Vec<String>,
}

fn artifact_component(path: &Path, excluded: &BTreeSet<String>) -> Option<String> {
    path.components().find_map(|component| {
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
        // never adopted, and an exact UTF-8 rule is required.
        let Ok(meta) = std::fs::symlink_metadata(repo.join(&path)) else {
            continue;
        };
        if meta.is_file() && meta.nlink() == 1 && meta.len() > policy.max_stage_file_bytes {
            patterns.push(crate::exclude::hygiene::literal_file_pattern(&path)?);
        }
    }
    Ok(patterns)
}

/// Detect hygiene work for this cycle. Returns `None` when nothing is eligible,
/// when operator staging exists (never reconciled around unknown intent), or on
/// an unborn branch (no tracked identities to verify against).
pub(super) async fn plan(
    repo: &Path,
    policy: &SyncPolicy,
    excluded: &BTreeSet<String>,
    deadline: Instant,
) -> Result<Option<Plan>> {
    let mut patterns = large_untracked(repo, policy, deadline).await?;
    let baseline =
        tokio::time::timeout_at(deadline, StagedIntentSnapshot::capture(repo, deadline))
            .await
            .context("hygiene baseline deadline expired")??;
    if !baseline.changed.is_empty() {
        return Ok(None);
    }
    let mut deleted = std::collections::BTreeMap::new();
    if policy.build_artifact_cleanup {
        let mut components = BTreeSet::new();
        for (raw, entry) in &baseline.entries {
            if let Some(name) = artifact_component(&staged_epoch::path_from_git_bytes(raw)?, excluded)
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

/// Persist the ignore rules once. Cheap no-op when no rule is needed or all are
/// already present; a deferred/failed canonical write is an error, never idle.
pub(super) async fn write_ignore(
    repo: &Path,
    plan: &Plan,
    deadline: Instant,
) -> Result<Option<String>> {
    if plan.patterns.is_empty() {
        return Ok(None);
    }
    let (applied, rendered) = hygiene_apply::apply(repo, &plan.patterns, deadline).await?;
    ensure!(
        applied || rendered.is_none(),
        "generated ignore deferred; operator bytes and staged work preserved"
    );
    Ok(rendered)
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

/// Independent re-verification of an authorized deletion: HEAD must still carry
/// exactly the identity recorded by the plan and the path must still be inside a
/// build-artifact component. Trusts no caller assertion and no later worktree.
pub(super) async fn verify_deletions(
    repo: &Path,
    excluded: &BTreeSet<String>,
    deleted: &std::collections::BTreeMap<Vec<u8>, String>,
    deadline: Instant,
) -> Result<()> {
    if deleted.is_empty() {
        return Ok(());
    }
    let tree = staged_epoch::git(
        repo,
        &["ls-tree", "-r", "-z", "--full-tree", "HEAD"],
        None,
    )
    .await?;
    let head: std::collections::BTreeMap<Vec<u8>, CachedIdentity> =
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
            "artifact path changed in HEAD; deletion not authorized"
        );
    }
    Ok(())
}
