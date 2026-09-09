use crate::policy::{RepoPolicyOverride, SyncPolicy};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolve a standard-file target against the canonical repository root.
///
/// Lexical checks prevent `..` and absolute paths, but they do not account
/// for a checkout-controlled symlink such as `.github -> /tmp/out`. Probe
/// the target or its nearest existing ancestor with `canonicalize` before
/// any existence check, directory creation, overwrite, or copy. A target
/// is accepted only when that resolved path remains below the repository.
pub(crate) fn resolve_standard_file_target(repo: &Path, target: &str) -> Result<PathBuf> {
    if !crate::policy::is_safe_standard_file_path(target) {
        anyhow::bail!("target path '{}' is not a safe relative path", target);
    }

    let repo_root = std::fs::canonicalize(repo)
        .with_context(|| format!("failed to resolve repository {}", repo.display()))?;
    let target_path = repo_root.join(target);

    // `canonicalize` requires the complete path to exist. Walk upward until
    // we find an existing entry, retaining symlinks (including dangling
    // ones) via symlink_metadata so they cannot be bypassed as "missing".
    let mut probe = target_path.clone();
    loop {
        match std::fs::symlink_metadata(&probe) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !probe.pop() {
                    anyhow::bail!(
                        "target path '{}' has no existing repository ancestor",
                        target
                    );
                }
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect standard-file target '{}'", target)
                });
            }
        }
    }

    let resolved_probe = std::fs::canonicalize(&probe)
        .with_context(|| format!("failed to resolve standard-file target '{}'", target))?;
    if !resolved_probe.starts_with(&repo_root) {
        anyhow::bail!(
            "standard-file target '{}' resolves outside repository ({} -> {})",
            target,
            probe.display(),
            resolved_probe.display()
        );
    }

    Ok(target_path)
}

#[cfg(unix)]
mod secure_target {
    use anyhow::{anyhow, Context, Result};
    use std::ffi::{CStr, CString, OsStr};
    use std::fs::File;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
    use std::path::{Component, Path};

    enum EntryKind {
        Directory,
        Symlink,
        Other,
    }

    fn component_name(component: &OsStr) -> Result<CString> {
        CString::new(component.as_bytes()).map_err(|_| {
            anyhow!(
                "standard-file target component {:?} contains an embedded NUL",
                component
            )
        })
    }

    fn target_components(target: &str) -> Result<Vec<CString>> {
        let mut components = Vec::new();
        for component in Path::new(target).components() {
            match component {
                Component::Normal(name) => components.push(component_name(name)?),
                Component::CurDir => {}
                _ => anyhow::bail!("target path '{}' is not a safe relative path", target),
            }
        }
        if components.is_empty() {
            anyhow::bail!("target path '{}' has no file component", target);
        }
        Ok(components)
    }

