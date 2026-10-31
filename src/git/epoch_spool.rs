//! Recovery spool inventory and duplication-proof reaping.
//!
//! Every staging epoch allocates `<gitdir>/dracon-staging-*`: its own index plus
//! an independent pack holding the objects it staged. A settled epoch releases
//! that directory; a refused, crashed or timed-out one leaves it behind.
//!
//! Age alone never authorizes deleting recovery data. The 2026-10-26 incident
//! removed 1,819 such directories / 83 GiB on an mtime floor and was then judged
//! unsafe: an old spool can hold the ONLY copy of an object. Both the automatic
//! reaper and `sweep_epoch_spools(apply = true)` were therefore turned into
//! fail-closed no-ops, which bounded the data loss but let the spools grow
//! forever (~15.2 GiB / 732 directories by 2026-10-31).
//!
//! This module restores reclamation WITHOUT restoring the age heuristic. The new
//! authority is duplication, which is a proof rather than an assumption:
//!
//! * every object in a spool's pack is already present in the repository's
//!   object database  -> the spool is a pure copy -> remove it;
//! * some object is absent -> pack exactly the absent objects into a compact
//!   rescue pack, index it, verify each one reads back from that pack ALONE, and
//!   only then remove the spool. The rescue keeps the data and drops the
//!   container: the measured worst case is a 4.98 GB spool whose 93,939 objects
//!   include 9 unique ones, rescued into 49,948 bytes.
//! * rescue cannot be verified -> preserve the spool untouched and report it.
//!
//! "Present in the object database" is the criterion, deliberately not
//! reachability: a present-but-dangling object still lives in `.git/objects`
//! under git's normal prune grace, so discarding the spool's duplicate of it
//! loses nothing.
//!
//! Age-only deletion remains impossible. `sweep_epoch_spools` still refuses to
//! apply because an untouched pack proves nothing on its own; only the
//! duplication test above authorizes removal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Sibling of the pack directories, so `git gc` never prunes rescued objects
/// (gc only touches `objects/`) and an operator can find them without digging.
const RESCUE_DIR: &str = "dracon-rescue";
const LEDGER_FILE: &str = "ledger.tsv";

/// Reclaim at most this many spools per repo per pass. A spool can hold a
/// multi-gigabyte pack, and verifying one means enumerating every object in it,
/// so the sweep is bounded rather than trying to drain a whole backlog in one
/// daemon pulse. Leftovers are reclaimed on the next pass.
const MAX_SPOOLS_PER_PASS: usize = 64;

