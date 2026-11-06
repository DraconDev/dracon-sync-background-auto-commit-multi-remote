//! Redundancy-only epoch spool reclamation.
//!
//! This route is deliberately separate from the qualified reclaim transaction
//! (`reclaim::attempt`), which preserves UNPUBLISHED bytes through an
//! independent recovery closure. This route removes a spool only when every
//! byte it holds is ALREADY durably published, so nothing can be lost:
//!
//! * quiescence: the producer fence plus an identity-bound settled marker
//!   (`ReclaimOwner::acquire` + `proves_inactivity`). A legacy spool without a
//!   producer-compatible lock is refused, never fenced retroactively.
//! * no preserve marker (`preserve-rejected`, `original-index`).
//! * the object universe is the spool index entries plus every object in the
//!   retained packs. Every one of those objects must be REACHABLE from a
//!   published remote-tracking ref in this repository's OWN object database.
//!   Instantaneous presence is not durability: a dangling object can already
//!   be past Git's prune grace.
//! * the spool index must describe exactly a published tree, entry for entry.
//!   `git write-tree` is never used: it can create tree objects in the main
//!   object store and honours a stale cache-tree extension.
//! * no other non-empty file in the spool may carry unknown payload.
//!
//! Nothing here mutates the repository's object store. Every Git invocation is
//! read-only plumbing, run with a controlled environment that inherits neither
//! `GIT_INDEX_FILE` nor an alternate/objects-directory override, so
//! reachability is judged against the repository's own objects only.

use super::ownership::ReclaimOwner;
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// Objects a single published-ref reachability scan may retain before the
/// repository is treated as too large to prove. A larger repository preserves
/// every candidate rather than guessing at its object universe.
const MAX_REACHABLE_OBJECTS: usize = 2_000_000;

/// How long a repository is left alone after a redundant-reclaim pass.
///
/// The pass builds a full published-reachability set: one `rev-list --objects`
/// walk PER published remote-tracking ref (a repo with origin+github+gitlab
/// pays it three times) plus one `ls-tree -r` per ref. On the fleet's larger
/// repos that is ~1.5M object lines per pass, and a pass that removes nothing
/// still pays it in full — blocking a sync worker every cycle. Without this
/// cooldown the route saturated the machine (load average 38-59, kswapd0
/// thrashing) while proving zero spools, because the dominant blockers (a
/// spool index that matches no published tree, a legacy spool with no
/// producer fence) are properties of the SPOOL, not of the repository, and
/// re-deriving them every cycle can never change the answer.
///
/// One hour matches the auto-gc cooldown: reclamation is housekeeping, and an
/// hour of latency on a housekeeping decision costs nothing.
const REDUNDANT_RECLAIM_COOLDOWN_SECS: u64 = 3600;

/// Per-repository cooldown state for this route.
static REDUNDANT_ATTEMPTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, std::time::Instant>>,
> = std::sync::OnceLock::new();

/// Whether this repository is still inside its post-pass cooldown.
fn cooling_down(repo: &Path) -> bool {
    let attempts = REDUNDANT_ATTEMPTS.get_or_init(|| std::sync::Mutex::new(Default::default()));
    attempts
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(repo)
        .is_some_and(|last| last.elapsed().as_secs() < REDUNDANT_RECLAIM_COOLDOWN_SECS)
}

/// Record that a pass ran for this repository now.
fn note_attempt(repo: &Path) {
    let attempts = REDUNDANT_ATTEMPTS.get_or_init(|| std::sync::Mutex::new(Default::default()));
    attempts
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(repo.to_path_buf(), std::time::Instant::now());
}

/// The durable record of removals this route performs.
const LEDGER: &str = "dracon-redundant-reclaim.jsonl";

/// A published remote-tracking ref and the tree it publishes.
/// The published tree of one ref, indexed by path for per-entry proof.
struct PublishedRef {
    name: String,
    entries: Vec<TreeEntry>,
    /// `path -> (mode, oid)` so a spool index entry can be proved against the
    /// path it actually occupies, not against a whole-tree digest.
    by_path: std::collections::BTreeMap<Vec<u8>, (String, String)>,
}

/// Everything the published refs prove about this repository.
struct Published {
    refs: Vec<PublishedRef>,
    reachable: HashSet<String>,
}

