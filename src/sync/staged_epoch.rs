//! Immutable commit epochs. Neither refusal nor publication installs an index.
use super::*;
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use anyhow::ensure;
use tokio::time::Instant;
#[cfg(unix)]
#[path = "unmerged_epoch.rs"]
mod unmerged;

pub(super) async fn reconcile_unmerged(
    repo: &Path,
    enabled: bool,
    deadline: Instant,
) -> Result<Option<(Epoch, usize)>> {
    #[cfg(unix)]
    {
        unmerged::reconcile(repo, enabled, deadline).await
    }
    #[cfg(not(unix))]
    {
        let _ = (repo, enabled, deadline);
        anyhow::bail!("descriptor-bound unmerged reconciliation unsupported; index retained")
    }
}

#[derive(Debug)]
enum EpochConflict {
    LockBusy,
    LockReplaced,
    ReferenceChanged,
    IndexChanged,
}

impl std::fmt::Display for EpochConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::LockBusy => "epoch index lock held by another operation",
            Self::LockReplaced => "epoch index lock was replaced; retained foreign lock",
            Self::ReferenceChanged => "epoch branch changed",
            Self::IndexChanged => {
                "epoch index or parent changed; actual staged/worktree data retained"
            }
        })
    }
}
impl std::error::Error for EpochConflict {}

pub(super) fn is_conflict(error: &anyhow::Error) -> bool {
    error.is::<EpochConflict>()
}

fn acquire_epoch_lock(path: PathBuf) -> Result<crate::git::IndexLock> {
    match crate::git::IndexLock::try_acquire_path(path) {
        Ok(lock) => Ok(lock),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(anyhow::Error::new(EpochConflict::LockBusy))
        }
        Err(error) => Err(error).context("cannot acquire epoch index lock"),
    }
}

pub(super) struct Epoch {
    snapshot: StagedIntentSnapshot,
    reference: Option<String>,
    deadline: Instant,
    // Keep the independent pack after installation: a later actual-index
    // replacement must not make the proven candidate collectible by prune.
    retained: Option<PathBuf>,
    // ADDED 2026-10-26: the spool is RETAINED for diagnosis but must not be
    // retained forever. Holding the lease for the epoch's lifetime makes
    // the periodic reaper skip this directory while it is in use, and
    // dropping it on EVERY exit path (early return, refusal, deadline,
    // success) releases the spool so `git::reap_abandoned_spools` can
    // reclaim it. Before this, 141 spools x ~875 MiB filled the root
    // filesystem to 100%.
    spool_lease: crate::git::SpoolLease,
    // Verified artifact de-index authorization: raw path -> captured HEAD object
    // id. Re-proved against HEAD in the commit transaction; never a general
    // exclusion waiver, and empty for every captured (user) entry epoch.
    deletions: std::collections::BTreeMap<Vec<u8>, String>,
    // Deferred canonical ignore rules for this ordinary cycle. Written only
    // after a successful publication so staging never sees its own new rules.
    ignore_patterns: Vec<String>,
}

