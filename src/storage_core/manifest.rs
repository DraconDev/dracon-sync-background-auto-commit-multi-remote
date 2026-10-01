//! Bounded restore metadata codec, before its required security transformation.
//!
//! Encoded bytes are private plaintext. They must never be staged, logged or
//! published directly. Production metadata protection and atomic Git staging
//! remain separate gates. Decoding is not authorization to contact a backend.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::journal::{identifier, validate_path_hex, Encryption};
use super::reference::{validate_sha256, Fingerprint, Pointer};

/// Maximum decoded metadata size, independent of large asset payload limits.
pub const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
/// Maximum sticky path enrollments, including deleted-path tombstones.
pub const MAX_ENROLLMENTS: usize = 10_000;
const VERSION: u32 = 1;

/// A path's sticky external-placement contract and current immutable payload.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    /// Lossless relative path; validated with the same constraints as jobs.
    pub path_hex: String,
    /// Digest of the approved enrollment contract, never a source fingerprint.
    pub contract_sha256: String,
    /// Portable operator-bound backend identifier, never a URL or local path.
    pub primary: String,
    /// Sorted unique identifiers of all required copies, including the primary.
    pub required_copies: Vec<String>,
    /// Required representation security, which cannot silently change on an edit.
    pub encryption: Encryption,
    /// Current payload identity; `None` is a deleted but still enrolled path.
    pub payload: Option<Fingerprint>,
}

impl Enrollment {
    fn validate(&self) -> Result<()> {
        validate_path_hex(&self.path_hex)?;
        validate_sha256(&self.contract_sha256)?;
        identifier(&self.primary)?;
        if self.required_copies.is_empty() || self.required_copies.len() > 64 {
            bail!("invalid manifest copy set");
        }
        let mut previous: Option<&str> = None;
        for id in &self.required_copies {
            identifier(id)?;
            if previous.is_some_and(|prior| prior >= id.as_str()) {
                bail!("manifest copy identifiers must be sorted and unique");
            }
            previous = Some(id);
        }
        if !self.required_copies.contains(&self.primary) {
            bail!("manifest primary must be a required copy");
        }
        if let Some(payload) = &self.payload {
            payload.validate()?;
        }
        Ok(())
    }

    /// Check the exact path version against a Git pointer, without backend I/O.
    pub fn matches_pointer(&self, pointer: &Pointer) -> bool {
        self.payload.as_ref() == Some(pointer.payload())
    }

    /// Compare sticky placement requirements independently of current bytes.
    /// A false result requires explicit enrollment migration, not a normal edit.
    pub fn same_contract(&self, other: &Self) -> bool {
        self.path_hex == other.path_hex
            && self.contract_sha256 == other.contract_sha256
            && self.primary == other.primary
            && self.required_copies == other.required_copies
            && self.encryption == other.encryption
    }
}

/// Private decoded restore metadata. Every version currently retains all objects.
///
/// No source digests, credentials, endpoints, executable names or recipient/key
/// material are present. Age headers and separately backed-up historical keys
/// carry the security adapter's recovery requirements.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u32,
    repo_id: String,
    retention: Retention,
    enrollments: Vec<Enrollment>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Retention {
    PreserveAll,
}

impl Manifest {
    /// Construct a validated, deterministic record; inputs must already be sorted.
    pub fn new(repo_id: String, enrollments: Vec<Enrollment>) -> Result<Self> {
        let result = Self {
            version: VERSION,
            repo_id,
            retention: Retention::PreserveAll,
            enrollments,
        };
        result.validate()?;
        Ok(result)
    }

    /// Operator-bound repository identity. The caller must verify this binding.
    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    /// Sorted sticky enrollments, including tombstones for deleted paths.
    pub fn enrollments(&self) -> &[Enrollment] {
        &self.enrollments
    }

    /// Look up a losslessly encoded path without interpreting it as a local path.
    pub fn enrollment(&self, path_hex: &str) -> Option<&Enrollment> {
        self.enrollments
            .binary_search_by(|entry| entry.path_hex.as_str().cmp(path_hex))
            .ok()
            .map(|index| &self.enrollments[index])
    }

    fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            bail!("unsupported restore manifest version");
        }
        validate_sha256(&self.repo_id)?;
        if self.enrollments.len() > MAX_ENROLLMENTS {
            bail!("restore manifest enrollment limit exceeded");
        }
        let mut previous: Option<&str> = None;
        for entry in &self.enrollments {
            entry.validate()?;
            if previous.is_some_and(|prior| prior >= entry.path_hex.as_str()) {
                bail!("manifest paths must be sorted and unique");
            }
            previous = Some(&entry.path_hex);
        }
        Ok(())
    }

    /// Encode private plaintext for the approved metadata-security adapter only.
    /// Repeated encodings of unchanged metadata produce identical bytes.
    pub fn encode_private(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_MANIFEST_BYTES {
            bail!("restore manifest byte limit exceeded");
        }
        Ok(bytes)
    }

    /// Decode bounded private plaintext after authenticated decryption.
    /// Diagnostics deliberately omit serde's potentially secret-bearing values.
    pub fn parse_private(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            bail!("restore manifest byte limit exceeded");
        }
        let result: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid restore manifest encoding"))?;
        result.validate()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_core::journal::encode_relative_path;

    fn entry(path: &[u8]) -> Enrollment {
        Enrollment {
            path_hex: encode_relative_path(path).unwrap(),
            contract_sha256: "b".repeat(64),
            primary: "primary".into(),
            required_copies: vec!["primary".into(), "recovery".into()],
            encryption: Encryption::WardenAge,
            payload: Some(Fingerprint::new("c".repeat(64), 101 * 1024 * 1024).unwrap()),
        }
    }

    #[test]
    fn private_metadata_round_trip_matches_exact_pointer_and_lossless_path() {
        let enrolled = entry(b"assets/odd-\xff file.mp4");
        let manifest = Manifest::new("a".repeat(64), vec![enrolled.clone()]).unwrap();
        let bytes = manifest.encode_private().unwrap();
        let decoded = Manifest::parse_private(&bytes).unwrap();
        assert!(decoded == manifest);
        assert_eq!(decoded.encode_private().unwrap(), bytes);
        let restored = decoded.enrollment(&enrolled.path_hex).unwrap();
        assert!(restored.matches_pointer(&Pointer::new(enrolled.payload.unwrap()).unwrap()));
        assert!(!restored.matches_pointer(
            &Pointer::new(Fingerprint::new("d".repeat(64), 101 * 1024 * 1024).unwrap()).unwrap()
        ));
        assert!(!restored.matches_pointer(
            &Pointer::new(Fingerprint::new("c".repeat(64), 1).unwrap()).unwrap()
        ));
        assert_eq!(decoded.repo_id(), "a".repeat(64));
        assert!(decoded.enrollment("not-enrolled").is_none());
    }

    #[test]
    fn tombstones_keep_contract_while_payload_edits_do_not_change_enrollment() {
        let original = entry(b"asset.mp4");
        let mut edited = original.clone();
        edited.payload = Some(Fingerprint::new("d".repeat(64), 0).unwrap());
        assert!(original.same_contract(&edited));
        edited.payload = None;
        assert!(original.same_contract(&edited));
        let pointer = Pointer::new(original.payload.clone().unwrap()).unwrap();
        assert!(!edited.matches_pointer(&pointer));
        let manifest = Manifest::new("a".repeat(64), vec![edited.clone()]).unwrap();
        assert!(Manifest::parse_private(&manifest.encode_private().unwrap()).unwrap() == manifest);
        edited.encryption = Encryption::None;
        assert!(!original.same_contract(&edited));
        edited = original.clone();
        edited.required_copies.pop();
        assert!(!original.same_contract(&edited));
        edited = original.clone();
        edited.contract_sha256 = "d".repeat(64);
        assert!(!original.same_contract(&edited));
    }

    #[test]
    fn malformed_untrusted_manifest_fails_without_echoing_private_fields() {
        let bytes = Manifest::new("a".repeat(64), vec![entry(b"asset.mp4")])
            .unwrap()
            .encode_private()
            .unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for bad in [
            text.replace("\"version\":1", "\"version\":2"),
            text.replace("\"version\":1", "\"version\":1,\"version\":1"),
            text.replace("preserve-all", "expire"),
            text.replace("warden-age", "PRIVATE-SECRET"),
            text.replace("\"primary\":\"primary\"", "\"primary\":\"https://PRIVATE-SECRET\""),
            text.replace("\"version\":1", "\"version\":1,\"token\":\"PRIVATE-SECRET\""),
            text.replace(&"c".repeat(64), &"C".repeat(64)),
            text.replace(&encode_relative_path(b"asset.mp4").unwrap(), "2e2e2f736563726574"),
        ] {
            let error = Manifest::parse_private(bad.as_bytes()).err().unwrap();
            assert!(!format!("{error:#}").contains("PRIVATE-SECRET"));
        }
        assert!(Manifest::parse_private(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
        assert!(Manifest::parse_private(b"{\"repo_id\": \"PRIVATE-SECRET\"").is_err());
    }

    #[test]
    fn ambiguous_paths_and_copy_sets_are_rejected() {
        let a = entry(b"a.mp4");
        let b = entry(b"b.mp4");
        assert!(Manifest::new("a".repeat(64), vec![a.clone(), b.clone()]).is_ok());
        assert!(Manifest::new("a".repeat(64), vec![b, a.clone()]).is_err());
        assert!(Manifest::new("a".repeat(64), vec![a.clone(), a.clone()]).is_err());
        assert!(Manifest::new("a".repeat(64), vec![a.clone(); MAX_ENROLLMENTS + 1]).is_err());
        for copies in [vec![], vec!["recovery".into()], vec!["primary".into(), "primary".into()], vec!["recovery".into(), "primary".into()]] {
            let mut malformed = a.clone();
            malformed.required_copies = copies;
            assert!(Manifest::new("a".repeat(64), vec![malformed]).is_err());
        }
    }
}
