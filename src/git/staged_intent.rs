//! Read-only staged observation, not a lock, authorization or atomic commit gate.
//! Equality says nothing about later hooks, staging provenance or ref publication.

use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, path::Path};
use tokio::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CapturedHead {
    Commit(String),
    Unborn { symbolic_ref: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CachedIdentity {
    pub(crate) mode: u32,
    pub(crate) oid: String,
}

/// Paths are Git's exact relative bytes (including non-UTF8 names on Unix).
/// `changed[path] == None` explicitly records a cached deletion from HEAD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StagedIntentSnapshot {
    pub(crate) head: CapturedHead,
    pub(crate) entries: BTreeMap<Vec<u8>, CachedIdentity>,
    pub(crate) changed: BTreeMap<Vec<u8>, Option<CachedIdentity>>,
}

impl StagedIntentSnapshot {
    /// One caller-supplied absolute deadline covers every read and parse. Dropping
    /// this future kills its owned Git process group. No index lock is acquired.
    /// Git reads identities and flags together from one index observation; later
    /// integration must conditionally validate at an actual atomic commit boundary.
    /// Nonzero flags (including intent-to-add, skip-worktree and split-index
    /// bookkeeping) and sparse directory entries conservatively refuse.
    pub(crate) async fn capture(repo: &Path, deadline: Instant) -> Result<Self> {
        tokio::time::timeout_at(deadline, async {
            ensure!(Instant::now() < deadline, "staged intent deadline expired");
            let head = read_head(repo).await?;
            let raw = read(repo, &["ls-files", "--stage", "--debug", "--sparse", "-z"]).await?;
            let entries = parse_index_observation(&raw, deadline)?;
            let base = match &head {
                CapturedHead::Commit(oid) => {
                    let tree = read(repo, &["ls-tree", "-r", "-z", "--full-tree", oid]).await?;
                    parse_entries(&tree, false, deadline)?
                }
                CapturedHead::Unborn { .. } => BTreeMap::new(),
            };
            let mut changed = BTreeMap::new();
            for (path, entry) in &entries {
                ensure!(Instant::now() < deadline, "staged intent deadline expired");
                if base.get(path) != Some(entry) {
                    changed.insert(path.clone(), Some(entry.clone()));
                }
            }
            for path in base.keys() {
                ensure!(Instant::now() < deadline, "staged intent deadline expired");
                if !entries.contains_key(path) {
                    changed.insert(path.clone(), None);
                }
            }
            ensure!(
                head == read_head(repo).await?,
                "HEAD changed during staged intent capture"
            );
            Ok(Self {
                head,
                entries,
                changed,
            })
        })
        .await
        .context("staged intent deadline expired; owned Git cancelled")?
    }
}

// Borrow basename slices rather than allocating every full directory prefix.
// These caps bound graph nodes/edges, borrowed snapshot payload, cumulative wire
// input and cumulative output respectively. The shared runner additionally caps
// each stdout/stderr stream at 64 MiB, including on a failing command.
const TREE_PAYLOAD_LIMIT: usize = 64 * 1024 * 1024;
const TREE_NODE_LIMIT: usize = 262_144;
const TREE_INPUT_LIMIT: usize = 128 * 1024 * 1024;
const TREE_OUTPUT_LIMIT: usize = 32 * 1024 * 1024;

enum TreeChild<'a> {
    Directory(usize),
    Entry(&'a CachedIdentity),
}

#[derive(Default)]
struct TreeDirectory<'a> {
    children: BTreeMap<&'a [u8], TreeChild<'a>>,
    oid: Option<String>,
}

