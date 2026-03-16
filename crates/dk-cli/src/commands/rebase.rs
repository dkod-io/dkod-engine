use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(branch: Option<String>, onto: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;
    let workdir = repo
        .workdir()
        .context("cannot rebase in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("rebase");

    if let Some(newbase) = &onto {
        cmd.arg("--onto").arg(newbase);
    }
    if let Some(b) = &branch {
        cmd.arg(b);
    }

    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("failed to run git rebase")?;
    if !status.success() {
        bail!("rebase failed");
    }
    Ok(())
}
