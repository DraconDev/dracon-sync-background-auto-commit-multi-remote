use super::*;
use std::process::{Command, Stdio};
fn native(repo: &Path, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
    use std::io::Write;
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = input {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn policy() -> SyncPolicy {
    toml::from_str("auto_github_private=false\nauto_commit=true\nauto_pull=false\nauto_push=false\nauto_bump_versions=false\nauto_resolve_unmerged=true\ntrusted_emails=['test@test']\ntrusted_authors=['test']\nremotes=[]\n").unwrap()
}
fn fixture(path: &Path) -> (tempfile::TempDir, Vec<(String, Vec<u8>)>) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    native(r, &["init", "--template=", "-q", "-b", "main"], None);
    native(r, &["config", "user.name", "test"], None);
    native(r, &["config", "user.email", "test@test"], None);
    native(r, &["config", "commit.gpgsign", "false"], None);
    let hooks = r.join(".git/fixture-hooks");
    std::fs::create_dir(&hooks).unwrap();
    native(
        r,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
        None,
    );
    std::fs::write(r.join(path), b"HEAD\0binary\xff\n").unwrap();
    std::fs::write(r.join("note.txt"), b"baseline\n").unwrap();
    let mut names = b":(literal)".to_vec();
    names.extend_from_slice(path.as_os_str().as_bytes());
    names.push(0);
    names.extend_from_slice(b"note.txt\0");
    native(
        r,
        &["add", "--pathspec-from-file=-", "--pathspec-file-nul"],
        Some(&names),
    );
    native(r, &["commit", "-q", "-m", "initial"], None);
    let mut update = format!("0 {}\t", "0".repeat(40)).into_bytes();
    update.extend_from_slice(path.as_os_str().as_bytes());
    update.push(0);
    let mut saved = Vec::new();
    for (stage, bytes) in [
        (1, b"base staged bytes\n".as_slice()),
        (2, b"ours staged bytes\n".as_slice()),
        (3, b"theirs staged bytes\n".as_slice()),
    ] {
        let oid = String::from_utf8(native(r, &["hash-object", "-w", "--stdin"], Some(bytes)))
            .unwrap()
            .trim()
            .to_owned();
        update.extend_from_slice(format!("100644 {oid} {stage}\t").as_bytes());
        update.extend_from_slice(path.as_os_str().as_bytes());
        update.push(0);
        saved.push((oid, bytes.to_vec()));
    }
    native(r, &["update-index", "-z", "--index-info"], Some(&update));
    assert!(!native(r, &["ls-files", "--unmerged", "-z"], None).is_empty());
    (d, saved)
}
fn head(repo: &Path) -> Vec<u8> {
    native(repo, &["rev-parse", "HEAD"], None)
}

#[tokio::test]
async fn unmerged_entry_resolves_exact_names_and_retains_all_stage_payloads_independently() {
    use std::os::unix::ffi::OsStringExt;
    for raw in [
        b"odd [*] \t\nname.bin".to_vec(),
        b"nonutf8-\xff.bin".to_vec(),
    ] {
        let path = PathBuf::from(std::ffi::OsString::from_vec(raw));
        let (d, saved) = fixture(&path);
        let r = d.path();
        let original = std::fs::read(r.join(".git/index")).unwrap();
        let before_head = head(r);
        let (epoch, count) = reconcile(r, true, Instant::now() + Duration::from_secs(15))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count, 1);
        assert!(!epoch.has_intent());
        assert!(native(r, &["ls-files", "--unmerged", "-z"], None).is_empty());
        assert_eq!(head(r), before_head);
        assert_eq!(std::fs::read(r.join(&path)).unwrap(), b"HEAD\0binary\xff\n");
        let retained = epoch.retained.as_ref().unwrap();
        assert_eq!(
            std::fs::read(retained.join("original-index")).unwrap(),
            original
        );
        assert_eq!(std::fs::metadata(retained).unwrap().mode() & 0o077, 0);
        // Import ONLY the retained private pack into an unrelated empty object
        // database. Canonical objects/index/history cannot fake cold recovery.
        let cold = tempfile::tempdir().unwrap();
        native(cold.path(), &["init", "--bare", "--template=", "-q"], None);
        let pack = std::fs::read_dir(retained)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "pack"))
            .unwrap();
        native(
            cold.path(),
            &["index-pack", "--stdin"],
            Some(&std::fs::read(pack).unwrap()),
        );
        let baseline = native(r, &["rev-parse", "HEAD:note.txt"], None);
        let absent = native(cold.path(), &["cat-file", "--batch-check"], Some(&baseline));
        assert!(
            absent.ends_with(b" missing\n"),
            "unchanged HEAD baseline must not be duplicated into the delta spool"
        );
        for (oid, bytes) in saved {
            assert_eq!(
                native(cold.path(), &["cat-file", "blob", &oid], None),
                bytes
            );
        }
    }
}

