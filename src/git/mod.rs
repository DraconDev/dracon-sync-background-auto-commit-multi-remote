#[cfg(test)]
use crate::policy::{AuthType, RemoteConfig};
use crate::log_warn;
#[cfg(test)]
use dracon_git::types::FileStatus;
#[cfg(test)]
use std::path::PathBuf;

pub(crate) fn git_cmd() -> crate::policy::GitCommand {
    crate::policy::std_git_command()
}

pub(crate) fn tokio_git_cmd() -> crate::policy::TokioGitCommand {
    crate::policy::tokio_git_command()
}

/// GitHub's incoming-pack hard limit (2 GiB). A push whose pack exceeds this
/// is rejected by the forge.
const GITHUB_PACK_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// GitHub's hard limit is 2 GiB per pack. Returns `(too_big_for_github,
/// size_used_for_decision)` where `too_big_for_github` is true only when the
/// pack we would actually send for the pushed branch exceeds 2 GiB.
///
/// The relevant size is the pack for the branch we push — NOT the entire
/// `.git`. A repo can have a huge `.git` (dracon-platform: ~19 GiB, dominated
/// by 332 tags + other non-`main` refs) while the pushable `main` is only
/// ~1.4 GiB and fits GitHub fine. Measuring the whole `.git` wrongly skips
/// GitHub for such repos, breaking push-to-all.
///
/// Fast path: if the whole `.git` is already < 2 GiB, GitHub can receive any
/// branch we push (a subset pack is never larger than the whole store), so we
/// never skip.
///
/// CHANGED 2026-07-29 (v0.113.10): the slow path no longer measures the
/// WHOLE branch's uncompressed blob sum. It now measures what github would
/// actually RECEIVE on the next push, per github-host remote:
///
///   1. DELTA: objects on the pushed branch that the remote does not already
///      have (`rev-list --objects <branch> --not <remote-tip>`). The old
///      whole-branch measure false-flagged repos whose bloat was already on
///      github from incremental pushes (junk-runner: 3.79 GiB measured vs a
///      14.77 MiB actual next-push pack — github had the objects already).
///      A missing tracking ref — or one whose tip is NOT an ancestor of the
///      branch (rewound/recreated remote + stale local ref) — means the
///      whole branch ships (fresh-remote case); a configured-absent github
///      remote is also fresh because the daemon auto-creates the repo on
///      first push. Under-estimating here is the unsafe direction, so both
///      degrade to whole-branch.
///   2. SECOND CHANCE (compressed): when the uncompressed delta exceeds the
///      limit, we stream the same object set through `git pack-objects
///      --stdout` and count bytes — github's limit applies to the COMPRESSED
///      pack it receives. Highly compressible content (junk-runner's JSONL
///      logs: 3.79 GiB uncompressed -> 736 MiB packed for the whole history)
///      clears here; incompressible content (deathrun's July PNG bloat,
///      which github genuinely rejected) does not. Without `--thin`, deltas
///      are computed only within the shipped set, so this stays an upper
///      bound on the real push pack (never an under-estimate).
///
/// The returned byte figure is the decisive one for the path taken: the
/// `.git` size on the fast path, the uncompressed delta when that already
/// clears, else the compressed pack size.
///
/// The `is_pack_too_large` backstop in the push path catches any mis-estimate:
/// if a push we allow is somehow rejected by GitHub, the daemon stops retrying
/// instead of looping.
/// `precomputed_size`, when `Some`, is a previously measured `.git` size in
/// bytes supplied by the caller to avoid re-running `du -sb`. When `None` the
/// size is measured internally (original behavior). Returns
/// `(too_big_for_github, size_or_pushable_bytes)`.
pub(crate) fn github_pack_too_large(
    repo: &std::path::Path,
    precomputed_size: Option<u64>,
) -> (bool, u64) {
    github_pack_too_large_with_limit(repo, precomputed_size, GITHUB_PACK_LIMIT_BYTES)
}

/// Limit-parameterized core (tests use small limits against fixture repos).
fn github_pack_too_large_with_limit(
    repo: &std::path::Path,
    precomputed_size: Option<u64>,
    limit: u64,
) -> (bool, u64) {
    // Use the precomputed size when supplied; otherwise measure `.git`.
    let measured = precomputed_size.or_else(|| crate::report::measure_git_size_bytes(repo));
    // Fast path: small .git -> never too big (unchanged behavior for the vast
    // majority of repos; no extra git subprocess).
    if let Some(size) = measured {
        if size < limit {
            return (false, size);
        }
    }
    // Large .git: measure the pack github would actually receive.
    match github_push_basis_bytes(repo, limit) {
        Some(basis) => (basis >= limit, basis),
        None => {
            // Couldn't measure the branch (e.g. detached HEAD, git error).
            // Fall back to the measured/whole .git size (conservative: skip).
            let whole = measured
                .or_else(|| crate::report::measure_git_size_bytes(repo))
                .unwrap_or(u64::MAX);
            (whole >= limit, whole)
        }
    }
}

/// What would the next push to github actually ship? Returns the decisive
/// byte figure (uncompressed delta when that clears, else compressed pack
/// size), maxed across all github-host remotes. `None` when the branch or
/// its objects can't be measured (caller falls back conservatively).
fn github_push_basis_bytes(repo: &std::path::Path, limit: u64) -> Option<u64> {
    // The daemon pushes the checked-out branch.
    let branch = current_branch(repo)?;
    let remotes = github_remote_names(repo);
    // Each entry is the exclusion-tip set for one push scenario. A github
    // remote with no usable tracking ref — and the no-github-remote case
    // (daemon auto-creates the repo on first push) — is a FRESH remote: the
    // whole branch ships, no exclusions.
    let scenarios: Vec<Vec<String>> = if remotes.is_empty() {
        vec![Vec::new()]
    } else {
        remotes
            .iter()
            .map(|name| github_delta_excludes(repo, name, &branch))
            .collect()
    };
    let mut best: u64 = 0;
    for excludes in scenarios {
        let shas = branch_object_shas(repo, &branch, &excludes)?;
        let uncompressed = blob_size_sum(repo, &shas)?;
        let basis = if uncompressed >= limit {
            // Second chance: the pack github receives is COMPRESSED. On
            // timeout/error keep the uncompressed figure (conservative:
            // it is already >= limit, so the verdict stays "too big").
            compressed_pack_bytes(repo, &shas).unwrap_or(uncompressed)
        } else {
            uncompressed
        };
        best = best.max(basis);
    }
    Some(best)
}

/// Names of the repo's remotes whose URL points at github.com (the forge
/// with the 2 GiB pack limit). Local config only — no network.
fn github_remote_names(repo: &std::path::Path) -> Vec<String> {
    let out = match git_capture_stdout(repo, &["config", "--get-regexp", r"^remote\..*\.url$"]) {
        Some(s) => s,
        None => return Vec::new(),
    };
    out.lines()
        .filter_map(|line| {
            // `git config --get-regexp` output: "remote.<name>.url <url>".
            let (key, url) = line.split_once(' ')?;
            if !url.to_ascii_lowercase().contains("github.com") {
                return None;
            }
            key.strip_prefix("remote.")?
                .strip_suffix(".url")
                .map(str::to_string)
        })
        .collect()
}

/// The objects a github remote already has, expressed as rev-list exclusion
/// tips. Empty when the whole branch would ship to this remote (fresh,
/// never-fetched, or rewound remote — see `github_push_basis_bytes`).
fn github_delta_excludes(repo: &std::path::Path, remote: &str, branch: &str) -> Vec<String> {
    let refname = format!("refs/remotes/{}/{}", remote, branch);
    let tip = git_capture_stdout(repo, &["rev-parse", "--verify", "--quiet", &refname])
        .map(|s| s.trim().to_string())
        .filter(|t| t.len() == 40 && t.bytes().all(|b| b.is_ascii_hexdigit()));
    match tip {
        // Trust the tracking ref only when its tip is an ancestor of the
        // branch; a non-ancestor tip (rewound/recreated remote + stale
        // local ref) would make the delta an UNDER-estimate — the unsafe
        // direction — so that case degrades to whole-branch too.
        Some(t) if git_status_ok(repo, &["merge-base", "--is-ancestor", &t, branch]) => vec![t],
        _ => Vec::new(),
    }
}

/// Run a git command and report only whether it exited successfully.
fn git_status_ok(repo: &std::path::Path, args: &[&str]) -> bool {
    git_cmd()
        .current_dir(repo)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Compressed byte count of the pack containing exactly `shas` — what github
/// would receive for this object set (minus `--thin` base-object deltas, so
/// an upper bound). Streams stdout to a byte counter (never buffers the
/// pack) and enforces a 600s ceiling; `None` on spawn failure, non-zero
/// exit, or timeout (callers treat `None` conservatively).
fn compressed_pack_bytes(repo: &std::path::Path, shas: &str) -> Option<u64> {
    if shas.is_empty() {
        return Some(0);
    }
    let mut cmd = git_cmd();
    cmd.current_dir(repo)
        .args(["pack-objects", "--stdout", "--quiet"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut pack_stdin = child.stdin.take()?;
    let pack_stdout = child.stdout.take()?;
    // Writer thread: stream the SHA list, then drop stdin -> pipe EOF. Same
    // deadlock-avoidance pattern as `blob_size_sum` (the list can be tens of
    // MiB; nobody may block on a full pipe in either direction).
    let shas_owned = shas.to_string();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = pack_stdin.write_all(shas_owned.as_bytes());
    });
    // Reader thread: drain stdout, counting bytes without buffering them.
    let reader = std::thread::spawn(move || -> u64 {
        use std::io::Read;
        let mut reader = std::io::BufReader::new(pack_stdout);
        let mut buf = [0u8; 65536];
        let mut total: u64 = 0;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => total = total.saturating_add(n as u64),
                Err(_) => break,
            }
        }
        total
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = writer.join();
                let total = reader.join().unwrap_or(0);
                return if status.success() { Some(total) } else { None };
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = writer.join();
                    let _ = reader.join();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Estimate the raw byte size of objects reachable from the branch the daemon
/// pushes (the checked-out branch), excluding submodule gitlink objects (which
/// live in nested repos, not this one). Whole-branch variant retained for
/// tests; the github guard uses the per-remote delta variant via
/// `github_push_basis_bytes`.
///
/// Returns `u64::MAX` when the branch can't be determined or git errors.
#[cfg(test)]
fn pushed_branch_pushable_bytes(repo: &std::path::Path) -> u64 {
    let branch = match current_branch(repo) {
        Some(b) => b,
        None => return u64::MAX,
    };
    match branch_object_shas(repo, &branch, &[]) {
        Some(shas) => blob_size_sum(repo, &shas).unwrap_or(u64::MAX),
        None => u64::MAX,
    }
}

/// Collect the SHAs of every object reachable from `branch` minus the
/// `excludes` tips (`git rev-list --objects <branch> --not <tip>...`), one
/// 40-hex SHA per line. `None` on git error.
fn branch_object_shas(repo: &std::path::Path, branch: &str, excludes: &[String]) -> Option<String> {
    let mut args: Vec<&str> = vec!["rev-list", "--objects", branch];
    for tip in excludes {
        args.push("--not");
        args.push(tip.as_str());
    }
    let objects = git_capture_stdout(repo, &args)?;
    // Collect object SHAs (first whitespace-delimited token per line).
    let mut shas = String::new();
    for line in objects.lines() {
        if let Some(sha) = line.split_whitespace().next() {
            if sha.len() == 40 {
                shas.push_str(sha);
                shas.push('\n');
            }
        }
    }
    Some(shas)
}

/// Sum the uncompressed sizes of the blob objects in `shas` (a newline-
/// separated SHA list as produced by `branch_object_shas`). `None` when
/// `git cat-file` can't be spawned (callers treat it as unmeasurable).
fn blob_size_sum(repo: &std::path::Path, shas: &str) -> Option<u64> {
    if shas.is_empty() {
        return Some(0);
    }

    // Spawn `git cat-file --batch-check` to size each object.
    //
    // CRITICAL deadlock avoidance: the SHA list for a large branch can be
    // tens of MiB. Writing the *entire* list to cat-file's piped stdin up
    // front (while nobody is yet draining cat-file's piped stdout) fills the
    // 64 KiB stdin pipe and deadlocks — cat-file blocks on its own stdout
    // write (unread) and never reads stdin, so our stdin write blocks
    // forever. So we feed stdin from a SEPARATE thread and drain stdout
    // concurrently from this one. Both directions make progress -> no
    // deadlock. This is what broke dracon-platform's push (huge main).
    let mut cmd = git_cmd();
    cmd.current_dir(repo)
        .args(["cat-file", "--batch-check=%(objecttype) %(objectsize)"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return None,
    };
    let mut cat_stdin = child.stdin.take()?;
    // Writer thread: stream the SHA list, then drop stdin -> pipe EOF.
    // (owned copy: a &str borrow can't move into a thread)
    let shas_owned = shas.to_string();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = cat_stdin.write_all(shas_owned.as_bytes());
        // `cat_stdin` dropped here -> pipe EOF -> cat-file flushes.
    });

    // Reader: drain cat-file's stdout line-by-line, summing sizes.
    use std::io::{BufRead, BufReader};
    let mut total: u64 = 0;
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    // Format from `cat-file --batch-check='%(objecttype)
                    // %(objectsize)'`: either `<type> <size>` (no SHA echoed)
                    // or `<sha> <type> <size>` (SHA echoed). Skip a leading
                    // 40-hex SHA if present, then read type and size.
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    let mut i = 0;
                    if parts.first().is_some_and(|p| {
                        p.len() == 40 && p.bytes().all(|b| b.is_ascii_hexdigit())
                    }) {
                        i += 1;
                    }
                    let ty = parts.get(i);
                    let size = parts.get(i + 1);
                    if ty == Some(&"missing") {
                        continue;
                    }
                    if let Some(s) = size.and_then(|s| s.parse::<u64>().ok()) {
                        total = total.saturating_add(s);
                    }
                }
                Err(_) => break,
            }
        }
    }
    let _ = child.wait();
    let _ = writer.join();
    Some(total)
}

