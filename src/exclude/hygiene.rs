//! Shared, side-effect-free ignore planning. Canonical mutation/epoch authority
//! is deliberately separate: rendering a rule does not prove a safe install.
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
use std::path::Path;

const BEGIN: &str = "# --- BEGIN DRACON MANAGED BLOCK ---";
const END: &str = "# --- END DRACON MANAGED BLOCK ---";
const HEADER: &str = "# Large files (auto-added by dracon-sync)";

/// Generic positive PATTERNS, not filenames. Literal callers must encode names
/// first. Preserve all existing bytes; add inside a single valid Warden block
/// before its protected/plaintext negations, never after those security rules.
pub(crate) fn render_positive_additions(
    current: &str,
    patterns: &[String],
) -> Result<Option<String>> {
    let mut added = Vec::new();
    let mut seen = BTreeSet::new();
    for pattern in patterns {
        // Leading whitespace is a literal part of Git's pattern, not
        // formatting. Preserve it in BOTH requested and existing rules.
        let pattern = pattern.as_str();
        if pattern.is_empty() {
            continue;
        }
        ensure!(
            !pattern.contains(['\0', '\r', '\n']),
            "ignore rule contains a control/line separator"
        );
        ensure!(
            !pattern.starts_with(['!', '#']),
            "hygiene requires a positive rule, not a negation/comment"
        );
        if seen.insert(pattern.to_owned()) && !current.lines().any(|line| line == pattern) {
            added.push(pattern);
        }
    }
    if added.is_empty() {
        return Ok(None);
    }
    let mut begin = None;
    let mut end = None;
    let mut offset = 0;
    let mut lines = Vec::new();
    for line in current.split_inclusive('\n') {
        let text = line.trim();
        for (marker, slot) in [(BEGIN, &mut begin), (END, &mut end)] {
            if line.contains(marker) {
                ensure!(
                    text == marker && slot.is_none(),
                    "ambiguous or repeated managed ignore marker"
                );
                *slot = Some(offset);
            }
        }
        lines.push((offset, line));
        offset += line.len();
    }
    let (insert, section) = match (begin, end) {
        (None, None) => (current.len(), current),
        (Some(begin), Some(end)) if begin < end => {
            let insert = lines
                .iter()
                .find(|(offset, line)| {
                    *offset > begin && *offset < end && line.trim_start().starts_with('!')
                })
                .map_or(end, |(offset, _)| *offset);
            (insert, &current[begin..end])
        }
        _ => anyhow::bail!("malformed managed ignore block; preserve original content"),
    };
    let newline = if current.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut addition = String::new();
    if insert != 0 && !current[..insert].ends_with('\n') {
        addition.push_str(newline);
    }
    if !section.lines().any(|line| line.trim() == HEADER) {
        addition.push_str(HEADER);
        addition.push_str(newline);
    }
    for pattern in added {
        addition.push_str(pattern);
        addition.push_str(newline);
    }
    let mut result = String::with_capacity(current.len() + addition.len());
    result.push_str(&current[..insert]);
    result.push_str(&addition);
    result.push_str(&current[insert..]);
    Ok(Some(result))
}

fn escaped_name(text: &str) -> Result<String> {
    ensure!(
        !text.is_empty() && !text.contains(['\0', '\r', '\n']),
        "filename cannot be represented by an exact ignore line"
    );
    // Warden trims positive rules when preserving its block. An escaped final
    // whitespace byte would be changed by that consumer, so refuse visibly.
    ensure!(
        !text.chars().last().is_some_and(char::is_whitespace),
        "filename ends in whitespace unsupported by Warden rule preservation"
    );
    let mut encoded = String::new();
    let mut leading = true;
    for ch in text.chars() {
        // Warden trims its managed positive lines: escaping leading whitespace
        // keeps the literal name stable across the next hardening pass.
        if matches!(ch, '*' | '?' | '[' | ']' | '\\' | '!' | '#') || (leading && ch.is_whitespace())
        {
            encoded.push('\\');
        }
        encoded.push(ch);
        leading &= ch.is_whitespace();
    }
    Ok(encoded)
}

/// Exact repository-relative file, anchored so a same-named eligible file in a
/// different directory cannot be hidden by a large-file exception.
pub(crate) fn literal_file_pattern(path: &Path) -> Result<String> {
    ensure!(
        crate::git::is_safe_git_path(path),
        "unsafe large-file ignore path"
    );
    let text = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF8 filename unsupported by Warden text rules"))?;
    Ok(format!("/{}", escaped_name(text)?))
}

