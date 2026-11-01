//! Source identity/content snapshot. Hashing runs off the async executor; the
//! enclosing preparation or check task owns the actual reclaimer fence.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    device: u64, inode: u64, size: u64, mode: u32, uid: u32, links: u64,
    modified: i64, modified_ns: i64, changed: i64, changed_ns: i64,
}
fn state(metadata: &Metadata) -> Result<State> {
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        Ok(State { device: metadata.dev(), inode: metadata.ino(), size: metadata.len(),
            mode: metadata.mode(), uid: metadata.uid(), links: metadata.nlink(),
            modified: metadata.mtime(), modified_ns: metadata.mtime_nsec(),
            changed: metadata.ctime(), changed_ns: metadata.ctime_nsec() })
    }
    #[cfg(not(unix))] { let _ = metadata; anyhow::bail!("source identity requires Unix") }
}
fn name(bytes: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    let path = { use std::os::unix::ffi::OsStringExt; PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())) };
    #[cfg(not(unix))]
    let path = PathBuf::from(String::from_utf8(bytes.to_vec())?);
    let mut parts = path.components();
    ensure!(matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none(), "invalid source filename");
    Ok(path)
}
fn hash(path: &Path) -> Result<(State, String)> {
    let mut file = super::regular(path)?;
    let before = state(&file.metadata()?)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop { let n = file.read(&mut buffer)?; if n == 0 { break; } digest.update(&buffer[..n]); }
    ensure!(state(&file.metadata()?)? == before && state(&fs::symlink_metadata(path)?)? == before,
        "source file changed while hashing");
    Ok((before, format!("{:x}", digest.finalize())))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry { name: Vec<u8>, state: State, sha256: String }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot { version: u8, directory: State, files: Vec<Entry> }
impl Snapshot {
    fn take(spool: &Path) -> Result<Self> {
        let directory = fs::symlink_metadata(spool)?;
        ensure!(directory.is_dir(), "source must be a real directory");
        let directory = state(&directory)?;
        let mut files = Vec::new();
        for entry in fs::read_dir(spool)? {
            let entry = entry?;
            ensure!(files.len() < 4096 && entry.file_type()?.is_file(), "unsupported source layout or entry count");
            let (state, sha256) = hash(&entry.path())?;
            files.push(Entry { name: entry.file_name().as_encoded_bytes().to_vec(), state, sha256 });
        }
        files.sort_by(|a,b| a.name.cmp(&b.name));
        ensure!(state(&fs::symlink_metadata(spool)?)? == directory, "source directory changed during snapshot");
        Ok(Self { version: 1, directory, files })
    }
    fn copies(&self, metadata: &Path, packs: &Path) -> Result<()> {
        for entry in &self.files {
            let name = name(&entry.name)?;
            let base = if name.extension().is_some_and(|e| e == "pack") { packs } else { metadata };
            let (actual, digest) = hash(&base.join(name))?;
            ensure!(digest == entry.sha256 && actual.size == entry.state.size && actual.mode == entry.state.mode,
                "archived source bytes or permissions differ from source snapshot");
        }
        Ok(())
    }
    fn metadata_unchanged(&self, spool: &Path) -> Result<()> {
        ensure!(self.version == 1 && state(&fs::symlink_metadata(spool)?)? == self.directory, "source directory changed");
        let mut names = fs::read_dir(spool)?.map(|entry| Ok(entry?.file_name().as_encoded_bytes().to_vec()))
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        ensure!(names == self.files.iter().map(|e| e.name.clone()).collect::<Vec<_>>(), "source entries changed");
        for entry in &self.files {
            let path = spool.join(name(&entry.name)?);
            ensure!(fs::symlink_metadata(&path)?.is_file() && state(&fs::symlink_metadata(&path)?)? == entry.state,
                "source file changed after verification");
        }
        Ok(())
    }
}
fn recorded(record: &Path) -> Result<Snapshot> {
    let mut bytes = Vec::new();
    super::regular(&record.join("source-fingerprint.json"))?.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16 * 1024 * 1024, "oversized source fingerprint");
    let snapshot: Snapshot = serde_json::from_slice(&bytes)?;
    ensure!(snapshot.version == 1 && snapshot.files.len() <= 4096, "unsupported source fingerprint");
    Ok(snapshot)
}
pub(super) async fn capture(spool: &Path) -> Result<Snapshot> {
    let spool = spool.to_owned();
    tokio::task::spawn_blocking(move || Snapshot::take(&spool)).await?
}
pub(super) async fn verify_copies(snapshot: &Snapshot, metadata: &Path, packs: &Path) -> Result<()> {
    let (snapshot, metadata, packs) = (snapshot.clone(), metadata.to_owned(), packs.to_owned());
    tokio::task::spawn_blocking(move || snapshot.copies(&metadata, &packs)).await?
}
pub(super) async fn verify_owned<G: Send + 'static>(record: &Path, spool: &Path, guard: G) -> Result<G> {
    let (record, spool) = (record.to_owned(), spool.to_owned());
    tokio::task::spawn_blocking(move || {
        let expected = recorded(&record)?;
        ensure!(Snapshot::take(&spool)? == expected, "source content/identity changed since preparation");
        Ok(guard)
    }).await.context("source verification task failed")?
}
/// The full hash was checked above while holding the same kernel fence. Perform
/// this cheap final no-await check immediately before the removal decision.
pub(super) fn final_check(record: &Path, spool: &Path) -> Result<()> {
    recorded(record)?.metadata_unchanged(spool)
}