/// A staged entry: mode, object identity and path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TreeEntry {
    mode: String,
    oid: String,
    path: Vec<u8>,
}

/// What a proven removal established, for the audit record.
struct Removal {
    objects: usize,
    reference: String,
}

/// Read-only Git plumbing with a controlled environment.
///
/// `env_clear` drops every inherited `GIT_*` variable, so no ambient
/// `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY` or
/// `GIT_ALTERNATE_OBJECT_DIRECTORIES` can influence the proof.
fn git(repo: &Path) -> Command {
    let mut command = Command::new(crate::policy::git_binary());
    let path = std::env::var_os("PATH").unwrap_or_default();
    command
        .env_clear()
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
    command
}

/// Run one read-only Git command and return its standard output.
async fn git_output(repo: &Path, args: &[&str], timeout: u64, label: &str) -> Result<Vec<u8>> {
    let mut command = git(repo);
    command.args(args);
    let output =
        crate::git::run_command_captured_output(command, timeout, label, None).await?;
    ensure!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}

/// The remote-tracking refs that stand for published state.
///
/// A local branch or tag may hold commits the remote has never seen, so
/// neither is published proof. Only `refs/remotes/<remote>/<branch>` is, and
/// only for a remote this repository is actually configured with.
async fn published_refs(repo: &Path, timeout: u64) -> Result<Vec<String>> {
    let listing = git_output(
        repo,
        &["for-each-ref", "--format=%(refname)", "refs/remotes/"],
        timeout,
        "published refs",
    )
    .await?;
    let configured = git_output(repo, &["remote"], timeout, "published remotes").await?;
    let remotes: HashSet<String> = configured
        .split(|byte| *byte == b'\n')
        .map(|line| String::from_utf8_lossy(line).trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    let mut refs = Vec::new();
    for line in listing.split(|byte| *byte == b'\n') {
        let name = String::from_utf8_lossy(line).trim().to_owned();
        if name.is_empty() {
            continue;
        }
        if !remotes.is_empty() {
            let Some(remote) = remote_of(&name) else {
                continue;
            };
            if !remotes.contains(remote) {
                continue;
            }
        }
        refs.push(name.to_owned());
    }
    Ok(refs)
}

/// The remote that owns a remote-tracking ref.
fn remote_of(refname: &str) -> Option<&str> {
    let rest = refname.strip_prefix("refs/remotes/")?;
    rest.split_once('/').map(|(remote, _)| remote)
}

/// Collect the published trees and the objects reachable from them.
///
/// Reachability is walked ONCE for all refs (`rev-list --objects <ref>...`):
/// the fleet's repos publish the same history to origin, github and gitlab, so
/// walking per ref repeated an identical ~500k-object traversal three times for
/// the same set. The union is identical — reachability is a union operation, so
/// one walk over every ref yields exactly what the per-ref walks accumulated —
/// and the proof never shrinks, only the work does.
async fn published_set(repo: &Path, timeout: u64) -> Result<Published> {
    let names = published_refs(repo, timeout).await?;
    let mut refs = Vec::new();
    let mut reachable: HashSet<String> = HashSet::new();

    // Trees are still read per ref: the tree-match proof needs each ref's own
    // entry list, and `ls-tree` is cheap (one tree read, no history walk).
    for name in &names {
        let tree = match git_output(repo, &["ls-tree", "-r", "-z", name], timeout, "published tree")
            .await
        {
            Ok(tree) => tree,
            Err(error) => {
                // A ref that cannot be read proves nothing. Skipping it only
                // shrinks the proof; it never widens it.
                eprintln!(
                    "⚠️ {}: published ref {name} is unreadable; ignored: {error:#}",
                    repo.display()
                );
                continue;
            }
        };
        refs.push(PublishedRef {
            name: name.clone(),
            entries: parse_tree(&tree)?,
        });
    let mut command = git(repo);
    command
        .args(["rev-list", "--objects", "--no-object-names"])
        .args(&names);
    let objects = crate::git::run_command_captured_output(
        command,
        timeout,
        "published objects",
        None,
    )
    .await?;
    ensure!(
        objects.status.success(),
        "published reachability walk failed: {}",
        String::from_utf8_lossy(&objects.stderr).trim()
    );
    let objects = objects.stdout;

    for line in objects.split(|byte| *byte == b'\n') {
        let oid = std::str::from_utf8(line)
            .context("invalid published object identity")?
            .trim();
        if oid.is_empty() {
            continue;
        }
        ensure!(
            crate::git::is_valid_object_id(oid),
            "invalid published object identity"
        );
        ensure!(
            reachable.len() < MAX_REACHABLE_OBJECTS,
            "published object universe is too large to prove"
        );
        reachable.insert(oid.to_owned());
    }
    Ok(Published { refs, reachable })
}

/// Parse `git ls-tree -r -z` output into sorted staged entries.
fn parse_tree(bytes: &[u8]) -> Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    for record in bytes.split(|byte| *byte == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("invalid tree entry")?;
        let fields: Vec<_> = std::str::from_utf8(&record[..tab])?
            .split(' ')
            .collect();
        ensure!(fields.len() == 3, "invalid tree entry fields");
        ensure!(
            crate::git::is_valid_object_id(fields[2]),
            "invalid tree entry object identity"
        );
        entries.push(TreeEntry {
            mode: fields[0].to_owned(),
            oid: fields[2].to_owned(),
            path: record[tab + 1..].to_vec(),
        });
    }
    entries.sort();
    Ok(entries)
}

/// Parse `git ls-files -s -z` output into sorted staged entries.
///
/// Only stage zero is accepted: any other stage means the index still records
/// conflict state that no published tree carries.
fn parse_index(bytes: &[u8]) -> Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    for record in bytes.split(|byte| *byte == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("invalid index entry")?;
        let fields: Vec<_> = std::str::from_utf8(&record[..tab])?
            .split(' ')
            .collect();
        ensure!(fields.len() == 3, "invalid index entry fields");
        ensure!(fields[2] == "0", "unmerged index entry");
        ensure!(
            crate::git::is_valid_object_id(fields[1]),
            "invalid index entry object identity"
        );
        entries.push(TreeEntry {
            mode: fields[0].to_owned(),
            oid: fields[1].to_owned(),
            path: record[tab + 1..].to_vec(),
        });
    }
    entries.sort();
    Ok(entries)
}

