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

/// How many published commits one pass's history walk may read when an entry
/// does not match any published tip.
///
/// CHANGED 2026-11-12: the historical proof used to spawn, PER unmatched entry
/// (≤512) and PER published ref (9 on browser-extensions-shared, five of them
/// the same tip), one `rev-list -- <path>` plus up to 64 `rev-parse`/`ls-tree`
/// calls, and restarted on the next ref whenever one entry failed. Measured on
/// a live 247-unmatched-entry spool at load ~60 that is >300 s for ONE ref, so
/// a single spool consumed the whole pass and no pass ever finished. It is now
/// ONE `git log --raw` over every published ref per pass, shared by all spools
/// in it: full history of browser-extensions-shared (69,778 commits) in
/// 13.5 s at load ~60; dracon-platform (45k) 3.1 s / 32 MB; ~/.dracon (130k,
/// the fleet's largest) 3.5 s / 44 MB / 257k distinct entries.
///
/// The window must cover FULL history, not a recent slice: a spool's index
/// holds every file of the working tree, and a file last changed months ago
/// is recorded only in that old commit. A 20k-commit window left 704 of 956
/// dracon-platform spools unprovable on exactly such entries (files last
/// edited 25k+ commits earlier). 250k covers every repo in the fleet with
/// headroom; an entry beyond it is simply not proved — the spool is kept.
const MAX_HISTORY_COMMITS: usize = 250_000;

/// Index/tree mode of a submodule gitlink.
const GITLINK_MODE: &str = "160000";

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
/// CHANGED 2026-11-12 (spool leak, 3.6k spools / 6.5 GiB in dracon-platform):
/// the cooldown was one hour and each pass examined at most 64 spools, a hard
/// ceiling of 1,536 spools/repo/day — below dracon-platform's ~4,800/day
/// creation rate even if every spool were provable. Worse, candidates are
/// visited oldest-first, and the oldest are exactly the permanently
/// unprovable ones (legacy spools without a fence), so every pass re-proved
/// the same refusals and the provable backlog behind them was never reached.
///
/// The "a pass cannot learn anything new" rationale is now enforced directly
/// by the refusal memo below instead of by wall-clock spacing: a spool is
/// re-proved only when the spool itself changed or (for refusals that depend
/// on what is published) the published ref tips moved. That leaves the walk
/// itself, which since the single-walk change costs ~2 s on the fleet's
/// largest repository (402k objects, load average 37) — so ten minutes is
/// ample spacing.
const REDUNDANT_RECLAIM_COOLDOWN_SECS: u64 = 600;

/// Soft wall-clock budget for one pass's spool loop. The pass runs inline on
/// the repository's sync worker, so it must not hold commits back for long;
/// the loop stops between spools once this is spent and the next pass picks
/// up where it left off (refused spools are memoized, removed ones are gone).
const REDUNDANT_PASS_BUDGET_SECS: u64 = 120;

/// Hard cap on spools examined per pass. The budget above is the real bound;
/// this only stops a pathological directory from being walked in one go.
const MAX_REDUNDANT_SPOOLS_PER_PASS: usize = 2048;

/// How long a refusal that does NOT depend on published state is trusted
/// before the spool is proved again. Such refusals (no producer fence,
/// unknown payload, preserve marker, history bound exceeded) are properties
/// of the spool; a daily re-check is a cheap backstop against a
/// misclassification, never a correctness requirement.
const INTRINSIC_REFUSAL_RETRY_SECS: u64 = 24 * 3600;

/// Per-repository cooldown state for this route.
static REDUNDANT_ATTEMPTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, std::time::Instant>>,
> = std::sync::OnceLock::new();

/// A refusal that can only change once something is newly published: an
/// object or entry the spool holds is not (yet) reachable from a published
/// ref. Distinguished by type so the memo can retry it when ref tips move.
#[derive(Debug)]
struct NotYetPublished(&'static str);
impl std::fmt::Display for NotYetPublished {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for NotYetPublished {}

/// A refusal caused by this PASS, not by the spool (the shared history walk
/// failed or hit its deadline). Never memoized: the next pass simply retries.
#[derive(Debug)]
struct PassUnavailable(String);
impl std::fmt::Display for PassUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "published history unavailable this pass: {}", self.0)
    }
}
impl std::error::Error for PassUnavailable {}

