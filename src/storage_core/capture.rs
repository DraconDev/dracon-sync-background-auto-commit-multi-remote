//! Bounded exact source selection, with descriptor-based containment on Unix.

use super::backend::BackendFailure;
use super::reference::Fingerprint;
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path};

/// Open/hash a regular repository-relative file without following source links.
/// Nested Git repositories are separate owners. Returned descriptor is rewound;
/// journal capture must revalidate its bytes against this selected fingerprint.
pub fn select_source(repo: &Path, path: &Path, max_bytes: u64) -> Result<(File, Fingerprint)> {
    if max_bytes == 0
        || !repo.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(name) if name != ".git"))
    {
        bail!(BackendFailure::Security);
    }
    let mut file = open_source(repo, path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!(BackendFailure::Security);
    }
    if metadata.len() > max_bytes {
        bail!(BackendFailure::Capacity);
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = match file.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(BackendFailure::Capacity)?;
        if total > max_bytes {
            bail!(BackendFailure::Capacity);
        }
        hash.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok((
        file,
        Fingerprint::new(format!("{:x}", hash.finalize()), total)?,
    ))
}

#[cfg(unix)]
fn open_source(repo: &Path, path: &Path) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    fn open(directory: &File, name: &std::ffi::OsStr, flags: i32) -> Result<File> {
        let name = std::ffi::CString::new(name.as_bytes())?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    let mut directory = File::open("/")?;
    for component in repo.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = open(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?
            }
            _ => bail!(BackendFailure::Security),
        }
    }
    let parts: Vec<_> = path.components().collect();
    for (index, component) in parts.iter().enumerate() {
        let Component::Normal(name) = component else {
            bail!(BackendFailure::Security)
        };
        if index + 1 == parts.len() {
            return open(&directory, name, libc::O_RDONLY | libc::O_NONBLOCK);
        }
        directory = open(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                c".git".as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            bail!(BackendFailure::Security);
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound {
            bail!(BackendFailure::Security);
        }
    }
    bail!(BackendFailure::Security)
}

#[cfg(not(unix))]
fn open_source(_: &Path, _: &Path) -> Result<File> {
    // A platform must supply equivalent descriptor containment before capture.
    bail!(BackendFailure::Security)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn exact_source_selection_is_bounded_and_rewound() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("asset"), b"exact retained bytes").unwrap();
        let (mut input, identity) = select_source(root.path(), Path::new("asset"), 20).unwrap();
        // A path replacement cannot redirect the already selected descriptor.
        std::fs::remove_file(root.path().join("asset")).unwrap();
        std::fs::write(root.path().join("asset"), b"newer edits preserved").unwrap();
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"exact retained bytes");
        assert_eq!(identity.bytes(), 20);
        assert_eq!(
            std::fs::read(root.path().join("asset")).unwrap(),
            b"newer edits preserved"
        );
        assert!(select_source(root.path(), Path::new("asset"), 19).is_err());
    }
    #[test]
    fn links_nested_repos_git_internals_and_nonregular_files_are_refused() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        std::fs::write(root.path().join("child/asset"), b"child bytes").unwrap();
        std::fs::write(root.path().join("child/.git"), b"gitdir: elsewhere").unwrap();
        symlink(root.path().join("child/asset"), root.path().join("link")).unwrap();
        symlink(root.path().join("child"), root.path().join("linked-dir")).unwrap();
        for path in [
            "child/asset",
            "link",
            "linked-dir/asset",
            "child",
            ".git/config",
            "../asset",
            "/asset",
            "",
        ] {
            assert!(select_source(root.path(), Path::new(path), 100).is_err());
        }
        let fifo = std::ffi::CString::new(root.path().join("fifo").as_os_str().as_encoded_bytes())
            .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(select_source(root.path(), Path::new("fifo"), 100).is_err());
    }
}
