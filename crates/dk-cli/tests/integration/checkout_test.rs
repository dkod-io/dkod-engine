use assert_cmd::Command;
use predicates::prelude::*;
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
fn checkout_create_and_switch() {
    let dir = TempDir::new().unwrap();
    init_with_commit(dir.path());

    dk().arg("checkout")
        .arg("-b")
        .arg("new-branch")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("branch")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("* new-branch"));
}

#[test]
fn checkout_existing_branch() {
    let dir = TempDir::new().unwrap();
    init_with_commit(dir.path());

    // Create branch without switching
    dk().arg("branch")
        .arg("other")
        .current_dir(dir.path())
        .assert()
        .success();

    // Switch to it
    dk().arg("checkout")
        .arg("other")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("branch")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("* other"));
}
