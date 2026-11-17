//! Private, version-bound custody records for forward enrollment review.
//!
//! This does not approve enrollment, install a filter, contact a backend, stage
//! files, or change history. A later executor must validate policy, copies,
//! source versions and these repository/index anchors before any transition.

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;

use super::{capture, git_custody, journal, reference::Fingerprint};

const MAX_PATHS: usize = 10_000;
const MAX_PATH_BYTES: usize = 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_REVIEW_BYTES: usize = 16 * 1024 * 1024;

/// Finite bounds for a private review, including previously retained reviews.
#[derive(Clone, Copy)]
pub struct ReviewLimits {
    /// Largest one-version source accepted by the review.
    pub max_blob_bytes: u64,
    /// Maximum unique copied payload bytes in this review.
    pub max_total_bytes: u64,
    /// Maximum bytes already retained plus this review's payloads/metadata.
    pub max_retained_bytes: u64,
    /// One deadline for the whole capture; not a fresh budget per asset.
    pub timeout: Duration,
}

/// Explicit operator-selected input. Repository files cannot select authority.
pub struct ReviewRequest<'a> {
    /// Canonical owning worktree, never a parent/nested repository guess.
    pub repo: &'a Path,
    /// Proposed or existing stable native storage repository ID.
    pub repo_id: &'a str,
    /// Exact repository-relative sources chosen for review.
    pub paths: &'a [PathBuf],
    /// Private operator directory outside the selected worktree.
    pub custody_root: &'a Path,
    /// Absolute operator-selected Git executable.
    pub git: &'a Path,
    /// Bound memory, disk and execution before any payload copy.
    pub limits: ReviewLimits,
}

/// Public summary without raw source fingerprints or source-path inventory.
#[derive(Debug, Serialize)]
pub struct PreparedReview {
    /// Private manifest location; its contents are not suitable for Git/logging.
    pub manifest_path: PathBuf,
    /// Digest used to bind a later explicit review/approval to these exact bytes.
    pub review_sha256: String,
    /// Number of selected logical paths.
    pub assets: usize,
    /// Number of independently verified preserved content versions.
    pub preserved_versions: usize,
    /// Preserved payload bytes; index/review metadata is accounted separately.
    pub preserved_bytes: u64,
    /// Retained index snapshot and review manifest bytes.
    pub metadata_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Head {
    reference: String,
    oid: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    file: String,
    fingerprint: Fingerprint,
    git_oid: Option<String>,
    mode: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    path_hex: String,
    head: Option<Version>,
    index: Option<Version>,
    worktree: Version,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    schema_version: u32,
    repo_id: String,
    worktree: PathBuf,
    git_directory: PathBuf,
    git_device: u64,
    git_inode: u64,
    head: Option<Head>,
    index: Option<Fingerprint>,
    index_copy: Option<String>,
    assets: Vec<Asset>,
    grants_enrollment: bool,
}

struct Selected {
    path: PathBuf,
    head: Option<(String, u32)>,
    index: Option<(String, u32)>,
}

fn private_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

fn fingerprint_file(
    file: &mut File,
    max: u64,
    mut output: Option<&mut File>,
) -> Result<Fingerprint> {
    file.rewind()?;
    let mut sha = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .context("review byte overflow")?;
        ensure!(bytes <= max, "review byte limit exceeded");
        if let Some(target) = &mut output {
            target.write_all(&buffer[..count])?;
        }
        sha.update(&buffer[..count]);
    }
    file.rewind()?;
    Fingerprint::new(format!("{:x}", sha.finalize()), bytes)
}

fn index_fingerprint(
    path: &Path,
    max: u64,
    copy: Option<&mut File>,
) -> Result<Option<Fingerprint>> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let before = file.metadata()?;
    ensure!(
        before.is_file()
            && before.uid() == unsafe { libc::geteuid() }
            && before.mode() & 0o022 == 0
            && before.len() <= max.min(MAX_INDEX_BYTES),
        "unsafe review index"
    );
    let fingerprint = fingerprint_file(&mut file, max.min(MAX_INDEX_BYTES), copy)?;
    let after = file.metadata()?;
    let named = std::fs::symlink_metadata(path)?;
    ensure!(
        !named.is_symlink() && stamp(&before) == stamp(&after) && stamp(&after) == stamp(&named),
        "index changed during review"
    );
    Ok(Some(fingerprint))
}

