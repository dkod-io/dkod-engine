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

fn init_repo_with_commit(msg: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());
    fs::write(dir.path().join("file.txt"), "content").unwrap();
    dk().arg("add")
        .arg("file.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg(msg)
        .current_dir(dir.path())
        .assert()
        .success();
    dir
}

#[test]
fn log_shows_commit() {
    let dir = init_repo_with_commit("initial commit");
    dk().arg("log")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("initial commit")
                .and(predicate::str::contains("commit "))
                .and(predicate::str::contains("Author:")),
        );
}

#[test]
fn log_oneline() {
    let dir = init_repo_with_commit("my message");
    dk().arg("log")
        .arg("--oneline")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("my message"));
}

#[test]
fn log_limit() {
    let dir = init_repo_with_commit("first");
    fs::write(dir.path().join("second.txt"), "second").unwrap();
    dk().arg("add")
        .arg("second.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("second")
        .current_dir(dir.path())
        .assert()
        .success();

    // -n 1 should only show latest
    dk().arg("log")
        .arg("-n")
        .arg("1")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("second").and(predicate::str::contains("first").not()));
}

#[test]
fn log_empty_repo_fails() {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    dk().arg("log")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no commits"));
}
