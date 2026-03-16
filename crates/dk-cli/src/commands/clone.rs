use std::path::PathBuf;

use anyhow::{bail, Context, Result};

pub fn run(url: String, path: Option<PathBuf>) -> Result<()> {
    let dest_name = match &path {
        Some(p) => p.display().to_string(),
        None => url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .trim_end_matches(".git")
            .to_string(),
    };

    let git_exe = gix::path::env::exe_invocation();
    let mut cmd = std::process::Command::new(git_exe);
    cmd.arg("clone").arg(&url);
    if let Some(p) = &path {
        cmd.arg(p);
    }

    // Inherit stderr so git clone progress (object counts, deltas, transfer
    // speed) streams directly to the terminal in real time. Git's own
    // "Cloning into" message also goes to stderr.
    cmd.stderr(std::process::Stdio::inherit());

    let output = cmd.output().context("failed to execute git clone")?;
    if !output.status.success() {
        bail!("clone failed");
    }

    // Print to stdout for programmatic consumers. Git's "Cloning into" and
    // progress output go to stderr (inherited above).
    println!("Cloning into '{}' complete.", dest_name);
    Ok(())
}
