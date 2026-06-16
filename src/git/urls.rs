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
