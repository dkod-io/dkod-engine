use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(staged: bool, path: Option<PathBuf>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;

    let workdir = repo
        .workdir()
        .context("cannot diff in a bare repository")?
        .to_path_buf();

    let git_dir = repo.git_dir().to_path_buf();

    // Delegate to git diff for the alpha release.
    // This is tech debt to be replaced with native gix diff support.
    let git_exe = gix::path::env::exe_invocation();

    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("diff");
    if std::env::var_os("NO_COLOR").is_none() {
        cmd.arg("--color=always");
    }
    if staged {
        cmd.arg("--staged");
    }
    if let Some(p) = &path {
        cmd.arg("--").arg(p);
    }
    cmd.current_dir(&workdir);
    cmd.env("GIT_DIR", &git_dir);

    let output = cmd.output().context("failed to run git diff")?;

    // git diff: exit 0 = no differences, exit 1 = differences (both normal).
    // Exit codes >= 128 indicate hard errors (invalid object, corrupt index).
    let code = output.status.code().unwrap_or(128);
    if code > 1 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("diff failed: {}", stderr.trim());
    }

    // Write stdout as raw bytes to avoid corrupting binary diff output.
    use std::io::Write;
    std::io::stdout()
        .write_all(&output.stdout)
        .context("failed to write diff output")?;

    Ok(())
}