/// Why a spool was last refused, and under which conditions that answer
/// still holds.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Refusal {
    /// Retry only after `INTRINSIC_REFUSAL_RETRY_SECS`.
    Intrinsic { at: std::time::Instant },
    /// Retry as soon as the published ref tips differ from these.
    Published { fingerprint: String },
}

#[derive(Clone, Debug)]
struct Memo {
    /// The spool directory's mtime when refused. Any entry added, removed or
    /// renamed (a new settled marker, a dropped pack) invalidates the memo.
    modified: std::time::SystemTime,
    refusal: Refusal,
}

/// Refusal memo, keyed by spool path. Process-local: a restart re-proves each
/// refused spool once, which is the conservative direction.
static REFUSALS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Memo>>> =
    std::sync::OnceLock::new();

fn refusals() -> std::sync::MutexGuard<'static, std::collections::HashMap<PathBuf, Memo>> {
    REFUSALS
        .get_or_init(|| std::sync::Mutex::new(Default::default()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn spool_modified(spool: &Path) -> Option<std::time::SystemTime> {
    fs::symlink_metadata(spool).ok()?.modified().ok()
}

/// Whether a previous refusal of this spool is still the answer, so proving
/// it again would only repeat known work. Never authorises removal: a memo
/// can only make the reclaimer skip a spool, never delete one.
fn refusal_still_holds(spool: &Path, fingerprint: &str) -> bool {
    let Some(memo) = refusals().get(spool).cloned() else {
        return false;
    };
    if spool_modified(spool) != Some(memo.modified) {
        return false;
    }
    match memo.refusal {
        Refusal::Intrinsic { at } => at.elapsed().as_secs() < INTRINSIC_REFUSAL_RETRY_SECS,
        Refusal::Published { fingerprint: seen } => seen == fingerprint,
    }
}

/// `; preserved: 12× reason a; 3× reason b` (most frequent first), or "".
pub(crate) fn refusal_summary(reasons: &std::collections::BTreeMap<String, usize>) -> String {
    if reasons.is_empty() {
        return String::new();
    }
    let mut ranked: Vec<(&String, &usize)> = reasons.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let parts: Vec<String> = ranked.iter().map(|(r, n)| format!("{n}× {r}")).collect();
    format!("; preserved: {}", parts.join("; "))
}

fn remember_refusal(spool: &Path, error: &anyhow::Error, fingerprint: &str) {
    let Some(modified) = spool_modified(spool) else {
        return;
    };
    let refusal = if error.downcast_ref::<NotYetPublished>().is_some() {
        Refusal::Published {
            fingerprint: fingerprint.to_owned(),
        }
    } else {
        Refusal::Intrinsic {
            at: std::time::Instant::now(),
        }
    };
    refusals().insert(spool.to_path_buf(), Memo { modified, refusal });
}

/// Drop memo entries for spools of this git dir that no longer exist, so the
/// memo stays bounded by the live spool population.
fn forget_vanished(gitdir: &Path, present: &BTreeSet<PathBuf>) {
    refusals().retain(|spool, _| spool.parent() != Some(gitdir) || present.contains(spool));
}

/// A cheap identity of the published state: every published ref with its tip.
/// Moves whenever a fetch or push advances what is published.
async fn published_fingerprint(repo: &Path, timeout: u64) -> Result<String> {
    let names = published_refs(repo, timeout).await?;
    if names.is_empty() {
        return Ok(String::new());
    }
    // Names come from `for-each-ref` and always start with `refs/remotes/`,
    // so none can be mistaken for an option.
    let mut args = vec!["rev-parse"];
    args.extend(names.iter().map(String::as_str));
    let tips = git_output(repo, &args, timeout, "published tips").await?;
    let mut fingerprint = String::new();
    for (name, tip) in names.iter().zip(String::from_utf8_lossy(&tips).lines()) {
        fingerprint.push_str(name);
        fingerprint.push(' ');
        fingerprint.push_str(tip.trim());
        fingerprint.push('\n');
    }
    Ok(fingerprint)
}

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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        let entries = parse_tree(&tree)?;
        let by_path = entries
            .iter()
            .map(|entry| (entry.path.clone(), (entry.mode.clone(), entry.oid.clone())))
            .collect();
        refs.push(PublishedRef {
            name: name.clone(),
            by_path,
        });
    }
    // Every ref failed to read: an empty proof, never a permissive one.
    if refs.is_empty() {
        return Ok(Published { refs, reachable });
    }
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
                if !stems.contains(producer_name.as_str()) {
                    return Err(NotYetPublished(
                        "producer endpoint names neither a published object nor this spool's own retained pack",
                    )
                    .into());
                }
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
    let mut reports = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // ADDED 2026-11-13: `git rm`'s success stdout (`rm '<path>'`) names
        // a path that was removed; it carries no repository content. 415 live
        // endpoints held only such lines and each preserved its whole spool
        // although the spool's index and objects are proven separately below.
        // Failure diagnostics (`fatal:`, warden stderr) are deliberately NOT
        // accepted: they are the only record of why that epoch failed.
        if is_git_rm_report(line) {
            reports += 1;
            continue;
        }
        for token in line.split_whitespace() {
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
    }
    ensure!(
        !names.is_empty() || reports > 0,
        "producer endpoint names nothing"
    );
    Ok(names)
}

