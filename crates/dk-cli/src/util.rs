use std::path::Path;

use anyhow::{Context, Result};
use gix::bstr::ByteSlice;

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
    Ok(url
        .to_bstring()
        .to_str()
        .context("origin remote URL is not valid UTF-8")?
        .to_string())
}

/// Extract `owner/repo` from a git remote URL.
///
/// Supported formats:
/// - SSH: `git@github.com:owner/repo.git` → `owner/repo`
/// - SSH explicit: `ssh://git@github.com/owner/repo.git` → `owner/repo`
/// - HTTPS: `https://github.com/owner/repo.git` → `owner/repo`
///
/// Returns `None` for unrecognised formats (local paths, bare names, `file://`, etc.).
pub fn repo_name_from_remote(url: &str) -> Option<String> {
    // Strip trailing slash first so `.git` suffix is always visible to the next step.
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");

    // SSH shorthand: git@github.com:owner/repo
    // Use nested `if let` instead of `?` so that a missing colon falls through
    // to the scheme-based branch rather than returning None from the function.
    if let Some(path) = url.strip_prefix("git@") {
        if let Some((_, after_colon)) = path.split_once(':') {
            let parts: Vec<&str> = after_colon.split('/').collect();
            if parts.len() >= 2 {
                let owner = parts[parts.len() - 2];
                let repo = parts[parts.len() - 1];
                if !owner.is_empty() && !repo.is_empty() {
                    return Some(format!("{owner}/{repo}"));
                }
            }
        }
    }

    // URL with a well-known remote scheme. Only accept protocols that indicate
    // a remote host; reject file:// and other local/exotic schemes.
    let is_remote_scheme = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("ssh://")
        || url.starts_with("git://");

    if is_remote_scheme {
        // A valid URL splits as: ["scheme:", "", "host", ..., "owner", "repo"]
        // so we need at least 5 parts to have both owner and repo segments.
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() >= 5 {
            let owner = parts[parts.len() - 2];
            let repo = parts[parts.len() - 1];
            if !owner.is_empty() && !repo.is_empty() {
                return Some(format!("{owner}/{repo}"));
            }
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
    fn https_dot_git_trailing_slash() {
        // ".git/" — trailing slash after .git suffix must still strip correctly.
        assert_eq!(
            repo_name_from_remote("https://github.com/owner/repo.git/"),
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

    #[test]
    fn rejects_https_host_only() {
        assert_eq!(repo_name_from_remote("https://github.com"), None);
    }

    #[test]
    fn rejects_https_single_path_segment() {
        assert_eq!(repo_name_from_remote("https://github.com/owner"), None);
    }

    #[test]
    fn ssh_malformed_no_colon_falls_through() {
        assert_eq!(repo_name_from_remote("git@github.com/owner/repo"), None);
    }

    #[test]
    fn rejects_file_protocol() {
        // file:// is a local path, not a remote — must return None.
        assert_eq!(
            repo_name_from_remote("file:///home/user/repos/myrepo.git"),
            None,
        );
    }

    #[test]
    fn http_protocol() {
        assert_eq!(
            repo_name_from_remote("http://github.com/owner/repo.git"),
            Some("owner/repo".to_string()),
        );
    }
}