pub(super) async fn git(
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<std::process::Output> {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(repo);
    let output =
        crate::git::run_command_captured_output(command.into_tokio(), 3600, "staged epoch", input)
            .await?;
    ensure!(
        output.status.success(),
        "staged epoch Git {:?} refused: {}",
        args.first(),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

async fn reference(repo: &Path) -> Result<Option<String>> {
    // --quiet's sole expected non-success denotes detached HEAD.
    let child = crate::git::spawn_git_command_cancellable_stdin(
        repo,
        &["symbolic-ref", "--quiet", "HEAD"],
        "epoch reference",
    )?;
    let (status, out, err) =
        crate::git::run_git_captured_output(child, repo, "epoch reference").await?;
    if status.code() == Some(1) {
        return Ok(None);
    }
    ensure!(
        status.success(),
        "cannot resolve epoch reference: {}",
        String::from_utf8_lossy(&err)
    );
    let name = std::str::from_utf8(&out)?
        .strip_suffix('\n')
        .context("unterminated ref")?;
    ensure!(
        name.starts_with("refs/heads/") && !name.bytes().any(|b| b.is_ascii_whitespace()),
        "invalid epoch reference"
    );
    Ok(Some(name.to_owned()))
}

impl Epoch {
    pub(super) async fn capture(repo: &Path, timeout_secs: u64) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
        tokio::time::timeout_at(deadline, async {
            let reference = reference(repo).await?;
            let snapshot = StagedIntentSnapshot::capture(repo, deadline).await?;
            ensure!(
                reference == self::reference(repo).await?,
                "branch changed during capture"
            );
            Ok(Self {
                snapshot,
                reference,
                deadline,
                retained: None,
                spool_lease: crate::git::SpoolLease::new(None),
                deletions: std::collections::BTreeMap::new(),
                ignore_patterns: Vec::new(),
            })
        })
        .await
        .context("epoch capture deadline expired")?
    }

    /// Reconciliation precedes ordinary capture, but its complete raw-index
    /// observation and private candidate are themselves the entry provenance.
    pub(super) async fn capture_entry(
        repo: &Path,
        timeout_secs: u64,
        auto_resolve: bool,
    ) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
        tokio::time::timeout_at(deadline, async {
            match Self::capture(repo, timeout_secs).await {
                Ok(mut epoch) => {
                    epoch.deadline = deadline;
                    #[cfg(target_os = "linux")]
                    {
                        epoch.deletions =
                            super::hygiene::recover(repo, &epoch.snapshot, deadline).await?;
                    }
                    Ok(epoch)
                }
                Err(original) if auto_resolve => {
                    if let Some((epoch, _)) = reconcile_unmerged(repo, true, deadline).await? {
                        Ok(epoch)
                    } else {
                        Err(original)
                            .context("unmerged/uncertain entry retained; no verified resolution")
                    }
                }
                Err(error) => Err(error),
            }
        })
        .await
        .context("entry capture/reconciliation absolute deadline expired")?
    }

    /// Deferred canonical ignore rules for this ordinary cycle. Read BEFORE
    /// `commit` consumes the epoch; the caller applies them only after the
    /// commit landed so the new rules can never block their own staging.
    pub(super) fn deferred_ignore(&self) -> Option<(Vec<String>, Instant)> {
        (!self.ignore_patterns.is_empty()).then(|| (self.ignore_patterns.clone(), self.deadline))
    }

    pub(super) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(super) fn has_intent(&self) -> bool {
        !self.snapshot.changed.is_empty()
    }

    /// Empty candidates still owe a truthful read-only follow-up. A foreign
    /// index/ref update is pending work, not permission to acquire/reset it.
    pub(super) async fn index_changed(&self, repo: &Path) -> Result<bool> {
        tokio::time::timeout_at(self.deadline, async {
            Ok(
                self.snapshot != StagedIntentSnapshot::capture(repo, self.deadline).await?
                    || self.reference != reference(repo).await?,
            )
        })
        .await
        .context("empty epoch follow-up deadline expired")?
    }

    /// Release the retained spool. Idempotent and best-effort.
    ///
    /// ADDED 2026-10-26 (disk-exhaustion incident). Only call this once the
    /// candidate's future is SETTLED: it is published (so normal history
    /// makes the objects reachable), or it is an empty candidate with
    /// nothing to commit. A refusal, deadline or unpublished error leaves
    /// the spool on disk for diagnosis; the age-based reaper in
    /// `git::epoch_spool` bounds those.
    ///
    /// Removal is skipped while the lease is still held because the lease
    /// is what tells the reaper this directory is in use; releasing the
    /// lease therefore has to come first.
    pub(super) fn release_spool(&mut self) {
        let directory = self.retained.take();
        // Drop the lease BEFORE removing, so the reaper never sees a live
        // lease on a directory that is already gone.
        self.spool_lease = crate::git::SpoolLease::new(None);
        if let Some(directory) = directory {
            if let Err(error) = std::fs::remove_dir_all(&directory) {
                // Best-effort: the periodic reaper retries an aged spool.
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "⚠️ cannot release epoch spool {}: {error}",
                        directory.display()
                    );
                }
            }
        }
    }

    pub(super) fn entries(&self) -> Result<Vec<dracon_git::types::DiffFile>> {
        use dracon_git::types::{DiffFile, FileStatus};
        self.snapshot
            .changed
            .iter()
            .map(|(path, value)| {
                #[cfg(unix)]
                let path = {
                    use std::os::unix::ffi::OsStringExt;
                    PathBuf::from(std::ffi::OsString::from_vec(path.clone()))
                };
                #[cfg(not(unix))]
                let path = PathBuf::from(std::str::from_utf8(path)?);
                Ok(DiffFile::new(
                    path,
                    if value.is_some() {
                        FileStatus::Modified
                    } else {
                        FileStatus::Deleted
                    },
                ))
            })
            .collect()
    }

    async fn unchanged(&self, repo: &Path, lock: &crate::git::IndexLock) -> Result<()> {
        ensure!(lock.is_owned(), EpochConflict::LockReplaced);
        ensure!(
            self.reference == reference(repo).await?,
            EpochConflict::ReferenceChanged
        );
        if self.snapshot != StagedIntentSnapshot::capture(repo, self.deadline).await? {
            // Name the retained spool AND hold its lease for the error path:
            // the reaper must not reclaim a candidate we are still telling
            // an operator to inspect.
            let retained = self.spool_lease.named().or(self.retained.as_deref());
            return Err(anyhow::Error::new(EpochConflict::IndexChanged))
                .with_context(|| format!("candidate backup: {retained:?}"));
        }
        Ok(())
    }

    async fn eligible(
        &self,
        repo: &Path,
        policy: &SyncPolicy,
        excluded: &BTreeSet<String>,
    ) -> Result<()> {
        ensure!(
            self.snapshot.changed.len() <= policy.stage_batch_limit(),
            "captured staged intent exceeds strict batch limit"
        );
        let rejected = self.ineligible_paths(repo, policy, excluded).await?;
        if let Some(path) = rejected.first() {
            anyhow::bail!(
                "captured staged path is excluded or oversized: {}",
                path_from_git_bytes(path)?.display()
            );
        }
        Ok(())
    }

    // Inspection errors are never eligibility decisions. Only policy/size
    // rejection may prune an OWNED ordinary candidate; captured user staging
    // still goes through eligible() and is refused intact. The ONE exception is
    // a verified artifact de-index recorded by this epoch's own plan: the path
    // is absent from the candidate and its captured HEAD identity is retained.
    async fn ineligible_paths(
        &self,
        repo: &Path,
        policy: &SyncPolicy,
        excluded: &BTreeSet<String>,
    ) -> Result<Vec<Vec<u8>>> {
        let overrides = load_repo_override(repo);
        let excludes = crate::policy::effective_auto_commit_excludes(policy, &overrides);
        // Query captured object IDs, not worktree sizes or path-resolved live blobs.
        let mut input = Vec::new();
        let mut paths = Vec::new();
        for (path, entry) in &self.snapshot.changed {
            if let Some(entry) = entry {
                if entry.mode != 0o160000 {
                    input.extend_from_slice(entry.oid.as_bytes());
                    input.push(b'\n');
                    paths.push(path);
                }
            }
        }
        ensure!(
            input.len() <= 16 * 1024 * 1024,
            "cached size input budget exceeded"
        );
        let sizes = if paths.is_empty() {
            Vec::new()
        } else {
            git(
                repo,
                &[
                    "cat-file",
                    "--batch-check=%(objectname) %(objecttype) %(objectsize)",
                ],
                Some(&input),
            )
            .await?
            .stdout
        };
        let mut records = sizes.split(|b| *b == b'\n').filter(|v| !v.is_empty());
        let mut measured = HashMap::new();
        for path in paths {
            let fields: Vec<_> =
                std::str::from_utf8(records.next().context("missing cached size")?)?
                    .split(' ')
                    .collect();
            let entry = self.snapshot.changed[path].as_ref().unwrap();
            ensure!(
                fields.len() == 3 && fields[0] == entry.oid && fields[1] == "blob",
                "unmeasurable captured blob"
            );
            measured.insert(path, fields[2].parse::<u64>()?);
        }
        ensure!(records.next().is_none(), "extra cached size records");
        let mut rejected = Vec::new();
        for ((raw, entry), path) in self.snapshot.changed.iter().zip(self.entries()?) {
            let size = measured.get(raw).copied().unwrap_or(0);
            if entry.is_none() && self.deletions.contains_key(raw) {
                // Only the directory exclusion can be waived. Explicit repo
                // opt-outs and file-pattern exclusions remain binding, including
                // after hooks. A captured user epoch never has this authority.
                let cleanup = overrides
                    .build_artifact_cleanup
                    .unwrap_or(policy.build_artifact_cleanup);
                if !cleanup
                    || !crate::exclude::should_stage_entry_with_blob_size(
                        repo,
                        &path,
                        size,
                        &BTreeSet::new(),
                        &policy.exclude_file_patterns,
                        policy.max_stage_file_bytes,
                        &excludes,
                    )
                {
                    rejected.push(raw.clone());
                }
                continue;
            }
            if !(crate::exclude::should_stage_entry_with_blob_size(
                repo,
                &path,
                size,
                excluded,
                &policy.exclude_file_patterns,
                policy.max_stage_file_bytes,
                &excludes,
            )) {
                rejected.push(raw.clone());
                continue;
            }
            // Preserve the existing untracked opt-outs, without interpreting a
            // worktree rewrite as the content of an already-staged blob.
            if matches!(self.snapshot.head, CapturedHead::Unborn { .. })
                && entry.is_some()
                && crate::exclude::matches_untracked_exclude(
                    repo,
                    &path.path,
                    &policy.untracked_exclude_patterns,
                )
            {
                rejected.push(raw.clone());
            }
        }
        Ok(rejected)
    }

    /// Returns true when index/worktree follow-up remains. One absolute deadline
    /// owns every child, hook, guard, signing operation and ref transaction.
    pub(super) async fn commit(
        self,
        repo: &Path,
        message: &str,
        policy: &SyncPolicy,
        excluded: &BTreeSet<String>,
        prepared: Option<&crate::bucket_assets::Prepared>,
    ) -> Result<bool> {
        let deadline = self.deadline;
        let mut published = false;
        let result = tokio::time::timeout_at(deadline, async {
            ensure!(
                policy.auto_commit && self.has_intent(),
                "no authorized staged commit intent"
            );
            ensure!(
                commit_allowed_by_ownership(repo, policy),
                "epoch ownership guard refused"
            );
            let index = git(
                repo,
                &["rev-parse", "--path-format=absolute", "--git-path", "index"],
                None,
            )
            .await?;
            let index = PathBuf::from(std::str::from_utf8(&index.stdout)?.trim_end_matches('\n'));
            ensure!(index.is_absolute(), "index identity is not absolute");
            let lock = acquire_epoch_lock(index.with_extension("lock"))?;
            self.unchanged(repo, &lock).await?;
            self.eligible(repo, policy, excluded).await?;
            super::hygiene::verify_deletions(repo, excluded, &self.deletions, deadline).await?;
            let tree = self.snapshot.materialize_tree(repo, deadline).await?;
            let private = if prepared.is_none() {
                prepare_private(repo, policy).await?
            } else {
                None
            };
            let prepared = prepared.or(private.as_ref());
            let guards = async {
                if let Some(prepared) = prepared {
                    prepared
                        .verify_staged(repo, policy.stage_op_timeout_secs)
                        .await?;
                }
                crate::bucket_guard::check_staged_forward_only(
                    repo,
                    policy.stage_op_timeout_secs,
                    std::env::var("DRACON_BUCKET_GUARD").ok().as_deref(),
                )
                .await
            };
            guards.await?;
            self.unchanged(repo, &lock).await?;
            // Original hook order/arguments/editor semantics; hook edits are
            // read back, refusals propagate, and no hook runs twice.
            let required_binding =
                verify_storage(repo, &tree, policy.stage_op_timeout_secs, false).await?;
            let approved =
                run_sealed_commit_hooks(repo, message, policy.stage_op_timeout_secs).await?;
            self.unchanged(repo, &lock).await?;
            if let Some(prepared) = prepared {
                prepared
                    .verify_staged(repo, policy.stage_op_timeout_secs)
                    .await?;
            }
            crate::bucket_guard::check_staged_forward_only(
                repo,
                policy.stage_op_timeout_secs,
                std::env::var("DRACON_BUCKET_GUARD").ok().as_deref(),
            )
            .await?;
            verify_storage(repo, &tree, policy.stage_op_timeout_secs, required_binding).await?;
            self.eligible(repo, policy, excluded).await?;
            self.unchanged(repo, &lock).await?;
            ensure!(
                commit_allowed_by_ownership(repo, policy),
                "epoch ownership changed"
            );
            let mut args = vec!["commit-tree", tree.as_str()];
            let parent = match &self.snapshot.head {
                CapturedHead::Commit(parent) => Some(parent.as_str()),
                _ => None,
            };
            if let Some(parent) = parent {
                args.extend(["-p", parent]);
            }
            // commit-tree doesn't honor commit.gpgSign by itself.
            let mut config = crate::policy::tokio_git_command();
            config
                .args(["config", "--bool", "--get", "commit.gpgSign"])
                .current_dir(repo);
            let signing = crate::git::run_command_captured_output(
                config.into_tokio(),
                policy.stage_op_timeout_secs,
                "epoch signing config",
                None,
            )
            .await?;
            if signing.status.success() {
                ensure!(
                    matches!(signing.stdout.as_slice(), b"true\n" | b"false\n"),
                    "invalid signing policy"
                );
                if signing.stdout == b"true\n" {
                    args.push("-S");
                }
            } else {
                ensure!(
                    signing.status.code() == Some(1),
                    "cannot read signing policy"
                );
            }
            let message = format!("{}\n", approved.message);
            ensure!(
                message.len() <= 1024 * 1024,
                "commit message exceeds budget"
            );
            let commit = git(repo, &args, Some(message.as_bytes())).await?;
            let oid = std::str::from_utf8(&commit.stdout)?.trim();
            ensure!(
                crate::git::is_valid_object_id(oid),
                "invalid created commit identity"
            );
            self.unchanged(repo, &lock).await?;
            let zero = "0".repeat(oid.len());
            let previous = parent.unwrap_or(&zero);
            let transaction = if let Some(reference) = &self.reference {
                // HEAD and its referent cannot both be transaction updates:
                // Git also records HEAD's reflog for this branch update. The
                // owned index lock blocks cooperative checkout, unchanged()
                // checked HEAD immediately above, and the explicit captured
                // branch CAS can never update a newly selected other branch.
                format!("start\nupdate {reference} {oid} {previous}\nprepare\ncommit\n")
            } else {
                format!("start\noption no-deref\nupdate HEAD {oid} {previous}\nprepare\ncommit\n")
            };
            git(
                repo,
                &[
                    "update-ref",
                    "-m",
                    "commit: captured staged epoch",
                    "--stdin",
                ],
                Some(transaction.as_bytes()),
            )
            .await?;
            published = true;
            let lock_retained = !lock.is_owned();
            drop(lock);
            // Native post-commit runs after publication and does not control its
            // success. A failing hook remains diagnostic and pending follow-up.
            let post_failed = if let Err(error) =
                run_configured_hook(repo, "post-commit", &[], policy.stage_op_timeout_secs).await
            {
                eprintln!(
                    "⚠️ {} post-commit failed AFTER epoch publication: {error:#}",
                    repo.display()
                );
                true
            } else {
                false
            };
            let after = StagedIntentSnapshot::capture(repo, deadline).await?;
            let child = crate::git::spawn_git_command_cancellable_stdin(
                repo,
                &["diff", "--quiet", "HEAD", "--"],
                "epoch follow-up",
            )?;
            let (status, _, _) =
                crate::git::run_git_captured_output(child, repo, "epoch follow-up").await?;
            ensure!(
                status.success() || status.code() == Some(1),
                "cannot inspect post-publication work"
            );
            let untracked = git(
                repo,
                &["ls-files", "--others", "--exclude-standard", "-z"],
                None,
            )
            .await?;
            let paths = untracked
                .stdout
                .split(|byte| *byte == 0)
                .filter(|raw| !raw.is_empty())
                .map(path_from_git_bytes)
                .collect::<Result<Vec<_>>>()?;
            let untracked_pending = match prepared {
                Some(prepared) => prepared.has_untracked_followup(repo, &paths).await?,
                None => !paths.is_empty(),
            };
            let branch_changed = reference(repo).await? != self.reference;
            let pending = lock_retained
                || post_failed
                || branch_changed
                || !after.changed.is_empty()
                || !status.success()
                || untracked_pending;
            Ok(pending)
        })
        .await;
        // ADDED 2026-10-26: deterministic release of the retained spool.
        //
        // This is the half that actually STOPS the leak. The original
        // rationale ("a later foreign index replacement can remove its only
        // ordinary index root before this epoch commits") is satisfied once
        // the commit published: the candidate is reachable through normal
        // history and the pack's only remaining purpose was crash diagnosis.
        // An age-only floor still leaked a full-tree index plus pack per
        // cycle (~875 MiB on browser-extensions-shared; 9 GiB in 4 minutes
        // measured after 0.113.96 shipped the age-only sweep).
        //
        // ONLY a published commit releases the spool. A refusal, conflict or
        // deadline leaves it for diagnosis, and `git::epoch_spool`'s aged
        // reaper still bounds those.
        //
        // `pending` deliberately does NOT block the release: an unstaged
        // edit, untracked file or post-commit hook failure is follow-up work
        // for the NEXT epoch, which allocates its own fresh spool.
        match result {
            Ok(Ok(pending)) => {
                if published {
                    self.release_spool();
                }
                Ok(pending)
            }
            Ok(Err(error)) => {
                if published {
                    // The commit is on the ref; only its follow-up failed.
                    self.release_spool();
                    eprintln!(
                        "\u{26a0}\u{fe0f} {} epoch was published; follow-up remains unverified: {error:#}",
                        repo.display()
                    );
                    Ok(true)
                } else {
                    Err(error)
                }
            }
            Err(error) => {
                if published {
                    self.release_spool();
                    eprintln!(
                        "\u{26a0}\u{fe0f} {} epoch was published; deadline cancelled post-publication follow-up",
                        repo.display()
                    );
                    Ok(true)
                } else {
                    Err(error)
                        .context("staged epoch deadline expired; ref outcome may require inspection")
                }
            }
        }
    }
}

