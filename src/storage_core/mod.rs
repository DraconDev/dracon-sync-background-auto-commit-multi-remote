//! Portable payload identity and durable exact-version job records.
//!
//! This layer contains no repository policy, backend credentials, or network I/O.
//! The caller must enforce ownership/security policy and verify evidence before
//! recording successful capture, transfer, Git commit, or push operations.

pub mod backend;
pub mod journal;
pub mod reference;
pub mod security;
pub mod transfer;