/// One line of `git rm`'s success report: exactly `rm '<path>'`.
fn is_git_rm_report(line: &str) -> bool {
    line.trim_end_matches('\r')
        .strip_prefix("rm '")
        .is_some_and(|path| path.len() > 1 && path.ends_with('\''))
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

/// Every (mode, blob, path) a published commit recorded, within the bounded
/// history window, built at most once per pass and shared by its spools.
type PublishedHistory = HashSet<TreeEntry>;

/// Read the published history window: the post-image of every change in the
/// newest `MAX_HISTORY_COMMITS` commits reachable from `refs`.
///
/// Sound by construction: a raw diff line's post-image `(mode, blob, path)` is
/// an entry of THAT commit's tree, and the commit is reachable from `refs`.
/// Only remote-tracking refs are ever passed in production, so a local branch
/// is never proof. Anything the window does not show (older commits, entries
/// introduced only by a merge) is simply absent, which preserves the spool.
async fn published_history(repo: &Path, refs: &[String], timeout: u64) -> Result<PublishedHistory> {
    // STREAMED, not captured (2026-11-12): full raw history is ~84 MB on
    // browser-extensions-shared, beyond the 64 MiB capture limit of
    // `run_command_captured_output`. Only the deduplicated entry set is kept.
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let mut command = git(repo);
    command
        .args([
            "log",
            "--no-renames",
            "--no-abbrev",
            "--no-ext-diff",
            "--no-textconv",
            "--raw",
            "-z",
            "--format=%H",
        ])
        .arg(format!("--max-count={MAX_HISTORY_COMMITS}"))
        .args(refs)
        .arg("--");
    let walk = async {
        let mut child = command.spawn().context("published history walk did not start")?;
        let stdout = child.stdout.take().context("published history stdout")?;
        let mut stderr = child.stderr.take().context("published history stderr")?;
        // Drain stderr concurrently so a full pipe can never stall Git.
        let errors = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut bytes).await;
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            bytes
        });
        let mut parser = RawHistoryParser::default();
        let mut reader = BufReader::new(stdout);
        let mut token = Vec::new();
        loop {
            token.clear();
            if reader.read_until(0, &mut token).await? == 0 {
                break;
            }
            if token.last() == Some(&0) {
                token.pop();
            }
            parser.feed(&token)?;
        }
        let status = child.wait().await?;
        let errors = errors.await.unwrap_or_default();
        ensure!(
            status.success(),
            "published history walk failed: {}",
            String::from_utf8_lossy(&errors).trim()
        );
        parser.finish()
    };
    tokio::time::timeout(std::time::Duration::from_secs(timeout), walk)
        .await
        .context("published history walk deadline")?
}

