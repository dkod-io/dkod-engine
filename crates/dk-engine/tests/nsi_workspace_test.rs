//! Integration tests for Native Session Isolation (NSI).
//!
//! These tests verify that workspace isolation works correctly WITHOUT
//! requiring a running PostgreSQL database. They exercise the overlay,
//! session graph, and workspace layers using in-memory-only helpers.
//!
//! Tests that create `SessionWorkspace` instances use `#[tokio::test]`
//! because the underlying `FileOverlay::new_inmemory()` constructs a
//! lazy `PgPool` which requires a Tokio runtime context.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use dk_core::{CallEdge, CallKind, Span, Symbol, SymbolKind, Visibility};
use dk_engine::git::{GitObjects, GitRepository};
use dk_engine::workspace::session_graph::SessionGraph;
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

/// Build a test symbol with the given qualified name and a random ID.
fn make_symbol(name: &str) -> Symbol {
    Symbol {
        id: Uuid::new_v4(),
        name: name.to_string(),
        qualified_name: name.to_string(),
        kind: SymbolKind::Function,
        visibility: Visibility::Public,
        file_path: std::path::PathBuf::from("test.rs"),
        span: Span {
            start_byte: 0,
            end_byte: 10,
        },
        signature: None,
        doc_comment: None,
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    }
}

/// Build a test symbol at a specific ID.
fn make_symbol_with_id(id: Uuid, name: &str) -> Symbol {
    Symbol {
        id,
        name: name.to_string(),
        qualified_name: name.to_string(),
        kind: SymbolKind::Function,
        visibility: Visibility::Public,
        file_path: std::path::PathBuf::from("test.rs"),
        span: Span {
            start_byte: 0,
            end_byte: 10,
        },
        signature: None,
        doc_comment: None,
        parent: None,
        last_modified_by: None,
        last_modified_intent: None,
    }
}

// ═══════════════════════════════════════════════════════════════════
// Test 1: Two sessions see isolated file views
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_two_sessions_see_isolated_views() {
    // 1. Create a temp git repo with an initial commit containing a file.
    let original_content = b"fn main() { println!(\"hello\"); }";
    let (repo, commit, _tmp) = init_repo_with_file("src/main.rs", original_content);

    // 2. Create two workspaces pinned to the same base commit.
    let repo_id = Uuid::new_v4();
    let ws_a = SessionWorkspace::new_test(
        Uuid::new_v4(),
        repo_id,
        "agent-a".into(),
        "refactor main".into(),
        commit.clone(),
        WorkspaceMode::Ephemeral,
    );
    let ws_b = SessionWorkspace::new_test(
        Uuid::new_v4(),
        repo_id,
        "agent-b".into(),
        "add feature".into(),
        commit.clone(),
        WorkspaceMode::Ephemeral,
    );

    // 3. Session A writes a modified version of the file via the overlay.
    let modified_content = b"fn main() { println!(\"goodbye\"); }";
    ws_a.overlay
        .write_local("src/main.rs", modified_content.to_vec(), false);

    // 4. Session B reads the same file -- sees the ORIGINAL content
    //    from the git tree, NOT session A's changes.
    let read_b = ws_b
        .read_file("src/main.rs", &repo)
        .expect("session B should read from git tree");
    assert_eq!(
        read_b.content, original_content,
        "Session B must see the original git content, not A's overlay"
    );
    assert!(
        !read_b.modified_in_session,
        "Session B has not modified this file"
    );

    // 5. Session A reads the file -- sees its OWN overlay content.
    let read_a = ws_a
        .read_file("src/main.rs", &repo)
        .expect("session A should read from its overlay");
    assert_eq!(
        read_a.content,
        modified_content.to_vec(),
        "Session A must see its own overlay content"
    );
    assert!(
        read_a.modified_in_session,
        "Session A has modified this file"
    );

    // 6. Verify overlay isolation: A's overlay has 1 entry, B's has 0.
    assert_eq!(ws_a.overlay.len(), 1);
    assert_eq!(ws_b.overlay.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════
// Test 2: Overlay content takes priority over base tree
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_overlay_priority_over_base() {
    let base_content = b"pub fn add(a: i32, b: i32) -> i32 { a + b }";
    let (repo, commit, _tmp) = init_repo_with_file("src/lib.rs", base_content);

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-x".into(),
        "optimize add".into(),
        commit,
        WorkspaceMode::Ephemeral,
    );

    // Before overlay write -- read should return base content.
    let before = ws
        .read_file("src/lib.rs", &repo)
        .expect("read from git tree");
    assert_eq!(before.content, base_content.to_vec());
    assert!(!before.modified_in_session);

    // Write new content via overlay.
    let overlay_content = b"pub fn add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }";
    ws.overlay
        .write_local("src/lib.rs", overlay_content.to_vec(), false);

    // After overlay write -- read should return overlay content.
    let after = ws
        .read_file("src/lib.rs", &repo)
        .expect("read from overlay");
    assert_eq!(after.content, overlay_content.to_vec());
    assert!(after.modified_in_session);
}

// ═══════════════════════════════════════════════════════════════════
// Test 2b: Overlay deletion hides file from workspace
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_overlay_deletion_hides_base_file() {
    let content = b"// to be deleted";
    let (repo, commit, _tmp) = init_repo_with_file("obsolete.rs", content);

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-d".into(),
        "cleanup".into(),
        commit,
        WorkspaceMode::Ephemeral,
    );

    // File is readable before deletion.
    assert!(ws.read_file("obsolete.rs", &repo).is_ok());

    // Mark as deleted in the overlay.
    ws.overlay.delete_local("obsolete.rs");

    // Now reading should fail -- the file is "deleted" in this session.
    let result = ws.read_file("obsolete.rs", &repo);
    assert!(result.is_err(), "deleted file should not be readable");
}

