use std::path::Path;

use dk_engine::git::{GitObjects, GitRepository};
use tempfile::TempDir;

#[test]
fn test_create_and_open_repo() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test-repo");

    let repo = GitRepository::init(&path).unwrap();
    assert!(path.join(".git").exists());

    let repo2 = GitRepository::open(&path).unwrap();
    assert_eq!(repo.path(), repo2.path());
}

#[test]
fn test_write_and_read_blob() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test-repo");

    let repo = GitRepository::init(&path).unwrap();
    let objects = GitObjects::new(&repo);

    let content = b"fn main() { println!(\"hello\"); }";
    let oid = objects.write_blob(content).unwrap();

    // OID should be a 40-character hex string (SHA-1)
    assert_eq!(oid.len(), 40);
    assert!(oid.chars().all(|c| c.is_ascii_hexdigit()));

    let data = objects.read_blob(&oid).unwrap();
    assert_eq!(data, content);
}

#[test]
fn test_write_and_read_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test-repo");

    let repo = GitRepository::init(&path).unwrap();
    let objects = GitObjects::new(&repo);

    let content = b"pub fn hello() {}";
    objects
        .write_file(Path::new("src/lib.rs"), content)
        .unwrap();

    let data = objects.read_file(Path::new("src/lib.rs")).unwrap();
    assert_eq!(data, content);

    // Verify the file actually exists on disk
    assert!(path.join("src/lib.rs").exists());
}

#[test]
fn test_head_hash_empty_repo() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test-repo");

    let repo = GitRepository::init(&path).unwrap();
    assert!(repo.head_hash().unwrap().is_none());
}
