use anyhow::{bail, Context, Result};
use clap::Subcommand;

use crate::util::discover_repo;

#[derive(Subcommand)]
pub enum RemoteAction {
    /// Add a remote
    Add { name: String, url: String },
    /// Remove a remote
    Remove { name: String },
}

pub fn run(action: Option<RemoteAction>, verbose: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;
    let workdir = repo
        .workdir()
        .context("cannot manage remotes in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("remote");

    match &action {
        Some(RemoteAction::Add { name, url }) => {
            cmd.arg("add").arg(name).arg(url);
        }
        Some(RemoteAction::Remove { name }) => {
            cmd.arg("remove").arg(name);
        }
        None => {
            if verbose {
                cmd.arg("-v");
            }
        }
    }

    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);

    let output = cmd.output().context("failed to run git remote")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("remote failed: {}", stderr.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{}", stdout);
    Ok(())
}
