use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[allow(deprecated)]
fn dk() -> Command {
    Command::cargo_bin("dk").unwrap()
}

fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());
    dir
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

#[test]
fn commit_with_message() {
    let dir = init_repo();
    fs::write(dir.path().join("hello.txt"), "hello world").unwrap();

    dk().arg("add")
        .arg("hello.txt")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("commit")
        .arg("-m")
        .arg("initial commit")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));
}

#[test]
fn commit_nothing_staged_fails() {
    let dir = init_repo();

    dk().arg("commit")
        .arg("-m")
        .arg("empty commit")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("nothing to commit"));
}

#[test]
fn commit_creates_head() {
    let dir = init_repo();
    fs::write(dir.path().join("file.txt"), "content").unwrap();

    dk().arg("add")
        .arg("file.txt")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("commit")
        .arg("-m")
        .arg("first commit")
        .current_dir(dir.path())
        .assert()
        .success();

    // Verify HEAD exists and points to our commit by checking git log
    let output = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let log_output = String::from_utf8_lossy(&output.stdout);
    assert!(
        log_output.contains("first commit"),
        "git log should contain 'first commit', got: {}",
        log_output
    );
}

#[test]
fn commit_second_commit() {
    let dir = init_repo();

    // First commit
    fs::write(dir.path().join("a.txt"), "aaa").unwrap();
    dk().arg("add")
        .arg("a.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("first commit")
        .current_dir(dir.path())
        .assert()
        .success();

    // Second commit
    fs::write(dir.path().join("b.txt"), "bbb").unwrap();
    dk().arg("add")
        .arg("b.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("second commit")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("second commit"));
}
