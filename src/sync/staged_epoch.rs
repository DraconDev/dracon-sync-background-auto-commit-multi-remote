//! Immutable commit epochs. Neither refusal nor publication installs an index.
use super::*;
use anyhow::{bail, ensure};
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use tokio::time::Instant;

pub(super) struct Epoch {
    snapshot: StagedIntentSnapshot,
    reference: Option<String>,
    deadline: Instant,
}

async fn git(repo: &Path, args: &[&str], input: Option<&[u8]>) -> Result<std::process::Output> {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") { command.env_remove(key); }
    }
    command.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_NO_LAZY_FETCH", "1")
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(args).current_dir(repo);
    let output = crate::git::run_command_captured_output(command, 3600, "staged epoch", input).await?;
    ensure!(output.status.success(), "staged epoch Git {:?} refused: {}", args.first(),
        String::from_utf8_lossy(&output.stderr));
    Ok(output)
}

async fn reference(repo: &Path) -> Result<Option<String>> {
    // --quiet's sole expected non-success denotes detached HEAD.
    let child = crate::git::spawn_git_command_cancellable_stdin(repo,
        &["symbolic-ref", "--quiet", "HEAD"], "epoch reference")?;
    let (status, out, err) = crate::git::run_git_captured_output(child, repo, "epoch reference").await?;
    if status.code() == Some(1) { return Ok(None); }
    ensure!(status.success(), "cannot resolve epoch reference: {}", String::from_utf8_lossy(&err));
    let name = std::str::from_utf8(&out)?.strip_suffix('\n').context("unterminated ref")?;
    ensure!(name.starts_with("refs/heads/") && !name.bytes().any(|b| b.is_ascii_whitespace()), "invalid epoch reference");
    Ok(Some(name.to_owned()))
}