pub(super) async fn prepare_private(
    repo: &Path,
    policy: &SyncPolicy,
) -> Result<Option<crate::bucket_assets::Prepared>> {
    match crate::bucket_runner_bridge::prepare(repo, false, policy).await? {
        Some(prepared) => Ok(Some(prepared)),
        None => crate::bucket_assets::prepare(repo, false, policy.stage_op_timeout_secs).await,
    }
}

async fn verify_storage(
    repo: &Path,
    tree: &str,
    timeout_secs: u64,
    require_binding: bool,
) -> Result<bool> {
    // Parent owns the private directory, including cancellation cleanup.
    let workspace = tempfile::tempdir()?;
    let index = workspace.path().join("candidate.index");
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(["read-tree", tree])
        .env("GIT_INDEX_FILE", &index)
        .current_dir(repo);
    let result = crate::git::run_command_captured_output(
        command.into_tokio(),
        timeout_secs,
        "epoch private view",
        None,
    )
    .await?;
    ensure!(
        result.status.success(),
        "cannot construct private storage view"
    );
    let executable = std::env::current_exe()?;
    #[cfg(test)]
    let executable = {
        let _ = executable;
        PathBuf::from(
            std::env::var_os("DRACON_EPOCH_TEST_HELPER")
                .context("matching revision-qualified epoch test helper is required")?,
        )
    };
    ensure!(
        executable.is_absolute(),
        "epoch helper executable must be absolute"
    );
    let mut command = tokio::process::Command::new(executable);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("TOKIO_WORKER_THREADS", "2")
        .arg("storage")
        .arg("verify-configured-tree")
        .arg("--repo")
        .arg(repo)
        .arg("--tree")
        .arg(tree)
        .arg("--index")
        .arg(&index)
        .current_dir(repo);
    if require_binding {
        command.arg("--require-binding");
    }
    let result = crate::git::run_command_captured_output(
        command,
        timeout_secs,
        "epoch storage verifier",
        None,
    )
    .await?;
    ensure!(
        result.status.success(),
        "immutable storage verifier refused: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bound = format!("{} {} bound\n", env!("DRACON_EPOCH_SOURCE_ID"), tree);
    let ordinary = format!("{} {} ordinary\n", env!("DRACON_EPOCH_SOURCE_ID"), tree);
    ensure!(
        result.stdout == bound.as_bytes()
            || (!require_binding && result.stdout == ordinary.as_bytes()),
        "immutable storage verifier executable/source identity or binding mismatch"
    );
    Ok(result.stdout == bound.as_bytes())
}

/// All Git mutations on this view are child-scoped. This runner never changes
/// the daemon's environment or substitutes an alternate index globally.
pub(super) async fn run_index_git(
    repo: &Path,
    args: &[&str],
    timeout_secs: u64,
    label: &str,
    index: Option<&Path>,
) -> Result<()> {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .args(args)
        .current_dir(repo)
        .env("GIT_NO_LAZY_FETCH", "1");
    if let Some(index) = index {
        ensure!(index.is_absolute(), "private index must be absolute");
        command.env("GIT_INDEX_FILE", index);
    }
    let output =
        crate::git::run_command_captured_output(command.into_tokio(), timeout_secs, label, None)
            .await?;
    ensure!(
        output.status.success(),
        "{label} refused: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

/// A retained private staging view. Refusals keep its index and object pack for
/// diagnosis; successful installation removes only this exclusively owned dir.
pub(super) struct Staging {
    baseline: Epoch,
    directory: PathBuf,
    index: PathBuf,
    admitted_large: Vec<PathBuf>,
}
impl Staging {
    pub(super) async fn new(repo: &Path, baseline: Epoch) -> Result<Self> {
        ensure!(
            !baseline.has_intent(),
            "ordinary staging cannot replace staged intent"
        );
        let directory = git(repo, &["rev-parse", "--absolute-git-dir"], None).await?;
        let directory =
            PathBuf::from(std::str::from_utf8(&directory.stdout)?.trim_end_matches('\n'));
        let mut builder = tempfile::Builder::new();
        builder.prefix("dracon-staging-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = builder.tempdir_in(directory)?.keep();
        let index = directory.join("index");
        let staging = Self {
            baseline,
            directory,
            index,
            admitted_large: Vec::new(),
        };
        let deadline = staging.baseline.deadline;
        tokio::time::timeout_at(deadline, async {
            let tree = staging
                .baseline
                .snapshot
                .materialize_tree(repo, deadline)
                .await?;
            run_index_git(
                repo,
                &["read-tree", &tree],
                3600,
                "initialize private staging",
                Some(&staging.index),
            )
            .await
        })
        .await
        .context("private staging initialization deadline")??;
        Ok(staging)
    }
    pub(super) fn index(&self) -> &Path {
        &self.index
    }

    /// Caller has already excluded private assets and auto-stage opt-outs.
    pub(super) fn with_large_candidates(mut self, entries: &[dracon_git::types::DiffFile]) -> Self {
        self.admitted_large = entries
            .iter()
            .filter(|entry| matches!(entry.status, dracon_git::types::FileStatus::Added))
            .map(|entry| entry.path.clone())
            .collect();
        self
    }

    pub(super) async fn finish(
        self,
        repo: &Path,
        policy: &SyncPolicy,
        excluded: &BTreeSet<String>,
        cleanup: bool,
        dry_run: bool,
    ) -> Result<Epoch> {
        if dry_run {
            return Ok(self.baseline);
        }
        let deadline = self.baseline.deadline;
        let retained = self.directory.clone();
        tokio::time::timeout_at(deadline, async {
            // Hygiene is ordinary-cycle only: a captured entry epoch must never
            // re-add, delete or widen operator staging.
            let plan = super::hygiene::plan(
                repo,
                policy,
                excluded,
                &self.baseline.snapshot,
                cleanup,
                &self.admitted_large,
                deadline,
            )
            .await?;
            if let Some(plan) = &plan {
                // Index-side work happens FIRST: writing ignore rules before
                // staging would make an ordinary `git add` refuse the very
                // paths we still need to stage. Canonical ignore bytes are
                // written by the committed epoch after publication.
                super::hygiene::deindex(repo, &self.index, plan, deadline).await?;
            }
            let mut command = crate::policy::tokio_git_command();
            command
                .args(["write-tree"])
                .current_dir(repo)
                .env("GIT_INDEX_FILE", &self.index);
            let output = crate::git::run_command_captured_output(
                command.into_tokio(),
                3600,
                "private staged tree",
                None,
            )
            .await?;
            ensure!(
                output.status.success(),
                "cannot close private staging epoch"
            );
            let tree = std::str::from_utf8(&output.stdout)?.trim();
            ensure!(
                crate::git::is_valid_object_id(tree),
                "invalid private staged tree"
            );
            // A durable pack preserves candidate payloads even if an external
            // prune runs after refusal. This is not a ref/publication waiver.
            let prefix = self.directory.join("retained");
            let mut pack_input = format!("{tree}\n");
            if let CapturedHead::Commit(parent) = &self.baseline.snapshot.head {
                // The no-rewrite parent remains reachable in normal history.
                // Retain only the candidate's new objects, not another copy of
                // every unchanged historical asset on every staging pass.
                pack_input.push_str(&format!("^{parent}\n"));
            }
            git(
                repo,
                &[
                    "pack-objects",
                    "--revs",
                    "--window=0",
                    "--depth=0",
                    prefix.to_str().context("unsupported retention path")?,
                ],
                Some(pack_input.as_bytes()),
            )
            .await?;
            for entry in std::fs::read_dir(&self.directory)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|ext| ext == "pack" || ext == "idx")
                {
                    std::fs::File::open(path)?.sync_all()?;
                }
            }
            std::fs::File::open(&self.directory)?.sync_all()?;
            let entries = git(repo, &["ls-tree", "-r", "-z", "--full-tree", tree], None).await?;
            let mut candidate = std::collections::BTreeMap::new();
            for record in entries.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
                let tab = record
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("invalid candidate path")?;
                let fields: Vec<_> = std::str::from_utf8(&record[..tab])?.split(' ').collect();
                ensure!(
                    fields.len() == 3 && crate::git::is_valid_object_id(fields[2]),
                    "invalid candidate identity"
                );
                candidate.insert(
                    record[tab + 1..].to_vec(),
                    crate::git::staged_intent::CachedIdentity {
                        mode: u32::from_str_radix(fields[0], 8)?,
                        oid: fields[2].to_owned(),
                    },
                );
            }
            let mut changed = std::collections::BTreeMap::new();
            for (path, entry) in &candidate {
                if self.baseline.snapshot.entries.get(path) != Some(entry) {
                    changed.insert(path.clone(), Some(entry.clone()));
                }
            }
            for path in self.baseline.snapshot.entries.keys() {
                if !candidate.contains_key(path) {
                    changed.insert(path.clone(), None);
                }
            }
            let expected = StagedIntentSnapshot {
                head: self.baseline.snapshot.head.clone(),
                entries: candidate,
                changed,
            };
            let mut epoch = Epoch {
                snapshot: expected,
                reference: self.baseline.reference.clone(),
                deadline,
                retained: Some(self.directory.clone()),
                spool_lease: crate::git::SpoolLease::new(Some(self.directory.clone())),
                deletions: plan
                    .as_ref()
                    .map(|plan| plan.deleted.clone())
                    .unwrap_or_default(),
                ignore_patterns: plan
                    .as_ref()
                    .map(|plan| plan.patterns.clone())
                    .unwrap_or_default(),
            };
            // Only this epoch's own verified artifact de-index survives the
            // exclusion gate; every other excluded path is still refused.
            let rejected = epoch.ineligible_paths(repo, policy, excluded).await?;
            for path in rejected {
                // This view began without staged intent. Restore only our
                // candidate to its baseline; never reset the actual index or
                // touch the worktree. The original candidate pack is retained.
                if let Some(original) = self.baseline.snapshot.entries.get(&path) {
                    epoch
                        .snapshot
                        .entries
                        .insert(path.clone(), original.clone());
                } else {
                    epoch.snapshot.entries.remove(&path);
                }
                epoch.snapshot.changed.remove(&path);
            }
            epoch.eligible(repo, policy, excluded).await?;
            let selected = epoch.snapshot.materialize_tree(repo, deadline).await?;
            run_index_git(
                repo,
                &["read-tree", &selected],
                3600,
                "select eligible private candidate",
                Some(&self.index),
            )
            .await?;
            // No candidate delta means there is NOTHING to install or commit.
            // Requiring the actual index lock here starves already committed
            // publication behind an unrelated editor/Git index owner. Return
            // only the empty epoch; foreign actual staging remains untouched.
            if !epoch.has_intent() {
                return Ok(epoch);
            }
            // Persist typed artifact intent before installing an index that a
            // commit actor can publish. Later HEAD no longer contains these
            // paths, so rediscovery is not a durable retry strategy.
            #[cfg(target_os = "linux")]
            super::hygiene::persist(
                repo,
                &self.baseline.snapshot,
                &epoch.snapshot,
                &selected,
                &epoch.deletions,
            )?;
            #[cfg(not(target_os = "linux"))]
            ensure!(
                epoch.deletions.is_empty(),
                "durable artifact reconciliation unsupported; candidate retained"
            );
            let actual_index = git(
                repo,
                &["rev-parse", "--path-format=absolute", "--git-path", "index"],
                None,
            )
            .await?;
            let actual_index =
                PathBuf::from(std::str::from_utf8(&actual_index.stdout)?.trim_end_matches('\n'));
            let mut lock = acquire_epoch_lock(actual_index.with_extension("lock"))?;
            self.baseline.unchanged(repo, &lock).await?;
            lock.install_index(repo, &self.index, &actual_index, deadline).await?;
            // The returned identities are the proven private candidate, not
            // a read-after-install observation of a concurrently replaced index.
            // Commit's unchanged check must reject any later delta intact.
            drop(lock);
            // Do not erase the independent candidate pack merely because
            // install succeeded. A later foreign replacement can remove its
            // only ordinary index root before this epoch commits.
            Ok(epoch)
        })
        .await
        .context("private staging deadline expired")?
        .with_context(|| format!("private staging retained at {}", retained.display()))
    }
}

pub(super) fn path_from_git_bytes(raw: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec())))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(std::str::from_utf8(raw)?))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    async fn fixture() -> (tempfile::TempDir, SyncPolicy) {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"], None)
            .await
            .unwrap();
        std::fs::write(root.path().join("file"), b"base\n").unwrap();
        git(root.path(), &["add", "--", "file"], None)
            .await
            .unwrap();
        git(
            root.path(),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "user.name=Epoch Fixture",
                "-c",
                "user.email=epoch@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
            None,
        )
        .await
        .unwrap();
        let policy = SyncPolicy {
            max_stage_batch_files: 10,
            max_stage_file_bytes: 1024,
            stage_op_timeout_secs: 60,
            ..SyncPolicy::default()
        };
        (root, policy)
    }

    async fn private_candidate(repo: &Path) -> (Staging, String) {
        let baseline = Epoch::capture(repo, 60).await.unwrap();
        let staging = Staging::new(repo, baseline).await.unwrap();
        std::fs::write(repo.join("file"), b"private candidate\n").unwrap();
        run_index_git(
            repo,
            &["add", "--", "file"],
            60,
            "fixture private add",
            Some(staging.index()),
        )
        .await
        .unwrap();
        let oid = git(
            repo,
            &["hash-object", "--stdin"],
            Some(b"private candidate\n"),
        )
        .await
        .unwrap();
        (
            staging,
            String::from_utf8(oid.stdout).unwrap().trim().to_owned(),
        )
    }

    async fn refused_install_preserves_actual_and_pack(child: bool) {
        let (root, policy) = fixture().await;
        let repo = root.path();
        let (staging, candidate_oid) = private_candidate(repo).await;
        let retained = staging.directory.clone();
        let foreign = if child {
            "directory/late-child"
        } else {
            "file"
        };
        if child {
            std::fs::create_dir(repo.join("directory")).unwrap();
        }
        std::fs::write(repo.join(foreign), b"foreign staged\n").unwrap();
        git(repo, &["add", "--", foreign], None).await.unwrap();
        std::fs::write(repo.join(foreign), b"foreign unstaged\n").unwrap();
        let actual_index = std::fs::read(repo.join(".git/index")).unwrap();
        let actual = Epoch::capture(repo, 60).await.unwrap();
        let error = staging
            .finish(repo, &policy, &BTreeSet::new(), false, false)
            .await
            .err()
            .unwrap();
        assert!(
            format!("{error:#}").contains("index or parent changed"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read(repo.join(".git/index")).unwrap(),
            actual_index
        );
        assert_eq!(
            std::fs::read(repo.join(foreign)).unwrap(),
            b"foreign unstaged\n"
        );
        assert_eq!(
            Epoch::capture(repo, 60).await.unwrap().snapshot,
            actual.snapshot
        );
        assert!(retained.join("index").is_file());
        let pack_index = std::fs::read_dir(&retained)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "idx"))
            .expect("durable candidate pack index");
        let contents = git(
            repo,
            &["verify-pack", "-v", pack_index.to_str().unwrap()],
            None,
        )
        .await
        .unwrap();
        assert!(
            String::from_utf8(contents.stdout)
                .unwrap()
                .contains(&candidate_oid),
            "candidate payload must survive outside Git's loose-object GC reachability"
        );
        assert!(!repo.join(".git/index.lock").exists());
    }

    #[tokio::test]
    async fn empty_candidate_does_not_install_or_require_foreign_index_admission() {
        let (root, policy) = fixture().await;
        let repo = root.path();
        let baseline = Epoch::capture(repo, 60).await.unwrap();
        let staging = Staging::new(repo, baseline).await.unwrap();
        std::fs::write(repo.join("file"), b"foreign staged bytes\n").unwrap();
        git(repo, &["add", "--", "file"], None).await.unwrap();
        std::fs::write(repo.join("file"), b"foreign unstaged bytes\n").unwrap();
        let foreign_index = std::fs::read(repo.join(".git/index")).unwrap();
        std::fs::write(repo.join(".git/index.lock"), b"foreign lock owner").unwrap();
        let epoch = staging
            .finish(repo, &policy, &BTreeSet::new(), false, false)
            .await
            .unwrap();
        assert!(!epoch.has_intent(), "foreign staging must not be absorbed");
        assert_eq!(
            std::fs::read(repo.join(".git/index")).unwrap(),
            foreign_index
        );
        assert_eq!(
            std::fs::read(repo.join(".git/index.lock")).unwrap(),
            b"foreign lock owner"
        );
        assert_eq!(
            std::fs::read(repo.join("file")).unwrap(),
            b"foreign unstaged bytes\n"
        );
    }

    #[tokio::test]
    async fn ordinary_size_rejection_prunes_only_private_delta_but_refuses_captured_intent() {
        let (root, policy) = fixture().await;
        let repo = root.path();
        let baseline = Epoch::capture(repo, 60).await.unwrap();
        let staging = Staging::new(repo, baseline).await.unwrap();
        std::fs::write(repo.join("file"), b"eligible change\n").unwrap();
        let large = vec![b'x'; 2048];
        std::fs::write(repo.join("large"), &large).unwrap();
        run_index_git(
            repo,
            &["add", "--", "file", "large"],
            60,
            "fixture owned candidate",
            Some(staging.index()),
        )
        .await
        .unwrap();
        let retained = staging.directory.clone();
        let epoch = staging
            .finish(repo, &policy, &BTreeSet::new(), false, false)
            .await
            .unwrap();
        assert_eq!(epoch.snapshot.changed.len(), 1);
        assert!(epoch.snapshot.changed.contains_key(b"file".as_slice()));
        assert!(!epoch.snapshot.entries.contains_key(b"large".as_slice()));
        assert_eq!(std::fs::read(repo.join("large")).unwrap(), large);
        assert_eq!(
            std::fs::read(repo.join("file")).unwrap(),
            b"eligible change\n"
        );
        let observed = Epoch::capture(repo, 60).await.unwrap();
        assert_eq!(observed.snapshot, epoch.snapshot);
        let large_oid = String::from_utf8(
            git(repo, &["hash-object", "--stdin"], Some(&large))
                .await
                .unwrap()
                .stdout,
        )
        .unwrap();
        let pack_index = std::fs::read_dir(retained)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "idx"))
            .expect("original candidate retained independently of the selected index");
        let packed = git(
            repo,
            &["verify-pack", "-v", pack_index.to_str().unwrap()],
            None,
        )
        .await
        .unwrap()
        .stdout;
        assert!(
            String::from_utf8(packed)
                .unwrap()
                .contains(large_oid.trim()),
            "even the rejected candidate blob must survive independently of loose-object pruning"
        );

        // A user's existing oversized staging is not OUR candidate to prune.
        git(repo, &["add", "--", "large"], None).await.unwrap();
        let bytes = std::fs::read(repo.join(".git/index")).unwrap();
        let captured = Epoch::capture(repo, 60).await.unwrap();
        assert!(captured
            .eligible(repo, &policy, &BTreeSet::new())
            .await
            .is_err());
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), bytes);
        assert_eq!(std::fs::read(repo.join("large")).unwrap(), large);
    }

    #[tokio::test]
    async fn zero_batch_limit_accepts_all_captured_files_without_relaxing_positive_limit() {
        let (root, mut policy) = fixture().await;
        let names: Vec<_> = (0..120)
            .map(|number| format!("related-{number:03}"))
            .collect();
        for name in &names {
            std::fs::write(root.path().join(name), b"related\n").unwrap();
        }
        let mut args = vec!["add", "--"];
        args.extend(names.iter().map(String::as_str));
        git(root.path(), &args, None).await.unwrap();
        let epoch = Epoch::capture(root.path(), 30).await.unwrap();
        assert_eq!(epoch.snapshot.changed.len(), 120);
        policy.max_stage_batch_files = 100;
        assert!(epoch
            .eligible(root.path(), &policy, &BTreeSet::new())
            .await
            .is_err());
        policy.max_stage_batch_files = 0;
        assert_eq!(policy.stage_batch_limit(), usize::MAX);
        epoch
            .eligible(root.path(), &policy, &BTreeSet::new())
            .await
            .unwrap();
        policy.max_stage_file_bytes = 1;
        assert!(
            epoch
                .eligible(root.path(), &policy, &BTreeSet::new())
                .await
                .is_err(),
            "unlimited file count must still enforce blob size"
        );
        policy.max_stage_file_bytes = 1024;
        policy.auto_commit_exclude_patterns = vec!["related-000".into()];
        assert!(
            epoch
                .eligible(root.path(), &policy, &BTreeSet::new())
                .await
                .is_err(),
            "unlimited file count must still enforce exclusions"
        );
        assert_eq!(
            git(
                root.path(),
                &["diff", "--cached", "--name-only", "-z"],
                None
            )
            .await
            .unwrap()
            .stdout
            .iter()
            .filter(|byte| **byte == 0)
            .count(),
            120
        );
    }

    #[test]
    fn lock_contention_is_deferred_but_filesystem_failure_is_not() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("index.lock");
        let owner = acquire_epoch_lock(path.clone()).unwrap();
        let busy = acquire_epoch_lock(path.clone()).err().expect("held lock");
        assert!(is_conflict(&busy));
        let failed = acquire_epoch_lock(root.path().join("missing/index.lock"))
            .err()
            .expect("missing parent must be an I/O failure");
        assert!(
            !is_conflict(&failed),
            "real I/O failure cannot be hidden as contention"
        );
        drop(owner);
        drop(acquire_epoch_lock(path).expect("owner cleanup permits retry"));
    }

    #[test]
    fn only_typed_intent_conflicts_are_nonfailure_deferrals() {
        let changed =
            anyhow::Error::new(EpochConflict::IndexChanged).context("retained candidate fixture");
        assert!(is_conflict(&changed));
        assert!(!is_conflict(&anyhow::anyhow!(
            "epoch index or parent changed"
        )));
        assert!(!is_conflict(&anyhow::anyhow!(
            "permission denied reading index"
        )));
    }

    #[tokio::test]
    async fn private_staging_same_path_conflict_retains_both_payloads() {
        refused_install_preserves_actual_and_pack(false).await;
    }

    #[tokio::test]
    async fn private_staging_foreign_child_is_not_a_prefix_waiver() {
        refused_install_preserves_actual_and_pack(true).await;
    }

    #[tokio::test]
    async fn published_epoch_deadline_is_pending_not_a_failed_commit() {
        use std::os::unix::fs::PermissionsExt;
        let (root, mut policy) = fixture().await;
        let repo = root.path();
        policy.auto_commit = true;
        policy.watch_roots = vec![repo.to_string_lossy().into_owned()];
        let hooks = repo.join(".git/epoch-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let post = hooks.join("post-commit");
        std::fs::write(&post, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&post, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(
            repo,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
            None,
        )
        .await
        .unwrap();
        std::fs::write(repo.join("file"), b"published before follow-up\n").unwrap();
        git(repo, &["add", "--", "file"], None).await.unwrap();
        let epoch = Epoch::capture(repo, 10).await.unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            epoch.commit(
                repo,
                "fixture bounded publication",
                &policy,
                &BTreeSet::new(),
                None,
            ),
        )
        .await
        .expect("absolute epoch deadline must terminate post hook");
        assert!(
            matches!(result, Ok(true)),
            "known publication must remain pending, not failure: {result:?}"
        );
        assert_eq!(
            git(repo, &["show", "HEAD:file"], None)
                .await
                .unwrap()
                .stdout,
            b"published before follow-up\n"
        );
        assert_eq!(
            std::fs::read(repo.join("file")).unwrap(),
            b"published before follow-up\n"
        );
        assert!(!repo.join(".git/index.lock").exists());
    }

    #[tokio::test]
    async fn installed_epoch_keeps_proven_identity_and_rejects_late_replacement() {
        let (root, policy) = fixture().await;
        let repo = root.path();
        let (staging, expected_oid) = private_candidate(repo).await;
        let epoch = staging
            .finish(repo, &policy, &BTreeSet::new(), false, false)
            .await
            .unwrap();
        assert_eq!(epoch.snapshot.entries[b"file".as_slice()].oid, expected_oid);
        let retained = epoch
            .retained
            .as_ref()
            .expect("installed candidate retention");
        assert!(retained.join("index").is_file());
        std::fs::write(repo.join("file"), b"late foreign replacement\n").unwrap();
        git(repo, &["add", "--", "file"], None).await.unwrap();
        std::fs::create_dir(repo.join("directory")).unwrap();
        std::fs::write(repo.join("directory/late-child"), b"late child\n").unwrap();
        git(repo, &["add", "--", "directory/late-child"], None)
            .await
            .unwrap();
        let bytes = std::fs::read(repo.join(".git/index")).unwrap();
        let lock = crate::git::IndexLock::acquire(repo).unwrap();
        assert!(epoch.unchanged(repo, &lock).await.is_err());
        assert_eq!(epoch.snapshot.entries[b"file".as_slice()].oid, expected_oid);
        assert!(!epoch
            .snapshot
            .entries
            .contains_key(b"directory/late-child".as_slice()));
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), bytes);
        let pack_index = std::fs::read_dir(retained)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "idx"))
            .expect("candidate pack survives late replacement");
        let objects = git(
            repo,
            &["verify-pack", "-v", pack_index.to_str().unwrap()],
            None,
        )
        .await
        .unwrap();
        assert!(String::from_utf8(objects.stdout)
            .unwrap()
            .contains(&expected_oid));
        assert_eq!(
            std::fs::read(repo.join("file")).unwrap(),
            b"late foreign replacement\n"
        );
        assert_eq!(
            std::fs::read(repo.join("directory/late-child")).unwrap(),
            b"late child\n"
        );
    }
}
