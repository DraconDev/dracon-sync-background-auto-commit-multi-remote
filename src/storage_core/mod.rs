//! Portable payload identity and durable exact-version job records.
//!
//! This layer does not resolve repository policy or backend credentials.
//! Operator-approved adapters own external I/O and security processing.
//! The caller must enforce ownership/security policy and verify evidence before
//! recording successful capture, transfer, Git commit, or push operations.

pub mod backend;
pub mod bindings;
pub mod clean;
pub mod index;
#[cfg(target_os = "linux")]
pub mod hydration;
pub mod journal;
pub mod manifest;
pub mod metadata;
pub mod reference;
pub mod restore;
pub mod security;
pub mod staging;
pub mod transfer;
