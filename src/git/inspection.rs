//! Daemon status inspection with an owned, cancellation-safe process tree.
//!
//! Do not use GitService::get_status here: it runs blocking libgit2/CLI work
//! that survives cancellation of its async caller. A wedged filter would then
//! outlive its reservation and repeated retries accumulate orphan work.

use anyhow::{bail, Context, Result};
use dracon_git::types::RepoStatus;
use std::{collections::BTreeSet, path::{Path, PathBuf}, time::Duration};

/// Shared by scheduler and reports: a healthy filtered/nested worktree may
/// exceed a short UI deadline. Keep cancellation and the finite ceiling alike.
pub(crate) const STATUS_INSPECTION_BUDGET: Duration = Duration::from_secs(60);

pub(crate) async fn inspect_status(repo: &Path, budget: Duration) -> Result<RepoStatus> {
    inspect_status_inner(repo, budget, None).await
}

/// Avoid recursively re-inspecting child worktrees only when every indexed
/// gitlink is independently present in this observation's watched inventory.
/// Uncovered children retain ordinary Git status semantics. Child HEAD changes
/// and staged gitlink changes remain visible in either mode.
pub(crate) async fn inspect_status_with_coverage(
    repo: &Path,
    budget: Duration,
    watched: &BTreeSet<PathBuf>,
) -> Result<RepoStatus> {
    if watched.is_empty() {
        return inspect_status(repo, budget).await;
    }
    inspect_status_inner(repo, budget, Some(watched)).await
}

async fn indexed_gitlinks(repo: &Path) -> Result<Option<BTreeSet<PathBuf>>> {
    let child = super::spawn_git_command_cancellable(
        repo, &["--no-optional-locks", "ls-files", "--stage", "-z"],
        "status gitlink coverage",
    )?;
    let (exit, stdout, stderr) =
        super::run_git_captured_output(child, repo, "status gitlink coverage").await?;
    if !exit.success() {
        bail!("status gitlink coverage failed: {}", String::from_utf8_lossy(&stderr));
    }
    parse_gitlinks(repo, &stdout)
}

fn parse_gitlinks(repo: &Path, bytes: &[u8]) -> Result<Option<BTreeSet<PathBuf>>> {
    let mut paths = BTreeSet::new();
    for record in bytes.split(|b| *b == 0).filter(|r| r.starts_with(b"160000 ")) {
        let tab = record.iter().position(|b| *b == b'\t')
            .context("malformed indexed gitlink")?;
        let (header, tail) = record.split_at(tab);
        let name = &tail[1..];
        let fields: Vec<_> = header.split(|b| *b == b' ').collect();
        if fields.len() != 3 || !matches!(fields[1].len(), 40 | 64)
            || !fields[1].iter().all(u8::is_ascii_hexdigit) {
            bail!("malformed indexed gitlink identity");
        }
        if fields[2] != b"0" {
            return Ok(None); // Conflicted gitlinks require full inspection.
        }
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            Path::new(std::ffi::OsStr::from_bytes(name))
        };
        #[cfg(not(unix))]
        let path = Path::new(std::str::from_utf8(name)?);
        if name.is_empty() || path.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            bail!("invalid indexed gitlink path");
        }
        paths.insert(repo.join(path));
    }
    Ok(Some(paths))
}

