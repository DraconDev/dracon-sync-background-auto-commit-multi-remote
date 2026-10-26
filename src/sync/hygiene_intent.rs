//! Durable artifact-ignore intent, separate from uncertain canonical recovery.
//! Records survive before index installation. Only current policy plus immutable
//! captured artifact identities authorize re-rendering; old whole-file bytes are
//! never replayed. Retirement requires actual committed ignore rules.
use super::*;
use crate::git::staged_intent::{CapturedHead, StagedIntentSnapshot};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

const PREFIX: &str = "dracon-hygiene-intent-";
const LIMIT: u64 = 4 * 1024 * 1024;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    path: Vec<u8>,
    oid: String,
    mode: u32,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    repo: Vec<u8>,
    parent: String,
    tree: String,
    paths: Vec<Proof>,
}
struct Loaded {
    directory: std::fs::File,
    intent: Intent,
    digest: String,
}

fn stamp(m: &std::fs::Metadata) -> (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.uid(),
        m.gid(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
fn owned_file(m: &std::fs::Metadata) -> Result<()> {
    ensure!(
        m.is_file()
            && m.uid() == unsafe { libc::geteuid() }
            && m.nlink() == 1
            && m.mode() & 0o077 == 0,
        "foreign/aliased/nonprivate hygiene record; retained"
    );
    Ok(())
}
fn open_leaf(
    dir: &std::fs::File,
    name: &std::ffi::CStr,
    flags: i32,
) -> Result<Option<std::fs::File>> {
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(e.into());
    }
    Ok(Some(unsafe { std::fs::File::from_raw_fd(fd) }))
}
fn ack_name(digest: &str) -> std::ffi::CString {
    std::ffi::CString::new(format!("committed-{digest}")).expect("hex digest")
}
fn load(path: &Path, repo: &[u8]) -> Result<Option<Loaded>> {
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let m = directory.metadata()?;
    ensure!(
        m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
        "hygiene intent directory must be owned/private; retained"
    );
    let mut file = open_leaf(&directory, c"intent.json", libc::O_RDONLY)?
        .context("incomplete hygiene intent; retained")?;
    let before = file.metadata()?;
    owned_file(&before)?;
    ensure!(before.len() <= LIMIT, "oversized hygiene intent; retained");
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == before.len() && stamp(&before) == stamp(&file.metadata()?),
        "hygiene intent changed while reading; retained"
    );
    let intent: Intent = serde_json::from_slice(&bytes)?;
    ensure!(
        intent.version == 1
            && intent.repo == repo
            && !intent.paths.is_empty()
            && intent.paths.len() <= 65536
            && crate::git::is_valid_object_id(&intent.parent)
            && crate::git::is_valid_object_id(&intent.tree),
        "unverified hygiene intent identity/version; retained"
    );
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if let Some(ack) = open_leaf(&directory, &ack_name(&digest), libc::O_RDONLY)? {
        let m = ack.metadata()?;
        owned_file(&m)?;
        ensure!(m.len() == 0, "invalid hygiene acknowledgement; retained");
        return Ok(None);
    }
    Ok(Some(Loaded {
        directory,
        intent,
        digest,
    }))
}
fn registry(repo: &Path, create: bool) -> Result<Option<std::fs::File>> {
    use std::os::unix::fs::DirBuilderExt;
    let gitdir =
        crate::git::path_gitdir(repo).context("cannot locate hygiene intent Git directory")?;
    let path = gitdir.join("dracon-hygiene-intents");
    if create {
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => std::fs::File::open(&gitdir)?.sync_all()?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let directory = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let m = directory.metadata()?;
    ensure!(
        m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
        "foreign/nonprivate hygiene registry; retained"
    );
    Ok(Some(directory))
}
fn records(repo: &Path) -> Result<Vec<Loaded>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let canonical = std::fs::canonicalize(repo)?;
    let Some(directory) = registry(repo, false)? else {
        return Ok(Vec::new());
    };
    let root = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let mut loaded = Vec::new();
    for e in std::fs::read_dir(root)? {
        ensure!(
            std::time::Instant::now() < deadline,
            "hygiene intent inspection deadline; retained"
        );
        let e = e?;
        if e.file_name().as_bytes().starts_with(PREFIX.as_bytes()) {
            if let Some(record) = load(&e.path(), canonical.as_os_str().as_bytes())? {
                loaded.push(record);
            }
        }
    }
    Ok(loaded)
}
pub(in crate::sync) fn has_pending(repo: &Path) -> Result<bool> {
    Ok(!records(repo)?.is_empty())
}

