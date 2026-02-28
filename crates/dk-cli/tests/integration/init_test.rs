use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[allow(deprecated)]
fn dk() -> Command {
    Command::cargo_bin("dk").unwrap()
}

#[test]
fn init_creates_git_directory() {
    let dir = TempDir::new().unwrap();
    dk().arg("init")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_defaults_to_current_directory() {
    let dir = TempDir::new().unwrap();
    dk().arg("init")
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(dir.path().join(".git").exists());
}

#[test]
fn init_creates_subdirectory_if_needed() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("my-repo");
    dk().arg("init")
        .arg(&sub)
        .assert()
        .success();
    assert!(sub.join(".git").exists());
}

#[test]
fn init_in_existing_repo_succeeds() {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    dk().arg("init")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Reinitialized").or(predicate::str::contains("Initialized")));
}
