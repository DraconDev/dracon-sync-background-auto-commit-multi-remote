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

/// Supported `.env` line dialect (DOCUMENTED 2026-10-03, audit
/// R4-SR-15 — previously only `KEY=value-verbatim` worked and the
/// rest failed closed into auth errors):
/// - `KEY=value` — value trimmed of surrounding whitespace.
/// - `export KEY=value` — a leading `export` + whitespace is stripped.
/// - `KEY="quoted"` / `KEY='quoted'` — one layer of MATCHING quotes
///   is stripped; the inside is kept verbatim (no comment stripping,
///   no escape processing). Mismatched quotes stay verbatim.
/// - `KEY=value # comment` — an UNQUOTED value is cut at the first
///   `#` preceded by a space/tab, then re-trimmed. A `#` with no
///   preceding whitespace (`KEY=abc#def`) stays verbatim: it may be
///   token material, and guessing wrong breaks auth either way.
/// - Blank lines and `#`-leading lines are skipped; values with
///   control characters are refused (F52/M27).
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
                    let mut line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    // R4-SR-15: strip a leading `export` + whitespace
                    // (`export KEY=val`). The whitespace requirement
                    // keeps `exportKEY=val` from collapsing to KEY.
                    if let Some(rest) = line.strip_prefix("export") {
                        if rest.starts_with([' ', '\t']) {
                            line = rest.trim_start();
                        }
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        if key.trim() == env_name {
                            let value = strip_env_value(value.trim());
                            if !value.is_empty() {
                                // ADDED 2026-07-21 (v0.112.33, audit
                                // M27/F3.10): apply the same F52
                                // control-character refusal as the
                                // env-var path. A mid-line `\r`
                                // survives `str::lines()` (only a
                                // TRAILING `\r` is stripped), so
                                // `GH_TOKEN=abc\rX-Injected: yes`
                                // produces a token containing `\r`,
                                // which is then interpolated into an
                                // HTTP header block passed to
                                // `curl -H @-` (header injection).
                                if value.chars().any(|c| c.is_control()) {
                                    eprintln!(
                                        "⚠️ {} in {} contains control characters; refusing",
                                        env_name,
                                        path.display()
                                    );
                                    continue;
                                }
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

/// Strip one layer of `.env` value decoration (R4-SR-15; see the
/// dialect doc on `load_secret_from_dir`): matching surrounding
/// quotes win over comment stripping (a `#` inside quotes is
/// literal); otherwise cut at the first whitespace-preceded `#`.
fn strip_env_value(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    let mut start = 0;
    while let Some(hash) = value[start..].find('#') {
        let idx = start + hash;
        if idx > 0 && (bytes[idx - 1] == b' ' || bytes[idx - 1] == b'\t') {
            return value[..idx].trim_end().to_string();
        }
        start = idx + 1;
    }
    value.to_string()
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
    // F60 (2026-07-19): the previous check was world-writable (0o002)
    // only. Group-writable (0o020) on a secrets directory lets any
    // user in the daemon's group inject/override secrets. Refuse
    // both world- AND group-writable (the daemon typically runs as
    // a single user; group access adds risk without value).
    if mode & 0o022 != 0 {
        return Err(format!(
            "directory is group- or world-writable (mode {:o}). Refusing to load secrets. \
             Run: chmod go-w {}",
            mode & 0o7777,
            dir.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_secrets_dir_permissions(_dir: &Path) -> Result<(), String> {
    Ok(())
}

/// Name which readability bits are set (R4-SR-15): the old warning
/// fired on group-OR-other (0o044) but always said "world-readable".
/// Returns `None` when neither bit is set.
#[cfg(unix)]
fn readable_scope(mode: u32) -> Option<&'static str> {
    match (mode & 0o040 != 0, mode & 0o004 != 0) {
        (true, true) => Some("group- and world-readable"),
        (true, false) => Some("group-readable"),
        (false, true) => Some("world-readable"),
        (false, false) => None,
    }
}

#[cfg(unix)]
fn warn_if_world_readable(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        let mode = metadata.permissions().mode();
        if let Some(scope) = readable_scope(mode) {
            eprintln!(
                "⚠️ secret file {} is {scope} (mode {:o}). Consider chmod 600.",
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

#[cfg(test)]
mod tests {
    /// ADDED 2026-07-21 (v0.112.33, audit M27/F3.10): a `.env`-file
    /// secret containing a mid-line control character (e.g. `\r`)
    /// must be REFUSED — it would otherwise be interpolated into a
    /// curl HTTP header block (header injection). The F52 guard
    /// previously covered only the env-var path.
    #[test]
    fn test_load_secret_from_dir_refuses_control_chars() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path();
        std::fs::write(
            secrets_dir.join("github.env"),
            "CONTROL_TEST_TOKEN=abc\rX-Injected: yes\n",
        )
        .unwrap();
        std::fs::write(
            secrets_dir.join("other.env"),
            "CONTROL_TEST_TOKEN=clean-token-123\n",
        )
        .unwrap();
        // The poisoned file sorts FIRST lexicographically for this
        // key (github.env < other.env), so without the refusal the
        // poisoned value would be returned.
        let result = super::load_secret_from_dir("CONTROL_TEST_TOKEN", secrets_dir);
        assert_eq!(
            result.as_deref(),
            Some("clean-token-123"),
            "control-char value must be refused; the clean file's value should win"
        );
    }

    /// ADDED 2026-07-21 (v0.112.33, audit M27/F3.10): clean values
    /// still load.
    #[test]
    fn test_load_secret_from_dir_loads_clean_value() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path();
        std::fs::write(
            secrets_dir.join("github.env"),
            "# comment\nCONTROL_TEST_TOKEN2=clean-token-456\n",
        )
        .unwrap();
        let result = super::load_secret_from_dir("CONTROL_TEST_TOKEN2", secrets_dir);
        assert_eq!(result.as_deref(), Some("clean-token-456"));
    }

    /// ADDED 2026-10-03 (audit R4-SR-15): the documented `.env`
    /// value dialect — matching quotes, unquoted trailing comments,
    /// and the verbatim fallbacks.
    #[test]
    fn test_strip_env_value_dialect() {
        use super::strip_env_value;
        assert_eq!(strip_env_value("abc"), "abc");
        assert_eq!(strip_env_value("\"abc\""), "abc");
        assert_eq!(strip_env_value("'abc'"), "abc");
        // Quotes win over comment stripping: `#` inside is literal.
        assert_eq!(strip_env_value("\"abc # x\""), "abc # x");
        // Unquoted trailing comments (space or tab) are cut.
        assert_eq!(strip_env_value("abc # x"), "abc");
        assert_eq!(strip_env_value("abc\t# x"), "abc");
        // `#` with no preceding whitespace stays verbatim: it may
        // be token material.
        assert_eq!(strip_env_value("abc#x"), "abc#x");
        // Mismatched quotes stay verbatim (fail closed).
        assert_eq!(strip_env_value("\"abc'"), "\"abc'");
        assert_eq!(strip_env_value("\""), "\"");
        // Empty quotes yield empty (caller skips empties).
        assert_eq!(strip_env_value("\"\""), "");
    }

    /// ADDED 2026-10-03 (audit R4-SR-15): `export` prefix and the
    /// value dialect end to end through the file loader.
    #[test]
    fn test_load_secret_from_dir_dialect_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path();
        std::fs::write(
            secrets_dir.join("dialect.env"),
            "export DIALECT_TOKEN_1=tok-1\n\
             DIALECT_TOKEN_2=\"tok-2\" # trailing\n\
             export DIALECT_TOKEN_3='tok-3'\n\
             DIALECT_TOKEN_4=tok-4 # rotated 2026\n\
             exportKEY=DIALECT_TOKEN_5-must-not-match\n",
        )
        .unwrap();
        assert_eq!(
            super::load_secret_from_dir("DIALECT_TOKEN_1", secrets_dir).as_deref(),
            Some("tok-1")
        );
        // Quoted: comment is literal inside quotes... except the
        // closing quote ends before the comment here.
        assert_eq!(
            super::load_secret_from_dir("DIALECT_TOKEN_2", secrets_dir).as_deref(),
            Some("\"tok-2\" # trailing")
        );
        assert_eq!(
            super::load_secret_from_dir("DIALECT_TOKEN_3", secrets_dir).as_deref(),
            Some("tok-3")
        );
        assert_eq!(
            super::load_secret_from_dir("DIALECT_TOKEN_4", secrets_dir).as_deref(),
            Some("tok-4")
        );
        // `exportKEY=...` must not collapse to KEY.
        assert_eq!(
            super::load_secret_from_dir("KEY", secrets_dir).as_deref(),
            None
        );
    }

    /// ADDED 2026-10-03 (audit R4-SR-15): the readability warning
    /// must name group vs other instead of always "world-readable".
    #[cfg(unix)]
    #[test]
    fn test_readable_scope_names_group_vs_other() {
        use super::readable_scope;
        assert_eq!(readable_scope(0o100600), None);
        assert_eq!(readable_scope(0o100604), Some("world-readable"));
        assert_eq!(readable_scope(0o100640), Some("group-readable"));
        assert_eq!(
            readable_scope(0o100644),
            Some("group- and world-readable")
        );
    }
}
