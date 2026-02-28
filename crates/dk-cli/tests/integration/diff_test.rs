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

fn init_committed_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());
    fs::write(dir.path().join("file.txt"), "line one\nline two\n").unwrap();
    dk().arg("add")
        .arg("file.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("initial")
        .current_dir(dir.path())
        .assert()
        .success();
    dir
}

#[test]
fn diff_shows_unstaged_changes() {
    let dir = init_committed_repo();
    fs::write(
        dir.path().join("file.txt"),
        "line one\nline two\nline three\n",
    )
    .unwrap();
    dk().arg("diff")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("file.txt")
                .and(predicate::str::contains("line three")),
        );
}

#[test]
fn diff_no_changes_empty_output() {
    let dir = init_committed_repo();
    dk().arg("diff")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn diff_staged_flag() {
    let dir = init_committed_repo();
    fs::write(dir.path().join("file.txt"), "modified content\n").unwrap();
    dk().arg("add")
        .arg("file.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("diff")
        .arg("--staged")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("modified content"));
}