/// The identity width used by this repository.
async fn object_width(repo: &Path, timeout: u64, published: &Published) -> Result<usize> {
    if let Ok(format) = git_output(
        repo,
        &["rev-parse", "--show-object-format"],
        timeout,
        "object format",
    )
    .await
    {
        match String::from_utf8_lossy(&format).trim() {
            "sha1" => return Ok(40),
            "sha256" => return Ok(64),
            _ => {}
        }
    }
    published
        .reachable
        .iter()
        .next()
        .map(|oid| oid.len())
        .context("repository object format is unknown")
}

/// Refuse an index whose bytes do not fully describe the staged tree.
///
/// A split index depends on a shared base outside the spool, unmerged stages
/// carry conflict state, extended flags carry intent-to-add/skip-worktree, and
/// any extension other than the cache tree can carry hidden payload. The
/// extension boundary is parsed here so Git cannot silently resolve a split
/// index while the entries are being read.
fn self_contained_index(path: &Path, width: usize) -> Result<()> {
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() >= 12 + width / 2 && &bytes[..4] == b"DIRC",
        "invalid spool index"
    );
    let number = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .context("truncated spool index")?
                .try_into()?,
        ))
    };
    let version = number(4)?;
    ensure!(
        version == 2 || version == 3,
        "only index v2/v3 are self-contained"
    );
    let end = bytes.len() - width / 2;
    let mut at = 12;
    for _ in 0..number(8)? {
        let start = at;
        let flags_at = at + 40 + width / 2;
        let flags = u16::from_be_bytes(
            bytes
                .get(flags_at..flags_at + 2)
                .context("truncated spool index flags")?
                .try_into()?,
        );
        ensure!(flags & 0x3000 == 0, "unmerged spool index");
        ensure!(flags & 0x4000 == 0, "extended spool index flags");
        at = flags_at + 2;
        while at < end && bytes[at] != 0 {
            at += 1;
        }
        ensure!(at < end, "unterminated spool index path");
        at = start + (at + 1 - start).div_ceil(8) * 8;
        ensure!(at <= end, "truncated spool index padding");
    }
    while at < end {
        let signature = bytes
            .get(at..at + 4)
            .context("truncated spool index extension")?;
        ensure!(
            signature == b"TREE",
            "unsupported spool index extension (including split index)"
        );
        at = at
            .checked_add(8 + number(at + 4)? as usize)
            .context("spool index extension overflow")?;
        ensure!(at <= end, "truncated spool index extension data");
    }
    ensure!(at == end, "invalid spool index boundary");
    Ok(())
}

