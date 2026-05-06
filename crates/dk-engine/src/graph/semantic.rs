use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Stable identifier for a semantic knowledge graph node.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct SemanticNodeId(String);

impl SemanticNodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SemanticNodeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SemanticNodeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Stable identifier for a semantic knowledge graph edge.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct SemanticEdgeId(String);

impl SemanticEdgeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SemanticEdgeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SemanticEdgeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Coarse source of a semantic fact, ready for SCIP/ctags/GraphQL importers later.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticEvidence {
    pub source: SemanticEvidenceSource,
    pub path: Option<String>,
    pub range: Option<SemanticRange>,
}

/// Ingestion source for a semantic node or edge.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SemanticEvidenceSource {
    Parser,
    Scip,
    Ctags,
    GraphQl,
    Manual,
    Unknown,
}

/// Byte-oriented source range. Importers can attach richer ranges elsewhere.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticRange {
    pub start_byte: u32,
    pub end_byte: u32,
}

/// Read-model node categories for the Track B semantic knowledge graph.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum SemanticNodeKind {
    Repository,
    File,
    Module,
    Symbol,
    Type,
    CallEdge,
    Dependency,
    Changeset,
    Session,
    Agent,
    Review,
    Invariant,
    Endpoint,
    Query,
    External,
}

/// Read-model edge categories for semantic relationships.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum SemanticEdgeKind {
    Contains,
    Defines,
    Modifies,
    CreatedBy,
    Imports,
    Calls,
    DependsOn,
    Implements,
    Extends,
    Tests,
    Violates,
    Reads,
    Writes,
    ResolvesTo,
    Related,
}

/// Read-only query defaults for future GraphQL/planner graph access.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticQueryOptions {
    pub cursor: Option<String>,
    pub limit: usize,
    pub max_depth: usize,
    pub max_cost: usize,
    pub include_source_bodies: bool,
}

impl Default for SemanticQueryOptions {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: 100,
            max_depth: 4,
            max_cost: 1_000,
            include_source_bodies: false,
        }
    }
}

/// A semantic graph node in the read model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticNode {
    pub id: SemanticNodeId,
    pub kind: SemanticNodeKind,
    pub label: String,
    pub qualified_name: Option<String>,
    pub evidence: Vec<SemanticEvidence>,
    pub properties: BTreeMap<String, String>,
}

impl SemanticNode {
    pub fn new(
        id: impl Into<SemanticNodeId>,
        kind: SemanticNodeKind,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            qualified_name: None,
            evidence: Vec::new(),
            properties: BTreeMap::new(),
        }
    }

    pub fn with_qualified_name(mut self, qualified_name: impl Into<String>) -> Self {
        self.qualified_name = Some(qualified_name.into());
        self
    }
}

/// A semantic graph edge in the read model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticEdge {
    pub id: SemanticEdgeId,
    pub kind: SemanticEdgeKind,
    pub from: SemanticNodeId,
    pub to: SemanticNodeId,
    pub label: Option<String>,
    pub confidence: f32,
    pub evidence: Vec<SemanticEvidence>,
    pub properties: BTreeMap<String, String>,
}

impl SemanticEdge {
    pub fn new(
        id: impl Into<SemanticEdgeId>,
        kind: SemanticEdgeKind,
        from: impl Into<SemanticNodeId>,
        to: impl Into<SemanticNodeId>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            from: from.into(),
            to: to.into(),
            label: None,
            confidence: 1.0,
            evidence: Vec::new(),
            properties: BTreeMap::new(),
        }
    }
}

/// Direction used by read-only graph traversals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticDirection {
    Incoming,
    Outgoing,
    Both,
}

/// Errors returned by the in-memory semantic graph store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticGraphError {
    MissingNode(SemanticNodeId),
}

impl std::fmt::Display for SemanticGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingNode(id) => write!(f, "semantic graph node not found: {}", id.as_str()),
        }
    }
}

impl std::error::Error for SemanticGraphError {}

/// Deterministic, query-safe in-memory read model for semantic graph data.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SemanticGraph {
    nodes: BTreeMap<SemanticNodeId, SemanticNode>,
    edges: BTreeMap<SemanticEdgeId, SemanticEdge>,
    outgoing: BTreeMap<SemanticNodeId, BTreeSet<SemanticEdgeId>>,
    incoming: BTreeMap<SemanticNodeId, BTreeSet<SemanticEdgeId>>,
    nodes_by_kind: BTreeMap<SemanticNodeKind, BTreeSet<SemanticNodeId>>,
}

impl SemanticGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn upsert_node(&mut self, node: SemanticNode) -> Option<SemanticNode> {
        let id = node.id.clone();
        let old = self.nodes.insert(id.clone(), node);

