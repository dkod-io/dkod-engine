use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(remote: Option<String>, branch: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;
    let workdir = repo
        .workdir()
        .context("cannot push from a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("push");
    if let Some(r) = &remote {
        cmd.arg(r);
    }
    if let Some(b) = &branch {
        cmd.arg(b);
    }
    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);

    // Inherit stderr so git push progress (object counting, delta
    // compression, remote refs) streams to the terminal in real time.
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status().context("failed to run git push")?;
    if !status.success() {
        bail!("push failed");
    }
    Ok(())
}
