use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(
    name: Option<String>,
    message: Option<String>,
    delete: Option<String>,
    list: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;
    let workdir = repo
        .workdir()
        .context("cannot manage tags in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("tag");

    if let Some(d) = &delete {
        cmd.arg("-d").arg(d);
    } else if let Some(n) = &name {
        if let Some(msg) = &message {
            cmd.arg("-a").arg(n).arg("-m").arg(msg);
        } else {
            cmd.arg(n);
        }
    } else if list {
        cmd.arg("-l");
    }

    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);

    let output = cmd.output().context("failed to run git tag")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("tag failed: {}", stderr.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{}", stdout);
    Ok(())
}