/// The object count the pack header declares.
fn pack_count(path: &Path) -> Result<usize> {
    let mut header = [0u8; 12];
    let mut file = File::open(path)?;
    file.read_exact(&mut header)?;
    ensure!(&header[..4] == b"PACK", "invalid retained pack header");
    let version = u32::from_be_bytes(header[4..8].try_into()?);
    ensure!(
        version == 2 || version == 3,
        "unsupported retained pack version"
    );
    Ok(u32::from_be_bytes(header[8..12].try_into()?) as usize)
}

/// The objects a retained pack holds, plus every delta base it references.
///
/// The pack header declares the object count and the listing must agree, so a
/// truncated or partial listing can never under-report the spool's payload.
/// A delta base is normally another object of the same pack, but a thin pack
/// may reference an external one; it is listed separately so it can be held
/// to the same reachability proof.
fn listed_objects(
    listing: &[u8],
    width: usize,
    expected: usize,
) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mut objects = BTreeSet::new();
    let mut bases = BTreeSet::new();
    for line in listing
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let line = std::str::from_utf8(line).context("invalid verify-pack output")?;
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields
            .first()
            .is_some_and(|field| is_object_id(field, width))
        {
            ensure!(
                fields.len() == 5 || fields.len() == 7,
                "invalid verify-pack object row"
            );
            ensure!(
                ["blob", "tree", "commit", "tag"].contains(&fields[1]),
                "unknown retained object type"
            );
            for field in &fields[2..5] {
                field.parse::<u64>()?;
            }
            if fields.len() == 7 {
                fields[5].parse::<u64>()?;
                ensure!(
                    is_object_id(fields[6], width),
                    "invalid retained delta base"
                );
                bases.insert(fields[6].to_owned());
            }
            ensure!(
                objects.insert(fields[0].to_owned()),
                "duplicate object within retained pack"
            );
        } else {
            ensure!(
                line.ends_with(": ok")
                    || line.starts_with("non delta: ")
                    || line.starts_with("chain length = "),
                "unknown verify-pack output: {line:?}"
            );
        }
    }
    ensure!(
        objects.len() == expected,
        "retained pack header/enumeration mismatch: {} listed, {expected} declared",
        objects.len()
    );
    Ok((objects, bases))
}

