//! Strict Git LFS v1 pointer encoding for immutable payload identities.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// SHA-256 payload identity and byte length. Encrypted payloads identify ciphertext.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    sha256: String,
    bytes: u64,
}

impl Fingerprint {
    /// Construct a validated lowercase hexadecimal SHA-256 identity.
    pub fn new(sha256: String, bytes: u64) -> Result<Self> {
        validate_sha256(&sha256)?;
        Ok(Self { sha256, bytes })
    }

    /// The SHA-256 identity; callers must not expose private source fingerprints.
    pub fn sha256(&self) -> &str { &self.sha256 }

    /// Exact payload length in bytes.
    pub fn bytes(&self) -> u64 { self.bytes }

    /// Revalidate a decoded identity before trusting persistent data.
    pub fn validate(&self) -> Result<()> { validate_sha256(&self.sha256) }
}

/// Validate an exact lowercase hexadecimal SHA-256 identity.
pub fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        bail!("invalid SHA-256 identity");
    }
    Ok(())
}

/// A standard Git LFS v1 pointer; this feature currently supports SHA-256 only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer(Fingerprint);

impl Pointer {
    /// Build a pointer from an immutable payload's validated identity.
    pub fn new(payload: Fingerprint) -> Result<Self> {
        payload.validate()?;
        Ok(Self(payload))
    }

    /// Immutable payload identity encoded by this pointer.
    pub fn payload(&self) -> &Fingerprint { &self.0 }

    /// Encode the canonical three-line representation with a final newline.
    pub fn encode(&self) -> Vec<u8> {
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n", self.0.sha256, self.0.bytes).into_bytes()
    }

    /// Parse a bounded canonical pointer, refusing unsupported versions/extensions.
    ///
    /// Failure never includes payload contents in its diagnostic. This deliberately
    /// does not reinterpret arbitrary data or silently accept unknown LFS extensions.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1024 { bail!("pointer exceeds size limit"); }
        let text = std::str::from_utf8(bytes).context("pointer is not UTF-8")?;
        let mut lines = text.split('\n');
        if lines.next() != Some("version https://git-lfs.github.com/spec/v1") {
            bail!("unsupported pointer version");
        }
        let oid = lines.next().and_then(|line| line.strip_prefix("oid sha256:")).context("unsupported pointer object identity")?;
        let size = lines.next().and_then(|line| line.strip_prefix("size ")).context("missing pointer length")?;
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) || (size.len() > 1 && size.starts_with('0')) {
            bail!("invalid pointer length");
        }
        if lines.next() != Some("") || lines.next().is_some() { bail!("noncanonical or extended pointer"); }
        Self::new(Fingerprint::new(oid.to_owned(), size.parse().context("pointer length overflow")?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_pointer_round_trips_zero_and_large_lengths() {
        for bytes in [0, 1, 101 * 1024 * 1024, u64::MAX] {
            let pointer = Pointer::new(Fingerprint::new("a".repeat(64), bytes).unwrap()).unwrap();
            assert_eq!(Pointer::parse(&pointer.encode()).unwrap(), pointer);
        }
    }

    #[test]
    fn unsupported_or_malformed_pointers_fail_closed() {
        let canonical = Pointer::new(Fingerprint::new("a".repeat(64), 20).unwrap()).unwrap().encode();
        let canonical = String::from_utf8(canonical).unwrap();
        for text in [canonical.replace("https:", "http:"), canonical.replace("sha256:", "sha1:"),
            canonical.replace("size 20", "size +20"), canonical.replace("size 20", "size 020"),
            canonical.replace("size 20", "size 18446744073709551616"), canonical.replace('\n', "\r\n"),
            canonical.trim_end().to_owned(), format!("{canonical}extra private content\n"),
            canonical.replace(&"a".repeat(64), &"A".repeat(64))] {
            assert!(Pointer::parse(text.as_bytes()).is_err());
        }
        assert!(Pointer::parse(&[0u8; 1025]).is_err());
        assert!(Pointer::parse(b"private token, not a pointer").unwrap_err().to_string().contains("version"));
    }
}
