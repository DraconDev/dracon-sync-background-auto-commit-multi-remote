//! Daemon status inspection with an owned, cancellation-safe process tree.
//!
//! Do not use GitService::get_status here: it runs blocking libgit2/CLI work
//! that survives cancellation of its async caller. A wedged filter would then
//! outlive its reservation and repeated retries accumulate orphan work.

use anyhow::{bail, Context, Result};
use dracon_git::types::RepoStatus;
use std::{path::Path, time::Duration};

pub(crate) async fn inspect_status(repo: &Path, budget: Duration) -> Result<RepoStatus> {
    tokio::time::timeout(budget, async {
        let child = super::spawn_git_command_cancellable(
            repo,
            &[
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
            ],
            "status inspection",
        )?;
        let (exit, stdout, stderr) =
            super::run_git_captured_output(child, repo, "status inspection").await?;
        if !exit.success() {
            bail!(
                "status inspection failed in {}: {}",
                repo.display(),
                String::from_utf8_lossy(&stderr)
            );
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
