//! Create-only S3 protocol driver. Credential resolution and signed HTTP transport
//! are separate operator-owned responsibilities; this module performs no discovery.

use anyhow::{bail, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use super::backend::{stream_digest, BackendFailure, ImmutableBackend};
use super::reference::Fingerprint;

/// Result of an atomic `PutObject` with a signed `If-None-Match: *` condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionalPut {
    /// The service accepted creation of a previously absent object.
    Created,
    /// The service refused creation because the key already exists.
    AlreadyExists,
}

/// Minimal S3 operations. Implementations must disable redirects, bound requests,
/// sign the conditional header, and redact credentials/provider response bodies.
/// No unconditional write, delete, list, or bucket mutation is exposed.
pub trait S3Transport {
    /// Send the exact seekable payload using create-only semantics. HTTP 412 maps
    /// to AlreadyExists; conflicts and all other failures remain errors.
    fn put_if_absent(&self, identity: &Fingerprint, source: File) -> Result<ConditionalPut>;
    /// Return the complete object body, not a range, cache hit or ETag assertion.
    /// The reader must retain transport deadlines until its final byte.
    fn get(&self, identity: &Fingerprint) -> Result<Box<dyn Read>>;
}

/// Streaming protocol driver. Construct only after resolving an operator grant
/// and selecting a transport that supports signed conditional writes.
pub struct S3Backend<T> {
    transport: T,
    max_object_bytes: u64,
}

impl<T: S3Transport> S3Backend<T> {
    /// Bind an approved transport to a positive per-object byte budget.
    pub fn new(transport: T, max_object_bytes: u64) -> Result<Self> {
        if max_object_bytes == 0 {
            bail!(BackendFailure::Capacity);
        }
        Ok(Self {
            transport,
            max_object_bytes,
        })
    }
}

impl<T: S3Transport> ImmutableBackend for S3Backend<T> {
    fn put(&self, input: &mut dyn Read) -> Result<Fingerprint> {
        // Anonymous private spool bounds memory and prevents mutable caller bytes
        // from changing between fingerprinting, signing and transmission.
        let mut spool = tempfile::tempfile()?;
        let identity = stream_digest(input, &mut spool, self.max_object_bytes)?;
        spool.sync_all()?;
        spool.seek(SeekFrom::Start(0))?;
        self.transport.put_if_absent(&identity, spool)?;
        // Both a new upload and 412 require full, independently hashed readback.
        // Never accept an ETag, a provider checksum, or an existence check as proof.
        self.get_verified(&identity, &mut std::io::sink())?;
        Ok(identity)
    }

