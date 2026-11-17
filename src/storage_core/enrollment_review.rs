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
use std::os::unix::ffi::{OsStrExt, OsStringExt};
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

/// Explicit approval of one privately preserved forward-enrollment review.
/// This grants no backend authority and cannot relax an existing enrollment
/// contract. It is deliberately not deserializable from repository data.
pub struct ApprovedEnrollment {
    review: Review,
    directory: PathBuf,
    limits: ReviewLimits,
}

/// Bind an operator-reviewed exact digest to its preserved versions. The caller
/// must select the review/digest from operator state, not repository content.
/// Ready-to-stage jobs and their required copies remain separate prerequisites.
pub async fn approve_review(
    repo: &Path,
    repo_id: &str,
    receipt: &PreparedReview,
    limits: ReviewLimits,
) -> Result<ApprovedEnrollment> {
    let repo = repo.to_path_buf();
    let repo_id = repo_id.to_owned();
    let path = receipt.manifest_path.clone();
    let expected = receipt.review_sha256.clone();
    tokio::task::spawn_blocking(move || {
        super::reference::validate_sha256(&expected)?;
        ensure!(
            path.is_absolute()
                && path.file_name().is_some_and(|n| n == "review.json")
                && !path.starts_with(&repo),
            "operator-selected private review required"
        );
        let directory = path
            .parent()
            .context("review parent unavailable")?
            .to_path_buf();
        journal::private_directory(&directory, false)?;
        let mut file = journal::open_private(&path, false, false)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_REVIEW_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= MAX_REVIEW_BYTES && format!("{:x}", Sha256::digest(&bytes)) == expected,
            "review digest mismatch"
        );
        let review: Review = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid private enrollment review"))?;
        ensure!(
            review.schema_version == 1
                && !review.grants_enrollment
                && !review.assets.is_empty()
                && review.assets.len() <= MAX_PATHS,
            "invalid enrollment review contract"
        );
        let approved = ApprovedEnrollment {
            review,
            directory,
            limits,
        };
        approved.validate_preservation(&repo, &repo_id)?;
        Ok(approved)
    })
    .await
    .context("enrollment approval worker failed")?
}

impl ApprovedEnrollment {
    fn validate_preservation(&self, root: &Path, id: &str) -> Result<()> {
        ensure!(
            !self.limits.timeout.is_zero() && self.limits.max_blob_bytes > 0,
            "positive enrollment approval bounds required"
        );
        let deadline = Instant::now()
            .checked_add(self.limits.timeout)
            .context("invalid approval deadline")?;
        ensure!(
            root.is_absolute()
                && root.canonicalize()? == root
                && self.review.repo_id == id
                && self.review.worktree == root,
            "enrollment approval repository mismatch"
        );
        let repository = git2::Repository::open(root)?;
        ensure!(
            repository
                .workdir()
                .context("bare approval repository")?
                .canonicalize()?
                == root
                && repository.path().canonicalize()? == self.review.git_directory,
            "enrollment approval Git directory mismatch"
        );
        let metadata = std::fs::metadata(&self.review.git_directory)?;
        ensure!(
            (metadata.dev(), metadata.ino()) == (self.review.git_device, self.review.git_inode)
                && head(&repository)? == self.review.head
                && index_fingerprint(
                    &self.review.git_directory.join("index"),
                    MAX_INDEX_BYTES,
                    None
                )? == self.review.index,
            "enrollment approval HEAD/index changed"
        );
        match (&self.review.index, &self.review.index_copy) {
            (Some(expected), Some(name)) if name == "index.before" => {
                ensure!(
                    index_fingerprint(&self.directory.join(name), MAX_INDEX_BYTES, None)?.as_ref()
                        == Some(expected),
                    "preserved enrollment index changed"
                );
            }
            (None, None) => {}
            _ => anyhow::bail!("invalid preserved index binding"),
        }
        let mut paths = BTreeSet::new();
        let mut versions = BTreeMap::<String, Fingerprint>::new();
        let mut total = 0u64;
        for asset in &self.review.assets {
            let path = super::staging::decode_path(&asset.path_hex)?;
            ensure!(paths.insert(path), "duplicate reviewed enrollment path");
            for version in asset
                .head
                .iter()
                .chain(asset.index.iter())
                .chain(std::iter::once(&asset.worktree))
            {
                version.fingerprint.validate()?;
                let named = if let Some(oid) = version
                    .file
                    .strip_prefix("git-")
                    .and_then(|s| s.strip_suffix(".blob"))
                {
                    matches!(oid.len(), 40 | 64)
                        && oid
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                } else {
                    version.file == format!("source-{}.blob", version.fingerprint.sha256())
                };
                ensure!(named, "invalid reviewed custody path");
                if let Some(prior) = versions.get(&version.file) {
                    ensure!(
                        prior == &version.fingerprint,
                        "conflicting reviewed versions"
                    );
                    continue;
                }
                ensure!(
                    Instant::now() < deadline,
                    "enrollment approval deadline expired"
                );
                let mut file =
                    journal::open_private(&self.directory.join(&version.file), false, false)?;
                ensure!(
                    fingerprint_file(&mut file, self.limits.max_blob_bytes, None)?
                        == version.fingerprint,
                    "reviewed custody bytes changed"
                );
                total = total
                    .checked_add(version.fingerprint.bytes())
                    .context("approval byte overflow")?;
                ensure!(
                    total <= self.limits.max_total_bytes,
                    "approval capacity exceeded"
                );
                versions.insert(version.file.clone(), version.fingerprint.clone());
            }
        }
        ensure!(
            Instant::now() < deadline,
            "enrollment approval deadline expired"
        );
        Ok(())
    }

