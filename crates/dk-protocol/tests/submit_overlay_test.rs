//! Tests for the submit handler's overlay materialization logic.
//!
//! The submit handler uses the overlay as the single source of truth
//! for changeset files. Each session has its own overlay (scoped by
//! workspace_id), so submit only captures files from the calling
//! session.
//!
//! These tests verify:
//! 1. Overlay entry -> operation mapping (add/modify/delete)
//! 2. Overlay snapshot captures content before workspace drop
//! 3. Unified path always uses overlay as source of truth
//! 4. Empty overlay produces no file records
//! 5. Two sessions writing to the same path only see their own files

use dk_engine::workspace::overlay::{FileOverlay, OverlayEntry};
use dk_engine::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};
use uuid::Uuid;

// ── Helper: simulate the overlay-to-changeset operation mapping ─────
//
// This mirrors the unified path in submit.rs that always uses the
// overlay as the source of truth for changeset files:
//
//   for (path, entry) in &overlay_snapshot {
//       let (op, content) = match entry { ... };
//       engine.changeset_store().upsert_file(changeset_id, path, op, content.as_deref()) ...
//   }

fn overlay_entry_to_op_and_content(entry: &OverlayEntry) -> (&str, Option<String>) {
    match entry {
        OverlayEntry::Added { content, .. } => {
            ("add", Some(String::from_utf8_lossy(content).into_owned()))
        }
        OverlayEntry::Modified { content, .. } => (
            "modify",
            Some(String::from_utf8_lossy(content).into_owned()),
        ),
        OverlayEntry::Deleted => ("delete", None),
    }
}

// ── Tests ───────────────────────────────────────────────────────────
//
// All tests use #[tokio::test] because FileOverlay::new_inmemory()
// internally creates a PgPool (connect_lazy_with) which requires a
// Tokio runtime context, even though no DB queries are executed.

#[tokio::test]
async fn overlay_added_entry_maps_to_add_operation() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/new_file.rs", b"fn main() {}".to_vec(), true);

    let changes = overlay.list_changes();
    assert_eq!(changes.len(), 1);

    let (path, entry) = &changes[0];
    assert_eq!(path, "src/new_file.rs");

    let (op, content) = overlay_entry_to_op_and_content(entry);
    assert_eq!(op, "add");
    assert_eq!(content.as_deref(), Some("fn main() {}"));
}

#[tokio::test]
async fn overlay_modified_entry_maps_to_modify_operation() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/lib.rs", b"pub mod updated;".to_vec(), false);

    let changes = overlay.list_changes();
    assert_eq!(changes.len(), 1);

    let (path, entry) = &changes[0];
    assert_eq!(path, "src/lib.rs");

    let (op, content) = overlay_entry_to_op_and_content(entry);
    assert_eq!(op, "modify");
    assert_eq!(content.as_deref(), Some("pub mod updated;"));
}

#[tokio::test]
async fn overlay_deleted_entry_maps_to_delete_operation() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.delete_local("src/obsolete.rs");

    let changes = overlay.list_changes();
    assert_eq!(changes.len(), 1);

    let (path, entry) = &changes[0];
    assert_eq!(path, "src/obsolete.rs");

    let (op, content) = overlay_entry_to_op_and_content(entry);
    assert_eq!(op, "delete");
    assert!(content.is_none());
}

#[tokio::test]
async fn overlay_mixed_entries_produce_correct_operations() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/new.rs", b"// new file".to_vec(), true);
    overlay.write_local("src/existing.rs", b"// modified".to_vec(), false);
    overlay.delete_local("src/removed.rs");

    let changes = overlay.list_changes();
    assert_eq!(changes.len(), 3);

    // Collect into a map for order-independent assertions
    let map: std::collections::HashMap<String, OverlayEntry> = changes.into_iter().collect();

    let (op, content) = overlay_entry_to_op_and_content(map.get("src/new.rs").unwrap());
    assert_eq!(op, "add");
    assert_eq!(content.as_deref(), Some("// new file"));

    let (op, content) = overlay_entry_to_op_and_content(map.get("src/existing.rs").unwrap());
    assert_eq!(op, "modify");
    assert_eq!(content.as_deref(), Some("// modified"));

    let (op, content) = overlay_entry_to_op_and_content(map.get("src/removed.rs").unwrap());
    assert_eq!(op, "delete");
    assert!(content.is_none());
}

