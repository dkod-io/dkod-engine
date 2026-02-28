//! Integration tests for Git repository tree operations (NSI).
//!
//! Tests for `read_tree_entry`, `list_tree_files`, and `commit_tree_overlay`
//! edge cases using temp git repos.

use std::path::Path;

use dk_engine::git::{GitObjects, GitRepository};
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────

/// Create a temporary git repo with an initial commit containing one file.
/// Returns (repo, commit_hash, tmpdir).
fn init_repo_with_file(filename: &str, content: &[u8]) -> (GitRepository, String, TempDir) {
    let tmp = TempDir::new().expect("create tempdir");
    let path = tmp.path().join("test-repo");

    let repo = GitRepository::init(&path).expect("init repo");
    let objects = GitObjects::new(&repo);
    objects
        .write_file(Path::new(filename), content)
        .expect("write file to working tree");

    let commit = repo
        .commit("initial commit", "test", "test@test.com")
        .expect("initial commit");

    (repo, commit, tmp)
}

// ═══════════════════════════════════════════════════════════════════
// Test 1: read_tree_entry with invalid commit hex
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_read_tree_entry_invalid_commit() {
    let (repo, _commit, _tmp) = init_repo_with_file("file.txt", b"content");

    // Bogus hex that is not a valid commit.
    let result = repo.read_tree_entry("not_a_valid_hex", "file.txt");
    assert!(
        result.is_err(),
        "read_tree_entry with invalid hex should error"
    );

    // Valid hex format but non-existent commit.
    let result = repo.read_tree_entry(
        "0000000000000000000000000000000000000000",
        "file.txt",
    );
    assert!(
        result.is_err(),
        "read_tree_entry with non-existent commit should error"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 2: read_tree_entry with nonexistent path
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_read_tree_entry_nonexistent_path() {
    let (repo, commit, _tmp) = init_repo_with_file("real.txt", b"exists");

    let result = repo.read_tree_entry(&commit, "does/not/exist.txt");
    assert!(
        result.is_err(),
        "read_tree_entry for nonexistent path should error"
    );

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("not found") || err_msg.contains("does/not/exist.txt"),
        "error should reference the missing path, got: {err_msg}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 3: list_tree_files on a repo with just one file
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_list_tree_files_single_file() {
    let (repo, commit, _tmp) = init_repo_with_file("only.txt", b"content");

    let files = repo
        .list_tree_files(&commit)
        .expect("list_tree_files should succeed");

    assert_eq!(files.len(), 1);
    assert_eq!(files[0], "only.txt");
}

// ═══════════════════════════════════════════════════════════════════
// Test 4: commit_tree_overlay with empty overlay
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_commit_tree_overlay_empty() {
    let (repo, commit, _tmp) = init_repo_with_file("base.txt", b"base content");

    // Empty overlay — should still create a valid commit with the base tree.
    let overlay: Vec<(String, Option<Vec<u8>>)> = vec![];

    let new_commit = repo
        .commit_tree_overlay(
            &commit,
            &overlay,
            &commit,
            "empty overlay commit",
            "test",
            "test@test.com",
        )
        .expect("commit_tree_overlay with empty overlay should succeed");

    assert!(!new_commit.is_empty());

    // The base file should still be present and unchanged.
    let content = repo
        .read_tree_entry(&new_commit, "base.txt")
        .expect("base.txt should exist in new commit");
    assert_eq!(content, b"base content");
}

// ═══════════════════════════════════════════════════════════════════
// Test 5: read_tree_entry returns correct content
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_read_tree_entry_correct_content() {
    let expected = b"fn main() { println!(\"hello\"); }";
    let (repo, commit, _tmp) = init_repo_with_file("src/main.rs", expected);

    let content = repo
        .read_tree_entry(&commit, "src/main.rs")
        .expect("should read file from tree");

    assert_eq!(content, expected.to_vec());
}

// ═══════════════════════════════════════════════════════════════════
// Test 6: list_tree_files with multiple files
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_list_tree_files_multiple() {
    let tmp = TempDir::new().expect("create tempdir");
    let path = tmp.path().join("multi-repo");

    let repo = GitRepository::init(&path).expect("init repo");
    let objects = GitObjects::new(&repo);

    objects
        .write_file(Path::new("a.txt"), b"a")
        .expect("write a.txt");
    objects
        .write_file(Path::new("b.txt"), b"b")
        .expect("write b.txt");
    objects
        .write_file(Path::new("dir/c.txt"), b"c")
        .expect("write dir/c.txt");

    let commit = repo
        .commit("multi file commit", "test", "test@test.com")
        .expect("commit");

    let mut files = repo
        .list_tree_files(&commit)
        .expect("list_tree_files should succeed");
    files.sort();

    assert_eq!(files.len(), 3);
    assert_eq!(files[0], "a.txt");
    assert_eq!(files[1], "b.txt");
    assert_eq!(files[2], "dir/c.txt");
}

// ═══════════════════════════════════════════════════════════════════
// Test 7: commit_tree_overlay with additions and deletions
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_commit_tree_overlay_add_and_delete() {
    let tmp = TempDir::new().expect("create tempdir");
    let path = tmp.path().join("overlay-repo");

    let repo = GitRepository::init(&path).expect("init repo");
    let objects = GitObjects::new(&repo);

    objects
        .write_file(Path::new("keep.txt"), b"keep")
        .expect("write keep.txt");
    objects
        .write_file(Path::new("remove.txt"), b"remove me")
        .expect("write remove.txt");

    let commit = repo
        .commit("initial", "test", "test@test.com")
        .expect("commit");

    // Overlay: add a new file and delete an existing one.
    let overlay: Vec<(String, Option<Vec<u8>>)> = vec![
        ("new.txt".to_string(), Some(b"brand new".to_vec())),
        ("remove.txt".to_string(), None),
    ];

    let new_commit = repo
        .commit_tree_overlay(
            &commit,
            &overlay,
            &commit,
            "add and delete",
            "test",
            "test@test.com",
        )
        .expect("commit_tree_overlay should succeed");

    // Verify new file exists.
    let new_content = repo
        .read_tree_entry(&new_commit, "new.txt")
        .expect("new.txt should exist");
    assert_eq!(new_content, b"brand new");

    // Verify deleted file is gone.
    let removed = repo.read_tree_entry(&new_commit, "remove.txt");
    assert!(removed.is_err(), "remove.txt should be deleted");

    // Verify kept file is unchanged.
    let kept = repo
        .read_tree_entry(&new_commit, "keep.txt")
        .expect("keep.txt should still exist");
    assert_eq!(kept, b"keep");
}