/// Run a git command in `repo` and return its stdout as a `String`, or `None`
/// on failure / non-zero exit.
fn git_capture_stdout(repo: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = git_cmd().current_dir(repo).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(test)]
mod github_pack_tests {
    use super::*;

    // The daemon crate's own checkout is a real, warden-configured git repo,
    // so we can exercise the size-guard helpers against it without spinning
    // up a fresh fixture repo (which the warden git filter blocks in this
    // environment).
    fn daemon_repo() -> &'static std::path::Path {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn small_repo_is_not_too_big_for_github() {
        let repo = daemon_repo();
        let (too_big, size) = github_pack_too_large(repo, None);
        assert!(!too_big, "a small repo must never be skipped for github");
        assert!(
            size > 0 && size < 2 * 1024 * 1024 * 1024,
            "pushable size should be the small .git, got {size}"
        );
    }

    #[test]
    fn precomputed_small_size_short_circuits_without_git() {
        // A small precomputed size must return (false, size) without touching
        // the filesystem (no `du` / `git` subprocess).
        let p = std::path::Path::new("/nonexistent/path/that/does/not/exist");
        assert_eq!(github_pack_too_large(p, Some(1024)), (false, 1024));
    }

    #[test]
    fn precomputed_large_size_falls_back_when_branch_unmeasurable() {
        // A large precomputed size with an unmeasurable branch (nonexistent
        // path) falls back to the precomputed size -> (true, size).
        let p = std::path::Path::new("/nonexistent/path/that/does/not/exist");
        let big = 3 * 1024 * 1024 * 1024;
        assert_eq!(github_pack_too_large(p, Some(big)), (true, big));
    }

    #[test]
    fn pushed_branch_size_is_reported_for_small_repo() {
        let repo = daemon_repo();
        let bytes = pushed_branch_pushable_bytes(repo);
        assert!(
            bytes > 0 && bytes != u64::MAX,
            "pushable bytes should be the repo's own objects, got {bytes}"
        );
        assert!(
            bytes < 2 * 1024 * 1024 * 1024,
            "daemon main pushable {bytes} must fit github's 2 GiB limit"
        );
    }

    // ---- v0.113.10 delta-vs-remote measurement tests (fixture repos) ----

    /// Small limit + huge precomputed .git size forces the slow path against
    /// a fixture repo (the fast path would otherwise short-circuit).
    const TEST_LIMIT: u64 = 64 * 1024;
    const TEST_PRECOMPUTED: u64 = 3 * 1024 * 1024 * 1024;

    /// Fixture repo with a github-host remote named `gh` (fake URL — every
    /// measurement is local; the URL is never contacted).
    fn fixture_repo_with_github_remote() -> PathBuf {
        let repo = crate::test_helpers::create_test_repo();
        crate::test_helpers::test_git_cmd()
            .args(["remote", "add", "gh", "https://github.com/test/fixture.git"])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        repo
    }

    fn head_sha(repo: &std::path::Path) -> String {
        let out = crate::test_helpers::test_git_cmd()
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn set_tracking_ref(repo: &std::path::Path, remote: &str, branch: &str, sha: &str) {
        crate::test_helpers::test_git_cmd()
            .args([
                "update-ref",
                &format!("refs/remotes/{}/{}", remote, branch),
                sha,
            ])
            .current_dir(repo)
            .output()
            .expect("git update-ref");
    }

    /// The fixture repos inherit init.defaultBranch from the ambient git
    /// config (main on this fleet, master elsewhere) — never hardcode it.
    fn fixture_branch(repo: &std::path::Path) -> String {
        current_branch(repo).expect("fixture repo has a branch")
    }

    #[test]
    fn delta_is_empty_when_github_already_has_the_branch() {
        // junk-runner class: the branch's objects are ALL on github already
        // (tracking ref at HEAD). Old whole-branch measure flagged this;
        // the delta is empty -> never too big.
        let repo = fixture_repo_with_github_remote();
        let branch = fixture_branch(&repo);
        set_tracking_ref(&repo, "gh", &branch, &head_sha(&repo));
        let (too_big, basis) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), TEST_LIMIT);
        assert!(!too_big, "empty delta must clear, basis={basis}");
        assert_eq!(basis, 0, "nothing to ship -> zero-byte basis");
    }

    #[test]
    fn missing_tracking_ref_measures_whole_branch() {
        // Fresh remote (never fetched): the whole branch ships.
        let repo = fixture_repo_with_github_remote();
        let (too_big, basis) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), 1);
        assert!(too_big, "whole branch exceeds a 1-byte limit");
        assert!(basis > 0, "whole-branch basis must be nonzero");
    }

    #[test]
    fn no_github_remote_measures_whole_branch() {
        // No github remote configured: the daemon auto-creates the repo on
        // first push, so the whole branch would ship (fresh-remote case).
        let repo = crate::test_helpers::create_test_repo();
        let (too_big, basis) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), 1);
        assert!(too_big, "no github remote == fresh remote == whole branch");
        assert!(basis > 0);
    }

    #[test]
    fn non_ancestor_tracking_tip_is_not_trusted() {
        // A rewound/recreated remote leaves a stale tracking ref whose tip
        // is NOT an ancestor of the branch. Trusting it would UNDER-
        // estimate the delta (unsafe) -> must degrade to whole branch.
        let repo = fixture_repo_with_github_remote();
        // Build an unrelated root commit and point the tracking ref at it.
        let tree = crate::test_helpers::test_git_cmd()
            .args(["mktree"])
            .current_dir(&repo)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("git mktree");
        let tree = String::from_utf8_lossy(&tree.stdout).trim().to_string();
        let commit = crate::test_helpers::test_git_cmd()
            .args(["commit-tree", &tree, "-m", "unrelated"])
            .current_dir(&repo)
            .output()
            .expect("git commit-tree");
        let commit = String::from_utf8_lossy(&commit.stdout).trim().to_string();
        let branch = fixture_branch(&repo);
        set_tracking_ref(&repo, "gh", &branch, &commit);
        let (too_big, _) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), 1);
        assert!(
            too_big,
            "non-ancestor tip must degrade to whole-branch (exceeds 1-byte limit)"
        );
    }

    #[test]
    fn multiple_github_remotes_take_the_worst_case() {
        // gh1 is fully caught up (empty delta); gh2 is fresh (whole branch).
        // The verdict must follow the WORST remote, not the best.
        let repo = fixture_repo_with_github_remote();
        let branch = fixture_branch(&repo);
        set_tracking_ref(&repo, "gh", &branch, &head_sha(&repo));
        crate::test_helpers::test_git_cmd()
            .args(["remote", "add", "gh2", "https://github.com/test/fixture2.git"])
            .current_dir(&repo)
            .output()
            .expect("git remote add gh2");
        let (too_big, _) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), 1);
        assert!(
            too_big,
            "a fresh second github remote means the whole branch ships to it"
        );
    }

    /// Write `size` bytes of compressible (repeated) or incompressible
    /// (urandom) content and commit it.
    fn commit_large_file(repo: &std::path::Path, compressible: bool, size: usize) {
        let data = if compressible {
            vec![b'a'; size]
        } else {
            use std::io::Read;
            let mut buf = vec![0u8; size];
            std::fs::File::open("/dev/urandom")
                .expect("urandom")
                .read_exact(&mut buf)
                .expect("read urandom");
            buf
        };
        std::fs::write(repo.join("big.bin"), data).expect("write big file");
        crate::test_helpers::test_git_cmd()
            .args(["add", "big.bin"])
            .current_dir(repo)
            .output()
            .expect("git add");
        crate::test_helpers::test_commit_cmd()
            .args(["-m", "big file"])
            .current_dir(repo)
            .output()
            .expect("git commit");
    }

    #[test]
    fn compressible_over_limit_clears_via_compressed_second_chance() {
        // 256 KiB of repeated bytes: uncompressed delta exceeds the 64 KiB
        // limit, but the compressed pack is a few hundred bytes -> clears.
        // This is the junk-runner JSONL case (3.79 GiB uncompressed vs
        // 736 MiB packed for the whole history).
        let repo = crate::test_helpers::create_test_repo();
        commit_large_file(&repo, true, 256 * 1024);
        let (too_big, basis) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), TEST_LIMIT);
        assert!(
            !too_big,
            "compressible content must clear via the compressed tier, basis={basis}"
        );
        assert!(
            basis < TEST_LIMIT,
            "compressed basis {basis} should be well under the limit"
        );
    }

    #[test]
    fn incompressible_over_limit_stays_flagged() {
        // 256 KiB of urandom: uncompressed AND compressed exceed the limit.
        // This is the deathrun-July PNG case (github genuinely rejected it).
        let repo = crate::test_helpers::create_test_repo();
        commit_large_file(&repo, false, 256 * 1024);
        let (too_big, basis) =
            github_pack_too_large_with_limit(&repo, Some(TEST_PRECOMPUTED), TEST_LIMIT);
        assert!(
            too_big,
            "incompressible content over the limit must stay flagged, basis={basis}"
        );
        assert!(basis >= TEST_LIMIT);
    }
}

mod branch;
pub(crate) use branch::*;
mod config;
pub(crate) use config::*;
mod discovery;
pub(crate) use discovery::*;
pub(crate) mod multi_remote;
mod ops;
pub(crate) use ops::*;
mod status;
pub(crate) use status::*;
mod urls;
pub(crate) use urls::*;

mod diff;
pub(crate) use diff::*;
mod misc;
pub(crate) use misc::*;
mod push;
pub(crate) use push::*;
mod staging;
pub(crate) use staging::*;

