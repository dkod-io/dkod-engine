//! Tests for the submit handler's overlay materialization logic.
//!
//! The Phase 1 bug fix ensures that when agents use the MCP path
//! (dk_file_write -> dk_submit), files in the workspace overlay are
//! correctly materialized into `changeset_files` even though
//! `req.changes` is empty.
//!
//! These tests verify:
//! 1. Overlay entry -> operation mapping (add/modify/delete)
//! 2. Overlay snapshot captures content before workspace drop
//! 3. MCP path vs standard path branch selection
//! 4. Empty overlay + empty changes produces no file records

use dk_engine::workspace::overlay::{FileOverlay, OverlayEntry};
use dk_engine::workspace::session_workspace::{SessionWorkspace, WorkspaceMode};
use uuid::Uuid;

// ── Helper: simulate the overlay-to-changeset operation mapping ─────
//
// This mirrors the logic in submit.rs lines 178-195:
//
//   if req.changes.is_empty() && !overlay_snapshot.is_empty() {
//       for (path, entry) in &overlay_snapshot {
//           let (op, content) = match entry { ... };
//           engine.changeset_store().upsert_file(changeset_id, path, op, content.as_deref()) ...
//       }
//   }

fn overlay_entry_to_op_and_content(entry: &OverlayEntry) -> (&str, Option<String>) {
    match entry {
        OverlayEntry::Added { content, .. } => {
            ("add", Some(String::from_utf8_lossy(content).into_owned()))
        }
        OverlayEntry::Modified { content, .. } => {
            ("modify", Some(String::from_utf8_lossy(content).into_owned()))
        }
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
    let map: std::collections::HashMap<String, OverlayEntry> =
        changes.into_iter().collect();

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
    let req_changes = vec!["some_change"];
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());
    overlay.write_local("src/file.rs", b"content".to_vec(), true);

    let overlay_snapshot = overlay.list_changes();

    // Standard path: req.changes is NOT empty, so MCP branch is skipped
    let mcp_path = req_changes.is_empty() && !overlay_snapshot.is_empty();
    assert!(!mcp_path, "MCP path should NOT be triggered when req.changes is present");
}

#[tokio::test]
async fn no_path_when_both_empty() {
    // Neither req.changes nor overlay have entries
    let req_changes: Vec<()> = vec![];
    let overlay = FileOverlay::new_inmemory(Uuid::new_v4());

    let overlay_snapshot = overlay.list_changes();

    let mcp_path = req_changes.is_empty() && !overlay_snapshot.is_empty();
    assert!(!mcp_path, "MCP path should NOT be triggered when overlay is empty");
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
    ws.overlay.write_local("src/a.rs", b"fn a() {}".to_vec(), true);
    ws.overlay.write_local("src/b.rs", b"fn b() {}".to_vec(), false);
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
    assert_ne!(hash, hash3, "Different content should produce a different hash");
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

    ws1.overlay.write_local("shared.rs", b"from agent 1".to_vec(), true);
    ws2.overlay.write_local("shared.rs", b"from agent 2".to_vec(), true);

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
    assert_eq!(changes.len(), 1, "Should have exactly one entry after overwrite");

    let (path, entry) = &changes[0];
    assert_eq!(path, "src/main.rs");

    // The latest write wins: Modified with "version 2"
    let (op, content) = overlay_entry_to_op_and_content(entry);
    assert_eq!(op, "modify");
    assert_eq!(content.as_deref(), Some("version 2"));
}
