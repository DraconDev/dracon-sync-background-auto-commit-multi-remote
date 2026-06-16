/// Returns true if the error indicates the remote repo already exists
/// (GitHub, GitLab, and Codeberg all use slightly different messages).
pub(crate) fn is_repo_already_exists(stderr: &str) -> bool {
    stderr.contains("Name already exists")
        || stderr.contains("already exists")
        || stderr.contains("has already been taken")
}
