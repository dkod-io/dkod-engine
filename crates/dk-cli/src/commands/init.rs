use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;

pub fn run(path: Option<PathBuf>) -> Result<()> {
    let target = match path {
        Some(p) => {
            std::fs::create_dir_all(&p)
                .with_context(|| format!("failed to create directory '{}'", p.display()))?;
            p
        }
        None => std::env::current_dir().context("failed to get current directory")?,
    };

    let target = target.canonicalize().unwrap_or_else(|_| target.clone());

    let git_dir = target.join(".git");
    let is_reinit = git_dir.exists();

    if is_reinit {
        // gix::init() refuses to initialize when .git already exists, so we
        // validate the existing repository by opening it instead. Unlike real
        // `git init`, this does not refresh hooks or templates — a known
        // limitation to be addressed when dk-engine replaces the gix layer.
        gix::open(&target).with_context(|| {
            format!(
                "failed to reinitialize repository at '{}'",
                target.display()
            )
        })?;
    } else {
        gix::init(&target).with_context(|| {
            format!("failed to initialize repository at '{}'", target.display())
        })?;
    }

    let (prefix, qualifier) = if is_reinit {
        ("Reinitialized", "existing")
    } else {
        ("Initialized", "empty")
    };

    println!(
        "{} {} Dekode repository in {}",
        prefix,
        qualifier,
        git_dir.display().to_string().bold()
    );

    Ok(())
}