impl StagedIntentSnapshot {
    /// Writes only unreferenced tree objects derived from captured identities.
    /// This is not authorization: production commit integration remains pending.
    /// Never reads or locks the live index, runs filters, or overrides hooks.
    pub(crate) async fn materialize_tree(&self, repo: &Path, deadline: Instant) -> Result<String> {
        tokio::time::timeout_at(deadline, async {
            let check_deadline = || -> Result<()> {
                ensure!(Instant::now() < deadline, "tree materialization deadline expired");
                Ok(())
            };
            check_deadline()?;
            let mut dirs = vec![TreeDirectory::default()];
            let mut levels = vec![vec![0usize]];
            let mut payload = 0usize;
            let mut nodes = 1usize;
            for (path, entry) in &self.entries {
                check_deadline()?;
                payload = payload.checked_add(path.len()).and_then(|n| n.checked_add(entry.oid.len() + 16))
                    .context("tree payload overflow")?;
                ensure!(payload <= TREE_PAYLOAD_LIMIT, "tree payload limit exceeded");
                ensure!(matches!(entry.mode, 0o100644 | 0o100755 | 0o120000 | 0o160000), "unsupported tree mode");
                ensure!(super::is_valid_object_id(&entry.oid) && !entry.oid.bytes().all(|b| b == b'0'), "invalid tree object ID");
                let mut components = path.split(|b| *b == b'/').peekable();
                let mut parent = 0;
                let mut depth = 0;
                while let Some(name) = components.next() {
                    check_deadline()?;
                    ensure!(!name.is_empty() && name != b"." && name != b".." && !name.contains(&0), "invalid tree path");
                    if components.peek().is_none() {
                        ensure!(!dirs[parent].children.contains_key(name), "duplicate child or file/directory collision");
                        nodes += 1;
                        ensure!(nodes <= TREE_NODE_LIMIT, "tree graph limit exceeded");
                        dirs[parent].children.insert(name, TreeChild::Entry(entry));
                    } else {
                        let child = match dirs[parent].children.get(name) {
                            Some(TreeChild::Directory(id)) => *id,
                            Some(TreeChild::Entry(_)) => anyhow::bail!("file/directory collision"),
                            None => {
                                nodes += 1;
                                ensure!(nodes <= TREE_NODE_LIMIT, "tree graph limit exceeded");
                                let id = dirs.len();
                                dirs.push(TreeDirectory::default());
                                dirs[parent].children.insert(name, TreeChild::Directory(id));
                                if levels.len() <= depth + 1 { levels.push(Vec::new()); }
                                levels[depth + 1].push(id);
                                id
                            }
                        };
                        parent = child;
                        depth += 1;
                    }
                }
            }
            check_deadline()?;
            let format = read(repo, &["rev-parse", "--show-object-format"]).await?;
            let width = match format.as_slice() {
                b"sha1\n" => 40,
                b"sha256\n" => 64,
                _ => anyhow::bail!("unsupported object format"),
            };
            let mut total_input = 0usize;
            let mut total_output = format.len();
            for level in levels.iter().rev() {
                check_deadline()?;
                let mut input = Vec::new();
                for &id in level {
                    for (name, child) in &dirs[id].children {
                        check_deadline()?;
                        let (mode, kind, oid) = match child {
                            TreeChild::Directory(child) => (0o40000, "tree", dirs[*child].oid.as_deref().context("missing child tree")?),
                            TreeChild::Entry(entry) => (entry.mode, if entry.mode == 0o160000 { "commit" } else { "blob" }, entry.oid.as_str()),
                        };
                        ensure!(oid.len() == width, "tree object ID width mismatch");
                        let header = format!("{mode:o} {kind} {oid}\t");
                        let size = header.len() + name.len() + 1;
                        ensure!(size <= TREE_INPUT_LIMIT.saturating_sub(total_input), "tree input limit exceeded");
                        total_input += size;
                        input.extend_from_slice(header.as_bytes());
                        input.extend_from_slice(name);
                        input.push(0);
                    }
                    ensure!(total_input < TREE_INPUT_LIMIT, "tree input limit exceeded");
                    total_input += 1;
                    input.push(0); // --batch -z tree boundary (also represents empty root)
                }
                check_deadline()?;
                let mut child = super::spawn_git_command_cancellable_stdin(repo, &[
                    "--no-optional-locks", "-c", "core.fsmonitor=false", "mktree", "--batch", "-z",
                ], "staged tree")?;
                let mut stdin = child.stdin.take().context("missing tree stdin")?;
                let write = async {
                    use tokio::io::AsyncWriteExt;
                    stdin.write_all(&input).await?;
                    stdin.shutdown().await?;
                    Ok::<_, anyhow::Error>(())
                };
                let ((status, output, stderr), ()) = tokio::try_join!(
                    super::run_git_captured_output(child, repo, "staged tree"), write
                )?;
                total_output += output.len() + stderr.len();
                ensure!(total_output <= TREE_OUTPUT_LIMIT, "tree output limit exceeded");
                ensure!(status.success(), "mktree failed: {status}");
                assign_tree_oids(&mut dirs, level, &output, width, deadline)?;
            }
            check_deadline()?;
            dirs[0].oid.take().context("missing root tree")
        }).await.context("tree materialization deadline expired; owned Git cancelled")?
    }
}

