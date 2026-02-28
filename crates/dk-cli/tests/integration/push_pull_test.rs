use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
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

/// Create a bare repo + clone with one commit
fn setup_remote_and_clone() -> (TempDir, TempDir, PathBuf) {
    let bare = TempDir::new().unwrap();
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .arg(bare.path())
        .output()
        .unwrap();

    let work = TempDir::new().unwrap();
    let work_path = work.path().join("repo");
    dk().arg("clone")
        .arg(bare.path())
        .arg(&work_path)
        .assert()
        .success();
    configure_git_user(&work_path);

    fs::write(work_path.join("file.txt"), "content").unwrap();
    dk().arg("add")
        .arg("file.txt")
        .current_dir(&work_path)
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("initial")
        .current_dir(&work_path)
        .assert()
        .success();

    (bare, work, work_path)
}

#[test]
fn push_to_remote() {
    let (bare, _work, work_path) = setup_remote_and_clone();
    dk().arg("push").current_dir(&work_path).assert().success();

    // Verify by cloning from bare
    let verify = TempDir::new().unwrap();
    let verify_path = verify.path().join("verify");
    dk().arg("clone")
        .arg(bare.path())
        .arg(&verify_path)
        .assert()
        .success();
    assert!(verify_path.join("file.txt").exists());
}

#[test]
fn pull_from_remote() {
    let (bare, _work, work_path) = setup_remote_and_clone();
    dk().arg("push").current_dir(&work_path).assert().success();

    // Second clone
    let clone2 = TempDir::new().unwrap();
    let clone2_path = clone2.path().join("repo2");
    dk().arg("clone")
        .arg(bare.path())
        .arg(&clone2_path)
        .assert()
        .success();
    configure_git_user(&clone2_path);

    // Add file in clone2, push
    fs::write(clone2_path.join("new.txt"), "new content").unwrap();
    dk().arg("add")
        .arg("new.txt")
        .current_dir(&clone2_path)
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("add new")
        .current_dir(&clone2_path)
        .assert()
        .success();
    dk().arg("push")
        .current_dir(&clone2_path)
        .assert()
        .success();

    // Pull in original
    dk().arg("pull").current_dir(&work_path).assert().success();
    assert!(work_path.join("new.txt").exists());
}

#[test]
fn push_no_remote_fails() {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());
    fs::write(dir.path().join("f.txt"), "x").unwrap();
    dk().arg("add")
        .arg("f.txt")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("x")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("push").current_dir(dir.path()).assert().failure();
}