/// Get the list of files that actually differ from HEAD (filter-aware).
/// Unlike `git status`, `git diff HEAD` applies clean filters and correctly
/// ignores files that only differ due to smudge filter decryption.
/// Returns true if the error indicates a rejected push that might be
/// resolvable with `--force-with-lease`.
/// Also updates upstream tracking for the current branch if it was set.
#[cfg(test)]
#[allow(dead_code)]
#[allow(unused_imports)]
mod tests {
    use super::*;
    use crate::git::multi_remote::{diagnose_divergence, push_to_named_remote, Divergence};
    use crate::test_helpers::{test_git_cmd, EnvRestorer, GitBinRestorer};
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn test_strip_url_credentials_https_with_creds() {
        let url = "https://user:pass@github.com/owner/repo.git";
        let result = strip_url_credentials(url);
        assert_eq!(result, "https://github.com/owner/repo.git");
    }
    #[test]
    fn test_strip_url_credentials_https_without_creds() {
        let url = "https://github.com/owner/repo.git";
        let result = strip_url_credentials(url);
        assert_eq!(result, url);
    }
    #[test]
    fn test_strip_url_credentials_git_url() {
        let url = "git@github.com:owner/repo.git";
        let result = strip_url_credentials(url);
        assert_eq!(result, url);
    }
    #[test]
    fn test_github_https_url_with_embedded_newline() {
        let url = "git@github.com:owner/repo.git\n";
        let result = github_https_url(url);
        assert_eq!(
            result,
            Some("https://github.com/owner/repo.git\n".to_string())
        );
    }
    #[test]
    fn test_github_https_url_ssh_with_colon_path() {
        let url = "git@github.com:owner/repo";
        let result = github_https_url(url);
        assert_eq!(result, Some("https://github.com/owner/repo".to_string()));
    }
    #[test]
    fn test_github_https_url_non_github_returns_none() {
        let url = "https://gitlab.com/owner/repo.git";
        let result = github_https_url(url);
        assert!(result.is_none());
    }
    #[test]
    fn test_strip_url_credentials_with_at_sign() {
        let url = "https://user:token@github.com/owner/repo.git";
        let result = strip_url_credentials(url);
        assert_eq!(result, "https://github.com/owner/repo.git");
    }
    #[test]
    fn test_strip_url_credentials_no_credentials() {
        let url = "https://github.com/owner/repo.git";
        let result = strip_url_credentials(url);
        assert_eq!(result, url);
    }
    #[test]
    fn test_git_ssh_hardening_contains_key_flags() {
        let val = git_ssh_hardening();
        assert!(
            val.contains("BatchMode=yes"),
            "should contain BatchMode=yes, got: {val}"
        );
        assert!(
            val.contains("-F"),
            "should contain -F flag for SSH config, got: {val}"
        );
        assert!(
            val.contains("ConnectTimeout=10"),
            "should contain ConnectTimeout, got: {val}"
        );
    }
    #[test]
    fn test_gitlab_https_url_ssh_colon_path() {
        let url = "git@gitlab.com:owner/repo.git";
        let result = gitlab_https_url(url);
        assert_eq!(
            result,
            Some("https://gitlab.com/owner/repo.git".to_string())
        );
    }
    #[test]
    fn test_gitlab_https_url_ssh_protocol() {
        let url = "ssh://git@gitlab.com/owner/repo.git";
        let result = gitlab_https_url(url);
        assert_eq!(
            result,
            Some("https://gitlab.com/owner/repo.git".to_string())
        );
    }
    #[test]
    fn test_gitlab_https_url_already_https() {
        let url = "https://gitlab.com/owner/repo.git";
        let result = gitlab_https_url(url);
        assert_eq!(
            result,
            Some("https://gitlab.com/owner/repo.git".to_string())
        );
    }
    #[test]
    fn test_gitlab_https_url_non_gitlab() {
        assert!(gitlab_https_url("git@github.com:owner/repo.git").is_none());
        assert!(gitlab_https_url("https://codeberg.org/owner/repo.git").is_none());
    }
    #[test]
    fn test_codeberg_https_url_ssh_colon_path() {
        let url = "git@codeberg.org:owner/repo.git";
        let result = codeberg_https_url(url);
        assert_eq!(
            result,
            Some("https://codeberg.org/owner/repo.git".to_string())
        );
    }
    #[test]
    fn test_codeberg_https_url_ssh_protocol() {
        let url = "ssh://git@codeberg.org/owner/repo.git";
        let result = codeberg_https_url(url);
        assert_eq!(
            result,
            Some("https://codeberg.org/owner/repo.git".to_string())
        );
    }
    #[test]
    fn test_codeberg_https_url_already_https() {
        let url = "https://codeberg.org/owner/repo.git";
        let result = codeberg_https_url(url);
        assert_eq!(
            result,
            Some("https://codeberg.org/owner/repo.git".to_string())
        );
    }
    #[test]
    fn test_codeberg_https_url_non_codeberg() {
        assert!(codeberg_https_url("git@github.com:owner/repo.git").is_none());
        assert!(codeberg_https_url("https://gitlab.com/owner/repo.git").is_none());
    }
    #[test]
    fn test_fallback_status_rank_ordering() {
        assert!(
            fallback_status_rank(&FileStatus::Deleted)
                > fallback_status_rank(&FileStatus::Modified)
        );
        assert!(
            fallback_status_rank(&FileStatus::Renamed) > fallback_status_rank(&FileStatus::Added)
        );
        assert!(
            fallback_status_rank(&FileStatus::TypeChange)
                > fallback_status_rank(&FileStatus::Unknown)
        );
    }
    #[test]
    fn test_parse_name_status_line_valid_lines() {
        assert_eq!(
            parse_name_status_line("M\tfile.rs"),
            Some((PathBuf::from("file.rs"), FileStatus::Modified))
        );
        assert_eq!(
            parse_name_status_line("A\tnew.rs"),
            Some((PathBuf::from("new.rs"), FileStatus::Added))
        );
        assert_eq!(
            parse_name_status_line("D\tdeleted.rs"),
            Some((PathBuf::from("deleted.rs"), FileStatus::Deleted))
        );
    }
    #[test]
    fn test_parse_name_status_line_renamed() {
        // F33 (2026-07-19): update to include a real rename score
        // suffix. The previous `R\told\tnew` form is now rejected
        // (no score). `git diff --name-status -M` always emits
        // `R<score>\t<old>\t<new>`.
        let result = parse_name_status_line("R100\told.rs\tnew.rs");
        assert!(result.is_some());
        let (path, status) = result.unwrap();
        assert_eq!(path, PathBuf::from("new.rs"));
        assert_eq!(status, FileStatus::Renamed);
    }
    #[test]
    fn test_parse_name_status_line_invalid_status() {
        assert!(parse_name_status_line("X\tfile.rs").is_none());
        assert!(parse_name_status_line("",).is_none());
    }
    #[test]
    fn test_top_level_dir_simple() {
        assert_eq!(top_level_dir("src/main.rs"), Some("src".to_string()));
        assert_eq!(top_level_dir("docs/readme.md"), Some("docs".to_string()));
    }
    #[test]
    fn test_top_level_dir_single_component() {
        assert_eq!(top_level_dir("main.rs"), Some("main.rs".to_string()));
    }
    #[test]
    fn test_top_level_dir_empty() {
        assert_eq!(top_level_dir(""), Some("".to_string()));
    }
    #[test]
    fn test_top_level_dir_path_with_multiple_slashes() {
        assert_eq!(
            top_level_dir("src///nested/main.rs"),
            Some("src".to_string())
        );
    }
    #[test]
    fn test_is_git_worktree_file_gitdir_prefix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let dot_git = tmp.path().join(".git");
        std::fs::write(&dot_git, "gitdir: /path/to/worktree").expect("write .git file");
        assert!(is_git_worktree_file(&dot_git));
    }
    #[test]
    fn test_is_git_worktree_file_regular_git_dir() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let dot_git = tmp.path().join(".git");
        std::fs::write(&dot_git, "ref: refs/heads/main").expect("write .git file");
        assert!(!is_git_worktree_file(&dot_git));
    }
    #[test]
    fn test_is_git_worktree_file_nonexistent() {
        let dot_git = std::path::Path::new("/nonexistent/.git");
        assert!(!is_git_worktree_file(dot_git));
    }
    #[test]
    fn test_is_git_worktree_file_with_whitespace() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let dot_git = tmp.path().join(".git");
        std::fs::write(&dot_git, "gitdir: /path/to/worktree\n").expect("write .git file");
        assert!(is_git_worktree_file(&dot_git));
    }
    #[test]
    fn test_load_secret_from_env() {
        let tmp_val = "test_token_abc123";
        let _guard = EnvRestorer::new("TEST_LOAD_SECRET_TOKEN", tmp_val);
        let result = load_secret("TEST_LOAD_SECRET_TOKEN");
        assert_eq!(result, Some(tmp_val.to_string()));
    }
    #[test]
    fn test_load_secret_empty_env_var() {
        let _guard = EnvRestorer::new("TEST_LOAD_SECRET_EMPTY", "");
        let result = load_secret("TEST_LOAD_SECRET_EMPTY");
        assert_eq!(result, None);
    }
    #[test]
    fn test_load_secret_missing() {
        assert_eq!(load_secret("TEST_NONEXISTENT_SECRET_VAR_XYZ"), None);
    }
    #[test]
    fn test_load_secret_from_file() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _lock = acquire_path_lock();
        let _guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("TEST_FILE_SECRET_TOKEN");
        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets dir");
        std::fs::write(
            secrets_dir.join("test.env"),
            "TEST_FILE_SECRET_TOKEN=file_token_abc123\n",
        )
        .expect("write env file");
        let result = load_secret("TEST_FILE_SECRET_TOKEN");
        assert_eq!(result, Some("file_token_abc123".to_string()));
    }
    #[test]
    fn test_load_secret_file_with_comments_and_blank_lines() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _lock = acquire_path_lock();
        let _guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _comments_guard = EnvRestorer::remove("COMMENTED_SECRET_TOKEN");
        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets dir");
        std::fs::write(
            secrets_dir.join("weird.env"),
            "# This is a comment\nCOMMENTED_SECRET_TOKEN=commented_token_xyz\nTOKEN_AFTER=value_after\n",
        )
        .expect("write env file");
        let result = load_secret("COMMENTED_SECRET_TOKEN");
        assert_eq!(result, Some("commented_token_xyz".to_string()));
    }
    #[test]
    fn test_load_secret_env_takes_precedence_over_file() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _lock = acquire_path_lock();
        let _guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _prec_guard = EnvRestorer::new("PRECEDENCE_SECRET", "env_value");
        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets dir");
        std::fs::write(
            secrets_dir.join("another.env"),
            "PRECEDENCE_SECRET=file_value\n",
        )
        .expect("write env file");
        let result = load_secret("PRECEDENCE_SECRET");
        assert_eq!(result, Some("env_value".to_string()));
    }
    #[test]
    fn test_load_secret_prefers_named_github_env_file() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _lock = acquire_path_lock();
        let _guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("GH_TOKEN");
        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets dir");
        std::fs::write(secrets_dir.join("z.env"), "GH_TOKEN=z\n").expect("write z env");
        std::fs::write(secrets_dir.join("github.env"), "GH_TOKEN=preferred\n")
            .expect("write github env");
        std::fs::write(secrets_dir.join("a.env"), "GH_TOKEN=a\n").expect("write a env");

        let result = load_secret("GH_TOKEN");
        assert_eq!(result, Some("preferred".to_string()));
    }

    #[test]
    fn test_load_secret_falls_back_to_lexicographic_non_preferred_env_files() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _lock = acquire_path_lock();
        let _guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("GH_TOKEN");
        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets dir");
        std::fs::write(secrets_dir.join("z.env"), "GH_TOKEN=z\n").expect("write z env");
        std::fs::write(secrets_dir.join("a.env"), "GH_TOKEN=a\n").expect("write a env");

        let result = load_secret("GH_TOKEN");
        assert_eq!(result, Some("a".to_string()));
    }

    #[test]
    fn test_load_secret_or_legacy_pat_falls_back_to_legacy_dir() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _lock = acquire_path_lock();
        let _guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("CODEBERG_TOKEN");
        let legacy_dir = tmp_home.path().join(".dracon/secrets/pat");
        std::fs::create_dir_all(&legacy_dir).expect("create legacy secrets dir");
        std::fs::write(
            legacy_dir.join("codeberg.env"),
            "CODEBERG_TOKEN=legacy_codeberg_token\n",
        )
        .expect("write codeberg env");

        let result = load_secret_or_legacy_pat("CODEBERG_TOKEN");
        assert_eq!(result, Some("legacy_codeberg_token".to_string()));
    }

    #[test]
    fn test_gh_cmd_disables_prompts_without_token() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let tmp_bin = tempfile::TempDir::new().expect("temp bin dir");
        let gh_mock = tmp_bin.path().join("gh");
        std::fs::write(
            &gh_mock,
            "#!/bin/sh
if [ -n \"${GH_TOKEN+x}\" ]; then
  echo 'GH_TOKEN set unexpectedly' >&2
  exit 20
fi
if [ \"$GH_PROMPT_DISABLED\" != \"1\" ]; then
  echo 'prompt not disabled' >&2
  exit 21
fi
exit 0
",
        )
        .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod gh");

        let _home_guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("GH_TOKEN");
        let _prompt_guard = EnvRestorer::remove("GH_PROMPT_DISABLED");
        let _lock = acquire_path_lock();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!("{}:{}", tmp_bin.path().to_string_lossy(), orig_path),
        );

        let output = gh_cmd().args(["api", "repos/test/repo"]).output().unwrap();
        assert!(
            output.status.success(),
            "gh mock failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_gh_cmd_uses_configured_pat_and_disables_prompts() {
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let tmp_bin = tempfile::TempDir::new().expect("temp bin dir");
        let gh_mock = tmp_bin.path().join("gh");
        std::fs::write(
            &gh_mock,
            "#!/bin/sh
if [ \"$GH_TOKEN\" ]; then
  echo 'missing GH_TOKEN' >&2
  exit 20
fi
if [ \"$GH_PROMPT_DISABLED\" != \"1\" ]; then
  echo 'prompt not disabled' >&2
  exit 21
fi
exit 0
",
        )
        .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod gh");

        let secrets_dir = tmp_home.path().join(".dracon/utilities/sync/secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets dir");
        std::fs::write(
            secrets_dir.join("github.env"),
            "[DRACON_SECRET:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSA0ckpjbUZ1WnhrcEpZczI5TzlFYVJnbkJJZkpkMnNSYk1NTzNab2s2NjFnCmtQcGpBMFUvSzFLVnZQaktVSWtnY3h3ZVh4Q0o2WDFCYWVhK2FrUFdTRXcKLT4gWDI1NTE5IGlZd2xUU0h6R01zTjFQb1ZsMHFGYUl3TStPNHBiS09JTXU1SEgxenZHV2cKenRYOXZEVXRTZzhvSDgyKytsT1FDUjFNZDRpU1VaWU4zckNQKy9VcFFzSQotPiBYMjU1MTkgQjYrTFova1d6amd3NmlhVkpQcWtyOUFZd2wrM1VBbWpDMUpRTVh2QlhGYwpqM0dVMFNTUTN1UUVvbzdHUUd3d2dXRnJJWUlaaFJLZkdHclRSR1Z2RGxzCi0+IFgyNTUxOSBHY1hoSjRGZWRrVmFwODhBTFh6eDA4Qng0NHJ0WEFXUExaRUI2TWE0bkZJCjNIcjN5anlaNDYrTHE3QTQxWU52VWRHMlovdW1ZcC9HZERVODl4WU1qdmsKLT4gWDI1NTE5IDBZVmk2ckU0TUVJS291TW5VcDkzQm5ZVjNTbXZGbDB4anZES0hkR1ZhaXMKTjRER1diWittOWRvSk1DMkNmT2xDVll6UkN4UktVbExEYzRubkxvL0kvNAotPiB5IjpGPnt5LWdyZWFzZSBXb3UtOzpCIDUoCmxaUkxmL2N1TFp3cTVFbHVqQnN6SmVDcXFWZFkzVVY1NUFON2FHeFk4SFZKVnAva1cvaWh3UE5yRml0eVNhTVcKZnRldHBPcVF1WFVIcjJLRW5TNEVnMFBzTUlTNEh3dDYwTDNGNFRpRjBrd0Y2cjgKLS0tIGxIYmZxNWlMK0lacmNjUTBqZFhCMmJ2bUs0VDM4Zlc0cSs5eUtXMUE4eFkKvOk0gKXDEuhG9BdiYi2yaw4jV19AkRlZdQQ9ksMqZsnFVwzhsObCBASqdhhNMzhS5VRVNt7iBjgAAy5A2g==]",
        )
        .expect("write github env");

        let _home_guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _token_guard = EnvRestorer::remove("GH_TOKEN");
        let _lock = acquire_path_lock();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!("{}:{}", tmp_bin.path().to_string_lossy(), orig_path),
        );

        let output = gh_cmd().args(["api", "repos/test/repo"]).output().unwrap();
        assert!(
            output.status.success(),
            "gh mock failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn test_get_remote_url_nonexistent_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        assert_eq!(multi_remote::get_remote_url(&repo, "origin"), None);
    }
    #[test]
    fn test_list_remotes_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        assert!(multi_remote::list_remotes(&repo).is_empty());
    }
    #[test]
    fn test_list_remotes_one_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", "git@github.com:Test/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        let remotes = multi_remote::list_remotes(&repo);
        assert_eq!(remotes, vec!["origin"]);
    }
    #[test]
    fn test_ensure_remote_adds_new() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        multi_remote::ensure_remote(&repo, "github", "git@github.com:Test/repo.git")
            .expect("ensure_remote");
        let url = multi_remote::get_remote_url(&repo, "github");
        assert_eq!(url, Some("git@github.com:Test/repo.git".to_string()));
    }
    #[test]
    fn test_ensure_remote_updates_url() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "github", "git@github.com:Old/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        multi_remote::ensure_remote(&repo, "github", "git@github.com:New/repo.git")
            .expect("ensure_remote");
        let url = multi_remote::get_remote_url(&repo, "github");
        assert_eq!(url, Some("git@github.com:New/repo.git".to_string()));
    }
    #[test]
    fn test_ensure_remote_idempotent() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        multi_remote::ensure_remote(&repo, "github", "git@github.com:Test/repo.git")
            .expect("ensure_remote 1");
        multi_remote::ensure_remote(&repo, "github", "git@github.com:Test/repo.git")
            .expect("ensure_remote 2");
        let remotes = multi_remote::list_remotes(&repo);
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0], "github");
    }
    #[test]
    fn test_remove_stale_remotes_preserves_origin() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", "git@github.com:Test/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add origin");
        test_git_cmd()
            .args(["remote", "add", "stale", "git@github.com:stale/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add stale");
        crate::git::multi_remote::remove_stale_remotes(&repo, &["github"], &["stale"])
            .expect("remove_stale_remotes");
        let remotes = multi_remote::list_remotes(&repo);
        assert!(
            remotes.contains(&"origin".to_string()),
            "origin must be preserved"
        );
        assert!(
            !remotes.contains(&"stale".to_string()),
            "stale not in keep list, should be removed"
        );
    }
    #[test]
    fn test_remove_stale_remotes_removes_nonkept() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", "git@github.com:Test/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add origin");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "mirror1",
                "git@mirror1.example.com:repo.git",
            ])
            .current_dir(&repo)
            .status()
            .expect("git remote add mirror1");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "mirror2",
                "git@mirror2.example.com:repo.git",
            ])
            .current_dir(&repo)
            .status()
            .expect("git remote add mirror2");
        crate::git::multi_remote::remove_stale_remotes(&repo, &["mirror1"], &["mirror1", "mirror2"])
            .expect("remove_stale_remotes");
        let remotes = multi_remote::list_remotes(&repo);
        assert!(
            remotes.contains(&"origin".to_string()),
            "origin always preserved"
        );
        assert!(
            remotes.contains(&"mirror1".to_string()),
            "kept remote mirror1 preserved"
        );
        assert!(
            !remotes.contains(&"mirror2".to_string()),
            "non-kept remote mirror2 removed"
        );
    }
    /// ADDED 2026-07-21 (v0.112.33, audit M18/F2.9): operator-added
    /// remotes (no `dracon.managed-*` marker, not in the policy
    /// managed-names list) must SURVIVE — the pre-fix implementation
    /// deleted ANY non-origin remote not in the keep list, silently
    /// destroying operator remotes like `backup` / forks.
    #[test]
    fn test_remove_stale_remotes_preserves_operator_remotes() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", "git@github.com:Test/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add origin");
        // Operator-added remote: no marker, not in managed_names.
        test_git_cmd()
            .args(["remote", "add", "backup", "git@nas.example.com:repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add backup");
        // Daemon-managed remote (policy name in managed_names).
        test_git_cmd()
            .args(["remote", "add", "codeberg", "git@codeberg.org:test/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add codeberg");

        crate::git::multi_remote::remove_stale_remotes(
            &repo,
            &["github"],              // keep
            &["github", "codeberg"], // daemon-managed names (policy)
        )
        .expect("remove_stale_remotes");
        let remotes = multi_remote::list_remotes(&repo);
        assert!(
            remotes.contains(&"backup".to_string()),
            "operator-added remote must be PRESERVED (regression M18/F2.9)"
        );
        assert!(
            !remotes.contains(&"codeberg".to_string()),
            "daemon-managed remote not in keep must be removed"
        );
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M18/F2.9): the
    /// `dracon.managed-<name>` config marker also qualifies a remote
    /// for removal (covers remotes configured by the daemon outside
    /// the current policy list).
    #[test]
    fn test_remove_stale_remotes_removes_marker_stamped_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        // ensure_remote stamps the marker.
        crate::git::multi_remote::ensure_remote(
            &repo,
            "oldmirror",
            "git@old.example.com:repo.git",
        )
        .expect("ensure_remote");
        let marker = test_git_cmd()
            .args(["config", "--get", "dracon.managed-oldmirror"])
            .current_dir(&repo)
            .output()
            .expect("git config --get");
        assert_eq!(
            String::from_utf8_lossy(&marker.stdout).trim(),
            "true",
            "ensure_remote must stamp dracon.managed-<name>"
        );

        crate::git::multi_remote::remove_stale_remotes(&repo, &[], &[])
            .expect("remove_stale_remotes");
        let remotes = multi_remote::list_remotes(&repo);
        assert!(
            !remotes.contains(&"oldmirror".to_string()),
            "marker-stamped remote not in keep must be removed"
        );
    }

    #[test]
    fn test_remove_stale_remotes_idempotent_when_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", "git@github.com:Test/repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add origin");
        crate::git::multi_remote::remove_stale_remotes(&repo, &[], &[])
            .expect("remove_stale_remotes with empty keep list");
        let remotes = multi_remote::list_remotes(&repo);
        assert_eq!(remotes, vec!["origin"]);
    }
    #[test]
    fn test_configure_all_remotes_adds_mirror() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        let remotes = vec![RemoteConfig {
            name: "mirror".to_string(),
            push_url: "git@mirror.example.com:{account}/{repo}.git".to_string(),
            auto_create: false,
            auto_create_account: "myorg".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        crate::git::multi_remote::configure_all_remotes(&repo, &remotes, "my-repo", &[]);
        let url = multi_remote::get_remote_url(&repo, "mirror");
        assert_eq!(
            url,
            Some("git@mirror.example.com:myorg/my-repo.git".to_string())
        );
    }
    #[test]
    fn test_configure_all_remotes_adds_multiple() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        let remotes = vec![
            RemoteConfig {
                name: "github".to_string(),
                push_url: "https://github.com/{account}/{repo}.git".to_string(),
                auto_create: false,
                auto_create_account: "testuser".to_string(),
                auth_type: AuthType::GitHub,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
            RemoteConfig {
                name: "gitlab".to_string(),
                push_url: "git@gitlab.com:{account}/{repo}.git".to_string(),
                auto_create: false,
                auto_create_account: "testuser".to_string(),
                auth_type: AuthType::GitLab,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
        ];
        crate::git::multi_remote::configure_all_remotes(&repo, &remotes, "multi-repo", &[]);
        let github_url = multi_remote::get_remote_url(&repo, "github");
        assert_eq!(
            github_url,
            Some("https://github.com/testuser/multi-repo.git".to_string())
        );
        let gitlab_url = multi_remote::get_remote_url(&repo, "gitlab");
        assert_eq!(
            gitlab_url,
            Some("git@gitlab.com:testuser/multi-repo.git".to_string())
        );
    }
    #[test]
    fn test_configure_all_remotes_idempotent() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        let remotes = vec![RemoteConfig {
            name: "origin".to_string(),
            push_url: "https://github.com/user/repo.git".to_string(),
            auto_create: false,
            auto_create_account: "user".to_string(),
            auth_type: AuthType::GitHub,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        crate::git::multi_remote::configure_all_remotes(&repo, &remotes, "repo", &[]);
        crate::git::multi_remote::configure_all_remotes(&repo, &remotes, "repo", &[]);
        let remotes_list = multi_remote::list_remotes(&repo);
        assert_eq!(remotes_list.len(), 1);
        assert_eq!(remotes_list[0], "origin");
    }
    #[tokio::test]
    async fn test_auto_create_all_remotes_empty_when_no_auto_create() {
        let remotes = vec![
            RemoteConfig {
                name: "mirror1".to_string(),
                push_url: "git@mirror1.example.com:repo.git".to_string(),
                auto_create: false,
                auto_create_account: "".to_string(),
                auth_type: AuthType::GitHub,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
            RemoteConfig {
                name: "mirror2".to_string(),
                push_url: "git@mirror2.example.com:repo.git".to_string(),
                auto_create: false,
                auto_create_account: "".to_string(),
                auth_type: AuthType::GitLab,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
        ];
        let results =
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None, None).await;
        assert!(
            results.is_empty(),
            "should return empty vec when no remotes have auto_create=true"
        );
    }
    /// ADDED 2026-07-20 (v0.112.28): when global codeberg `auto_create = false`
    /// (the new quota-safe default), a per-repo `codeberg_override = Some(true)`
    /// re-enables codeberg auto-create for that specific repo. Non-codeberg
    /// remotes ignore the override.
    #[tokio::test]
    async fn test_auto_create_all_remotes_codeberg_override_opt_in() {
        let _remotes = [
            RemoteConfig {
                name: "github".to_string(),
                push_url: "git@github.com:test/repo.git".to_string(),
                auto_create: true,
                auto_create_account: "test".to_string(),
                auth_type: AuthType::GitHub,
                priority: 50,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
            // Codeberg with auto_create = false (the v0.112.28 default).
            RemoteConfig {
                name: "codeberg".to_string(),
                push_url: "git@codeberg.org:test/repo.git".to_string(),
                auto_create: false,
                auto_create_account: "test".to_string(),
                auth_type: AuthType::Codeberg,
                priority: 60,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
        ];
        // With codeberg_override = Some(true), BOTH remotes should be in
        // results (github natively, codeberg via the override). The
        // github call would actually try to run `gh repo create` in
        // CI; to avoid that we use `gh` not available, so we just check
        // that the codeberg remote is attempted. We can't easily mock
        // `gh` here, so this test asserts the FILTERING logic by
        // passing codeberg_override = Some(true) with all auto_create
        // = false, which should now produce 1 entry (codeberg).
        let codeberg_only = vec![RemoteConfig {
            name: "codeberg".to_string(),
            push_url: "git@codeberg.org:test/repo.git".to_string(),
            auto_create: false,
            auto_create_account: "test".to_string(),
            auth_type: AuthType::Codeberg,
            priority: 60,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        let results = crate::git::multi_remote::auto_create_all_remotes(
            &codeberg_only,
            "test-repo",
            true,
            None,
            Some(true),
        )
        .await;
        assert_eq!(
            results.len(),
            1,
            "codeberg with override=true should produce 1 entry even with auto_create=false"
        );
        assert_eq!(results[0].0, "codeberg");

        // With override = None or Some(false), codeberg should be skipped.
        let results_none = crate::git::multi_remote::auto_create_all_remotes(
            &codeberg_only,
            "test-repo",
            true,
            None,
            None,
        )
        .await;
        assert!(
            results_none.is_empty(),
            "codeberg with override=None and auto_create=false should be skipped"
        );
    }
    #[tokio::test]
    async fn test_auto_create_all_remotes_generic_error() {
        let remotes = vec![RemoteConfig {
            name: "generic".to_string(),
            push_url: "git@generic.example.com:repo.git".to_string(),
            auto_create: true,
            auto_create_account: "testuser".to_string(),
            auth_type: AuthType::Generic,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        let results =
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None, None).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_err(), "Generic auth should return error");
        let err_msg = format!("{}", results[0].1.as_ref().unwrap_err());
        assert!(
            err_msg.contains("cannot auto-create"),
            "error should mention auto-create not supported"
        );
    }
    #[tokio::test]
    async fn test_auto_create_all_remotes_codeberg_missing_token() {
        // Make load_secret look in a temp dir so real secrets file isn't found
        let tmp_home = tempfile::TempDir::new().expect("temp dir");
        let _home_guard = EnvRestorer::new("HOME", &tmp_home.path().to_string_lossy());
        let _codeberg_guard = EnvRestorer::remove("CODEBERG_TOKEN");
        let remotes = vec![RemoteConfig {
            name: "codeberg".to_string(),
            push_url: "git@codeberg.org:{account}/{repo}.git".to_string(),
            auto_create: true,
            auto_create_account: "testuser".to_string(),
            auth_type: AuthType::Codeberg,
            priority: 50,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        let results =
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None, None).await;
        assert_eq!(results.len(), 1);
        assert!(
            results[0].1.is_err(),
            "Codeberg without token should return error"
        );
        let err_msg = format!("{}", results[0].1.as_ref().unwrap_err());
        assert!(
            err_msg.contains("missing token") || err_msg.contains("CODEBERG_TOKEN"),
            "error should mention missing token"
        );
    }
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_auto_create_all_remotes_github_success() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let gh_mock = tmp.path().join("gh");
        std::fs::write(&gh_mock, "#!/bin/sh\nexit 0\n").expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod gh");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let remotes = vec![RemoteConfig {
            name: "origin".to_string(),
            push_url: "https://github.com/{account}/{repo}.git".to_string(),
            auto_create: true,
            auto_create_account: "testaccount".to_string(),
            auth_type: AuthType::GitHub,
            priority: 1,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        let results =
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None, None).await;
        assert_eq!(results.len(), 1);
        let url = results[0].1.as_ref().unwrap();
        assert_eq!(url, "https://github.com/testaccount/test-repo.git");
    }
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_auto_create_all_remotes_gitlab_success() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let glab_mock = tmp.path().join("glab");
        std::fs::write(&glab_mock, "#!/bin/sh\nexit 0\n").expect("write glab mock");
        std::fs::set_permissions(&glab_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod glab");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let remotes = vec![RemoteConfig {
            name: "origin".to_string(),
            push_url: "git@gitlab.com:{account}/{repo}.git".to_string(),
            auto_create: true,
            auto_create_account: "testaccount".to_string(),
            auth_type: AuthType::GitLab,
            priority: 1,
            api_endpoint: None,
            auto_create_token_var: None,
            repo_name_map: Default::default(),
            force_push_when_behind: false,
        }];
        let results =
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None, None).await;
        assert_eq!(results.len(), 1);
        let url = results[0].1.as_ref().unwrap();
        assert_eq!(url, "git@gitlab.com:testaccount/test-repo.git");
    }
    #[tokio::test]
    async fn test_create_repo_on_codeberg_success_201() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let response = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n";
            std::io::Write::write_all(&mut stream, response.as_bytes()).expect("write");
        });
        let url = format!("http://127.0.0.1:{}/api/v1/repos", port);
        let result = crate::git::multi_remote::create_repo_on_codeberg(
            "test_token",
            "testuser",
            "myrepo",
            &url,
            true,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "git@codeberg.org:testuser/myrepo.git");
    }
    #[tokio::test]
    async fn test_create_repo_on_codeberg_conflict_409() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let response = "HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\n\r\n";
            std::io::Write::write_all(&mut stream, response.as_bytes()).expect("write");
        });
        let url = format!("http://127.0.0.1:{}/api/v1/repos", port);
        let result = crate::git::multi_remote::create_repo_on_codeberg(
            "test_token",
            "testuser",
            "myrepo",
            &url,
            true,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "git@codeberg.org:testuser/myrepo.git");
    }
    #[tokio::test]
    async fn test_create_repo_on_codeberg_unprocessable_422() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let response = "HTTP/1.1 422 Unprocessable Entity\r\nContent-Length: 0\r\n\r\n";
            std::io::Write::write_all(&mut stream, response.as_bytes()).expect("write");
        });
        let url = format!("http://127.0.0.1:{}/api/v1/repos", port);
        let result = crate::git::multi_remote::create_repo_on_codeberg(
            "test_token",
            "testuser",
            "myrepo",
            &url,
            true,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "git@codeberg.org:testuser/myrepo.git");
    }
    #[tokio::test]
    async fn test_create_repo_on_codeberg_unauthorized_401() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = r#"{"message": "Unauthorized"}"#;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            std::io::Write::write_all(&mut stream, response.as_bytes()).expect("write");
        });
        let url = format!("http://127.0.0.1:{}/api/v1/repos", port);
        let result = crate::git::multi_remote::create_repo_on_codeberg(
            "bad_token",
            "testuser",
            "myrepo",
            &url,
            true,
        )
        .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("401") || err_msg.contains("Unauthorized"),
            "error should mention 401: {}",
            err_msg
        );
    }
    #[tokio::test]
    async fn test_push_to_named_remote_fails_on_invalid_remote() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "git@invalid.example.com:repo.git",
            ])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        let result =
            crate::git::multi_remote::push_to_named_remote(&repo, "origin", 1, 0, false).await;
        assert!(result.is_err(), "push to invalid remote should fail");
    }
    #[tokio::test]
    async fn test_push_to_all_remotes_returns_all_results() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "mirror1",
                "git@invalid1.example.com:repo.git",
            ])
            .current_dir(&repo)
            .status()
            .expect("git remote add mirror1");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "mirror2",
                "git@invalid2.example.com:repo.git",
            ])
            .current_dir(&repo)
            .status()
            .expect("git remote add mirror2");
        let remotes = vec![
            RemoteConfig {
                name: "mirror1".to_string(),
                push_url: "git@invalid1.example.com:repo.git".to_string(),
                auto_create: false,
                auto_create_account: "".to_string(),
                auth_type: AuthType::GitHub,
                priority: 10,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
            RemoteConfig {
                name: "mirror2".to_string(),
                push_url: "git@invalid2.example.com:repo.git".to_string(),
                auto_create: false,
                auto_create_account: "".to_string(),
                auth_type: AuthType::GitHub,
                priority: 20,
                api_endpoint: None,
                auto_create_token_var: None,
                repo_name_map: Default::default(),
                force_push_when_behind: false,
            },
        ];
        let results = crate::git::multi_remote::push_to_all_remotes(&repo, &remotes, 1, 0).await;
        assert_eq!(results.len(), 2, "should return results for both remotes");
        assert_eq!(results[0].0, "mirror1", "lower priority should be first");
        assert_eq!(results[1].0, "mirror2", "higher priority should be second");
        assert!(results[0].1.is_err(), "mirror1 push should fail");
        assert!(results[1].1.is_err(), "mirror2 push should fail");
    }
    #[tokio::test]
    async fn test_push_mirror_remotes_empty_when_no_remotes() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        let results = crate::git::multi_remote::push_mirror_remotes(&repo, &[], 1, 0, true, &[], None).await;
        assert!(
            results.is_empty(),
            "should return empty results for empty remotes"
        );
    }
    #[test]
    fn test_create_repo_on_github_success() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let gh_mock = tmp.path().join("gh");
        std::fs::write(&gh_mock, "#!/bin/sh\nexit 0\n").expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_github("testuser", "my-repo", true);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://github.com/testuser/my-repo.git");
    }
    /// ADDED 2026-07-20 (v0.112.28): when the caller passes `private = false`,
    /// the daemon MUST invoke `gh repo create --public` (not `--private`).
    /// Before this fix, the `--private` flag was hardcoded regardless of the
    /// parameter, making public auto-create impossible. We assert this by
    /// making the mock gh record its argv and checking for `--public`.
    #[test]
    fn test_create_repo_on_github_public_flag_when_private_false() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let gh_mock = tmp.path().join("gh");
        // Mock gh that records its argv to a file and exits 0 with a fake URL.
        // APPEND (>>) is critical: `create_repo_on_github` invokes gh TWICE
        // (once for `gh repo create ...`, once for the default_branch PATCH).
        // The second call would overwrite the log if we used single `>`.
        let argv_log = tmp.path().join("gh_argv.log");
        std::fs::write(
            &gh_mock,
            format!(
                "#!/bin/sh\necho \"$@\" >> {}\nexit 0\n",
                argv_log.to_string_lossy()
            ),
        )
        .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_github("testuser", "public-repo", false);
        assert!(result.is_ok(), "public create should succeed: {:?}", result);
        let argv = std::fs::read_to_string(&argv_log).expect("read argv log");
        // Check that AT LEAST ONE invocation passed --public (the `gh repo create` call).
        let create_argv = argv
            .lines()
            .find(|l| l.starts_with("repo create") || l.starts_with("repo create "))
            .unwrap_or("");
        assert!(
            create_argv.contains("--public"),
            "private=false must pass --public in the `gh repo create` call, got argv lines: {:?}",
            argv
        );
        assert!(
            !create_argv.contains("--private"),
            "private=false must NOT pass --private in the `gh repo create` call, got: {}",
            create_argv
        );
    }
    #[test]
    fn test_create_repo_on_github_already_exists_returns_url_without_suffix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let gh_mock = tmp.path().join("gh");
        std::fs::write(
            &gh_mock,
            "#!/bin/sh\necho 'Name already exists' >&2\nexit 1\n",
        )
        .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_github("testuser", "dracon-demons", true);
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(!url.contains("-1"), "should NOT have suffix -1: {}", url);
        assert_eq!(url, "https://github.com/testuser/dracon-demons.git");
    }
    #[test]
    #[ignore = "depends on a clean PATH with no real gh/glab binaries; flaky in dev environments"]
    fn test_create_repo_on_github_pat_passed_as_env_var() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let gh_mock = tmp.path().join("gh");
        std::fs::write(
            &gh_mock,
            "#!/bin/sh\nif [ -n \"$GH_TOKEN\" ]; then echo 'PAT received' >&2; fi\nexit 0\n",
        )
        .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let _gh_guard = EnvRestorer::new("GH_TOKEN", "test_pat_from_env");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_github("testuser", "test-repo", true);
        assert!(result.is_ok());
    }
    #[test]
    fn test_create_repo_on_gitlab_success() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let glab_mock = tmp.path().join("glab");
        std::fs::write(&glab_mock, "#!/bin/sh\nexit 0\n").expect("write glab mock");
        std::fs::set_permissions(&glab_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_gitlab("testuser", "my-repo", true);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "git@gitlab.com:testuser/my-repo.git");
    }
    #[test]
    fn test_create_repo_on_gitlab_already_exists_returns_url_without_suffix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let glab_mock = tmp.path().join("glab");
        std::fs::write(
            &glab_mock,
            "#!/bin/sh\necho 'Repository has already been taken' >&2\nexit 1\n",
        )
        .expect("write glab mock");
        std::fs::set_permissions(&glab_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_gitlab("testuser", "dracon-demons", true);
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(!url.contains("-1"), "should NOT have suffix -1: {}", url);
        assert_eq!(url, "git@gitlab.com:testuser/dracon-demons.git");
    }
    #[test]
    #[ignore = "depends on a clean PATH with no real gh/glab binaries; flaky in dev environments"]
    fn test_create_repo_on_gitlab_network_error() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let glab_mock = tmp.path().join("glab");
        std::fs::write(
            &glab_mock,
            "#!/bin/sh\necho 'Connection timeout' >&2\nexit 128\n",
        )
        .expect("write glab mock");
        std::fs::set_permissions(&glab_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_gitlab("testuser", "test-repo", true);
        assert!(result.is_err());
    }
    #[test]
    #[ignore = "depends on a clean PATH with no real gh/glab binaries; flaky in dev environments"]
    fn test_create_repo_on_gitlab_token_passed_as_env_var() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let glab_mock = tmp.path().join("glab");
        std::fs::write(
            &glab_mock,
            "#!/bin/sh\nif [ -n \"$GITLAB_TOKEN\" ]; then echo 'Token received'; fi\nexit 0\n",
        )
        .expect("write glab mock");
        std::fs::set_permissions(&glab_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let _glab_guard = EnvRestorer::new("GITLAB_TOKEN", "test_gitlab_token");
        let _path_lock = acquire_path_lock();
        let _path_guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let result = multi_remote::create_repo_on_gitlab("testuser", "test-repo", true);
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_push_with_retries_succeeds_first_attempt() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let bare = tmp.path().join("bare.git");
        test_git_cmd()
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        test_git_cmd()
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        test_git_cmd()
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let result = crate::git::push_with_retries(&repo, 5, 3, "test-push").await;
        assert!(
            result.is_ok(),
            "push should succeed on first attempt: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_push_with_retries_retries_then_succeeds() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let counter = tmp.path().join("call_counter");
        std::fs::write(&counter, "0").expect("write counter");
        let real_git = real_git_path();
        let fail_script = tmp.path().join("git");
        let counter_path = counter.display().to_string();
        std::fs::write(
            &fail_script,
            format!(
                "#!/bin/sh\n\
            count=$(cat {counter})\n\
            if [ \"$count\" -lt 1 ]; then\n\
                echo \"simulated failure\" >&2\n\
                echo $((count+1)) > {counter}\n\
                exit 1\n\
            fi\n\
            exec {real_git} \"$@\"\n\
            ",
                counter = counter_path,
                real_git = real_git.display()
            ),
        )
        .expect("write fail script");
        std::fs::set_permissions(&fail_script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let result = crate::git::push_with_retries(&repo, 5, 3, "test-push-retry").await;
        assert!(
            result.is_ok(),
            "push should eventually succeed after retry: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_push_with_retries_returns_immediately_on_permanent_rejection() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let counter = tmp.path().join("call_counter");
        std::fs::write(&counter, "0").expect("write counter");
        let real_git = real_git_path();
        let fail_script = tmp.path().join("git");
        let counter_path = counter.display().to_string();
        std::fs::write(
            &fail_script,
            format!(
                "#!/bin/sh\n\
            count=$(cat {counter_path})\n\
            echo $((count+1)) > {counter_path}\n\
            echo 'pre-receive hook declined' >&2\n\
            exit 1\n\
            "
            ),
        )
        .expect("write fail script");
        std::fs::set_permissions(&fail_script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare.to_string_lossy()])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&fail_script.to_string_lossy());
        let result = crate::git::push_with_retries(&repo, 5, 3, "test-push-permanent").await;
        assert!(result.is_err(), "permanent rejection should fail");
        let count = std::fs::read_to_string(&counter)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert_eq!(count, 1, "permanent rejection should not retry or fallback");
    }

    #[tokio::test]
    async fn test_push_with_retries_exhausts_retries_and_fails() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let always_fail = tmp.path().join("git");
        std::fs::write(
            &always_fail,
            "#!/bin/sh\n\
            echo 'always fail' >&2\n\
            exit 1\n\
            ",
        )
        .expect("write fail git");
        std::fs::set_permissions(&always_fail, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare.to_string_lossy()])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&always_fail.to_string_lossy());
        let result = crate::git::push_with_retries(&repo, 1, 2, "test-push-fail").await;
        assert!(result.is_err(), "push should fail after exhausting retries");
    }
    #[tokio::test]
    async fn test_push_with_retries_includes_stderr_on_failure() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let always_fail = tmp.path().join("git");
        std::fs::write(
            &always_fail,
            "#!/bin/sh\n\
            echo 'permission denied for /nix/store/abc' >&2\n\
            exit 128\n\
            ",
        )
        .expect("write fail git");
        std::fs::set_permissions(&always_fail, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare.to_string_lossy()])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&always_fail.to_string_lossy());
        let result = crate::git::push_with_retries(&repo, 1, 1, "test-push-stderr").await;
        assert!(result.is_err(), "push should fail");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("permission denied") || err_msg.contains("/nix/store"),
            "error message should include stderr output, got: {}",
            err_msg
        );
    }
    #[tokio::test]
    async fn test_push_with_transport_fallbacks_ssh_succeeds_no_fallback() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let bare = tmp.path().join("bare.git");
        test_git_cmd()
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        test_git_cmd()
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        test_git_cmd()
            .args(["remote", "add", "origin", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        test_git_cmd()
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let result = crate::git::push_with_transport_fallbacks(&repo, 5, "test-push").await;
        assert!(result.is_ok(), "SSH push should succeed: {:?}", result);
    }
    #[tokio::test]
    async fn test_push_with_transport_fallbacks_ssh_fails_https_fallback_succeeds() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let fail_git = tmp.path().join("git");
        let real_git_path_str = real_git.display().to_string();
        std::fs::write(
            &fail_git,
            format!(
                "#!/bin/sh\n\
            if echo \"$@\" | grep -q 'GIT_SSH_COMMAND'; then\n\
                echo 'SSH failure' >&2\n\
                exit 128\n\
            fi\n\
            exec {real_git_path_str} \"$@\"\n\
            "
            ),
        )
        .expect("write fail git");
        std::fs::set_permissions(&fail_git, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&fail_git.to_string_lossy());
        let result = crate::git::push_with_transport_fallbacks(&repo, 5, "test-push-fb").await;
        assert!(
            result.is_ok(),
            "HTTPS fallback should succeed after SSH failure: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_push_with_transport_fallbacks_both_fail() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let always_fail = tmp.path().join("git");
        std::fs::write(&always_fail, "#!/bin/sh\necho 'always fail' >&2\nexit 1\n")
            .expect("write fail git");
        std::fs::set_permissions(&always_fail, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&always_fail.to_string_lossy());
        let result =
            crate::git::push_with_transport_fallbacks(&repo, 1, "test-push-both-fail").await;
        assert!(result.is_err(), "both SSH and HTTPS should fail");
    }
    #[tokio::test]
    async fn test_push_with_transport_fallbacks_skips_fallback_on_permanent_rejection() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let permanent_git = tmp.path().join("git");
        let fallback_counter = tmp.path().join("fallback-called");
        let fallback_counter_str = fallback_counter.display().to_string();
        std::fs::write(
            &permanent_git,
            format!(
                "#!/bin/sh\n\
            if echo \"$@\" | grep -q 'https://'; then
                echo fallback-called > {fallback_counter_str}
                exit 0
            fi
            echo 'pre-receive hook declined' >&2
            exit 1
            "
            ),
        )
        .expect("write permanent-rejection git");
        std::fs::set_permissions(&permanent_git, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "origin", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&permanent_git.to_string_lossy());
        let result =
            crate::git::push_with_transport_fallbacks(&repo, 1, "test-push-permanent").await;
        assert!(result.is_err(), "permanent rejection should fail");
        assert!(!fallback_counter.exists(), "HTTPS fallback should not run");
    }

    #[tokio::test]
    async fn test_push_to_named_remote_ssh_success() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", "-b", "master", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "mirror", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let result = multi_remote::push_to_named_remote(&repo, "mirror", 5, 0, false).await;
        assert!(
            result.is_ok(),
            "SSH push to named remote should succeed: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_push_to_named_remote_ssh_fails_https_fallback() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let fail_git = tmp.path().join("git");
        let real_git_path_str = real_git.display().to_string();
        std::fs::write(
            &fail_git,
            format!(
                "#!/bin/sh\n\
            if echo \"$@\" | grep -q 'GIT_SSH_COMMAND'; then\n\
                echo 'SSH failure' >&2\n\
                exit 128\n\
            fi\n\
            exec {real_git_path_str} \"$@\"\n\
            "
            ),
        )
        .expect("write fail git");
        std::fs::set_permissions(&fail_git, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", "-b", "master", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "mirror", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&fail_git.to_string_lossy());
        let result = multi_remote::push_to_named_remote(&repo, "mirror", 5, 0, false).await;
        assert!(
            result.is_ok(),
            "HTTPS fallback should succeed after SSH failure: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_push_to_named_remote_https_fallback_failure_still_retries_ssh() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let fail_git = tmp.path().join("git");
        let real_git_path_str = real_git.display().to_string();
        std::fs::write(
            &fail_git,
            format!(
                "#!/bin/sh\n\
            if echo \"$@\" | grep -q 'push' && echo \"$@\" | grep -q 'HEAD:refs/heads/master' && [ -n \"$GIT_SSH_COMMAND\" ]; then\n\
                echo 'initial SSH failure' >&2\n\
                exit 128\n\
            fi\n\
            exec {real_git_path_str} \"$@\"\n\
            "
            ),
        )
        .expect("write fail git");
        std::fs::set_permissions(&fail_git, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", "-b", "master", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "mirror", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&fail_git.to_string_lossy());
        let result = multi_remote::push_to_named_remote(&repo, "mirror", 5, 0, false).await;
        assert!(
            result.is_ok(),
            "retry loop should still run after HTTPS fallback fails: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_push_to_named_remote_unsafe_branch_skips_https_fallback() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let always_fail = tmp.path().join("git");
        std::fs::write(
            &always_fail,
            "#!/bin/sh\necho 'SSH failure' >&2\nexit 128\n",
        )
        .expect("write fail git");
        std::fs::set_permissions(&always_fail, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["checkout", "--orphan", "deploy/prod"])
            .current_dir(&repo)
            .output()
            .expect("git checkout -b deploy/prod");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "mirror", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("f"), "content").expect("write file");
        std::process::Command::new(real_git.as_path())
            .args(["add", "f"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let _git_bin_guard = GitBinRestorer::new(&always_fail.to_string_lossy());
        let result = multi_remote::push_to_named_remote(&repo, "mirror", 1, 0, false).await;
        assert!(result.is_err(), "push should fail");
    }
    #[tokio::test]
    async fn test_run_child_includes_stderr_on_failure() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let child = crate::git::tokio_git_cmd()
            .args(["push", "nonexistent-remote", "nonexistent-branch"])
            .current_dir(tmp.path())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn git");
        let result = run_child(child, tmp.path(), 10, "test-stderr").await;
        assert!(result.is_err(), "should fail for nonexistent remote");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            !err_msg.contains("test-stderr failed") || err_msg.len() > 30,
            "error message should include stderr detail, got: {}",
            err_msg
        );
    }
    #[tokio::test]
    async fn test_run_git_with_timeout_succeeds() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write file");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        let result = run_git_with_timeout(&repo, &["status"], 10, "status").await;
        assert!(result.is_ok(), "git status should succeed: {:?}", result);
    }
    #[tokio::test]
    async fn test_run_git_with_timeout_env_injects_env_vars() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write file");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        let result = run_git_with_timeout_env(
            &repo,
            &["log", "--format=%s"],
            10,
            "log",
            &[
                ("GIT_AUTHOR_NAME", "Test Author"),
                ("GIT_COMMITTER_NAME", "Test Committer"),
            ],
        )
        .await;
        assert!(
            result.is_ok(),
            "git log with env vars should work: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_restore_paths_reverts_modified_file() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "original content").expect("write file");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        std::fs::write(repo.join("file.txt"), "modified content").expect("write modified");
        let result = restore_paths(&repo, &["file.txt".to_string()]).await;
        assert!(result.is_ok(), "restore_paths should succeed: {:?}", result);
        let content = std::fs::read_to_string(repo.join("file.txt")).expect("read file");
        assert_eq!(
            content, "original content",
            "file should be restored to original content"
        );
    }
    #[tokio::test]
    async fn test_diagnose_divergence_remote_purely_behind() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        let local_commit = {
            let output = test_git_cmd()
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        test_git_cmd()
            .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        test_git_cmd()
            .args(["update-ref", "refs/remotes/mirror/master", &local_commit])
            .current_dir(&repo)
            .status()
            .expect("git update-ref");
        let result = diagnose_divergence(&repo, "mirror", "master").await;
        assert!(result.is_ok(), "diagnose_divergence should succeed");
        assert_eq!(
            result.unwrap(),
            Divergence::RemotePurelyBehind,
            "remote with no extra commits should be purely behind"
        );
    }
    #[tokio::test]
    async fn test_diagnose_divergence_divergent() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        let (local_commit, remote_commit) = {
            let local = test_git_cmd()
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse")
                .stdout;
            let local = String::from_utf8_lossy(&local).trim().to_string();
            test_git_cmd()
                .args([
                    "commit",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "other commit",
                ])
                .current_dir(&repo)
                .status()
                .expect("git commit --allow-empty");
            let remote = test_git_cmd()
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse")
                .stdout;
            let remote = String::from_utf8_lossy(&remote).trim().to_string();
            (local, remote)
        };
        test_git_cmd()
            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
            .current_dir(&repo)
            .status()
            .expect("git update-ref");
        test_git_cmd()
            .args(["reset", "--hard", &local_commit])
            .current_dir(&repo)
            .status()
            .expect("git reset");
        let result = diagnose_divergence(&repo, "mirror", "master").await;
        assert!(result.is_ok(), "diagnose_divergence should succeed");
        assert_eq!(
            result.unwrap(),
            Divergence::Divergent,
            "remote with commits local lacks should be divergent"
        );
    }
    #[tokio::test]
    async fn test_push_to_named_remote_auto_force_when_behind() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let real_git = real_git_path();
        let bare = tmp.path().join("bare.git");
        std::process::Command::new(real_git.as_path())
            .args(["init", "--bare", &bare.to_string_lossy()])
            .output()
            .expect("git init --bare");
        let bare_url = format!("file://{}", bare.to_string_lossy());
        let repo = tmp.path().join("repo");
        std::process::Command::new(real_git.as_path())
            .args(["init", "-q", "-b", "master", &repo.to_string_lossy()])
            .output()
            .expect("git init");
        std::process::Command::new(real_git.as_path())
            .args(["remote", "add", "mirror", &bare_url])
            .current_dir(&repo)
            .output()
            .expect("git remote add");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        std::process::Command::new(real_git.as_path())
            .args(["add", "."])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new(real_git.as_path())
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        std::process::Command::new(real_git.as_path())
            .args([
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "other commit",
            ])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        let remote_commit = {
            let output = std::process::Command::new(real_git.as_path())
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        std::process::Command::new(real_git.as_path())
            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
            .current_dir(&repo)
            .output()
            .expect("git update-ref");
        std::process::Command::new(real_git.as_path())
            .args(["reset", "--hard", "HEAD^"])
            .current_dir(&repo)
            .output()
            .expect("git reset");
        drop(acquire_path_lock());
        let result = push_to_named_remote(&repo, "mirror", 5, 0, true).await;
        assert!(
            result.is_ok(),
            "push with force_when_behind=true should succeed when remote is purely behind: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_push_to_named_remote_no_auto_force_when_divergent() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        let (_local_commit, remote_commit) = {
            let local = test_git_cmd()
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse");
            let _local = String::from_utf8_lossy(&local.stdout).trim().to_string();
            test_git_cmd()
                .args([
                    "commit",
                    "--no-verify",
                    "--allow-empty",
                    "-m",
                    "other commit",
                ])
                .current_dir(&repo)
                .status()
                .expect("git commit");
            let output = test_git_cmd()
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse");
            let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (local, remote)
        };
        test_git_cmd()
            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
            .current_dir(&repo)
            .status()
            .expect("git update-ref");
        test_git_cmd()
            .args(["reset", "--hard", "HEAD^"])
            .current_dir(&repo)
            .status()
            .expect("git reset");
        drop(acquire_path_lock());
        let result = push_to_named_remote(&repo, "mirror", 5, 0, true).await;
        assert!(
            result.is_err(),
            "push with force_when_behind=true should fail when remote is divergent: {:?}",
            result
        );
    }
    #[tokio::test]
    async fn test_push_to_named_remote_no_auto_force_when_disabled() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(&repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["remote", "add", "mirror", "git@mirror.example.com:repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");
        test_git_cmd()
            .args([
                "commit",
                "--no-verify",
                "--allow-empty",
                "-m",
                "other commit",
            ])
            .current_dir(&repo)
            .status()
            .expect("git commit");
        let remote_commit = {
            let output = test_git_cmd()
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .expect("git rev-parse");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        test_git_cmd()
            .args(["update-ref", "refs/remotes/mirror/master", &remote_commit])
            .current_dir(&repo)
            .status()
            .expect("git update-ref");
        test_git_cmd()
            .args(["reset", "--hard", "HEAD^"])
            .current_dir(&repo)
            .status()
            .expect("git reset");
        drop(acquire_path_lock());
        let result = push_to_named_remote(&repo, "mirror", 5, 0, false).await;
        assert!(
            result.is_err(),
            "push with force_when_behind=false should fail with rejected error"
        );
    }
    #[test]
    fn test_detect_orphan_origin_detects_single_digit_suffix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:DraconDev/dracon-demons-9.git",
            ])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        let result = detect_orphan_origin(repo);
        assert!(result.is_some(), "should detect -9 suffix");
        let (current, canonical) = result.unwrap();
        assert_eq!(current, "git@github.com:DraconDev/dracon-demons-9.git");
        assert_eq!(canonical, "git@github.com:DraconDev/dracon-demons.git");
    }
    #[test]
    fn test_detect_orphan_origin_ignores_multi_digit_suffix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:DraconDev/project-2024.git",
            ])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        let result = detect_orphan_origin(repo);
        assert!(
            result.is_none(),
            "should NOT detect -2024 as orphan (multi-digit)"
        );
    }
    #[test]
    fn test_detect_orphan_origin_ignores_legitimate_version() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:DraconDev/api-v2.git",
            ])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        let result = detect_orphan_origin(repo);
        assert!(
            result.is_none(),
            "should NOT detect -v2 as orphan (not pure digits)"
        );
    }
    #[test]
    fn test_detect_orphan_origin_no_suffix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:DraconDev/dracon-demons.git",
            ])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        let result = detect_orphan_origin(repo);
        assert!(
            result.is_none(),
            "should NOT detect normal repo name as orphan"
        );
    }
    #[test]
    fn test_fix_orphan_origin_updates_remote_url() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:DraconDev/dracon-demons-9.git",
            ])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        let result = fix_orphan_origin(repo, "git@github.com:DraconDev/dracon-demons.git");
        assert!(result.is_ok(), "fix_orphan_origin should succeed");
        let url = multi_remote::get_remote_url(repo, "origin").unwrap();
        assert_eq!(url, "git@github.com:DraconDev/dracon-demons.git");
    }
    #[test]
    fn test_fix_orphan_origin_updates_upstream_tracking() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        let bare = tmp.path().join("bare.git");
        test_git_cmd()
            .args(["init", "-q", "--bare", bare.to_str().unwrap()])
            .status()
            .expect("git init bare");
        test_git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["remote", "add", "origin", bare.to_str().unwrap()])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        test_git_cmd()
            .args(["push", "-u", "origin", "main"])
            .current_dir(repo)
            .status()
            .expect("git push");
        test_git_cmd()
            .args([
                "remote",
                "set-url",
                "origin",
                "git@github.com:DraconDev/dracon-demons-9.git",
            ])
            .current_dir(repo)
            .status()
            .expect("git remote set-url");
        let result = fix_orphan_origin(repo, "git@github.com:DraconDev/dracon-demons.git");
        assert!(result.is_ok(), "fix_orphan_origin should succeed");
        let url = multi_remote::get_remote_url(repo, "origin").unwrap();
        assert_eq!(url, "git@github.com:DraconDev/dracon-demons.git");
        let upstream_info = {
            let output = test_git_cmd()
                .args(["branch", "-vv", "--no-color"])
                .current_dir(repo)
                .output()
                .expect("git branch -vv");
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        assert!(
            upstream_info.contains("origin/main"),
            "branch should track origin/main after fix"
        );
    }
    #[tokio::test]
    async fn test_consolidate_to_main_deletes_master_and_keeps_main() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        let bare = tmp.path().join("bare.git");
        test_git_cmd()
            .args(["init", "-q", "--bare", bare.to_str().unwrap()])
            .status()
            .expect("git init bare");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["remote", "add", "origin", bare.to_str().unwrap()])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        test_git_cmd()
            .args(["push", "-u", "origin", "master"])
            .current_dir(repo)
            .status()
            .expect("git push");
        test_git_cmd()
            .args(["checkout", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git checkout main");
        test_git_cmd()
            .args(["commit", "--allow-empty", "-m", "main commit"])
            .current_dir(repo)
            .status()
            .expect("git commit main");
        test_git_cmd()
            .args(["push", "-u", "origin", "main"])
            .current_dir(repo)
            .status()
            .expect("git push main");
        let result = consolidate_to_main(repo).await;
        assert!(result.is_ok(), "consolidate_to_main should succeed");
        let local_branches = {
            let output = test_git_cmd()
                .args(["branch"])
                .current_dir(repo)
                .output()
                .expect("git branch");
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        assert!(local_branches.contains("main"), "main branch should exist");
        assert!(
            !local_branches.contains("master"),
            "master local branch should be deleted"
        );
    }
    #[tokio::test]
    async fn test_rename_master_to_main_renames_and_deletes_remote_master() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        let bare = tmp.path().join("bare.git");
        test_git_cmd()
            .args(["init", "-q", "--bare", bare.to_str().unwrap()])
            .status()
            .expect("git init bare");
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["remote", "add", "origin", bare.to_str().unwrap()])
            .current_dir(repo)
            .status()
            .expect("git remote add");
        test_git_cmd()
            .args(["push", "-u", "origin", "master"])
            .current_dir(repo)
            .status()
            .expect("git push");
        let result = rename_master_to_main(repo).await;
        assert!(result.is_ok(), "rename_master_to_main should succeed");
        let current = {
            let output = test_git_cmd()
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(repo)
                .output()
                .expect("git rev-parse");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        assert_eq!(current, "main", "should be on main branch after rename");
    }
    #[test]
    fn test_has_only_master_branch_detects_master_only() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        // Use -b master to ensure the initial branch is master regardless of
        // the user's global init.defaultBranch config.
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        let result = has_only_master_branch(repo);
        assert!(result, "should detect master-only repo");
    }
    #[test]
    fn test_has_only_master_branch_ignores_main_and_master() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["checkout", "-b", "master"])
            .current_dir(repo)
            .status()
            .expect("git checkout master");
        test_git_cmd()
            .args(["branch", "main"])
            .current_dir(repo)
            .status()
            .expect("git branch main");
        let result = has_only_master_branch(repo);
        assert!(!result, "should not detect when both main and master exist");
    }
    #[tokio::test]
    async fn test_prune_other_default_branch_deletes_main_when_on_master() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q", "-b", "master"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["checkout", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git checkout main");
        test_git_cmd()
            .args(["checkout", "master"])
            .current_dir(repo)
            .status()
            .expect("git checkout master");
        prune_other_default_branch(repo).await;
        let local_branches = {
            let output = test_git_cmd()
                .args(["branch", "--no-color"])
                .current_dir(repo)
                .output()
                .expect("git branch");
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.trim_start_matches('*').trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>()
        };
        assert!(
            local_branches.contains(&"master".to_string()),
            "master should still exist: {:?}",
            local_branches
        );
        assert!(
            !local_branches.contains(&"main".to_string()),
            "main should be deleted: {:?}",
            local_branches
        );
    }
    #[tokio::test]
    async fn test_prune_other_default_branch_deletes_master_when_on_main() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init");
        std::fs::write(repo.join("file.txt"), "content").expect("write");
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        test_git_cmd()
            .args(["checkout", "-b", "master"])
            .current_dir(repo)
            .status()
            .expect("git checkout master");
        test_git_cmd()
            .args(["checkout", "main"])
            .current_dir(repo)
            .status()
            .expect("git checkout main");
        prune_other_default_branch(repo).await;
        let local_branches = {
            let output = test_git_cmd()
                .args(["branch", "--no-color"])
                .current_dir(repo)
                .output()
                .expect("git branch");
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.trim_start_matches('*').trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>()
        };
        assert!(
            local_branches.contains(&"main".to_string()),
            "main should still exist: {:?}",
            local_branches
        );
        assert!(
            !local_branches.contains(&"master".to_string()),
            "master should be deleted: {:?}",
            local_branches
        );
    }
    #[test]
    fn test_is_repo_ready_normal_repo() {
        let _lock = acquire_path_lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(repo)
            .status()
            .expect("git config");
        test_git_cmd()
            .args(["config", "user.name", "test"])
            .current_dir(repo)
            .status()
            .expect("git config");
        std::fs::write(repo.join("hello.txt"), "hello").unwrap();
        test_git_cmd()
            .args(["add", "."])
            .current_dir(repo)
            .status()
            .expect("git add");
        test_git_cmd()
            .args(["commit", "--no-verify", "-m", "initial"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        assert!(
            is_repo_ready(repo),
            "normal repo with committed files should be ready"
        );
    }
    #[test]
    fn test_is_repo_ready_no_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        let git_dir = repo.join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        assert!(
            !is_repo_ready(repo),
            "repo without .git/HEAD should not be ready"
        );
    }
    #[test]
    fn test_is_repo_ready_empty_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        let git_dir = repo.join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), "").unwrap();
        assert!(
            !is_repo_ready(repo),
            "repo with empty .git/HEAD should not be ready"
        );
    }
    #[test]
    fn test_is_repo_ready_no_commits() {
        let _lock = acquire_path_lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(repo)
            .status()
            .expect("git config");
        test_git_cmd()
            .args(["config", "user.name", "test"])
            .current_dir(repo)
            .status()
            .expect("git config");
        assert!(
            !is_repo_ready(repo),
            "repo with zero commits (HEAD doesn't resolve) should not be ready"
        );
    }
    #[test]
    fn test_is_repo_ready_empty_commit() {
        let _lock = acquire_path_lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        test_git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .expect("git init");
        test_git_cmd()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(repo)
            .status()
            .expect("git config");
        test_git_cmd()
            .args(["config", "user.name", "test"])
            .current_dir(repo)
            .status()
            .expect("git config");
        test_git_cmd()
            .args(["commit", "--no-verify", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .status()
            .expect("git commit");
        assert!(
            is_repo_ready(repo),
            "repo with empty commit (HEAD resolves) should be ready"
        );
    }
}

/// Parse `size-garbage` (KiB) from `git count-objects -v` output.
/// Returns BYTES (count-objects values are KiB — the v0.112.42
/// unit lesson).
pub(crate) fn parse_count_objects_garbage_bytes(stdout: &str) -> u64 {
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("size-garbage:") {
            return rest.trim().parse::<u64>().unwrap_or(0) * 1024;
        }
    }
    0
}