    pub(crate) fn validate_bundle(
        &self,
        root: &Path,
        id: &str,
        bundle: &super::staging::StageBundle<'_>,
    ) -> Result<BTreeSet<Vec<u8>>> {
        self.validate_preservation(root, id)?;
        ensure!(
            bundle.repo_id() == id,
            "reviewed staging repository mismatch"
        );
        let mut paths = BTreeSet::new();
        for asset in &self.review.assets {
            let path = super::staging::decode_path(&asset.path_hex)?;
            let job = bundle
                .jobs()
                .iter()
                .find_map(|lease| {
                    let job = lease.load().ok()?;
                    (job.spec().path_hex == asset.path_hex).then_some(job)
                })
                .context("reviewed enrollment lacks a verified staging job")?;
            ensure!(
                job.spec().source == asset.worktree.fingerprint,
                "reviewed enrollment source version mismatch"
            );
            let source_path = PathBuf::from(std::ffi::OsString::from_vec(path.clone()));
            let (_, actual) =
                capture::select_source(root, &source_path, self.limits.max_blob_bytes)?;
            ensure!(
                actual == asset.worktree.fingerprint,
                "reviewed working source changed"
            );
            paths.insert(path);
        }
        Ok(paths)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::process::Command;

    struct Fixture {
        _root: tempfile::TempDir,
        repo: PathBuf,
        custody: PathBuf,
        git: PathBuf,
        id: String,
        paths: Vec<PathBuf>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let base = root.path().canonicalize().unwrap();
            let repo_path = base.join("repo");
            std::fs::create_dir(&repo_path).unwrap();
            let repo = git2::Repository::init(&repo_path).unwrap();
            std::fs::write(repo_path.join("art.bin"), b"committed original").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("art.bin")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let signature =
                git2::Signature::now("Enrollment Fixture", "enrollment@example.invalid").unwrap();
            repo.commit(Some("HEAD"), &signature, &signature, "baseline", &tree, &[])
                .unwrap();
            std::fs::write(repo_path.join("art.bin"), b"staged intermediate").unwrap();
            index.add_path(Path::new("art.bin")).unwrap();
            index.write().unwrap();
            std::fs::write(repo_path.join("art.bin"), b"working latest").unwrap();
            let output = Command::new("sh")
                .args(["-c", "command -v git"])
                .output()
                .unwrap();
            let git = PathBuf::from(std::str::from_utf8(&output.stdout).unwrap().trim())
                .canonicalize()
                .unwrap();
            Self {
                _root: root,
                repo: repo_path,
                custody: base.join("custody"),
                git,
                id: "a".repeat(64),
                paths: vec![PathBuf::from("art.bin")],
            }
        }

        fn request(&self) -> ReviewRequest<'_> {
            ReviewRequest {
                repo: &self.repo,
                repo_id: &self.id,
                paths: &self.paths,
                custody_root: &self.custody,
                git: &self.git,
                limits: ReviewLimits {
                    max_blob_bytes: 1024 * 1024,
                    max_total_bytes: 8 * 1024 * 1024,
                    max_retained_bytes: 16 * 1024 * 1024,
                    timeout: Duration::from_secs(20),
                },
            }
        }

