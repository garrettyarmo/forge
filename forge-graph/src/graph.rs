//! ForgeGraph - the main knowledge graph container.

use crate::edge::{Edge, EdgeType};
use crate::error::GraphError;
use crate::node::{Node, NodeId, NodeType};
use chrono::{DateTime, Utc};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Metadata about the graph itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    /// Forge version that created this graph
    pub forge_version: String,

    /// When the graph was created
    pub created_at: DateTime<Utc>,

    /// When the graph was last modified
    pub modified_at: DateTime<Utc>,

    /// Number of surveys that have updated this graph
    pub survey_count: u32,

    /// Configuration used for last survey
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_survey_config: Option<serde_json::Value>,
}

impl Default for GraphMetadata {
    fn default() -> Self {
        Self {
            forge_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: Utc::now(),
            modified_at: Utc::now(),
            survey_count: 0,
            last_survey_config: None,
        }
    }
}

/// JSON-serializable representation of the graph.
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// Metadata about the graph
    pub metadata: GraphMetadata,

    /// All nodes in the graph
    pub nodes: Vec<Node>,

    /// All edges in the graph
    pub edges: Vec<Edge>,
}

/// The main knowledge graph container.
pub struct ForgeGraph {
    /// Underlying directed graph from petgraph
    inner: DiGraph<Node, Edge>,

    /// Index from NodeId to petgraph NodeIndex for O(1) lookup
    node_index: HashMap<NodeId, NodeIndex>,

    /// Graph metadata
    pub metadata: GraphMetadata,
}

impl Default for ForgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ForgeGraph {
    /// Create a new empty graph.
    pub fn new() -> Self {
        Self {
            inner: DiGraph::new(),
            node_index: HashMap::new(),
            metadata: GraphMetadata::default(),
        }
    }

    // === Node Operations ===

    /// Add a node to the graph.
    /// Returns error if a node with the same ID already exists.
    pub fn add_node(&mut self, node: Node) -> Result<NodeIndex, GraphError> {
        if self.node_index.contains_key(&node.id) {
            return Err(GraphError::DuplicateNode(node.id.to_string()));
        }

        let id = node.id.clone();
        let idx = self.inner.add_node(node);
        self.node_index.insert(id, idx);
        self.metadata.modified_at = Utc::now();
        Ok(idx)
    }

    /// Add or update a node (upsert semantics).
    /// If node exists, merges attributes and updates metadata.
    pub fn upsert_node(&mut self, node: Node) -> NodeIndex {
        if let Some(&idx) = self.node_index.get(&node.id) {
            // Merge with existing node
            let existing = &mut self.inner[idx];
            for (k, v) in node.attributes {
                existing.attributes.insert(k, v);
            }
            existing.metadata.updated_at = Utc::now();
            if node.business_context.is_some() {
                existing.business_context = node.business_context;
            }
            self.metadata.modified_at = Utc::now();
            idx
        } else {
            // Insert new node
            self.add_node(node).unwrap() // Safe: we just checked it doesn't exist
        }
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &NodeId) -> Option<&Node> {
        self.node_index.get(id).map(|&idx| &self.inner[idx])
    }

    /// Get a mutable reference to a node by ID.
    pub fn get_node_mut(&mut self, id: &NodeId) -> Option<&mut Node> {
        self.node_index
            .get(id)
            .copied()
            .map(|idx| &mut self.inner[idx])
    }

    /// Remove a node and all its edges.
    pub fn remove_node(&mut self, id: &NodeId) -> Option<Node> {
        if let Some(idx) = self.node_index.remove(id) {
            self.metadata.modified_at = Utc::now();
            self.inner.remove_node(idx)
        } else {
            None
        }
    }

    /// Check if a node exists.
    pub fn contains_node(&self, id: &NodeId) -> bool {
        self.node_index.contains_key(id)
    }

