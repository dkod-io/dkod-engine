use std::path::Path;

use anyhow::{Context, Result};

/// Discover a git repository by walking up from the given directory.
pub fn discover_repo(path: &Path) -> Result<gix::Repository> {
    gix::discover(path).context("not a git repository (or any parent up to mount point)")
}

/// Read the remote URL for "origin" from the git repository at the current directory.
/// Uses `gix` for pure-Rust git config access (no `git` binary required).
pub fn remote_origin_url() -> Result<String> {
    let repo = gix::discover(".").context("not inside a git repository")?;
    let remote = repo
        .find_remote("origin")
        .context("no 'origin' remote configured")?;
    let url = remote
        .url(gix::remote::Direction::Fetch)
        .context("origin remote has no URL")?;
    Ok(url.to_bstring().to_string())
}

/// Extract `owner/repo` from a git remote URL.
///
/// Supported formats:
/// - SSH: `git@github.com:owner/repo.git` → `owner/repo`
/// - SSH explicit: `ssh://git@github.com/owner/repo.git` → `owner/repo`
/// - HTTPS: `https://github.com/owner/repo.git` → `owner/repo`
///
/// Returns `None` for unrecognised formats (local paths, bare names, etc.).
pub fn repo_name_from_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git").trim_end_matches('/');

    // SSH shorthand: git@github.com:owner/repo
    if let Some(path) = url.strip_prefix("git@") {
        let path = path.split_once(':')?.1;
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some(format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]));
        }
    }

    // URL with scheme (https://, ssh://, git://): require "://" to avoid matching
    // local file paths or other non-URL strings.
    if url.contains("://") {
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() >= 2 {
            return Some(format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_github() {
        assert_eq!(
            repo_name_from_remote("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn https_no_dot_git() {
        assert_eq!(
            repo_name_from_remote("https://github.com/owner/repo"),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn https_trailing_slash() {
        assert_eq!(
            repo_name_from_remote("https://github.com/owner/repo/"),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn ssh_shorthand() {
        assert_eq!(
            repo_name_from_remote("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn ssh_shorthand_no_dot_git() {
        assert_eq!(
            repo_name_from_remote("git@github.com:owner/repo"),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn ssh_explicit_scheme() {
        assert_eq!(
            repo_name_from_remote("ssh://git@github.com/owner/repo.git"),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn gitlab_nested_group_ssh() {
        // For nested groups, returns the last two path segments (subgroup/repo).
        // This is the expected behaviour — the platform resolves by owner/repo.
        assert_eq!(
            repo_name_from_remote("git@gitlab.com:org/subgroup/repo.git"),
            Some("subgroup/repo".to_string()),
        );
    }

    #[test]
    fn gitlab_nested_group_https() {
        assert_eq!(
            repo_name_from_remote("https://gitlab.com/org/subgroup/repo.git"),
            Some("subgroup/repo".to_string()),
        );
    }

    #[test]
    fn rejects_bare_slash_string() {
        // "foo/bar" has no scheme — must return None.
        assert_eq!(repo_name_from_remote("foo/bar"), None);
    }

    #[test]
    fn rejects_local_file_path() {
        assert_eq!(repo_name_from_remote("/home/user"), None);
    }

    #[test]
    fn rejects_empty_string() {
        assert_eq!(repo_name_from_remote(""), None);
    }

    #[test]
    fn rejects_single_word() {
        assert_eq!(repo_name_from_remote("repo"), None);
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(
            repo_name_from_remote("  https://github.com/owner/repo.git  "),
            Some("owner/repo".to_string()),
        );
    }

    #[test]
    fn git_protocol_scheme() {
        assert_eq!(
            repo_name_from_remote("git://github.com/owner/repo.git"),
            Some("owner/repo".to_string()),
        );
    }
}
