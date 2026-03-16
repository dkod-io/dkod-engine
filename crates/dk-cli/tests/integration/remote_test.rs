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
    dk().arg("git").arg("init").arg(dir.path()).assert().success();

    dk().args(["git", "remote", "add", "origin", "https://example.com/repo.git"])
        .current_dir(dir.path())
        .assert()
        .success();

    dk().args(["git", "remote"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("origin"));
}

#[test]
fn remote_remove() {
    let dir = TempDir::new().unwrap();
    dk().arg("git").arg("init").arg(dir.path()).assert().success();

    dk().args(["git", "remote", "add", "upstream", "https://example.com/upstream.git"])
        .current_dir(dir.path())
        .assert()
        .success();

    dk().args(["git", "remote", "remove", "upstream"])
        .current_dir(dir.path())
        .assert()
        .success();

    dk().args(["git", "remote"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("upstream").not());
}