    /// Get count of nodes.
    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    /// Iterate over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.inner.node_weights()
    }

    /// Get all nodes of a specific type.
    pub fn nodes_by_type(&self, node_type: NodeType) -> impl Iterator<Item = &Node> {
        self.inner
            .node_weights()
            .filter(move |n| n.node_type == node_type)
    }

    // === Edge Operations ===

    /// Add an edge to the graph.
    /// Validates that source and target nodes exist.
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), GraphError> {
        let source_idx = self
            .node_index
            .get(&edge.source)
            .ok_or_else(|| GraphError::NodeNotFound(edge.source.to_string()))?;
        let target_idx = self
            .node_index
            .get(&edge.target)
            .ok_or_else(|| GraphError::NodeNotFound(edge.target.to_string()))?;

        // Check for duplicate edge
        for existing_edge in self.inner.edges_connecting(*source_idx, *target_idx) {
            if existing_edge.weight().edge_type == edge.edge_type {
                return Err(GraphError::DuplicateEdge {
                    source_node: edge.source.to_string(),
                    target_node: edge.target.to_string(),
                    edge_type: edge.edge_type,
                });
            }
        }

        self.inner.add_edge(*source_idx, *target_idx, edge);
        self.metadata.modified_at = Utc::now();
        Ok(())
    }

    /// Add or update an edge (upsert semantics).
    pub fn upsert_edge(&mut self, edge: Edge) -> Result<(), GraphError> {
        let source_idx = *self
            .node_index
            .get(&edge.source)
            .ok_or_else(|| GraphError::NodeNotFound(edge.source.to_string()))?;
        let target_idx = *self
            .node_index
            .get(&edge.target)
            .ok_or_else(|| GraphError::NodeNotFound(edge.target.to_string()))?;

        // Find and update existing or add new
        let mut found_edge_idx = None;
        for edge_ref in self.inner.edges_connecting(source_idx, target_idx) {
            if edge_ref.weight().edge_type == edge.edge_type {
                found_edge_idx = Some(edge_ref.id());
                break;
            }
        }

        if let Some(edge_idx) = found_edge_idx {
            self.inner[edge_idx] = edge;
        } else {
            self.inner.add_edge(source_idx, target_idx, edge);
        }

        self.metadata.modified_at = Utc::now();
        Ok(())
    }

    /// Get all edges from a node.
    pub fn edges_from(&self, id: &NodeId) -> Vec<&Edge> {
        self.node_index
            .get(id)
            .map(|&idx| {
                self.inner
                    .edges_directed(idx, Direction::Outgoing)
                    .map(|e| e.weight())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all edges to a node.
    pub fn edges_to(&self, id: &NodeId) -> Vec<&Edge> {
        self.node_index
            .get(id)
            .map(|&idx| {
                self.inner
                    .edges_directed(idx, Direction::Incoming)
                    .map(|e| e.weight())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all edges of a specific type from a node.
    pub fn edges_from_by_type(&self, id: &NodeId, edge_type: EdgeType) -> Vec<&Edge> {
        self.edges_from(id)
            .into_iter()
            .filter(|e| e.edge_type == edge_type)
            .collect()
    }

    /// Get all edges of a specific type to a node.
    pub fn edges_to_by_type(&self, id: &NodeId, edge_type: EdgeType) -> Vec<&Edge> {
        self.edges_to(id)
            .into_iter()
            .filter(|e| e.edge_type == edge_type)
            .collect()
    }

    /// Get edge count.
    pub fn edge_count(&self) -> usize {
        self.inner.edge_count()
    }

    /// Iterate over all edges.
    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.inner.edge_weights()
    }

    /// Get all edges of a specific type.
    pub fn edges_by_type(&self, edge_type: EdgeType) -> impl Iterator<Item = &Edge> {
        self.inner
            .edge_weights()
            .filter(move |e| e.edge_type == edge_type)
    }

    // === Serialization ===

    /// Serialize the graph to a JSON file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), GraphError> {
        let snapshot = GraphSnapshot {
            metadata: self.metadata.clone(),
            nodes: self.inner.node_weights().cloned().collect(),
            edges: self.inner.edge_weights().cloned().collect(),
        };

        let file = std::fs::File::create(path.as_ref())?;
        let writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &snapshot)
            .map_err(|e| GraphError::SerializationError(e.to_string()))?;

        Ok(())
    }

    /// Load a graph from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, GraphError> {
        let file = std::fs::File::open(path.as_ref())?;
        let reader = std::io::BufReader::new(file);
        let snapshot: GraphSnapshot = serde_json::from_reader(reader)
            .map_err(|e| GraphError::DeserializationError(e.to_string()))?;

        let mut graph = Self::new();
        graph.metadata = snapshot.metadata;

        // Add all nodes first
        for node in snapshot.nodes {
            graph.add_node(node)?;
        }

        // Then add all edges
        for edge in snapshot.edges {
            graph.add_edge(edge)?;
        }

        Ok(graph)
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> Result<String, GraphError> {
        let snapshot = GraphSnapshot {
            metadata: self.metadata.clone(),
            nodes: self.inner.node_weights().cloned().collect(),
            edges: self.inner.edge_weights().cloned().collect(),
        };

        serde_json::to_string_pretty(&snapshot)
            .map_err(|e| GraphError::SerializationError(e.to_string()))
    }

    /// Deserialize from JSON string.
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let snapshot: GraphSnapshot = serde_json::from_str(json)
            .map_err(|e| GraphError::DeserializationError(e.to_string()))?;

        let mut graph = Self::new();
        graph.metadata = snapshot.metadata;

        for node in snapshot.nodes {
            graph.add_node(node)?;
        }

        for edge in snapshot.edges {
            graph.add_edge(edge)?;
        }

        Ok(graph)
    }

    /// Get the internal petgraph for advanced operations.
    pub fn inner(&self) -> &DiGraph<Node, Edge> {
        &self.inner
    }

    /// Get the node index map for advanced operations.
    pub fn node_index_map(&self) -> &HashMap<NodeId, NodeIndex> {
        &self.node_index
    }
}

impl std::fmt::Debug for ForgeGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForgeGraph")
            .field("node_count", &self.node_count())
            .field("edge_count", &self.edge_count())
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{DiscoverySource, NodeBuilder};
    use pretty_assertions::assert_eq;

    fn create_test_service(namespace: &str, name: &str, display: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, namespace, name).unwrap())
            .node_type(NodeType::Service)
            .display_name(display)
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    fn create_test_database(namespace: &str, name: &str, display: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, namespace, name).unwrap())
            .node_type(NodeType::Database)
            .display_name(display)
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    fn create_test_graph() -> ForgeGraph {
        let mut graph = ForgeGraph::new();

        // Add services
        graph
            .add_node(create_test_service("ns", "svc-a", "Service A"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "svc-b", "Service B"))
            .unwrap();
        graph
            .add_node(create_test_database("ns", "users-db", "Users DB"))
            .unwrap();

        // Add edges
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                    NodeId::new(NodeType::Database, "ns", "users-db").unwrap(),
                    EdgeType::Reads,
                )
                .unwrap(),
            )
            .unwrap();

        graph
    }

    #[test]
    fn test_add_and_get_node() {
        let mut graph = ForgeGraph::new();

        let node = create_test_service("ns", "test", "Test");
        graph.add_node(node).unwrap();

        let id = NodeId::new(NodeType::Service, "ns", "test").unwrap();
        let retrieved = graph.get_node(&id).unwrap();
        assert_eq!(retrieved.display_name, "Test");
    }

    #[test]
    fn test_duplicate_node_error() {
        let mut graph = ForgeGraph::new();

        let node1 = create_test_service("ns", "test", "Test 1");
        let node2 = create_test_service("ns", "test", "Test 2");

        graph.add_node(node1).unwrap();
        let result = graph.add_node(node2);

        assert!(matches!(result, Err(GraphError::DuplicateNode(_))));
    }

    #[test]
    fn test_upsert_node_merges_attributes() {
        let mut graph = ForgeGraph::new();

        let node1 = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
            .node_type(NodeType::Service)
            .display_name("Test")
            .attribute("key1", "value1")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(node1).unwrap();

        let node2 = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
            .node_type(NodeType::Service)
            .display_name("Test")
            .attribute("key2", "value2")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.upsert_node(node2);

        let id = NodeId::new(NodeType::Service, "ns", "test").unwrap();
        let node = graph.get_node(&id).unwrap();

        assert!(node.attributes.contains_key("key1"));
        assert!(node.attributes.contains_key("key2"));
    }

    #[test]
    fn test_remove_node() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "test", "Test");
        graph.add_node(node).unwrap();

        let id = NodeId::new(NodeType::Service, "ns", "test").unwrap();
        assert!(graph.contains_node(&id));

        let removed = graph.remove_node(&id);
        assert!(removed.is_some());
        assert!(!graph.contains_node(&id));
    }

    #[test]
    fn test_add_edge_missing_source() {
        let mut graph = ForgeGraph::new();

        let target = create_test_service("ns", "target", "Target");
        graph.add_node(target).unwrap();

        let result = graph.add_edge(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "source").unwrap(),
                NodeId::new(NodeType::Service, "ns", "target").unwrap(),
                EdgeType::Calls,
            )
            .unwrap(),
        );

        assert!(matches!(result, Err(GraphError::NodeNotFound(_))));
    }

    #[test]
    fn test_duplicate_edge() {
        let mut graph = create_test_graph();

        let result = graph.add_edge(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                EdgeType::Calls,
            )
            .unwrap(),
        );

        assert!(matches!(result, Err(GraphError::DuplicateEdge { .. })));
    }

    #[test]
    fn test_upsert_edge() {
        let mut graph = create_test_graph();

        // Should not error on duplicate
        let result = graph.upsert_edge(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                EdgeType::Calls,
            )
            .unwrap(),
        );

        assert!(result.is_ok());
        // Should still have only one CALLS edge
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_edges_from() {
        let graph = create_test_graph();

        let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let edges = graph.edges_from(&svc_a_id);

        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_edges_to() {
        let graph = create_test_graph();

        let svc_b_id = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();
        let edges = graph.edges_to(&svc_b_id);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::Calls);
    }

    #[test]
    fn test_edges_by_type() {
        let graph = create_test_graph();

        let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let reads_edges = graph.edges_from_by_type(&svc_a_id, EdgeType::Reads);

        assert_eq!(reads_edges.len(), 1);
    }

    #[test]
    fn test_nodes_by_type() {
        let graph = create_test_graph();

        let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
        let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();

        assert_eq!(services.len(), 2);
        assert_eq!(databases.len(), 1);
    }

    #[test]
    fn test_persistence_roundtrip() {
        let graph = create_test_graph();
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test_graph.json");

        graph.save_to_file(&path).unwrap();
        let loaded = ForgeGraph::load_from_file(&path).unwrap();

        assert_eq!(graph.node_count(), loaded.node_count());
        assert_eq!(graph.edge_count(), loaded.edge_count());

        // Verify specific node exists
        let id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        assert!(loaded.get_node(&id).is_some());
    }

    #[test]
    fn test_json_roundtrip() {
        let graph = create_test_graph();

        let json = graph.to_json().unwrap();
        let loaded = ForgeGraph::from_json(&json).unwrap();

        assert_eq!(graph.node_count(), loaded.node_count());
        assert_eq!(graph.edge_count(), loaded.edge_count());
    }

    #[test]
    fn test_default() {
        let graph = ForgeGraph::default();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }
}
