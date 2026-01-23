//! Query interface for the knowledge graph.

use crate::edge::{Edge, EdgeType};
use crate::graph::ForgeGraph;
use crate::node::{AttributeValue, Node, NodeId};
use petgraph::Direction;
use petgraph::algo::astar;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};

/// Direction for edge traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Follow outgoing edges only
    Outgoing,
    /// Follow incoming edges only
    Incoming,
    /// Follow both directions
    Both,
}

/// Configuration for subgraph extraction.
#[derive(Debug, Clone)]
pub struct SubgraphConfig {
    /// Starting nodes for extraction
    pub seed_nodes: Vec<NodeId>,

    /// Maximum hops from seed nodes to include
    pub max_depth: u32,

    /// Include implicit couplings
    pub include_implicit_couplings: bool,

    /// Minimum relevance score to include (0.0 - 1.0)
    pub min_relevance: f64,

    /// Edge types to follow (None = all types)
    pub edge_types: Option<Vec<EdgeType>>,
}

impl Default for SubgraphConfig {
    fn default() -> Self {
        Self {
            seed_nodes: vec![],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.1,
            edge_types: None,
        }
    }
}

/// A node with its relevance score.
#[derive(Debug, Clone)]
pub struct ScoredNode<'a> {
    /// Reference to the node
    pub node: &'a Node,
    /// Relevance score (0.0 - 1.0)
    pub score: f64,
    /// Distance from seed node
    pub depth: u32,
}

/// Result of subgraph extraction.
#[derive(Debug)]
pub struct ExtractedSubgraph<'a> {
    /// Nodes ordered by relevance (highest first)
    pub nodes: Vec<ScoredNode<'a>>,

    /// Edges between included nodes
    pub edges: Vec<&'a Edge>,

    /// Reference to the original graph
    graph: &'a ForgeGraph,
}

impl<'a> ExtractedSubgraph<'a> {
    /// Get a reference to the original graph.
    pub fn graph(&self) -> &'a ForgeGraph {
        self.graph
    }

    /// Get total node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get total edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// Calculate relevance decay based on edge type.
/// Higher values mean the connected node is more relevant.
fn edge_relevance_decay(edge: &Edge) -> f64 {
    match edge.edge_type {
        // Direct dependencies have high relevance
        EdgeType::Calls => 0.8,
        EdgeType::Owns => 0.9,

        // Data access is important
        EdgeType::Reads | EdgeType::Writes => 0.75,
        EdgeType::ReadsShared | EdgeType::WritesShared => 0.7,

        // Message patterns are moderately relevant
        EdgeType::Publishes | EdgeType::Subscribes => 0.65,

        // Generic usage
        EdgeType::Uses => 0.6,

        // Implicit coupling is contextual but not primary
        EdgeType::ImplicitlyCoupled => 0.5,
    }
}

impl ForgeGraph {
    /// Find all nodes connected to a given node via specific edge types.
    ///
    /// # Arguments
    /// * `node_id` - Starting node
    /// * `edge_types` - Edge types to follow (None = all types)
    /// * `direction` - Outgoing, Incoming, or Both
    pub fn traverse_edges(
        &self,
        node_id: &NodeId,
        edge_types: Option<&[EdgeType]>,
        direction: TraversalDirection,
    ) -> Vec<&Node> {
        let Some(&idx) = self.node_index_map().get(node_id) else {
            return vec![];
        };

        let mut result = Vec::new();

        let directions = match direction {
            TraversalDirection::Outgoing => vec![Direction::Outgoing],
            TraversalDirection::Incoming => vec![Direction::Incoming],
            TraversalDirection::Both => vec![Direction::Outgoing, Direction::Incoming],
        };

        for dir in directions {
            for edge_ref in self.inner().edges_directed(idx, dir) {
                let edge = edge_ref.weight();

                // Filter by edge type if specified
                if let Some(types) = edge_types {
                    if !types.contains(&edge.edge_type) {
                        continue;
                    }
                }

                // Get the connected node
                let connected_idx = match dir {
                    Direction::Outgoing => edge_ref.target(),
                    Direction::Incoming => edge_ref.source(),
                };

                result.push(&self.inner()[connected_idx]);
            }
        }

        result
    }

