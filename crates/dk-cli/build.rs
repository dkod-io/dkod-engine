use std::process::Command;

fn main() {
    // Get version from git tag (e.g. "v0.2.68" → "0.2.68").
    // Falls back to Cargo.toml version if git is unavailable.
    let version = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|v| v.trim().trim_start_matches('v').to_string());

    if let Some(ver) = version {
        println!("cargo:rustc-env=DK_VERSION={ver}");
    }

    // Rerun when tags change.
    // cargo:rerun-if-changed paths are relative to the package root (crates/dk-cli/),
    // so we walk up from CARGO_MANIFEST_DIR to find the workspace root containing .git.
    // We also watch packed-refs (where CI-fetched tags land) and HEAD.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(root) = manifest.ancestors().find(|p| p.join(".git").exists()) {
        println!(
            "cargo:rerun-if-changed={}",
            root.join(".git/refs/tags").display()
        );
        println!(
            "cargo:rerun-if-changed={}",
            root.join(".git/packed-refs").display()
        );
        println!(
            "cargo:rerun-if-changed={}",
            root.join(".git/HEAD").display()
        );
    }
}