/// Incremental parser for `git log --raw -z --format=%H` output.
///
/// Layout: `<commit>\0` then, per change, `\n:<old> <new> <oid> <oid> <status>\0<path>\0`.
#[derive(Default)]
struct RawHistoryParser {
    history: PublishedHistory,
    /// `(mode, oid)` of a change header whose path token comes next; `None`
    /// inside the pending slot means the change was a deletion (skipped).
    pending: Option<Option<(String, String)>>,
}

impl RawHistoryParser {
    fn feed(&mut self, token: &[u8]) -> Result<()> {
        if let Some(change) = self.pending.take() {
            if let Some((mode, oid)) = change {
                self.history.insert(TreeEntry {
                    mode,
                    oid,
                    path: token.to_vec(),
                });
            }
            return Ok(());
        }
        let token = token.strip_prefix(b"\n").unwrap_or(token);
        let Some(header) = token.strip_prefix(b":") else {
            return Ok(());
        };
        let fields: Vec<&[u8]> = header.split(|byte| *byte == b' ').collect();
        ensure!(fields.len() == 5, "malformed raw history change");
        if fields[4].first() == Some(&b'D') {
            self.pending = Some(None);
            return Ok(());
        }
        let mode = std::str::from_utf8(fields[1]).context("non-UTF8 raw mode")?;
        let oid = std::str::from_utf8(fields[3]).context("non-UTF8 raw object id")?;
        ensure!(
            crate::git::is_valid_object_id(oid),
            "invalid object identity in raw history"
        );
        self.pending = Some(Some((mode.to_owned(), oid.to_owned())));
        Ok(())
    }
    fn finish(self) -> Result<PublishedHistory> {
        ensure!(self.pending.is_none(), "raw history change without a path");
        Ok(self.history)
    }
}

/// Parse a complete `git log --raw -z --format=%H` output.
#[cfg(test)]
fn parse_raw_history(bytes: &[u8]) -> Result<PublishedHistory> {
    let mut parser = RawHistoryParser::default();
    let mut tokens: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    if tokens.last().is_some_and(|last| last.is_empty()) {
        tokens.pop();
    }
    for token in tokens {
        parser.feed(token)?;
    }
    parser.finish()
}

