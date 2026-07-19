pub(crate) fn extract_version_from_cargo(content: &str) -> Option<String> {
    let mut section = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches(&['[', ']'][..]).trim().to_string();
        }
        if section == "package" || section == "workspace.package" {
            if let Some(rest) = trimmed.strip_prefix("version") {
                let rest = rest.trim_start().trim_start_matches('=').trim();
                // F43 (2026-07-18): allow a trailing `;` (legal TOML
                // for `version = "1.0.0";`). Strip the trailing `;`
                // before the closing `"` check.
                let rest = rest.strip_suffix(';').unwrap_or(rest);
                if let Some(v) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

pub(crate) fn extract_version_from_json(content: &str, key: &str) -> Option<String> {
    // F51 (2026-07-18): replaced the manual byte-search with a
    // serde_json parse so values containing escaped quotes (e.g.
    // `{"version": "1.0.0\"hotfix"}`) are handled correctly. The
    // previous implementation matched the first `"` after `q1`,
    // which could be the `\"` escape and produce garbage.
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    v.get(key).and_then(|val| val.as_str()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version_from_cargo_package() {
        let content = r#"[package]
name = "test"
version = "1.2.3""#;
        assert_eq!(
            extract_version_from_cargo(content),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_cargo_with_trailing_semicolon() {
        // F43 (2026-07-18): legal TOML `version = "1.2.3";` (trailing
        // semicolon) is valid syntax; the previous parser silently
        // returned None.
        let content = "[package]\nname = \"test\"\nversion = \"1.2.3\";\n";
        assert_eq!(
            extract_version_from_cargo(content),
            Some("1.2.3".to_string())
        );

        // Same for the workspace.package form.
        let content2 = "[workspace.package]\nversion = \"0.9.0\";\n";
        assert_eq!(
            extract_version_from_cargo(content2),
            Some("0.9.0".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_cargo_workspace_package() {
        let content = r#"[workspace.package]
version = "2.0.0"

[package]
name = "test""#;
        assert_eq!(
            extract_version_from_cargo(content),
            Some("2.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_cargo_no_version() {
        let content = r#"[package]
name = "test""#;
        assert_eq!(extract_version_from_cargo(content), None);
    }

    #[test]
    fn test_extract_version_from_cargo_ignorefile() {
        let content = r#"[workspace]
members = ["crate1", "crate2"]

[package]
name = "test"
version = "1.0.0""#;
        assert_eq!(
            extract_version_from_cargo(content),
            Some("1.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_json() {
        let content = r#"{"version": "1.2.3"}"#;
        assert_eq!(
            extract_version_from_json(content, "version"),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn test_extract_version_from_json_not_found() {
        let content = r#"{"name": "test"}"#;
        assert_eq!(extract_version_from_json(content, "version"), None);
    }

    #[test]
    fn test_extract_version_from_json_escaped_quotes() {
        // F51 (2026-07-18): a value containing an escaped quote must
        // be returned verbatim, not truncated at the first `\"`.
        let content = r#"{"version": "1.0.0\"hotfix"}"#;
        assert_eq!(
            extract_version_from_json(content, "version"),
            Some(r#"1.0.0"hotfix"#.to_string())
        );
    }

    #[test]
    fn test_extract_version_from_json_multiple_keys() {
        let content = r#"{"name": "test", "version": "1.0.0", "other": "value"}"#;
        assert_eq!(
            extract_version_from_json(content, "version"),
            Some("1.0.0".to_string())
        );
    }
}