pub(crate) struct SpoolLease {
    path: Option<PathBuf>,
}
impl SpoolLease {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
    pub(crate) fn named(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// Verdict for one spool after comparing its pack against the object database.
enum Verdict {
    /// Nothing in the spool is absent from the repository: a pure copy.
    Duplicate,
    /// These objects exist nowhere else. `(oid, size)` pairs, uncompressed size.
    Unique(Vec<(String, u64)>),
    /// The pack could not be read or enumerated, so no claim can be made.
    Unreadable,
}

/// Reclaim spools whose contents are provably preserved elsewhere.
///
/// Returns the number of spool directories removed.
pub(crate) async fn reap_abandoned_spools(
    repo: &Path,
    max_age_secs: u64,
    op_timeout_secs: u64,
) -> usize {
    // A live epoch rewrites its spool as it stages, so an untouched pack is the
    // in-flight guard. A zero policy value falls back to the floor rather than
    // meaning "no floor".
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 { 3600 } else { max_age_secs });
    let Some(gitdir) = absolute_git_dir(repo).await else { return 0 };
    let candidates = spool_candidates(&gitdir, floor);
    if candidates.is_empty() {
        return 0;
    }
    let timeout = op_timeout_secs.clamp(30, 600);
    let mut reaped = 0;
    for spool in candidates.into_iter().take(MAX_SPOOLS_PER_PASS) {
        // A refusal or conflict payload is deliberate diagnostic evidence. Both
        // markers exist for exactly this purpose and no proof overrides them.
        if spool.join("preserve-rejected").exists() || spool.join("original-index").exists() {
            continue;
        }
        match classify_spool(repo, &spool, timeout).await {
            Verdict::Duplicate => {
                let before = directory_bytes(&spool);
                if std::fs::remove_dir_all(&spool).is_ok() {
                    reaped += 1;
                    eprintln!(
                        "♻️  {}: reaped duplicate epoch spool {} ({})",
                        repo.display(),
                        spool.display(),
                        human_bytes(before)
                    );
                }
            }
            Verdict::Unique(unique) => {
                if rescue_and_remove(repo, &gitdir, &spool, &unique, timeout).await {
                    reaped += 1;
                }
            }
            Verdict::Unreadable => {
                eprintln!(
                    "⚠️  {}: epoch spool {} could not be verified; preserved",
                    repo.display(),
                    spool.display()
                );
            }
        }
    }
    reaped
}

/// Report retained directories. `apply` deliberately refuses destructive
/// cleanup on age: an untouched pack is not evidence that its objects survive
/// elsewhere. Reclamation happens through [`reap_abandoned_spools`], which
/// proves duplication or rescues the unique objects first.
pub(crate) async fn sweep_epoch_spools(repo: &Path, max_age_secs: u64, apply: bool) -> (usize, u64) {
    if apply {
        eprintln!(
            "⚠️ {}: recovery spools preserved; age-only deletion is unsafe",
            repo.display()
        );
        return (0, 0);
    }
    let Some(gitdir) = absolute_git_dir(repo).await else { return (0, 0) };
    let floor = std::time::Duration::from_secs(if max_age_secs == 0 { 3600 } else { max_age_secs });
    let mut count = 0;
    let mut bytes = 0;
    for spool in spool_candidates(&gitdir, floor) {
        count += 1;
        if let Ok(files) = std::fs::read_dir(&spool) {
            for file in files.flatten() {
                if let Ok(meta) = std::fs::symlink_metadata(file.path()) {
                    if meta.is_file() {
                        bytes += meta.len();
                    }
                }
            }
        }
    }
    (count, bytes)
}

/// Spool directories untouched for at least `floor`.
fn spool_candidates(gitdir: &Path, floor: std::time::Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(gitdir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if !["dracon-staging-", "dracon-epoch-message-"].iter().any(|p| name.starts_with(p)) {
                return None;
            }
            let meta = std::fs::symlink_metadata(entry.path()).ok()?;
            if !meta.is_dir() {
                return None;
            }
            // Unreadable mtime is not proof of age: skip rather than guess.
            let age = meta.modified().ok()?.elapsed().ok()?;
            (age >= floor).then(|| entry.path())
        })
        .collect();
    // Oldest first so a bounded pass makes progress instead of revisiting the
    // newest spools that the age floor already excludes.
    found.sort();
    found
}

/// Compare a spool's packs against the repository object database.
async fn classify_spool(repo: &Path, spool: &Path, timeout: u64) -> Verdict {
    let packs = retained_packs(spool);
    if packs.is_empty() {
        // No pack means the spool never wrote objects of its own: everything it
        // references lives in the main object database by construction, so there
        // is nothing here that only exists here.
        return Verdict::Duplicate;
    }
    let Some(objects) = pack_objects(repo, &packs, timeout).await else {
        return Verdict::Unreadable;
    };
    if objects.is_empty() {
        return Verdict::Duplicate;
    }
    let mut input = String::new();
    for oid in objects.keys() {
        input.push_str(oid);
        input.push('\n');
    }
    let mut command = clean_git_command();
    command.args(["cat-file", "--batch-check=%(objectname) %(objecttype)"]).current_dir(repo);
    let Ok(output) =
        crate::git::run_command_captured_output(command.into_tokio(), timeout, "spool objects", Some(input.as_bytes()))
            .await
    else {
        return Verdict::Unreadable;
    };
    if !output.status.success() {
        return Verdict::Unreadable;
    }
    let Ok(text) = std::str::from_utf8(&output.stdout) else { return Verdict::Unreadable };
    let present: std::collections::BTreeSet<&str> = text
        .lines()
        .filter(|line| !line.ends_with(" missing"))
        .filter_map(|line| line.split(' ').next())
        .collect();
    let unique: Vec<(String, u64)> = objects
        .iter()
        .filter(|(oid, _)| !present.contains(oid.as_str()))
        .map(|(oid, size)| (oid.clone(), *size))
        .collect();
    if unique.is_empty() {
        Verdict::Duplicate
    } else {
        Verdict::Unique(unique)
    }
}

