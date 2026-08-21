//! URL helpers — extract origin URLs, strip credentials, convert between SSH and HTTPS.

use std::path::Path;

/// Get the origin remote URL.
pub(crate) fn origin_url(repo: &Path) -> Option<String> {
    let out = crate::policy::std_git_command()
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

/// Strip userinfo credentials from an HTTPS URL.
pub(crate) fn strip_url_credentials(url: &str) -> String {
    if let Some(stripped) = url.strip_prefix("https://") {
        if let Some(at_pos) = stripped.find('@') {
            return format!("https://{}", &stripped[at_pos + 1..]);
        }
    }
    url.to_string()
}

/// Convert a GitHub SSH or HTTPS URL to HTTPS format.
pub(crate) fn github_https_url(origin: &str) -> Option<String> {
    if let Some(rest) = origin.strip_prefix("git@github.com:") {
        return Some(format!("https://github.com/{}", rest));
    }
    if let Some(rest) = origin.strip_prefix("ssh://git@github.com/") {
        return Some(format!("https://github.com/{}", rest));
    }
    if origin.starts_with("https://github.com/") {
        return Some(strip_url_credentials(origin));
    }
    None
}

/// Convert a GitLab SSH or HTTPS URL to HTTPS format.
pub(crate) fn gitlab_https_url(origin: &str) -> Option<String> {
    if let Some(rest) = origin.strip_prefix("git@gitlab.com:") {
        return Some(format!("https://gitlab.com/{}", rest));
    }
    if let Some(rest) = origin.strip_prefix("ssh://git@gitlab.com/") {
        return Some(format!("https://gitlab.com/{}", rest));
    }
    if origin.starts_with("https://gitlab.com/") {
        return Some(strip_url_credentials(origin));
    }
    None
}

/// Convert a Codeberg SSH or HTTPS URL to HTTPS format.
pub(crate) fn codeberg_https_url(origin: &str) -> Option<String> {
    if let Some(rest) = origin.strip_prefix("git@codeberg.org:") {
        return Some(format!("https://codeberg.org/{}", rest));
    }
    if let Some(rest) = origin.strip_prefix("ssh://git@codeberg.org/") {
        return Some(format!("https://codeberg.org/{}", rest));
    }
    if origin.starts_with("https://codeberg.org/") {
        return Some(strip_url_credentials(origin));
    }
    None
}

/// Return a transport-neutral repository identity for a Git remote URL.
///
/// Git permits the same repository to be written as scp-style SSH,
/// `ssh://`, or HTTPS, with optional credentials, a trailing slash, and a
/// `.git` suffix. Remote *host* checks alone are not sufficient: an
/// `origin` pointing at `github.com/DraconDev/ultratap` must not suppress a
/// distinct `github` mirror pointing at `github.com/DraconDev/doomtap`.
///
/// The result intentionally contains the host and normalized path, but not
/// the transport scheme, so SSH and HTTPS forms compare equal. This helper is
/// for bookkeeping decisions, not URL fetching or authorization.
///
/// Known accepted limitations (documented 2026-08-21, audit M1): alias
/// spellings that differ textually are NOT unified — `ssh.github.com:443`
/// / port-2222 endpoints, `www.github.com`, and non-default ports compare
/// distinct from their canonical hosts. Consequence beyond mirror-dedup:
/// such an origin also misses GitHub-specific classification downstream.
/// Unifying these would need per-forge alias tables; deferred as LOW.
pub(crate) fn canonical_repository_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let (scheme, authority, path) = if raw.contains("://") {
        let (scheme, rest) = raw.split_once("://")?;
        let (authority, path) = rest.split_once('/')?;
        (scheme, authority, path)
    } else {
        // scp-style syntax: [user@]host:path. A leading bracketed host is
        // taken literally up to `]` so IPv6 colons never participate in the
        // split (audit M1: `git@[2001:db8::1]:org/repo.git` previously split
        // at the first inner colon and produced garbage). Any `user@`
        // prefix is stripped, not just the literal `git@` (audit M1:
        // `deploy@github.com:org/repo.git` previously fell through and
        // returned None, silently dropping GitHub classification and pack-
        // guard behavior for that remote).
        let (authority, path) = if raw.starts_with('[') {
            let close = raw.find(']')?;
            let rest = &raw[close + 1..];
            (&raw[1..close], rest.strip_prefix(':')?)
        } else {
            let colon = raw.find(':')?;
            let user_and_host = &raw[..colon];
            (
                user_and_host
                    .rsplit_once('@')
                    .map_or(user_and_host, |(_, host)| host),
                &raw[colon + 1..],
            )
        };
        ("ssh", authority, path)
    };

    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let authority = authority.trim().trim_end_matches('/');
    if authority.is_empty() {
        return None;
    }

    let host = authority
        .trim_end_matches(if scheme.eq_ignore_ascii_case("ssh") {
            ":22"
        } else if scheme.eq_ignore_ascii_case("https") {
            ":443"
        } else if scheme.eq_ignore_ascii_case("http") {
            ":80"
        } else {
            "\0"
        })
        .to_ascii_lowercase();
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if host.is_empty() || path.is_empty() {
        return None;
    }

    let mut path = path.join("/").to_ascii_lowercase();
    while path.ends_with(".git") {
        path.truncate(path.len() - 4);
    }
    if path.is_empty() {
        return None;
    }

    Some(format!("{host}/{path}"))
}

