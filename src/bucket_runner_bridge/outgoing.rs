//! Read-only, bounded upper bound on newly outgoing raw blob bytes.
//! No filter execution, object writes, pack creation or network access.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(test)]
#[path = "outgoing_tests.rs"]
mod tests;

const MAX_ITEMS: usize = 20_000;
const MAX_FILE: u64 = 16 * 1024 * 1024;
type FileIdentity = (u64, u64, u64, i64, i64, i64, i64);
fn identity(m: &std::fs::Metadata) -> FileIdentity {
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

async fn git(repo: &Path, args: &[String], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut command = vec![
        "--no-optional-locks".to_owned(),
        "--no-replace-objects".to_owned(),
        "-c".to_owned(),
        "core.fsmonitor=false".to_owned(),
    ];
    command.extend_from_slice(args);
    let refs: Vec<&str> = command.iter().map(String::as_str).collect();
    let mut child =
        crate::git::spawn_git_command_cancellable_stdin(repo, &refs, "bucket-accounting")?;
    let stdin = child.stdin.take();
    let capture = crate::git::run_git_captured_output(child, repo, "bucket-accounting");
    let write = async move {
        if let (Some(mut pipe), Some(bytes)) = (stdin, input) {
            pipe.write_all(&bytes).await?;
        }
        Ok::<(), anyhow::Error>(())
    };
    let ((status, stdout, _stderr), ()) = tokio::try_join!(capture, write)?;
    // Never propagate captured diagnostics: filters/config may contain secrets.
    if !status.success() {
        bail!("outgoing-accounting-git-refused");
    }
    Ok(stdout)
}
fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}
fn text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).context("accounting-non-utf8")
}
fn oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|c| c.is_ascii_hexdigit())
}
fn add(total: &mut u64, bytes: u64) -> Result<()> {
    *total = total.checked_add(bytes).context("accounting-overflow")?;
    if *total > MAX_JS_INTEGER {
        bail!("accounting-unsafe-integer");
    }
    Ok(())
}

/// Filter-free status: disable every configured clean/process filter before
/// asking Git for dirty paths. This may overselect normalized/filtered files,
/// never run an upload-capable command to obtain a routing estimate.
async fn status_args(repo: &Path) -> Result<Vec<String>> {
    let config = git(repo, &args(&["config", "--name-only", "--list"]), None).await?;
    let mut result = args(&["--no-optional-locks", "-c", "core.fsmonitor=false"]);
    let mut filters = BTreeSet::new();
    for key in text(&config)?.lines() {
        if let Some(rest) = key.strip_prefix("filter.") {
            if let Some((name, _)) = rest.rsplit_once('.') {
                filters.insert(name.to_owned());
            }
        }
    }
    if filters.len() > 128 {
        bail!("accounting-filter-limit");
    }
    for name in filters {
        for (key, value) in [("clean", ""), ("process", ""), ("required", "false")] {
            result.extend(["-c".into(), format!("filter.{name}.{key}={value}")]);
        }
    }
    result.extend(args(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=all",
    ]));
    Ok(result)
}

#[derive(PartialEq, Eq)]
struct Snapshot {
    head: Vec<u8>,
    index: Vec<u8>,
    tips: Vec<u8>,
    status: Vec<u8>,
}
impl Snapshot {
    fn ensure_unchanged(&self, after: &Self) -> Result<()> {
        if self != after {
            bail!("accounting-snapshot-changed");
        }
        Ok(())
    }
}
async fn snapshot(repo: &Path, status_cmd: &[String]) -> Result<Snapshot> {
    Ok(Snapshot {
        head: git(repo, &args(&["rev-parse", "--verify", "HEAD"]), None).await?,
        index: git(repo, &args(&["ls-files", "--stage", "-z"]), None).await?,
        tips: git(
            repo,
            &args(&[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/remotes/",
            ]),
            None,
        )
        .await?,
        status: git(repo, status_cmd, None).await?,
    })
}