async fn inspect_status_inner(
    repo: &Path,
    budget: Duration,
    watched: Option<&BTreeSet<PathBuf>>,
) -> Result<RepoStatus> {
    tokio::time::timeout(budget, async {
        // A copied submodule .git file may still name another checkout via
        // core.worktree. Never inspect or schedule that neighbor as this repo.
        // Use Git's own resolution (including linked-worktree configuration),
        // not a second, incomplete configuration parser.
        let child = super::spawn_git_command_cancellable(
            repo, &["rev-parse", "--show-toplevel"], "status worktree identity",
        )?;
        let (exit, root, stderr) = super::run_git_captured_output(
            child, repo, "status worktree identity",
        ).await?;
        if !exit.success() {
            bail!("status worktree identity failed in {}: {}", repo.display(), String::from_utf8_lossy(&stderr));
        }
        let root = root.strip_suffix(b"\n").unwrap_or(&root);
        #[cfg(unix)]
        let root = {
            use std::os::unix::ffi::OsStrExt;
            Path::new(std::ffi::OsStr::from_bytes(root))
        };
        #[cfg(not(unix))]
        let root = Path::new(std::str::from_utf8(root)?);
        if std::fs::canonicalize(root)? != std::fs::canonicalize(repo)? {
            bail!("Git metadata resolves to a different worktree: {} (requested {}); inspect checkout wiring before recovery", root.display(), repo.display());
        }
        let covered_gitlinks = if let Some(watched) = watched {
            indexed_gitlinks(repo).await?.filter(|paths|
                !paths.is_empty() && paths.is_subset(watched))
        } else {
            None
        };
        let mut args = vec![
                "--no-optional-locks",
                // A service task limit includes push-hook workers too. Do not
                // fan out index/preload threads for each of four inspections.
                "-c",
                "index.threads=1",
                "-c",
                "core.preloadIndex=false",
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
        ];
        if covered_gitlinks.is_some() {
            args.push("--ignore-submodules=dirty");
        }
        let child = super::spawn_git_command_cancellable(repo, &args, "status inspection")?;
        let (exit, stdout, stderr) =
            super::run_git_captured_output(child, repo, "status inspection").await?;
        if !exit.success() {
            bail!(
                "status inspection failed in {}: {}",
                repo.display(),
                String::from_utf8_lossy(&stderr)
            );
        }
        if let Some(expected) = covered_gitlinks {
            if indexed_gitlinks(repo).await? != Some(expected) {
                bail!("status gitlink coverage changed during inspection; retry with a fresh inventory");
            }
        }
        parse_status(&stdout)
    })
    .await
    .context("status inspection timed out; owned Git process group cancelled")?
}

fn parse_status(bytes: &[u8]) -> Result<RepoStatus> {
    let mut status = RepoStatus::new();
    let mut records = bytes.split(|b| *b == 0).peekable();
    for_record(&mut records, &mut status)?;
    status.is_clean =
        status.modified_files == 0 && status.staged_files == 0 && status.untracked_files == 0;
    Ok(status)
}

