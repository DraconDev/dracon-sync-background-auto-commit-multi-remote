//! Resolve user-installed hook toolchains without sourcing interactive shells.
//!
//! systemd does not inherit a login shell's PATH. Keep the operator's existing
//! precedence and append only existing, conventional user executable directories.
//! Initialization runs before Tokio starts any threads; tests exercise the pure
//! path builder without mutating the process environment.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

fn extended_path(current: &OsStr, home: &Path) -> Result<OsString, std::env::JoinPathsError> {
    let mut paths: Vec<PathBuf> = std::env::split_paths(current).collect();
    // A relative HOME would add cwd-dependent executable search directories.
    if home.is_absolute() {
        for relative in [".local/bin", ".bun/bin", ".npm-global/bin", ".cargo/bin"] {
            let candidate = home.join(relative);
            if candidate.is_dir() && !paths.contains(&candidate) {
                paths.push(candidate);
            }
        }
    }
    std::env::join_paths(paths)
}

/// Git settings the daemon forces on every Git child it spawns.
///
/// ADDED 2026-11-14: `git commit`/`fetch`/`merge` start Git's own
/// `maintenance run --auto` → `gc --auto`, which then runs as a daemon child
/// in the daemon's 1 GiB cgroup with no timeout. Live: one on `~/.dracon`
/// sat in `pack-objects` for 2.6 hours at 1.1 GiB RSS, holding the unit at
/// 1385 of its 1400 MiB. The daemon's own auto-gc (`maybe_auto_gc`) already
/// covers loose objects, garbage and pack growth -- bounded, isolated in
/// `dracon-sync-push.slice`, and backed off on failure -- so Git's built-in
/// trigger is turned off for daemon-spawned Git only. Interactive Git and
/// CLI subcommands keep the repository's own settings.
const DAEMON_GIT_CONFIG: &[(&str, &str)] = &[("gc.auto", "0"), ("maintenance.auto", "false")];

/// `GIT_CONFIG_PARAMETERS` with the daemon's settings appended. Git reads the
/// value as whitespace-separated `'key'='value'` pairs, later entries winning.
/// Returns `None` when every setting is already present (idempotent).
fn daemon_git_config_parameters(current: Option<&str>) -> Option<String> {
    let current = current.unwrap_or("").trim();
    let mut out = current.to_string();
    for (key, value) in DAEMON_GIT_CONFIG {
        let entry = format!("'{key}'='{value}'");
        if current.split_whitespace().any(|existing| existing == entry) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&entry);
    }
    (out != current).then_some(out)
}

/// Call only from synchronous main, before constructing the Tokio runtime.
pub(crate) fn initialize() -> anyhow::Result<()> {
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "daemon") {
        let current = std::env::var("GIT_CONFIG_PARAMETERS").ok();
        if let Some(value) = daemon_git_config_parameters(current.as_deref()) {
            // Same single-threaded entry point as PATH below.
            std::env::set_var("GIT_CONFIG_PARAMETERS", value);
        }
    }
    if let (Some(current), Some(home)) = (std::env::var_os("PATH"), std::env::var_os("HOME")) {
        let path = extended_path(&current, Path::new(&home))?;
        if path != current {
            // No threads have been created at this entry point. Never move this
            // mutation into the async daemon or a per-repository worker.
            std::env::set_var("PATH", path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_operator_precedence_and_adds_installed_toolchains() {
        let home = tempfile::tempdir().unwrap();
        for relative in [".bun/bin", ".npm-global/bin", ".local/bin"] {
            std::fs::create_dir_all(home.path().join(relative)).unwrap();
        }
        let inherited = vec![
            PathBuf::from("/operator/bin"),
            home.path().join(".local/bin"),
        ];
        let current = std::env::join_paths(&inherited).unwrap();
        let result = extended_path(&current, home.path()).unwrap();
        let paths: Vec<_> = std::env::split_paths(&result).collect();
        assert!(paths.starts_with(&inherited));
        assert!(paths.contains(&home.path().join(".bun/bin")));
        assert!(paths.contains(&home.path().join(".npm-global/bin")));
        assert_eq!(
            paths
                .iter()
                .filter(|p| **p == home.path().join(".local/bin"))
                .count(),
            1
        );
        assert!(!paths.contains(&home.path().join(".cargo/bin")));
        // Repeated initialization must not keep growing the search path.
        assert_eq!(extended_path(&result, home.path()).unwrap(), result);
    }

    #[test]
    fn daemon_git_config_disables_builtin_auto_gc_idempotently() {
        let set = daemon_git_config_parameters(None).unwrap();
        assert_eq!(set, "'gc.auto'='0' 'maintenance.auto'='false'");
        assert_eq!(daemon_git_config_parameters(Some(&set)), None);
        let kept = daemon_git_config_parameters(Some("'core.pager'='cat'")).unwrap();
        assert_eq!(kept, "'core.pager'='cat' 'gc.auto'='0' 'maintenance.auto'='false'");
    }

    /// Git itself must parse the value and apply it over repository config.
    #[test]
    fn git_honours_the_daemon_git_config() {
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            crate::policy::std_git_command()
                .args(args)
                .current_dir(repo.path())
                .env("GIT_CONFIG_PARAMETERS", daemon_git_config_parameters(None).unwrap())
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        assert!(git(&["config", "--local", "gc.auto", "1"]).status.success());
        let auto = git(&["config", "gc.auto"]);
        assert_eq!(String::from_utf8_lossy(&auto.stdout).trim(), "0");
        let maintenance = git(&["config", "maintenance.auto"]);
        assert_eq!(String::from_utf8_lossy(&maintenance.stdout).trim(), "false");
    }

    #[test]
    fn absent_installations_do_not_change_path() {
        let home = tempfile::tempdir().unwrap();
        let current = OsStr::new("/operator/bin:/usr/bin");
        assert_eq!(extended_path(current, home.path()).unwrap(), current);
    }

    #[test]
    fn relative_home_cannot_add_cwd_dependent_executables() {
        let current = OsStr::new("/operator/bin:/usr/bin");
        assert_eq!(extended_path(current, Path::new(".")).unwrap(), current);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_and_existing_empty_path_entries() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let home = tempfile::tempdir().unwrap();
        let current = OsString::from_vec(b"/operator/\xff::/usr/bin".to_vec());
        let result = extended_path(&current, home.path()).unwrap();
        assert_eq!(result.as_bytes(), current.as_bytes());
    }
}
