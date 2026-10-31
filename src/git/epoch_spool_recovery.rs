//! Independent, complete recovery records. The caller must hold spool ownership
//! across this call and any later removal. This module NEVER removes a spool.
//! One full closure pack per record is intentional: this is retention, not a
//! claim of net space savings. Unsupported recovery semantics fail closed.

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug)]
#[allow(dead_code)] // Read by the future qualified caller, not the current reaper.
pub(super) struct DurableReceipt {
    pub record: PathBuf,
    pub manifest_sha256: String,
}

fn git(repo: &Path) -> Command {
    let mut c = Command::new("git");
    // In particular, do not inherit alternates, replacement refs, index paths,
    // config injection, or lazy-fetch credentials from the daemon environment.
    let path = std::env::var_os("PATH").unwrap_or_default();
    c.env_clear()
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("--no-replace-objects")
        .current_dir(repo)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

async fn capture<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    const LIMIT: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        pipe.take(LIMIT + 1).read_to_end(&mut bytes).await?;
        ensure!(bytes.len() as u64 <= LIMIT, "recovery Git output limit exceeded");
    }
    Ok(bytes)
}

async fn run(mut c: Command, seconds: u64) -> Result<Vec<u8>> {
    // Keep file-backed stdout intact. Each command here is audited Git plumbing
    // in a template-free bare builder (or read-only rev-parse), not hook/filter
    // execution. On timeout JOIN the killed process before releasing its files.
    // The enclosing preparation task outlives cancellation of its caller.
    let mut child = c.spawn()?;
    let stdout = tokio::spawn(capture(child.stdout.take()));
    let stderr = tokio::spawn(capture(child.stderr.take()));
    let waited = tokio::time::timeout(
        std::time::Duration::from_secs(seconds),
        child.wait(),
    ).await;
    let timed_out = waited.is_err();
    let status = match waited {
        Ok(result) => result?,
        Err(_) => {
            // A pending uninterruptible process retains the preparation task and
            // its temporary storage. The caller's whole-transaction deadline
            // still returns without granting any removal authority.
            child.start_kill()?;
            child.wait().await?
        }
    };
    let out = stdout.await??;
    let err = stderr.await??;
    ensure!(!timed_out, "recovery Git timeout; process termination joined");
    ensure!(status.success(), "recovery Git failed: {}", String::from_utf8_lossy(&err));
    Ok(out)
}

async fn text(repo: &Path, args: &[&str], timeout: u64) -> Result<String> {
    let mut c = git(repo);
    c.args(args);
    Ok(String::from_utf8(run(c, timeout).await?)?
        .trim_end_matches('\n')
        .to_owned())
}

fn regular(path: &Path) -> Result<File> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "not a regular file: {}",
        path.display()
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(file.metadata()?.is_file(), "file changed type");
    Ok(file)
}

fn copy_regular(from: &Path, to: &Path) -> Result<()> {
    let mut source = regular(from)?;
    let mut target = OpenOptions::new().write(true).create_new(true).open(to)?;
    std::io::copy(&mut source, &mut target)?;
    Ok(())
}