fn for_record<'a>(
    records: &mut impl Iterator<Item = &'a [u8]>,
    status: &mut RepoStatus,
) -> Result<()> {
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(oid) = record.strip_prefix(b"# branch.oid ") {
            // Reports use presence of HEAD to distinguish a real repository
            // from an unborn branch. Omitting this made every report EMPTY
            // after switching from GitService to the owned inspection path.
            if oid != b"(initial)" {
                if !matches!(oid.len(), 40 | 64) || !oid.iter().all(u8::is_ascii_hexdigit) {
                    bail!("malformed branch.oid");
                }
                status.last_commit_hash = Some(std::str::from_utf8(oid)?.into());
            }
        } else if let Some(head) = record.strip_prefix(b"# branch.head ") {
            status.branch = if head == b"(detached)" {
                "detached".into()
            } else {
                std::str::from_utf8(head)?.into()
            };
        } else if let Some(ab) = record.strip_prefix(b"# branch.ab ") {
            let ab = std::str::from_utf8(ab)?;
            let (ahead, behind) = ab.split_once(' ').context("malformed branch.ab")?;
            status.ahead = ahead
                .strip_prefix('+')
                .context("missing ahead prefix")?
                .parse()?;
            status.behind = behind
                .strip_prefix('-')
                .context("missing behind prefix")?
                .parse()?;
        } else if record.starts_with(b"# ") {
            // upstream and future optional branch headers.
        } else if record.starts_with(b"? ") {
            status.untracked_files += 1;
        } else if matches!(record[0], b'1' | b'2' | b'u') {
            let fields = match record[0] {
                b'1' => 9,
                b'2' => 10,
                _ => 11,
            };
            if record.len() < 5
                || record[1] != b' '
                || record[4] != b' '
                || record.splitn(fields, |b| *b == b' ').count() != fields
            {
                bail!("malformed porcelain status record");
            }
            if record[2] != b'.' {
                status.staged_files += 1;
            }
            if record[3] != b'.' {
                status.modified_files += 1;
            }
            // -z rename records have a second NUL-delimited ORIGINAL path.
            // Never interpret that path as another status record.
            if record[0] == b'2' && records.next().is_none_or(|path| path.is_empty()) {
                bail!("missing rename source path");
            }
        } else {
            bail!("unrecognized porcelain status record");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlink_coverage_parsing_is_exact_and_conflicts_require_full_status() {
        let root = Path::new("/fixture");
        for width in [40, 64] {
            let bytes = format!("100644 {} 0\tfile\0160000 {} 0\tchild\0", "a".repeat(width), "b".repeat(width));
            assert_eq!(parse_gitlinks(root, bytes.as_bytes()).unwrap(),
                Some(BTreeSet::from([root.join("child")])));
            let conflict = format!("160000 {} 2\tchild\0", "b".repeat(width));
            assert_eq!(parse_gitlinks(root, conflict.as_bytes()).unwrap(), None);
        }
        for name in ["", "../outside", "/absolute"] {
            let bytes = format!("160000 {} 0\t{name}\0", "b".repeat(40));
            assert!(parse_gitlinks(root, bytes.as_bytes()).is_err());
        }
        assert!(parse_gitlinks(root, b"160000 broken\0").is_err());
    }

    #[tokio::test]
    async fn covered_children_skip_recursive_dirt_but_keep_gitlink_and_child_work() {
        fn commit_index(repo: &git2::Repository) -> git2::Oid {
            let tree = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree).unwrap();
            let parents: Vec<_> = repo.head().ok().map(|h| h.peel_to_commit().unwrap()).into_iter().collect();
            let refs: Vec<_> = parents.iter().collect();
            let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
            repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &refs).unwrap()
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let parent = git2::Repository::init(root).unwrap();
        for name in ["child", "other"] {
            let path = root.join(name);
            let child = git2::Repository::init(&path).unwrap();
            std::fs::write(path.join("tracked"), "baseline").unwrap();
            let mut index = child.index().unwrap();
            index.add_path(Path::new("tracked")).unwrap();
            index.write().unwrap();
            commit_index(&child);
        }
        std::fs::write(root.join(".gitmodules"),
            "[submodule \"child\"]\npath = child\nurl = fixture\n[submodule \"other\"]\npath = other\nurl = fixture\n").unwrap();
        let added = std::process::Command::new("git").arg("-C").arg(root)
            .args(["add", "--", ".gitmodules", "child", "other"]).output().unwrap();
        assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
        commit_index(&parent);
        for name in ["child", "other"] {
            std::fs::write(root.join(name).join("tracked"), "unstaged child work").unwrap();
        }
        std::fs::write(root.join("child/untracked"), "new child work").unwrap();
        let partial = BTreeSet::from([root.to_path_buf(), root.join("child")]);
        let full = BTreeSet::from([root.to_path_buf(), root.join("child"), root.join("other")]);
        let uncovered = inspect_status_with_coverage(root, STATUS_INSPECTION_BUDGET, &partial).await.unwrap();
        assert_eq!(uncovered.modified_files, 2, "one uncovered child must retain full recursive status");
        let covered = inspect_status_with_coverage(root, STATUS_INSPECTION_BUDGET, &full).await.unwrap();
        assert!(covered.is_clean, "child work cannot be staged by its parent");
        let child_status = inspect_status_with_coverage(&root.join("child"), STATUS_INSPECTION_BUDGET, &full).await.unwrap();
        assert_eq!((child_status.modified_files, child_status.untracked_files), (1, 1));
        assert!(!child_status.is_clean, "child work must remain visible in its own row");
        let child = git2::Repository::open(root.join("child")).unwrap();
        let mut index = child.index().unwrap();
        index.add_path(Path::new("tracked")).unwrap();
        index.write().unwrap();
        commit_index(&child);
        let advanced = inspect_status_with_coverage(root, STATUS_INSPECTION_BUDGET, &full).await.unwrap();
        assert_eq!(advanced.modified_files, 1, "advanced child HEAD must stay visible");
        let added = std::process::Command::new("git").arg("-C").arg(root)
            .args(["add", "--", "child"]).output().unwrap();
        assert!(added.status.success());
        let staged = inspect_status_with_coverage(root, STATUS_INSPECTION_BUDGET, &full).await.unwrap();
        assert_eq!(staged.staged_files, 1, "staged gitlink change must stay visible");
        assert_eq!(std::fs::read(root.join("other/tracked")).unwrap(), b"unstaged child work");
        assert_eq!(std::fs::read(root.join("child/untracked")).unwrap(), b"new child work");
    }

    #[test]
    fn head_presence_survives_owned_inspection_for_report_empty_detection() {
        for len in [40, 64] {
            let oid = "a".repeat(len);
            let bytes = format!("# branch.oid {oid}\0# branch.head main\0");
            let status = parse_status(bytes.as_bytes()).unwrap();
            assert_eq!(status.last_commit_hash.as_deref(), Some(oid.as_str()));
        }
        assert!(
            parse_status(b"# branch.oid (initial)\0# branch.head main\0")
                .unwrap()
                .last_commit_hash
                .is_none()
        );
        assert!(parse_status(b"# branch.oid garbage\0").is_err());
    }

    #[test]
    fn parses_branch_counts_renames_conflicts_and_non_utf8_paths() {
        let output = b"# branch.head main\0# branch.ab +12 -3\0\
            1 .M N... 100644 100644 100644 a b modified\0\
            2 R. N... 100644 100644 100644 a b R100 new name\0? old name\0\
            u UU N... 100644 100644 100644 100644 a b c conflict\0\
            ? dir/\xff\nfile\0? dir/other\0";
        let s = parse_status(output).unwrap();
        assert_eq!(s.branch, "main");
        assert_eq!((s.ahead, s.behind), (12, 3));
        assert_eq!(
            (s.modified_files, s.staged_files, s.untracked_files),
            (2, 2, 2)
        );
        assert!(!s.is_clean);
    }

    #[test]
    fn handles_clean_detached_and_unborn_heads_and_rejects_bad_output() {
        let detached = parse_status(b"# branch.head (detached)\0").unwrap();
        assert_eq!(detached.branch, "detached");
        assert!(detached.is_clean);
        assert_eq!(
            parse_status(b"# branch.oid (initial)\0# branch.head main\0")
                .unwrap()
                .branch,
            "main"
        );
        assert!(parse_status(b"# branch.ab bad\0").is_err());
        assert!(parse_status(b"garbage\0").is_err());
        assert!(parse_status(b"2 R. path\0").is_err());
    }

    #[cfg(target_os = "linux")]
    async fn check_hung_filter_recovery(abort_owner: bool) {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--initial-branch=main"]);
        std::fs::write(dir.path().join(".gitattributes"), "asset filter=stall\n").unwrap();
        std::fs::write(
            dir.path().join("filter.sh"),
            "if test -e stall; then sleep 60 & echo $! > child.pid.tmp; mv child.pid.tmp child.pid; wait; fi\ncat\n",
        )
        .unwrap();
        git(&["config", "filter.stall.clean", "sh filter.sh"]);
        git(&["config", "filter.stall.required", "true"]);
        std::fs::write(dir.path().join("asset"), "before\n").unwrap();
        git(&["add", "asset"]);
        tokio::time::sleep(Duration::from_millis(1100)).await;
        // Equal size forces content/filter comparison rather than Git's
        // cheap size-mismatch dirty shortcut.
        std::fs::write(dir.path().join("asset"), "after!\n").unwrap();
        std::fs::write(dir.path().join("stall"), "").unwrap();
        if abort_owner {
            let repo = dir.path().to_path_buf();
            let inspection =
                tokio::spawn(async move { inspect_status(&repo, Duration::from_secs(60)).await });
            // Reproduce the scheduler watchdog dropping the OWNER, rather
            // than relying on inspect_status's own timeout to fire.
            let ready = tokio::time::timeout(Duration::from_secs(5), async {
                while !dir.path().join("child.pid").exists() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            inspection.abort();
            assert!(inspection.await.unwrap_err().is_cancelled());
            assert!(
                ready.is_ok(),
                "filter did not start before owner cancellation"
            );
        } else {
            let error = inspect_status(dir.path(), Duration::from_secs(2))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("timed out"));
        }
        // Readiness must publish a complete identity before owner abortion;
        // otherwise an empty marker accidentally addresses /proc/stat.
        let pid: u32 = std::fs::read_to_string(dir.path().join("child.pid"))
            .unwrap()
            .trim()
            .parse()
            .expect("complete published helper PID");
        assert!(pid > 0);
        let proc_stat = format!("/proc/{pid}/stat");
        let killed = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let stat = std::fs::read_to_string(&proc_stat);
                if stat.as_ref().map_or(true, |s| {
                    s.split_once(") ")
                        .is_some_and(|(_, tail)| tail.starts_with('Z'))
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert!(
            killed.is_ok(),
            "filter helper survived inspection cancellation"
        );
        std::fs::remove_file(dir.path().join("stall")).unwrap();
        let recovered = inspect_status(dir.path(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(recovered.modified_files, 1);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn hung_filter_is_killed_and_next_inspection_recovers() {
        check_hung_filter_recovery(false).await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn owner_abort_kills_filter_and_next_inspection_recovers() {
        check_hung_filter_recovery(true).await;
    }

    /// A legitimate filter may outlast the old report-only eight-second
    /// deadline. It must still report real dirt, not fail or fabricate clean.
    #[cfg(unix)]
    #[tokio::test]
    async fn shared_budget_accepts_slow_successful_filter() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "--initial-branch=main"]);
        std::fs::write(dir.path().join(".gitattributes"), "asset filter=slow\n").unwrap();
        std::fs::write(
            dir.path().join("filter.sh"),
            "if test -e slow; then sleep 9; fi\ncat\n",
        )
        .unwrap();
        git(&["config", "filter.slow.clean", "sh filter.sh"]);
        git(&["config", "filter.slow.required", "true"]);
        std::fs::write(dir.path().join("asset"), "before\n").unwrap();
        git(&["add", "asset"]);
        tokio::time::sleep(Duration::from_millis(1100)).await;
        std::fs::write(dir.path().join("asset"), "after!\n").unwrap();
        std::fs::write(dir.path().join("slow"), "").unwrap();
        let started = std::time::Instant::now();
        let status = inspect_status(dir.path(), STATUS_INSPECTION_BUDGET)
            .await
            .unwrap();
        assert!(started.elapsed() >= Duration::from_secs(9));
        assert_eq!(status.modified_files, 1);
        assert!(!status.is_clean);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_copied_gitdir_alias_but_accepts_symlink_to_same_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("primary");
        let alias = dir.path().join("alias");
        std::fs::create_dir(&alias).unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(&primary)
            .output()
            .unwrap();
        assert!(init.status.success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&primary)
            .args(["config", "core.worktree"])
            .arg(&primary)
            .status()
            .unwrap()
            .success());
        std::fs::write(alias.join(".git"), "gitdir: ../primary/.git\n").unwrap();
        std::fs::write(alias.join("local.txt"), "alias edits\n").unwrap();
        let error = inspect_status(&alias, STATUS_INSPECTION_BUDGET)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("different worktree"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(alias.join("local.txt")).unwrap(),
            "alias edits\n"
        );
        assert!(inspect_status(&primary, STATUS_INSPECTION_BUDGET)
            .await
            .is_ok());
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&primary, &link).unwrap();
        assert!(inspect_status(&link, STATUS_INSPECTION_BUDGET)
            .await
            .is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn identity_accepts_linked_worktree_and_non_utf8_newline_roots() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let primary = dir
            .path()
            .join(std::ffi::OsStr::from_bytes(b"primary-\xff\n"));
        let linked = dir
            .path()
            .join(std::ffi::OsStr::from_bytes(b"linked-\xfe\n"));
        let repo = git2::Repository::init(&primary).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        // Synthetic identity belongs only to this disposable fixture object;
        // no Git configuration or operator repository is modified.
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&primary)
            .args(["worktree", "add", "--detach"])
            .arg(&linked)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for path in [&primary, &linked] {
            let status = inspect_status(path, STATUS_INSPECTION_BUDGET)
                .await
                .unwrap();
            assert!(status.is_clean);
            assert!(status.last_commit_hash.is_some());
        }
    }

    #[tokio::test]
    async fn inspects_real_repository_and_counts_nested_untracked_files() {
        let dir = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(init.status.success());
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/a"), "a").unwrap();
        std::fs::write(dir.path().join("nested/b"), "b").unwrap();
        let status = inspect_status(dir.path(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(status.branch, "main");
        assert_eq!(status.untracked_files, 2);
        assert!(!status.is_clean);
    }
}
