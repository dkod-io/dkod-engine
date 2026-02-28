//! SessionGraph — delta-based semantic graph layered on a shared base.
//!
//! The shared base symbol table is stored in an `ArcSwap` so it can be
//! atomically replaced when the repository is re-indexed. Each session
//! maintains its own deltas (added, modified, removed symbols and edges)
//! in lock-free `DashMap`/`DashSet` collections.

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use dashmap::{DashMap, DashSet};
use dk_core::{CallEdge, Symbol, SymbolId};
use uuid::Uuid;

// ── SessionGraph ─────────────────────────────────────────────────────

/// A delta-based semantic graph for a single session workspace.
///
/// Lookups resolve in order: removed -> modified -> added -> base.
/// This gives each session a consistent, isolated view of the symbol
/// graph without copying the entire base.
pub struct SessionGraph {
    /// Shared, read-only base symbol table (repo-wide).
    base_symbols: Option<Arc<ArcSwap<HashMap<SymbolId, Symbol>>>>,

    /// Symbols that existed in the base and were modified in this session.
    modified_symbols: DashMap<SymbolId, Symbol>,

    /// Symbols that are newly created in this session.
    added_symbols: DashMap<SymbolId, Symbol>,

    /// Symbols that existed in the base and were removed in this session.
    removed_symbols: DashSet<SymbolId>,

    /// Call edges added in this session.
    added_edges: DashMap<Uuid, CallEdge>,

    /// Call edge IDs removed from the base in this session.
    removed_edges: DashSet<Uuid>,
}

impl SessionGraph {
    /// Fork from a shared base symbol table.
    pub fn fork_from(base: Arc<ArcSwap<HashMap<SymbolId, Symbol>>>) -> Self {
        Self {
            base_symbols: Some(base),
            modified_symbols: DashMap::new(),
            added_symbols: DashMap::new(),
            removed_symbols: DashSet::new(),
            added_edges: DashMap::new(),
            removed_edges: DashSet::new(),
        }
    }

    /// Create an empty session graph (no shared base).
    pub fn empty() -> Self {
        Self {
            base_symbols: None,
            modified_symbols: DashMap::new(),
            added_symbols: DashMap::new(),
            removed_symbols: DashSet::new(),
            added_edges: DashMap::new(),
            removed_edges: DashSet::new(),
        }
    }

    /// Look up a symbol by ID, respecting the session delta.
    ///
    /// Resolution order:
    /// 1. If removed in this session, return `None`.
    /// 2. If modified in this session, return the modified version.
    /// 3. If added in this session, return it.
    /// 4. Fall through to the shared base.
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        // Removed in this session?
        if self.removed_symbols.contains(&id) {
            return None;
        }

        // Modified in this session?
        if let Some(sym) = self.modified_symbols.get(&id) {
            return Some(sym.value().clone());
        }

        // Added in this session?
        if let Some(sym) = self.added_symbols.get(&id) {
            return Some(sym.value().clone());
        }

        // Base lookup
        if let Some(base) = &self.base_symbols {
            let snapshot = base.load();
            return snapshot.get(&id).cloned();
        }

