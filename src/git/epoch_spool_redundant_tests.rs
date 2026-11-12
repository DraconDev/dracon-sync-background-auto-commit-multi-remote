//! Tests for the redundancy-only spool reclamation route.
//!
//! Each fixture builds a real repository with a published remote-tracking
//! ref and a real spool directory (private index, retained pack, producer
//! fence and identity-bound settled marker), so the route is exercised
//! against actual Git plumbing rather than a stub.

use super::*;
use crate::git::epoch_spool::SpoolLease;
use std::process::{Command, Output};

fn command(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .current_dir(repo);
    cmd
}

fn git(repo: &Path, args: &[&str]) -> Output {
    let out = command(repo).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "Git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn git_text(repo: &Path, args: &[&str]) -> String {
    String::from_utf8(git(repo, args).stdout)
        .unwrap()
        .trim()
        .to_owned()
}

/// Write a blob into the repository's object database and return its
/// identity. It is deliberately not reachable from any ref.
fn dangling_blob(repo: &Path, payload: &[u8]) -> String {
    let mut child = command(repo)
        .arg("hash-object")
        .arg("-w")
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, message: &str) {
    git(
        repo,
        &[
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "user.name=Fixture",
            "commit",
            "-qm",
            message,
        ],
    );
}

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    spool: PathBuf,
}

/// A repository with one published commit and one published remote-tracking
/// ref, plus a spool whose content duplicates that published state.
fn fixture(label: &str) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "--template="]);
    git(&repo, &["remote", "add", "origin", "https://example.invalid/repo.git"]);
    fs::write(repo.join("a.txt"), b"alpha\n").unwrap();
    fs::create_dir(repo.join("dir")).unwrap();
    fs::write(repo.join("dir/b.txt"), b"beta\n").unwrap();
    git(&repo, &["add", "-A"]);
    commit(&repo, "init");
    // A fetch from the remote is what creates this ref in the daemon.
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);

    let spool = repo.join(".git").join(format!("dracon-staging-{label}"));
    fs::create_dir(&spool).unwrap();
    fs::copy(repo.join(".git/index"), spool.join("index")).unwrap();
    let objects = git_text(&repo, &["rev-list", "--objects", "--no-object-names", "HEAD"]);
    pack(&repo, &spool, "retained", &objects);
    Fixture {
        _root: root,
        repo,
        spool,
    }
}

/// Write a retained pack (and its index) holding the given objects.
fn pack(repo: &Path, spool: &Path, prefix: &str, objects: &str) {
    let mut child = command(repo)
        .arg("pack-objects")
        .arg(spool.join(prefix))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all((objects.to_owned() + "\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "pack fixture failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Remove the retained pack files of a spool.
fn drop_packs(spool: &Path) {
    for entry in fs::read_dir(spool).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|ext| ext == "pack" || ext == "idx" || ext == "rev")
        {
            fs::remove_file(path).unwrap();
        }
    }
}

/// Mint the producer fence and the identity-bound settled marker.
fn seal(spool: &Path) {
    let mut owner = SpoolLease::acquire(spool).unwrap();
    owner.settle_empty_fixture().unwrap();
}

/// Age the spool past the floor.
fn backdate(spool: &Path) {
    fs::File::open(spool)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
}

async fn reap(repo: &Path) -> usize {
    crate::git::reap_redundant_spools(repo, 1, 30).await
}

#[tokio::test]
async fn redundant_spool_is_removed() {
    let f = fixture("redundant");
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists(), "a provably redundant spool must be gone");
    assert!(
        f.repo.join(".git/dracon-redundant-reclaim.jsonl").is_file(),
        "a removal must leave a durable record"
    );
}

