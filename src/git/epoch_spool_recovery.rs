//! Independent, complete recovery records. The caller must hold spool ownership
//! across this call and any later removal. This module NEVER removes a spool.
//! Records share immutable loose-object inodes, without alternates. Each has
//! independent GC roots and complete metadata; physical savings are measured,
//! not assumed. Unsupported recovery semantics fail closed.

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[path = "epoch_spool_cas.rs"]
mod cas;
#[path = "epoch_spool_source.rs"]
mod source;
#[path = "epoch_spool_unique.rs"]
mod unique;

#[derive(Debug)]
pub(super) struct DurableReceipt {
    pub record: PathBuf,
    pub manifest_sha256: String,
    pub unique_objects: usize,
    pub unique_uncompressed_bytes: u64,
    pub rescue_pack_bytes: u64,
    pub rescue_pack: Option<PathBuf>,
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
        ensure!(
            bytes.len() as u64 <= LIMIT,
            "recovery Git output limit exceeded"
        );
    }
    Ok(bytes)
}

async fn join_child(child: &mut tokio::process::Child) -> std::process::ExitStatus {
    loop {
        match child.wait().await {
            Ok(status) => return status,
            Err(error) => {
                // No completion proof: keep preparation storage, even if the
                // caller has already timed out. Never guess that a failed wait
                // means a process can no longer use its files.
                eprintln!("recovery child settlement uncertain: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
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
        join_child(&mut child),
    )
    .await;
    let timed_out = waited.is_err();
    let status = match waited {
        Ok(status) => status,
        Err(_) => {
            // A pending uninterruptible process retains the preparation task and
            // its temporary storage. The caller's whole-transaction deadline
            // still returns without granting any removal authority.
            if let Err(error) = child.start_kill() {
                eprintln!(
                    "recovery child kill request failed; retaining storage until joined: {error}"
                );
            }
            join_child(&mut child).await
        }
    };
    let out = stdout.await??;
    let err = stderr.await??;
    ensure!(
        !timed_out,
        "recovery Git timeout; process termination joined"
    );
    ensure!(
        status.success(),
        "recovery Git failed: {}",
        String::from_utf8_lossy(&err)
    );
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
    target.set_permissions(source.metadata()?.permissions())?;
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

/// Validate semantic retention after physical ODB layout changes (for example
/// ordinary Git GC). Receipts never rely on obsolete pack/loose filenames.
pub(super) async fn verify_receipt(record: &Path, expected: &str, timeout: u64) -> Result<()> {
    let mut receipt = String::new();
    regular(&record.join("receipt"))?.read_to_string(&mut receipt)?;
    ensure!(
        receipt == format!("dracon-epoch-recovery-v2 {expected}\n"),
        "published receipt mismatch"
    );
    let mut manifest = Vec::new();
    regular(&record.join("manifest.sha256"))?.read_to_end(&mut manifest)?;
    ensure!(
        format!("{:x}", Sha256::digest(&manifest)) == expected,
        "receipt manifest digest mismatch"
    );
    let manifest = std::str::from_utf8(&manifest)?;
    let mut lines = manifest.lines();
    ensure!(
        lines.next() == Some("dracon-epoch-recovery-v2"),
        "unsupported recovery receipt"
    );
    let mut paths = BTreeSet::new();
    for line in lines {
        let (digest, encoded) = line
            .split_once(' ')
            .context("invalid recovery manifest row")?;
        ensure!(oid(digest, 64), "invalid metadata digest");
        let bytes: Vec<u8> = serde_json::from_str(encoded)?;
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(bytes))
        };
        #[cfg(not(unix))]
        let path = PathBuf::from(String::from_utf8(bytes)?);
        ensure!(
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "unsafe manifest path"
        );
        ensure!(
            !path.starts_with("store.git") && paths.insert(path.clone()),
            "invalid semantic manifest dependency"
        );
        let mut source = regular(&record.join(&path))?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let length = source.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            hash.update(&buffer[..length]);
        }
        ensure!(
            format!("{:x}", hash.finalize()) == digest,
            "recovery metadata changed"
        );
    }
    for required in ["source.json", "roots", "objects.tsv", "metadata/index"] {
        ensure!(
            paths.contains(Path::new(required)),
            "required semantic retention dependency missing"
        );
    }
    let store = record.join("store.git");
    verify(&store, &record.join("roots"), timeout).await?;
    let actual = text(
        &store,
        &[
            "cat-file",
            "--batch-all-objects",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ],
        timeout,
    )
    .await?;
    let actual: BTreeSet<_> = actual.lines().collect();
    for required in fs::read_to_string(record.join("objects.tsv"))?.lines() {
        ensure!(
            actual.contains(required),
            "recovery object/type/size missing"
        );
    }
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
            let relative = path.strip_prefix(base)?;
            // ODB layouts/packed refs may change under ordinary record GC.
            // Version 2 receipts bind immutable metadata plus semantic roots
            // and objects.tsv, not mutable physical Git filenames. Final
            // isolated fsck verifies those object contents before publication.
            if !relative.starts_with("store.git") {
                manifest.push_str(&format!(
                    "{:x} {}\n",
                    hash.finalize(),
                    serde_json::to_string(&relative.as_os_str().as_encoded_bytes())?
                ));
            }
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
#[cfg(test)] // Direct component fixtures; production always transfers its fence.
pub(super) async fn prepare_record(
    repo: &Path,
    spool: &Path,
    timeout: u64,
) -> Result<DurableReceipt> {
    prepare_with_fault(repo, spool, timeout, Fault::None).await
}

#[derive(Clone, Copy)]
pub(super) enum Fault {
    None,
    #[cfg(test)]
    CorruptBeforeVerify,
    #[cfg(test)]
    CorruptUniqueBeforeVerify,
    #[cfg(test)]
    BeforeReceipt,
    #[cfg(test)]
    AfterRetain,
    #[cfg(test)]
    SlowBeforeVerify,
}

#[cfg(test)]
async fn prepare_with_fault(
    repo: &Path,
    spool: &Path,
    seconds: u64,
    fault: Fault,
) -> Result<DurableReceipt> {
    Ok(prepare_guarded(repo, spool, seconds, fault, ()).await?.0)
}

/// Preserve the actual kernel fence across requester cancellation too. Keeping
/// only temporary storage alive would permit successive timed-out reapers to
/// prepare the same source concurrently after their caller dropped its lock.
pub(super) async fn prepare_owned_record(
    repo: &Path,
    spool: &Path,
    seconds: u64,
    owner: super::ownership::ReclaimOwner,
) -> Result<(DurableReceipt, super::ownership::ReclaimOwner)> {
    prepare_guarded(repo, spool, seconds, Fault::None, owner).await
}

pub(super) async fn verify_source_owned(
    record: &Path,
    spool: &Path,
    owner: super::ownership::ReclaimOwner,
) -> Result<super::ownership::ReclaimOwner> {
    source::verify_owned(record, spool, owner).await
}

pub(super) fn final_source_check(record: &Path, spool: &Path) -> Result<()> {
    source::final_check(record, spool)
}

#[cfg(test)]
pub(super) async fn prepare_owned_fault(
    repo: &Path,
    spool: &Path,
    seconds: u64,
    owner: super::ownership::ReclaimOwner,
    fault: Fault,
) -> Result<(DurableReceipt, super::ownership::ReclaimOwner)> {
    prepare_guarded(repo, spool, seconds, fault, owner).await
}

async fn prepare_guarded<G: Send + 'static>(
    repo: &Path,
    spool: &Path,
    seconds: u64,
    fault: Fault,
    guard: G,
) -> Result<(DurableReceipt, G)> {
    let (repo, spool) = (repo.to_path_buf(), spool.to_path_buf());
    let preparation = tokio::spawn(async move {
        let receipt = prepare_inner(&repo, &spool, seconds, fault).await?;
        Ok((receipt, guard))
    });
    tokio::time::timeout(std::time::Duration::from_secs(seconds), preparation)
        .await
        .context("recovery transaction deadline; source preserved")?
        .context("recovery preparation task failed")?
}

async fn prepare_inner(
    repo: &Path,
    spool: &Path,
    timeout: u64,
    _fault: Fault,
) -> Result<DurableReceipt> {
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
    let source_snapshot = source::capture(spool).await?;
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
    source::verify_copies(&source_snapshot, &metadata, &inputs).await?;
    fs::write(
        record.path().join("source-fingerprint.json"),
        serde_json::to_vec(&source_snapshot)?,
    )?;
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
    fs::write(
        record.path().join("source-oids"),
        roots
            .iter()
            .map(|oid| oid.clone() + "\n")
            .collect::<String>(),
    )?;
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
    cas::share(work.path(), record.path(), &rescue, &format, &hash, timeout).await?;
    #[cfg(test)]
    if matches!(_fault, Fault::SlowBeforeVerify) {
        let mut probe = Command::new(std::env::current_exe()?);
        probe
            .args([
                "--exact",
                "git::epoch_spool::recovery::tests::recovery_writer_probe",
            ])
            .env("DRACON_RECOVERY_WRITER_PROBE", record.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run(probe, timeout).await?;
    }
    #[cfg(test)]
    if matches!(_fault, Fault::CorruptBeforeVerify) {
        let inventory = fs::read_to_string(record.path().join("objects.tsv"))?;
        let object = inventory
            .split_whitespace()
            .next()
            .context("fault fixture empty")?;
        let path = store.join("objects").join(&object[..2]).join(&object[2..]);
        let mut bytes = fs::read(&path)?;
        ensure!(bytes.len() > 15, "fault fixture object too short");
        bytes[15] ^= 0x55;
        fs::write(path, bytes)?;
    }
    verify(&store, &roots_path, timeout).await?;
    #[cfg(test)]
    let corrupt_unique = matches!(_fault, Fault::CorruptUniqueBeforeVerify);
    #[cfg(not(test))]
    let corrupt_unique = false;
    let payload = unique::prepare(
        repo,
        work.path(),
        record.path(),
        width,
        timeout,
        corrupt_unique,
    )
    .await?;
    fs::write(
        record.path().join("classification.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "main_missing_objects": payload.count,
        "unique_uncompressed_bytes": payload.uncompressed_bytes,
        "rescue_pack_bytes": payload.packed_bytes,
            "rescue_pack_name_bytes": payload.pack_name.as_ref().map(|path| path.as_os_str().as_encoded_bytes()),
            "classification_is_not_deletion_authority": true,
        }))?,
    )?;
    let mut manifest = String::from("dracon-epoch-recovery-v2\n");
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
    writeln!(receipt, "dracon-epoch-recovery-v2 {manifest_sha256}")?;
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
    let rescue_pack = payload.pack_name.map(|name| record.join(name));
    Ok(DurableReceipt {
        record,
        manifest_sha256,
        unique_objects: payload.count,
        unique_uncompressed_bytes: payload.uncompressed_bytes,
        rescue_pack_bytes: payload.packed_bytes,
        rescue_pack,
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

    #[cfg(unix)]
    #[tokio::test]
    async fn shared_objects_are_independent_and_semantic_receipts_survive_record_gc() {
        use std::os::unix::fs::MetadataExt;
        let (_root, repo, spool, blob) = fixture("sha1").await;
        let first = prepare_record(&repo, &spool, 20).await.unwrap();
        let second = prepare_record(&repo, &spool, 20).await.unwrap();
        let object = |record: &Path| {
            record
                .join("store.git/objects")
                .join(&blob[..2])
                .join(&blob[2..])
        };
        let left = fs::metadata(object(&first.record)).unwrap();
        let right = fs::metadata(object(&second.record)).unwrap();
        assert_eq!(
            (left.dev(), left.ino()),
            (right.dev(), right.ino()),
            "identical objects must share physical storage"
        );
        let manifest = fs::read(first.record.join("manifest.sha256")).unwrap();
        text(&first.record.join("store.git"), &["gc", "--prune=now"], 10)
            .await
            .unwrap();
        assert_eq!(
            fs::read(first.record.join("manifest.sha256")).unwrap(),
            manifest
        );
        verify_receipt(&first.record, &first.manifest_sha256, 10)
            .await
            .unwrap();
        verify(
            &second.record.join("store.git"),
            &second.record.join("roots"),
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            text(
                &second.record.join("store.git"),
                &["cat-file", "blob", &blob],
                10
            )
            .await
            .unwrap(),
            "original recovery payload"
        );
        fs::write(
            first.record.join("metadata/notes"),
            b"fixture metadata corruption",
        )
        .unwrap();
        assert!(
            verify_receipt(&first.record, &first.manifest_sha256, 10)
                .await
                .is_err(),
            "a semantic receipt must still detect changed recovery metadata after GC"
        );
    }

    #[tokio::test]
    async fn corrupt_existing_shared_object_is_not_overwritten_or_accepted() {
        let (_root, repo, spool, blob) = fixture("sha1").await;
        let _first = prepare_record(&repo, &spool, 20).await.unwrap();
        let shared = repo
            .join(".git/dracon-rescue/cas/objects")
            .join(&blob[..2])
            .join(&blob[2..]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Deliberate fault injection into a disposable fixture; Git's
            // normal read-only object mode prevents accidental overwrites.
            fs::set_permissions(&shared, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(&shared, b"corrupt shared object").unwrap();
        let index = fs::read(spool.join("index")).unwrap();
        assert!(prepare_record(&repo, &spool, 20).await.is_err());
        assert_eq!(
            fs::read(shared).unwrap(),
            b"corrupt shared object",
            "recovery must never repair by overwriting retained objects"
        );
        assert_eq!(fs::read(spool.join("index")).unwrap(), index);
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

    #[test]
    fn recovery_writer_probe() {
        let Some(path) = std::env::var_os("DRACON_RECOVERY_WRITER_PROBE") else {
            return;
        };
        let path = PathBuf::from(path);
        fs::write(path.join("probe-ready"), std::process::id().to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(5));
        fs::write(path.join("late-write"), b"still using preparation storage").unwrap();
    }

    // Protect cancellation ownership: dropping the requester must not destroy
    // storage while its already-launched writer is still alive.
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_request_retains_storage_until_writer_is_joined() {
        let (_root, repo, spool, _) = fixture("sha1").await;
        let original = fs::read(spool.join("index")).unwrap();
        let gitdir = repo.join(".git");
        let owner = super::super::ownership::ReclaimOwner::acquire(&gitdir, &spool).unwrap();
        let (r, s) = (repo.clone(), spool.clone());
        let request = tokio::spawn(async move {
            prepare_guarded(&r, &s, 20, Fault::SlowBeforeVerify, owner).await
        });
        let rescue = repo.join(".git/dracon-rescue");
        let record = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(entries) = fs::read_dir(&rescue) {
                    for entry in entries.flatten() {
                        if entry.path().join("probe-ready").exists() {
                            return entry.path();
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let pid: i32 = fs::read_to_string(record.join("probe-ready"))
            .unwrap()
            .parse()
            .unwrap();
        request.abort();
        assert!(matches!(request.await, Err(error) if error.is_cancelled()));
        assert!(
            record.exists(),
            "cancelled requester must not clean beneath its writer"
        );
        assert!(!record.join("receipt").exists());
        assert!(
            super::super::ownership::ReclaimOwner::acquire(&gitdir, &spool).is_err(),
            "cancelled requester must not release its live preparation fence"
        );
        // This PID came from this fixture's single writer, not a live daemon.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while record.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if super::super::ownership::ReclaimOwner::acquire(&gitdir, &spool).is_ok() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(fs::read(spool.join("index")).unwrap(), original);
    }

    // Protect the actual prepublication gate: corruption must fail preparation,
    // not merely be detectable by a verifier invoked after a successful receipt.
    #[tokio::test]
    async fn post_write_corruption_and_publication_faults_preserve_source() {
        for fault in [
            Fault::CorruptBeforeVerify,
            Fault::BeforeReceipt,
            Fault::AfterRetain,
        ] {
            let (_root, repo, spool, _) = fixture("sha1").await;
            let original = fs::read(spool.join("index")).unwrap();
            assert!(prepare_with_fault(&repo, &spool, 20, fault).await.is_err());
            assert_eq!(fs::read(spool.join("index")).unwrap(), original);
            for record in fs::read_dir(repo.join(".git/dracon-rescue")).unwrap() {
                assert!(
                    !record.unwrap().path().join("receipt").exists(),
                    "failed preparation must not authorize removal"
                );
            }
        }
    }

    #[tokio::test]
    async fn isolated_verification_rejects_corrupt_pack_with_source_intact() {
        let (_root, repo, spool, blob) = fixture("sha1").await;
        let source = fs::read(spool.join("index")).unwrap();
        let receipt = prepare_record(&repo, &spool, 10).await.unwrap();
        let store = receipt.record.join("store.git");
        let pack = store.join("objects").join(&blob[..2]).join(&blob[2..]);
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
