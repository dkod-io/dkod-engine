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

#[test]
fn full_workflow_init_add_commit_log_diff() {
    let dir = TempDir::new().unwrap();

    // 1. Init
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());

    // 2. Status on empty repo
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to commit"));

    // 3. Create files
    fs::write(dir.path().join("README.md"), "# My Project\n").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    // 4. Status shows untracked
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("README.md").and(predicate::str::contains("main.rs")),
        );

    // 5. Add all
    dk().arg("add")
        .arg("-A")
        .current_dir(dir.path())
        .assert()
        .success();

    // 6. Commit
    dk().arg("commit")
        .arg("-m")
        .arg("initial commit")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));

    // 7. Log shows commit
    dk().arg("log")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));

    // 8. Log oneline
    dk().arg("log")
        .arg("--oneline")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));

    // 9. Modify a file
    fs::write(dir.path().join("README.md"), "# My Project\n\nUpdated.\n").unwrap();

    // 10. Diff shows change
    dk().arg("diff")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated"));

    // 11. Add and commit the modification
    dk().arg("add")
        .arg("README.md")
        .current_dir(dir.path())
        .assert()
        .success();
    dk().arg("diff")
        .arg("--staged")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated"));
    dk().arg("commit")
        .arg("-m")
        .arg("update readme")
        .current_dir(dir.path())
        .assert()
        .success();

    // 12. Log shows both commits
    dk().arg("log")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("update readme")
                .and(predicate::str::contains("initial commit")),
        );

    // 13. Status is clean
    dk().arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to commit"));
}

#[test]
fn branch_checkout_merge_workflow() {
    let dir = TempDir::new().unwrap();
    dk().arg("init").arg(dir.path()).assert().success();
    configure_git_user(dir.path());

    // Initial commit on main
    std::fs::write(dir.path().join("main.txt"), "main content").unwrap();
    dk().args(["add", "main.txt"])
        .current_dir(dir.path())
        .assert()
        .success();
    dk().args(["commit", "-m", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Create feature branch
    dk().args(["checkout", "-b", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Add feature work
    std::fs::write(dir.path().join("feature.txt"), "feature work").unwrap();
    dk().args(["add", "feature.txt"])
        .current_dir(dir.path())
        .assert()
        .success();
    dk().args(["commit", "-m", "add feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Switch back to main
    dk().args(["checkout", "main"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(!dir.path().join("feature.txt").exists());

    // Merge feature into main
    dk().args(["merge", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(dir.path().join("feature.txt").exists());

    // Log should show both commits
    dk().args(["log", "--oneline"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("add feature"));
}

#[test]
fn clone_push_pull_workflow() {
    // 1. Create bare "remote"
    let bare = TempDir::new().unwrap();
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .arg(bare.path())
        .output()
        .unwrap();

    // 2. Clone, add, commit, push
    let work = TempDir::new().unwrap();
    let work_path = work.path().join("myrepo");
    dk().arg("clone")
        .arg(bare.path())
        .arg(&work_path)
        .assert()
        .success();
    configure_git_user(&work_path);

    fs::write(work_path.join("hello.txt"), "hello").unwrap();
    dk().arg("add")
        .arg("hello.txt")
        .current_dir(&work_path)
        .assert()
        .success();
    dk().arg("commit")
        .arg("-m")
        .arg("first")
        .current_dir(&work_path)
        .assert()
        .success();
    dk().arg("push")
        .current_dir(&work_path)
        .assert()
        .success();

    // 3. Second clone should have the file
    let clone2 = TempDir::new().unwrap();
    let clone2_path = clone2.path().join("repo2");
    dk().arg("clone")
        .arg(bare.path())
        .arg(&clone2_path)
        .assert()
        .success();
    assert!(clone2_path.join("hello.txt").exists());
    assert_eq!(
        fs::read_to_string(clone2_path.join("hello.txt")).unwrap(),
        "hello"
    );
}