fn assign_tree_oids(dirs: &mut [TreeDirectory<'_>], level: &[usize], output: &[u8], width: usize, deadline: Instant) -> Result<()> {
    ensure!(output.len() == level.len() * (width + 1), "invalid mktree output count/width");
    for (&id, record) in level.iter().zip(output.chunks_exact(width + 1)) {
        ensure!(Instant::now() < deadline, "tree materialization deadline expired");
        ensure!(record[width] == b'\n', "invalid mktree output terminator");
        let oid = std::str::from_utf8(&record[..width])?;
        ensure!(super::is_valid_object_id(oid) && !oid.bytes().all(|b| b == b'0'), "invalid mktree output ID");
        dirs[id].oid = Some(oid.to_owned());
    }
    Ok(())
}

async fn run(repo: &Path, args: &[&str]) -> Result<(std::process::ExitStatus, Vec<u8>)> {
    let mut argv = vec!["--no-optional-locks", "-c", "core.fsmonitor=false"];
    argv.extend_from_slice(args);
    // This existing runner sanitizes only the child's Git environment and
    // disables promisor lazy fetches. No worktree diff/filter commands are used.
    let mut child = super::spawn_git_command_cancellable_stdin(repo, &argv, "staged intent")?;
    drop(child.stdin.take());
    let (status, stdout, _) = super::run_git_captured_output(child, repo, "staged intent").await?;
    Ok((status, stdout))
}

async fn read(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let (status, bytes) = run(repo, args).await?;
    ensure!(
        status.success(),
        "staged intent Git read failed: {args:?}: {status}"
    );
    Ok(bytes)
}

async fn read_head(repo: &Path) -> Result<CapturedHead> {
    let (status, oid) = run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).await?;
    if status.success() {
        let oid = std::str::from_utf8(&oid)?.trim();
        ensure!(super::is_valid_object_id(oid), "invalid HEAD object ID");
        // Reject non-commit HEAD without resolving it into a different identity.
        let kind = read(repo, &["cat-file", "-t", oid]).await?;
        ensure!(kind == b"commit\n", "HEAD is not a commit");
        return Ok(CapturedHead::Commit(oid.to_owned()));
    }
    ensure!(status.code() == Some(1), "cannot resolve HEAD");
    let name = read(repo, &["symbolic-ref", "--quiet", "HEAD"]).await?;
    let name = name.strip_suffix(b"\n").context("invalid symbolic HEAD")?;
    let reference = std::str::from_utf8(name)?;
    ensure!(
        reference.starts_with("refs/heads/"),
        "unsupported unborn HEAD"
    );
    let (status, _) = run(repo, &["show-ref", "--verify", "--quiet", reference]).await?;
    ensure!(
        status.code() == Some(1),
        "HEAD is not an absent unborn branch"
    );
    Ok(CapturedHead::Unborn {
        symbolic_ref: name.to_vec(),
    })
}