// ═══════════════════════════════════════════════════════════════════
// Test 2c: Overlay-added file visible even if not in base tree
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_overlay_new_file_visible() {
    let (repo, commit, _tmp) = init_repo_with_file("existing.rs", b"// exists");

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-n".into(),
        "add module".into(),
        commit,
        WorkspaceMode::Ephemeral,
    );

    // "brand_new.rs" doesn't exist in the base tree.
    assert!(ws.read_file("brand_new.rs", &repo).is_err());

    // Add it via overlay.
    let new_content = b"pub fn brand_new() {}";
    ws.overlay
        .write_local("brand_new.rs", new_content.to_vec(), true);

    // Now it should be readable.
    let result = ws.read_file("brand_new.rs", &repo).expect("new file");
    assert_eq!(result.content, new_content.to_vec());
    assert!(result.modified_in_session);
}

// ═══════════════════════════════════════════════════════════════════
// Test 3: SessionGraph delta isolation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_session_graph_delta_isolation() {
    // 1. Build a shared base with two symbols.
    let sym_base_1 = make_symbol("base::parse");
    let sym_base_2 = make_symbol("base::render");
    let id1 = sym_base_1.id;
    let id2 = sym_base_2.id;

    let mut base_map = HashMap::new();
    base_map.insert(id1, sym_base_1);
    base_map.insert(id2, sym_base_2);

    let shared_base = Arc::new(ArcSwap::from_pointee(base_map));

    // 2. Fork two session graphs from the same shared base.
    let graph_a = SessionGraph::fork_from(Arc::clone(&shared_base));
    let graph_b = SessionGraph::fork_from(Arc::clone(&shared_base));

    // Both can see base symbols.
    assert!(
        graph_a.get_symbol(id1).is_some(),
        "Graph A should see base::parse"
    );
    assert!(
        graph_a.get_symbol(id2).is_some(),
        "Graph A should see base::render"
    );
    assert!(
        graph_b.get_symbol(id1).is_some(),
        "Graph B should see base::parse"
    );
    assert!(
        graph_b.get_symbol(id2).is_some(),
        "Graph B should see base::render"
    );

    // 3. Session A adds a new symbol.
    let sym_a_only = make_symbol("session_a::new_helper");
    let id_a = sym_a_only.id;
    graph_a.add_symbol(sym_a_only);

    // 4. Session A can see its own symbol.
    assert!(
        graph_a.get_symbol(id_a).is_some(),
        "Graph A should see its own added symbol"
    );

    // 5. Session B CANNOT see A's symbol.
    assert!(
        graph_b.get_symbol(id_a).is_none(),
        "Graph B must NOT see session A's symbol"
    );

    // 6. Session B adds its own symbol.
    let sym_b_only = make_symbol("session_b::another_helper");
    let id_b = sym_b_only.id;
    graph_b.add_symbol(sym_b_only);

    // B sees its own, A does not.
    assert!(graph_b.get_symbol(id_b).is_some());
    assert!(graph_a.get_symbol(id_b).is_none());

    // 7. Both still see the shared base symbols.
    assert!(graph_a.get_symbol(id1).is_some());
    assert!(graph_b.get_symbol(id1).is_some());

    // 8. Change counts reflect only per-session deltas.
    assert_eq!(graph_a.change_count(), 1, "A added one symbol");
    assert_eq!(graph_b.change_count(), 1, "B added one symbol");
}