        if let Some(old_node) = &old {
            if old_node.kind != self.nodes[&id].kind {
                remove_index_value(&mut self.nodes_by_kind, &old_node.kind, &id);
            }
        }

        self.nodes_by_kind
            .entry(self.nodes[&id].kind.clone())
            .or_default()
            .insert(id);

        old
    }

    pub fn upsert_edge(
        &mut self,
        edge: SemanticEdge,
    ) -> Result<Option<SemanticEdge>, SemanticGraphError> {
        if !self.nodes.contains_key(&edge.from) {
            return Err(SemanticGraphError::MissingNode(edge.from));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(SemanticGraphError::MissingNode(edge.to));
        }

        let id = edge.id.clone();
        let old = self.edges.insert(id.clone(), edge);
        if let Some(old_edge) = &old {
            remove_index_value(&mut self.outgoing, &old_edge.from, &id);
            remove_index_value(&mut self.incoming, &old_edge.to, &id);
        }

        let edge = &self.edges[&id];
        self.outgoing
            .entry(edge.from.clone())
            .or_default()
            .insert(id.clone());
        self.incoming.entry(edge.to.clone()).or_default().insert(id);

        Ok(old)
    }

    pub fn get_node(&self, id: &SemanticNodeId) -> Option<&SemanticNode> {
        self.nodes.get(id)
    }

    pub fn get_edge(&self, id: &SemanticEdgeId) -> Option<&SemanticEdge> {
        self.edges.get(id)
    }

    pub fn nodes_by_kind(&self, kind: SemanticNodeKind) -> Vec<&SemanticNode> {
        self.nodes_by_kind
            .get(&kind)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    pub fn edges_for(
        &self,
        node_id: &SemanticNodeId,
        direction: SemanticDirection,
    ) -> Vec<&SemanticEdge> {
        let mut edge_ids = BTreeSet::new();

        if matches!(
            direction,
            SemanticDirection::Outgoing | SemanticDirection::Both
        ) {
            if let Some(outgoing) = self.outgoing.get(node_id) {
                edge_ids.extend(outgoing.iter().cloned());
            }
        }

        if matches!(
            direction,
            SemanticDirection::Incoming | SemanticDirection::Both
        ) {
            if let Some(incoming) = self.incoming.get(node_id) {
                edge_ids.extend(incoming.iter().cloned());
            }
        }

        edge_ids
            .iter()
            .filter_map(|id| self.edges.get(id))
            .collect()
    }

    pub fn neighbors(
        &self,
        node_id: &SemanticNodeId,
        direction: SemanticDirection,
    ) -> Vec<&SemanticNode> {
        let mut node_ids = BTreeSet::new();

        for edge in self.edges_for(node_id, direction) {
            if matches!(
                direction,
                SemanticDirection::Outgoing | SemanticDirection::Both
            ) && &edge.from == node_id
            {
                node_ids.insert(edge.to.clone());
            }

            if matches!(
                direction,
                SemanticDirection::Incoming | SemanticDirection::Both
            ) && &edge.to == node_id
            {
                node_ids.insert(edge.from.clone());
            }
        }

        node_ids
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    pub fn edges_between(&self, from: &SemanticNodeId, to: &SemanticNodeId) -> Vec<&SemanticEdge> {
        self.outgoing
            .get(from)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.edges.get(id))
            .filter(|edge| &edge.to == to)
            .collect()
    }
}

