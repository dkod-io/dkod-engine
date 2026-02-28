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
fn status_clean_repo() {
    let dir = init_repo();
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to commit"));
}

#[test]
fn status_shows_untracked_files() {
    let dir = init_repo();
    fs::write(dir.path().join("hello.txt"), "hello").unwrap();

    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hello.txt"));
}

#[test]
fn status_outside_repo_fails() {
    let dir = TempDir::new().unwrap();
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a git repository"));
}

#[test]
fn status_shows_staged_and_unstaged() {
    let dir = init_repo();

    // Create a file, stage it, and commit so we have a baseline.
    fs::write(dir.path().join("file.txt"), "v1").unwrap();
    dk().arg("add").arg("file.txt").current_dir(dir.path()).assert().success();
    dk().args(["commit", "-m", "initial"]).current_dir(dir.path()).assert().success();

    // Modify the file and stage it (staged change).
    fs::write(dir.path().join("file.txt"), "v2").unwrap();
    dk().arg("add").arg("file.txt").current_dir(dir.path()).assert().success();

    // Modify the file again in the worktree (unstaged change on top of staged).
    fs::write(dir.path().join("file.txt"), "v3").unwrap();

    let assert = dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success();

    // Should show staged section.
    assert
        .stdout(predicate::str::contains("Changes to be committed"))
        .stdout(predicate::str::contains("modified"))
        // Should also show unstaged section.
        .stdout(predicate::str::contains("Changes not staged for commit"));
}

#[test]
fn status_shows_branch_name() {
    let dir = init_repo();

    // gix::init creates a repo; the default branch is typically "main".
    // We just check that "On branch" appears in the output.
    let assert = dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success();

    assert.stdout(predicate::str::contains("On branch"));
}