    /// Find the shortest path between two nodes.
    /// Returns None if no path exists.
    pub fn find_path(&self, from: &NodeId, to: &NodeId) -> Option<Vec<&Node>> {
        let start_idx = *self.node_index_map().get(from)?;
        let goal_idx = *self.node_index_map().get(to)?;

        let result = astar(
            self.inner(),
            start_idx,
            |n| n == goal_idx,
            |_| 1, // uniform edge weight
            |_| 0, // no heuristic
        );

        result.map(|(_, path)| path.iter().map(|&idx| &self.inner()[idx]).collect())
    }

    /// Extract a subgraph containing the specified nodes and all edges between them.
    pub fn get_subgraph(&self, node_ids: &[NodeId]) -> ForgeGraph {
        let mut subgraph = ForgeGraph::new();

        // Collect valid indices
        let indices: HashSet<_> = node_ids
            .iter()
            .filter_map(|id| self.node_index_map().get(id).copied())
            .collect();

        // Add nodes
        for &idx in &indices {
            subgraph.add_node(self.inner()[idx].clone()).ok();
        }

        // Add edges where both endpoints are in the subgraph
        for edge in self.inner().edge_weights() {
            if let (Some(&src_idx), Some(&tgt_idx)) = (
                self.node_index_map().get(&edge.source),
                self.node_index_map().get(&edge.target),
            ) {
                if indices.contains(&src_idx) && indices.contains(&tgt_idx) {
                    subgraph.add_edge(edge.clone()).ok();
                }
            }
        }

        subgraph
    }

    /// Find all services that access a shared resource.
    pub fn services_accessing_resource(&self, resource_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(
            resource_id,
            Some(&[
                EdgeType::Reads,
                EdgeType::Writes,
                EdgeType::ReadsShared,
                EdgeType::WritesShared,
                EdgeType::Publishes,
                EdgeType::Subscribes,
                EdgeType::Uses,
            ]),
            TraversalDirection::Incoming,
        )
    }

    /// Get all services/resources that a given service depends on (calls, reads, writes, or uses).
    pub fn dependencies(&self, service_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(
            service_id,
            Some(&[
                EdgeType::Calls,
                EdgeType::Reads,
                EdgeType::Writes,
                EdgeType::Publishes,
                EdgeType::Uses,
            ]),
            TraversalDirection::Outgoing,
        )
    }

    /// Get all services that depend on a given service.
    pub fn dependents(&self, service_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(
            service_id,
            Some(&[EdgeType::Calls]),
            TraversalDirection::Incoming,
        )
    }

    /// Find all IMPLICITLY_COUPLED edges in the graph.
    pub fn implicit_couplings(&self) -> Vec<(&Node, &Node, &Edge)> {
        self.inner()
            .edge_references()
            .filter(|e| e.weight().edge_type == EdgeType::ImplicitlyCoupled)
            .map(|e| {
                let source = &self.inner()[e.source()];
                let target = &self.inner()[e.target()];
                (source, target, e.weight())
            })
            .collect()
    }

    /// Search nodes by attribute value.
    pub fn find_nodes_by_attribute(&self, key: &str, value: &AttributeValue) -> Vec<&Node> {
        self.inner()
            .node_weights()
            .filter(|n| n.attributes.get(key) == Some(value))
            .collect()
    }