#[tokio::test]
async fn unmerged_entry_retention_budget_uses_actual_blob_sizes_and_preserves_index_on_refusal() {
    let (d, saved) = fixture(Path::new("a.bin"));
    let r = d.path();
    let original = std::fs::read(r.join(".git/index")).unwrap();
    let objects = saved
        .iter()
        .map(|(oid, _)| oid.clone())
        .collect::<BTreeSet<_>>();
    let size = saved
        .iter()
        .map(|(_, bytes)| bytes.len() as u64)
        .sum::<u64>();
    assert!(retention_input(r, &objects, size - 1).await.is_err());
    let (input, observed) = retention_input(r, &objects, size).await.unwrap();
    assert_eq!(observed, size);
    assert_eq!(
        input.lines().map(str::to_owned).collect::<BTreeSet<_>>(),
        objects
    );
    assert!(require_free_space(r, u64::MAX).is_err());
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), original);
}

#[tokio::test]
async fn unmerged_entry_preserves_prior_partial_staging_and_rejects_late_cached_replacement() {
    let (d, _) = fixture(Path::new("a.bin"));
    let r = d.path();
    std::fs::write(r.join("note.txt"), b"captured staged B\n").unwrap();
    native(r, &["add", "--", "note.txt"], None);
    let expected = native(r, &["rev-parse", ":note.txt"], None);
    std::fs::write(r.join("note.txt"), b"later unstaged C\n").unwrap();
    let original = std::fs::read(r.join(".git/index")).unwrap();
    let epoch = Epoch::capture_entry(r, 15, true).await.unwrap();
    assert!(epoch.has_intent());
    assert_eq!(native(r, &["rev-parse", ":note.txt"], None), expected);
    assert_eq!(
        std::fs::read(epoch.retained.as_ref().unwrap().join("original-index")).unwrap(),
        original
    );
    native(r, &["add", "--", "note.txt"], None);
    let foreign = std::fs::read(r.join(".git/index")).unwrap();
    let before = head(r);
    assert!(epoch
        .commit(r, "must not absorb C", &policy(), &BTreeSet::new(), None)
        .await
        .is_err());
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), foreign);
    assert_eq!(head(r), before);
    assert_eq!(
        std::fs::read(r.join("note.txt")).unwrap(),
        b"later unstaged C\n"
    );
}

#[tokio::test]
async fn unmerged_entry_foreign_lock_disabled_policy_and_active_merge_preserve_exact_index() {
    let (d, _) = fixture(Path::new("a.bin"));
    let r = d.path();
    let original = std::fs::read(r.join(".git/index")).unwrap();
    assert!(reconcile(r, false, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap()
        .is_none());
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), original);
    std::fs::write(r.join(".git/index.lock"), b"foreign lock bytes").unwrap();
    assert!(reconcile(r, true, Instant::now() + Duration::from_secs(5))
        .await
        .is_err());
    assert_eq!(
        std::fs::read(r.join(".git/index.lock")).unwrap(),
        b"foreign lock bytes"
    );
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), original);
    std::fs::remove_file(r.join(".git/index.lock")).unwrap();
    let marker = head(r);
    std::fs::write(r.join(".git/MERGE_HEAD"), &marker).unwrap();
    assert!(reconcile(r, true, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap()
        .is_none());
    assert_eq!(std::fs::read(r.join(".git/MERGE_HEAD")).unwrap(), marker);
    assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), original);
}

#[tokio::test]
async fn unmerged_entry_unknown_worktree_and_nonzero_other_index_flags_refuse_intact() {
    let (d, _) = fixture(Path::new("a.bin"));
    let r = d.path();
    std::fs::write(r.join("a.bin"), b"unknown current WIP\n").unwrap();
    let original = std::fs::read(r.join(".git/index")).unwrap();
    for _ in 0..3 {
        assert!(reconcile(r, true, Instant::now() + Duration::from_secs(10))
            .await
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), original);
        assert!(
            !std::fs::read_dir(r.join(".git")).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("dracon-unmerged-recovery-")),
            "no-progress retries must not duplicate retained payloads"
        );
    }
    assert_eq!(
        std::fs::read(r.join("a.bin")).unwrap(),
        b"unknown current WIP\n"
    );
    std::fs::write(r.join("a.bin"), b"HEAD\0binary\xff\n").unwrap();
    native(
        r,
        &["update-index", "--skip-worktree", "--", "note.txt"],
        None,
    );
    let original = std::fs::read(r.join(".git/index")).unwrap();
    for _ in 0..3 {
        assert!(reconcile(r, true, Instant::now() + Duration::from_secs(10))
            .await
            .is_err());
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), original);
        assert!(
            !std::fs::read_dir(r.join(".git")).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("dracon-unmerged-recovery-")),
            "unsupported flags must refuse before allocating recovery"
        );
    }
    assert!(native(r, &["ls-files", "-v", "--", "note.txt"], None).starts_with(b"S "));
}