fn parse_entries(
    bytes: &[u8],
    index: bool,
    deadline: Instant,
) -> Result<BTreeMap<Vec<u8>, CachedIdentity>> {
    let mut entries = BTreeMap::new();
    ensure!(
        bytes.is_empty() || bytes.ends_with(&[0]),
        "unterminated Git entry"
    );
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        ensure!(Instant::now() < deadline, "staged intent deadline expired");
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("missing entry path")?;
        let header = std::str::from_utf8(&record[..tab])?;
        let fields: Vec<_> = header.split(' ').collect();
        ensure!(fields.len() == 3, "invalid entry header");
        let (oid, kind) = if index {
            (fields[1], fields[2])
        } else {
            (fields[2], fields[1])
        };
        ensure!(
            !index || kind == "0",
            "unmerged index refuses staged intent capture"
        );
        let mode = u32::from_str_radix(fields[0], 8)?;
        ensure!(
            matches!(mode, 0o100644 | 0o100755 | 0o120000 | 0o160000),
            "unsupported index/tree mode"
        );
        ensure!(
            index || kind == if mode == 0o160000 { "commit" } else { "blob" },
            "unsupported tree entry"
        );
        ensure!(
            super::is_valid_object_id(oid) && !oid.bytes().all(|b| b == b'0'),
            "invalid entry object ID"
        );
        let path = &record[tab + 1..];
        ensure!(
            !path.is_empty()
                && !path.starts_with(b"/")
                && path
                    .split(|b| *b == b'/')
                    .all(|p| !p.is_empty() && p != b"." && p != b".."),
            "invalid entry path"
        );
        ensure!(
            entries
                .insert(
                    path.to_vec(),
                    CachedIdentity {
                        mode,
                        oid: oid.to_owned()
                    }
                )
                .is_none(),
            "duplicate index path"
        );
    }
    Ok(entries)
}