fn oid(s: &str, width: usize) -> bool {
    s.len() == width
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Reject dependencies that copying a single index cannot preserve. Parsing the
/// extension boundary ourselves prevents Git silently resolving a split index.
fn supported_index(path: &Path, width: usize) -> Result<()> {
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() >= 12 + width / 2 && &bytes[..4] == b"DIRC",
        "invalid index"
    );
    let number = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .context("truncated index")?
                .try_into()?,
        ))
    };
    let version = number(4)?;
    ensure!(version == 2 || version == 3, "only index v2/v3 supported");
    let end = bytes.len() - width / 2;
    let mut at = 12;
    for _ in 0..number(8)? {
        let start = at;
        let flags_at = at + 40 + width / 2;
        let flags = u16::from_be_bytes(
            bytes
                .get(flags_at..flags_at + 2)
                .context("truncated flags")?
                .try_into()?,
        );
        ensure!(flags & 0x3000 == 0, "unmerged index unsupported");
        ensure!(flags & 0x4000 == 0, "extended index flags unsupported");
        at = flags_at + 2;
        while at < end && bytes[at] != 0 {
            at += 1;
        }
        ensure!(at < end, "unterminated index path");
        at = start + (at + 1 - start).div_ceil(8) * 8;
        ensure!(at <= end, "truncated index padding");
    }
    while at < end {
        let signature = bytes.get(at..at + 4).context("truncated extension")?;
        ensure!(
            signature == b"TREE",
            "unsupported index extension (including split index)"
        );
        at = at
            .checked_add(8 + number(at + 4)? as usize)
            .context("index extension overflow")?;
        ensure!(at <= end, "truncated extension data");
    }
    ensure!(at == end, "invalid index boundary");
    Ok(())
}

fn pack_count(path: &Path) -> Result<usize> {
    let mut header = [0; 12];
    regular(path)?.read_exact(&mut header)?;
    ensure!(&header[..4] == b"PACK", "invalid pack header");
    let version = u32::from_be_bytes(header[4..8].try_into()?);
    ensure!(version == 2 || version == 3, "unsupported pack version");
    Ok(u32::from_be_bytes(header[8..12].try_into()?) as usize)
}

fn listed_objects(path: &Path, width: usize, expected: usize) -> Result<BTreeSet<String>> {
    let mut objects = BTreeSet::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.first().is_some_and(|s| oid(s, width)) {
            ensure!(
                fields.len() == 5 || fields.len() == 7,
                "invalid verify-pack object row"
            );
            ensure!(
                ["blob", "tree", "commit", "tag"].contains(&fields[1]),
                "unknown object type"
            );
            for field in &fields[2..5] {
                field.parse::<u64>()?;
            }
            if fields.len() == 7 {
                fields[5].parse::<u64>()?;
                ensure!(oid(fields[6], width), "invalid delta base");
            }
            ensure!(
                objects.insert(fields[0].to_owned()),
                "duplicate OID within pack"
            );
        } else {
            // Summary lines are not object rows; malformed rows are never
            // silently accepted because the per-pack header count must agree.
            ensure!(
                line.ends_with(": ok")
                    || line.starts_with("non delta: ")
                    || line.starts_with("chain length = "),
                "unknown verify-pack output"
            );
        }
    }
    ensure!(
        objects.len() == expected,
        "pack header/enumeration mismatch: {} listed, {expected} declared",
        objects.len()
    );
    Ok(objects)
}

async fn init(path: &Path, format: &str, timeout: u64) -> Result<()> {
    fs::create_dir(path)?;
    text(
        path,
        &[
            "init",
            "--bare",
            "--template=",
            &format!("--object-format={format}"),
            ".",
        ],
        timeout,
    )
    .await?;
    Ok(())
}

/// Verify without alternates or a source ODB. fsck --full reads and hashes every
/// object's contents and validates closure (gitlinks deliberately have no local
/// commit obligation). Batch reads additionally prove every required root exists.
async fn verify(store: &Path, roots: &Path, timeout: u64) -> Result<()> {
    ensure!(
        !store.join("objects/info/alternates").exists(),
        "published alternates forbidden"
    );
    ensure!(
        !store.join("shallow").exists(),
        "shallow recovery forbidden"
    );
    let mut c = git(store);
    c.args(["cat-file", "--batch"])
        .stdin(File::open(roots)?)
        .stdout(Stdio::null());
    run(c, timeout).await?;
    let mut c = git(store);
    c.args(["cat-file", "--batch-check=%(objectname) %(objecttype)"])
        .stdin(File::open(roots)?);
    let result = String::from_utf8(run(c, timeout).await?)?;
    let requested = fs::read_to_string(roots)?;
    let lines: Vec<_> = result.lines().collect();
    ensure!(
        lines.len() == requested.lines().count(),
        "missing batch results"
    );
    for (root, line) in requested.lines().zip(lines) {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(
            fields.len() == 2
                && fields[0] == root
                && ["blob", "tree", "commit", "tag"].contains(&fields[1]),
            "missing recovery root"
        );
    }
    text(
        store,
        &["fsck", "--full", "--strict", "--no-reflogs"],
        timeout,
    )
    .await?;
    Ok(())
}