impl Epoch {
    pub(super) async fn capture(repo: &Path, timeout_secs: u64) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
        tokio::time::timeout_at(deadline, async {
            let reference = reference(repo).await?;
            let snapshot = StagedIntentSnapshot::capture(repo, deadline).await?;
            ensure!(reference == self::reference(repo).await?, "branch changed during capture");
            Ok(Self { snapshot, reference, deadline })
        }).await.context("epoch capture deadline expired")?
    }

    pub(super) fn has_intent(&self) -> bool { !self.snapshot.changed.is_empty() }

    pub(super) fn entries(&self) -> Result<Vec<dracon_git::types::DiffFile>> {
        use dracon_git::types::{DiffFile, FileStatus};
        self.snapshot.changed.iter().map(|(path, value)| {
            #[cfg(unix)]
            let path = { use std::os::unix::ffi::OsStringExt; PathBuf::from(std::ffi::OsString::from_vec(path.clone())) };
            #[cfg(not(unix))]
            let path = PathBuf::from(std::str::from_utf8(path)?);
            Ok(DiffFile::new(path, if value.is_some() { FileStatus::Modified } else { FileStatus::Deleted }))
        }).collect()
    }

    async fn unchanged(&self, repo: &Path, lock: &crate::git::IndexLock) -> Result<()> {
        ensure!(lock.is_owned(), "epoch index lock was replaced; retained foreign lock");
        ensure!(self.reference == reference(repo).await?, "epoch branch changed");
        ensure!(self.snapshot == StagedIntentSnapshot::capture(repo, self.deadline).await?,
            "epoch index or parent changed; all staged/worktree data retained");
        Ok(())
    }

    async fn eligible(&self, repo: &Path, policy: &SyncPolicy, excluded: &BTreeSet<String>) -> Result<()> {
        ensure!(self.snapshot.changed.len() <= policy.max_stage_batch_files, "captured staged intent exceeds strict batch limit");
        let overrides = load_repo_override(repo);
        let excludes = crate::policy::effective_auto_commit_excludes(policy, &overrides);
        // Query captured object IDs, not worktree sizes or path-resolved live blobs.
        let mut input = Vec::new();
        let mut paths = Vec::new();
        for (path, entry) in &self.snapshot.changed {
            if let Some(entry) = entry {
                if entry.mode != 0o160000 {
                    input.extend_from_slice(entry.oid.as_bytes()); input.push(b'\n'); paths.push(path);
                }
            }
        }
        ensure!(input.len() <= 16 * 1024 * 1024, "cached size input budget exceeded");
        let sizes = if paths.is_empty() { Vec::new() } else {
            git(repo, &["cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"], Some(&input)).await?.stdout
        };
        let mut records = sizes.split(|b| *b == b'\n').filter(|v| !v.is_empty());
        let mut measured = HashMap::new();
        for path in paths {
            let fields: Vec<_> = std::str::from_utf8(records.next().context("missing cached size")?)?.split(' ').collect();
            let entry = self.snapshot.changed[path].as_ref().unwrap();
            ensure!(fields.len() == 3 && fields[0] == entry.oid && fields[1] == "blob", "unmeasurable captured blob");
            measured.insert(path, fields[2].parse::<u64>()?);
        }
        ensure!(records.next().is_none(), "extra cached size records");
        for (raw, entry) in &self.snapshot.changed {
            let path = self.entries()?.into_iter().find(|e| {
                #[cfg(unix)] { use std::os::unix::ffi::OsStrExt; e.path.as_os_str().as_bytes() == raw }
                #[cfg(not(unix))] { e.path.to_str().map(str::as_bytes) == Some(raw.as_slice()) }
            }).context("missing captured path")?;
            let size = measured.get(raw).copied().unwrap_or(0);
            ensure!(crate::exclude::should_stage_entry_with_blob_size(repo, &path, size, excluded,
                &policy.exclude_file_patterns, policy.max_stage_file_bytes, &excludes),
                "captured staged path is excluded or oversized: {}", path.path.display());
            // Preserve the existing untracked opt-outs, without interpreting a
            // worktree rewrite as the content of an already-staged blob.
            if matches!(self.snapshot.head, CapturedHead::Unborn { .. }) && entry.is_some() {
                ensure!(!crate::exclude::matches_untracked_exclude(repo, &path.path, &policy.untracked_exclude_patterns),
                    "captured root path excluded by untracked policy");
            }
        }
        Ok(())
    }

    /// Returns true when index/worktree follow-up remains. One absolute deadline
    /// owns every child, hook, guard, signing operation and ref transaction.
    pub(super) async fn commit(self, repo: &Path, message: &str, policy: &SyncPolicy,
        excluded: &BTreeSet<String>, prepared: Option<&crate::bucket_assets::Prepared>) -> Result<bool> {
        let deadline = self.deadline;
        tokio::time::timeout_at(deadline, async {
            ensure!(policy.auto_commit && self.has_intent(), "no authorized staged commit intent");
            ensure!(commit_allowed_by_ownership(repo, policy), "epoch ownership guard refused");
            let index = git(repo, &["rev-parse", "--path-format=absolute", "--git-path", "index"], None).await?;
            let index = PathBuf::from(std::str::from_utf8(&index.stdout)?.trim_end_matches('\n'));
            ensure!(index.is_absolute(), "index identity is not absolute");
            let lock = crate::git::IndexLock::acquire_path(index.with_extension("lock"), repo).map_err(anyhow::Error::msg)?;
            self.unchanged(repo, &lock).await?;
            self.eligible(repo, policy, excluded).await?;
            let tree = self.snapshot.materialize_tree(repo, deadline).await?;
            let private = if prepared.is_none() { prepare_private(repo, policy).await? } else { None };
            let prepared = prepared.or(private.as_ref());
            let guards = async {
                if let Some(prepared) = prepared { prepared.verify_staged(repo, policy.stage_op_timeout_secs).await?; }
                crate::bucket_guard::check_staged_forward_only(repo, policy.stage_op_timeout_secs,
                    std::env::var("DRACON_BUCKET_GUARD").ok().as_deref()).await
            };
            guards.await?;
            self.unchanged(repo, &lock).await?;
            // Original hook order/arguments/editor semantics; hook edits are
            // read back, refusals propagate, and no hook runs twice.
            let approved = run_sealed_commit_hooks(repo, message, policy.stage_op_timeout_secs).await?;
            self.unchanged(repo, &lock).await?;
            if let Some(prepared) = prepared { prepared.verify_staged(repo, policy.stage_op_timeout_secs).await?; }
            crate::bucket_guard::check_staged_forward_only(repo, policy.stage_op_timeout_secs,
                std::env::var("DRACON_BUCKET_GUARD").ok().as_deref()).await?;
            verify_storage(repo, &tree, policy.stage_op_timeout_secs).await?;
            self.eligible(repo, policy, excluded).await?;
            self.unchanged(repo, &lock).await?;
            ensure!(commit_allowed_by_ownership(repo, policy), "epoch ownership changed");
            let mut args = vec!["commit-tree", tree.as_str()];
            let parent = match &self.snapshot.head { CapturedHead::Commit(parent) => Some(parent.as_str()), _ => None };
            if let Some(parent) = parent { args.extend(["-p", parent]); }
            // commit-tree doesn't honor commit.gpgSign by itself.
            let mut config = crate::policy::tokio_git_command();
            config.args(["config", "--bool", "--get", "commit.gpgSign"]).current_dir(repo);
            let signing = crate::git::run_command_captured_output(config, policy.stage_op_timeout_secs, "epoch signing config", None).await?;
            if signing.status.success() {
                ensure!(matches!(signing.stdout.as_slice(), b"true\n" | b"false\n"), "invalid signing policy");
                if signing.stdout == b"true\n" { args.push("-S"); }
            } else { ensure!(signing.status.code() == Some(1), "cannot read signing policy"); }
            let message = format!("{}\n", approved.message);
            ensure!(message.len() <= 1024 * 1024, "commit message exceeds budget");
            let commit = git(repo, &args, Some(message.as_bytes())).await?;
            let oid = std::str::from_utf8(&commit.stdout)?.trim();
            ensure!(crate::git::is_valid_object_id(oid), "invalid created commit identity");
            self.unchanged(repo, &lock).await?;
            let zero = "0".repeat(oid.len());
            let previous = parent.unwrap_or(&zero);
            let transaction = if let Some(reference) = &self.reference {
                format!("start\nsymref-verify HEAD {reference}\nupdate {reference} {oid} {previous}\nprepare\ncommit\n")
            } else {
                format!("start\noption no-deref\nupdate HEAD {oid} {previous}\nprepare\ncommit\n")
            };
            git(repo, &["update-ref", "-m", "commit: captured staged epoch", "--stdin"], Some(transaction.as_bytes())).await?;
            // Keep the real lock through post-commit. Never reset/restore/re-add
            // the captured tree over later operator index or worktree content.
            run_configured_hook(repo, "post-commit", &[], policy.stage_op_timeout_secs).await
                .context("post-commit failed AFTER epoch publication")?;
            ensure!(lock.is_owned(), "index lock replaced AFTER epoch publication");
            let after = StagedIntentSnapshot::capture(repo, deadline).await?;
            let child = crate::git::spawn_git_command_cancellable_stdin(repo,
                &["diff", "--quiet", "HEAD", "--"], "epoch follow-up")?;
            let (status, _, _) = crate::git::run_git_captured_output(child, repo, "epoch follow-up").await?;
            ensure!(status.success() || status.code() == Some(1), "cannot inspect post-publication work");
            Ok(!after.changed.is_empty() || !status.success())
        }).await.context("staged epoch deadline expired; publication may already have occurred, inspect HEAD")?
    }
}

