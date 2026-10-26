//! Generated metadata never borrows excluded-restore policy authorization.
//! One observe/render/lease/exchange implementation binds the original content
//! all the way to installation; no fresh read can adopt intervening edits.
use std::path::Path;
use anyhow::Result;
use tokio::time::Instant;

pub(super) async fn apply(
    repo: &Path,
    patterns: &[String],
    deadline: Instant,
) -> Result<(bool, Option<String>)> {
    super::excluded_revert::apply_ignore(repo, patterns, deadline).await
}
