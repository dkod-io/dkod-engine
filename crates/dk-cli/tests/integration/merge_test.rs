use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[allow(deprecated)]
fn dk() -> Command {
    Command::cargo_bin("dk").unwrap()
}

fn configure_git_user(dir: &std::path::Path) {
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn init_with_commit(dir: &std::path::Path) {
    dk().arg("init").arg(dir).assert().success();
    configure_git_user(dir);
    fs::write(dir.join("file.txt"), "content").unwrap();
    dk().arg("add")
        .arg("file.txt")
        .current_dir(dir)
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("initial")
        .current_dir(dir)
        .assert()
        .success();
}

#[test]
fn merge_branch_into_main() {
    let dir = TempDir::new().unwrap();
    init_with_commit(dir.path());

    // Create and switch to feature branch
    dk().arg("checkout")
        .arg("-b")
        .arg("feature")
        .current_dir(dir.path())
        .assert()
        .success();

    // Add a new file on feature branch
    fs::write(dir.path().join("feature.txt"), "feature content").unwrap();
    dk().arg("add")
        .arg("feature.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("add feature file")
        .current_dir(dir.path())
        .assert()
        .success();

    // Switch back to main/master
    // Detect default branch name
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // We're on feature, need to go back to the initial branch
    dk().arg("checkout")
        .arg("-b")
        .arg("feature") // already exists, so let's figure out the default
        .current_dir(dir.path());

    // Determine the default branch by listing branches
    let branch_output = dk()
        .arg("branch")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&branch_output.stdout);
    let default_branch = if branches.contains("main") {
        "main"
    } else {
        "master"
    };

    dk().arg("checkout")
        .arg(default_branch)
        .current_dir(dir.path())
        .assert()
        .success();

    // feature.txt should not exist on main yet
    assert!(!dir.path().join("feature.txt").exists());

    // Merge feature into main
    dk().arg("merge")
        .arg("feature")
        .current_dir(dir.path())
        .assert()
        .success();

    // Now feature.txt should exist
    assert!(dir.path().join("feature.txt").exists());
    let _ = output; // suppress unused warning
}