        None
    }

    /// Add a new symbol to this session.
    pub fn add_symbol(&self, symbol: Symbol) {
        self.added_symbols.insert(symbol.id, symbol);
    }

    /// Modify an existing symbol (base or previously added).
    pub fn modify_symbol(&self, symbol: Symbol) {
        let id = symbol.id;

        // If it was added in this session, update the added entry.
        if self.added_symbols.contains_key(&id) {
            self.added_symbols.insert(id, symbol);
        } else {
            self.modified_symbols.insert(id, symbol);
        }
    }

    /// Remove a symbol from the session view.
    pub fn remove_symbol(&self, id: SymbolId) {
        // If it was added in this session, just drop it.
        if self.added_symbols.remove(&id).is_some() {
            return;
        }

        // If it was modified, drop the modification.
        self.modified_symbols.remove(&id);

        // Mark as removed from base.
        self.removed_symbols.insert(id);
    }

    /// Add a call edge.
    pub fn add_edge(&self, edge: CallEdge) {
        self.added_edges.insert(edge.id, edge);
    }

    /// Remove a call edge.
    pub fn remove_edge(&self, edge_id: Uuid) {
        // If it was added in this session, just drop it.
        if self.added_edges.remove(&edge_id).is_some() {
            return;
        }
        self.removed_edges.insert(edge_id);
    }

    /// Return the names of all symbols changed in this session
    /// (added, modified, or removed).
    ///
    /// Used by the conflict detector to find overlapping changes.
    pub fn changed_symbol_names(&self) -> Vec<String> {
        let mut names = Vec::new();

        for entry in self.added_symbols.iter() {
            names.push(entry.value().qualified_name.clone());
        }

        for entry in self.modified_symbols.iter() {
            names.push(entry.value().qualified_name.clone());
        }

        // For removed symbols, look up the name from the base.
        if let Some(base) = &self.base_symbols {
            let snapshot = base.load();
            for id in self.removed_symbols.iter() {
                if let Some(sym) = snapshot.get(id.key()) {
                    names.push(sym.qualified_name.clone());
                }
            }
        }

        names
    }

    /// Update the session graph from a parse result for a single file.
    ///
    /// Compares the new symbols against the base symbols for that file,
    /// and classifies each as added, modified, or removed within the
    /// session delta.
    ///
    /// `base_symbols_for_file` should contain all symbols from the base
    /// that belong to the given file path.
    pub fn update_from_parse(
        &self,
        _file_path: &str,
        new_symbols: Vec<Symbol>,
        base_symbols_for_file: &[Symbol],
    ) {
        // Build a lookup of base symbols by qualified name for this file.
        let base_by_name: HashMap<&str, &Symbol> = base_symbols_for_file
            .iter()
            .map(|s| (s.qualified_name.as_str(), s))
            .collect();

        let new_by_name: HashMap<&str, &Symbol> = new_symbols
            .iter()
            .map(|s| (s.qualified_name.as_str(), s))
            .collect();

        // Symbols in new but not in base -> added
        // Symbols in both but changed -> modified
        for sym in &new_symbols {
            if let Some(base_sym) = base_by_name.get(sym.qualified_name.as_str()) {
                // Compare span, signature, etc. to detect modification.
                if sym.span != base_sym.span
                    || sym.signature != base_sym.signature
                    || sym.kind != base_sym.kind
                    || sym.visibility != base_sym.visibility
                {
                    self.modify_symbol(sym.clone());
                }
            } else {
                self.add_symbol(sym.clone());
            }
        }

        // Symbols in base but not in new -> removed
        for base_sym in base_symbols_for_file {
            if !new_by_name.contains_key(base_sym.qualified_name.as_str()) {
                self.remove_symbol(base_sym.id);
            }
        }
    }

    /// Number of symbols changed (added + modified + removed).
    pub fn change_count(&self) -> usize {
        self.added_symbols.len() + self.modified_symbols.len() + self.removed_symbols.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dk_core::{Span, SymbolKind, Visibility};
    use std::path::PathBuf;

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

    #[test]
    fn empty_graph_returns_none() {
        let g = SessionGraph::empty();
        assert!(g.get_symbol(Uuid::new_v4()).is_none());
    }

    #[test]
    fn add_and_get_symbol() {
        let g = SessionGraph::empty();
        let sym = make_symbol("foo");
        let id = sym.id;
        g.add_symbol(sym);
        assert!(g.get_symbol(id).is_some());
        assert_eq!(g.get_symbol(id).unwrap().name, "foo");
    }

    #[test]
    fn remove_added_symbol() {
        let g = SessionGraph::empty();
        let sym = make_symbol("bar");
        let id = sym.id;
        g.add_symbol(sym);
        g.remove_symbol(id);
        assert!(g.get_symbol(id).is_none());
    }

    #[test]
    fn modify_added_symbol_updates_in_place() {
        let g = SessionGraph::empty();
        let mut sym = make_symbol("baz");
        let id = sym.id;
        g.add_symbol(sym.clone());

        sym.name = "baz_v2".to_string();
        g.modify_symbol(sym);

        let got = g.get_symbol(id).unwrap();
        assert_eq!(got.name, "baz_v2");
    }

    #[test]
    fn fork_from_base_lookup() {
        let mut base = HashMap::new();
        let sym = make_symbol("base_fn");
        let id = sym.id;
        base.insert(id, sym);

        let shared = Arc::new(ArcSwap::from_pointee(base));
        let g = SessionGraph::fork_from(shared);

        assert!(g.get_symbol(id).is_some());
        assert_eq!(g.get_symbol(id).unwrap().name, "base_fn");
    }

    #[test]
    fn remove_base_symbol_hides_it() {
        let mut base = HashMap::new();
        let sym = make_symbol("base_fn");
        let id = sym.id;
        base.insert(id, sym);

        let shared = Arc::new(ArcSwap::from_pointee(base));
        let g = SessionGraph::fork_from(shared);

        g.remove_symbol(id);
        assert!(g.get_symbol(id).is_none());
    }

    #[test]
    fn changed_symbol_names_collects_all() {
        let mut base = HashMap::new();
        let sym = make_symbol("removed_fn");
        let removed_id = sym.id;
        base.insert(removed_id, sym);

        let shared = Arc::new(ArcSwap::from_pointee(base));
        let g = SessionGraph::fork_from(shared);

        let added = make_symbol("added_fn");
        g.add_symbol(added);

        let mut modified = make_symbol("modified_fn");
        modified.id = Uuid::new_v4();
        let mid = modified.id;
        // Pretend it's in base by inserting to modified_symbols directly
        g.modified_symbols.insert(mid, modified);

        g.remove_symbol(removed_id);

        let names = g.changed_symbol_names();
        assert!(names.contains(&"added_fn".to_string()));
        assert!(names.contains(&"modified_fn".to_string()));
        assert!(names.contains(&"removed_fn".to_string()));
    }

    #[test]
    fn change_count() {
        let g = SessionGraph::empty();
        assert_eq!(g.change_count(), 0);

        g.add_symbol(make_symbol("a"));
        assert_eq!(g.change_count(), 1);
    }
}