// ═══════════════════════════════════════════════════════════════════
// Test 3b: SessionGraph removal isolation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_session_graph_removal_isolation() {
    let sym = make_symbol("shared::func");
    let id = sym.id;

    let mut base_map = HashMap::new();
    base_map.insert(id, sym);
    let shared_base = Arc::new(ArcSwap::from_pointee(base_map));

    let graph_a = SessionGraph::fork_from(Arc::clone(&shared_base));
    let graph_b = SessionGraph::fork_from(Arc::clone(&shared_base));

    // Session A removes the base symbol.
    graph_a.remove_symbol(id);

    // A cannot see it, B still can.
    assert!(
        graph_a.get_symbol(id).is_none(),
        "A removed it from its view"
    );
    assert!(
        graph_b.get_symbol(id).is_some(),
        "B still sees the base symbol"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 3c: SessionGraph modification isolation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_session_graph_modification_isolation() {
    let sym = make_symbol("shared::compute");
    let id = sym.id;

    let mut base_map = HashMap::new();
    base_map.insert(id, sym);
    let shared_base = Arc::new(ArcSwap::from_pointee(base_map));

    let graph_a = SessionGraph::fork_from(Arc::clone(&shared_base));
    let graph_b = SessionGraph::fork_from(Arc::clone(&shared_base));

    // A modifies the symbol (changes its signature).
    let mut modified = make_symbol_with_id(id, "shared::compute");
    modified.signature = Some("fn compute(x: f64) -> f64".into());
    graph_a.modify_symbol(modified);

    // A sees the modified version.
    let from_a = graph_a.get_symbol(id).expect("A sees it");
    assert_eq!(
        from_a.signature,
        Some("fn compute(x: f64) -> f64".to_string())
    );

    // B sees the original (no signature).
    let from_b = graph_b.get_symbol(id).expect("B sees it");
    assert!(from_b.signature.is_none(), "B should see original");
}

// ═══════════════════════════════════════════════════════════════════
// Test 3d: SessionGraph edge isolation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_session_graph_edge_isolation() {
    let graph_a = SessionGraph::empty();
    let graph_b = SessionGraph::empty();

    let edge = CallEdge {
        id: Uuid::new_v4(),
        repo_id: Uuid::new_v4(),
        caller: Uuid::new_v4(),
        callee: Uuid::new_v4(),
        kind: CallKind::DirectCall,
    };
    let edge_id = edge.id;

    // A adds an edge.
    graph_a.add_edge(edge);

    // Each graph owns its own DashMap, so edges cannot leak across sessions.
    // Remove the edge from A to verify clean removal.
    graph_a.remove_edge(edge_id);

    // B sees 0 changes (no symbols or edges from A).
    assert_eq!(graph_b.change_count(), 0);
}

