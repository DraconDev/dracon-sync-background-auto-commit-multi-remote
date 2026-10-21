//! Git configuration — SSH hardening and git binary path resolution.

use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

/// Returns the SSH command with hardened security options for git operations.
/// Disables SSH_ASKPASS from NixOS/system environment to prevent GUI password prompts
/// in daemon context where they would block or fail silently.
pub(crate) fn git_ssh_hardening() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let ssh_config = format!("{home}/.dracon/secrets/ssh/config");
    ssh_hardening_command(&ssh_config, Path::new(&ssh_config).exists())
}

/// The hardened SSH command line.
///
/// The operator's SSH config is wired in with `-F` only when the file is
/// actually there. Pointing ssh at a file that does not exist makes ssh
/// refuse to start at all ("Can't open user config file ..."), so a machine
/// without the fleet's config store turned every push into a configuration
/// error and hid the real cause behind it. With no file there is nothing to
/// point at, and the rest of the hardening still applies.
pub(crate) fn ssh_hardening_command(ssh_config: &str, config_exists: bool) -> String {
    // Unset SSH_ASKPASS so NixOS's kshaskpass doesn't interfere with git SSH auth
    let config_flag = if config_exists {
        format!("-F {} ", shell_quote(ssh_config))
    } else {
        String::new()
    };
    format!(
        "env -u SSH_ASKPASS ssh -o BatchMode=yes {config_flag}-o ConnectTimeout=10 -o ConnectionAttempts=1 -o ServerAliveInterval=5 -o ServerAliveCountMax=2",
    )
}

/// Quote one value for the POSIX shell used to interpret GIT_SSH_COMMAND.
/// HOME is operator-controlled and may contain spaces or shell metacharacters;
/// leaving the SSH config path unquoted makes those paths unusable and can
/// turn a configuration value into shell syntax.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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

#[cfg(test)]
mod tests {
    use super::{shell_quote, ssh_hardening_command};

    #[test]
    fn shell_quote_handles_spaces_and_single_quotes() {
        assert_eq!(
            shell_quote("/tmp/operator's ssh config"),
            "'/tmp/operator'\"'\"'s ssh config'"
        );
    }

    #[test]
    fn ssh_config_is_wired_in_only_when_the_file_exists() {
        let with = ssh_hardening_command("/home/ops/.dracon/secrets/ssh/config", true);
        assert!(
            with.contains("-F '/home/ops/.dracon/secrets/ssh/config'"),
            "an existing config must be used, got: {with}"
        );
        let without = ssh_hardening_command("/home/ops/.dracon/secrets/ssh/config", false);
        assert!(
            !without.contains("-F"),
            "a missing config must not be passed to ssh, got: {without}"
        );
        for cmd in [&with, &without] {
            assert!(cmd.contains("BatchMode=yes"), "got: {cmd}");
            assert!(cmd.contains("ConnectTimeout=10"), "got: {cmd}");
            assert!(cmd.contains("env -u SSH_ASKPASS"), "got: {cmd}");
        }
    }
}