/// Pack the unique objects, prove they read back, then remove the spool.
///
/// Returns true only when the removal happened. Any failure leaves the spool
/// intact: a rescue that cannot be verified is not a rescue.
async fn rescue_and_remove(
    repo: &Path,
    gitdir: &Path,
    spool: &Path,
    unique: &[(String, u64)],
    timeout: u64,
) -> bool {
    let rescue_dir = gitdir.join(RESCUE_DIR);
    if std::fs::create_dir_all(&rescue_dir).is_err() {
        return false;
    }
    let Some(alternates) = alternates_for(spool) else { return false };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let label = spool
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("spool")
        .to_string();
    let pack_path = rescue_dir.join(format!("{label}-{stamp}.pack"));

    let mut input = String::new();
    for (oid, _) in unique {
        input.push_str(oid);
        input.push('\n');
    }
    let mut command = clean_git_command();
    command
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &alternates)
        .args(["pack-objects", "--stdout"])
        .current_dir(repo);
    let Ok(output) =
        crate::git::run_command_captured_output(command.into_tokio(), timeout, "spool rescue", Some(input.as_bytes()))
            .await
    else {
        return false;
    };
    if !output.status.success() || output.stdout.is_empty() {
        return false;
    }
    if std::fs::write(&pack_path, &output.stdout).is_err() {
        return false;
    }
    // Index it so the pack is readable on its own, then verify from that pack
    // alone. The rescued objects are absent from the repository by definition,
    // so a successful read cannot be satisfied by the main object database.
    let mut index = clean_git_command();
    index.args(["index-pack", "--stdin"]).current_dir(&rescue_dir);
    let Ok(indexed) = crate::git::run_command_captured_output(
        index.into_tokio(),
        timeout,
        "spool rescue index",
        Some(&output.stdout),
    )
    .await
    else {
        let _ = std::fs::remove_file(&pack_path);
        return false;
    };
    if !indexed.status.success() {
        let _ = std::fs::remove_file(&pack_path);
        return false;
    }
    if !verify_rescue(repo, &rescue_dir, &pack_path, unique, timeout).await {
        eprintln!(
            "⚠️  {}: rescue for {} failed verification; spool preserved",
            repo.display(),
            spool.display()
        );
        let _ = std::fs::remove_file(&pack_path);
        return false;
    }

    let spool_bytes = directory_bytes(spool);
    let unique_bytes: u64 = unique.iter().map(|(_, size)| size).sum();
    if std::fs::remove_dir_all(spool).is_err() {
        return false;
    }
    append_ledger(
        &rescue_dir,
        &label,
        unique.len(),
        unique_bytes,
        spool_bytes,
        &pack_path,
    );
    eprintln!(
        "♻️  {}: rescued {} unique object(s) from epoch spool {} ({}) into {} ({})",
        repo.display(),
        unique.len(),
        spool.display(),
        human_bytes(spool_bytes),
        pack_path.display(),
        human_bytes(output.stdout.len() as u64)
    );
    true
}

