use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Secret files with these names are tried before other `.env` files when the
/// environment variable is not set. This makes PAT selection deterministic and
/// lets an operator choose the intended token by filename (for example
/// `github.env` for `GH_TOKEN`) instead of relying on filesystem order.
const PREFERRED_SECRET_FILE_NAMES: &[&str] = &[
    "github.env",
    "gh.env",
    "gitlab.env",
    "glab.env",
    "codeberg.env",
];

/// Load a secret value from an environment variable or `.env` files.
///
/// Strategy:
/// 1. Check the env var `env_name` directly — if set and non-empty, return it.
/// 2. Scan all `*.env` files in the given `secrets_dir`, parse `KEY=VALUE` lines,
///    and return the matching value.
///
/// Security: if the secrets directory is world-writable, secrets are refused
/// to prevent malicious injection by other users.
///
/// The secrets directory:
/// - `~/.dracon/utilities/sync/secrets` — general sync secrets (git.rs)
pub(crate) fn load_secret(env_name: &str, secrets_dir: &Path) -> Option<String> {
    // 1. Check env var directly
    if let Ok(val) = std::env::var(env_name) {
        if !val.is_empty() {
            // F52 (2026-07-18): refuse env values containing control
            // characters (including `\n`), which can break git
            // credential protocols or smuggle commands. Git PATs are
            // alphanumeric; anything with control bytes is malformed.
            if val.chars().any(|c| c.is_control()) {
                eprintln!(
                    "⚠️ {env_name} contains control characters; refusing and falling back to secrets dir"
                );
                return load_secret_from_dir(env_name, secrets_dir);
            }
            return Some(val);
        }
    }
    load_secret_from_dir(env_name, secrets_dir)
}

fn load_secret_from_dir(env_name: &str, secrets_dir: &Path) -> Option<String> {
    // 2. Permission check on secrets directory
    if let Err(e) = check_secrets_dir_permissions(secrets_dir) {
        eprintln!(
            "⚠️ secrets directory permission check failed for {}: {}",
            secrets_dir.display(),
            e
        );
        return None;
    }

    // 3. Scan .env files in deterministic order: preferred names first, then
    // lexicographic order. This avoids silently depending on filesystem order
    // when multiple secret files define the same key.
    if let Ok(entries) = std::fs::read_dir(secrets_dir) {
        let mut secret_paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "env"))
            .collect();

        secret_paths.sort_by_key(|path| {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let preferred = preferred_secret_file_index(env_name, &file_name);
            (preferred, file_name)
        });

        for path in secret_paths {
            #[cfg(unix)]
            warn_if_world_readable(&path);
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        if key.trim() == env_name {
                            let value = value.trim();
                            if !value.is_empty() {
                                return Some(value.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn preferred_secret_file_index(env_name: &str, file_name: &str) -> usize {
    if let Some(index) = PREFERRED_SECRET_FILE_NAMES
        .iter()
        .position(|preferred| *preferred == file_name)
    {
        return index;
    }

    let normalized = env_name.to_ascii_lowercase();
    if format!("{normalized}.env") == file_name {
        return PREFERRED_SECRET_FILE_NAMES.len();
    }

    if let Some(stem) = normalized.strip_suffix("_token") {
        if format!("{stem}.env") == file_name {
            return PREFERRED_SECRET_FILE_NAMES.len() + 1;
        }
    }

    usize::MAX
}

/// Verify that the secrets directory is not world-writable.
/// A world-writable secrets directory allows any user to inject malicious
/// credential files, which could lead to credential theft or repo hijacking.
#[cfg(unix)]
fn check_secrets_dir_permissions(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        // Directory doesn't exist yet — not a security issue
        return Ok(());
    }
    let metadata = std::fs::metadata(dir).map_err(|e| format!("cannot read metadata: {}", e))?;
    let mode = metadata.permissions().mode();
    if mode & 0o002 != 0 {
        return Err(format!(
            "directory is world-writable (mode {:o}). Refusing to load secrets.",
            mode & 0o7777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_secrets_dir_permissions(_dir: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn warn_if_world_readable(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        let mode = metadata.permissions().mode();
        if mode & 0o044 != 0 {
            eprintln!(
                "⚠️ secret file {} is world-readable (mode {:o}). Consider chmod 600.",
                path.display(),
                mode & 0o7777
            );
        }
    }
}

/// Returns the default sync secrets directory: `~/.dracon/utilities/sync/secrets`.
pub(crate) fn sync_secrets_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dracon/utilities/sync/secrets")
}

/// Returns the legacy PAT directory used by git credential helpers:
/// `~/.dracon/secrets/pat`.
pub(crate) fn legacy_pat_secrets_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dracon/secrets/pat")
}