/// Intentional any-depth directory-component rule from the existing build
/// artifact policy; unlike file exceptions, its scope is explicitly a basename.
pub(crate) fn literal_directory_pattern(name: &str) -> Result<String> {
    ensure!(
        crate::git::is_safe_git_path(Path::new(name)) && !name.contains('/'),
        "invalid artifact directory component"
    );
    Ok(format!("{}/", escaped_name(name)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn native_git(repo: &Path) -> std::process::Command {
        let mut command = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    #[test]
    fn hygiene_render_preserves_outside_bytes_and_protected_negation_precedence() {
        let current=format!("operator header  \r\n{BEGIN}\r\n# managed by dracon-warden\r\ncache/\r\n!keep.log\r\n{END}\r\noperator footer");
        let rendered =
            render_positive_additions(&current, &["*.log".into(), "cache/".into(), "*.log".into()])
                .unwrap()
                .unwrap();
        assert!(rendered.starts_with("operator header  \r\n"));
        assert!(rendered.ends_with(&format!("{END}\r\noperator footer")));
        assert!(rendered.find("*.log\r\n").unwrap() < rendered.find("!keep.log\r\n").unwrap());
        assert_eq!(rendered.matches("*.log\r\n").count(), 1);
        assert!(render_positive_additions(&rendered, &["*.log".into()])
            .unwrap()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn hygiene_leading_whitespace_is_significant_and_literal_rules_survive_warden_trimming() {
        let rendered = render_positive_additions(" dist/\r\n", &["dist/".into()])
            .unwrap()
            .unwrap();
        assert!(rendered.starts_with(" dist/\r\n"));
        assert!(rendered.lines().any(|line| line == "dist/"));
        let requested = render_positive_additions("", &[" leading/".into()])
            .unwrap()
            .unwrap();
        assert!(requested.lines().any(|line| line == " leading/"));
        assert!(!requested.lines().any(|line| line == "leading/"));
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        assert!(native_git(r)
            .args(["init", "--template=", "-q"])
            .status()
            .unwrap()
            .success());
        for name in [" space", "\tspace", "\u{00a0}space"] {
            let pattern = literal_directory_pattern(name).unwrap();
            assert_eq!(
                pattern.trim(),
                pattern,
                "Warden preservation must not change a literal rule"
            );
            std::fs::write(r.join(".gitignore"), format!("{}\n", pattern.trim())).unwrap();
            std::fs::create_dir(r.join(name)).unwrap();
            let path = format!("{name}/probe");
            std::fs::write(r.join(&path), b"probe").unwrap();
            assert!(native_git(r)
                .args(["check-ignore", "--no-index", "--quiet", "--", &path])
                .status()
                .unwrap()
                .success());
            let plain = native_git(r)
                .args(["check-ignore", "--no-index", "--quiet", "--", "space/probe"])
                .status()
                .unwrap();
            assert_eq!(
                plain.code(),
                Some(1),
                "literal leading whitespace must not broaden to plain space/"
            );
        }
    }

    #[test]
    fn hygiene_render_rejects_ambiguous_blocks_and_multiline_rules_without_rewriting() {
        for source in [
            format!("{BEGIN}\ntail"),
            format!("{END}\n{BEGIN}"),
            format!("{BEGIN}\n{END}\n{BEGIN}\n{END}"),
            format!("prefix {BEGIN}\n{END}"),
        ] {
            assert!(render_positive_additions(&source, &["cache/".into()]).is_err());
        }
        for rule in [
            "one\ntwo",
            "one\rtwo",
            "!protected",
            "#comment",
            "bad\0rule",
        ] {
            assert!(render_positive_additions("operator data", &[rule.into()]).is_err());
        }
        assert!(
            render_positive_additions("operator data without newline", &[])
                .unwrap()
                .is_none()
        );
        let changed = render_positive_additions("operator data without newline", &["*.log".into()])
            .unwrap()
            .unwrap();
        assert!(changed.starts_with("operator data without newline\n"));
    }

    #[test]
    fn hygiene_literal_patterns_refuse_unrepresentable_names_not_lossy_wildcards() {
        for name in ["a\nb", "a\rb", "tail ", "tail\t", "../escape", "/absolute"] {
            assert!(literal_file_pattern(Path::new(name)).is_err(), "{name:?}");
        }
        assert!(literal_directory_pattern("a/b").is_err());
        assert_eq!(
            literal_file_pattern(Path::new("nested/!a[1]*?.bin")).unwrap(),
            "/nested/\\!a\\[1\\]\\*\\?.bin"
        );
        assert_eq!(literal_directory_pattern("out[1]").unwrap(), "out\\[1\\]/");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert!(
                literal_file_pattern(Path::new(std::ffi::OsStr::from_bytes(b"bad-\xff"))).is_err()
            );
        }
    }

    #[test]
    fn hygiene_legacy_wrapper_refuses_invalid_source_bytes_without_overwriting() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(".gitignore");
        for bytes in [
            b"operator-\xff".to_vec(),
            format!("{END}\n{BEGIN}\noperator tail").into_bytes(),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(super::super::append_to_gitignore(d.path(), &["cache/".into()]).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[cfg(unix)]
    #[test]
    fn hygiene_native_git_keeps_protection_and_does_not_ignore_glob_siblings() {
        let d = tempfile::tempdir().unwrap();
        let repo = d.path();
        let status = native_git(repo)
            .args(["init", "--template=", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        let rule = literal_file_pattern(Path::new("a[1]*?.bin")).unwrap();
        let current = format!("{BEGIN}\n!keep.log\n{END}\n");
        let mut rules = vec![rule, "*.log".into()];
        for name in ["#comment.bin", "!literal.bin", "a\\b.bin"] {
            rules.push(literal_file_pattern(Path::new(name)).unwrap());
        }
        let rendered = render_positive_additions(&current, &rules)
            .unwrap()
            .unwrap();
        std::fs::write(repo.join(".gitignore"), rendered).unwrap();
        let ignored = |path: &str| {
            std::fs::write(repo.join(path), b"fixture").unwrap();
            let output = native_git(repo)
                .args(["check-ignore", "--no-index", "--quiet", "--", path])
                .output()
                .unwrap();
            assert!(
                matches!(output.status.code(), Some(0 | 1)),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.status.success()
        };
        assert!(ignored("a[1]*?.bin"));
        assert!(!ignored("a1zz.bin"));
        assert!(ignored("ordinary.log"));
        assert!(!ignored("keep.log"));
        for name in ["#comment.bin", "!literal.bin", "a\\b.bin"] {
            assert!(ignored(name), "{name}");
        }
        std::fs::create_dir(repo.join("nested")).unwrap();
        assert!(!ignored("nested/a[1]*?.bin"));
    }
}