pub(super) async fn prepare_private(repo: &Path, policy: &SyncPolicy) -> Result<Option<crate::bucket_assets::Prepared>> {
    match crate::bucket_runner_bridge::prepare(repo, false, policy).await? {
        Some(prepared) => Ok(Some(prepared)),
        None => crate::bucket_assets::prepare(repo, false, policy.stage_op_timeout_secs).await,
    }
}

async fn verify_storage(repo: &Path, tree: &str, timeout_secs: u64) -> Result<()> {
    // Parent owns the private directory, including cancellation cleanup.
    let workspace = tempfile::tempdir()?;
    let index = workspace.path().join("candidate.index");
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() { if key.to_string_lossy().starts_with("GIT_") { command.env_remove(key); } }
    command.args(["read-tree", tree]).env("GIT_INDEX_FILE", &index).current_dir(repo);
    let result = crate::git::run_command_captured_output(command, timeout_secs, "epoch private view", None).await?;
    ensure!(result.status.success(), "cannot construct private storage view");
    let executable = std::env::current_exe()?;
    #[cfg(test)]
    let executable = std::env::var_os("DRACON_EPOCH_TEST_HELPER").map(PathBuf::from).unwrap_or(executable);
    ensure!(executable.is_absolute(), "epoch helper executable must be absolute");
    let mut command = tokio::process::Command::new(executable);
    for (key, _) in std::env::vars_os() { if key.to_string_lossy().starts_with("GIT_") { command.env_remove(key); } }
    command.arg("storage").arg("verify-configured-tree").arg("--repo").arg(repo)
        .arg("--tree").arg(tree).arg("--index").arg(&index).current_dir(repo);
    let result = crate::git::run_command_captured_output(command, timeout_secs, "epoch storage verifier", None).await?;
    ensure!(result.status.success(), "immutable storage verifier refused: {}", String::from_utf8_lossy(&result.stderr));
    ensure!(result.stdout == format!("{} {}\n", env!("DRACON_EPOCH_SOURCE_ID"), tree).as_bytes(),
        "immutable storage verifier executable/source identity mismatch");
    Ok(())
}
