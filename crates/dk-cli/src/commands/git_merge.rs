use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(branch: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;
    let workdir = repo
        .workdir()
        .context("cannot merge in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("merge").arg(&branch);
    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("failed to run git merge")?;
    if !status.success() {
        bail!("merge failed");
    }
    Ok(())
}
