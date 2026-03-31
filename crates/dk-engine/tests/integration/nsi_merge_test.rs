//! Integration tests for workspace merge operations (NSI).
//!
//! These tests verify `merge_workspace` for fast-forward, rebase, and
//! empty-overlay scenarios, using in-memory overlays and temp git repos.

use std::path::Path;

use dk_engine::git::{GitObjects, GitRepository};
use dk_engine::parser::ParserRegistry;
use dk_engine::workspace::merge::{merge_workspace, WorkspaceMergeResult};
use dk_engine::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};
use tempfile::TempDir;
use uuid::Uuid;

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

/// Add a new file and create a second commit (advancing HEAD past the base).
fn advance_repo(
    repo: &GitRepository,
    filename: &str,
    content: &[u8],
    message: &str,
) -> String {
    let objects = GitObjects::new(repo);
    objects
        .write_file(Path::new(filename), content)
        .expect("write file");

    repo.commit(message, "test", "test@test.com")
        .expect("commit")
}

// ═══════════════════════════════════════════════════════════════════
// Test 1: Fast-forward merge — HEAD == base_commit
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_fast_forward_merge() {
    let (repo, commit, _tmp) = init_repo_with_file("src/main.rs", b"fn main() {}");

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-merge".into(),
        "fast forward test".into(),
        commit.clone(),
        WorkspaceMode::Ephemeral,
    );

    // Write a new file via overlay.
    ws.overlay
        .write_local("src/helper.rs", b"pub fn help() {}".to_vec(), true);

    let parser = ParserRegistry::new();

    let result = merge_workspace(
        &ws,
        &repo,
        &parser,
        "add helper",
        "test",
        "test@test.com",
    )
    .expect("merge should succeed");

    match result {
        WorkspaceMergeResult::FastMerge { commit_hash } => {
            // Verify commit was created.
            assert!(!commit_hash.is_empty());
            assert_ne!(commit_hash, commit, "new commit should differ from base");

            // Verify overlay content now appears in the repo tree.
            let content = repo
                .read_tree_entry(&commit_hash, "src/helper.rs")
                .expect("helper.rs should exist in new commit");
            assert_eq!(content, b"pub fn help() {}");

            // Original file should still be present.
            let original = repo
                .read_tree_entry(&commit_hash, "src/main.rs")
                .expect("main.rs should still exist");
            assert_eq!(original, b"fn main() {}");
        }
        other => panic!("expected FastMerge, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Test 2: Rebase merge — HEAD advanced, no conflict (different files)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_rebase_merge_no_conflict() {
    let (repo, base_commit, _tmp) = init_repo_with_file("src/main.rs", b"fn main() {}");

    // Create a workspace pinned to the initial commit.
    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-rebase".into(),
        "rebase test".into(),
        base_commit.clone(),
        WorkspaceMode::Ephemeral,
    );

    // Advance the repo HEAD with a different file.
    let _head_commit = advance_repo(&repo, "src/other.rs", b"pub fn other() {}", "add other");

    // Workspace modifies a different file via overlay.
    ws.overlay
        .write_local("src/helper.rs", b"pub fn help() {}".to_vec(), true);

    let parser = ParserRegistry::new();

    let result = merge_workspace(
        &ws,
        &repo,
        &parser,
        "add helper after rebase",
        "test",
        "test@test.com",
    )
    .expect("merge should succeed");

    match result {
        WorkspaceMergeResult::RebaseMerge {
            commit_hash,
            auto_rebased_files,
        } => {
            assert!(!commit_hash.is_empty());
            // helper.rs was not in base or head, so it's a pure addition — no rebase needed.
            // auto_rebased_files should be empty since the file didn't exist in both.
            assert!(
                auto_rebased_files.is_empty(),
                "pure additions should not appear in auto_rebased_files"
            );

            // Verify all files present in the new commit.
            let helper = repo
                .read_tree_entry(&commit_hash, "src/helper.rs")
                .expect("helper.rs should exist");
            assert_eq!(helper, b"pub fn help() {}");
        }
        other => panic!("expected RebaseMerge, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Test 3: Merge with empty overlay — should be rejected
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_merge_empty_overlay_rejected() {
    let (repo, commit, _tmp) = init_repo_with_file("src/main.rs", b"fn main() {}");

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-empty".into(),
        "empty merge test".into(),
        commit,
        WorkspaceMode::Ephemeral,
    );

    // Do NOT write anything to the overlay.
    let parser = ParserRegistry::new();

    let result = merge_workspace(
        &ws,
        &repo,
        &parser,
        "empty merge",
        "test",
        "test@test.com",
    );

    assert!(
        result.is_err(),
        "merge with empty overlay should return an error"
    );

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("no changes"),
        "error should mention 'no changes', got: {err_msg}"
    );
}