/// Every rescued object must read back from the rescue pack alone.
async fn verify_rescue(
    repo: &Path,
    rescue_dir: &Path,
    pack_path: &Path,
    unique: &[(String, u64)],
    timeout: u64,
) -> bool {
    // A scratch object directory holding ONLY this pack, so the main object
    // database cannot mask a failed rescue.
    let scratch = rescue_dir.join(format!(
        ".verify-{}",
        pack_path.file_name().and_then(|n| n.to_str()).unwrap_or("pack")
    ));
    let pack_subdir = scratch.join("pack");
    if std::fs::create_dir_all(&pack_subdir).is_err() {
        return false;
    }
    let pack_name = pack_path.file_name().and_then(|n| n.to_str()).unwrap_or("pack.pack");
    if std::fs::copy(pack_path, pack_subdir.join(pack_name)).is_err() {
        let _ = std::fs::remove_dir_all(&scratch);
        return false;
    }
    if let Some(stem) = pack_name.strip_suffix(".pack") {
        if std::fs::copy(pack_path.with_extension("idx"), pack_subdir.join(format!("{stem}.idx"))).is_err() {
            let _ = std::fs::remove_dir_all(&scratch);
            return false;
        }
    }
    let mut all_ok = true;
    for (oid, _) in unique {
        let mut command = clean_git_command();
        command
            .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &scratch)
            .args(["cat-file", "-t", oid])
            .current_dir(repo);
        match crate::git::run_command_captured_output(command.into_tokio(), timeout, "spool rescue verify", None)
            .await
        {
            Ok(output) if output.status.success() => {}
            _ => {
                all_ok = false;
                break;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    all_ok
}

/// A temporary object directory exposing the spool's own packs to git.
fn alternates_for(spool: &Path) -> Option<PathBuf> {
    let packs = retained_packs(spool);
    if packs.is_empty() {
        return None;
    }
    let root = std::env::temp_dir().join(format!(
        "dracon-spool-alt-{}-{}",
        std::process::id(),
        spool.file_name().and_then(|n| n.to_str()).unwrap_or("spool")
    ));
    let pack_dir = root.join("pack");
    std::fs::create_dir_all(&pack_dir).ok()?;
    for pack in packs {
        let name = pack.file_name()?.to_str()?.to_string();
        std::fs::copy(&pack, pack_dir.join(&name)).ok()?;
        let idx = pack.with_extension("idx");
        if idx.exists() {
            let idx_name = idx.file_name()?.to_str()?.to_string();
            std::fs::copy(&idx, pack_dir.join(idx_name)).ok()?;
        }
    }
    Some(root)
}

fn retained_packs(spool: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(spool) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            (name.starts_with("retained-") && name.ends_with(".pack")).then_some(path)
        })
        .collect()
}

/// `(oid, uncompressed size)` for every object in the given packs.
async fn pack_objects(repo: &Path, packs: &[PathBuf], timeout: u64) -> Option<BTreeMap<String, u64>> {
    let mut command = clean_git_command();
    command.arg("verify-pack").arg("-v").args(packs).current_dir(repo);
    let output =
        crate::git::run_command_captured_output(command.into_tokio(), timeout, "spool pack listing", None)
            .await
            .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    let mut objects = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.split(' ');
        let (Some(oid), Some(_kind), Some(size)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        if oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(size) = size.parse::<u64>() {
            objects.insert(oid.to_string(), size);
        }
    }
    Some(objects)
}

async fn absolute_git_dir(repo: &Path) -> Option<PathBuf> {
    let mut command = clean_git_command();
    command.args(["rev-parse", "--absolute-git-dir"]).current_dir(repo);
    let output =
        crate::git::run_command_captured_output(command.into_tokio(), 60, "spool gitdir", None).await.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    let path = PathBuf::from(text.trim_end_matches('\n'));
    path.is_absolute().then_some(path)
}

/// A git command with the caller's `GIT_*` environment removed, so an inherited
/// index or object directory cannot redirect what this module inspects.
fn clean_git_command() -> crate::policy::TokioGitCommand {
    let mut command = crate::policy::tokio_git_command();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
}

fn directory_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
            if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

/// Append-only record of every rescue, so a reclaimed spool can always be
/// traced to the pack that still holds its unique objects.
fn append_ledger(rescue_dir: &Path, spool: &str, unique: usize, unique_bytes: u64, spool_bytes: u64, pack: &Path) {
    use std::io::Write;
    let path = rescue_dir.join(LEDGER_FILE);
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(
        file,
        "{stamp}\t{spool}\t{unique}\t{unique_bytes}\t{spool_bytes}\t{}",
        pack.display()
    );
}

pub(crate) fn human_bytes(bytes: u64) -> String {
    format!("{:.2} MiB", bytes as f64 / 1048576.0)
}
