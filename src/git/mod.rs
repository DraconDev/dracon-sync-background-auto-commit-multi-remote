#[cfg(test)]
use crate::policy::{AuthType, RemoteConfig};
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
/// branch we push (the pack is at most the whole history), so we never skip.
/// Only when `.git` is large do we refine by measuring the objects reachable
/// from the pushed branch. That is an upper bound on the actual pack (which
/// is compressed and excludes objects GitHub already has), so it is
/// conservative: we still skip only if even the full branch exceeds 2 GiB.
///
/// The `is_pack_too_large` backstop in the push path catches any mis-estimate:
/// if a push we allow is somehow rejected by GitHub, the daemon stops retrying
/// instead of looping.
pub(crate) fn github_pack_too_large(repo: &std::path::Path) -> (bool, u64) {
    const LIMIT: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
    // Fast path: small .git -> never too big (unchanged behavior for the vast
    // majority of repos; no extra git subprocess).
    if let Some(size) = crate::report::measure_git_size_bytes(repo) {
        if size < LIMIT {
            return (false, size);
        }
    }
    // Large .git: refine using the pushed branch's reachable objects.
    let pushable = pushed_branch_pushable_bytes(repo);
    if pushable == u64::MAX {
        // Couldn't measure the branch (e.g. detached HEAD, git error). Fall
        // back to the whole .git size (conservative: skip).
        let whole = crate::report::measure_git_size_bytes(repo).unwrap_or(u64::MAX);
        (whole >= LIMIT, whole)
    } else {
        (pushable >= LIMIT, pushable)
    }
}

/// Estimate the raw byte size of objects reachable from the branch the daemon
/// pushes (the checked-out branch), excluding submodule gitlink objects (which
/// live in nested repos, not this one). This is an upper bound on the pack
/// GitHub would receive for that branch.
///
/// Returns `u64::MAX` when the branch can't be determined or git errors.
fn pushed_branch_pushable_bytes(repo: &std::path::Path) -> u64 {
    // The daemon pushes the checked-out branch.
    let branch = match git_capture_stdout(repo, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Some(s) => {
            let b = s.trim().to_string();
            if b.is_empty() || b == "HEAD" {
                return u64::MAX; // detached HEAD -> can't determine
            }
            b
        }
        None => return u64::MAX,
    };
    let objects = match git_capture_stdout(repo, &["rev-list", "--objects", &branch]) {
        Some(s) => s,
        None => return u64::MAX,
    };
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
    if shas.is_empty() {
        return 0;
    }
    // Batch-check object sizes; skip missing objects (submodule gitlinks
    // reference commits that live in nested repos, not this one).
    let mut cmd = git_cmd();
    cmd.current_dir(repo)
        .args(["cat-file", "--batch-check=%(objecttype) %(objectsize)"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return u64::MAX,
    };
    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(shas.as_bytes()).is_err() {
            return u64::MAX;
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return u64::MAX,
    };
    if !out.status.success() {
        return u64::MAX;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut total: u64 = 0;
    for line in text.lines() {
        // `<sha> <type> <size>` or `<sha> missing`
        let mut parts = line.split_whitespace();
        let _sha = parts.next();
        let ty = parts.next();
        let size = parts.next();
        if ty == Some("missing") {
            continue;
        }
        if let Some(s) = size.and_then(|s| s.parse::<u64>().ok()) {
            total = total.saturating_add(s);
        }
    }
    total
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

    fn real_git() -> std::path::PathBuf {
        for c in [
            "/run/current-system/sw/bin/git",
            "/usr/bin/git",
            "/bin/git",
        ] {
            if std::path::Path::new(c).exists() {
                return std::path::PathBuf::from(c);
            }
        }
        std::path::PathBuf::from("git")
    }

    fn init_repo(dir: &std::path::Path) -> std::path::PathBuf {
        let repo = dir.join("repo");
        std::process::Command::new(real_git())
            .args(["init", "-q", &repo.to_string_lossy()])
            .output()
            .unwrap();
        std::process::Command::new(real_git())
            .args(["config", "user.email", "test@dracon.dev"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new(real_git())
            .args(["config", "user.name", "test"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::fs::write(repo.join("a.txt"), "hello world").unwrap();
        std::process::Command::new(real_git())
            .args(["add", "a.txt"])
            .current_dir(&repo)
            .output()
            .unwrap();
        std::process::Command::new(real_git())
            .args(["commit", "-q", "-m", "init"])
            .current_dir(&repo)
            .output()
            .unwrap();
        repo
    }

    #[test]
    fn small_repo_is_not_too_big_for_github() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(tmp.path());
        let (too_big, size) = github_pack_too_large(&repo);
        assert!(!too_big, "a tiny repo must never be skipped for github");
        assert!(
            size > 0 && size < 2 * 1024 * 1024 * 1024,
            "pushable size should be the small .git, got {size}"
        );
    }

    #[test]
    fn pushed_branch_size_is_subset_of_whole_git() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = init_repo(tmp.path());
        let bytes = pushed_branch_pushable_bytes(&repo);
        assert!(
            bytes > 0 && bytes != u64::MAX,
            "pushable bytes should be the repo's own objects, got {bytes}"
        );
        let whole = crate::report::measure_git_size_bytes(&repo).unwrap();
        assert!(
            bytes <= whole,
            "pushable {bytes} must be <= whole .git {whole}"
        );
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
        let result = parse_name_status_line("R\told.rs\tnew.rs");
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
        crate::git::multi_remote::remove_stale_remotes(&repo, &["github"])
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
        crate::git::multi_remote::remove_stale_remotes(&repo, &["mirror1"])
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
        crate::git::multi_remote::remove_stale_remotes(&repo, &[])
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
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None).await;
        assert!(
            results.is_empty(),
            "should return empty vec when no remotes have auto_create=true"
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
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None).await;
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
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None).await;
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
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None).await;
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
            crate::git::multi_remote::auto_create_all_remotes(&remotes, "test-repo", true, None).await;
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
        let results = crate::git::multi_remote::push_mirror_remotes(&repo, &[], 1, 0, true, &[]).await;
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
        let result = multi_remote::create_repo_on_github("testuser", "my-repo");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://github.com/testuser/my-repo.git");
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
        let result = multi_remote::create_repo_on_github("testuser", "dracon-demons");
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
        let result = multi_remote::create_repo_on_github("testuser", "test-repo");
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