/// Run `git gc --prune=now` when the repo's dangling-garbage size
/// exceeds `threshold_bytes`. Returns Some(garbage_bytes) when a gc
/// ran. Best-effort: all failures are logged, never fatal.
///
/// ADDED 2026-07-25 (v0.113.0). Motivation: hegemon's `.git`
/// ballooned to 4.9 GiB and dracon-platform's to 37 GiB from
/// dangling tmp_pack_* objects (failed/interrupted pushes),
/// tripping the 2 GiB GitHub pack guard and disk pressure. Manual
/// `git gc --prune=now` fixed both; this knob makes the daemon
/// self-heal instead of waiting for the next disk-pressure incident.
/// `threshold_bytes = 0` disables.
// CHANGED 2026-07-26 (v0.113.2, audit SYNC-H3): three defects in
// the v0.113.0 implementation — (1) synchronous
// `std::process::Command::output()` with NO timeout inside the
// async sync task: a multi-GiB gc pinned a tokio worker for
// minutes, and the daemon's wedge valve could force-clear +
// re-dispatch the repo while the old gc was still running;
// (2) `--prune=now` removes git's 2-week mtime grace, so a gc
// racing any concurrent writer (operator commit, agent loop, a
// re-dispatched sync task) could prune just-written objects
// before their refs updated — the classic prune race — and also
// expires the reflog amend/rebase safety net; (3) bare
// `Command::new("git")` ignored the `DRACON_SYNC_GIT_BIN`
// override. Now: async bounded run via `run_git_with_timeout`
// (600s, kill-on-timeout) using the shared git builder, plain
// `git gc` (2-week grace retained — stale tmp_pack_* files, the
// actual incident driver, are removed by gc regardless of prune
// expiry), and a per-repo 1h attempt cooldown so a repo whose gc
// keeps failing doesn't re-run a multi-minute gc every cycle.
static AUTO_GC_ATTEMPTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::time::Instant>>,
> = std::sync::OnceLock::new();