/// Whether a string is a full object identity of this repository's width.
fn is_object_id(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The retained pack stem of a spool file name.
fn retained_stem(name: &str) -> Option<&str> {
    let stem = name.strip_prefix("retained-")?;
    stem.strip_suffix(".pack")
        .or_else(|| stem.strip_suffix(".idx"))
        .or_else(|| stem.strip_suffix(".rev"))
}

/// Whether a spool file is known to hold no payload of its own.
///
/// The retained packs and their index are published object storage. A
/// retained reverse index is derived from its own pack and carries no
/// payload; it is only recognised next to that pack.
fn known_payload_free(name: &str, spool: &Path) -> bool {
    if name == "index" || name == "reclaim-owner.lock" {
        return true;
    }
    if name.starts_with("reclaim-") && name.ends_with(".json") {
        return true;
    }
    let Some(stem) = retained_stem(name) else {
        return false;
    };
    if name.ends_with(".rev") {
        return spool.join(format!("retained-{stem}.pack")).is_file();
    }
    true
}

/// Refuse a spool that carries an unknown non-empty file.
///
/// Unknown payload must not be deleted. An empty file proves nothing, so it is
/// accepted; anything else is a reason to preserve the whole spool.
///
/// A producer endpoint (`producer-io-*`) is the daemon↔producer I/O channel:
/// it carries the stdin payload or the captured stdout/stderr of one epoch
/// Git command. Those files are usually empty (stderr) or hold a revision
/// expression (a ref update's argument, a rev-parse/write-tree result).
///
/// Such a file is payload-free ONLY when every token is either
///
/// - an object id — optionally with Git's peel suffix — that is already
///   reachable from a published ref, or
/// - the stem of a retained pack this spool owns, whose every object is
///   separately proven reachable before removal.
///
/// Then the file names content that is itself proven redundant and carries no
/// unpublished bytes of its own. A file that holds anything else (a blob's
/// content, a diagnostic, a partial write, a path) is unknown payload and
/// preserves the whole spool.
fn only_known_files(spool: &Path, published: &Published, width: usize) -> Result<()> {
    for entry in fs::read_dir(spool)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            anyhow::bail!("non-UTF8 spool entry name");
        };
        let metadata = fs::symlink_metadata(entry.path())?;
        ensure!(
            metadata.is_file(),
            "unsupported spool entry layout; spool preserved"
        );
        if known_payload_free(name, spool) {
            continue;
        }
        if metadata.len() == 0 {
            continue;
        }
        if name.starts_with("producer-io-") {
            let names = producer_names(&entry.path(), width)
                .context("producer endpoint holds payload that is not a name")?;
            let stems = retained_pack_stems(spool)?;
            for producer_name in &names {
                if published.reachable.contains(producer_name.as_str()) {
                    continue;
                }
                ensure!(
                    stems.contains(producer_name.as_str()),
                    "producer endpoint names neither a published object nor this spool's own retained pack"
                );
            }
            continue;
        }
        anyhow::bail!("spool holds unknown non-empty payload; spool preserved");
    }
    Ok(())
}

/// The retained pack stems this spool owns.
///
/// `git pack-objects <basename>` writes `<basename>-<hash>.pack`, where the
/// hash is a content hash of the pack itself. A producer endpoint that names
/// the pack it wrote therefore holds a pack name, not an object id. Only a
/// stem whose pack AND index are both present counts: an unpaired index is
/// already a reason to preserve the spool, and must not become a name that
/// vouches for itself.
fn retained_pack_stems(spool: &Path) -> Result<BTreeSet<String>> {
    let mut packs: BTreeSet<String> = BTreeSet::new();
    let mut indexes: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(spool)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(stem) = retained_stem(name) else {
            continue;
        };
        if name.ends_with(".pack") {
            packs.insert(stem.to_owned());
        } else if name.ends_with(".idx") {
            indexes.insert(stem.to_owned());
        }
    }
    packs.retain(|stem| indexes.contains(stem));
    Ok(packs)
}

/// Every name a producer endpoint file holds.
///
/// The file must consist of nothing but whitespace-separated revision
/// expressions built from a full-width hash: an optional `^`, the hash itself,
/// and an optional Git peel suffix (`^{tree}`, `^{commit}`, `^{tag}`,
/// `^{blob}`). The same shape covers an object id and a retained pack stem, so
/// the caller decides which of the two each name is. Anything else means the
/// file carries bytes of its own.
fn producer_names(path: &Path, width: usize) -> Result<Vec<String>> {
    const LIMIT: u64 = 1024 * 1024;
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= LIMIT,
        "producer endpoint exceeds the object-id limit"
    );
    let mut bytes = Vec::new();
    std::io::Read::take(&mut { file }, LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "producer endpoint exceeds the object-id limit"
    );
    let text = std::str::from_utf8(&bytes).context("producer endpoint is not text")?;
    let mut names = Vec::new();
    for token in text.split_whitespace() {
        let token = token.strip_prefix('^').unwrap_or(token);
        let token = ["^{tree}", "^{commit}", "^{tag}", "^{blob}"]
            .iter()
            .find_map(|suffix| token.strip_suffix(suffix))
            .unwrap_or(token);
        ensure!(
            is_object_id(token, width),
            "producer endpoint holds a token that is not a full-width hash"
        );
        names.push(token.to_owned());
    }
    ensure!(!names.is_empty(), "producer endpoint names nothing");
    Ok(names)
}