    fn open_directory_at(parent: RawFd, name: &CStr) -> io::Result<File> {
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a valid owned descriptor.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn open_repository(repo: &Path) -> Result<File> {
        let repo_name = CString::new(repo.as_os_str().as_bytes())
            .map_err(|_| anyhow!("repository path contains an embedded NUL"))?;
        let fd = unsafe {
            libc::open(
                repo_name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error()).with_context(|| {
                format!("failed to open repository directory {}", repo.display())
            });
        }
        // SAFETY: open returned a valid owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn make_directory_at(parent: RawFd, name: &CStr) -> io::Result<()> {
        let result = unsafe { libc::mkdirat(parent, name.as_ptr(), 0o755) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn open_or_create_parent(root: File, components: &[CString]) -> io::Result<File> {
        let mut current = root;
        for component in components {
            match open_directory_at(current.as_raw_fd(), component.as_c_str()) {
                Ok(next) => current = next,
                Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
                    match make_directory_at(current.as_raw_fd(), component.as_c_str()) {
                        Ok(()) => {}
                        Err(mkdir_error)
                            if mkdir_error.raw_os_error() == Some(libc::EEXIST) => {}
                        Err(mkdir_error) => return Err(mkdir_error),
                    }
                    // Re-open with O_NOFOLLOW. If a concurrent actor replaced
                    // the new directory with a symlink, this fails closed.
                    current = open_directory_at(current.as_raw_fd(), component.as_c_str())?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(current)
    }

    fn stat_at(parent: RawFd, name: &CStr) -> io::Result<Option<libc::stat>> {
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                parent,
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(None)
            } else {
                Err(error)
            }
        } else {
            // SAFETY: fstatat initialized metadata when it returned success.
            Ok(Some(unsafe { metadata.assume_init() }))
        }
    }

    fn entry_kind(metadata: &libc::stat) -> EntryKind {
        let mode = metadata.st_mode as libc::mode_t;
        if mode & libc::S_IFMT == libc::S_IFDIR {
            EntryKind::Directory
        } else if mode & libc::S_IFMT == libc::S_IFLNK {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        }
    }

    fn unlink_at(parent: RawFd, name: &CStr, flags: libc::c_int) -> io::Result<()> {
        let result = unsafe { libc::unlinkat(parent, name.as_ptr(), flags) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn clear_errno() {
        // SAFETY: __errno_location returns this thread's errno slot.
        unsafe {
            *libc::__errno_location() = 0;
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn clear_errno() {}

    fn remove_directory_at(parent: RawFd, name: &CStr) -> io::Result<()> {
        let directory = open_directory_at(parent, name)?;
        let directory_fd = directory.as_raw_fd();
        let raw_directory_fd = directory.into_raw_fd();
        let directory_stream = unsafe { libc::fdopendir(raw_directory_fd) };
        if directory_stream.is_null() {
            let error = io::Error::last_os_error();
            // fdopendir did not take ownership when it failed.
            unsafe {
                libc::close(raw_directory_fd);
            }
            return Err(error);
        }

        let scan_result = loop {
            clear_errno();
            let entry = unsafe { libc::readdir(directory_stream) };
            if entry.is_null() {
                #[cfg(target_os = "linux")]
                {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error().unwrap_or(0) != 0 {
                        break Err(error);
                    }
                }
                break Ok(());
            }

            // SAFETY: readdir returns a pointer to a valid dirent until the
            // next readdir call, and d_name is NUL-terminated by the API.
            let child_name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if child_name.to_bytes() == b"." || child_name.to_bytes() == b".." {
                continue;
            }

            let Some(metadata) = stat_at(directory_fd, child_name)? else {
                // The entry may have vanished concurrently; there is nothing
                // left for this operation to remove.
                continue;
            };
            match entry_kind(&metadata) {
                EntryKind::Directory => {
                    match remove_directory_at(directory_fd, child_name) {
                        Ok(()) => {}
                        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {}
                        Err(error) => break Err(error),
                    }
                }
                // unlinkat never follows a symlink, so a symlink child is
                // removed as an entry rather than traversed.
                EntryKind::Symlink | EntryKind::Other => {
                    match unlink_at(directory_fd, child_name, 0) {
                        Ok(()) => {}
                        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {}
                        Err(error) => break Err(error),
                    }
                }
            }
        };
        let close_result = unsafe { libc::closedir(directory_stream) };
        scan_result?;
        if close_result < 0 {
            return Err(io::Error::last_os_error());
        }

        // The directory was opened with O_NOFOLLOW and all descendants were
        // removed through its descriptor. AT_REMOVEDIR removes only this
        // directory entry and never follows a symlink.
        unlink_at(parent, name, libc::AT_REMOVEDIR)
    }

    fn create_file_at(parent: RawFd, name: &CStr) -> io::Result<File> {
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC,
                0o666,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: openat returned a valid owned descriptor.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    pub(super) fn copy(
        repo: &Path,
        target: &str,
        source: &Path,
        overwrite: bool,
    ) -> Result<bool> {
        let components = target_components(target)?;
        let root = open_repository(repo)?;
        let mut source_file = File::open(source)
            .with_context(|| format!("failed to open standard-file source {}", source.display()))?;
        let (filename, parents) = components
            .split_last()
            .expect("target_components always returns a filename");
        let parent = open_or_create_parent(root, parents).with_context(|| {
            format!("failed to open standard-file target parent for '{}'", target)
        })?;

        if let Some(metadata) = stat_at(parent.as_raw_fd(), filename.as_c_str())? {
            match entry_kind(&metadata) {
                EntryKind::Symlink => {
                    if !overwrite {
                        return Ok(false);
                    }
                    // Remove only the link itself. This is safe even if a
                    // concurrent actor inserted a link to an external path.
                    unlink_at(parent.as_raw_fd(), filename.as_c_str(), 0).with_context(|| {
                        format!("failed to remove existing standard-file link '{}'", target)
                    })?;
                }
                EntryKind::Directory => {
                    if !overwrite {
                        return Ok(false);
                    }
                    remove_directory_at(parent.as_raw_fd(), filename.as_c_str()).with_context(
                        || format!("failed to remove existing directory {}", target),
                    )?;
                }
                EntryKind::Other => {
                    if !overwrite {
                        return Ok(false);
                    }
                    unlink_at(parent.as_raw_fd(), filename.as_c_str(), 0).with_context(|| {
                        format!("failed to remove existing standard file {}", target)
                    })?;
                }
            }
        }

        let mut output = match create_file_at(parent.as_raw_fd(), filename.as_c_str()) {
            Ok(file) => file,
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) && !overwrite => {
                // A target appeared after the no-overwrite check. Do not
                // follow it; report that the caller should treat it as done.
                return Ok(false);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create standard-file target {}", target)
                });
            }
        };

        if let Err(error) = std::io::copy(&mut source_file, &mut output) {
            // Best effort cleanup is descriptor-relative and cannot follow a
            // replacement symlink. The original copy error remains primary.
            let _ = unlink_at(parent.as_raw_fd(), filename.as_c_str(), 0);
            return Err(error).with_context(|| {
                format!("failed to copy standard-file source to {}", target)
            });
        }
        Ok(true)
    }
}

pub(crate) fn copy_standard_file_within_repo(
    repo: &Path,
    target: &str,
    source: &Path,
    overwrite: bool,
) -> Result<bool> {
    // Keep the canonical preflight as the clear, user-facing rejection path.
    // Unix mutation below repeats the path decomposition using directory
    // descriptors so a later ancestor replacement cannot redirect I/O.
    let target_path = resolve_standard_file_target(repo, target)?;

    #[cfg(unix)]
    {
        let _ = target_path;
        secure_target::copy(repo, target, source, overwrite)
    }

    #[cfg(not(unix))]
    {
        if target_path.exists() && !overwrite {
            return Ok(false);
        }
        if target_path.exists() && overwrite {
            if target_path.is_dir() {
                std::fs::remove_dir_all(&target_path).with_context(|| {
                    format!("failed to remove existing directory {}", target)
                })?;
            } else {
                std::fs::remove_file(&target_path)
                    .with_context(|| format!("failed to remove existing {}", target))?;
            }
        }
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        std::fs::copy(source, &target_path).with_context(|| {
            format!("failed to copy {} to {}", source.display(), target)
        })?;
        Ok(true)
    }
}

pub(crate) fn ensure_standard_files(
    repo: &Path,
    policy: &SyncPolicy,
    repo_override: &RepoPolicyOverride,
    policy_base_dir: Option<&Path>,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    if policy.standard_files.is_empty() {
        return Ok(vec![]);
    }

    let sync_base = policy_base_dir
        .map(|p| p.to_path_buf())
        .or_else(|| dirs::home_dir().map(|h| h.join(".dracon/utilities/sync")));

    let Some(base) = sync_base else {
        anyhow::bail!("cannot resolve standard files base dir: no policy path and no home dir");
    };

    let mut copied = Vec::new();

    for cfg in &policy.standard_files {
        if repo_override.skip_standard_files.contains(&cfg.target) {
            continue;
        }

        // ADDED 2026-07-26 (v0.113.4, audit SYNC-H5; tightened
        // 2026-09-09, audit F28): point-of-use enforcement — the
        // daemon's execution path never calls `validate_config`, so an
        // unsafe source/target here would copy ANY readable file
        // (`~/.ssh/id_rsa`, `../../etc/passwd`) into every watched
        // repo, auto-committed + auto-pushed to public forges. F28:
        // `~/...` counts as unsafe (tilde is absolute-after-expansion
        // and resolves outside the sync base). Empty and root-equivalent
        // paths are unsafe too: with overwrite enabled, they could resolve
        // to `repo` and recursively delete the checkout. Skip + warn instead.
        if !crate::policy::is_safe_standard_file_path(&cfg.source)
            || !crate::policy::is_safe_standard_file_path(&cfg.target)
        {
            eprintln!(
                "⚠️ standard file '{}' (source '{}') rejected: paths must be non-empty relative paths below the base dir (no absolute, '~'-prefixed, '..', or root-equivalent paths) — skipping",
                cfg.target,
                cfg.source
            );
            continue;
        }

        let target_path = match resolve_standard_file_target(repo, &cfg.target) {
            Ok(path) => path,
            Err(error) => {
                eprintln!(
                    "⚠️ standard file '{}' rejected: {} — skipping",
                    cfg.target, error
                );
                continue;
            }
        };

        if target_path.exists() && !cfg.overwrite {
            continue;
        }

        let source_path = cfg.source_path(&base);

        if !source_path.exists() {
            eprintln!(
                "⚠️ standard file template missing: {} (tried {})",
                cfg.target,
                source_path.display()
            );
            continue;
        }

        if dry_run {
            println!(
                "📝 Would copy standard file: {} -> {}",
                source_path.display(),
                target_path.display()
            );
            copied.push(target_path);
            continue;
        }

        if copy_standard_file_within_repo(repo, &cfg.target, &source_path, cfg.overwrite)
            .with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?
        {
            copied.push(target_path);
        }
    }

    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::StandardFileConfig;
    use tempfile::TempDir;

    fn make_policy(standard_files: Vec<StandardFileConfig>) -> SyncPolicy {
        SyncPolicy {
            standard_files,
            ..Default::default()
        }
    }

    fn make_override(skip: Vec<String>) -> RepoPolicyOverride {
        RepoPolicyOverride {
            skip_standard_files: skip,
            ..Default::default()
        }
    }

    #[test]
    fn test_copies_file_when_missing() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "AGPL"
        );
    }

    #[test]
    fn test_skips_when_target_exists() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        std::fs::write(repo_dir.join("LICENSE"), "EXISTING").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert!(copied.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "EXISTING"
        );
    }

    #[test]
    fn test_overwrites_when_configured() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "NEW_AGPL").unwrap();
        std::fs::write(repo_dir.join("LICENSE"), "OLD_LICENSE").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: true,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "NEW_AGPL"
        );
    }

    #[test]
    fn test_skips_from_repo_override() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("CUSTOM.md"), "custom content").unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/CUSTOM.md".to_string(),
            target: "CUSTOM.md".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec!["CUSTOM.md".to_string()]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert!(copied.is_empty());
        assert!(!repo_dir.join("CUSTOM.md").exists());
    }

    #[test]
    fn test_warns_when_template_missing() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/NONEXISTENT".to_string(),
            target: "NONEXISTENT.txt".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dry_run_does_not_copy_files() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), true);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert!(
            !repo_dir.join("LICENSE").exists(),
            "dry-run must not write files"
        );
    }

    #[test]
    fn test_subdirectory_target_creates_parent() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "docs/LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("docs/LICENSE")).unwrap(),
            "AGPL"
        );
    }

    #[test]
    fn test_overwrite_directory_target() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();
        std::fs::create_dir(repo_dir.join("LICENSE")).unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: true,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        assert!(repo_dir.join("LICENSE").is_file());
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "AGPL"
        );
    }

    #[test]
    fn test_absolute_source_path() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let abs_template = dir.path().join("custom_license.txt");
        std::fs::write(&abs_template, "CUSTOM").unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: abs_template.to_string_lossy().to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        // CHANGED 2026-07-26 (v0.113.4, audit SYNC-H5): raw-absolute
        // sources are now rejected at the point of use (the daemon
        // path never ran validate_config, so absolute/`..` sources
        // were a read-anywhere → publish-everywhere primitive).
        // Nothing is copied and the target must not appear.
        assert_eq!(copied.len(), 0);
        assert!(!repo_dir.join("LICENSE").exists());
    }

    #[test]
    fn test_parent_dir_source_rejected_at_point_of_use() {
        // ADDED 2026-07-26 (v0.113.4, audit SYNC-H5).
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        let policy = make_policy(vec![StandardFileConfig {
            source: "../../etc/passwd".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);
        let repo_override = make_override(vec![]);
        let result = ensure_standard_files(&repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
        assert!(!repo_dir.join("LICENSE").exists());
    }

    #[test]
    fn test_tilde_source_rejected() {
        // CHANGED 2026-09-09 (audit F28): `~/...` was blessed by
        // SYNC-H5 but resolves outside the sync base, so `~/.ssh/id_rsa`
        // exfiltrated HOME keys into watched repos. Tilde sources are
        // now rejected; templates must be relative to the sync base.
        assert!(!crate::policy::is_safe_standard_file_path(
            "~/templates/LICENSE"
        ));
        assert!(!crate::policy::is_safe_standard_file_path("~"));
        assert!(!crate::policy::is_safe_standard_file_path("~templates/LICENSE"));
        assert!(crate::policy::is_safe_standard_file_path(
            "templates/LICENSE"
        ));
        assert!(crate::policy::is_safe_standard_file_path(
            ".github/FUNDING.yml"
        ));
        assert!(!crate::policy::is_safe_standard_file_path("/etc/passwd"));
        assert!(!crate::policy::is_safe_standard_file_path("../secret"));
        assert!(!crate::policy::is_safe_standard_file_path("a/../../b"));
        assert!(!crate::policy::is_safe_standard_file_path(""));
        assert!(!crate::policy::is_safe_standard_file_path("."));
        assert!(!crate::policy::is_safe_standard_file_path("./"));
        assert!(!crate::policy::is_safe_standard_file_path("././"));
    }

    #[test]
    fn test_root_equivalent_target_cannot_delete_repo_on_overwrite() {
        for target in [".", "", "./", "././"] {
            let dir = TempDir::new().unwrap();
            let repo_dir = dir.path().join("repo");
            std::fs::create_dir_all(repo_dir.join(".git")).unwrap();
            std::fs::write(repo_dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
            let template_dir = dir.path().join("templates");
            std::fs::create_dir(&template_dir).unwrap();
            std::fs::write(template_dir.join("LICENSE"), "AGPL").unwrap();

            let policy = make_policy(vec![StandardFileConfig {
                source: "templates/LICENSE".to_string(),
                target: target.to_string(),
                overwrite: true,
            }]);
            let repo_override = make_override(vec![]);

            let result = ensure_standard_files(
                &repo_dir,
                &policy,
                &repo_override,
                Some(dir.path()),
                false,
            )
            .unwrap();

            assert!(
                result.is_empty(),
                "unsafe target {target:?} must not be copied"
            );
            assert!(
                repo_dir.is_dir(),
                "unsafe target {target:?} must not remove repository"
            );
            assert!(
                repo_dir.join(".git/HEAD").is_file(),
                "unsafe target {target:?} must preserve checkout metadata"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_directory_target_cannot_write_or_delete_external() {
        use std::os::unix::fs::symlink;

        for (overwrite, existing_external_file) in [(false, false), (true, true)] {
            let dir = TempDir::new().unwrap();
            let repo_dir = dir.path().join("repo");
            std::fs::create_dir_all(repo_dir.join(".git")).unwrap();
            let external_dir = dir.path().join("outside");
            std::fs::create_dir(&external_dir).unwrap();
            let external_file = external_dir.join("FUNDING.yml");
            if existing_external_file {
                std::fs::write(&external_file, "outside content").unwrap();
            }
            symlink(&external_dir, repo_dir.join(".github")).unwrap();

            let template_dir = dir.path().join("templates");
            std::fs::create_dir(&template_dir).unwrap();
            std::fs::write(template_dir.join("FUNDING.yml"), "repo content").unwrap();

            let policy = make_policy(vec![StandardFileConfig {
                source: "templates/FUNDING.yml".to_string(),
                target: ".github/FUNDING.yml".to_string(),
                overwrite,
            }]);
            let repo_override = make_override(vec![]);
            let copied = ensure_standard_files(
                &repo_dir,
                &policy,
                &repo_override,
                Some(dir.path()),
                false,
            )
            .unwrap();

            assert!(copied.is_empty(), "symlink escape must not copy");
            assert!(repo_dir.join(".git").is_dir());
            assert!(
                std::fs::symlink_metadata(repo_dir.join(".github"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the checkout symlink must remain untouched"
            );
            if existing_external_file {
                assert_eq!(
                    std::fs::read_to_string(external_file).unwrap(),
                    "outside content"
                );
            } else {
                assert!(
                    !external_file.exists(),
                    "rejected target must not write outside the repository"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_directory_target_cannot_delete_external_directory() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path().join("repo");
        std::fs::create_dir_all(repo_dir.join(".git")).unwrap();
        let external_dir = dir.path().join("outside");
        std::fs::create_dir(&external_dir).unwrap();
        std::fs::write(external_dir.join("keep.txt"), "keep me").unwrap();
        symlink(&external_dir, repo_dir.join(".github")).unwrap();

        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("FUNDING.yml"), "repo content").unwrap();
        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/FUNDING.yml".to_string(),
            target: ".github".to_string(),
            overwrite: true,
        }]);

        let copied = ensure_standard_files(
            &repo_dir,
            &policy,
            &make_override(vec![]),
            Some(dir.path()),
            false,
        )
        .unwrap();

        assert!(copied.is_empty(), "symlink escape must not delete or copy");
        assert!(external_dir.is_dir(), "external directory must remain");
        assert_eq!(
            std::fs::read_to_string(external_dir.join("keep.txt")).unwrap(),
            "keep me"
        );
        assert!(std::fs::symlink_metadata(repo_dir.join(".github"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn test_funding_yml_in_dot_github_subdir() {
        // GitHub discovers FUNDING.yml at .github/FUNDING.yml. The standard
        // files flow must allow long-form entries that target subdirectories
        // like .github/ while pulling the source from templates/FUNDING.yml.
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("FUNDING.yml"), "github: []\n").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/FUNDING.yml".to_string(),
            target: ".github/FUNDING.yml".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert!(repo_dir.join(".github/FUNDING.yml").exists());
        assert_eq!(
            std::fs::read_to_string(repo_dir.join(".github/FUNDING.yml")).unwrap(),
            "github: []\n"
        );
    }

    #[test]
    fn test_funding_yml_skip_standard_files_optout() {
        // Per-repo skip_standard_files must opt out FUNDING.yml cleanly.
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("FUNDING.yml"), "github: []\n").unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/FUNDING.yml".to_string(),
            target: ".github/FUNDING.yml".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![".github/FUNDING.yml".to_string()]);
        let result = ensure_standard_files(repo_dir, &policy, &repo_override, None, false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert!(copied.is_empty());
        assert!(!repo_dir.join(".github/FUNDING.yml").exists());
    }

    #[test]
    fn test_short_form_source_resolution() {
        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path();
        let template_dir = dir.path().join("templates");
        std::fs::create_dir(&template_dir).unwrap();
        std::fs::write(template_dir.join("LICENSE"), "AGPLv3").unwrap();
        let sync_path = dir.path().join("sync.toml");
        let sync_dir = sync_path.parent().unwrap();

        let policy = make_policy(vec![StandardFileConfig {
            source: "templates/LICENSE".to_string(),
            target: "LICENSE".to_string(),
            overwrite: false,
        }]);

        let repo_override = make_override(vec![]);
        let result =
            ensure_standard_files(repo_dir, &policy, &repo_override, Some(sync_dir), false);
        assert!(result.is_ok());
        let copied = result.unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("LICENSE")).unwrap(),
            "AGPLv3"
        );
    }
}
