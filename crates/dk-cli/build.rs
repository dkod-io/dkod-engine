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
    println!("cargo:rerun-if-changed=.git/refs/tags");
}
