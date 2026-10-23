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

/// Call only from synchronous main, before constructing the Tokio runtime.
pub(crate) fn initialize() -> anyhow::Result<()> {
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
        let inherited = vec![PathBuf::from("/operator/bin"), home.path().join(".local/bin")];
        let current = std::env::join_paths(&inherited).unwrap();
        let result = extended_path(&current, home.path()).unwrap();
        let paths: Vec<_> = std::env::split_paths(&result).collect();
        assert!(paths.starts_with(&inherited));
        assert!(paths.contains(&home.path().join(".bun/bin")));
        assert!(paths.contains(&home.path().join(".npm-global/bin")));
        assert_eq!(paths.iter().filter(|p| **p == home.path().join(".local/bin")).count(), 1);
        assert!(!paths.contains(&home.path().join(".cargo/bin")));
        // Repeated initialization must not keep growing the search path.
        assert_eq!(extended_path(&result, home.path()).unwrap(), result);
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