/// A preserve marker forbids removal.
fn protected(spool: &Path) -> bool {
    ["preserve-rejected", "original-index"].iter().any(|name| {
        !matches!(
            fs::symlink_metadata(spool.join(name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
    })
}

/// The durable intent record of a removal.
///
/// The record is written and synced BEFORE the source is removed, so a crash
/// can never leave an announced removal without its intent.
fn append_ledger(gitdir: &Path, value: &serde_json::Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(gitdir.join(LEDGER))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "redundant reclaim ledger is not a regular file"
    );
    file.try_lock()
        .context("another reclamation is journaling; source retained")?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(gitdir)?.sync_all()?;
    Ok(())
}

/// Whether this repository still uses a graft file.
///
/// Grafts rewrite the parent list, so they change what "reachable" means.
/// Their presence makes every reachability statement ambiguous.
async fn grafts_present(repo: &Path, gitdir: &Path, timeout: u64) -> bool {
    if gitdir.join("info/grafts").exists() {
        return true;
    }
    match git_output(
        repo,
        &["rev-parse", "--git-path", "info/grafts"],
        timeout,
        "graft path",
    )
    .await
    {
        Ok(path) => {
            let path = String::from_utf8_lossy(&path).trim().to_owned();
            if path.is_empty() {
                return false;
            }
            let resolved = PathBuf::from(&path);
            let resolved = if resolved.is_absolute() {
                resolved
            } else {
                repo.join(resolved)
            };
            resolved.exists()
        }
        Err(_) => false,
    }
}

/// Prove one spool redundant and remove it.
///
/// Every check must pass. Any failure preserves the spool and returns the
/// reason; nothing is removed on a partial proof.
async fn remove_if_redundant(
    repo: &Path,
    gitdir: &Path,
    spool: &Path,
    published: &Published,
    width: usize,
    floor: std::time::Duration,
    timeout: u64,
) -> Result<Option<Removal>> {
    // Quiescence: the producer fence plus an identity-bound settled marker.
    let owner = ReclaimOwner::acquire(gitdir, spool, floor)?;
    owner
        .proves_inactivity()
        .context("source inactivity is not proven")?;
    ensure!(!protected(spool), "preserve-marked source");
    only_known_files(spool, published, width)?;

    let index = spool.join("index");
    ensure!(index.is_file(), "spool has no index to compare");
    self_contained_index(&index, width)?;

    let mut command = git(repo);
    command
        .env("GIT_INDEX_FILE", &index)
        .args(["ls-files", "-s", "-z"]);
    let listing =
        crate::git::run_command_captured_output(command, timeout, "spool index entries", None).await?;
    ensure!(
        listing.status.success(),
        "spool index entries could not be read: {}",
        String::from_utf8_lossy(&listing.stderr).trim()
    );
    let entries = parse_index(&listing.stdout)?;

    // The object universe is the index plus every retained pack. A pack may
    // hold objects the index never references; those are still covered.
    let mut universe: BTreeSet<String> = entries.iter().map(|entry| entry.oid.clone()).collect();
    let mut packs: BTreeSet<String> = BTreeSet::new();
    let mut indexes: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(spool)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            anyhow::bail!("non-UTF8 spool entry name");
        };
        let Some(stem) = retained_stem(name) else {
            continue;
        };
        if name.ends_with(".pack") {
            packs.insert(stem.to_owned());
        } else if name.ends_with(".idx") {
            indexes.insert(stem.to_owned());
        }
    }
    // Every pack must be enumerable, and every index must describe a pack.
    // An unreadable pack could hide objects the proof never saw.
    ensure!(
        packs.is_subset(&indexes),
        "retained pack without a readable index; spool preserved"
    );
    for stem in &packs {
        let pack = spool.join(format!("retained-{stem}.pack"));
        let pack_index = spool.join(format!("retained-{stem}.idx"));
        let listing = git_output(
            repo,
            &[
                "verify-pack",
                "-v",
                pack_index.to_str().context("invalid retained index path")?,
            ],
            timeout,
            "retained pack objects",
        )
        .await?;
        let (objects, bases) = listed_objects(&listing, width, pack_count(&pack)?)?;
        universe.extend(objects);
        universe.extend(bases);
    }

    // Durability: every object the spool holds must be reachable from a
    // published ref in this repository's own object database.
    for oid in &universe {
        ensure!(
            published.reachable.contains(oid.as_str()),
            "spool holds an object that no published ref reaches"
        );
    }

    // The index must describe exactly a published tree.
    let matched = published
        .refs
        .iter()
        .find(|reference| reference.entries == entries)
        .map(|reference| reference.name.clone());
    let Some(reference) = matched else {
        anyhow::bail!("spool index does not describe any published tree");
    };

    // Final, no-await checks. The owner fence is held across the removal, so
    // a live producer cannot write through this source while it is removed.
    ensure!(owner.still_owned(), "spool identity changed before removal");
    ensure!(!protected(spool), "preserve marker appeared during verification");
    owner
        .proves_inactivity()
        .context("source inactivity changed during verification")?;

    let intent = serde_json::json!({
        "version": 1,
        "phase": "redundant-authorized-not-yet-removed",
        "spool_path_bytes": spool.as_os_str().as_encoded_bytes(),
        "objects": universe.len(),
        "published_ref_bytes": reference.as_bytes(),
        "verified_unix_seconds": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    });
    append_ledger(gitdir, &intent)?;
    fs::remove_dir_all(spool).context("spool removal failed; spool preserved")?;
    File::open(gitdir)?
        .sync_all()
        .context("spool removed, directory durability unconfirmed")?;
    Ok(Some(Removal {
        objects: universe.len(),
        reference,
    }))
}

