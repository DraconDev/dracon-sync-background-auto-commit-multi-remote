//! Immutable commit epochs. Neither refusal nor publication installs an index.
use super::*;
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use anyhow::ensure;
use tokio::time::Instant;

pub(super) struct Epoch {
    snapshot: StagedIntentSnapshot,
    reference: Option<String>,
    deadline: Instant,
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
            })
        })
        .await
        .context("epoch capture deadline expired")?
    }

    pub(super) fn has_intent(&self) -> bool {
        !self.snapshot.changed.is_empty()
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
        ensure!(
            lock.is_owned(),
            "epoch index lock was replaced; retained foreign lock"
        );
        ensure!(
            self.reference == reference(repo).await?,
            "epoch branch changed"
        );
        ensure!(
            self.snapshot == StagedIntentSnapshot::capture(repo, self.deadline).await?,
            "epoch index or parent changed; all staged/worktree data retained"
        );
        Ok(())
    }

    async fn eligible(
        &self,
        repo: &Path,
        policy: &SyncPolicy,
        excluded: &BTreeSet<String>,
    ) -> Result<()> {
        ensure!(
            self.snapshot.changed.len() <= policy.max_stage_batch_files,
            "captured staged intent exceeds strict batch limit"
        );
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
        for ((raw, entry), path) in self.snapshot.changed.iter().zip(self.entries()?) {
            let size = measured.get(raw).copied().unwrap_or(0);
            ensure!(
                crate::exclude::should_stage_entry_with_blob_size(
                    repo,
                    &path,
                    size,
                    excluded,
                    &policy.exclude_file_patterns,
                    policy.max_stage_file_bytes,
                    &excludes
                ),
                "captured staged path is excluded or oversized: {}",
                path.path.display()
            );
            // Preserve the existing untracked opt-outs, without interpreting a
            // worktree rewrite as the content of an already-staged blob.
            if matches!(self.snapshot.head, CapturedHead::Unborn { .. }) && entry.is_some() {
                ensure!(
                    !crate::exclude::matches_untracked_exclude(
                        repo,
                        &path.path,
                        &policy.untracked_exclude_patterns
                    ),
                    "captured root path excluded by untracked policy"
                );
            }
        }
        Ok(())
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
            let required_binding = verify_storage(repo, &tree, policy.stage_op_timeout_secs, false).await?;
            let approved = run_sealed_commit_hooks(repo, message, policy.stage_op_timeout_secs).await?;
            self.unchanged(repo, &lock).await?;
            if let Some(prepared) = prepared { prepared.verify_staged(repo, policy.stage_op_timeout_secs).await?; }
            crate::bucket_guard::check_staged_forward_only(repo, policy.stage_op_timeout_secs,
                std::env::var("DRACON_BUCKET_GUARD").ok().as_deref()).await?;
            verify_storage(repo, &tree, policy.stage_op_timeout_secs, required_binding).await?;
            self.eligible(repo, policy, excluded).await?;
            self.unchanged(repo, &lock).await?;
            ensure!(commit_allowed_by_ownership(repo, policy), "epoch ownership changed");
            let mut args = vec!["commit-tree", tree.as_str()];
            let parent = match &self.snapshot.head { CapturedHead::Commit(parent) => Some(parent.as_str()), _ => None };
            if let Some(parent) = parent { args.extend(["-p", parent]); }
            // commit-tree doesn't honor commit.gpgSign by itself.
            let mut config = crate::policy::tokio_git_command();
            config.args(["config", "--bool", "--get", "commit.gpgSign"]).current_dir(repo);
            let signing = crate::git::run_command_captured_output(config.into_tokio(), policy.stage_op_timeout_secs, "epoch signing config", None).await?;
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
            let lock_retained = !lock.is_owned();
            drop(lock);
            // Native post-commit runs after publication and does not control its
            // success. A failing hook remains diagnostic and pending follow-up.
            let post_failed = if let Err(error) = run_configured_hook(repo, "post-commit", &[], policy.stage_op_timeout_secs).await {
                eprintln!("⚠️ {} post-commit failed AFTER epoch publication: {error:#}", repo.display());
                true
            } else { false };
            let after = StagedIntentSnapshot::capture(repo, deadline).await?;
            let child = crate::git::spawn_git_command_cancellable_stdin(repo,
                &["diff", "--quiet", "HEAD", "--"], "epoch follow-up")?;
            let (status, _, _) = crate::git::run_git_captured_output(child, repo, "epoch follow-up").await?;
            ensure!(status.success() || status.code() == Some(1), "cannot inspect post-publication work");
            Ok(lock_retained || post_failed || !after.changed.is_empty() || !status.success())
        }).await.context("staged epoch deadline expired; publication may already have occurred, inspect HEAD")?
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
        let directory = tempfile::Builder::new()
            .prefix("dracon-staging-")
            .tempdir_in(directory)?
            .keep();
        let index = directory.join("index");
        let staging = Self {
            baseline,
            directory,
            index,
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
            // Cleanup routing is deliberately not guessed from filenames here:
            // its .gitignore/worktree authority must be reconciled separately.
            if cleanup && self.baseline.snapshot.entries.keys().any(|raw| {
                path_from_git_bytes(raw).is_ok_and(|path| crate::exclude::is_excluded_change_path(&path, excluded))
            }) {
                anyhow::bail!("build-artifact cleanup requires isolated .gitignore reconciliation; private staging retained at {}", self.directory.display());
            }
            let mut command = crate::policy::tokio_git_command();
            command.args(["write-tree"]).current_dir(repo).env("GIT_INDEX_FILE", &self.index);
            let output = crate::git::run_command_captured_output(command.into_tokio(), 3600, "private staged tree", None).await?;
            ensure!(output.status.success(), "cannot close private staging epoch");
            let tree = std::str::from_utf8(&output.stdout)?.trim();
            ensure!(crate::git::is_valid_object_id(tree), "invalid private staged tree");
            // A durable pack preserves candidate payloads even if an external
            // prune runs after refusal. This is not a ref/publication waiver.
            let prefix = self.directory.join("retained");
            git(repo, &["pack-objects", "--revs", "--window=0", "--depth=0", prefix.to_str().context("unsupported retention path")?], Some(format!("{tree}\n").as_bytes())).await?;
            let entries = git(repo, &["ls-tree", "-r", "-z", "--full-tree", tree], None).await?;
            let mut candidate = std::collections::BTreeMap::new();
            for record in entries.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
                let tab = record.iter().position(|b| *b == b'\t').context("invalid candidate path")?;
                let fields: Vec<_> = std::str::from_utf8(&record[..tab])?.split(' ').collect();
                ensure!(fields.len() == 3 && crate::git::is_valid_object_id(fields[2]), "invalid candidate identity");
                candidate.insert(record[tab+1..].to_vec(), crate::git::staged_intent::CachedIdentity {
                    mode: u32::from_str_radix(fields[0], 8)?, oid: fields[2].to_owned(),
                });
            }
            let mut changed = std::collections::BTreeMap::new();
            for (path, entry) in &candidate {
                if self.baseline.snapshot.entries.get(path) != Some(entry) { changed.insert(path.clone(), Some(entry.clone())); }
            }
            for path in self.baseline.snapshot.entries.keys() {
                if !candidate.contains_key(path) { changed.insert(path.clone(), None); }
            }
            let expected = StagedIntentSnapshot { head: self.baseline.snapshot.head.clone(), entries: candidate, changed };
            let epoch = Epoch { snapshot: expected, reference: self.baseline.reference.clone(), deadline };
            epoch.eligible(repo, policy, excluded).await?;
            let actual_index = git(repo, &["rev-parse", "--path-format=absolute", "--git-path", "index"], None).await?;
            let actual_index = PathBuf::from(std::str::from_utf8(&actual_index.stdout)?.trim_end_matches('\n'));
            let mut lock = crate::git::IndexLock::acquire_path(actual_index.with_extension("lock"), repo).map_err(anyhow::Error::msg)?;
            self.baseline.unchanged(repo, &lock).await?;
            lock.install_index(&self.index, &actual_index)?;
            // The returned identities are the proven private candidate, not
            // a read-after-install observation of a concurrently replaced index.
            // Commit's unchanged check must reject any later delta intact.
            drop(lock);
            std::fs::remove_dir_all(&self.directory)?;
            Ok(epoch)
        }).await.context("private staging deadline expired")?
            .with_context(|| format!("private staging retained at {}", retained.display()))
    }
}

fn path_from_git_bytes(raw: &[u8]) -> Result<PathBuf> {
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
    async fn private_staging_same_path_conflict_retains_both_payloads() {
        refused_install_preserves_actual_and_pack(false).await;
    }

    #[tokio::test]
    async fn private_staging_foreign_child_is_not_a_prefix_waiver() {
        refused_install_preserves_actual_and_pack(true).await;
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