    fn get_verified(&self, identity: &Fingerprint, output: &mut dyn Write) -> Result<()> {
        identity.validate()?;
        if identity.bytes() > self.max_object_bytes {
            bail!(BackendFailure::Capacity);
        }
        let mut body = self.transport.get(identity)?;
        // Bound even a dishonest provider to the requested object's size. Output
        // is provisional: callers publish only after successful verification.
        let actual = stream_digest(&mut body, output, identity.bytes())?;
        if actual != *identity {
            bail!(BackendFailure::Integrity);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct Provider {
        objects: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
        writes: Rc<Cell<usize>>,
        reads: Rc<Cell<usize>>,
        conflict: bool,
        corrupt_created: bool,
        read_failure: bool,
    }

    impl S3Transport for Provider {
        fn put_if_absent(&self, id: &Fingerprint, mut source: File) -> Result<ConditionalPut> {
            self.writes.set(self.writes.get() + 1);
            if self.conflict {
                bail!("retryable conditional write conflict");
            }
            let mut objects = self.objects.borrow_mut();
            if objects.contains_key(id.sha256()) {
                return Ok(ConditionalPut::AlreadyExists);
            }
            let mut data = Vec::new();
            source.read_to_end(&mut data)?;
            assert_eq!(data.len() as u64, id.bytes());
            if self.corrupt_created {
                data.fill(b'x');
            }
            objects.insert(id.sha256().into(), data);
            Ok(ConditionalPut::Created)
        }
        fn get(&self, id: &Fingerprint) -> Result<Box<dyn Read>> {
            self.reads.set(self.reads.get() + 1);
            if self.read_failure {
                bail!("provider unavailable");
            }
            let bytes = self
                .objects
                .borrow()
                .get(id.sha256())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("object missing"))?;
            Ok(Box::new(std::io::Cursor::new(bytes)))
        }
    }

    #[test]
    fn create_and_existing_both_require_complete_readback() {
        let provider = Provider::default();
        let backend = S3Backend::new(provider.clone(), 1024).unwrap();
        let id = backend.put(&mut &b"preserved"[..]).unwrap();
        assert_eq!(backend.put(&mut &b"preserved"[..]).unwrap(), id);
        assert_eq!(provider.writes.get(), 2);
        assert_eq!(provider.reads.get(), 2);
        let mut restored = Vec::new();
        backend.get_verified(&id, &mut restored).unwrap();
        assert_eq!(restored, b"preserved");
        assert_eq!(provider.objects.borrow().len(), 1);
    }

    #[test]
    fn existing_corrupt_object_is_never_overwritten_or_accepted() {
        let provider = Provider::default();
        let backend = S3Backend::new(provider.clone(), 1024).unwrap();
        let id = backend.put(&mut &b"original"[..]).unwrap();
        provider
            .objects
            .borrow_mut()
            .insert(id.sha256().into(), b"corrupt!".to_vec());
        assert!(backend.put(&mut &b"original"[..]).is_err());
        assert_eq!(provider.objects.borrow()[id.sha256()], b"corrupt!");
        assert_eq!(provider.reads.get(), 2);
    }

    #[test]
    fn failed_readback_and_write_conflicts_cannot_issue_receipts() {
        for provider in [
            Provider {
                conflict: true,
                ..Provider::default()
            },
            Provider {
                corrupt_created: true,
                ..Provider::default()
            },
            Provider {
                read_failure: true,
                ..Provider::default()
            },
        ] {
            let backend = S3Backend::new(provider.clone(), 100).unwrap();
            assert!(backend.put(&mut &b"original"[..]).is_err());
            assert_eq!(provider.writes.get(), 1);
            assert_eq!(provider.reads.get(), usize::from(!provider.conflict));
        }
    }

    #[test]
    fn bounded_capture_refuses_before_remote_write_and_retrieval_bounds_body() {
        let provider = Provider::default();
        let backend = S3Backend::new(provider.clone(), 8).unwrap();
        assert!(backend.put(&mut &b"too many bytes"[..]).is_err());
        assert_eq!(provider.writes.get(), 0);
        let id = backend.put(&mut &b"original"[..]).unwrap();
        for bad in [b"short".to_vec(), b"too many bytes".to_vec()] {
            provider
                .objects
                .borrow_mut()
                .insert(id.sha256().into(), bad);
            assert!(backend.get_verified(&id, &mut Vec::new()).is_err());
        }
        let oversized = Fingerprint::new(id.sha256().into(), 9).unwrap();
        let prior = provider.reads.get();
        assert!(backend.get_verified(&oversized, &mut Vec::new()).is_err());
        assert_eq!(provider.reads.get(), prior);
    }

    #[test]
    fn streaming_payload_above_git_limit_uses_bounded_reads() {
        struct Source {
            remaining: u64,
            largest: usize,
        }
        impl Read for Source {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.largest = self.largest.max(buf.len());
                let count = self.remaining.min(buf.len() as u64) as usize;
                buf[..count].fill(0x7a);
                self.remaining -= count as u64;
                Ok(count)
            }
        }
        // Transport echoes a private seekable object rather than retaining a
        // payload-sized Vec, so this test covers the driver's streaming boundary.
        #[derive(Default)]
        struct StreamingProvider {
            object: RefCell<Option<File>>,
        }
        impl S3Transport for StreamingProvider {
            fn put_if_absent(&self, _: &Fingerprint, source: File) -> Result<ConditionalPut> {
                *self.object.borrow_mut() = Some(source);
                Ok(ConditionalPut::Created)
            }
            fn get(&self, _: &Fingerprint) -> Result<Box<dyn Read>> {
                let mut file = self.object.borrow().as_ref().unwrap().try_clone()?;
                file.seek(SeekFrom::Start(0))?;
                Ok(Box::new(file))
            }
        }
        let bytes = 101 * 1024 * 1024;
        let backend = S3Backend::new(StreamingProvider::default(), bytes).unwrap();
        let mut source = Source {
            remaining: bytes,
            largest: 0,
        };
        let id = backend.put(&mut source).unwrap();
        assert_eq!(id.bytes(), bytes);
        assert_eq!(source.largest, 64 * 1024);
        backend.get_verified(&id, &mut std::io::sink()).unwrap();
    }

    #[test]
    fn interrupted_provider_body_and_destination_errors_fail_verification() {
        struct BrokenReader;
        impl Read for BrokenReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("transport failed"))
            }
        }
        struct InterruptedTransport;
        impl S3Transport for InterruptedTransport {
            fn put_if_absent(&self, _: &Fingerprint, _: File) -> Result<ConditionalPut> {
                Ok(ConditionalPut::Created)
            }
            fn get(&self, _: &Fingerprint) -> Result<Box<dyn Read>> {
                Ok(Box::new(BrokenReader))
            }
        }
        let backend = S3Backend::new(InterruptedTransport, 1024).unwrap();
        assert!(backend.put(&mut &b"original"[..]).is_err());
        struct BrokenOutput;
        impl Write for BrokenOutput {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("destination full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let backend = S3Backend::new(Provider::default(), 1024).unwrap();
        let id = backend.put(&mut &b"original"[..]).unwrap();
        assert!(backend.get_verified(&id, &mut BrokenOutput).is_err());
    }

    #[test]
    fn interrupted_source_cannot_contact_provider() {
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("source unavailable"))
            }
        }
        let provider = Provider::default();
        let backend = S3Backend::new(provider.clone(), 1024).unwrap();
        assert!(backend.put(&mut Failed).is_err());
        assert_eq!(provider.writes.get(), 0);
        assert_eq!(provider.reads.get(), 0);
    }
}
