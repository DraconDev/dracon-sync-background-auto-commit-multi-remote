//! Cooldown management for the sync daemon.
//!
//! NOTE: The daemon currently uses raw `HashMap<PathBuf, Instant>` for cooldowns
//! (see daemon.rs). This module previously contained a `CooldownManager` struct
//! that was never adopted. Kept as a placeholder for future consolidation.

// CooldownManager was removed — it was dead code. The daemon uses inline
// HashMaps for repair_cooldowns, filter_cooldowns, and remote_notify_cooldowns.