pub(in crate::sync) fn persist(
    repo: &Path,
    baseline: &StagedIntentSnapshot,
    selected: &StagedIntentSnapshot,
    tree: &str,
    deletions: &BTreeMap<Vec<u8>, String>,
) -> Result<()> {
    ensure!(
        baseline.changed.is_empty() && crate::git::is_valid_object_id(tree),
        "artifact intent must originate in an unstaged ordinary cycle"
    );
    let mut paths = Vec::new();
    for (path, oid) in deletions {
        if selected.changed.get(path) != Some(&None) {
            continue;
        }
        let entry = baseline
            .entries
            .get(path)
            .context("missing original artifact identity")?;
        ensure!(
            entry.oid == *oid && entry.mode != 0o160000,
            "unverified artifact identity"
        );
        paths.push(Proof {
            path: path.clone(),
            oid: oid.clone(),
            mode: entry.mode,
        });
    }
    if paths.is_empty() {
        return Ok(());
    }
    let CapturedHead::Commit(parent) = &baseline.head else {
        anyhow::bail!("artifact intent requires captured committed provenance")
    };
    let intent = Intent {
        version: 1,
        repo: std::fs::canonicalize(repo)?.as_os_str().as_bytes().to_vec(),
        parent: parent.clone(),
        tree: tree.to_owned(),
        paths,
    };
    let bytes = serde_json::to_vec(&intent)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "hygiene intent exceeds durable budget"
    );
    let directory = registry(repo, true)?.context("hygiene registry disappeared")?;
    let root = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let dir = tempfile::Builder::new()
        .prefix("dracon-hygiene-preparing-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(&root)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.path().join("intent.json"))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::File::open(dir.path())?.sync_all()?;
    // Publish a complete durable manifest atomically. A killed preparer cannot
    // leave a half-JSON ACTIVE record or hide an installed index: installation
    // is still forbidden until this rename and directory fsync have succeeded.
    let preparing = dir
        .path()
        .file_name()
        .context("missing prepared intent name")?
        .as_bytes()
        .to_vec();
    let active =
        std::ffi::CString::new(format!("{PREFIX}{}", String::from_utf8_lossy(&preparing)))?;
    let preparing = std::ffi::CString::new(preparing)?;
    let _retained = dir.keep();
    let renamed = unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            preparing.as_ptr(),
            directory.as_raw_fd(),
            active.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if renamed != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    directory.sync_all()?;
    Ok(())
}

/// Restore only a recorded OWNED deletion grant, never replace the index or
/// widen a captured user epoch. Any new/foreign cached delta changes the tree
/// and therefore cannot inherit this grant.
pub(in crate::sync) async fn recover(
    repo: &Path,
    snapshot: &StagedIntentSnapshot,
    deadline: Instant,
) -> Result<BTreeMap<Vec<u8>, String>> {
    let mut granted = BTreeMap::new();
    if snapshot.changed.is_empty() {
        return Ok(granted);
    }
    let CapturedHead::Commit(parent) = &snapshot.head else {
        return Ok(granted);
    };
    let pending = records(repo)?;
    if pending.is_empty() {
        return Ok(granted);
    }
    let tree = snapshot.materialize_tree(repo, deadline).await?;
    for record in pending {
        if &record.intent.parent != parent || record.intent.tree != tree {
            continue;
        }
        let original =
            staged_epoch::git(repo, &["ls-tree", "-r", "-z", "--full-tree", parent], None).await?;
        let mut identities = BTreeMap::new();
        for raw in original.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = raw
                .iter()
                .position(|b| *b == b'\t')
                .context("invalid captured artifact tree")?;
            let fields: Vec<_> = std::str::from_utf8(&raw[..tab])?.split(' ').collect();
            ensure!(fields.len() == 3, "invalid captured artifact identity");
            identities.insert(
                raw[tab + 1..].to_vec(),
                (u32::from_str_radix(fields[0], 8)?, fields[2].to_owned()),
            );
        }
        for proof in record.intent.paths {
            let path = staged_epoch::path_from_git_bytes(&proof.path)?;
            ensure!(
                crate::git::is_safe_git_path(&path)
                    && proof.mode != 0o160000
                    && identities.get(&proof.path) == Some(&(proof.mode, proof.oid.clone()))
                    && snapshot.changed.get(&proof.path) == Some(&None),
                "uncertain recorded artifact deletion; index retained"
            );
            granted.insert(proof.path, proof.oid);
        }
    }
    Ok(granted)
}