fn sync_tree(path: &Path, base: &Path, manifest: &mut String) -> Result<()> {
    let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            sync_tree(&path, base, manifest)?;
        } else {
            let mut file = regular(&path)?;
            let mut hash = Sha256::new();
            let mut buf = [0; 65536];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
            }
            file.sync_all()?;
            // JSON escaping makes arbitrary metadata names unambiguous.
            manifest.push_str(&format!(
                "{:x} {}\n",
                hash.finalize(),
                serde_json::to_string(&path.strip_prefix(base)?.as_os_str().as_encoded_bytes())?
            ));
        }
    }
    File::open(path)?.sync_all()?;
    Ok(())
}

/// Caller must have established irreversible producer quiescence and own the
/// source lease. A receipt grants no removal authority without final identity
/// and preservation checks. Cancellation detaches preparation, never destroys
/// storage beneath a running Git process; an abandoned task may retain a record
/// but NEVER removes its source. The caller has a whole-transaction deadline.
#[allow(dead_code)] // Deliberately not connected to destructive reclamation.
pub(super) async fn prepare_record(
    repo: &Path,
    spool: &Path,
    timeout: u64,
) -> Result<DurableReceipt> {
    prepare_with_fault(repo, spool, timeout, Fault::None).await
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    #[cfg(test)]
    CorruptBeforeVerify,
    #[cfg(test)]
    BeforeReceipt,
    #[cfg(test)]
    AfterRetain,
}

