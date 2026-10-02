//! Bounded read-only operator credential resolution, without environment fallback.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::path::Path;
use std::time::{Duration, SystemTime};
use zeroize::{Zeroize, Zeroizing};

use super::http::Credentials;
use crate::storage_core::backend::BackendFailure;

const MAX_BYTES: u64 = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    access_key_id: String,
    secret_access_key: String,
    #[serde(default)]
    session_token: Option<String>,
    #[serde(default)]
    expires_unix_secs: Option<u64>,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.access_key_id.zeroize();
        self.secret_access_key.zeroize();
        if let Some(token) = &mut self.session_token {
            token.zeroize();
        }
    }
}

/// Read `<reference>.json` from an explicitly selected private operator directory.
/// Never creates files, searches other stores, follows symlinks or echoes errors.
pub fn load(root: &Path, reference: &str) -> Result<Credentials> {
    load_inner(root, reference).map_err(|_| BackendFailure::Security.into())
}

#[cfg(unix)]
fn load_inner(root: &Path, reference: &str) -> Result<Credentials> {
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    crate::storage_core::journal::identifier(reference)?;
    if !root.is_absolute() {
        bail!(BackendFailure::Security);
    }
    let mut directory = File::open("/")?;
    for component in root.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => std::ffi::CString::new(name.as_bytes())?,
            _ => bail!(BackendFailure::Security),
        };
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        directory = unsafe { File::from_raw_fd(descriptor) };
    }
    let metadata = directory.metadata()?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!(BackendFailure::Security);
    }
    let name = std::ffi::CString::new(format!("{reference}.json"))?;
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || !matches!(metadata.mode() & 0o777, 0o400 | 0o600)
        || metadata.len() > MAX_BYTES
    {
        bail!(BackendFailure::Security);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!(BackendFailure::Security);
    }
    let mut record: Record =
        serde_json::from_slice(&bytes).map_err(|_| BackendFailure::Security)?;
    if record.version != 1 {
        bail!(BackendFailure::Security);
    }
    let expiry = match record.expires_unix_secs {
        None => None,
        Some(seconds) => Some(
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(seconds))
                .ok_or(BackendFailure::Security)?,
        ),
    };
    let credentials = Credentials::new(
        std::mem::take(&mut record.access_key_id),
        std::mem::take(&mut record.secret_access_key),
        record.session_token.take(),
        expiry,
    )?;
    credentials.check_expiration()?;
    Ok(credentials)
}

#[cfg(not(unix))]
fn load_inner(_: &Path, _: &str) -> Result<Credentials> {
    bail!(BackendFailure::Security)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let file = root.path().join("approved.json");
        std::fs::write(&file, br#"{"version":1,"access_key_id":"TESTACCESS123","secret_access_key":"isolated-secret-not-live","session_token":"test-session"}"#).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        (root, file)
    }
    #[test]
    fn explicit_private_credentials_load_without_mutating_files() {
        let (root, file) = fixture();
        let before = std::fs::read(&file).unwrap();
        assert!(load(root.path(), "approved").is_ok());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert!(load(root.path(), "missing").is_err());
        assert!(load(root.path(), "../approved").is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn unsafe_permissions_links_and_oversized_records_refuse() {
        let (root, file) = fixture();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(root.path(), "approved").is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = root.path().join("alias.json");
        std::fs::hard_link(&file, &alias).unwrap();
        assert!(load(root.path(), "approved").is_err());
        std::fs::remove_file(&alias).unwrap();
        symlink(&file, &alias).unwrap();
        assert!(load(root.path(), "alias").is_err());
        let link = root.path().join("linked-root");
        symlink(root.path(), &link).unwrap();
        assert!(load(&link, "approved").is_err());
        std::fs::write(&file, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        assert!(load(root.path(), "approved").is_err());
    }
    #[test]
    fn malformed_unknown_and_expired_records_redact_contents() {
        let (root, file) = fixture();
        for bytes in [br#"{"version":1,"access_key_id":"PRIVATE-SENTINEL"}"#.as_slice(),
            br#"{"version":2,"access_key_id":"TESTACCESS123","secret_access_key":"isolated-secret-not-live"}"#,
            br#"{"version":1,"access_key_id":"TESTACCESS123","secret_access_key":"isolated-secret-not-live","unknown":"PRIVATE-SENTINEL"}"#,
            br#"{"version":1,"access_key_id":"TESTACCESS123","secret_access_key":"isolated-secret-not-live","expires_unix_secs":1}"#] {
            std::fs::write(&file,bytes).unwrap();
            let error=load(root.path(),"approved").err().unwrap();
            assert_eq!(error.to_string(),BackendFailure::Security.to_string());
            assert!(!format!("{error:#}").contains("PRIVATE-SENTINEL"));
        }
    }
}