#[tokio::test]
async fn spool_below_the_age_floor_is_preserved() {
    let f = fixture("fresh");
    seal(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn dangling_index_object_is_preserved() {
    let f = fixture("dangling");
    // A blob that exists in the object database but that no published ref
    // reaches: presence is not durability.
    let blob = dangling_blob(&f.repo, b"dangling payload\n");
    assert!(
        command(&f.repo)
            .args(["cat-file", "-e", &blob])
            .status()
            .unwrap()
            .success(),
        "the fixture object must exist in the object database"
    );
    // Reference it from the private index: the spool then holds an object
    // that is present but unpublished.
    let index = f.spool.join("index");
    let added = command(&f.repo)
        .env("GIT_INDEX_FILE", &index)
        .args([
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},dangling.txt"),
        ])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "index fixture failed: {}",
        String::from_utf8_lossy(&added.stderr)
    );
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
    assert!(f.spool.join("index").is_file());
}

#[tokio::test]
async fn pack_only_unpublished_object_is_preserved() {
    let f = fixture("pack-only");
    // The index matches the published tree exactly, but the retained pack
    // additionally holds an object no published ref reaches.
    let blob = dangling_blob(&f.repo, b"pack only payload\n");
    let published = git_text(
        &f.repo,
        &["rev-list", "--objects", "--no-object-names", "HEAD"],
    );
    drop_packs(&f.spool);
    pack(&f.repo, &f.spool, "retained", &format!("{published}\n{blob}"));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn legacy_spool_without_a_fence_is_preserved() {
    let f = fixture("legacy");
    // No producer fence at all: the legacy shape must never be reaped.
    assert!(!f.spool.join("reclaim-owner.lock").exists());
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
    assert!(!f.spool.join("reclaim-settled.json").exists());
}

#[tokio::test]
async fn mismatched_settled_marker_is_preserved() {
    let f = fixture("mismatched");
    seal(&f.spool);
    // A settled marker whose recorded identity does not match the live
    // directory and lock is not a settlement proof.
    fs::write(
        f.spool.join("reclaim-settled.json"),
        br#"{"version":1,"directory_device":1,"directory_inode":2,"lock_device":3,"lock_inode":4}"#,
    )
    .unwrap();
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn preserve_marked_spool_is_preserved() {
    for marker in ["preserve-rejected", "original-index"] {
        let f = fixture("marked");
        fs::write(f.spool.join(marker), b"deliberate evidence").unwrap();
        seal(&f.spool);
        backdate(&f.spool);
        assert_eq!(reap(&f.repo).await, 0);
        assert!(f.spool.exists());
        assert_eq!(
            fs::read(f.spool.join(marker)).unwrap(),
            b"deliberate evidence"
        );
    }
}

#[tokio::test]
async fn unknown_non_empty_file_is_preserved() {
    let f = fixture("unknown-payload");
    fs::write(f.spool.join("producer-io-endpoint"), b"captured bytes").unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
    assert_eq!(
        fs::read(f.spool.join("producer-io-endpoint")).unwrap(),
        b"captured bytes"
    );
}

#[tokio::test]
async fn unknown_empty_file_is_tolerated() {
    let f = fixture("empty-unknown");
    fs::write(f.spool.join("producer-io-endpoint"), b"").unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn index_matching_a_published_historical_tree_is_redundant() {
    // RENAMED 2026-11-06. The spool was written from the FIRST commit's tree; a
    // second commit has since moved the tip. The spool's bytes are still
    // published — in history — so they are pure duplication and must go. This
    // used to assert preservation, which held 31.4 GiB hostage in one repo.
    let f = fixture("tree-mismatch");
    fs::write(f.repo.join("a.txt"), b"alpha changed\n").unwrap();
    fs::write(f.repo.join("c.txt"), b"gamma\n").unwrap();
    git(&f.repo, &["add", "-A"]);
    commit(&f.repo, "second");
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn pairing_that_exists_only_on_a_local_branch_is_preserved() {
    // The safety boundary of the historical proof, isolated precisely.
    //
    // Every BLOB here is published and every PATH is published, so neither the
    // object-universe check nor a naive path check can reject this spool. What
    // is NOT published is the (path, blob) PAIRING: `a.txt` holding beta's bytes
    // exists only on an unpushed local commit. If the historical lookup consulted
    // refs/heads/* it would find that pairing and delete the spool; scoping it to
    // refs/remotes/* correctly refuses.
    let f = fixture("local-only-history");
    let beta = git_text(&f.repo, &["rev-parse", ":dir/b.txt"]);

    // A local-only commit that moves beta's bytes to a.txt.
    fs::write(f.repo.join("a.txt"), b"beta\n").unwrap();
    git(&f.repo, &["add", "a.txt"]);
    commit(&f.repo, "swap a.txt to beta content");
    git(&f.repo, &["branch", "unpushed-work", "HEAD"]);
    // Published history does NOT contain it: origin/main stays at the first commit.
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);

    // Point the spool index at the local-only pairing.
    let mut cmd = command(&f.repo);
    cmd.env("GIT_INDEX_FILE", f.spool.join("index"))
        .args(["update-index", "--cacheinfo", &format!("100644,{beta},a.txt")]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(
        reap(&f.repo).await,
        0,
        "a pairing present only on a local branch must not prove redundancy"
    );
    assert!(f.spool.exists());
}

#[tokio::test]
async fn local_branch_only_content_is_preserved() {
    let f = fixture("local-only");
    // The spool index still describes the published tree exactly, so only
    // the reachability proof can reject this spool: its pack also holds the
    // objects of a LOCAL commit that no published ref reaches.
    let published = git_text(
        &f.repo,
        &["rev-list", "--objects", "--no-object-names", "HEAD"],
    );
    fs::write(f.repo.join("local.txt"), b"local branch payload\n").unwrap();
    git(&f.repo, &["add", "-A"]);
    commit(&f.repo, "local");
    let local = git_text(
        &f.repo,
        &["rev-list", "--objects", "--no-object-names", "HEAD"],
    );
    drop_packs(&f.spool);
    pack(&f.repo, &f.spool, "retained", &format!("{published}\n{local}"));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn unconfigured_remote_ref_is_not_published_proof() {
    let f = fixture("unconfigured-remote");
    // A tracking ref for a remote this repository does not configure is not
    // published state.
    git(&f.repo, &["update-ref", "refs/remotes/other/main", "HEAD"]);
    git(&f.repo, &["update-ref", "-d", "refs/remotes/origin/main"]);
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn repository_without_a_published_ref_preserves_everything() {
    let f = fixture("unpublished-repo");
    git(&f.repo, &["update-ref", "-d", "refs/remotes/origin/main"]);
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn concurrent_reapers_remove_only_once() {
    let f = fixture("concurrent");
    seal(&f.spool);
    backdate(&f.spool);
    let (a, b) = tokio::join!(reap(&f.repo), reap(&f.repo));
    assert_eq!(a + b, 1);
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn producer_endpoint_naming_published_objects_is_redundant() {
    let f = fixture("producer-published");
    // The real producer endpoint holds a revision expression: an object id,
    // here with Git's peel suffix and a leading `^` exclusion.
    let head = git_text(&f.repo, &["rev-parse", "HEAD"]);
    let tree = git_text(&f.repo, &["rev-parse", "HEAD^{tree}"]);
    fs::write(
        f.spool.join("producer-io-stdin"),
        format!("{head}\n^{tree}\n{head}^{{commit}}\n"),
    )
    .unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn producer_endpoint_naming_unpublished_object_is_preserved() {
    let f = fixture("producer-dangling");
    let dangling = dangling_blob(&f.repo, b"payload never published");
    fs::write(f.spool.join("producer-io-stdin"), format!("{dangling}\n")).unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
    assert_eq!(
        fs::read(f.spool.join("producer-io-stdin")).unwrap(),
        format!("{dangling}\n").as_bytes()
    );
}

#[tokio::test]
async fn producer_endpoint_holding_real_payload_is_preserved() {
    // A file that mixes an object id with other bytes still carries payload.
    let f = fixture("producer-mixed");
    let head = git_text(&f.repo, &["rev-parse", "HEAD"]);
    fs::write(
        f.spool.join("producer-io-stdout"),
        format!("{head}\nfatal: something went wrong\n"),
    )
    .unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn producer_endpoint_that_is_not_text_is_preserved() {
    let f = fixture("producer-binary");
    fs::write(f.spool.join("producer-io-stdin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
    assert_eq!(
        fs::read(f.spool.join("producer-io-stdin")).unwrap(),
        [0xff, 0xfe, 0x00, 0x01]
    );
}

#[tokio::test]
async fn producer_endpoint_without_any_object_id_is_preserved() {
    let f = fixture("producer-no-id");
    fs::write(f.spool.join("producer-io-stderr"), b"   \n\t\n").unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

/// The stem of the single retained pack a fixture spool holds.
fn retained_stem_of(spool: &Path) -> String {
    let mut stems = Vec::new();
    for entry in fs::read_dir(spool).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "pack") {
            let name = path.file_name().unwrap().to_str().unwrap();
            stems.push(name.trim_start_matches("retained-").trim_end_matches(".pack").to_owned());
        }
    }
    assert_eq!(stems.len(), 1, "fixture spool must hold one retained pack");
    stems.pop().unwrap()
}

#[tokio::test]
async fn producer_endpoint_naming_its_own_retained_pack_is_redundant() {
    let f = fixture("producer-own-pack");
    // `git pack-objects <basename>` names its output after a content hash of the
    // pack, so the producer endpoint records a pack NAME, not an object id. The
    // pack's own objects are proven reachable separately, so naming it carries
    // no unpublished bytes.
    let stem = retained_stem_of(&f.spool);
    fs::write(f.spool.join("producer-io-Stdin"), format!("{stem}\n")).unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn producer_endpoint_naming_a_foreign_pack_stem_is_preserved() {
    let f = fixture("producer-foreign-pack");
    // A hash that is neither published nor one of this spool's own packs names
    // something the proof never saw.
    let foreign = dangling_blob(&f.repo, b"not a pack this spool owns");
    assert_ne!(foreign, retained_stem_of(&f.spool));
    fs::write(f.spool.join("producer-io-Stdin"), format!("{foreign}\n")).unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn producer_endpoint_naming_an_unpaired_pack_stem_is_preserved() {
    let f = fixture("producer-unpaired-pack");
    // An index without its pack must not vouch for itself: only a pack AND
    // index pair counts as a stem this spool owns.
    let orphan = "0123456789abcdef0123456789abcdef01234567";
    fs::copy(
        f.spool.join(format!("retained-{}.idx", retained_stem_of(&f.spool))),
        f.spool.join(format!("retained-{orphan}.idx")),
    )
    .unwrap();
    fs::write(f.spool.join("producer-io-Stdin"), format!("{orphan}\n")).unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn producer_endpoint_with_a_path_is_preserved() {
    let f = fixture("producer-path");
    // A `git rm --cached <path>`-style endpoint carries repository paths, which
    // are not names the redundancy proof covers.
    fs::write(f.spool.join("producer-io-Stdin"), b"rm 'cpu-leases/unit.json'\n").unwrap();
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
    assert!(f.spool.exists());
}

#[tokio::test]
async fn a_pass_cools_the_repository_down() {
    // The reachability walk is the most expensive thing this route does, and a
    // pass that just ran cannot learn anything new about the same spools, so a
    // second pass inside the cooldown must not even scan.
    let f = fixture("cooldown");
    seal(&f.spool);
    backdate(&f.spool);
    // Seed the cooldown as a completed pass would.
    note_attempt(&f.repo);
    assert_eq!(reap(&f.repo).await, 0, "a pass inside the cooldown is skipped");
    assert!(f.spool.exists(), "the skipped pass removed nothing");
    // A different repository is unaffected by this one's cooldown.
    let other = fixture("cooldown-other");
    seal(&other.spool);
    backdate(&other.spool);
    assert_eq!(reap(&other.repo).await, 1, "an unrelated repository still reclaims");
    assert!(!other.spool.exists());
}

// ---------------------------------------------------------------------------
// CHANGED 2026-11-06: the proof is per-entry, not whole-tree equality.
//
// A spool is written whenever an epoch stages a working tree, and a working
// tree normally holds the daemon's own not-yet-pushed commits — so demanding
// that the WHOLE index equal some published tree rejected ~98% of the backlog
// even when every object was already published. What must hold instead is that
// every entry is published at the SAME path with the SAME mode and blob.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn index_that_is_a_subset_of_a_published_tree_is_redundant() {
    // The shape the old whole-tree rule wrongly refused: a partial stage whose
    // every entry is published verbatim.
    let f = fixture("subset-of-published");
    let published = git_text(&f.repo, &["rev-parse", "HEAD"]);
    fs::write(f.spool.join("producer-io-head"), format!("{published}\n")).unwrap();
    // Drop `dir/b.txt` from the spool index: the index now describes a strict
    // subset of the published tree.
    let index = f.spool.join("index");
    let mut cmd = command(&f.repo);
    cmd.env("GIT_INDEX_FILE", &index)
        .args(["rm", "--cached", "-q", "dir/b.txt"]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn index_entry_with_unpublished_content_at_a_published_path_is_preserved() {
    // Same path, different blob: the spool holds an edit that never shipped.
    let f = fixture("diverged-blob");
    fs::write(f.repo.join("a.txt"), b"edited but never published\n").unwrap();
    git(&f.repo, &["add", "a.txt"]);
    let blob = git_text(&f.repo, &["rev-parse", ":a.txt"]);
    let mut cmd = command(&f.repo);
    cmd.env("GIT_INDEX_FILE", f.spool.join("index"))
        .args(["update-index", "--cacheinfo", &format!("100644,{blob},a.txt")]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // The blob is reachable (it is in the object store via hash-object -w above)
    // so only the per-entry path/mode/blob check can catch this.
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0, "unpublished content must preserve");
    assert!(f.spool.exists());
}

#[tokio::test]
async fn index_entry_at_a_path_absent_from_published_is_preserved() {
    // A PUBLISHED blob parked at a path that is not published. Reachability
    // alone cannot catch this — the object is published — so only the per-entry
    // path/mode/blob proof stands between it and deletion. This is the test
    // that pins the per-entry rule rather than the object-universe rule.
    let f = fixture("new-path");
    let published_blob = git_text(&f.repo, &["rev-parse", ":dir/b.txt"]);
    let mut cmd = command(&f.repo);
    cmd.env("GIT_INDEX_FILE", f.spool.join("index"))
        .args(["update-index", "--add", "--cacheinfo", &format!("100644,{published_blob},relocated.txt")]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0, "a path not in published must preserve");
    assert!(f.spool.exists());
}

#[tokio::test]
async fn index_entry_using_a_published_blob_at_another_published_path_is_preserved() {
    // Blob AND path are each published, just not together: `a.txt`'s content
    // placed at `dir/b.txt`. Reachability passes, the path exists, only the
    // (path, blob) pairing proves it wrong.
    let f = fixture("crossed-pairing");
    let blob_a = git_text(&f.repo, &["rev-parse", ":a.txt"]);
    let mut cmd = command(&f.repo);
    cmd.env("GIT_INDEX_FILE", f.spool.join("index"))
        .args(["update-index", "--cacheinfo", &format!("100644,{blob_a},dir/b.txt")]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0, "a crossed pairing must preserve");
    assert!(f.spool.exists());
}

#[tokio::test]
async fn index_entry_with_a_changed_mode_is_preserved() {
    // Same blob and path but a different mode is still a different state.
    let f = fixture("mode-change");
    let blob = git_text(&f.repo, &["rev-parse", ":a.txt"]);
    let mut cmd = command(&f.repo);
    cmd.env("GIT_INDEX_FILE", f.spool.join("index"))
        .args(["update-index", "--cacheinfo", &format!("100755,{blob},a.txt")]);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0, "a mode change must preserve");
    assert!(f.spool.exists());
}

/// The historical lookup is scoped to published remote-tracking refs.
///
/// This pins the boundary directly instead of relying on a full reap: it asks
/// the helper itself, so a change that widened the scope to `refs/heads/*`
/// fails here even if reachability would happen to cover the same bytes.
#[tokio::test]
async fn published_history_lookup_is_scoped_to_remote_tracking_refs() {
    let f = fixture("history-scope");
    let beta = git_text(&f.repo, &["rev-parse", ":dir/b.txt"]);
    // A local-only commit that pairs a.txt with beta's bytes.
    fs::write(f.repo.join("a.txt"), b"beta\n").unwrap();
    git(&f.repo, &["add", "a.txt"]);
    commit(&f.repo, "local pairing");
    git(&f.repo, &["branch", "unpushed-work", "HEAD"]);
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);

    let entry = TreeEntry {
        mode: "100644".to_owned(),
        oid: beta.clone(),
        path: b"a.txt".to_vec(),
    };
    // Directly: is (a.txt, beta) in PUBLISHED history? It is only in the local
    // branch, so this must be false.
    assert!(
        !entry_is_published_at_path(&f.repo, "refs/remotes/origin/main", &entry, 60)
            .await
            .unwrap(),
        "the local-only pairing must not be found in published history"
    );
    // And the pairing IS findable on the local branch, which proves the probe
    // itself works and only the SCOPE is doing the rejecting.
    assert!(
        entry_is_published_at_path(&f.repo, "unpushed-work", &entry, 60)
            .await
            .unwrap(),
        "the probe must find the pairing on the local branch"
    );
}

// ---------------------------------------------------------------------------
// ADDED 2026-11-12: refusal memo + multi-pass progress (spool leak fix).
//
// The oldest spools are typically the permanently unprovable ones, and the
// reclaimer visits candidates oldest-first. Without a memo, every pass
// re-proved the same refusals and never reached the provable backlog behind
// them. These tests pin the memo's two halves: it must SKIP a refusal that is
// still the answer, and it must RETRY when the answer can have changed.
// ---------------------------------------------------------------------------

/// Let the next pass run immediately, as if the cooldown had expired.
fn expire_cooldown(repo: &Path) {
    REDUNDANT_ATTEMPTS
        .get_or_init(|| std::sync::Mutex::new(Default::default()))
        .lock()
        .unwrap()
        .remove(repo);
}

async fn reap_one(repo: &Path) -> usize {
    expire_cooldown(repo);
    reap_limited(repo, 1, 30, 1, true).await
}

/// A second spool of the same fixture repository, provably redundant.
fn sibling_redundant_spool(f: &Fixture, label: &str) -> PathBuf {
    let spool = f.repo.join(".git").join(format!("dracon-staging-{label}"));
    fs::create_dir(&spool).unwrap();
    fs::copy(f.repo.join(".git/index"), spool.join("index")).unwrap();
    let objects = git_text(&f.repo, &["rev-list", "--objects", "--no-object-names", "HEAD"]);
    pack(&f.repo, &spool, "retained", &objects);
    seal(&spool);
    // Newer than the fixture's backdated spool, still past the 1 s floor.
    fs::File::open(&spool)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000))
        .unwrap();
    spool
}

#[tokio::test]
async fn an_unprovable_oldest_spool_does_not_starve_the_backlog() {
    // Oldest: legacy (no fence) — refused forever. Newer: redundant.
    let f = fixture("starve-legacy");
    backdate(&f.spool);
    let newer = sibling_redundant_spool(&f, "starve-newer");

    // One spool per pass. Pass 1 spends its whole budget on the legacy spool.
    assert_eq!(reap_one(&f.repo).await, 0);
    assert!(newer.exists());
    // Pass 2 must skip the known refusal and reach the redundant spool.
    assert_eq!(reap_one(&f.repo).await, 1, "memo lets the pass reach the backlog");
    assert!(!newer.exists());
    assert!(f.spool.exists(), "the legacy spool is still preserved");
}

#[tokio::test]
async fn an_intrinsic_refusal_is_not_retried_when_published_state_moves() {
    let f = fixture("intrinsic-memo");
    backdate(&f.spool);
    assert_eq!(reap_one(&f.repo).await, 0);
    let fingerprint = published_fingerprint(&f.repo, 30).await.unwrap();
    assert!(refusal_still_holds(&f.spool, &fingerprint));
    // Publishing something new does not make a fence-less spool provable.
    fs::write(f.repo.join("c.txt"), b"gamma\n").unwrap();
    git(&f.repo, &["add", "-A"]);
    commit(&f.repo, "more");
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let moved = published_fingerprint(&f.repo, 30).await.unwrap();
    assert_ne!(moved, fingerprint);
    assert!(refusal_still_holds(&f.spool, &moved));
}

#[tokio::test]
async fn a_not_yet_published_refusal_is_retried_once_it_is_published() {
    let f = fixture("published-memo");
    // The index matches published state; the pack holds one extra object
    // that is not published yet.
    let blob = dangling_blob(&f.repo, b"shipped later\n");
    let published = git_text(&f.repo, &["rev-list", "--objects", "--no-object-names", "HEAD"]);
    drop_packs(&f.spool);
    pack(&f.repo, &f.spool, "retained", &format!("{published}\n{blob}"));
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap_one(&f.repo).await, 0);
    let before = published_fingerprint(&f.repo, 30).await.unwrap();
    assert!(
        refusal_still_holds(&f.spool, &before),
        "unchanged published state: the refusal is still the answer"
    );
    // Publish the blob in a SEPARATE ref so origin/main's tree (which the
    // index must match) stays as it was.
    let tree = String::from_utf8(
        command(&f.repo)
            .args(["mktree"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(format!("100644 blob {blob}\tlater.txt\n").as_bytes())?;
                child.wait_with_output()
            })
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let side = git_text(
        &f.repo,
        &[
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "user.name=Fixture",
            "commit-tree",
            &tree,
            "-m",
            "side",
        ],
    );
    git(&f.repo, &["update-ref", "refs/remotes/origin/side", &side]);
    let after = published_fingerprint(&f.repo, 30).await.unwrap();
    assert!(!refusal_still_holds(&f.spool, &after), "moved tips invalidate the memo");
    assert_eq!(reap_one(&f.repo).await, 1, "the spool is now provably redundant");
    assert!(!f.spool.exists());
}

#[tokio::test]
async fn a_changed_spool_invalidates_its_memo() {
    let f = fixture("changed-memo");
    backdate(&f.spool);
    assert_eq!(reap_one(&f.repo).await, 0, "legacy: refused");
    let fingerprint = published_fingerprint(&f.repo, 30).await.unwrap();
    assert!(refusal_still_holds(&f.spool, &fingerprint));
    // The fence and settled marker appear (directory mtime moves).
    seal(&f.spool);
    backdate(&f.spool);
    // backdate restores the same mtime the memo saw, so force a distinct one.
    fs::File::open(&f.spool)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(5))
        .unwrap();
    assert!(!refusal_still_holds(&f.spool, &fingerprint));
    assert_eq!(reap_one(&f.repo).await, 1);
}

#[tokio::test]
async fn preview_proves_without_removing_or_cooling_down() {
    let f = fixture("preview");
    seal(&f.spool);
    backdate(&f.spool);
    let before: Vec<_> = fs::read_dir(&f.spool).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(preview(&f.repo, 1, 30).await, 1, "the spool is provably redundant");
    assert!(f.spool.exists(), "preview removed nothing");
    let after: Vec<_> = fs::read_dir(&f.spool).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(before.len(), after.len(), "preview wrote nothing into the spool");
    assert!(!f.repo.join(".git").join(LEDGER).exists(), "preview wrote no ledger");
    // No cooldown was recorded, so a real pass right after still runs.
    assert_eq!(reap(&f.repo).await, 1);
    assert!(!f.spool.exists());
}