#[tokio::test]
async fn empty_overlay_produces_no_changes() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    let changes = overlay.list_changes();
    assert!(changes.is_empty());
}

// ── MCP path branch selection tests ─────────────────────────────────
//
// These verify the conditional logic:
//   if req.changes.is_empty() && !overlay_snapshot.is_empty() { ... }

#[tokio::test]
async fn mcp_path_triggered_when_changes_empty_and_overlay_populated() {
    // Simulate: req.changes is empty, overlay has files (MCP path)
    let req_changes: Vec<()> = vec![];
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/agent_wrote.rs", b"content".to_vec(), true);

    let overlay_snapshot = overlay.list_changes();

    // This is the branch condition from submit.rs
    let mcp_path = req_changes.is_empty() && !overlay_snapshot.is_empty();
    assert!(mcp_path, "MCP path should be triggered");

    // Verify the snapshot has the expected content
    assert_eq!(overlay_snapshot.len(), 1);
    let (path, _entry) = &overlay_snapshot[0];
    assert_eq!(path, "src/agent_wrote.rs");
}

#[tokio::test]
async fn standard_path_when_changes_present() {
    // Simulate: req.changes has entries (standard protocol path)
    let req_changes: &[&str] = &["some_change"];
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/file.rs", b"content".to_vec(), true);

    let overlay_snapshot = overlay.list_changes();

    // Standard path: req.changes is NOT empty, so MCP branch is skipped
    let mcp_path = req_changes.is_empty() && !overlay_snapshot.is_empty();
    assert!(
        !mcp_path,
        "MCP path should NOT be triggered when req.changes is present"
    );
}

#[tokio::test]
async fn no_path_when_both_empty() {
    // Neither req.changes nor overlay have entries
    let req_changes: Vec<()> = vec![];
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());

    let overlay_snapshot = overlay.list_changes();

    let mcp_path = req_changes.is_empty() && !overlay_snapshot.is_empty();
    assert!(
        !mcp_path,
        "MCP path should NOT be triggered when overlay is empty"
    );
}

// ── Overlay snapshot timing test ────────────────────────────────────
//
// Verifies that list_changes() captures a snapshot of overlay state
// BEFORE the workspace guard is dropped. This is critical because the
// DashMap data must be read while we still hold the reference.

#[tokio::test]
async fn overlay_snapshot_captures_state_before_drop() {
    let session_id = Uuid::new_v4();
    let repo_id = Uuid::new_v4();

    let ws = SessionWorkspace::new_test(
        session_id,
        repo_id,
        "test-agent".into(),
        "test intent".into(),
        "abc123".into(),
        WorkspaceMode::Ephemeral,
    );

    // Write files to the overlay
    ws.overlay
        .write_local("src/a.rs", b"fn a() {}".to_vec(), true);
    ws.overlay
        .write_local("src/b.rs", b"fn b() {}".to_vec(), false);
    ws.overlay.delete_local("src/c.rs");

    // Snapshot BEFORE drop -- this mirrors submit.rs line 156:
    //   let overlay_snapshot = ws.overlay.list_changes();
    let overlay_snapshot = ws.overlay.list_changes();

    // Drop the workspace (simulates: drop(ws) on line 159)
    drop(ws);

    // The snapshot should still be valid after the workspace is dropped
    assert_eq!(overlay_snapshot.len(), 3);

    let map: std::collections::HashMap<String, OverlayEntry> =
        overlay_snapshot.into_iter().collect();

    // Verify Added entry
    match map.get("src/a.rs").unwrap() {
        OverlayEntry::Added { content, .. } => {
            assert_eq!(content, b"fn a() {}");
        }
        other => panic!("Expected Added, got {:?}", other),
    }

    // Verify Modified entry
    match map.get("src/b.rs").unwrap() {
        OverlayEntry::Modified { content, .. } => {
            assert_eq!(content, b"fn b() {}");
        }
        other => panic!("Expected Modified, got {:?}", other),
    }

    // Verify Deleted entry
    assert!(matches!(
        map.get("src/c.rs").unwrap(),
        OverlayEntry::Deleted
    ));
}

// ── Content fidelity tests ──────────────────────────────────────────

#[tokio::test]
async fn overlay_preserves_utf8_content_through_lossy_conversion() {
    // The submit handler uses String::from_utf8_lossy for content.
    // Verify that valid UTF-8 content round-trips correctly.
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    let source = "fn hello() -> &'static str { \"world\" }";
    overlay.write_local("src/hello.rs", source.as_bytes().to_vec(), true);

    let changes = overlay.list_changes();
    let (_path, entry) = &changes[0];
    let (op, content) = overlay_entry_to_op_and_content(entry);

    assert_eq!(op, "add");
    assert_eq!(content.as_deref(), Some(source));
}