#[tokio::test]
async fn unmerged_entry_production_sync_commits_only_verified_entry_staging_and_preserves_worktree()
{
    let (d, saved) = fixture(Path::new("a.bin"));
    let r = d.path();
    let before = String::from_utf8(head(r)).unwrap().trim().to_owned();
    std::fs::write(r.join("note.txt"), b"captured staged B\n").unwrap();
    native(r, &["add", "--", "note.txt"], None);
    std::fs::write(r.join("note.txt"), b"later unstaged C\n").unwrap();
    std::fs::write(r.join("unrelated.txt"), b"untracked producer\n").unwrap();
    let hook = r.join(".git/fixture-hooks/pre-commit");
    std::fs::write(&hook,b"#!/bin/sh\ntest \"$(git show :note.txt)\" = 'captured staged B' || exit 41\nif git ls-files --error-unmatch unrelated.txt >/dev/null 2>&1; then exit 42; fi\nprintf 'pre-commit\\n' >> .git/meaningful-hook-called\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let original_index = std::fs::read(r.join(".git/index")).unwrap();
    let result = sync_repo(r, &policy(), &BTreeSet::new(), 0, None, false, None).await;
    assert!(result.is_ok(), "real production sync failed: {result:?}");
    let commits = String::from_utf8(native(
        r,
        &["rev-list", "--reverse", &format!("{before}..HEAD")],
        None,
    ))
    .unwrap();
    let first = commits
        .lines()
        .next()
        .expect("real captured commit, not a private-helper-only positive");
    assert_eq!(
        native(r, &["show", &format!("{first}:note.txt")], None),
        b"captured staged B\n"
    );
    assert!(native(r, &["ls-tree", "-r", "--name-only", first], None)
        .split(|b| *b == b'\n')
        .all(|p| p != b"unrelated.txt"));
    assert_eq!(
        std::fs::read(r.join("note.txt")).unwrap(),
        b"later unstaged C\n"
    );
    assert_eq!(
        std::fs::read(r.join("unrelated.txt")).unwrap(),
        b"untracked producer\n"
    );
    assert_eq!(
        std::fs::read(r.join(".git/meaningful-hook-called")).unwrap(),
        b"pre-commit\n",
        "meaningful original hook executes exactly once"
    );
    let recovery = std::fs::read_dir(r.join(".git")).unwrap().map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("dracon-unmerged-recovery-")).unwrap();
    assert_eq!(std::fs::read(recovery.join("original-index")).unwrap(), original_index,
        "publication must preserve the original conflict index");
    let pack = std::fs::read_dir(&recovery).unwrap().map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|ext| ext == "pack")).unwrap();
    let cold = tempfile::tempdir().unwrap();
    native(cold.path(), &["init", "--bare", "--template=", "-q"], None);
    native(cold.path(), &["index-pack", "--stdin"], Some(&std::fs::read(pack).unwrap()));
    for (oid, bytes) in saved {
        assert_eq!(native(cold.path(), &["cat-file", "blob", &oid], None), bytes,
            "original conflict payload must remain cold-recoverable after publication");
    }
}

#[tokio::test]
async fn unmerged_entry_never_relocates_an_active_worktree_writer() {
    use std::io::Write;
    let (d, _) = fixture(Path::new("a.bin"));
    let r = d.path();
    let mut writer = OpenOptions::new()
        .write(true)
        .open(r.join("a.bin"))
        .unwrap();
    let inode = writer.metadata().unwrap().ino();
    let (epoch, _) = reconcile(r, true, Instant::now() + Duration::from_secs(10))
        .await
        .unwrap()
        .unwrap();
    assert!(!epoch.has_intent());
    writer.write_all(b"AFTER").unwrap();
    writer.sync_all().unwrap();
    assert_eq!(std::fs::metadata(r.join("a.bin")).unwrap().ino(), inode);
    assert!(std::fs::read(r.join("a.bin"))
        .unwrap()
        .starts_with(b"AFTER"));
    assert!(native(r, &["ls-files", "--unmerged", "-z"], None).is_empty());
}