    /// Search nodes by display name (case-insensitive substring).
    pub fn find_nodes_by_name(&self, query: &str) -> Vec<&Node> {
        let query_lower = query.to_lowercase();
        self.inner()
            .node_weights()
            .filter(|n| n.display_name.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Get all neighbors of a node (connected via any edge, any direction).
    pub fn neighbors(&self, node_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(node_id, None, TraversalDirection::Both)
    }

    /// Get the depth (hop count) from one node to another.
    /// Returns None if no path exists.
    pub fn distance(&self, from: &NodeId, to: &NodeId) -> Option<usize> {
        self.find_path(from, to)
            .map(|path| path.len().saturating_sub(1))
    }

    /// Find all nodes within a given distance from a starting node.
    pub fn nodes_within_distance(&self, start: &NodeId, max_distance: usize) -> Vec<&Node> {
        let Some(&start_idx) = self.node_index_map().get(start) else {
            return vec![];
        };

        let mut visited = HashSet::new();
        let mut result = Vec::new();
        let mut queue = VecDeque::new();

        queue.push_back((start_idx, 0usize));
        visited.insert(start_idx);

        while let Some((idx, dist)) = queue.pop_front() {
            result.push(&self.inner()[idx]);

            if dist >= max_distance {
                continue;
            }

            // Collect unvisited neighbors first to avoid borrow conflict
            let unvisited_neighbors: Vec<_> = self
                .inner()
                .neighbors_undirected(idx)
                .filter(|n| !visited.contains(n))
                .collect();

            // Visit all neighbors
            for neighbor_idx in unvisited_neighbors {
                visited.insert(neighbor_idx);
                queue.push_back((neighbor_idx, dist + 1));
            }
        }

        result
    }

    /// Extract a relevance-scored subgraph starting from seed nodes.
    ///
    /// Uses BFS with depth-limited relevance decay to identify nodes
    /// most relevant to the seed nodes. Nodes are scored based on their
    /// distance and the types of edges connecting them.
    ///
    /// # Arguments
    /// * `config` - Configuration specifying seed nodes, max depth, and filtering options
    ///
    /// # Returns
    /// An `ExtractedSubgraph` containing scored nodes (sorted by relevance)
    /// and all edges between included nodes.
    pub fn extract_subgraph(&self, config: &SubgraphConfig) -> ExtractedSubgraph<'_> {
        // Track scores: NodeId -> (score, depth)
        let mut node_scores: HashMap<&NodeId, (f64, u32)> = HashMap::new();
        let mut visited: HashSet<&NodeId> = HashSet::new();
        let mut frontier: VecDeque<(&NodeId, u32, f64)> = VecDeque::new();

        // Initialize frontier with seed nodes (full relevance)
        for seed in &config.seed_nodes {
            if self.contains_node(seed) {
                frontier.push_back((seed, 0, 1.0));
            }
        }

        // BFS with depth-limited relevance decay
        while let Some((node_id, depth, score)) = frontier.pop_front() {
            if visited.contains(&node_id) {
                // If we've visited this node before with a lower score, update if this is better
                if let Some(&(existing_score, _)) = node_scores.get(&node_id) {
                    if score > existing_score {
                        node_scores.insert(node_id, (score, depth));
                    }
                }
                continue;
            }
            visited.insert(node_id);

            // Skip if below minimum relevance
            if score < config.min_relevance {
                continue;
            }

            // Record score
            node_scores.insert(node_id, (score, depth));

            // Stop expanding at max depth
            if depth >= config.max_depth {
                continue;
            }

            // Expand via outgoing edges
            for edge in self.edges_from(node_id) {
                // Filter by edge type if specified
                if let Some(ref edge_types) = config.edge_types {
                    if !edge_types.contains(&edge.edge_type) {
                        continue;
                    }
                }

                // Skip implicit couplings if not requested
                if !config.include_implicit_couplings
                    && edge.edge_type == EdgeType::ImplicitlyCoupled
                {
                    continue;
                }

                let neighbor_score = score * edge_relevance_decay(edge);
                frontier.push_back((&edge.target, depth + 1, neighbor_score));
            }

            // Also follow incoming edges (for bidirectional relevance)
            for edge in self.edges_to(node_id) {
                // Filter by edge type if specified
                if let Some(ref edge_types) = config.edge_types {
                    if !edge_types.contains(&edge.edge_type) {
                        continue;
                    }
                }

                // Skip implicit couplings if not requested
                if !config.include_implicit_couplings
                    && edge.edge_type == EdgeType::ImplicitlyCoupled
                {
                    continue;
                }

                // Incoming edges have lower relevance contribution (0.7 multiplier)
                let neighbor_score = score * edge_relevance_decay(edge) * 0.7;
                frontier.push_back((&edge.source, depth + 1, neighbor_score));
            }
        }

        // Build scored nodes list
        let mut nodes: Vec<ScoredNode<'_>> = node_scores
            .iter()
            .filter_map(|(id, &(score, depth))| {
                self.get_node(id).map(|node| ScoredNode { node, score, depth })
            })
            .collect();

        // Sort by relevance score (descending)
        nodes.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Collect edges between included nodes
        let included_ids: HashSet<&NodeId> = node_scores.keys().copied().collect();
        let edges: Vec<&Edge> = self
            .edges()
            .filter(|e| included_ids.contains(&e.source) && included_ids.contains(&e.target))
            .collect();

        ExtractedSubgraph {
            nodes,
            edges,
            graph: self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{DiscoverySource, NodeBuilder, NodeType};
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

        // Add services: A -> B -> C, A -> DB, B -> DB
        graph
            .add_node(create_test_service("ns", "svc-a", "Service A"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "svc-b", "Service B"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "svc-c", "Service C"))
            .unwrap();
        graph
            .add_node(create_test_database("ns", "users-db", "Users DB"))
            .unwrap();

        // A calls B
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

        // B calls C
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "svc-c").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        // A reads DB
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

        // B writes DB
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                    NodeId::new(NodeType::Database, "ns", "users-db").unwrap(),
                    EdgeType::Writes,
                )
                .unwrap(),
            )
            .unwrap();