// ═══════════════════════════════════════════════════════════════════
// Test 4: Workspace lifecycle (create, retrieve, destroy)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_workspace_manager_lifecycle() {
    // WorkspaceManager requires a PgPool which we cannot construct without
    // a real DB. Instead, we test the conceptual lifecycle using DashMap
    // directly, mirroring the WorkspaceManager's behavior.

    use dashmap::DashMap;

    let workspaces: DashMap<Uuid, SessionWorkspace> = DashMap::new();

    let session_id = Uuid::new_v4();
    let repo_id = Uuid::new_v4();

    // 1. Create a workspace and register it.
    let ws = SessionWorkspace::new_test(
        session_id,
        repo_id,
        "agent-lifecycle".into(),
        "test lifecycle".into(),
        "deadbeef".into(),
        WorkspaceMode::Ephemeral,
    );

    workspaces.insert(session_id, ws);

    // 2. Verify it is retrievable.
    assert!(
        workspaces.get(&session_id).is_some(),
        "workspace should be retrievable after insertion"
    );
    {
        let ws_ref = workspaces.get(&session_id).unwrap();
        assert_eq!(ws_ref.repo_id, repo_id);
        assert_eq!(ws_ref.agent_id, "agent-lifecycle");
        assert_eq!(ws_ref.intent, "test lifecycle");
    }

    // 3. Destroy the workspace.
    let removed = workspaces.remove(&session_id);
    assert!(removed.is_some(), "workspace should be removable");

    // 4. Verify it is gone.
    assert!(
        workspaces.get(&session_id).is_none(),
        "workspace should be gone after destruction"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 4b: Multiple workspaces for the same repo
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_multiple_workspaces_same_repo() {
    use dashmap::DashMap;

    let workspaces: DashMap<Uuid, SessionWorkspace> = DashMap::new();
    let repo_id = Uuid::new_v4();

    let sid_1 = Uuid::new_v4();
    let sid_2 = Uuid::new_v4();
    let sid_3 = Uuid::new_v4();

    for (sid, agent) in [(sid_1, "agent-1"), (sid_2, "agent-2"), (sid_3, "agent-3")] {
        let ws = SessionWorkspace::new_test(
            sid,
            repo_id,
            agent.into(),
            "work".into(),
            "abc123".into(),
            WorkspaceMode::Ephemeral,
        );
        workspaces.insert(sid, ws);
    }

    // Count active workspaces for this repo.
    let count = workspaces
        .iter()
        .filter(|e| e.value().repo_id == repo_id)
        .count();
    assert_eq!(count, 3);

    // Destroy one.
    workspaces.remove(&sid_2);

    let count = workspaces
        .iter()
        .filter(|e| e.value().repo_id == repo_id)
        .count();
    assert_eq!(count, 2);
}

// ═══════════════════════════════════════════════════════════════════
// Test 5: list_files merges overlay with base tree
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_list_files_merges_overlay_with_base() {
    let (repo, commit, _tmp) = init_repo_with_file("src/main.rs", b"fn main() {}");

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-list".into(),
        "list files test".into(),
        commit,
        WorkspaceMode::Ephemeral,
    );

    // Base tree has "src/main.rs".
    let base_files = ws.list_files(&repo, false, None).expect("list base files");
    assert!(base_files.contains(&"src/main.rs".to_string()));

    // Add a new file via overlay.
    ws.overlay
        .write_local("src/util.rs", b"pub fn util() {}".to_vec(), true);

    // Full listing should include both base and overlay files.
    let all_files = ws.list_files(&repo, false, None).expect("list all files");
    assert!(all_files.contains(&"src/main.rs".to_string()));
    assert!(all_files.contains(&"src/util.rs".to_string()));

    // only_modified listing should return just the overlay file.
    let modified = ws.list_files(&repo, true, None).expect("list modified");
    assert_eq!(modified.len(), 1);
    assert!(modified.contains(&"src/util.rs".to_string()));

    // Delete the base file via overlay.
    ws.overlay.delete_local("src/main.rs");

    // Full listing should no longer include the deleted file.
    let after_delete = ws.list_files(&repo, false, None).expect("list after delete");
    assert!(
        !after_delete.contains(&"src/main.rs".to_string()),
        "deleted file should be excluded"
    );
    assert!(after_delete.contains(&"src/util.rs".to_string()));
}

