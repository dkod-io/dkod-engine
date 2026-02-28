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

fn create_source_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());
    fs::write(dir.path().join("hello.txt"), "hello world").unwrap();
    dk().arg("add")
        .arg("hello.txt")
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
fn clone_local_repo() {
    let source = create_source_repo();
    let dest = TempDir::new().unwrap();
    let clone_path = dest.path().join("cloned");

    dk().arg("clone")
        .arg(source.path())
        .arg(&clone_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Cloning into"));

    assert!(clone_path.join(".git").exists());
    assert!(clone_path.join("hello.txt").exists());
    assert_eq!(
        fs::read_to_string(clone_path.join("hello.txt")).unwrap(),
        "hello world"
    );
}

#[test]
fn clone_into_default_directory() {
    let source = create_source_repo();
    let work_dir = TempDir::new().unwrap();

    let source_name = source
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    dk().arg("clone")
        .arg(source.path())
        .current_dir(work_dir.path())
        .assert()
        .success();

    assert!(work_dir.path().join(&source_name).join(".git").exists());
}