/// Compare two Git remote URLs by repository identity, ignoring transport
/// syntax, credentials, a trailing slash, and the conventional `.git`
/// suffix.
pub(crate) fn same_repository_url(left: &str, right: &str) -> bool {
    matches!(
        (canonical_repository_url(left), canonical_repository_url(right)),
        (Some(left), Some(right)) if left == right
    )
}

#[cfg(test)]
mod tests {
    use super::{canonical_repository_url, same_repository_url};

    #[test]
    fn canonical_repository_url_normalizes_transport_and_suffix() {
        let ssh = "git@github.com:DraconDev/fleetmaster.git";
        let https = "https://token:secret@GITHUB.com/DraconDev/fleetmaster/";
        assert_eq!(
            canonical_repository_url(ssh),
            Some("github.com/dracondev/fleetmaster".to_string())
        );
        assert!(same_repository_url(ssh, https));
    }

    #[test]
    fn canonical_repository_url_keeps_distinct_repositories_distinct() {
        assert!(!same_repository_url(
            "git@github.com:DraconDev/ultratap.git",
            "git@github.com:DraconDev/doomtap.git"
        ));
        assert!(!same_repository_url(
            "git@gitlab.com:DraconDev/fleetmaster.git",
            "git@github.com:DraconDev/fleetmaster.git"
        ));
    }

    #[test]
    fn canonical_repository_url_parses_scp_style_ipv6_literal() {
        // Audit M1: the scp branch previously split at the first inner
        // colon, producing host "[2001" and path "db8::1]:org/repo".
        let canonical = canonical_repository_url("git@[2001:db8::1]:org/repo.git");
        assert_eq!(canonical, Some("[2001:db8::1]/org/repo".to_string()));
        // The ssh:// URL form of the same host must dedup with the scp form.
        assert!(same_repository_url(
            "git@[2001:db8::1]:org/repo.git",
            "ssh://git@[2001:db8::1]/org/repo.git"
        ));
        // Bracketed host without the mandatory trailing colon is invalid.
        assert_eq!(canonical_repository_url("git@[2001:db8::1]/org/repo.git"), None);
    }

    #[test]
    fn canonical_repository_url_accepts_any_scp_username() {
        // Audit M1: only the literal "git@" prefix was recognized; other
        // usernames (deploy keys, uppercase spellings) fell through to None,
        // silently disabling mirror-dedup and GitHub classification.
        for user in ["deploy", "GIT", "obama"] {
            let url = format!("{user}@github.com:org/repo.git");
            assert_eq!(
                canonical_repository_url(&url),
                Some("github.com/org/repo".to_string()),
                "scp username {user:?} must parse"
            );
            assert!(same_repository_url(&url, "https://github.com/org/repo.git"));
        }
        // User-less scp form still works.
        assert_eq!(
            canonical_repository_url("github.com:org/repo.git"),
            Some("github.com/org/repo".to_string())
        );
    }
}
