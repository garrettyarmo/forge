//! Query interface for the knowledge graph.

use crate::edge::{Edge, EdgeType};
use crate::graph::ForgeGraph;
use crate::node::{AttributeValue, Node, NodeId};
use petgraph::Direction;
use petgraph::algo::astar;
use petgraph::visit::EdgeRef;
use std::collections::HashSet;

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
        use std::collections::VecDeque;

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
}
