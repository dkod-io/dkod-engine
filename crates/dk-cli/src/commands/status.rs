use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::util::discover_repo;

/// A staged file change (index vs HEAD).
struct StagedEntry {
    kind: &'static str, // "new file", "modified", "deleted", "renamed", "copied", "typechange"
    path: String,
}

/// An unstaged file change (worktree vs index).
struct UnstagedEntry {
    kind: &'static str, // "modified", "deleted", "typechange"
    path: String,
}

/// Parse the porcelain v2 status output from git and render human-readable output
/// that closely matches `git status`.
pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;

    let workdir = repo
        .workdir()
        .context("cannot show status in a bare repository")?
        .to_path_buf();
    let git_dir = repo.git_dir().to_path_buf();

    let git_exe = gix::path::env::exe_invocation();

    // Use porcelain v2 with branch info for stable, machine-readable output.
    let output = std::process::Command::new(git_exe)
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(&workdir)
        .env("GIT_DIR", &git_dir)
        .output()
        .context("failed to run git status")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("status failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse porcelain v2 output.
    let mut branch_head: Option<String> = None;
    let mut branch_oid_initial = false;
    let mut staged: Vec<StagedEntry> = Vec::new();
    let mut unstaged: Vec<UnstagedEntry> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();

    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            branch_head = Some(rest.to_string());
            continue;
        }

        // Parse: # branch.oid <oid-or-initial>
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            if rest == "(initial)" {
                branch_oid_initial = true;
            }
            continue;
        }

        // Skip other header lines.
        if line.starts_with('#') {
            continue;
        }

        // Untracked files: `? <path>`
        if let Some(path) = line.strip_prefix("? ") {
            untracked.push(path.to_string());
            continue;
        }

        // Ordinary changed entries: `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`
        // Renamed/copied entries: `2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path><tab><origPath>`
        if line.starts_with("1 ") || line.starts_with("2 ") {
            let parts: Vec<&str> = line.splitn(10, ' ').collect();
            if parts.len() < 9 {
                continue;
            }
            let xy = parts[1];
            let x = xy.as_bytes().first().copied().unwrap_or(b'.');
            let y = xy.as_bytes().get(1).copied().unwrap_or(b'.');

            // For renamed/copied entries, the path field contains `<path>\t<origPath>`.
            let path_field = if line.starts_with("2 ") {
                // parts[9] contains the path with tab-separated original
                parts.get(9).unwrap_or(&"")
            } else {
                parts.get(8).unwrap_or(&"")
            };
            let path = path_field.split('\t').next().unwrap_or("").to_string();

            // Index change (X column): staged changes.
            match x {
                b'A' => staged.push(StagedEntry {
                    kind: "new file",
                    path: path.clone(),
                }),
                b'M' => staged.push(StagedEntry {
                    kind: "modified",
                    path: path.clone(),
                }),
                b'D' => staged.push(StagedEntry {
                    kind: "deleted",
                    path: path.clone(),
                }),
                b'R' => {
                    let orig = path_field.split('\t').nth(1).unwrap_or("").to_string();
                    staged.push(StagedEntry {
                        kind: "renamed",
                        path: format!("{} -> {}", orig, path),
                    });
                }
                b'C' => {
                    let orig = path_field.split('\t').nth(1).unwrap_or("").to_string();
                    staged.push(StagedEntry {
                        kind: "copied",
                        path: format!("{} -> {}", orig, path),
                    });
                }
                b'T' => staged.push(StagedEntry {
                    kind: "typechange",
                    path: path.clone(),
                }),
                _ => {}
            }

            // Worktree change (Y column): unstaged changes.
            match y {
                b'M' => unstaged.push(UnstagedEntry {
                    kind: "modified",
                    path: path.clone(),
                }),
                b'D' => unstaged.push(UnstagedEntry {
                    kind: "deleted",
                    path: path.clone(),
                }),
                b'T' => unstaged.push(UnstagedEntry {
                    kind: "typechange",
                    path,
                }),
                _ => {}
            }

            continue;
        }

        // Unmerged entries: `u <XY> ...` -- treat as unstaged modified for now.
        if line.starts_with("u ") {
            let parts: Vec<&str> = line.splitn(11, ' ').collect();
            if let Some(&path) = parts.last() {
                unstaged.push(UnstagedEntry {
                    kind: "modified",
                    path: path.to_string(),
                });
            }
        }
    }

    // --- Render output matching `git status` format ---

    // Branch header.
    if let Some(ref branch) = branch_head {
        if branch == "(detached)" {
            println!("HEAD detached");
        } else {
            println!("On branch {}", branch);
        }
    }

    let has_staged = !staged.is_empty();
    let has_unstaged = !unstaged.is_empty();
    let has_untracked = !untracked.is_empty();

    // Staged changes.
    if has_staged {
        println!("\nChanges to be committed:");
        println!("  (use \"dk reset HEAD <file>...\" to unstage)\n");
        for entry in &staged {
            let line = format!("\t{}:   {}", entry.kind, entry.path);
            println!("{}", line.green());
        }
    }

    // Unstaged changes.
    if has_unstaged {
        println!("\nChanges not staged for commit:");
        println!("  (use \"dk add <file>...\" to update what will be committed)\n");
        for entry in &unstaged {
            let line = format!("\t{}:   {}", entry.kind, entry.path);
            println!("{}", line.red());
        }
    }

    // Untracked files.
    if has_untracked {
        println!("\nUntracked files:");
        println!("  (use \"dk add <file>...\" to include in what will be committed)\n");
        for path in &untracked {
            let line = format!("\t{}", path);
            println!("{}", line.red());
        }
    }

    // Clean working tree.
    if !has_staged && !has_unstaged && !has_untracked {
        if branch_oid_initial {
            println!("\nnothing to commit (create/copy files and use \"dk add\" to track)");
        } else {
            println!("nothing to commit, working tree clean");
        }
    }

    Ok(())
}