// ═══════════════════════════════════════════════════════════════════
// Test 6: Full cross-session integration (file + graph)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_full_cross_session_isolation() {
    // Setup: repo with a Rust file.
    let initial = b"pub fn greet() { println!(\"hi\"); }";
    let (repo, commit, _tmp) = init_repo_with_file("src/lib.rs", initial);

    let repo_id = Uuid::new_v4();

    // Two workspaces from the same base.
    let ws_a = SessionWorkspace::new_test(
        Uuid::new_v4(),
        repo_id,
        "agent-a".into(),
        "change greet".into(),
        commit.clone(),
        WorkspaceMode::Ephemeral,
    );
    let ws_b = SessionWorkspace::new_test(
        Uuid::new_v4(),
        repo_id,
        "agent-b".into(),
        "add farewell".into(),
        commit.clone(),
        WorkspaceMode::Ephemeral,
    );

    // Build shared symbol base with one symbol.
    let base_sym = make_symbol("greet");
    let base_sym_id = base_sym.id;
    let mut base_map = HashMap::new();
    base_map.insert(base_sym_id, base_sym);
    let shared = Arc::new(ArcSwap::from_pointee(base_map));

    // Fork session graphs from the same base.
    let graph_a = SessionGraph::fork_from(Arc::clone(&shared));
    let graph_b = SessionGraph::fork_from(Arc::clone(&shared));

    // -- File layer isolation --
    // Session A modifies src/lib.rs.
    ws_a.overlay.write_local(
        "src/lib.rs",
        b"pub fn greet() { println!(\"hello world\"); }".to_vec(),
        false,
    );

    // Session B adds a new file.
    ws_b.overlay.write_local(
        "src/farewell.rs",
        b"pub fn farewell() { println!(\"bye\"); }".to_vec(),
        true,
    );

    // A sees its modified lib.rs, B sees original.
    let a_lib = ws_a.read_file("src/lib.rs", &repo).unwrap();
    assert!(a_lib.modified_in_session);
    let b_lib = ws_b.read_file("src/lib.rs", &repo).unwrap();
    assert!(!b_lib.modified_in_session);
    assert_eq!(b_lib.content, initial.to_vec());

    // B sees farewell.rs, A does not.
    assert!(ws_b.read_file("src/farewell.rs", &repo).is_ok());
    assert!(ws_a.read_file("src/farewell.rs", &repo).is_err());

    // -- Graph layer isolation --
    // A modifies the base symbol.
    let mut modified_greet = make_symbol_with_id(base_sym_id, "greet");
    modified_greet.signature = Some("fn greet()".into());
    graph_a.modify_symbol(modified_greet);

    // B adds a new symbol.
    let farewell_sym = make_symbol("farewell");
    let farewell_id = farewell_sym.id;
    graph_b.add_symbol(farewell_sym);

    // Cross checks.
    assert!(graph_a.get_symbol(farewell_id).is_none());
    assert!(graph_b.get_symbol(farewell_id).is_some());

    let a_greet = graph_a.get_symbol(base_sym_id).unwrap();
    assert_eq!(a_greet.signature, Some("fn greet()".to_string()));

    let b_greet = graph_b.get_symbol(base_sym_id).unwrap();
    assert!(b_greet.signature.is_none(), "B sees original greet");
}