fn stamp(m: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

fn head(repo: &git2::Repository) -> Result<Option<Head>> {
    match repo.head() {
        Ok(reference) => Ok(Some(Head {
            reference: reference.name().context("non-UTF8 review ref")?.to_owned(),
            oid: reference.peel_to_commit()?.id().to_string(),
        })),
        Err(e)
            if matches!(
                e.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

fn retained_bytes(root: &Path, deadline: Instant) -> Result<u64> {
    let mut directories = vec![root.to_path_buf()];
    let mut bytes = 0u64;
    let mut entries = 0usize;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            ensure!(
                Instant::now() < deadline,
                "enrollment review deadline expired"
            );
            entries += 1;
            ensure!(entries <= 100_000, "review retention entry limit exceeded");
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            ensure!(
                !metadata.is_symlink()
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.mode() & 0o077 == 0,
                "unsafe retained review entry"
            );
            if metadata.is_dir() {
                directories.push(entry.path());
            } else {
                ensure!(
                    metadata.is_file() && metadata.nlink() == 1,
                    "unsupported retained review entry"
                );
                bytes = bytes
                    .checked_add(metadata.len())
                    .context("review retention overflow")?;
            }
        }
    }
    Ok(bytes)
}

/// Preserve HEAD, staged and working versions into a private review. Copies are
/// retained on refusal; only a complete, unchanged HEAD/index capture publishes
/// `review.json`. Nothing here authorizes replacing an existing tracked path.
pub async fn prepare_review(request: ReviewRequest<'_>) -> Result<PreparedReview> {
    ensure!(
        request.paths.len() <= MAX_PATHS
            && request
                .paths
                .iter()
                .map(|p| p.as_os_str().as_bytes().len())
                .sum::<usize>()
                <= MAX_PATH_BYTES,
        "review input exceeds bounds"
    );
    let deadline = Instant::now()
        .checked_add(request.limits.timeout)
        .context("invalid review deadline")?;
    let repo = request.repo.to_path_buf();
    let repo_id = request.repo_id.to_owned();
    let paths = request.paths.to_vec();
    let custody_root = request.custody_root.to_path_buf();
    let git = request.git.to_path_buf();
    let limits = request.limits;
    let runtime = tokio::runtime::Handle::current();
    // Disk hashing never blocks a scheduler executor. If the caller is
    // cancelled, this finite worker retains its budget lease until settlement.
    tokio::task::spawn_blocking(move || {
        runtime.block_on(prepare_inner(
            ReviewRequest {
                repo: &repo,
                repo_id: &repo_id,
                paths: &paths,
                custody_root: &custody_root,
                git: &git,
                limits,
            },
            deadline,
        ))
    })
    .await
    .context("enrollment review worker failed")?
}

async fn prepare_inner(request: ReviewRequest<'_>, deadline: Instant) -> Result<PreparedReview> {
    let ReviewRequest {
        repo,
        repo_id,
        paths,
        custody_root,
        git,
        limits,
    } = request;
    super::reference::validate_sha256(repo_id)?;
    ensure!(
        repo.is_absolute()
            && repo.canonicalize()? == repo
            && custody_root.is_absolute()
            && !custody_root.starts_with(repo),
        "explicit isolated review bindings required"
    );
    ensure!(
        !paths.is_empty()
            && paths.len() <= MAX_PATHS
            && limits.max_blob_bytes > 0
            && limits.max_total_bytes > 0
            && limits.max_retained_bytes > 0
            && !limits.timeout.is_zero(),
        "invalid review bounds"
    );
    ensure!(
        Instant::now() < deadline,
        "enrollment review deadline expired"
    );
    let mut seen = BTreeSet::new();
    let mut path_bytes = 0usize;
    for path in paths {
        journal::encode_relative_path(path.as_os_str().as_bytes())?;
        ensure!(seen.insert(path), "duplicate enrollment review path");
        path_bytes = path_bytes
            .checked_add(path.as_os_str().as_bytes().len())
            .context("review path overflow")?;
        ensure!(path_bytes <= MAX_PATH_BYTES, "review path limit exceeded");
    }
    journal::private_directory(custody_root, true)?;
    journal::runtime::protect(custody_root)?;
    let _budget = journal::try_lock(&custody_root.join("review-budget.lock"))?;
    let retained = retained_bytes(custody_root, deadline)?;
    ensure!(
        retained < limits.max_retained_bytes,
        "review retention capacity exceeded"
    );
    let directory = tempfile::Builder::new()
        .prefix("enrollment-review-")
        .tempdir_in(custody_root)?
        .keep();
    // Release all libgit2 borrows before asynchronous subprocess capture.
    let (git_directory, git_identity, original_head, original_index, selected) = {
        let repository = git2::Repository::open(repo)?;
        ensure!(
            repository
                .workdir()
                .context("bare review repository")?
                .canonicalize()?
                == repo,
            "review repository root mismatch"
        );
        match repository
            .config()?
            .open_level(git2::ConfigLevel::Local)?
            .get_string("dracon.storageRepoId")
        {
            Ok(id) => ensure!(id == repo_id, "review storage repository ID mismatch"),
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let git_directory = repository.path().canonicalize()?;
        let identity = std::fs::metadata(&git_directory)?;
        ensure!(
            identity.uid() == unsafe { libc::geteuid() } && identity.mode() & 0o022 == 0,
            "unsafe review Git directory"
        );
        let private_index_path = directory.join("index.before");
        let mut private_index = private_file(&private_index_path)?;
        let index_fingerprint = index_fingerprint(
            &git_directory.join("index"),
            limits.max_retained_bytes - retained,
            Some(&mut private_index),
        )?;
        private_index.sync_all()?;
        if let Some(expected) = &index_fingerprint {
            ensure!(
                fingerprint_file(&mut private_index, MAX_INDEX_BYTES, None)? == *expected,
                "review index readback mismatch"
            );
        }
        // Parse the captured index, not a second canonical-index read that
        // could observe an intervening replacement between anchor checks.
        let index = if index_fingerprint.is_some() {
            git2::Index::open(&private_index_path)?
        } else {
            git2::Index::new()?
        };
        ensure!(
            !index.has_conflicts(),
            "unmerged index blocks enrollment review"
        );
        let original_head = head(&repository)?;
        let tree = if original_head.is_some() {
            Some(repository.head()?.peel_to_tree()?)
        } else {
            None
        };
        let mut selected = Vec::new();
        for path in paths {
            let indexed = index.get_path(path, 0).map(|e| (e.id.to_string(), e.mode));
            let committed = match &tree {
                Some(tree) => match tree.get_path(path) {
                    Ok(entry) => Some((entry.id().to_string(), entry.filemode() as u32)),
                    Err(error) if error.code() == git2::ErrorCode::NotFound => None,
                    Err(error) => return Err(error.into()),
                },
                None => None,
            };
            ensure!(
                indexed
                    .iter()
                    .chain(committed.iter())
                    .all(|(_, mode)| matches!(mode, 0o100644 | 0o100755)),
                "non-regular Git entry blocks enrollment review"
            );
            selected.push(Selected {
                path: path.clone(),
                head: committed,
                index: indexed,
            });
        }
        (
            git_directory,
            (identity.dev(), identity.ino()),
            original_head,
            index_fingerprint,
            selected,
        )
    };
    let index_bytes = original_index.as_ref().map_or(0, Fingerprint::bytes);
    let capacity = limits
        .max_total_bytes
        .min(limits.max_retained_bytes - retained - index_bytes);
    let mut versions = BTreeMap::<String, Version>::new();
    let mut copied = 0u64;
    let mut assets = Vec::new();
    for selected in selected {
        let mut captured = Vec::new();
        for entry in [&selected.head, &selected.index] {
            let Some((oid, mode)) = entry else {
                captured.push(None);
                continue;
            };
            if !versions.contains_key(oid) {
                let file_name = format!("git-{oid}.blob");
                let mut file = private_file(&directory.join(&file_name))?;
                let remaining = capacity
                    .checked_sub(copied)
                    .context("review capacity exceeded")?;
                let fingerprint = git_custody::preserve_git_blob(
                    repo,
                    git,
                    oid,
                    &mut file,
                    limits.max_blob_bytes.min(remaining),
                    deadline.saturating_duration_since(Instant::now()),
                )
                .await?;
                copied = copied
                    .checked_add(fingerprint.bytes())
                    .context("review byte overflow")?;
                versions.insert(
                    oid.clone(),
                    Version {
                        file: file_name,
                        fingerprint,
                        git_oid: Some(oid.clone()),
                        mode: None,
                    },
                );
            }
            let mut version = versions[oid].clone();
            version.mode = Some(*mode);
            captured.push(Some(version));
        }
        ensure!(
            Instant::now() < deadline,
            "enrollment review deadline expired"
        );
        let (mut source, expected) =
            capture::select_source(repo, &selected.path, limits.max_blob_bytes)?;
        let metadata = source.metadata()?;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
            "unsafe enrollment source ownership"
        );
        let reused = versions
            .values()
            .find(|v| v.fingerprint == expected)
            .cloned();
        let worktree = match reused {
            Some(version) => Version {
                git_oid: None,
                mode: Some(0o100000 | metadata.mode() & 0o777),
                ..version
            },
            None => {
                ensure!(
                    expected.bytes() <= capacity.saturating_sub(copied),
                    "review capacity exceeded"
                );
                let file_name = format!("source-{}.blob", expected.sha256());
                let mut file = private_file(&directory.join(&file_name))?;
                let fingerprint =
                    fingerprint_file(&mut source, limits.max_blob_bytes, Some(&mut file))?;
                ensure!(
                    fingerprint == expected,
                    "source changed during enrollment capture"
                );
                file.sync_all()?;
                file.rewind()?;
                ensure!(
                    fingerprint_file(&mut file, limits.max_blob_bytes, None)? == expected,
                    "enrollment custody readback mismatch"
                );
                copied = copied
                    .checked_add(expected.bytes())
                    .context("review byte overflow")?;
                let version = Version {
                    file: file_name,
                    fingerprint: expected.clone(),
                    git_oid: None,
                    mode: Some(0o100000 | metadata.mode() & 0o777),
                };
                versions.insert(format!("source-{}", expected.sha256()), version.clone());
                version
            }
        };
        let named = std::fs::symlink_metadata(repo.join(&selected.path))?;
        ensure!(
            !named.is_symlink()
                && stamp(&metadata) == stamp(&source.metadata()?)
                && stamp(&metadata) == stamp(&named),
            "source changed during enrollment review"
        );
        assets.push(Asset {
            path_hex: journal::encode_relative_path(selected.path.as_os_str().as_bytes())?,
            head: captured.remove(0),
            index: captured.remove(0),
            worktree,
        });
    }
    let repository = git2::Repository::open(repo)?;
    let identity = std::fs::metadata(repository.path().canonicalize()?)?;
    ensure!(
        (identity.dev(), identity.ino()) == git_identity
            && head(&repository)? == original_head
            && index_fingerprint(&git_directory.join("index"), MAX_INDEX_BYTES, None)?
                == original_index,
        "HEAD or index changed during enrollment review"
    );
    ensure!(
        Instant::now() < deadline,
        "enrollment review deadline expired"
    );
    let review = Review {
        schema_version: 1,
        repo_id: repo_id.into(),
        worktree: repo.into(),
        git_directory,
        git_device: git_identity.0,
        git_inode: git_identity.1,
        head: original_head,
        index_copy: original_index.as_ref().map(|_| "index.before".into()),
        index: original_index,
        assets,
        grants_enrollment: false,
    };
    let bytes = serde_json::to_vec(&review)?;
    ensure!(
        bytes.len() <= MAX_REVIEW_BYTES
            && (bytes.len() as u64) <= limits.max_retained_bytes - retained - index_bytes - copied,
        "review metadata capacity exceeded"
    );
    let manifest_path = directory.join("review.json");
    let mut file = private_file(&manifest_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    File::open(&directory)?.sync_all()?;
    Ok(PreparedReview {
        manifest_path,
        review_sha256: format!("{:x}", Sha256::digest(&bytes)),
        assets: review.assets.len(),
        preserved_versions: versions.len(),
        preserved_bytes: copied,
        metadata_bytes: index_bytes + bytes.len() as u64,
    })
}