fn parse_index_observation(
    mut bytes: &[u8],
    deadline: Instant,
) -> Result<BTreeMap<Vec<u8>, CachedIdentity>> {
    let mut entries = BTreeMap::new();
    while !bytes.is_empty() {
        ensure!(Instant::now() < deadline, "staged intent deadline expired");
        let nul = bytes
            .iter()
            .position(|b| *b == 0)
            .context("missing index path terminator")?;
        // Parse identity and exact byte path before consuming this same entry's
        // debug payload. Never join flags from a second index read.
        let record = parse_entries(&bytes[..=nul], true, deadline)?;
        let (path, identity) = record.into_iter().next().context("missing index entry")?;
        bytes = &bytes[nul + 1..];
        for prefix in [
            b"  ctime: ".as_slice(),
            b"  mtime: ",
            b"  dev: ",
            b"  uid: ",
            b"  size: ",
        ] {
            let end = bytes
                .iter()
                .position(|b| *b == b'\n')
                .context("invalid index debug record")?;
            let line = &bytes[..end];
            ensure!(line.starts_with(prefix), "unsupported index debug payload");
            if prefix == b"  size: " {
                let text = std::str::from_utf8(line)?;
                let (_, flags) = text
                    .rsplit_once("\tflags: ")
                    .context("missing index flags")?;
                ensure!(
                    u32::from_str_radix(flags, 16)? == 0,
                    "unsupported index flags"
                );
            }
            bytes = &bytes[end + 1..];
        }
        ensure!(
            entries.insert(path, identity).is_none(),
            "duplicate index path"
        );
    }
    Ok(entries)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, time::Duration};

    fn git(repo: &Path, args: &[&OsStr]) -> Vec<u8> {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    fn cmd(repo: &Path, args: &[&str]) -> Vec<u8> {
        git(repo, &args.iter().map(OsStr::new).collect::<Vec<_>>())
    }
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        cmd(dir.path(), &["init", "--template=", "-b", "main"]);
        dir
    }
    async fn capture(repo: &Path) -> StagedIntentSnapshot {
        StagedIntentSnapshot::capture(repo, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap()
    }
    fn preserved(repo: &Path) -> (Vec<u8>, Vec<u8>) {
        (
            std::fs::read(repo.join(".git/index")).unwrap(),
            std::fs::read(repo.join("file")).unwrap(),
        )
    }

    #[tokio::test]
    async fn partial_staged_reverted_worktree_deletion_modes_gitlink_and_byte_paths() {
        let dir = fixture();
        let repo = dir.path();
        std::fs::write(repo.join("file"), b"base").unwrap();
        std::fs::write(repo.join("deleted"), b"delete me").unwrap();
        cmd(repo, &["add", "--", "file", "deleted"]);
        cmd(repo, &["commit", "-m", "base"]);
        let base = capture(repo).await;
        std::fs::write(repo.join("file"), b"staged").unwrap();
        cmd(repo, &["add", "--", "file"]);
        cmd(repo, &["update-index", "--chmod=+x", "--", "file"]);
        std::fs::write(repo.join("file"), b"base").unwrap();
        std::fs::write(repo.join("untracked"), b"unrelated").unwrap();
        cmd(repo, &["rm", "--cached", "--", "deleted"]);
        let name = OsStr::from_bytes(b"byte-\xff\t\n");
        std::fs::write(repo.join(name), b"byte content").unwrap();
        git(repo, &[OsStr::new("add"), OsStr::new("--"), name]);
        let head = String::from_utf8(cmd(repo, &["rev-parse", "HEAD"])).unwrap();
        cmd(
            repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},link", head.trim()),
            ],
        );
        // A required, failing clean filter must never run during capture.
        std::fs::write(repo.join(".gitattributes"), b"* filter=refuse\n").unwrap();
        cmd(repo, &["config", "filter.refuse.clean", "false"]);
        cmd(repo, &["config", "filter.refuse.required", "true"]);
        let foreign_lock = repo.join(".git/index.lock");
        std::fs::write(&foreign_lock, b"foreign lock preserved").unwrap();
        let before = preserved(repo);
        let snapshot = capture(repo).await;
        assert_eq!(
            std::fs::read(&foreign_lock).unwrap(),
            b"foreign lock preserved"
        );
        assert_eq!(before, preserved(repo));
        assert_eq!(snapshot.changed.get(b"deleted".as_slice()), Some(&None));
        assert_eq!(snapshot.entries[b"file".as_slice()].mode, 0o100755);
        assert_ne!(
            snapshot.entries[b"file".as_slice()].oid,
            base.entries[b"file".as_slice()].oid
        );
        assert!(snapshot.entries.contains_key(name.as_bytes()));
        assert_eq!(snapshot.entries[b"link".as_slice()].mode, 0o160000);
        assert!(!snapshot.entries.contains_key(b"untracked".as_slice()));
        assert_eq!(snapshot, capture(repo).await);
        assert_eq!(std::fs::read(repo.join("deleted")).unwrap(), b"delete me");
    }

    #[tokio::test]
    async fn unborn_rename_and_sha256() {
        for sha256 in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path();
            let init = std::process::Command::new("git")
                .args(["init", "--template=", "-b", "main"])
                .args(if sha256 {
                    vec!["--object-format=sha256"]
                } else {
                    vec![]
                })
                .current_dir(repo)
                .output()
                .unwrap();
            if !init.status.success() && sha256 {
                eprintln!(
                    "SHA256 unsupported: {}",
                    String::from_utf8_lossy(&init.stderr)
                );
                continue;
            }
            assert!(init.status.success());
            std::fs::write(repo.join("old"), b"content").unwrap();
            cmd(repo, &["add", "--", "old"]);
            let first = capture(repo).await;
            assert!(matches!(first.head, CapturedHead::Unborn { .. }));
            assert_eq!(
                first.entries[b"old".as_slice()].oid.len(),
                if sha256 { 64 } else { 40 }
            );
            cmd(repo, &["commit", "-m", "base"]);
            cmd(repo, &["mv", "--", "old", "new"]);
            let snapshot = capture(repo).await;
            assert_eq!(snapshot.changed[b"old".as_slice()], None);
            assert_eq!(
                snapshot.changed[b"new".as_slice()],
                Some(first.entries[b"old".as_slice()].clone())
            );
        }
    }

    #[tokio::test]
    async fn unmerged_and_intent_to_add_refuse_without_index_changes() {
        let dir = fixture();
        let repo = dir.path();
        std::fs::write(repo.join("file"), b"content").unwrap();
        cmd(repo, &["add", "-N", "--", "file"]);
        let before = preserved(repo);
        assert!(
            StagedIntentSnapshot::capture(repo, Instant::now() + Duration::from_secs(5))
                .await
                .is_err()
        );
        assert_eq!(before, preserved(repo));
        cmd(repo, &["add", "--", "file"]);
        let oid = String::from_utf8(cmd(repo, &["rev-parse", ":file"])).unwrap();
        cmd(repo, &["update-index", "--index-info"]); // empty stdin, no changes
        let mut child = std::process::Command::new("git")
            .args(["update-index", "--index-info"])
            .current_dir(repo)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                format!(
                    "0 {}\tfile\n100644 {} 1\tfile\n100644 {} 2\tfile\n",
                    "0".repeat(40),
                    oid.trim(),
                    oid.trim()
                )
                .as_bytes(),
            )
            .unwrap();
        assert!(child.wait().unwrap().success());
        let before = preserved(repo);
        let error = StagedIntentSnapshot::capture(repo, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unmerged"), "{error:#}");
        assert_eq!(before, preserved(repo));
    }

    #[cfg(target_os = "linux")]
    fn process_identity(pid: u32) -> Option<(String, String)> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields: Vec<_> = text.rsplit_once(") ")?.1.split_whitespace().collect();
        Some((fields[0].to_owned(), fields[19].to_owned()))
    }

    #[cfg(target_os = "linux")]
    async fn assert_terminated(pid: u32, start: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while process_identity(pid)
                .is_some_and(|(state, current)| state != "Z" && current == start)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned process remained active after cancellation");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn observation_deadline_and_caller_drop_terminate_actual_git_pid() {
        for cancel in [false, true] {
            let dir = fixture();
            let repo = dir.path();
            assert!(StagedIntentSnapshot::capture(repo, Instant::now())
                .await
                .is_err());
            // Git itself blocks opening HEAD; the observer does no synchronous
            // gitdir/index resolution, even for a stalled filesystem operation.
            let head = repo.join(".git/HEAD");
            std::fs::remove_file(&head).unwrap();
            let name = std::ffi::CString::new(head.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            let deadline = Instant::now() + Duration::from_secs(if cancel { 30 } else { 5 });
            let mut run = Box::pin(StagedIntentSnapshot::capture(repo, deadline));
            let find_git = async {
                loop {
                    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
                        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                            continue;
                        };
                        if std::fs::read_link(entry.path().join("cwd")).ok().as_deref()
                            == Some(repo)
                            && std::fs::read_to_string(entry.path().join("comm"))
                                .ok()
                                .as_deref()
                                == Some("git\n")
                        {
                            return (pid, process_identity(pid).unwrap().1);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            };
            let (pid, start) = tokio::select! {
                result = &mut run => panic!("blocked Git returned early: {result:?}"),
                found = tokio::time::timeout(Duration::from_secs(3), find_git) => found.unwrap(),
            };
            if cancel {
                drop(run);
            } else {
                let error = run.await.unwrap_err();
                assert!(format!("{error:#}").contains("deadline"));
            }
            assert_terminated(pid, &start).await;
            assert!(!repo.join(".git/index.lock").exists());
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn shared_git_runner_deadline_and_drop_terminate_owned_descendant() {
        // This exercises the exact process-group runner used by observation,
        // independently of its plumbing-only arguments (which spawn no helpers).
        for cancel in [false, true] {
            let dir = fixture();
            let repo = dir.path();
            let child = super::super::spawn_git_command_cancellable_stdin(
                repo,
                &[
                    "-c",
                    "alias.intent-probe=!sleep 30 & echo $! > helper.pid; wait",
                    "intent-probe",
                ],
                "intent cancellation fixture",
            )
            .unwrap();
            let git_pid = child.id().unwrap();
            let git_start = process_identity(git_pid).unwrap().1;
            let mut run = Box::pin(tokio::time::timeout(
                Duration::from_secs(if cancel { 30 } else { 5 }),
                super::super::run_git_captured_output(child, repo, "intent cancellation fixture"),
            ));
            let marker = repo.join("helper.pid");
            tokio::select! {
                result = &mut run => panic!("helper returned early: {result:?}"),
                ready = tokio::time::timeout(Duration::from_secs(3), async {
                    while !marker.exists() { tokio::time::sleep(Duration::from_millis(10)).await; }
                }) => ready.unwrap(),
            }
            let pid = std::fs::read_to_string(marker)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let start = process_identity(pid).unwrap().1;
            if cancel {
                drop(run);
            } else {
                assert!(run.await.is_err());
            }
            assert_terminated(git_pid, &git_start).await;
            assert_terminated(pid, &start).await;
        }
    }

    #[tokio::test]
    async fn unsupported_flags_sparse_and_split_index_refuse() {
        let dir = fixture();
        let repo = dir.path();
        std::fs::create_dir(repo.join("outside")).unwrap();
        std::fs::create_dir(repo.join("inside")).unwrap();
        std::fs::write(repo.join("file"), b"content").unwrap();
        std::fs::write(repo.join("outside/leaf"), b"outside").unwrap();
        std::fs::write(repo.join("inside/leaf"), b"inside").unwrap();
        cmd(repo, &["add", "--", "file", "outside/leaf", "inside/leaf"]);
        cmd(repo, &["commit", "-m", "base"]);
        for (flag, reset) in [
            ("--assume-unchanged", "--no-assume-unchanged"),
            ("--skip-worktree", "--no-skip-worktree"),
        ] {
            cmd(repo, &["update-index", flag, "--", "file"]);
            let before = preserved(repo);
            let error =
                StagedIntentSnapshot::capture(repo, Instant::now() + Duration::from_secs(5))
                    .await
                    .unwrap_err();
            assert!(
                error.to_string().contains("unsupported index flags"),
                "{error:#}"
            );
            assert_eq!(before, preserved(repo));
            cmd(repo, &["update-index", reset, "--", "file"]);
        }
        std::fs::write(repo.join("file"), b"staged content").unwrap();
        cmd(repo, &["add", "--", "file"]);
        assert!(capture(repo).await.changed.contains_key(b"file".as_slice()));
        cmd(repo, &["update-index", "--split-index"]);
        assert!(std::fs::read_dir(repo.join(".git"))
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("sharedindex.")));
        let before = preserved(repo);
        // Git supports split indexes, but its nonzero internal flags are not
        // admitted by this deliberately conservative observation API.
        let error = StagedIntentSnapshot::capture(repo, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("unsupported index flags"),
            "{error:#}"
        );
        assert_eq!(before, preserved(repo));
        cmd(repo, &["update-index", "--no-split-index"]);
        cmd(
            repo,
            &["sparse-checkout", "init", "--cone", "--sparse-index"],
        );
        cmd(repo, &["sparse-checkout", "set", "inside"]);
        let listing = cmd(repo, &["ls-files", "--stage", "--sparse", "-z"]);
        assert!(listing.windows(7).any(|w| w == b"040000 "));
        let before = preserved(repo);
        let error = StagedIntentSnapshot::capture(repo, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unsupported"), "{error:#}");
        assert_eq!(before, preserved(repo));
    }

    #[test]
    fn index_lock_contention_and_replaced_inode_cleanup() {
        let dir = fixture();
        let repo = dir.path();
        let lock = super::super::status::IndexLock::acquire(repo).unwrap();
        assert!(lock.is_owned());
        assert!(super::super::status::IndexLock::acquire(repo).is_err());
        let path = repo.join(".git/index.lock");
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"foreign").unwrap();
        assert!(!lock.is_owned());
        drop(lock);
        assert_eq!(std::fs::read(&path).unwrap(), b"foreign");
        std::fs::remove_file(&path).unwrap();
        drop(super::super::status::IndexLock::acquire(repo).unwrap());
        assert!(!path.exists());
    }
}
