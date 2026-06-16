//! Git configuration — SSH hardening and git binary path resolution.

#[cfg(test)]
use std::path::PathBuf;

/// Returns the SSH command with hardened security options for git operations.
/// Disables SSH_ASKPASS from NixOS/system environment to prevent GUI password prompts
/// in daemon context where they would block or fail silently.
pub(crate) fn git_ssh_hardening() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    // Unset SSH_ASKPASS so NixOS's ksshaskpass doesn't interfere with git SSH auth
    format!(
        "env -u SSH_ASKPASS ssh -o BatchMode=yes -F {home}/.dracon/secrets/ssh/config -o ConnectTimeout=10 -o ConnectionAttempts=1 -o ServerAliveInterval=5 -o ServerAliveCountMax=2"
    )
}

/// Lock for serializing tests that modify PATH.
/// Get the real git binary path, checking env var override first.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn real_git_path() -> PathBuf {
    if let Ok(custom) = std::env::var("DRACON_SYNC_GIT_BIN") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    static REAL_GIT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REAL_GIT
        .get_or_init(|| {
            for candidate in ["/run/current-system/sw/bin/git", "/usr/bin/git", "/bin/git"] {
                let path = PathBuf::from(candidate);
                if path.exists() {
                    return path;
                }
            }
            PathBuf::from("git")
        })
        .clone()
}
