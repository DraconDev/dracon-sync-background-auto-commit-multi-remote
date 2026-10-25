//! Retained originals can still receive an open that raced an exchange. Moving
//! their directory is NOT immutability proof. Probe metadata without rehashing
//! historical payloads; any change/missing seal remains a persistent concern.
use super::*;
use std::os::unix::fs::MetadataExt;

const SEAL: &str = "archive-state.json";
const MAX_SEAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Stamp {
    identity: (u64, u64, u64, u32, u32, u32, i64, i64),
    ctime: (i64, i64),
    links: u64,
}
impl Stamp {
    pub(super) fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            identity: identity(meta),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            links: meta.nlink(),
        }
    }
}
#[derive(PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Entry {
    pub path: Vec<u8>,
    pub observed: Option<Stamp>,
}
#[derive(PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Seal {
    pub schema: u8,
    pub repo: Vec<u8>,
    pub request: Stamp,
    pub targets: Vec<Entry>,
}

pub(super) fn stamp(location: &Location) -> Result<Option<Stamp>> {
    location
        .file()?
        .map(|file| {
            let meta = file.metadata()?;
            ensure!(
                meta.is_file() && meta.nlink() == 1,
                "archived source must remain a single-link regular file"
            );
            Ok(Stamp::of(&meta))
        })
        .transpose()
}
fn private_directory(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "recovery/archive directory must be owned, private and nonsymlink: {}",
        path.display()
    );
    Ok(())
}
fn directories(repo: &Path, deadline: std::time::Instant) -> Result<Vec<PathBuf>> {
    let gitdir = crate::git::path_gitdir(repo).context("cannot locate archive Git directory")?;
    let archive = gitdir.join("dracon-excluded-archive");
    match std::fs::symlink_metadata(&archive) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
        Ok(_) => private_directory(&archive)?,
    }
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(archive)? {
        ensure!(
            std::time::Instant::now() < deadline,
            "excluded archive inspection deadline expired"
        );
        let entry = entry?;
        if entry
            .file_name()
            .as_bytes()
            .starts_with(b"dracon-excluded-recovery-")
        {
            private_directory(&entry.path())?;
            paths.push(entry.path());
        }
    }
    Ok(paths)
}
/// Reversible metadata repair in the WORKER only, never the read-only report.
/// Older tempfile defaults left inner records 0755. Prove a private owned archive
/// container and owned nonsymlink child first; restrict directory permissions
/// without adopting content, touching canonical files or changing any seal.
pub(super) fn tighten_legacy(repo: &Path) -> Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let gitdir = crate::git::path_gitdir(repo).context("cannot locate archive Git directory")?;
    let root = gitdir.join("dracon-excluded-archive");
    match std::fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
        Ok(_) => private_directory(&root)?,
    }
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&root)?;
    let parent_meta = parent.metadata()?;
    ensure!(
        parent_meta.uid() == unsafe { libc::geteuid() } && parent_meta.mode() & 0o077 == 0,
        "archive container changed; no permission repair authorized"
    );
    for entry in std::fs::read_dir(&root)? {
        ensure!(
            std::time::Instant::now() < deadline,
            "excluded archive privacy repair deadline expired"
        );
        let entry = entry?;
        if !entry
            .file_name()
            .as_bytes()
            .starts_with(b"dracon-excluded-recovery-")
        {
            continue;
        }
        let name = CString::new(entry.file_name().as_bytes())?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let meta = file.metadata()?;
        ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() },
            "foreign archive directory; permissions/content retained"
        );
        if meta.mode() & 0o077 != 0 {
            file.set_permissions(std::fs::Permissions::from_mode(meta.mode() & !0o077))?;
            file.sync_all()?;
            eprintln!(
                "🛟 restricted owned archived recovery directory permissions: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

/// Missing seals from an older revision may be classified exactly once using
/// original recovery identities. A CHANGED sealed record is never resealed here.
pub(super) fn unsealed(repo: &Path) -> Result<Vec<PathBuf>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut paths = Vec::new();
    for directory in directories(repo, deadline)? {
        ensure!(
            std::time::Instant::now() < deadline,
            "excluded archive inspection deadline expired"
        );
        if Location::open(&directory, Path::new(SEAL))?
            .file()?
            .is_none()
        {
            paths.push(directory);
        }
    }
    Ok(paths)
}
/// No index/worktree writes, hashing or Git/filter subprocesses. ctime is part
/// of the seal: restoring mtime/length does not hide a later same-size write.
pub(super) fn pending(repo: &Path) -> Result<bool> {
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let canonical = std::fs::canonicalize(repo)?;
    for directory in directories(repo, deadline)? {
        ensure!(
            std::time::Instant::now() < deadline,
            "excluded archive inspection deadline expired"
        );
        let location = Location::open(&directory, Path::new(SEAL))?;
        let Some(file) = location.file()? else {
            return Ok(true);
        };
        let before = file.metadata()?;
        ensure!(
            before.is_file() && before.nlink() == 1 && before.len() <= MAX_SEAL_BYTES,
            "invalid or oversized excluded archive seal"
        );
        let original_stamp = Stamp::of(&before);
        let mut bytes = Vec::new();
        file.take(MAX_SEAL_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_SEAL_BYTES,
            "excluded archive seal grew past budget"
        );
        ensure!(
            stamp(&location)? == Some(original_stamp),
            "excluded archive seal changed during inspection"
        );
        let seal: Seal = serde_json::from_slice(&bytes)?;
        ensure!(
            seal.schema == 1 && seal.repo == canonical.as_os_str().as_bytes(),
            "excluded archive seal identity mismatch"
        );
        if stamp(&Location::open(&directory, Path::new("recovery.json"))?)? != Some(seal.request) {
            return Ok(true);
        }
        for target in seal.targets {
            ensure!(
                std::time::Instant::now() < deadline,
                "excluded archive inspection deadline expired"
            );
            let path = PathBuf::from(std::ffi::OsString::from_vec(target.path));
            if stamp(&Location::open(&directory.join("files"), &path)?)? != target.observed {
                return Ok(true);
            }
        }
    }
    ensure!(
        std::time::Instant::now() < deadline,
        "excluded archive inspection deadline expired"
    );
    Ok(false)
}

pub(super) fn persist(directory: &Path, seal: &Seal) -> Result<()> {
    use std::io::Write;
    private_directory(directory)?;
    if let Some(file) = Location::open(directory, Path::new(SEAL))?.file()? {
        use std::io::Read;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() == 1 && meta.len() <= MAX_SEAL_BYTES,
            "invalid existing archive seal; retained without replacement"
        );
        let mut bytes = Vec::new();
        file.take(MAX_SEAL_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_SEAL_BYTES
                && serde_json::from_slice::<Seal>(&bytes)? == *seal,
            "existing archive seal differs; never reseal uncertain bytes"
        );
        return Ok(());
    }
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(file.as_file_mut(), seal)?;
    file.flush()?;
    file.as_file().sync_all()?;
    // Never overwrite a previous observation to hide subsequently changed bytes.
    file.persist_noclobber(directory.join(SEAL))?;
    File::open(directory)?.sync_all()?;
    Ok(())
}
