use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[allow(deprecated)]
fn dk() -> Command {
    Command::cargo_bin("dk").unwrap()
}

#[test]
fn remote_add_and_list() {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();

    dk().arg("remote")
        .arg("add")
        .arg("origin")
        .arg("https://example.com/repo.git")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("remote")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("origin"));
}

#[test]
fn remote_remove() {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();

    dk().arg("remote")
        .arg("add")
        .arg("upstream")
        .arg("https://example.com/upstream.git")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("remote")
        .arg("remove")
        .arg("upstream")
        .current_dir(dir.path())
        .assert()
        .success();

    dk().arg("remote")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("upstream").not());
}
