use anyhow::{bail, Context, Result};

use crate::util::discover_repo;

pub fn run(message: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;

    let workdir = repo
        .workdir()
        .context("cannot commit in a bare repository")?
        .to_path_buf();

    let git_dir = repo.git_dir().to_path_buf();

    // Check if there's anything staged to commit.
    // We do this by running `git diff --cached --quiet` which exits 1 if there
    // are staged changes. On a brand-new repo with no HEAD, we check if the
    // index has any entries at all.
    let has_staged = has_staged_changes(&workdir, &git_dir)?;

    if !has_staged {
        bail!("nothing to commit");
    }

    let message = message.context("no commit message provided; use -m <message>")?;

    // Delegate to git commit for the alpha release.
    // This is tech debt to be replaced with native gix commit creation.
    let git_exe = gix::path::env::exe_invocation();

    let output = std::process::Command::new(git_exe)
        .args(["commit", "-m", &message])
        .current_dir(&workdir)
        .env("GIT_DIR", &git_dir)
        .output()
        .context("failed to execute git commit")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("nothing to commit");
        }
        bail!("commit failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Print a summary line that includes the commit message.
    // git commit output typically looks like:
    //   [main (root-commit) abc1234] initial commit
    //    1 file changed, 1 insertion(+)
    // We print it as-is, which naturally contains the commit message.
    print!("{}", stdout);

    Ok(())
}

/// Check whether there are staged changes ready to commit.
fn has_staged_changes(
    workdir: &std::path::Path,
    git_dir: &std::path::Path,
) -> Result<bool> {
    let git_exe = gix::path::env::exe_invocation();

    // First check: does HEAD exist? If not, this is a fresh repo and we just
    // need to check if the index has entries.
    let head_check = std::process::Command::new(git_exe)
        .args(["rev-parse", "HEAD"])
        .current_dir(workdir)
        .env("GIT_DIR", git_dir)
        .output()
        .context("failed to check HEAD")?;

    if !head_check.status.success() {
        // No HEAD -- fresh repo. Check if any files are staged in the index
        // using ls-files. We avoid the empty tree SHA approach because gix-
        // initialized repos may not have the empty tree object available.
        let ls_files = std::process::Command::new(git_exe)
            .args(["ls-files", "--cached"])
            .current_dir(workdir)
            .env("GIT_DIR", git_dir)
            .output()
            .context("failed to list staged files")?;

        let code = ls_files.status.code().unwrap_or(128);
        if code > 1 {
            let stderr = String::from_utf8_lossy(&ls_files.stderr);
            bail!("failed to check index: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&ls_files.stdout);
        return Ok(!stdout.trim().is_empty());
    }

    // HEAD exists -- compare index to HEAD
    let diff_cached = std::process::Command::new(git_exe)
        .args(["diff-index", "--cached", "--quiet", "HEAD"])
        .current_dir(workdir)
        .env("GIT_DIR", git_dir)
        .output()
        .context("failed to check staged changes")?;

    // Exit code 1 = differences exist (staged changes), 0 = no differences.
    // Codes > 1 indicate an error (e.g. 128 for corrupt index/missing objects).
    let code = diff_cached.status.code().unwrap_or(128);
    if code > 1 {
        let stderr = String::from_utf8_lossy(&diff_cached.stderr);
        bail!("failed to check staged changes: {}", stderr.trim());
    }
    Ok(code == 1)
}
