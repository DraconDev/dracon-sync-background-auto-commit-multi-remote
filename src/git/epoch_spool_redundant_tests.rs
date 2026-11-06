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
async fn index_that_matches_no_published_tree_is_preserved() {
    let f = fixture("tree-mismatch");
    // Publish a second commit so the ref's tree no longer matches the spool
    // index, even though the older objects stay reachable in history.
    fs::write(f.repo.join("a.txt"), b"alpha changed\n").unwrap();
    fs::write(f.repo.join("c.txt"), b"gamma\n").unwrap();
    git(&f.repo, &["add", "-A"]);
    commit(&f.repo, "second");
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    seal(&f.spool);
    backdate(&f.spool);
    assert_eq!(reap(&f.repo).await, 0);
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
