use std::path::Path;

use anyhow::{Context, Result};

/// Discover a git repository by walking up from the given directory.
pub fn discover_repo(path: &Path) -> Result<gix::Repository> {
    gix::discover(path).context("not a git repository (or any parent up to mount point)")
}

/// Extract repo name from a git remote URL.
/// Handles both HTTPS and SSH formats:
/// - `https://github.com/owner/repo.git` → `owner/repo`
/// - `git@github.com:owner/repo.git` → `owner/repo`
pub fn repo_name_from_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git").trim_end_matches('/');

    // SSH format: git@github.com:owner/repo
    if let Some(path) = url.strip_prefix("git@") {
        let path = path.split_once(':')?.1;
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some(format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]));
        }
    }

    // HTTPS format: https://github.com/owner/repo
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() >= 2 {
        return Some(format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]));
    }

    None
}