#[tokio::test]
async fn overlay_write_local_produces_consistent_hash() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    let content = b"fn main() {}";
    let hash = overlay.write_local("src/main.rs", content.to_vec(), true);

    // Verify the hash is non-empty and deterministic (same content -> same hash)
    assert!(!hash.is_empty());
    let hash2 = overlay.write_local("src/main2.rs", content.to_vec(), true);
    assert_eq!(hash, hash2, "Same content should produce the same hash");

    // Different content should produce a different hash
    let hash3 = overlay.write_local("src/other.rs", b"fn other() {}".to_vec(), true);
    assert_ne!(
        hash, hash3,
        "Different content should produce a different hash"
    );
}

// ── changed_files population test ───────────────────────────────────
//
// In the MCP path, overlay entries should also populate the
// changed_files vec so re-indexing happens on those files.

#[tokio::test]
async fn mcp_path_populates_changed_files_from_overlay() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/new.rs", b"new content".to_vec(), true);
    overlay.write_local("src/mod.rs", b"modified".to_vec(), false);
    overlay.delete_local("src/old.rs");

    let req_changes: Vec<()> = vec![];
    let overlay_snapshot = overlay.list_changes();

    // Simulate the changed_files population from submit.rs lines 178-195
    let mut changed_files = Vec::new();
    if req_changes.is_empty() && !overlay_snapshot.is_empty() {
        for (path, _entry) in &overlay_snapshot {
            changed_files.push(std::path::PathBuf::from(path));
        }
    }

    assert_eq!(changed_files.len(), 3);
    let paths: std::collections::HashSet<String> = changed_files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains("src/new.rs"));
    assert!(paths.contains("src/mod.rs"));
    assert!(paths.contains("src/old.rs"));
}

// ── Workspace overlay isolation test ────────────────────────────────
//
// Two workspaces should have independent overlays.

#[tokio::test]
async fn workspace_overlays_are_isolated() {
    let ws1 = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-1".into(),
        "intent 1".into(),
        "commit1".into(),
        WorkspaceMode::Ephemeral,
    );
    let ws2 = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-2".into(),
        "intent 2".into(),
        "commit2".into(),
        WorkspaceMode::Ephemeral,
    );

    ws1.overlay
        .write_local("shared.rs", b"from agent 1".to_vec(), true);
    ws2.overlay
        .write_local("shared.rs", b"from agent 2".to_vec(), true);

    let snap1 = ws1.overlay.list_changes();
    let snap2 = ws2.overlay.list_changes();

    assert_eq!(snap1.len(), 1);
    assert_eq!(snap2.len(), 1);

    match &snap1[0].1 {
        OverlayEntry::Added { content, .. } => assert_eq!(content, b"from agent 1"),
        other => panic!("Expected Added, got {:?}", other),
    }
    match &snap2[0].1 {
        OverlayEntry::Added { content, .. } => assert_eq!(content, b"from agent 2"),
        other => panic!("Expected Added, got {:?}", other),
    }
}

// ── Overwrite semantics test ────────────────────────────────────────
//
// Verify that writing to the same path overwrites the previous entry.
// This is important for the MCP path where agents may dk_file_write
// multiple times to the same file before dk_submit.

#[tokio::test]
async fn overlay_write_overwrites_previous_entry() {
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());

    // First write: add
    overlay.write_local("src/main.rs", b"version 1".to_vec(), true);
    // Second write: modify (overwrite)
    overlay.write_local("src/main.rs", b"version 2".to_vec(), false);

    let changes = overlay.list_changes();
    assert_eq!(
        changes.len(),
        1,
        "Should have exactly one entry after overwrite"
    );

    let (path, entry) = &changes[0];
    assert_eq!(path, "src/main.rs");

    // The latest write wins: Modified with "version 2"
    let (op, content) = overlay_entry_to_op_and_content(entry);
    assert_eq!(op, "modify");
    assert_eq!(content.as_deref(), Some("version 2"));
}

// ── Submit isolation test ───────────────────────────────────────────
//
// Simulates two sessions writing to overlapping files, then each
// "submitting". Each session's overlay snapshot must only contain that
// session's files — never files from the other session.