#[tokio::test]
async fn unmerged_entry_production_sync_resolves_stale_slots_without_manufacturing_a_commit() {
    let (d, _) = fixture(Path::new("a.bin"));
    let r = d.path();
    let before = head(r);
    let result = sync_repo(r, &policy(), &BTreeSet::new(), 0, None, false, None).await;
    assert_eq!(result.unwrap(), SyncOutcome::NothingToDo);
    assert_eq!(head(r), before, "no manual, empty or safety commit");
    assert!(native(r, &["ls-files", "--unmerged", "-z"], None).is_empty());
    assert_eq!(
        std::fs::read(r.join("a.bin")).unwrap(),
        b"HEAD\0binary\xff\n"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn unmerged_entry_concurrent_index_ref_and_deadline_refuse_without_losing_data() {
    const ROLE: &str = "DRACON_UNMERGED_RACE_ROLE";
    if let Ok(mode) = std::env::var(ROLE) {
        let repo = PathBuf::from(std::env::var_os("DRACON_UNMERGED_RACE_REPO").unwrap());
        let started = Instant::now();
        let result = reconcile(&repo, true, Instant::now() + Duration::from_secs(2)).await;
        assert!(
            result.is_err(),
            "injected {mode} must refuse, not install or accept"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "one absolute deadline owns the held-FD hash process"
        );
        if mode == "deadline" {
            let pid = std::fs::read_to_string(repo.join(".git/hash-descendant-pid")).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let gone = match std::fs::read_to_string(format!("/proc/{}/stat", pid.trim())) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                    Err(error) => panic!("cannot inspect owned descendant: {error}"),
                    Ok(stat) => stat.rsplit_once(") ").unwrap().1.starts_with("Z "),
                };
                if gone {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed-out held-file hash descendant survived"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        return;
    }
    // Environment overrides exist ONLY at exec of an isolated owned test child.
    // The custom Git wrapper injects at the actual held-source hash boundary.
    for mode in ["index", "reference", "deadline"] {
        let (d, _) = fixture(Path::new("a.bin"));
        let r = d.path();
        let original = std::fs::read(r.join(".git/index")).unwrap();
        let before = head(r);
        std::fs::write(r.join("note.txt"), b"foreign cached C\n").unwrap();
        let foreign = r.join(".git/foreign-index");
        std::fs::write(&foreign, &original).unwrap();
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let out = command
            .arg("-C")
            .arg(r)
            .args(["add", "--", "note.txt"])
            .env("GIT_INDEX_FILE", &foreign)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success());
        let foreign_bytes = std::fs::read(&foreign).unwrap();
        let tree = String::from_utf8(native(r, &["rev-parse", "HEAD^{tree}"], None)).unwrap();
        let newer = native(
            r,
            &[
                "commit-tree",
                tree.trim(),
                "-p",
                std::str::from_utf8(&before).unwrap().trim(),
                "-m",
                "independent concurrent advance",
            ],
            None,
        );
        let script = r.join(".git/race-git");
        std::fs::write(&script,b"#!/bin/sh\ncase \" $* \" in *' hash-object '*)\n case \"$DRACON_UNMERGED_RACE_ROLE\" in\n index) cp -- \"$DRACON_UNMERGED_RACE_REPO/.git/foreign-index\" \"$DRACON_UNMERGED_RACE_REPO/.git/index\" ;;\n reference) \"$DRACON_UNMERGED_REAL_GIT\" update-ref refs/heads/main \"$DRACON_UNMERGED_NEW_REF\" || exit 73 ;;\n deadline) sleep 10 & echo $! > \"$DRACON_UNMERGED_RACE_REPO/.git/hash-descendant-pid\"; wait; exit 74 ;;\n esac ;;\n esac\nexec \"$DRACON_UNMERGED_REAL_GIT\" \"$@\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let output=tokio::time::timeout(Duration::from_secs(25),tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","sync::staged_epoch::unmerged::tests::unmerged_entry_concurrent_index_ref_and_deadline_refuse_without_losing_data","--nocapture"])
            .env(ROLE,mode).env("DRACON_UNMERGED_RACE_REPO",r).env("DRACON_SYNC_GIT_BIN",&script)
            .env("DRACON_UNMERGED_REAL_GIT",crate::policy::git_binary()).env("DRACON_UNMERGED_NEW_REF",std::str::from_utf8(&newer).unwrap().trim())
            .env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1").kill_on_drop(true).output()).await.expect("bounded isolated witness").unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(r.join(".git/index")).unwrap(),
            if mode == "index" {
                foreign_bytes
            } else {
                original
            }
        );
        assert_eq!(head(r), if mode == "reference" { newer } else { before });
        assert_eq!(
            std::fs::read(r.join("a.bin")).unwrap(),
            b"HEAD\0binary\xff\n"
        );
        assert_eq!(
            std::fs::read(r.join("note.txt")).unwrap(),
            b"foreign cached C\n"
        );
        assert!(
            !r.join(".git/index.lock").exists(),
            "only the owned lock is released"
        );
    }
}