/// Prove every entry the published TIPS did not already prove against the
/// published history window, loading the window on first use in this pass.
///
/// An entry that matches no published tip is still redundant when a PUBLISHED
/// historical commit recorded exactly (mode, blob) at that path: the spool
/// staged a working tree whose file was published earlier and edited later.
async fn published_history_reference(
    repo: &Path,
    entries: &[TreeEntry],
    published: &Published,
    history: &mut Option<std::result::Result<PublishedHistory, String>>,
    timeout: u64,
) -> Result<Option<String>> {
    let unmatched: Vec<&TreeEntry> = entries
        .iter()
        .filter(|entry| {
            !published
                .refs
                .iter()
                .any(|reference| reference.by_path.get(&entry.path) == Some(&(entry.mode.clone(), entry.oid.clone())))
        })
        .collect();
    let label = format!("published history (newest {MAX_HISTORY_COMMITS} commits)");
    if unmatched.is_empty() {
        // Every entry matched SOME tip; they were split across refs.
        return Ok(Some(published.refs.first().map_or(label, |r| r.name.clone())));
    }
    if history.is_none() {
        // Loaded at most ONCE per pass, failure included: a walk that failed
        // (deadline, Git error) fails identically for every later spool, and
        // re-running it per spool is what made one pass examine 145 spools.
        let names: Vec<String> = published.refs.iter().map(|r| r.name.clone()).collect();
        *history = Some(
            published_history(repo, &names, timeout)
                .await
                .map_err(|error| format!("{error:#}")),
        );
    }
    let history = match history.as_ref().expect("history loaded above") {
        Ok(history) => history,
        Err(error) => return Err(PassUnavailable(error.clone()).into()),
    };
    Ok(unmatched
        .iter()
        .all(|entry| history.contains(entry))
        .then_some(label))
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
    apply: bool,
    history: &mut Option<std::result::Result<PublishedHistory, String>>,
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
    //
    // CHANGED 2026-11-12: gitlink entries (mode 160000) are excluded from the
    // universe. A gitlink names a commit in the SUBMODULE's object database,
    // not this one, so `rev-list --objects` over the parent never lists it and
    // every spool of a repo with submodules was refused as "holds an object
    // that no published ref reaches" (dracon-platform: 884 of 1,136 examined,
    // 0 provable). The spool holds no bytes for a gitlink. The gitlink is still
    // PROVED below like every other entry: the published tip or published
    // history must record exactly (path, 160000, commit).
    let mut universe: BTreeSet<String> = entries
        .iter()
        .filter(|entry| entry.mode != GITLINK_MODE)
        .map(|entry| entry.oid.clone())
        .collect();
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
        if !published.reachable.contains(oid.as_str()) {
            return Err(NotYetPublished("spool holds an object that no published ref reaches").into());
        }
    }

    // The index must be PROVED against published state, entry by entry.
    //
    // CHANGED 2026-11-06: this used to demand that the whole index equal some
    // published ref's tree, entry for entry. That is sound but far too strict:
    // a spool is written whenever an epoch stages a WORKING TREE, and a working
    // tree normally contains the daemon's own auto-commits that have not been
    // pushed yet — so its index matched a published tree in roughly 1 spool out
    // of 50 even though every object in it was already published. The rule
    // rejected ~98% of a 86.8 GiB backlog while proving nothing extra.
    //
    // The invariant that actually matters is per-entry, not per-tree: a spool
    // can be removed when every entry it holds is already published AT THE SAME
    // PATH with the SAME mode and SAME blob. Then the spool describes a state
    // that exists verbatim in published history, and its own copy is pure
    // duplication. A path the published tree does not have, or has with a
    // different blob or mode, is NOT proved — it is work that never shipped —
    // and it preserves the spool. This is strictly narrower than "some objects
    // are published": it demands a published identity for all three of
    // (path, mode, blob), which is what makes the spool's bytes redundant.
    //
    // The object-universe reachability check above still runs first and still
    // requires EVERY retained-pack object (not just index entries) to be
    // published, so a pack holding unpublished bytes is refused even when the
    // index itself looks published.
    let reference = published
        .refs
        .iter()
        .find(|reference| {
            entries
                .iter()
                .all(|entry| reference.by_path.get(&entry.path) == Some(&(entry.mode.clone(), entry.oid.clone())))
        })
        .map(|reference| reference.name.clone());

    // ADDED 2026-11-06: an entry that does not match the published TIP is still
    // redundant when it matches a PUBLISHED HISTORICAL commit's tree. A spool
    // stages a working tree, and a file that was published in an earlier commit
    // and later edited leaves the spool holding the OLD published blob — the
    // exact same bytes, already shipped, just not at the tip. Demanding a tip
    // match held 31.4 GiB in a single repository while blocking on 8 entries out
    // of 91,073.
    //
    // The proof stays sound because "historical" is scoped to commits reachable
    // from a PUBLISHED remote-tracking ref. `refs/heads/*` is deliberately never
    // consulted: a local branch is work that may never ship, so accepting it
    // would prove redundancy against something that is not published. The
    // object-universe check above has already required every object in the
    // spool to be published, so this only decides WHERE published bytes are
    // allowed to be found.
    let reference = match reference {
        Some(reference) => Some(reference),
        None => published_history_reference(repo, &entries, published, history, timeout).await?,
    };

    let Some(reference) = reference else {
        return Err(NotYetPublished(
            "spool index holds a staged state that no published tree contains entry-for-entry",
        )
        .into());
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
    if !apply {
        // Preview: the full proof ran and passed; nothing is written.
        return Ok(Some(Removal {
            objects: universe.len(),
            reference,
        }));
    }
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
    reap_limited(repo, max_age_secs, op_timeout_secs, MAX_REDUNDANT_SPOOLS_PER_PASS, true).await
}

