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
    dir
}

#[test]
fn add_single_file() {
    let dir = init_repo();
    fs::write(dir.path().join("hello.txt"), "hello world").unwrap();

    dk().arg("add")
        .arg("hello.txt")
        .current_dir(dir.path())
        .assert()
        .success();

    // After adding, the file should no longer appear as untracked
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hello.txt").not().or(
            predicate::str::contains("new file").or(
                predicate::str::contains("Changes to be committed"),
            ),
        ));
}

#[test]
fn add_multiple_files() {
    let dir = init_repo();
    fs::write(dir.path().join("a.txt"), "aaa").unwrap();
    fs::write(dir.path().join("b.txt"), "bbb").unwrap();

    dk().arg("add")
        .arg("a.txt")
        .arg("b.txt")
        .current_dir(dir.path())
        .assert()
        .success();

    // Both files should no longer appear as untracked
    let output = dk()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success();

    // After adding, they should not be listed under "Untracked files"
    output.stdout(predicate::str::contains("Untracked files").not());
}

#[test]
fn add_all_flag() {
    let dir = init_repo();
    fs::write(dir.path().join("x.txt"), "xxx").unwrap();
    fs::write(dir.path().join("y.txt"), "yyy").unwrap();

    dk().arg("add")
        .arg("-A")
        .current_dir(dir.path())
        .assert()
        .success();

    // After adding all, no untracked files should remain
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Untracked files").not());
}

#[test]
fn add_nonexistent_file_fails() {
    let dir = init_repo();

    dk().arg("add")
        .arg("does_not_exist.txt")
        .current_dir(dir.path())
        .assert()
        .failure();
}