pub(in crate::sync) async fn retry(
    repo: &Path,
    policy: &SyncPolicy,
    excluded: &BTreeSet<String>,
    cleanup: bool,
    deadline: Instant,
) -> Result<()> {
    tokio::time::timeout_at(deadline, async {
        for record in records(repo)? {
            ensure!(
                cleanup,
                "artifact ignore intent retained: current cleanup policy disabled"
            );
            let overrides = load_repo_override(repo);
            ensure!(
                overrides
                    .build_artifact_cleanup
                    .unwrap_or(policy.build_artifact_cleanup),
                "artifact intent retained: current cleanup override disabled"
            );
            let excludes = crate::policy::effective_auto_commit_excludes(policy, &overrides);
            let head = staged_epoch::git(repo, &["rev-parse", "HEAD"], None).await?;
            let head = std::str::from_utf8(&head.stdout)?.trim();
            // A branch switch is not authority to apply another branch's intent.
            staged_epoch::git(
                repo,
                &["merge-base", "--is-ancestor", &record.intent.parent, head],
                None,
            )
            .await
            .context("artifact intent parent is not verified in current ancestry; retained")?;
            let tree = staged_epoch::git(
                repo,
                &["ls-tree", "-r", "-z", "--full-tree", &record.intent.parent],
                None,
            )
            .await?;
            let mut original = BTreeMap::new();
            for raw in tree.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
                let tab = raw
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("invalid artifact provenance tree")?;
                let fields: Vec<_> = std::str::from_utf8(&raw[..tab])?.split(' ').collect();
                ensure!(fields.len() == 3, "invalid artifact provenance fields");
                original.insert(
                    raw[tab + 1..].to_vec(),
                    (u32::from_str_radix(fields[0], 8)?, fields[2].to_owned()),
                );
            }
            let mut names = BTreeSet::new();
            let mut seen = BTreeSet::new();
            for proof in &record.intent.paths {
                ensure!(
                    seen.insert(&proof.path)
                        && proof.mode != 0o160000
                        && crate::git::is_valid_object_id(&proof.oid)
                        && original.get(&proof.path) == Some(&(proof.mode, proof.oid.clone())),
                    "unverified captured artifact path/OID; retained"
                );
                let path = staged_epoch::path_from_git_bytes(&proof.path)?;
                ensure!(
                    crate::git::is_safe_git_path(&path),
                    "unsafe artifact intent path"
                );
                let entry = dracon_git::types::DiffFile::new(
                    path.clone(),
                    dracon_git::types::FileStatus::Deleted,
                );
                ensure!(
                    crate::exclude::should_stage_entry_with_blob_size(
                        repo,
                        &entry,
                        0,
                        &BTreeSet::new(),
                        &policy.exclude_file_patterns,
                        policy.max_stage_file_bytes,
                        &excludes
                    ),
                    "artifact intent conflicts with current explicit exclusions; retained"
                );
                names.insert(
                    artifact_component(&path, excluded).context(
                        "artifact intent no longer authorized by current directory policy",
                    )?,
                );
            }
            let patterns: Vec<_> = names
                .iter()
                .map(|n| crate::exclude::hygiene::literal_directory_pattern(n))
                .collect::<Result<_>>()?;
            let listed =
                staged_epoch::git(repo, &["ls-tree", "-z", head, "--", ".gitignore"], None).await?;
            let committed = if listed.stdout.is_empty() {
                false
            } else {
                let body = staged_epoch::git(
                    repo,
                    &["cat-file", "--filters", &format!("{head}:.gitignore")],
                    None,
                )
                .await?;
                ensure!(
                    body.stdout.len() as u64 <= LIMIT,
                    "committed ignore exceeds inspection budget"
                );
                crate::exclude::hygiene::render_positive_additions(
                    std::str::from_utf8(&body.stdout)?,
                    &patterns,
                )?
                .is_none()
            };
            let current_tree =
                staged_epoch::git(repo, &["ls-tree", "-r", "-z", "--full-tree", head], None)
                    .await?;
            let mut present = BTreeSet::new();
            for raw in current_tree
                .stdout
                .split(|b| *b == 0)
                .filter(|r| !r.is_empty())
            {
                let tab = raw
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("invalid current artifact tree")?;
                present.insert(raw[tab + 1..].to_vec());
            }
            let removed = record
                .intent
                .paths
                .iter()
                .all(|proof| !present.contains(&proof.path));
            if committed && removed {
                // Independent immutable Git content, not a helper exit/marker,
                // establishes that ordinary staging has caught up. Do not undo
                // newer canonical edits made after that committed metadata.
                let ack = open_leaf(
                    &record.directory,
                    &ack_name(&record.digest),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                )?
                .context("cannot acknowledge committed hygiene intent")?;
                ack.sync_all()?;
                record.directory.sync_all()?;
            } else {
                // Render fresh canonical bytes and preserve foreign index/writer
                // ownership. A successful write still owes ordinary staging;
                // the intent remains pending until committed content proves it.
                let _ = super::super::hygiene_apply::apply(repo, &patterns, deadline).await?;
            }
        }
        Ok(())
    })
    .await
    .context("hygiene retry deadline expired; intent retained")?
}
