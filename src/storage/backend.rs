//! Local immutable-backend prototype. Deliberately not enabled in the daemon.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const BUFFER_BYTES: usize = 64 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectIdentity {
    sha256: String,
    bytes: u64,
}

trait ImmutableBackend {
    fn put(&self, input: &mut dyn Read) -> Result<ObjectIdentity>;
    fn get_verified(&self, identity: &ObjectIdentity, output: &mut dyn Write) -> Result<()>;
}

struct LocalBackend {
    root: PathBuf,
    max_object_bytes: u64,
}

struct TemporaryObject {
    path: PathBuf,
    file: File,
}

impl Drop for TemporaryObject {
    fn drop(&mut self) {
        // A transfer spool is not a recorded payload version. Published links remain.
        let _ = std::fs::remove_file(&self.path);
    }
}

impl LocalBackend {
    fn open(root: &Path, max_object_bytes: u64) -> Result<Self> {
        if !root.is_absolute() || max_object_bytes == 0 {
            bail!("absolute root and positive object budget required");
        }
        std::fs::create_dir_all(root)?;
        let metadata = std::fs::symlink_metadata(root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("backend root must be a real directory");
        }
        Ok(Self {
            root: root.canonicalize()?,
            max_object_bytes,
        })
    }

    fn object_path(&self, identity: &ObjectIdentity) -> Result<PathBuf> {
        if identity.sha256.len() != 64
            || !identity
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("invalid SHA-256 object identity");
        }
        if identity.bytes > self.max_object_bytes {
            bail!("object exceeds configured backend budget");
        }
        Ok(self.root.join(&identity.sha256))
    }

    fn spool(&self) -> Result<TemporaryObject> {
        for _ in 0..32 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let path = self
                .root
                .join(format!(".upload-{}-{time}-{sequence}", std::process::id()));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            match options.open(&path) {
                Ok(file) => return Ok(TemporaryObject { path, file }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).context("cannot create transfer spool"),
            }
        }
        bail!("cannot allocate unique transfer spool");
    }
}

fn stream_digest(
    input: &mut dyn Read,
    output: &mut dyn Write,
    max_bytes: u64,
) -> Result<ObjectIdentity> {
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; BUFFER_BYTES];
    loop {
        let read = match input.read(&mut buffer) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .context("object length overflow")?;
        if bytes > max_bytes {
            bail!("object exceeds configured backend budget");
        }
        digest.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
    }
    Ok(ObjectIdentity {
        sha256: format!("{:x}", digest.finalize()),
        bytes,
    })
}

impl ImmutableBackend for LocalBackend {
    fn put(&self, input: &mut dyn Read) -> Result<ObjectIdentity> {
        let mut spool = self.spool()?;
        let identity = stream_digest(input, &mut spool.file, self.max_object_bytes)?;
        spool.file.sync_all()?;
        let destination = self.object_path(&identity)?;
        // Unlike rename(), link() atomically refuses to replace an existing object.
        match std::fs::hard_link(&spool.path, &destination) {
            Ok(()) => {
                File::open(&self.root)?.sync_all()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).context("immutable publication failed"),
        }
        // Readback proves both an existing object's identity and the freshly written bytes.
        self.get_verified(&identity, &mut std::io::sink())?;
        Ok(identity)
    }

    fn get_verified(&self, identity: &ObjectIdentity, output: &mut dyn Write) -> Result<()> {
        let path = self.object_path(identity)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut input = options
            .open(path)
            .context("object missing or inaccessible")?;
        if !input.metadata()?.is_file() {
            bail!("object is not a regular file");
        }
        let actual = stream_digest(&mut input, output, self.max_object_bytes)?;
        if actual != *identity {
            bail!("object failed length/digest verification");
        }
        // Callers must publish their destination only after success; this is a streamed API.
        Ok(())
    }
}

#[test]
fn immutable_round_trip_deduplicates_and_detects_corruption() {
    let root = tempfile::tempdir().unwrap();
    let store = LocalBackend::open(root.path(), 1024).unwrap();
    let source = b"exact-version";
    let identity = store.put(&mut &source[..]).unwrap();
    assert_eq!(store.put(&mut &source[..]).unwrap(), identity);
    let mut restored = Vec::new();
    store.get_verified(&identity, &mut restored).unwrap();
    assert_eq!(restored, source);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    std::fs::write(store.object_path(&identity).unwrap(), b"tampered").unwrap();
    assert!(store.get_verified(&identity, &mut Vec::new()).is_err());
    assert!(store.put(&mut &source[..]).is_err());
    assert_eq!(
        std::fs::read(store.object_path(&identity).unwrap()).unwrap(),
        b"tampered"
    );
}

#[test]
fn streamed_large_payload_has_bounded_read_requests_and_survives_cold_reopen() {
    struct Synthetic {
        remaining: u64,
        largest: usize,
    }
    impl Read for Synthetic {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.largest = self.largest.max(buf.len());
            let count = self.remaining.min(buf.len() as u64) as usize;
            buf[..count].fill(0x7a);
            self.remaining -= count as u64;
            Ok(count)
        }
    }
    let root = tempfile::tempdir().unwrap();
    let bytes = 101 * 1024 * 1024;
    let store = LocalBackend::open(root.path(), bytes).unwrap();
    let mut source = Synthetic {
        remaining: bytes,
        largest: 0,
    };
    let identity = store.put(&mut source).unwrap();
    assert_eq!(identity.bytes, bytes);
    assert_eq!(source.largest, BUFFER_BYTES);
    drop(store);
    let cold = LocalBackend::open(root.path(), bytes).unwrap();
    cold.get_verified(&identity, &mut std::io::sink()).unwrap();
}

#[test]
fn failed_capture_does_not_publish_partial_object() {
    struct Interrupted;
    impl Read for Interrupted {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("capture failed"))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let store = LocalBackend::open(root.path(), 20).unwrap();
    assert!(store.put(&mut Interrupted).is_err());
    assert!(store.put(&mut &[0u8; 21][..]).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn malicious_object_symlink_cannot_escape_backend() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let store = LocalBackend::open(root.path(), 1024).unwrap();
    let bytes = b"external";
    let identity = stream_digest(&mut &bytes[..], &mut std::io::sink(), 1024).unwrap();
    let other = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(other.path(), bytes).unwrap();
    symlink(other.path(), store.object_path(&identity).unwrap()).unwrap();
    assert!(store.get_verified(&identity, &mut Vec::new()).is_err());
    assert!(store.put(&mut &bytes[..]).is_err());
    assert_eq!(std::fs::read(other.path()).unwrap(), bytes);
}