async fn prepare_with_fault(repo: &Path, spool: &Path, seconds: u64, fault: Fault) -> Result<DurableReceipt> {
    let (repo, spool) = (repo.to_path_buf(), spool.to_path_buf());
    let preparation = tokio::spawn(async move {
        prepare_inner(&repo, &spool, seconds, fault).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(seconds), preparation)
        .await.context("recovery transaction deadline; source preserved")?
        .context("recovery preparation task failed")?
}

async fn prepare_inner(repo: &Path, spool: &Path, timeout: u64, _fault: Fault) -> Result<DurableReceipt> {
    let gitdir = PathBuf::from(text(repo, &["rev-parse", "--absolute-git-dir"], timeout).await?);
    ensure!(
        fs::symlink_metadata(spool)?.is_dir(),
        "spool must be a real directory"
    );
    ensure!(
        spool
            .parent()
            .context("spool has no parent")?
            .canonicalize()?
            == gitdir.canonicalize()?,
        "spool outside originating gitdir"
    );
    ensure!(
        !spool.join("preserve-rejected").exists() && !spool.join("original-index").exists(),
        "preservation marker"
    );
    let format = text(repo, &["rev-parse", "--show-object-format"], timeout).await?;
    let width = match format.as_str() {
        "sha1" => 40,
        "sha256" => 64,
        _ => bail!("unknown object format"),
    };
    let objects = PathBuf::from(
        text(
            repo,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "objects",
            ],
            timeout,
        )
        .await?,
    );
    // Unusual alternate-path syntax is refused instead of incorrectly quoting it.
    ensure!(
        !objects
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|b| [b':', b'\n', b'"'].contains(b)),
        "unsupported object directory path"
    );
    let rescue = gitdir.join("dracon-rescue");
    match fs::create_dir(&rescue) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    ensure!(
        fs::symlink_metadata(&rescue)?.is_dir(),
        "recovery area is not a real directory"
    );
    File::open(&gitdir)?.sync_all()?;
    let work = tempfile::Builder::new()
        .prefix(".prepare-")
        .tempdir_in(&rescue)?;
    let record = tempfile::Builder::new()
        .prefix("record-")
        .tempdir_in(&rescue)?;
    let metadata = record.path().join("metadata");
    fs::create_dir(&metadata)?;
    fs::write(
        record.path().join("source.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "originating_gitdir_bytes": gitdir.as_os_str().as_encoded_bytes(),
            "spool_path_bytes": spool.as_os_str().as_encoded_bytes(),
            "object_format": format,
        }))?,
    )?;
    let inputs = work.path().join("inputs");
    fs::create_dir(&inputs)?;
    let mut packs = Vec::new();
    for entry in fs::read_dir(spool)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "unknown directory or symlink in spool"
        );
        let name = entry.file_name();
        ensure!(
            !name.as_encoded_bytes().starts_with(b"sharedindex."),
            "split index dependency unsupported"
        );
        if entry.path().extension().is_some_and(|e| e == "pack") {
            copy_regular(&entry.path(), &inputs.join(&name))?;
            packs.push(name);
        } else {
            copy_regular(&entry.path(), &metadata.join(&name))?;
        }
    }
    ensure!(metadata.join("index").is_file(), "spool index missing");
    supported_index(&metadata.join("index"), width)?;
    let build = work.path().join("build.git");
    init(&build, &format, timeout).await?;
    fs::copy(metadata.join("index"), work.path().join("index"))?;
    let packdir = build.join("objects/pack");
    let mut roots = BTreeSet::new();
    for (i, name) in packs.iter().enumerate() {
        let pack = packdir.join(format!("source-{i}.pack"));
        copy_regular(&inputs.join(name), &pack)?;
        let count = pack_count(&pack)?;
        let mut c = git(&build);
        c.arg("index-pack").arg(&pack);
        run(c, timeout).await?;
        let listing = work.path().join(format!("listing-{i}"));
        let mut c = git(&build);
        c.arg("verify-pack")
            .arg("-v")
            .arg(pack.with_extension("idx"))
            .stdout(File::create(&listing)?);
        run(c, timeout).await?;
        roots.extend(listed_objects(&listing, width, count)?);
    }
    // Only the temporary builder may consult the main ODB. Published config and
    // object storage never contain that dependency.
    let index = work.path().join("index");
    let mut c = git(&build);
    c.args(["ls-files", "--stage", "-z"])
        .env("GIT_INDEX_FILE", &index)
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects);
    let entries = run(c, timeout).await?;
    for entry in entries.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let header = entry
            .split(|b| *b == b'\t')
            .next()
            .context("missing index header")?;
        let fields: Vec<_> = std::str::from_utf8(header)?.split_whitespace().collect();
        ensure!(
            fields.len() == 3 && fields[2] == "0" && oid(fields[1], width),
            "unsupported index entry"
        );
        ensure!(
            fields[1].bytes().any(|b| b != b'0'),
            "intent-to-add unsupported"
        );
        if fields[0] != "160000" {
            roots.insert(fields[1].to_owned());
        }
    }
    let mut c = git(&build);
    c.arg("write-tree")
        .env("GIT_INDEX_FILE", &index)
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects);
    let tree = String::from_utf8(run(c, timeout).await?)?.trim().to_owned();
    ensure!(oid(&tree, width), "invalid index tree");
    roots.insert(tree);
    let roots_path = record.path().join("roots");
    fs::write(
        &roots_path,
        roots.into_iter().map(|s| s + "\n").collect::<String>(),
    )?;
    let store = record.path().join("store.git");
    init(&store, &format, timeout).await?;
    let prefix = store.join("objects/pack/pack");
    let mut c = git(&build);
    c.args([
        "pack-objects",
        "--revs",
        "--no-reuse-object",
        "--no-reuse-delta",
    ])
    .arg(&prefix)
    .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects)
    .stdin(File::open(&roots_path)?);
    let hash = String::from_utf8(run(c, timeout).await?)?.trim().to_owned();
    ensure!(oid(&hash, width), "invalid output pack identity");
    fs::write(
        store.join(format!("objects/pack/pack-{hash}.keep")),
        b"Independent epoch recovery; do not prune\n",
    )?;
    #[cfg(test)]
    if matches!(_fault, Fault::CorruptBeforeVerify) {
        let pack = store.join(format!("objects/pack/pack-{hash}.pack"));
        let mut bytes = fs::read(&pack)?;
        ensure!(bytes.len() > 15, "fault fixture pack too short");
        bytes[15] ^= 0x55;
        fs::write(pack, bytes)?;
    }
    verify(&store, &roots_path, timeout).await?;
    let mut manifest = String::from("dracon-epoch-recovery-v1\n");
    sync_tree(record.path(), record.path(), &mut manifest)?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(manifest.as_bytes()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(record.path().join("manifest.sha256"))?;
    file.write_all(manifest.as_bytes())?;
    file.sync_all()?;
    let mut receipt = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(record.path().join("receipt.pending"))?;
    writeln!(receipt, "dracon-epoch-recovery-v1 {manifest_sha256}")?;
    receipt.sync_all()?;
    File::open(record.path())?.sync_all()?;
    #[cfg(test)]
    if matches!(_fault, Fault::BeforeReceipt) {
        bail!("injected failure before retaining prepared record");
    }
    // Retain before publication: no later error may remove a published receipt.
    // An interrupted publication can leave a prepared directory without one.
    let record = record.keep();
    File::open(&rescue)?.sync_all()?;
    #[cfg(test)]
    if matches!(_fault, Fault::AfterRetain) {
        bail!("injected failure after retaining record, before receipt publication");
    }
    // Hard-link publication is create-only and exposes only complete bytes.
    fs::hard_link(record.join("receipt.pending"), record.join("receipt"))?;
    File::open(&record)?.sync_all()?;
    Ok(DurableReceipt {
        record,
        manifest_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture(format: &str) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("main");
        fs::create_dir(&repo).unwrap();
        text(
            &repo,
            &["init", "--template=", &format!("--object-format={format}")],
            10,
        )
        .await
        .unwrap();
        fs::write(
            repo.join("file with spaces"),
            b"original recovery payload\n",
        )
        .unwrap();
        text(&repo, &["add", "--", "file with spaces"], 10)
            .await
            .unwrap();
        let blob = text(&repo, &["rev-parse", ":file with spaces"], 10)
            .await
            .unwrap();
        let spool = repo.join(".git/dracon-staging-test");
        fs::create_dir(&spool).unwrap();
        fs::copy(repo.join(".git/index"), spool.join("index")).unwrap();
        fs::write(spool.join("reclaim-owner.lock"), b"").unwrap();
        fs::write(spool.join("notes"), b"exact metadata\0\xff").unwrap();
        (root, repo, spool, blob)
    }

    async fn pack(repo: &Path, spool: &Path, roots: &[String], label: &str) {
        let input = spool.join(format!("{label}-roots"));
        fs::write(&input, roots.join("\n") + "\n").unwrap();
        let mut c = git(repo);
        c.arg("pack-objects")
            .arg(spool.join(label))
            .stdin(File::open(&input).unwrap());
        run(c, 10).await.unwrap();
    }

    #[tokio::test]
    async fn index_only_reconstructs_exact_bytes_and_paths_without_main_or_spool() {
        for format in ["sha1", "sha256"] {
            let (root, repo, spool, blob) = fixture(format).await;
            let original = fs::read(spool.join("index")).unwrap();
            let mut c = git(&repo);
            c.args(["ls-files", "--stage", "-z"])
                .env("GIT_INDEX_FILE", spool.join("index"));
            let mapping = run(c, 10).await.unwrap();
            let receipt = prepare_record(&repo, &spool, 10).await.unwrap();
            assert_eq!(receipt.manifest_sha256.len(), 64);
            assert!(spool.exists(), "engine never removes its source");
            let saved = root.path().join("saved-record");
            fs::rename(&receipt.record, &saved).unwrap();
            fs::rename(&repo, root.path().join("unavailable-main")).unwrap();
            assert!(!repo.exists() && !spool.exists());
            assert_eq!(fs::read(saved.join("metadata/index")).unwrap(), original);
            assert_eq!(
                fs::read(saved.join("metadata/notes")).unwrap(),
                b"exact metadata\0\xff"
            );
            let store = saved.join("store.git");
            let mut c = git(&store);
            c.args(["ls-files", "--stage", "-z"])
                .env("GIT_INDEX_FILE", saved.join("metadata/index"));
            assert_eq!(run(c, 10).await.unwrap(), mapping);
            assert_eq!(
                text(&store, &["cat-file", "blob", &blob], 10)
                    .await
                    .unwrap(),
                "original recovery payload"
            );
            verify(&store, &saved.join("roots"), 10).await.unwrap();
            assert!(!store.join("objects/info/alternates").exists());
            assert_eq!(text(&store, &["remote"], 10).await.unwrap(), "");
        }
    }

    #[tokio::test]
    async fn mixed_overlapping_packs_keep_dangling_objects_after_main_gc() {
        for format in ["sha1", "sha256"] {
            let (_root, repo, spool, blob) = fixture(format).await;
            fs::write(repo.join("dangling"), b"unique dangling object").unwrap();
            let unique = text(&repo, &["hash-object", "-w", "dangling"], 10)
                .await
                .unwrap();
            let tree = text(&repo, &["write-tree"], 10).await.unwrap();
            let commit_data = format!("tree {tree}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nrecovery commit\n");
            fs::write(repo.join("commit-data"), &commit_data).unwrap();
            let commit = text(
                &repo,
                &["hash-object", "-t", "commit", "-w", "commit-data"],
                10,
            )
            .await
            .unwrap();
            fs::write(repo.join("tag-data"), format!("object {commit}\ntype commit\ntag recovery\ntagger Fixture <fixture@example.invalid> 1 +0000\n\nrecovery tag\n")).unwrap();
            let tag = text(&repo, &["hash-object", "-t", "tag", "-w", "tag-data"], 10)
                .await
                .unwrap();
            // Retain the tag only, not its commit/tree: the engine must supply
            // that closure from the temporary main alternate and retain it.
            pack(
                &repo,
                &spool,
                &[blob.clone(), unique.clone(), tag.clone()],
                "retained-one",
            )
            .await;
            pack(&repo, &spool, std::slice::from_ref(&blob), "retained-two").await;
            let receipt = prepare_record(&repo, &spool, 10).await.unwrap();
            text(&repo, &["gc", "--prune=now"], 10).await.unwrap();
            let store = receipt.record.join("store.git");
            text(&store, &["gc", "--prune=now"], 10).await.unwrap();
            assert_eq!(
                text(&store, &["cat-file", "blob", &unique], 10)
                    .await
                    .unwrap(),
                "unique dangling object"
            );
            assert_eq!(
                text(&store, &["cat-file", "commit", &commit], 10)
                    .await
                    .unwrap(),
                commit_data.trim_end()
            );
            assert_eq!(
                text(&store, &["rev-parse", &format!("{tag}^{{tree}}")], 10)
                    .await
                    .unwrap(),
                tree
            );
            verify(&store, &receipt.record.join("roots"), 10)
                .await
                .unwrap();
            assert!(spool.join("index").exists());
        }
    }

    // Protect the actual prepublication gate: corruption must fail preparation,
    // not merely be detectable by a verifier invoked after a successful receipt.
    #[tokio::test]
    async fn post_write_corruption_and_publication_faults_preserve_source() {
        for fault in [Fault::CorruptBeforeVerify, Fault::BeforeReceipt, Fault::AfterRetain] {
            let (_root, repo, spool, _) = fixture("sha1").await;
            let original = fs::read(spool.join("index")).unwrap();
            assert!(prepare_with_fault(&repo, &spool, 20, fault).await.is_err());
            assert_eq!(fs::read(spool.join("index")).unwrap(), original);
            for record in fs::read_dir(repo.join(".git/dracon-rescue")).unwrap() {
                assert!(!record.unwrap().path().join("receipt").exists(), "failed preparation must not authorize removal");
            }
        }
    }

    #[tokio::test]
    async fn isolated_verification_rejects_corrupt_pack_with_source_intact() {
        let (_root, repo, spool, _) = fixture("sha1").await;
        let source = fs::read(spool.join("index")).unwrap();
        let receipt = prepare_record(&repo, &spool, 10).await.unwrap();
        let store = receipt.record.join("store.git");
        let pack = fs::read_dir(store.join("objects/pack"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "pack"))
            .unwrap();
        let mut bytes = fs::read(&pack).unwrap();
        bytes[15] ^= 0x55;
        let mut permissions = fs::metadata(&pack).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        fs::set_permissions(&pack, permissions).unwrap();
        fs::write(pack, bytes).unwrap();
        assert!(verify(&store, &receipt.record.join("roots"), 10)
            .await
            .is_err());
        assert_eq!(fs::read(spool.join("index")).unwrap(), source);
    }

    #[tokio::test]
    async fn corrupt_input_and_unknown_directories_refuse_without_receipt() {
        let (_root, repo, spool, _) = fixture("sha1").await;
        fs::write(
            spool.join("retained-bad.pack"),
            b"PACK\0\0\0\x02\0\0\0\x01broken",
        )
        .unwrap();
        assert!(prepare_record(&repo, &spool, 10).await.is_err());
        assert_eq!(
            fs::read(spool.join("retained-bad.pack")).unwrap(),
            b"PACK\0\0\0\x02\0\0\0\x01broken"
        );
        fs::create_dir(spool.join("unknown")).unwrap();
        assert!(prepare_record(&repo, &spool, 10).await.is_err());
        assert!(spool.join("index").exists());
        assert!(fs::read_dir(repo.join(".git/dracon-rescue"))
            .unwrap()
            .next()
            .is_none());
    }

    #[tokio::test]
    async fn split_index_and_missing_index_object_refuse_preserving_sources() {
        let (_root, repo, spool, blob) = fixture("sha1").await;
        let loose = repo.join(".git/objects").join(&blob[..2]).join(&blob[2..]);
        let moved = loose.with_extension("unavailable");
        fs::rename(&loose, &moved).unwrap();
        let original = fs::read(spool.join("index")).unwrap();
        assert!(prepare_record(&repo, &spool, 10).await.is_err());
        assert_eq!(fs::read(spool.join("index")).unwrap(), original);
        fs::rename(&moved, &loose).unwrap();
        text(&repo, &["update-index", "--split-index"], 10)
            .await
            .unwrap();
        fs::copy(repo.join(".git/index"), spool.join("index")).unwrap();
        let split = fs::read(spool.join("index")).unwrap();
        assert!(prepare_record(&repo, &spool, 10).await.is_err());
        assert_eq!(fs::read(spool.join("index")).unwrap(), split);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_metadata_is_refused_without_following_it() {
        let (_root, repo, spool, _) = fixture("sha1").await;
        std::os::unix::fs::symlink(repo.join("file with spaces"), spool.join("alias")).unwrap();
        assert!(prepare_record(&repo, &spool, 10).await.is_err());
        assert!(fs::symlink_metadata(spool.join("alias"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
