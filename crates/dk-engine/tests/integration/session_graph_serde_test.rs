use std::path::PathBuf;

use dk_core::{CallEdge, CallKind, Span, Symbol, SymbolKind, Visibility};
use dk_engine::workspace::session_graph::SessionGraph;
use uuid::Uuid;

fn make_symbol(name: &str) -> Symbol {
    Symbol {
        id: Uuid::new_v4(),
        name: name.to_string(),
        qualified_name: name.to_string(),
        kind: SymbolKind::Function,
        visibility: Visibility::Public,
        file_path: PathBuf::from("test.rs"),
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

fn make_edge(caller: Uuid, callee: Uuid) -> CallEdge {
    CallEdge {
        id: Uuid::new_v4(),
        repo_id: Uuid::new_v4(),
        caller,
        callee,
        kind: CallKind::DirectCall,
    }
}

// ── round-trip: empty graph ───────────────────────────────────────────

#[test]
fn roundtrip_empty_graph() {
    let g = SessionGraph::empty();
    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");
    assert_eq!(g2.change_count(), 0);
}

// ── round-trip: added symbols survive ────────────────────────────────

#[test]
fn roundtrip_added_symbols() {
    let g = SessionGraph::empty();
    let sym_a = make_symbol("alpha");
    let sym_b = make_symbol("beta");
    let id_a = sym_a.id;
    let id_b = sym_b.id;
    g.add_symbol(sym_a);
    g.add_symbol(sym_b);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    let got_a = g2
        .get_symbol(id_a)
        .expect("alpha not found after roundtrip");
    assert_eq!(got_a.name, "alpha");

    let got_b = g2.get_symbol(id_b).expect("beta not found after roundtrip");
    assert_eq!(got_b.name, "beta");

    assert_eq!(g2.change_count(), 2);
}

// ── round-trip: modified symbols survive ────────────────────────────

#[test]
fn roundtrip_modified_symbols() {
    let g = SessionGraph::empty();
    let mut sym = make_symbol("my_fn");
    let id = sym.id;
    // Use modify_symbol — since the symbol is not in added_symbols, it goes
    // directly into modified_symbols (simulating a base-symbol modification).
    g.modify_symbol(sym.clone());

    sym.name = "my_fn_v2".to_string();
    g.modify_symbol(sym);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    let got = g2.get_symbol(id).expect("symbol not found after roundtrip");
    assert_eq!(got.name, "my_fn_v2");
}

// ── round-trip: removed symbol IDs survive ───────────────────────────

#[test]
fn roundtrip_removed_symbols() {
    let g = SessionGraph::empty();
    // remove_symbol on an ID that was never added puts it in removed_symbols
    // (no added or modified entry to drop, so it falls through to the base
    // removal path).
    let sym1 = make_symbol("gone_fn_1");
    let sym2 = make_symbol("gone_fn_2");
    let id1 = sym1.id;
    let id2 = sym2.id;
    g.remove_symbol(id1);
    g.remove_symbol(id2);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    // Removed symbols should still be hidden
    assert!(g2.get_symbol(id1).is_none());
    assert!(g2.get_symbol(id2).is_none());
    assert_eq!(g2.change_count(), 2);
}

// ── round-trip: added edges survive ──────────────────────────────────

#[test]
fn roundtrip_added_edges() {
    let g = SessionGraph::empty();
    let sym_a = make_symbol("caller_fn");
    let sym_b = make_symbol("callee_fn");
    let edge = make_edge(sym_a.id, sym_b.id);
    let edge_id = edge.id;
    let expected_caller = sym_a.id;
    let expected_callee = sym_b.id;
    g.add_edge(edge);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    let got = g2
        .get_edge(edge_id)
        .expect("edge not found after roundtrip");
    assert_eq!(got.caller, expected_caller);
    assert_eq!(got.callee, expected_callee);
    assert_eq!(got.kind, CallKind::DirectCall);
}

// ── round-trip: removed edges survive ────────────────────────────────

#[test]
fn roundtrip_removed_edges() {
    let g = SessionGraph::empty();
    let edge_id_1 = Uuid::new_v4();
    let edge_id_2 = Uuid::new_v4();
    // remove_edge on IDs not in added_edges marks them as removed from base.
    g.remove_edge(edge_id_1);
    g.remove_edge(edge_id_2);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    assert!(g2.is_edge_removed(edge_id_1));
    assert!(g2.is_edge_removed(edge_id_2));
}

// ── round-trip: mixed delta (added + modified + removed) ─────────────

#[test]
fn roundtrip_mixed_delta() {
    let g = SessionGraph::empty();

    let added = make_symbol("new_fn");
    let added_id = added.id;
    g.add_symbol(added);

    let modified = make_symbol("changed_fn");
    let modified_id = modified.id;
    // modify_symbol on a symbol not in added_symbols puts it in
    // modified_symbols (simulating a base-symbol modification).
    g.modify_symbol(modified);

    let removed_id = Uuid::new_v4();
    g.remove_symbol(removed_id);

    let edge = make_edge(added_id, modified_id);
    let edge_id = edge.id;
    g.add_edge(edge);

    let removed_edge_id = Uuid::new_v4();
    g.remove_edge(removed_edge_id);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    assert!(g2.get_symbol(added_id).is_some());
    assert!(g2.get_symbol(modified_id).is_some());
    assert!(g2.get_symbol(removed_id).is_none());
    assert!(g2.get_edge(edge_id).is_some());
    assert!(g2.is_edge_removed(removed_edge_id));
    assert_eq!(g2.change_count(), 3);
}

// ── base_symbols are NOT serialized ──────────────────────────────────

#[test]
fn base_symbols_not_included_in_snapshot() {
    use arc_swap::ArcSwap;
    use std::collections::HashMap;
    use std::sync::Arc;

    let mut base = HashMap::new();
    let base_sym = make_symbol("base_only_fn");
    let base_id = base_sym.id;
    base.insert(base_id, base_sym);

    let shared = Arc::new(ArcSwap::from_pointee(base));
    let g = SessionGraph::fork_from(shared);

    // Base symbol is visible in original
    assert!(g.get_symbol(base_id).is_some());

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");

    // After round-trip, base symbol is NOT available — snapshot has no base
    assert!(g2.get_symbol(base_id).is_none());
}

// ── msgpack bytes are compact (sanity check) ─────────────────────────

#[test]
fn msgpack_bytes_are_nonempty() {
    let g = SessionGraph::empty();
    let sym = make_symbol("foo");
    g.add_symbol(sym);

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    assert!(!bytes.is_empty());
}

// ── invalid bytes return an error ────────────────────────────────────

#[test]
fn invalid_bytes_return_error() {
    let garbage = b"this is not msgpack";
    let result = SessionGraph::from_msgpack(garbage);
    assert!(result.is_err());
}

// ── symbol fields preserved faithfully ───────────────────────────────

#[test]
fn symbol_fields_preserved() {
    let g = SessionGraph::empty();
    let sym = Symbol {
        id: Uuid::new_v4(),
        name: "complex_fn".to_string(),
        qualified_name: "my_module::complex_fn".to_string(),
        kind: SymbolKind::Function,
        visibility: Visibility::Crate,
        file_path: PathBuf::from("src/complex.rs"),
        span: Span {
            start_byte: 42,
            end_byte: 99,
        },
        signature: Some("fn complex_fn(x: u32) -> bool".to_string()),
        doc_comment: Some("/// Does something complex".to_string()),
        parent: Some(Uuid::new_v4()),
        last_modified_by: Some("agent-007".to_string()),
        last_modified_intent: Some("refactor".to_string()),
    };
    let id = sym.id;
    g.add_symbol(sym.clone());

    let bytes = g.to_msgpack().expect("to_msgpack failed");
    let g2 = SessionGraph::from_msgpack(&bytes).expect("from_msgpack failed");
    let got = g2.get_symbol(id).expect("symbol not found");

    assert_eq!(got.name, "complex_fn");
    assert_eq!(got.qualified_name, "my_module::complex_fn");
    assert_eq!(got.kind, SymbolKind::Function);
    assert_eq!(got.visibility, Visibility::Crate);
    assert_eq!(got.file_path, PathBuf::from("src/complex.rs"));
    assert_eq!(got.span.start_byte, 42);
    assert_eq!(got.span.end_byte, 99);
    assert_eq!(
        got.signature.as_deref(),
        Some("fn complex_fn(x: u32) -> bool")
    );
    assert_eq!(
        got.doc_comment.as_deref(),
        Some("/// Does something complex")
    );
    assert_eq!(got.last_modified_by.as_deref(), Some("agent-007"));
    assert_eq!(got.last_modified_intent.as_deref(), Some("refactor"));
    assert_eq!(got.parent, sym.parent);
}