        graph
    }

    #[test]
    fn test_traverse_edges_outgoing() {
        let graph = create_test_graph();

        let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let connected = graph.traverse_edges(&svc_a_id, None, TraversalDirection::Outgoing);

        assert_eq!(connected.len(), 2); // svc-b and users-db
    }

    #[test]
    fn test_traverse_edges_incoming() {
        let graph = create_test_graph();

        let svc_b_id = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();
        let connected = graph.traverse_edges(&svc_b_id, None, TraversalDirection::Incoming);

        assert_eq!(connected.len(), 1); // Only svc-a
        assert_eq!(connected[0].display_name, "Service A");
    }

    #[test]
    fn test_traverse_edges_both() {
        let graph = create_test_graph();

        let svc_b_id = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();
        let connected = graph.traverse_edges(&svc_b_id, None, TraversalDirection::Both);

        assert_eq!(connected.len(), 3); // svc-a (in), svc-c (out), users-db (out)
    }

    #[test]
    fn test_traverse_edges_with_type_filter() {
        let graph = create_test_graph();

        let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let connected = graph.traverse_edges(
            &svc_a_id,
            Some(&[EdgeType::Calls]),
            TraversalDirection::Outgoing,
        );

        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].display_name, "Service B");
    }

    #[test]
    fn test_find_path() {
        let graph = create_test_graph();

        let from = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let to = NodeId::new(NodeType::Service, "ns", "svc-c").unwrap();

        let path = graph.find_path(&from, &to);
        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(path.len(), 3); // svc-a -> svc-b -> svc-c
        assert_eq!(path[0].display_name, "Service A");
        assert_eq!(path[1].display_name, "Service B");
        assert_eq!(path[2].display_name, "Service C");
    }

    #[test]
    fn test_find_path_no_path() {
        let graph = create_test_graph();

        let from = NodeId::new(NodeType::Service, "ns", "svc-c").unwrap();
        let to = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();

        let path = graph.find_path(&from, &to);
        assert!(path.is_none()); // No path from C to A (edges are directed)
    }

    #[test]
    fn test_get_subgraph() {
        let graph = create_test_graph();

        let ids = vec![
            NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
            NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
        ];

        let subgraph = graph.get_subgraph(&ids);

        assert_eq!(subgraph.node_count(), 2);
        assert_eq!(subgraph.edge_count(), 1); // Only the CALLS edge between A and B
    }

    #[test]
    fn test_services_accessing_resource() {
        let graph = create_test_graph();

        let db_id = NodeId::new(NodeType::Database, "ns", "users-db").unwrap();
        let services = graph.services_accessing_resource(&db_id);

        assert_eq!(services.len(), 2); // svc-a (reads) and svc-b (writes)
    }

    #[test]
    fn test_dependencies() {
        let graph = create_test_graph();

        let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let deps = graph.dependencies(&svc_a_id);

        assert_eq!(deps.len(), 2); // svc-b (calls) and users-db (reads)
    }

    #[test]
    fn test_dependents() {
        let graph = create_test_graph();

        let svc_b_id = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();
        let dependents = graph.dependents(&svc_b_id);

        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].display_name, "Service A");
    }

    #[test]
    fn test_find_nodes_by_name() {
        let graph = create_test_graph();

        let results = graph.find_nodes_by_name("service");
        assert_eq!(results.len(), 3); // Service A, B, C

        let results = graph.find_nodes_by_name("users");
        assert_eq!(results.len(), 1); // Users DB
    }

    #[test]
    fn test_find_nodes_by_attribute() {
        let mut graph = ForgeGraph::new();

        let node = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
            .node_type(NodeType::Service)
            .display_name("Test")
            .attribute("language", "typescript")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(node).unwrap();

        let results =
            graph.find_nodes_by_attribute("language", &AttributeValue::String("typescript".into()));
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_neighbors() {
        let graph = create_test_graph();

        let svc_b_id = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();
        let neighbors = graph.neighbors(&svc_b_id);

        assert_eq!(neighbors.len(), 3); // svc-a, svc-c, users-db
    }

    #[test]
    fn test_distance() {
        let graph = create_test_graph();

        let from = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let to = NodeId::new(NodeType::Service, "ns", "svc-c").unwrap();

        let dist = graph.distance(&from, &to);
        assert_eq!(dist, Some(2)); // A -> B -> C
    }

    #[test]
    fn test_nodes_within_distance() {
        let graph = create_test_graph();

        let start = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let nodes = graph.nodes_within_distance(&start, 1);

        // Should include svc-a (distance 0), svc-b (distance 1), users-db (distance 1)
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_nodes_within_distance_zero() {
        let graph = create_test_graph();

        let start = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let nodes = graph.nodes_within_distance(&start, 0);

        // Should only include svc-a itself
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].display_name, "Service A");
    }

    // === M5-T1: Subgraph Extraction Tests ===

    #[test]
    fn test_extract_subgraph_from_seed() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.1,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Should include svc-a (seed), svc-b (1 hop), users-db (1 hop), svc-c (2 hops)
        assert_eq!(subgraph.node_count(), 4);

        // First node should be the seed with score 1.0
        assert_eq!(subgraph.nodes[0].node.display_name, "Service A");
        assert_eq!(subgraph.nodes[0].score, 1.0);
        assert_eq!(subgraph.nodes[0].depth, 0);
    }

    #[test]
    fn test_extract_subgraph_relevance_decay() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Find svc-b which is 1 hop away via CALLS edge (decay = 0.8)
        let svc_b = subgraph
            .nodes
            .iter()
            .find(|n| n.node.display_name == "Service B")
            .unwrap();
        assert_eq!(svc_b.score, 0.8);
        assert_eq!(svc_b.depth, 1);

        // Find users-db which is 1 hop away via READS edge (decay = 0.75)
        let db = subgraph
            .nodes
            .iter()
            .find(|n| n.node.display_name == "Users DB")
            .unwrap();
        assert_eq!(db.score, 0.75);
        assert_eq!(db.depth, 1);

        // Find svc-c which is 2 hops away via CALLS edges (0.8 * 0.8 = 0.64)
        let svc_c = subgraph
            .nodes
            .iter()
            .find(|n| n.node.display_name == "Service C")
            .unwrap();
        assert!((svc_c.score - 0.64).abs() < 0.001);
        assert_eq!(svc_c.depth, 2);
    }

    #[test]
    fn test_extract_subgraph_max_depth() {
        let graph = create_test_graph();

        // With max_depth=1, should only get seed and direct neighbors
        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Should include svc-a (seed), svc-b (1 hop), users-db (1 hop)
        // svc-c is 2 hops away, should not be included
        assert_eq!(subgraph.node_count(), 3);

        let names: Vec<_> = subgraph
            .nodes
            .iter()
            .map(|n| n.node.display_name.as_str())
            .collect();
        assert!(names.contains(&"Service A"));
        assert!(names.contains(&"Service B"));
        assert!(names.contains(&"Users DB"));
        assert!(!names.contains(&"Service C"));
    }

    #[test]
    fn test_extract_subgraph_min_relevance() {
        let graph = create_test_graph();

        // With high min_relevance, should exclude nodes below threshold
        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 3,
            include_implicit_couplings: true,
            min_relevance: 0.7,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // svc-a (1.0), svc-b (0.8), and users-db (0.75) should pass the 0.7 threshold
        // Note: users-db is discovered via Reads edge from svc-a (score = 1.0 * 0.75 = 0.75)
        assert_eq!(subgraph.node_count(), 3);
    }

    #[test]
    fn test_extract_subgraph_edge_type_filter() {
        let graph = create_test_graph();

        // Only follow CALLS edges
        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: Some(vec![EdgeType::Calls]),
        };

        let subgraph = graph.extract_subgraph(&config);

        // Should include svc-a, svc-b, svc-c (via CALLS chain)
        // Should NOT include users-db (connected via READS/WRITES)
        let names: Vec<_> = subgraph
            .nodes
            .iter()
            .map(|n| n.node.display_name.as_str())
            .collect();
        assert!(names.contains(&"Service A"));
        assert!(names.contains(&"Service B"));
        assert!(names.contains(&"Service C"));
        assert!(!names.contains(&"Users DB"));
    }

    #[test]
    fn test_extract_subgraph_multiple_seeds() {
        let graph = create_test_graph();

        // Start from both svc-a and svc-c
        let config = SubgraphConfig {
            seed_nodes: vec![
                NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "svc-c").unwrap(),
            ],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Both seeds should have score 1.0
        let svc_a = subgraph
            .nodes
            .iter()
            .find(|n| n.node.display_name == "Service A")
            .unwrap();
        let svc_c = subgraph
            .nodes
            .iter()
            .find(|n| n.node.display_name == "Service C")
            .unwrap();
        assert_eq!(svc_a.score, 1.0);
        assert_eq!(svc_c.score, 1.0);
    }

    #[test]
    fn test_extract_subgraph_empty_seeds() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // No seeds means no nodes extracted
        assert_eq!(subgraph.node_count(), 0);
        assert_eq!(subgraph.edge_count(), 0);
    }

    #[test]
    fn test_extract_subgraph_nonexistent_seed() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "nonexistent").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Nonexistent seed is ignored
        assert_eq!(subgraph.node_count(), 0);
    }

    #[test]
    fn test_extract_subgraph_edges_included() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Should include edges between extracted nodes:
        // A -> B (CALLS), A -> DB (READS), B -> DB (WRITES)
        // Note: svc-b is included via outgoing edge, and users-db is included,
        // so the B -> DB edge is also included
        assert_eq!(subgraph.edge_count(), 3);

        let edge_types: Vec<_> = subgraph.edges.iter().map(|e| e.edge_type).collect();
        assert!(edge_types.contains(&EdgeType::Calls));
        assert!(edge_types.contains(&EdgeType::Reads));
        assert!(edge_types.contains(&EdgeType::Writes));
    }

    #[test]
    fn test_extract_subgraph_sorted_by_relevance() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Verify nodes are sorted by relevance (descending)
        let scores: Vec<f64> = subgraph.nodes.iter().map(|n| n.score).collect();
        for i in 1..scores.len() {
            assert!(scores[i] <= scores[i - 1], "Nodes should be sorted by score descending");
        }
    }

    #[test]
    fn test_extract_subgraph_with_implicit_coupling() {
        let mut graph = ForgeGraph::new();

        // Create two services implicitly coupled
        graph
            .add_node(create_test_service("ns", "svc-a", "Service A"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "svc-b", "Service B"))
            .unwrap();

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                    EdgeType::ImplicitlyCoupled,
                )
                .unwrap(),
            )
            .unwrap();

        // With implicit couplings included
        let config_with = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };
        let subgraph_with = graph.extract_subgraph(&config_with);
        assert_eq!(subgraph_with.node_count(), 2);

        // Without implicit couplings
        let config_without = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 1,
            include_implicit_couplings: false,
            min_relevance: 0.0,
            edge_types: None,
        };
        let subgraph_without = graph.extract_subgraph(&config_without);
        assert_eq!(subgraph_without.node_count(), 1); // Only seed
    }

    #[test]
    fn test_subgraph_config_default() {
        let config = SubgraphConfig::default();

        assert!(config.seed_nodes.is_empty());
        assert_eq!(config.max_depth, 2);
        assert!(config.include_implicit_couplings);
        assert!((config.min_relevance - 0.1).abs() < 0.001);
        assert!(config.edge_types.is_none());
    }

    #[test]
    fn test_extracted_subgraph_methods() {
        let graph = create_test_graph();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "svc-a").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Test accessor methods
        // nodes: svc-a, svc-b (via CALLS), users-db (via READS)
        assert_eq!(subgraph.node_count(), 3);
        // edges: A->B (CALLS), A->DB (READS), B->DB (WRITES)
        assert_eq!(subgraph.edge_count(), 3);
        assert_eq!(subgraph.graph().node_count(), 4); // Original graph still has 4 nodes
    }

    #[test]
    fn test_edge_relevance_decay_values() {
        // Create edges of different types and verify decay values
        let edge_calls = Edge::new(
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::Service, "ns", "b").unwrap(),
            EdgeType::Calls,
        )
        .unwrap();
        assert_eq!(edge_relevance_decay(&edge_calls), 0.8);

        let edge_reads = Edge::new(
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();
        assert_eq!(edge_relevance_decay(&edge_reads), 0.75);

        let edge_publishes = Edge::new(
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::Queue, "ns", "q").unwrap(),
            EdgeType::Publishes,
        )
        .unwrap();
        assert_eq!(edge_relevance_decay(&edge_publishes), 0.65);

        let edge_uses = Edge::new(
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::CloudResource, "ns", "bucket").unwrap(),
            EdgeType::Uses,
        )
        .unwrap();
        assert_eq!(edge_relevance_decay(&edge_uses), 0.6);
    }
}