/// Run the full redundancy proof without removing anything.
///
/// ADDED 2026-11-12: lets an operator (and release verification) measure on
/// LIVE spools how many a pass would remove, what blocks the rest, and how
/// long a pass takes — a copied repository cannot answer that, because spool
/// identity is bound to device+inode. Read-only apart from the same transient
/// flock on the existing owner file that a real pass takes. It bypasses the
/// cooldown and records no refusal memo.
pub(super) async fn preview(repo: &Path, max_age_secs: u64, op_timeout_secs: u64) -> usize {
    reap_limited(repo, max_age_secs, op_timeout_secs, MAX_REDUNDANT_SPOOLS_PER_PASS, false).await
}

/// `reap` with an explicit per-pass spool cap, so tests can exercise
/// multi-pass progress without building thousands of spools.
async fn reap_limited(
    repo: &Path,
    max_age_secs: u64,
    op_timeout_secs: u64,
    limit: usize,
    apply: bool,
) -> usize {
    let seconds = op_timeout_secs.clamp(30, 600);
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 {
        3600
    } else {
        max_age_secs
    });
    // Housekeeping, not convergence: bound how often a repository's sync
    // worker pays for even the cheap gate below.
    if apply {
        if cooling_down(repo) {
            return 0;
        }
        note_attempt(repo);
    }
    let verb = if apply { "removed" } else { "would be removed (preview)" };
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
        let present: BTreeSet<PathBuf> = candidates.iter().cloned().collect();
        forget_vanished(&gitdir, &present);
        if candidates.is_empty() {
            return;
        }
        // An unreadable published state still lets the walk below decide (it
        // will fail closed); it only means no published-dependent memo can
        // match, so those spools are simply proved again.
        let fingerprint = published_fingerprint(repo, seconds)
            .await
            .unwrap_or_default();
        // Skip spools whose last refusal is still the answer. This is what
        // stops the oldest permanently-unprovable spools from consuming every
        // pass while the provable backlog behind them is never reached.
        let total = candidates.len();
        let candidates: Vec<PathBuf> = candidates
            .into_iter()
            .filter(|spool| !refusal_still_holds(spool, &fingerprint))
            .collect();
        let known = total - candidates.len();
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
        let budget = std::time::Duration::from_secs(REDUNDANT_PASS_BUDGET_SECS.min(seconds));
        let started = std::time::Instant::now();
        let mut refused = 0usize;
        // Refusal reasons for this pass (outermost context), summarized once.
        let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();
        let mut examined = 0usize;
        let mut history: Option<std::result::Result<PublishedHistory, String>> = None;
        for spool in candidates.iter().take(limit) {
            if started.elapsed() >= budget {
                break;
            }
            examined += 1;
            match remove_if_redundant(
                repo,
                &gitdir,
                spool,
                &published,
                width,
                floor,
                seconds,
                apply,
                &mut history,
            )
            .await
            {
                Ok(Some(removal)) => {
                    removed += 1;
                    eprintln!(
                        "🧹 {}: redundant epoch spool {} {verb}; {} object(s) proven published by {}",
                        repo.display(),
                        spool.display(),
                        removal.objects,
                        removal.reference
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    refused += 1;
                    if apply && error.downcast_ref::<PassUnavailable>().is_none() {
                        remember_refusal(spool, &error, &fingerprint);
                    }
                    // CHANGED 2026-11-13: one line per spool per pass came to
                    // ~55,000 journal lines a day (the memo is in-memory, so
                    // every restart re-logs every spool) and buried real
                    // failures. The pass summary now counts refusals by
                    // reason; the per-spool proof is a debug line.
                    *reasons.entry(error.to_string()).or_default() += 1;
                    if crate::policy::debug_enabled() {
                        eprintln!(
                            "🐛 {}: epoch spool {} preserved; not provably redundant: {error:#}",
                            repo.display(),
                            spool.display()
                        );
                    }
                }
            }
        }
        let deferred = candidates.len() - examined;
        if removed + refused > 0 || deferred > 0 {
            eprintln!(
                "🧹 {}: spool reclaim pass: {removed} {verb}, {refused} newly refused, {known} known-refused skipped, {deferred} deferred to the next pass ({:.1}s){}",
                repo.display(),
                started.elapsed().as_secs_f64(),
                refusal_summary(&reasons)
            );
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(seconds), pass).await;
    removed
}

#[cfg(test)]
#[path = "epoch_spool_redundant_tests.rs"]
mod tests;