/// Remove every spool whose content is provably redundant.
///
/// The route is additive: it never calls the qualified reclaim transaction and
/// never widens age-only deletion authority. Age only schedules the scan.
pub(super) async fn reap(
    repo: &Path,
    max_age_secs: u64,
    op_timeout_secs: u64,
) -> usize {
    let seconds = op_timeout_secs.clamp(30, 600);
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 {
        3600
    } else {
        max_age_secs
    });
    // Housekeeping, not convergence: a pass that just ran cannot learn anything
    // new about the same spools, and the reachability walk it needs is the most
    // expensive thing this route does.
    if cooling_down(repo) {
        return 0;
    }
    note_attempt(repo);
    let mut removed = 0usize;
    let pass = async {
        let Some(gitdir) = super::absolute_git_dir(repo).await else {
            return;
        };
        let scan_dir = gitdir.clone();
        let candidates =
            tokio::task::spawn_blocking(move || super::spool_candidates(&scan_dir, floor))
                .await
                .unwrap_or_default();
        let candidates: Vec<PathBuf> = candidates
            .into_iter()
            .take(super::MAX_SPOOLS_PER_PASS)
            .collect();
        if candidates.is_empty() {
            return;
        }
        if grafts_present(repo, &gitdir, seconds).await {
            eprintln!(
                "⚠️ {}: graft file present; published reachability is ambiguous and every spool is preserved",
                repo.display()
            );
            return;
        }
        let published = match published_set(repo, seconds).await {
            Ok(published) => published,
            Err(error) => {
                eprintln!(
                    "⚠️ {}: published reachability could not be established; every spool is preserved: {error:#}",
                    repo.display()
                );
                return;
            }
        };
        if published.refs.is_empty() {
            eprintln!(
                "⚠️ {}: no published remote-tracking ref; every spool is preserved",
                repo.display()
            );
            return;
        }
        let width = match object_width(repo, seconds, &published).await {
            Ok(width) => width,
            Err(error) => {
                eprintln!(
                    "⚠️ {}: repository object format is unknown; every spool is preserved: {error:#}",
                    repo.display()
                );
                return;
            }
        };
        for spool in candidates {
            match remove_if_redundant(
                repo,
                &gitdir,
                &spool,
                &published,
                width,
                floor,
                seconds,
            )
            .await
            {
                Ok(Some(removal)) => {
                    removed += 1;
                    eprintln!(
                        "🧹 {}: redundant epoch spool {} removed; {} object(s) proven published by {}",
                        repo.display(),
                        spool.display(),
                        removal.objects,
                        removal.reference
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "⚠️ {}: epoch spool {} preserved; not provably redundant: {error:#}",
                        repo.display(),
                        spool.display()
                    );
                }
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(seconds), pass).await;
    removed
}

#[cfg(test)]
#[path = "epoch_spool_redundant_tests.rs"]
mod tests;
