use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(target: Option<String>, b: Option<String>) -> Result<()> {
    if target.is_none() && b.is_none() {
        bail!("usage: dk checkout [-b <new-branch>] <branch>");
    }

    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;
    let workdir = repo
        .workdir()
        .context("cannot checkout in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("checkout");

    if let Some(new_branch) = &b {
        cmd.arg("-b").arg(new_branch);
    } else if let Some(ref_name) = &target {
        cmd.arg(ref_name);
    }

    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("failed to run git checkout")?;
    if !status.success() {
        bail!("checkout failed");
    }
    Ok(())
}