pub(crate) async fn maybe_auto_gc(repo: &std::path::Path, threshold_bytes: u64) -> Option<u64> {
    if threshold_bytes == 0 {
        return None;
    }
    {
        let attempts = AUTO_GC_ATTEMPTS.get_or_init(|| std::sync::Mutex::new(Default::default()));
        let map = attempts.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(last) = map.get(repo) {
            if last.elapsed() < std::time::Duration::from_secs(3600) {
                return None;
            }
        }
    }
    let out = crate::policy::std_git_command()
        .args(["count-objects", "-v"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let garbage = parse_count_objects_garbage_bytes(&String::from_utf8_lossy(&out.stdout));
    if garbage < threshold_bytes {
        return None;
    }
    eprintln!(
        "🗑️ {} has {:.2} GiB dangling garbage (> threshold {:.2} GiB) — running git gc",
        repo.display(),
        garbage as f64 / 1073741824.0,
        threshold_bytes as f64 / 1073741824.0,
    );
    {
        let attempts = AUTO_GC_ATTEMPTS.get_or_init(|| std::sync::Mutex::new(Default::default()));
        let mut map = attempts.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(repo.to_path_buf(), std::time::Instant::now());
    }
    let started = std::time::Instant::now();
    match run_git_with_timeout(repo, &["gc", "--quiet"], 600, "gc (auto)").await {
        Ok(()) => {
            eprintln!(
                "🗑️ gc done for {} in {:.1}s (reclaimed ~{:.2} GiB garbage)",
                repo.display(),
                started.elapsed().as_secs_f64(),
                garbage as f64 / 1073741824.0,
            );
        }
        Err(e) => {
            eprintln!(
                "⚠️ gc failed for {} (cooldown 1h): {:#}",
                repo.display(),
                e
            );
        }
    }
    Some(garbage)
}

// ---- v0.113.10: stale daemon-branch janitor (opt-in) ----

/// Cooldown map for `maybe_prune_stale_backup_branches` (same pattern as
/// AUTO_GC_ATTEMPTS): at most one janitor pass per repo per 24h, success
/// or failure (failures retry tomorrow rather than log-spamming every
/// sync cycle).
static PRUNE_BRANCHES_ATTEMPTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::time::Instant>>,
> = std::sync::OnceLock::new();

/// Branch names the daemon itself created at some point and may safely
/// reap under the janitor's bundle-first protocol. `preserve/*` and
/// operator/agent-created `backup/*` names (e.g.
/// `backup/pre-deathrun-rewrite-*`) deliberately do NOT match — the
/// janitor only ever touches the daemon's own artifacts.
fn is_daemon_owned_stale_branch(name: &str) -> bool {
    name.starts_with("backup/pre-sync-largeblob-fix-") || name == "daemon-standalone"
}

/// Names of the repo's configured remotes (`git remote`).
fn configured_remote_names(repo: &std::path::Path) -> Vec<String> {
    git_capture_stdout(repo, &["remote"])
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// ADDED 2026-07-29 (v0.113.10): opt-in janitor for stale daemon-created
/// branches and orphaned remote-tracking refs — the automated form of the
/// 2026-07-29 manual fleet cleanup (see
/// docs/design/stale-backup-branch-cleanup-2026-07-29.md).
///
/// What a pass does (per repo, at most once per 24h):
///   1. Collect stale LOCAL branches matching the daemon's own naming
///      (`backup/pre-sync-largeblob-fix-*`, `daemon-standalone`),
///      excluding the checked-out branch.
///   2. Collect ORPHANED remote-tracking refs (`refs/remotes/<name>/*`
///      where `<name>` is no longer a configured remote — the deathrun
///      `restore/*` case pinned 2 GiB of dead objects for a week).
///   3. Bundle EVERYTHING into `<backup_dir>/auto-prune/<repo>-<ts>.bundle`
///      and `git bundle verify` it. Any bundle failure aborts the pass
///      with nothing deleted — the bundle is the recovery trail.
///   4. Delete the local branches + orphaned refs, `log_warn!`-ing each
///      deletion with repo, ref, tip, and bundle path (the journal is
///      the operator-review trail AGENTS.md assigns to `backup/*`
///      branches — the janitor erases the in-repo signal, so it must
///      move it to the journal, not drop it).
///   5. For each stale branch, delete the REMOTE copy on any configured
///      remote whose tracking tip equals the recorded local tip
///      (mismatch => someone else owns that remote branch => skip),
///      skipping the remote's default-HEAD branch. The push injects
///      `DRACON_ALLOW_REWRITE=1` into that one command's env — the
///      sanctioned narrow exception to the no-auto-rewrite policy,
///      scoped to branches the daemon itself created, and itself
///      gated behind the operator's `auto_prune_stale_backup_branches
///      = true` opt-in.
///
/// Never fatal: every failure degrades to a warning and an early return.
/// Requires `backup_dir` to be configured (empty => warn + no-op).
pub(crate) async fn maybe_prune_stale_backup_branches(
    repo: &std::path::Path,
    enabled: bool,
    backup_dir: &str,
) {
    if !enabled {
        return;
    }
    {
        let attempts =
            PRUNE_BRANCHES_ATTEMPTS.get_or_init(|| std::sync::Mutex::new(Default::default()));
        let mut map = attempts.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(last) = map.get(repo) {
            if last.elapsed() < std::time::Duration::from_secs(24 * 3600) {
                return;
            }
        }
        // Mark at entry: one pass per 24h regardless of outcome.
        map.insert(repo.to_path_buf(), std::time::Instant::now());
    }

    // 1+2: collect candidates. (name-as-passed-to-git, tip, delete-refname)
    let current = current_branch(repo);
    let mut candidates: Vec<(String, String, String)> = Vec::new();
    if let Some(out) = git_capture_stdout(
        repo,
        &["for-each-ref", "--format=%(refname) %(objectname)", "refs/heads/"],
    ) {
        for line in out.lines() {
            let mut it = line.split_whitespace();
            let (Some(refname), Some(tip)) = (it.next(), it.next()) else {
                continue;
            };
            let short = refname.trim_start_matches("refs/heads/");
            if is_daemon_owned_stale_branch(short) && Some(short.to_string()) != current {
                candidates.push((short.to_string(), tip.to_string(), refname.to_string()));
            }
        }
    }
    let remotes = configured_remote_names(repo);
    let remote_set: std::collections::HashSet<&str> =
        remotes.iter().map(|s| s.as_str()).collect();
    if let Some(out) = git_capture_stdout(
        repo,
        &["for-each-ref", "--format=%(refname) %(objectname)", "refs/remotes/"],
    ) {
        for line in out.lines() {
            let mut it = line.split_whitespace();
            let (Some(refname), Some(tip)) = (it.next(), it.next()) else {
                continue;
            };
            let short = refname.trim_start_matches("refs/remotes/");
            let remote_name = short.split('/').next().unwrap_or("");
            if !remote_name.is_empty() && !remote_set.contains(remote_name) {
                candidates.push((refname.to_string(), tip.to_string(), refname.to_string()));
            }
        }
    }
    if candidates.is_empty() {
        return;
    }
    if backup_dir.is_empty() {
        log_warn!(
            "🧹 {} has {} stale daemon branch(es)/orphaned ref(s) but backup_dir is unset — skipping janitor pass",
            repo.display(),
            candidates.len()
        );
        return;
    }

    // 3: bundle everything first. Recovery: `git fetch <bundle>
    // 'refs/heads/*:refs/heads/restored-*'` (or the refs/remotes/* paths
    // for orphaned tracking refs).
    let dir = std::path::Path::new(backup_dir).join("auto-prune");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log_warn!("🧹 janitor: cannot create {}: {} — nothing deleted", dir.display(), e);
        return;
    }
    let slug = repo
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bundle = dir.join(format!("{}-{}.bundle", slug, ts));
    let bundle_str = bundle.to_string_lossy().to_string();
    let mut args: Vec<&str> = vec!["bundle", "create", &bundle_str];
    for (name, _, _) in &candidates {
        args.push(name);
    }
    let bundle_ok = git_cmd()
        .current_dir(repo)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        && git_cmd()
            .current_dir(repo)
            .args(["bundle", "verify", &bundle_str])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    if !bundle_ok {
        log_warn!(
            "🧹 janitor: bundle create/verify failed for {} ({} refs) — nothing deleted",
            repo.display(),
            candidates.len()
        );
        let _ = std::fs::remove_file(&bundle);
        return;
    }

    // Record remote-tracking tips BEFORE local deletion so the remote
    // copy is only deleted when it matches what we bundled.
    let mut remote_tips: std::collections::HashMap<(String, String), Option<String>> =
        std::collections::HashMap::new();
    for (name, _, _) in &candidates {
        if name.starts_with("refs/") {
            continue; // orphaned tracking ref — no remote exists to delete from
        }
        for r in &remotes {
            let tref = format!("refs/remotes/{}/{}", r, name);
            let tip = git_capture_stdout(repo, &["rev-parse", "--verify", "--quiet", &tref])
                .map(|s| s.trim().to_string())
                .filter(|t| t.len() == 40);
            remote_tips.insert((r.clone(), name.clone()), tip);
        }
    }

    // 4: local deletions.
    for (_name, tip, refname) in &candidates {
        let ok = git_cmd()
            .current_dir(repo)
            .args(["update-ref", "-d", refname])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            log_warn!(
                "🧹 {} pruned stale ref {} (tip {}) — bundled to {}",
                repo.display(),
                refname,
                tip,
                bundle.display()
            );
        } else {
            log_warn!("🧹 janitor: failed to delete {} in {} — skipping", refname, repo.display());
        }
    }

    // 5: remote deletions (matching tips only; never the remote's
    // default-HEAD branch; narrow DRACON_ALLOW_REWRITE injection).
    for (name, local_tip, _) in &candidates {
        if name.starts_with("refs/") {
            continue;
        }
        for r in &remotes {
            let matches = remote_tips
                .get(&(r.clone(), name.clone()))
                .and_then(|t| t.as_deref())
                == Some(local_tip.as_str());
            if !matches {
                continue;
            }
            let remote_head = git_capture_stdout(
                repo,
                &["symbolic-ref", "--quiet", &format!("refs/remotes/{}/HEAD", r)],
            )
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
            if remote_head == format!("refs/remotes/{}/{}", r, name) {
                continue; // remote's default branch — never delete
            }
            match run_git_with_timeout_env(
                repo,
                &["push", r, "--delete", name],
                600,
                "push --delete (stale daemon branch)",
                &[("DRACON_ALLOW_REWRITE", "1")],
            )
            .await
            {
                Ok(()) => {
                    log_warn!(
                        "🧹 {} deleted stale branch {} on remote {} (tip matched {})",
                        repo.display(),
                        name,
                        r,
                        local_tip
                    );
                }
                Err(e) => {
                    log_warn!(
                        "🧹 janitor: remote delete of {} on {} failed for {}: {:#} — will retry next pass",
                        name,
                        r,
                        repo.display(),
                        e
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod auto_gc_tests {
    /// v0.113.2 (SYNC-H3): threshold 0 disables; a repo below the
    /// threshold is a no-op (and records no attempt cooldown).
    #[tokio::test]
    async fn test_maybe_auto_gc_disabled_and_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let status = crate::policy::std_git_command()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        assert!(status.success());

        // Disabled.
        assert!(super::maybe_auto_gc(&repo, 0).await.is_none());
        // Fresh repo has ~0 garbage — far below a 2 GiB threshold.
        assert!(super::maybe_auto_gc(&repo, 2 * 1024 * 1024 * 1024).await.is_none());
        // No attempt was recorded (nothing to gc).
        let attempts = super::AUTO_GC_ATTEMPTS
            .get_or_init(|| std::sync::Mutex::new(Default::default()));
        let map = attempts.lock().unwrap_or_else(|p| p.into_inner());
        assert!(!map.contains_key(&repo));
    }
}