#[tokio::test]
async fn submit_overlay_isolation_two_sessions_same_file() {
    // Session A writes "src/shared.rs" and "src/a_only.rs"
    let ws_a = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-a".into(),
        "intent A".into(),
        "base_commit".into(),
        WorkspaceMode::Ephemeral,
    );
    ws_a.overlay
        .write_local("src/shared.rs", b"content from A".to_vec(), true);
    ws_a.overlay
        .write_local("src/a_only.rs", b"only in A".to_vec(), true);

    // Session B writes "src/shared.rs" and "src/b_only.rs"
    let ws_b = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-b".into(),
        "intent B".into(),
        "base_commit".into(),
        WorkspaceMode::Ephemeral,
    );
    ws_b.overlay
        .write_local("src/shared.rs", b"content from B".to_vec(), true);
    ws_b.overlay
        .write_local("src/b_only.rs", b"only in B".to_vec(), true);

    // Simulate submit for session A: snapshot its overlay
    let snap_a = ws_a.overlay.list_changes();
    // Simulate submit for session B: snapshot its overlay
    let snap_b = ws_b.overlay.list_changes();

    // Session A should see exactly 2 files (its own)
    assert_eq!(
        snap_a.len(),
        2,
        "Session A should see exactly its own 2 files"
    );
    let paths_a: std::collections::HashSet<String> =
        snap_a.iter().map(|(p, _)| p.clone()).collect();
    assert!(paths_a.contains("src/shared.rs"));
    assert!(paths_a.contains("src/a_only.rs"));
    assert!(
        !paths_a.contains("src/b_only.rs"),
        "Session A must not see B's files"
    );

    // Session B should see exactly 2 files (its own)
    assert_eq!(
        snap_b.len(),
        2,
        "Session B should see exactly its own 2 files"
    );
    let paths_b: std::collections::HashSet<String> =
        snap_b.iter().map(|(p, _)| p.clone()).collect();
    assert!(paths_b.contains("src/shared.rs"));
    assert!(paths_b.contains("src/b_only.rs"));
    assert!(
        !paths_b.contains("src/a_only.rs"),
        "Session B must not see A's files"
    );

    // Verify content isolation: "src/shared.rs" has different content per session
    let a_shared = snap_a.iter().find(|(p, _)| p == "src/shared.rs").unwrap();
    let b_shared = snap_b.iter().find(|(p, _)| p == "src/shared.rs").unwrap();

    match &a_shared.1 {
        OverlayEntry::Added { content, .. } => {
            assert_eq!(content, b"content from A");
        }
        other => panic!("Expected Added for A, got {:?}", other),
    }
    match &b_shared.1 {
        OverlayEntry::Added { content, .. } => {
            assert_eq!(content, b"content from B");
        }
        other => panic!("Expected Added for B, got {:?}", other),
    }
}

#[tokio::test]
async fn submit_unified_path_records_all_overlay_entries() {
    // Verifies the unified path: regardless of whether req.changes is
    // empty or not, the overlay snapshot is always used for changeset files.
    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent".into(),
        "intent".into(),
        "commit".into(),
        WorkspaceMode::Ephemeral,
    );

    // Simulate MCP writes (no req.changes)
    ws.overlay
        .write_local("src/new.rs", b"new file".to_vec(), true);
    ws.overlay
        .write_local("src/existing.rs", b"modified".to_vec(), false);
    ws.overlay.delete_local("src/removed.rs");

    let overlay_snapshot = ws.overlay.list_changes();
    drop(ws);

    // Simulate the unified path: iterate overlay snapshot and collect ops
    let mut recorded: Vec<(String, &str, Option<String>)> = Vec::new();
    for (path, entry) in &overlay_snapshot {
        let (op, content) = overlay_entry_to_op_and_content(entry);
        recorded.push((path.clone(), op, content));
    }

    assert_eq!(recorded.len(), 3);

    let map: std::collections::HashMap<String, (&str, Option<String>)> = recorded
        .into_iter()
        .map(|(p, op, c)| (p, (op, c)))
        .collect();

    assert_eq!(map["src/new.rs"].0, "add");
    assert_eq!(map["src/new.rs"].1.as_deref(), Some("new file"));
    assert_eq!(map["src/existing.rs"].0, "modify");
    assert_eq!(map["src/existing.rs"].1.as_deref(), Some("modified"));
    assert_eq!(map["src/removed.rs"].0, "delete");
    assert!(map["src/removed.rs"].1.is_none());
}
