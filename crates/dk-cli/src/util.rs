use std::path::Path;

use anyhow::{Context, Result};

/// Discover a git repository by walking up from the given directory.
pub fn discover_repo(path: &Path) -> Result<gix::Repository> {
    gix::discover(path).context("not a git repository (or any parent up to mount point)")
}