pub(super) async fn measure(repo: &Path, policy: &SyncPolicy) -> Result<u64> {
    tokio::time::timeout(Duration::from_secs(60), measure_inner(repo, policy))
        .await
        .context("outgoing-accounting-timeout")?
}
async fn measure_inner(repo: &Path, policy: &SyncPolicy) -> Result<u64> {
    let status_cmd = status_args(repo).await?;
    let before = snapshot(repo, &status_cmd).await?;
    let head = text(&before.head)?.trim();
    if !oid(head) {
        bail!("accounting-invalid-head");
    }
    let override_ = crate::policy::load_repo_override(repo);
    if override_.override_parse_error.is_some() {
        bail!("accounting-invalid-repo-policy");
    }
    let configured = git(repo, &args(&["remote"]), None).await?;
    let mut destinations: BTreeSet<&str> = text(&configured)?
        .lines()
        .filter(|name| {
            !override_.exclude_remotes.iter().any(|r| r == name)
                && (*name == "origin" || policy.remotes.iter().any(|r| r.name == *name))
        })
        .collect();
    // Mirror push can configure a policy destination later in this cycle.
    // Include a conservative superset of potential policy destinations, even
    // visibility/quota-gated Codeberg: a public cache can activate that mirror
    // despite auto_create=false. Only explicit exclusions remove destinations.
    // No mutable/cache-dependent availability guess may reduce this bound.
    for remote in &policy.remotes {
        if !override_.exclude_remotes.contains(&remote.name) {
            destinations.insert(&remote.name);
        }
    }
    if destinations.is_empty() {
        bail!("accounting-no-publish-destination");
    }
    let branch = git(repo, &args(&["symbolic-ref", "--short", "HEAD"]), None).await?;
    let branch = text(&branch)?.trim();
    let tips: BTreeMap<&str, &str> = text(&before.tips)?
        .lines()
        .map(|line| line.split_once(' ').context("accounting-invalid-tip"))
        .collect::<Result<_>>()?;
    let upstream = git(
        repo,
        &args(&[
            "config",
            "--default",
            "",
            "--get",
            &format!("branch.{branch}.remote"),
        ]),
        None,
    )
    .await?;
    let upstream = text(&upstream)?.trim();
    let mut objects = BTreeSet::new();
    for remote in destinations {
        let reference = format!("refs/remotes/{remote}/{branch}");
        let mut traversal = args(&["rev-list", "--objects", "--no-object-names", head]);
        if let Some(tip) = tips.get(reference.as_str()) {
            if !oid(tip) {
                bail!("accounting-invalid-tip");
            }
            traversal.extend(args(&["--not", tip]));
        } else if remote == upstream {
            // A configured upstream is expected to have a tracking tip. Do
            // not turn a vanished/corrupt tracking input into a measured zero.
            bail!("accounting-missing-publish-tip");
        }
        // A new/non-upstream destination without availability evidence uses
        // ALL reachable HEAD objects as a conservative bound, not an assertion
        // that the remote is empty. This permits safe first mirror publication.
        let output = git(repo, &traversal, None).await?;
        for object in text(&output)?.lines() {
            if !oid(object) {
                bail!("accounting-invalid-object");
            }
            objects.insert(object.to_owned());
            if objects.len() > MAX_ITEMS {
                bail!("accounting-object-limit");
            }
        }
    }
    let mut blob_sizes = BTreeMap::new();
    let mut total = 0;
    if !objects.is_empty() {
        let input = objects.iter().map(|s| format!("{s}\n")).collect::<String>();
        let output = git(
            repo,
            &args(&[
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ]),
            Some(input.into_bytes()),
        )
        .await?;
        let mut rows = 0;
        for row in text(&output)?.lines() {
            let fields: Vec<_> = row.split(' ').collect();
            if fields.len() != 3 || !objects.contains(fields[0]) {
                bail!("accounting-unreadable-object");
            }
            let bytes = fields[2]
                .parse::<u64>()
                .context("accounting-invalid-size")?;
            if fields[1] == "blob" {
                blob_sizes.insert(fields[0].to_owned(), bytes);
                add(&mut total, bytes)?;
            }
            rows += 1;
        }
        if rows != objects.len() {
            bail!("accounting-incomplete-object-list");
        }
    }
    let mut index = BTreeMap::new();
    for entry in before.index.split(|b| *b == 0).filter(|v| !v.is_empty()) {
        let entry = text(entry)?;
        let (header, path) = entry.split_once('\t').context("accounting-index-entry")?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3 || fields[2] != "0" || !oid(fields[1]) {
            bail!("accounting-unmerged-index");
        }
        index.insert(
            path.to_owned(),
            (fields[0].to_owned(), fields[1].to_owned()),
        );
        if index.len() > MAX_ITEMS {
            bail!("accounting-index-limit");
        }
    }
    let excludes = crate::policy::effective_auto_commit_excludes(policy, &override_);
    let dirs = policy.exclude_dir_names.iter().cloned().collect();
    let mut records = before.status.split(|b| *b == 0).filter(|v| !v.is_empty());
    let mut files = Vec::new();
    while let Some(record) = records.next() {
        if record.len() < 4 {
            bail!("accounting-invalid-status");
        }
        let xy = &record[..2];
        let path = text(&record[3..])?;
        if xy.contains(&b'R') || xy.contains(&b'C') {
            records.next().context("accounting-rename-source")?;
        }
        if !super::safe_relative(path) {
            bail!("accounting-unsafe-path");
        }
        let relative = PathBuf::from(path);
        let untracked = xy == b"??";
        let status = if xy.contains(&b'D') {
            dracon_git::types::FileStatus::Deleted
        } else if untracked {
            dracon_git::types::FileStatus::Added
        } else if xy.contains(&b'T') {
            dracon_git::types::FileStatus::TypeChange
        } else {
            dracon_git::types::FileStatus::Modified
        };
        let entry = dracon_git::types::DiffFile::new(relative.clone(), status);
        if untracked
            && (!policy.auto_stage_untracked
                || crate::exclude::matches_untracked_exclude(
                    repo,
                    &relative,
                    &policy.untracked_exclude_patterns,
                ))
        {
            continue;
        }
        // Use the shared pattern/size gate without dereferencing links or
        // invoking the directory/gitlink subprocess path. Raw stat length is
        // conservative for normal text normalization; arbitrary filters below
        // have no safe output bound and refuse the measurement.
        let source_meta = match std::fs::symlink_metadata(repo.join(path)) {
            Ok(meta) => meta,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && entry.status == dracon_git::types::FileStatus::Deleted =>
            {
                continue
            }
            Err(error) => return Err(error).context("accounting-unreadable-candidate"),
        };
        if source_meta.is_dir() {
            continue;
        }
        if !crate::exclude::should_stage_entry_with_blob_size(
            repo,
            &entry,
            source_meta.len(),
            &dirs,
            &policy.exclude_file_patterns,
            policy.max_stage_file_bytes,
            &excludes,
        ) {
            continue;
        }
        if let Some((mode, _)) = index.get(path) {
            if mode == "160000" {
                continue;
            }
        }
        if entry.status == dracon_git::types::FileStatus::Deleted && !repo.join(path).exists() {
            continue;
        }
        // No traversal through linked ancestors; symlink blobs are sized by
        // link text rather than their referent. Nested Git directories add
        // no parent blob and are never enumerated recursively here.
        let mut parent = repo.to_path_buf();
        for component in relative.parent().into_iter().flat_map(Path::components) {
            parent.push(component);
            if std::fs::symlink_metadata(&parent)?.file_type().is_symlink() {
                bail!("accounting-symlink-ancestor");
            }
            if parent.join(".git").exists() {
                bail!("accounting-nested-repository");
            }
        }
        let full = repo.join(path);
        let meta = std::fs::symlink_metadata(&full).context("accounting-unreadable-candidate")?;
        if meta.is_dir() {
            continue;
        }
        let attrs = git(
            repo,
            &args(&[
                "check-attr",
                "-z",
                "filter",
                "working-tree-encoding",
                "--",
                path,
            ]),
            None,
        )
        .await?;
        for attr in attrs.split(|b| *b == 0).collect::<Vec<_>>().chunks(3) {
            if attr.len() == 3 && !matches!(attr[2], b"unspecified" | b"unset") {
                bail!("accounting-unknown-filter-bound");
            }
        }
        let bytes = if meta.file_type().is_symlink() {
            use std::os::unix::ffi::OsStrExt;
            std::fs::read_link(&full)?.as_os_str().as_bytes().to_vec()
        } else {
            if !meta.is_file() || meta.len() > MAX_FILE {
                bail!("accounting-file-read-limit");
            }
            let mut file = tokio::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&full)
                .await?;
            if identity(&file.metadata().await?) != identity(&meta) {
                bail!("accounting-candidate-changed");
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .await?;
            if identity(&file.metadata().await?) != identity(&meta)
                || bytes.len() as u64 != meta.len()
            {
                bail!("accounting-candidate-changed");
            }
            bytes
        };
        let observed_identity = identity(&meta);
        let object = git(
            repo,
            &args(&["hash-object", "--no-filters", "--stdin"]),
            Some(bytes.clone()),
        )
        .await?;
        let object = text(&object)?.trim().to_owned();
        if !oid(&object) {
            bail!("accounting-invalid-worktree-object");
        }
        // Worktree restaging wins on selected paths. Conservatively retain
        // staged-only index blobs as well; no outgoing historical subtraction.
        if let Some((mode, staged)) = index.get(path) {
            if xy[0] != b' '
                && mode != "160000"
                && staged != &object
                && !blob_sizes.contains_key(staged)
            {
                let size = git(repo, &args(&["cat-file", "-s", staged]), None).await?;
                let size = text(&size)?.trim().parse::<u64>()?;
                add(&mut total, size)?;
                blob_sizes.insert(staged.clone(), size);
            }
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = blob_sizes.entry(object) {
            add(&mut total, bytes.len() as u64)?;
            entry.insert(bytes.len() as u64);
        }
        files.push((full, observed_identity));
        if files.len() > MAX_ITEMS {
            bail!("accounting-candidate-limit");
        }
    }
    let after = snapshot(repo, &status_cmd).await?;
    before.ensure_unchanged(&after)?;
    for (path, observed_identity) in files {
        let m = std::fs::symlink_metadata(path)?;
        if observed_identity != identity(&m) {
            bail!("accounting-candidate-changed");
        }
    }
    Ok(total)
}