// ═══════════════════════════════════════════════════════════════════
// Test 7: Event bus cross-session awareness
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_event_bus_cross_session_notification() {
    use dk_engine::workspace::event_bus::{RepoEventBus, SessionEvent};

    let bus = RepoEventBus::new();
    let repo_id = Uuid::new_v4();

    // Two sessions subscribe to the same repo.
    let mut rx_a = bus.subscribe(repo_id);
    let mut rx_b = bus.subscribe(repo_id);

    assert_eq!(bus.subscriber_count(repo_id), 2);

    // Session A publishes a FileModified event.
    let session_a_id = Uuid::new_v4();
    bus.publish(
        repo_id,
        SessionEvent::FileModified {
            session_id: session_a_id,
            file_path: "src/main.rs".into(),
        },
    );

    // Both receivers should get the event.
    let event_a = rx_a.recv().await.expect("rx_a receives");
    let event_b = rx_b.recv().await.expect("rx_b receives");

    match (&event_a, &event_b) {
        (
            SessionEvent::FileModified {
                session_id: sid_a,
                file_path: path_a,
            },
            SessionEvent::FileModified {
                session_id: sid_b,
                file_path: path_b,
            },
        ) => {
            assert_eq!(*sid_a, session_a_id);
            assert_eq!(*sid_b, session_a_id);
            assert_eq!(path_a, "src/main.rs");
            assert_eq!(path_b, "src/main.rs");
        }
        _ => panic!("expected FileModified events"),
    }

    // Different repo gets its own channel -- no cross-contamination.
    let other_repo = Uuid::new_v4();
    let mut rx_other = bus.subscribe(other_repo);

    bus.publish(
        repo_id,
        SessionEvent::ChangesetMerged {
            session_id: session_a_id,
            commit_hash: "abc123".into(),
        },
    );

    // rx_other should NOT receive this event (different repo).
    // We verify by publishing an event on other_repo and confirming
    // it is the first thing rx_other sees.
    bus.publish(
        other_repo,
        SessionEvent::SessionCreated {
            session_id: Uuid::new_v4(),
            agent_id: "other-agent".into(),
            intent: "other work".into(),
        },
    );

    let other_event = rx_other.recv().await.expect("other repo event");
    match other_event {
        SessionEvent::SessionCreated { agent_id, .. } => {
            assert_eq!(agent_id, "other-agent");
        }
        _ => panic!("expected SessionCreated on other repo, not cross-contamination"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Test 8: WorkspaceManager GC expired persistent workspaces
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_workspace_manager_gc_expired() {
    use dashmap::DashMap;
    use std::time::Duration;
    use tokio::time::Instant;

    let workspaces: DashMap<Uuid, SessionWorkspace> = DashMap::new();

    // Create a persistent workspace with an already-expired deadline.
    let sid_expired = Uuid::new_v4();
    let repo_id = Uuid::new_v4();
    let mut ws_expired = SessionWorkspace::new_test(
        sid_expired,
        repo_id,
        "agent-expired".into(),
        "expired work".into(),
        "abc123".into(),
        WorkspaceMode::Persistent {
            expires_at: Some(Instant::now()), // expires immediately
        },
    );
    // Force the mode to an already-past deadline.
    ws_expired.mode = WorkspaceMode::Persistent {
        expires_at: Some(Instant::now() - Duration::from_secs(10)),
    };
    workspaces.insert(sid_expired, ws_expired);

    // Create a persistent workspace that has NOT expired.
    let sid_active = Uuid::new_v4();
    let ws_active = SessionWorkspace::new_test(
        sid_active,
        repo_id,
        "agent-active".into(),
        "active work".into(),
        "abc123".into(),
        WorkspaceMode::Persistent {
            expires_at: Some(Instant::now() + Duration::from_secs(3600)),
        },
    );
    workspaces.insert(sid_active, ws_active);

    // Create an ephemeral workspace (should never be GC'd by this logic).
    let sid_ephemeral = Uuid::new_v4();
    let ws_ephemeral = SessionWorkspace::new_test(
        sid_ephemeral,
        repo_id,
        "agent-ephemeral".into(),
        "ephemeral work".into(),
        "abc123".into(),
        WorkspaceMode::Ephemeral,
    );
    workspaces.insert(sid_ephemeral, ws_ephemeral);

    assert_eq!(workspaces.len(), 3);

    // Simulate GC: collect expired persistent workspaces (same logic as WorkspaceManager::gc_expired).
    let now = Instant::now();
    let mut expired_ids = Vec::new();
    workspaces.iter().for_each(|entry| {
        if let WorkspaceMode::Persistent {
            expires_at: Some(deadline),
        } = &entry.value().mode
        {
            if now >= *deadline {
                expired_ids.push(*entry.key());
            }
        }
    });

    for sid in &expired_ids {
        workspaces.remove(sid);
    }

    // The expired workspace should be removed.
    assert_eq!(expired_ids.len(), 1);
    assert_eq!(expired_ids[0], sid_expired);
    assert!(workspaces.get(&sid_expired).is_none(), "expired workspace should be GC'd");

    // The active persistent and ephemeral workspaces should remain.
    assert!(workspaces.get(&sid_active).is_some(), "active persistent should remain");
    assert!(workspaces.get(&sid_ephemeral).is_some(), "ephemeral should remain");
    assert_eq!(workspaces.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════
// Test 9: Active sessions for a specific repo
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_workspace_manager_active_sessions_for_repo() {
    use dashmap::DashMap;

    let workspaces: DashMap<Uuid, SessionWorkspace> = DashMap::new();

    let repo_a = Uuid::new_v4();
    let repo_b = Uuid::new_v4();

    let sid_a1 = Uuid::new_v4();
    let sid_a2 = Uuid::new_v4();
    let sid_a3 = Uuid::new_v4();
    let sid_b1 = Uuid::new_v4();

    // Three workspaces for repo_a, one for repo_b.
    for (sid, repo_id, agent) in [
        (sid_a1, repo_a, "agent-a1"),
        (sid_a2, repo_a, "agent-a2"),
        (sid_a3, repo_a, "agent-a3"),
        (sid_b1, repo_b, "agent-b1"),
    ] {
        let ws = SessionWorkspace::new_test(
            sid,
            repo_id,
            agent.into(),
            "work".into(),
            "def456".into(),
            WorkspaceMode::Ephemeral,
        );
        workspaces.insert(sid, ws);
    }

    // Query active sessions for repo_a (same logic as WorkspaceManager::active_sessions_for_repo).
    let sessions_a: Vec<Uuid> = workspaces
        .iter()
        .filter(|entry| entry.value().repo_id == repo_a)
        .map(|entry| *entry.key())
        .collect();

    assert_eq!(sessions_a.len(), 3, "repo_a should have 3 sessions");
    assert!(sessions_a.contains(&sid_a1));
    assert!(sessions_a.contains(&sid_a2));
    assert!(sessions_a.contains(&sid_a3));

    // Query with exclusion (exclude sid_a2).
    let sessions_a_excl: Vec<Uuid> = workspaces
        .iter()
        .filter(|entry| {
            entry.value().repo_id == repo_a && *entry.key() != sid_a2
        })
        .map(|entry| *entry.key())
        .collect();

    assert_eq!(sessions_a_excl.len(), 2);
    assert!(!sessions_a_excl.contains(&sid_a2));

    // Query for repo_b.
    let sessions_b: Vec<Uuid> = workspaces
        .iter()
        .filter(|entry| entry.value().repo_id == repo_b)
        .map(|entry| *entry.key())
        .collect();

    assert_eq!(sessions_b.len(), 1);
    assert_eq!(sessions_b[0], sid_b1);
}

// ═══════════════════════════════════════════════════════════════════
// Test 10: Semantic conflict analysis — auto-merge of non-overlapping changes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_analyze_file_conflict_semantic_auto_merge() {
    use dk_engine::workspace::conflict::{analyze_file_conflict, MergeAnalysis};
    use dk_engine::parser::ParserRegistry;

    let parser = ParserRegistry::new();

    // Base version: one function.
    let base = b"pub fn existing() -> i32 { 42 }\n";

    // HEAD adds a different function (non-overlapping).
    let head = b"pub fn existing() -> i32 { 42 }\npub fn head_fn() -> bool { true }\n";

    // Overlay adds yet another different function (non-overlapping with HEAD's addition).
    let overlay = b"pub fn existing() -> i32 { 42 }\npub fn overlay_fn() -> String { String::new() }\n";

    let result = analyze_file_conflict("test.rs", base, head, overlay, &parser);

    match result {
        MergeAnalysis::AutoMerge { merged_content } => {
            // Since no symbols overlap, auto-merge should succeed.
            // The merged content should include the overlay's version.
            assert!(
                !merged_content.is_empty(),
                "merged content should not be empty"
            );
        }
        MergeAnalysis::Conflict { conflicts } => {
            // Both sides added different functions — they should NOT conflict
            // because the symbol names are different.
            panic!(
                "expected auto-merge for non-overlapping additions, got {} conflict(s): {:?}",
                conflicts.len(),
                conflicts
                    .iter()
                    .map(|c| &c.symbol_name)
                    .collect::<Vec<_>>()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Test 10b: Semantic conflict analysis — byte-level fallback for non-parseable files
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_analyze_file_conflict_byte_level_fallback() {
    use dk_engine::workspace::conflict::{analyze_file_conflict, MergeAnalysis};
    use dk_engine::parser::ParserRegistry;

    let parser = ParserRegistry::new();

    // Use a .txt file which has no parser — triggers byte-level fallback.
    let base = b"line one\nline two\n";
    let head = b"line one\nline two\n"; // unchanged
    let overlay = b"line one\nline two\nline three\n"; // modified

    let result = analyze_file_conflict("notes.txt", base, head, overlay, &parser);

    match result {
        MergeAnalysis::AutoMerge { merged_content } => {
            assert_eq!(
                merged_content,
                overlay.to_vec(),
                "overlay content should win when only one side changed"
            );
        }
        MergeAnalysis::Conflict { .. } => {
            panic!("expected auto-merge when only overlay changed");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Test 11: overlay_for_tree builds correct commit overlay vector
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_overlay_for_tree_output() {
    let (_repo, commit, _tmp) = init_repo_with_file("README.md", b"# Hello");

    let ws = SessionWorkspace::new_test(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "agent-t".into(),
        "test overlay_for_tree".into(),
        commit,
        WorkspaceMode::Ephemeral,
    );

    // Modify an existing file.
    ws.overlay
        .write_local("README.md", b"# Updated".to_vec(), false);

    // Add a new file.
    ws.overlay
        .write_local("CHANGELOG.md", b"## v1".to_vec(), true);

    // Delete a file (mark it deleted even though it may not exist in base).
    ws.overlay.delete_local("to_delete.txt");

    let overlay = ws.overlay_for_tree();
    assert_eq!(overlay.len(), 3);

    let map: HashMap<&str, &Option<Vec<u8>>> = overlay
        .iter()
        .map(|(path, content)| (path.as_str(), content))
        .collect();

    // Modified/Added entries have Some(content).
    assert!(map["README.md"].is_some());
    assert_eq!(map["README.md"].as_ref().unwrap(), b"# Updated");
    assert!(map["CHANGELOG.md"].is_some());

    // Deleted entries have None.
    assert!(map["to_delete.txt"].is_none());
}