        fn index(&self) -> Vec<u8> {
            std::fs::read(self.repo.join(".git/index")).unwrap()
        }

        fn manifests(&self) -> Vec<PathBuf> {
            if !self.custody.exists() {
                return vec![];
            }
            std::fs::read_dir(&self.custody)
                .unwrap()
                .filter_map(|e| {
                    let path = e.unwrap().path().join("review.json");
                    path.exists().then_some(path)
                })
                .collect()
        }

        fn shim(&self, action: &str) -> PathBuf {
            let path = self._root.path().join("review-git-shim");
            let real = self.git.to_string_lossy().replace('\'', "'\\''");
            std::fs::write(&path, format!(
                "#!/bin/sh\nif [ ! -e review-fixture-switched ]; then touch review-fixture-switched; {action}; fi\nexec '{real}' \"$@\"\n"
            )).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
    }

    #[tokio::test]
    async fn preserves_three_distinct_versions_and_the_exact_index_without_enrollment() {
        let fixture = Fixture::new();
        let before = fixture.index();
        let result = prepare_review(fixture.request()).await.unwrap();
        let bytes = std::fs::read(&result.manifest_path).unwrap();
        assert_eq!(
            result.review_sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
        let review: Review = serde_json::from_slice(&bytes).unwrap();
        assert!(!review.grants_enrollment);
        assert_eq!(review.assets.len(), 1);
        assert_eq!(result.preserved_versions, 3);
        let directory = result.manifest_path.parent().unwrap();
        let asset = &review.assets[0];
        assert_eq!(
            std::fs::read(directory.join(&asset.head.as_ref().unwrap().file)).unwrap(),
            b"committed original"
        );
        assert_eq!(
            std::fs::read(directory.join(&asset.index.as_ref().unwrap().file)).unwrap(),
            b"staged intermediate"
        );
        assert_eq!(
            std::fs::read(directory.join(&asset.worktree.file)).unwrap(),
            b"working latest"
        );
        assert_eq!(
            std::fs::read(directory.join(review.index_copy.unwrap())).unwrap(),
            before
        );
        assert_eq!(fixture.index(), before);
        assert_eq!(
            std::fs::read(fixture.repo.join("art.bin")).unwrap(),
            b"working latest"
        );
        assert!(git2::Repository::open(&fixture.repo)
            .unwrap()
            .config()
            .unwrap()
            .get_string("dracon.storageRepoId")
            .is_err());
        for entry in std::fs::read_dir(directory).unwrap() {
            assert_eq!(entry.unwrap().metadata().unwrap().mode() & 0o077, 0);
        }
    }

    #[tokio::test]
    async fn duplicate_or_escaping_paths_are_refused_before_creating_custody() {
        let mut fixture = Fixture::new();
        fixture.paths.push(PathBuf::from("art.bin"));
        assert!(prepare_review(fixture.request()).await.is_err());
        assert!(!fixture.custody.exists());
        fixture.paths = vec![PathBuf::from("../outside.bin")];
        assert!(prepare_review(fixture.request()).await.is_err());
        assert!(!fixture.custody.exists());
    }

    #[tokio::test]
    async fn source_symlinks_do_not_produce_a_complete_review() {
        let fixture = Fixture::new();
        let outside = fixture._root.path().join("outside.bin");
        std::fs::write(&outside, b"not authorized").unwrap();
        std::fs::remove_file(fixture.repo.join("art.bin")).unwrap();
        symlink(outside, fixture.repo.join("art.bin")).unwrap();
        assert!(prepare_review(fixture.request()).await.is_err());
        assert!(fixture.manifests().is_empty());
    }

    #[tokio::test]
    async fn byte_and_retention_limits_never_publish_partial_review_authority() {
        let fixture = Fixture::new();
        let before = fixture.index();
        let mut request = fixture.request();
        request.limits.max_blob_bytes = 3;
        assert!(prepare_review(request).await.is_err());
        assert!(fixture.manifests().is_empty());
        let mut request = fixture.request();
        request.limits.max_retained_bytes = 1;
        assert!(prepare_review(request).await.is_err());
        assert!(fixture.manifests().is_empty());
        assert_eq!(fixture.index(), before);
    }

    #[tokio::test]
    async fn another_repository_id_cannot_be_reviewed_under_an_existing_binding() {
        let fixture = Fixture::new();
        git2::Repository::open(&fixture.repo)
            .unwrap()
            .config()
            .unwrap()
            .set_str("dracon.storageRepoId", &"b".repeat(64))
            .unwrap();
        assert!(prepare_review(fixture.request()).await.is_err());
        assert!(fixture.manifests().is_empty());
    }

    #[tokio::test]
    async fn head_changes_during_capture_refuse_the_review_and_keep_custody() {
        let fixture = Fixture::new();
        let repo = git2::Repository::open(&fixture.repo).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let signature =
            git2::Signature::now("Enrollment Fixture", "enrollment@example.invalid").unwrap();
        let next = repo
            .commit(
                None,
                &signature,
                &signature,
                "new fixture head",
                &parent.tree().unwrap(),
                &[&parent],
            )
            .unwrap();
        let real = fixture.git.to_string_lossy().replace('\'', "'\\''");
        let shim = fixture.shim(&format!("'{real}' update-ref HEAD {next}"));
        let request = ReviewRequest {
            git: &shim,
            ..fixture.request()
        };
        assert!(prepare_review(request).await.is_err());
        assert!(fixture.manifests().is_empty());
        assert!(std::fs::read_dir(&fixture.custody)
            .unwrap()
            .any(|e| e.unwrap().path().is_dir()));
    }

    #[tokio::test]
    async fn an_index_edit_during_capture_is_preserved_and_refuses_completion() {
        let fixture = Fixture::new();
        std::fs::write(fixture.repo.join("manual.txt"), b"operator note").unwrap();
        let real = fixture.git.to_string_lossy().replace('\'', "'\\''");
        let shim = fixture.shim(&format!("'{real}' add -- manual.txt"));
        assert!(prepare_review(ReviewRequest {
            git: &shim,
            ..fixture.request()
        })
        .await
        .is_err());
        assert!(fixture.manifests().is_empty());
        assert!(git2::Repository::open(&fixture.repo)
            .unwrap()
            .index()
            .unwrap()
            .get_path(Path::new("manual.txt"), 0)
            .is_some());
    }

    #[tokio::test]
    async fn approval_binds_exact_review_and_rejects_changed_custody() {
        let fixture = Fixture::new();
        let mut receipt = prepare_review(fixture.request()).await.unwrap();
        let original = receipt.review_sha256.clone();
        receipt.review_sha256 = "b".repeat(64);
        assert!(approve_review(
            &fixture.repo,
            &fixture.id,
            &receipt,
            fixture.request().limits
        )
        .await
        .is_err());
        receipt.review_sha256 = original;
        assert!(approve_review(
            &fixture.repo,
            &fixture.id,
            &receipt,
            fixture.request().limits
        )
        .await
        .is_ok());
        let review: Review =
            serde_json::from_slice(&std::fs::read(&receipt.manifest_path).unwrap()).unwrap();
        std::fs::write(
            receipt
                .manifest_path
                .parent()
                .unwrap()
                .join(&review.assets[0].index.as_ref().unwrap().file),
            b"corrupt",
        )
        .unwrap();
        assert!(approve_review(
            &fixture.repo,
            &fixture.id,
            &receipt,
            fixture.request().limits
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn approval_cannot_follow_a_forged_artifact_path() {
        let fixture = Fixture::new();
        let mut receipt = prepare_review(fixture.request()).await.unwrap();
        let mut review: Review =
            serde_json::from_slice(&std::fs::read(&receipt.manifest_path).unwrap()).unwrap();
        review.assets[0].worktree.file = "../outside.bin".into();
        let bytes = serde_json::to_vec(&review).unwrap();
        std::fs::write(&receipt.manifest_path, &bytes).unwrap();
        receipt.review_sha256 = format!("{:x}", Sha256::digest(&bytes));
        assert!(approve_review(
            &fixture.repo,
            &fixture.id,
            &receipt,
            fixture.request().limits
        )
        .await
        .is_err());
    }
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