fn remove_index_value<K, V>(index: &mut BTreeMap<K, BTreeSet<V>>, key: &K, value: &V)
where
    K: Ord + Clone,
    V: Ord,
{
    let should_remove = if let Some(values) = index.get_mut(key) {
        values.remove(value);
        values.is_empty()
    } else {
        false
    };

    if should_remove {
        index.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: SemanticNodeKind) -> SemanticNode {
        SemanticNode::new(id, kind, id)
    }

    #[test]
    fn indexes_nodes_by_kind_deterministically() {
        let mut graph = SemanticGraph::new();
        graph.upsert_node(node("symbol:beta", SemanticNodeKind::Symbol));
        graph.upsert_node(node("file:lib.rs", SemanticNodeKind::File));
        graph.upsert_node(node("symbol:alpha", SemanticNodeKind::Symbol));

        let ids: Vec<_> = graph
            .nodes_by_kind(SemanticNodeKind::Symbol)
            .into_iter()
            .map(|node| node.id.as_str())
            .collect();

        assert_eq!(ids, vec!["symbol:alpha", "symbol:beta"]);
    }

    #[test]
    fn requires_edges_to_reference_existing_nodes() {
        let mut graph = SemanticGraph::new();
        graph.upsert_node(node("symbol:caller", SemanticNodeKind::Symbol));

        let err = graph
            .upsert_edge(SemanticEdge::new(
                "edge:missing",
                SemanticEdgeKind::Calls,
                "symbol:caller",
                "symbol:callee",
            ))
            .expect_err("missing target should fail");

        assert_eq!(
            err,
            SemanticGraphError::MissingNode(SemanticNodeId::from("symbol:callee"))
        );
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn supports_query_safe_directional_traversal() {
        let mut graph = SemanticGraph::new();
        graph.upsert_node(node("symbol:caller", SemanticNodeKind::Symbol));
        graph.upsert_node(node("symbol:callee", SemanticNodeKind::Symbol));
        graph.upsert_node(node("type:result", SemanticNodeKind::Type));

        graph
            .upsert_edge(SemanticEdge::new(
                "edge:calls",
                SemanticEdgeKind::Calls,
                "symbol:caller",
                "symbol:callee",
            ))
            .unwrap();
        graph
            .upsert_edge(SemanticEdge::new(
                "edge:returns",
                SemanticEdgeKind::ResolvesTo,
                "symbol:callee",
                "type:result",
            ))
            .unwrap();

        let caller = SemanticNodeId::from("symbol:caller");
        let callee = SemanticNodeId::from("symbol:callee");

        let outgoing: Vec<_> = graph
            .neighbors(&caller, SemanticDirection::Outgoing)
            .into_iter()
            .map(|node| node.id.as_str())
            .collect();
        let incoming: Vec<_> = graph
            .neighbors(&callee, SemanticDirection::Incoming)
            .into_iter()
            .map(|node| node.id.as_str())
            .collect();
        let between: Vec<_> = graph
            .edges_between(&caller, &callee)
            .into_iter()
            .map(|edge| edge.id.as_str())
            .collect();

        assert_eq!(outgoing, vec!["symbol:callee"]);
        assert_eq!(incoming, vec!["symbol:caller"]);
        assert_eq!(between, vec!["edge:calls"]);
    }

    #[test]
    fn reindexing_replaced_edges_keeps_queries_consistent() {
        let mut graph = SemanticGraph::new();
        graph.upsert_node(node("symbol:a", SemanticNodeKind::Symbol));
        graph.upsert_node(node("symbol:b", SemanticNodeKind::Symbol));
        graph.upsert_node(node("symbol:c", SemanticNodeKind::Symbol));

        graph
            .upsert_edge(SemanticEdge::new(
                "edge:relation",
                SemanticEdgeKind::Calls,
                "symbol:a",
                "symbol:b",
            ))
            .unwrap();
        graph
            .upsert_edge(SemanticEdge::new(
                "edge:relation",
                SemanticEdgeKind::Calls,
                "symbol:c",
                "symbol:b",
            ))
            .unwrap();

        let a = SemanticNodeId::from("symbol:a");
        let b = SemanticNodeId::from("symbol:b");
        let c = SemanticNodeId::from("symbol:c");

        assert!(graph.edges_between(&a, &b).is_empty());
        assert_eq!(graph.edges_between(&c, &b).len(), 1);
        assert!(graph.neighbors(&a, SemanticDirection::Outgoing).is_empty());
    }

    #[test]
    fn track_b_schema_covers_planned_provenance_entities() {
        let mut graph = SemanticGraph::new();
        graph.upsert_node(node("changeset:1", SemanticNodeKind::Changeset));
        graph.upsert_node(node("agent:planner", SemanticNodeKind::Agent));
        graph.upsert_node(node("symbol:target", SemanticNodeKind::Symbol));
        graph.upsert_node(node("invariant:auth", SemanticNodeKind::Invariant));

        graph
            .upsert_edge(SemanticEdge::new(
                "edge:created-by",
                SemanticEdgeKind::CreatedBy,
                "changeset:1",
                "agent:planner",
            ))
            .unwrap();
        graph
            .upsert_edge(SemanticEdge::new(
                "edge:modifies",
                SemanticEdgeKind::Modifies,
                "changeset:1",
                "symbol:target",
            ))
            .unwrap();
        graph
            .upsert_edge(SemanticEdge::new(
                "edge:violates",
                SemanticEdgeKind::Violates,
                "symbol:target",
                "invariant:auth",
            ))
            .unwrap();

        assert_eq!(graph.nodes_by_kind(SemanticNodeKind::Changeset).len(), 1);
        assert_eq!(
            graph
                .edges_for(
                    &SemanticNodeId::from("changeset:1"),
                    SemanticDirection::Outgoing,
                )
                .len(),
            2
        );
    }

    #[test]
    fn query_options_default_to_read_only_graphql_limits() {
        let options = SemanticQueryOptions::default();

        assert_eq!(options.limit, 100);
        assert_eq!(options.max_depth, 4);
        assert_eq!(options.max_cost, 1_000);
        assert!(!options.include_source_bodies);
    }
}
