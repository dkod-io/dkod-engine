use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(pathspec: Vec<PathBuf>, all: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;

    let workdir = repo
        .workdir()
        .context("cannot add files in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    if all {
        // Delegate to `git add -A`, optionally scoped to specific paths.
        // Real `git add -A <pathspec>` stages all changes within the given paths.
        let mut cmd = std::process::Command::new(git_exe);
        cmd.args(["add", "-A"]);
        if !pathspec.is_empty() {
            cmd.arg("--");
            for p in &pathspec {
                cmd.arg(p);
            }
        }
        cmd.current_dir(&workdir);
        cmd.env("GIT_DIR", &git_dir);

        let output = cmd.output().context("failed to execute git add")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git add failed: {}", stderr.trim());
        }
    } else {
        if pathspec.is_empty() {
            bail!("nothing specified, nothing added\nhint: use 'dk add -A' to add all files");
        }

        // Resolve pathspecs relative to cwd and make them relative to workdir.
        // We do NOT check exists() because deleted files are valid pathspecs
        // for staging removals (git add <deleted-file> stages the deletion).
        let mut resolved = Vec::new();
        for p in &pathspec {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            };
            let rel = abs
                .strip_prefix(&workdir)
                .with_context(|| format!("'{}' is outside the repository", p.display()))?
                .to_path_buf();
            resolved.push(rel);
        }

        // Delegate to git add with explicit paths — git itself will validate
        // that the pathspecs match tracked or untracked files.
        let mut cmd = std::process::Command::new(git_exe);
        cmd.arg("add");
        cmd.arg("--");
        for p in &resolved {
            cmd.arg(p);
        }
        cmd.current_dir(&workdir);
        cmd.env("GIT_DIR", &git_dir);

        let output = cmd.output().context("failed to execute git add")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git add failed: {}", stderr.trim());
        }
    }

    Ok(())
}
