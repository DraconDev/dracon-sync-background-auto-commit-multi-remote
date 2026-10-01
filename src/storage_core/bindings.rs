//! Operator-resolved copy permissions, never deserialized from repository data.

use anyhow::{bail, Result};
use std::collections::BTreeMap;

use super::backend::ImmutableBackend;
use super::journal::{identifier, Encryption, JobSpec};
use super::reference::validate_sha256;

/// An operator-selected adapter and explicitly approved representation classes.
/// Construction is an authorization boundary for the trusted caller, not a
/// classifier or proof of provider ownership/independent durability.
pub struct ApprovedBackend<'a> {
    backend: &'a dyn ImmutableBackend,
    allowed: Vec<Encryption>,
}

impl<'a> ApprovedBackend<'a> {
    /// Approve encrypted payloads only; this does not allow plaintext fallback.
    pub fn encrypted(backend: &'a dyn ImmutableBackend) -> Self {
        Self {
            backend,
            allowed: vec![Encryption::WardenAge],
        }
    }

    /// Apply an explicit operator allowlist, including non-sensitive bytes only
    /// when that class has actually been approved. No repository rule can grant it.
    pub fn for_security(
        backend: &'a dyn ImmutableBackend,
        allowed: Vec<Encryption>,
    ) -> Result<Self> {
        if allowed.is_empty()
            || allowed.len() > 2
            || (allowed.len() == 2 && allowed[0] == allowed[1])
        {
            bail!("invalid approved backend security classes");
        }
        Ok(Self { backend, allowed })
    }
}

/// Exact copy set resolved for an operator-bound repository. Repository manifests
/// select approved identifiers; they cannot create these adapters/permissions.
pub struct CopyBindings<'a> {
    repo_id: String,
    backends: BTreeMap<String, ApprovedBackend<'a>>,
}

impl<'a> CopyBindings<'a> {
    /// Bind operator-authorized adapters to the stable owning repository identity.
    /// The caller must establish the trusted ID-to-checkout/config mapping.
    pub fn new(
        repo_id: String,
        backends: BTreeMap<String, ApprovedBackend<'a>>,
    ) -> Result<Self> {
        validate_sha256(&repo_id)?;
        if backends.len() > 64 {
            bail!("approved copy binding limit exceeded");
        }
        for id in backends.keys() {
            identifier(id)?;
        }
        Ok(Self { repo_id, backends })
    }

    pub(crate) fn validate_job(&self, spec: &JobSpec) -> Result<()> {
        spec.validate()?;
        if self.repo_id != spec.repo_id
            || self.backends.len() != spec.required_copies.len()
            || spec.required_copies.iter().any(|id| {
                self.backends
                    .get(id)
                    .is_none_or(|binding| !binding.allowed.contains(&spec.encryption))
            })
        {
            bail!("approved copy bindings do not match repository, copy set or security");
        }
        Ok(())
    }

    pub(crate) fn backend(&self, id: &str) -> &dyn ImmutableBackend {
        self.backends[id].backend
    }
}
