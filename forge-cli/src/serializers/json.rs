//! JSON serializer for knowledge graphs.
//!
//! Produces structured JSON output for programmatic access and tool-based
//! LLM queries. The output follows a documented schema optimized for
//! agent consumption.
//!
//! ## Output Schema
//!
//! ```json
//! {
//!   "$schema": "https://forge.dev/schemas/graph-v1.json",
//!   "version": "1.0.0",
//!   "generated_at": "2024-01-15T10:30:00Z",
//!   "query": { "type": "full" },
//!   "nodes": [...],
//!   "edges": [...],
//!   "summary": { "total_nodes": 5, "total_edges": 8, "by_type": {...} }
//! }
//! ```

use chrono::Utc;
use forge_graph::{EdgeType, ExtractedSubgraph, ForgeGraph, Node, NodeType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// JSON output structure for serialized graphs.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonOutput {
    /// JSON Schema reference
    #[serde(rename = "$schema")]
    pub schema: String,

    /// Schema version
    pub version: String,

    /// Timestamp when output was generated
    pub generated_at: String,

    /// Query information (if filtered)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryInfo>,

    /// All nodes in the graph/subgraph
    pub nodes: Vec<JsonNode>,

    /// All edges in the graph/subgraph
    pub edges: Vec<JsonEdge>,

    /// Summary statistics
    pub summary: Summary,
}

/// Information about the query that produced this output.
#[derive(Debug, Serialize, Deserialize)]
pub struct QueryInfo {
    /// Type of query: "full", "subgraph", "service_filter"
    #[serde(rename = "type")]
    pub query_type: String,

    /// Seed node IDs for subgraph extraction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seeds: Option<Vec<String>>,

    /// Maximum depth for subgraph extraction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

/// A node in JSON format.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonNode {
    /// Unique node identifier
    pub id: String,

    /// Node type (service, database, queue, etc.)
    #[serde(rename = "type")]
    pub node_type: String,

    /// Human-readable name
    pub name: String,

    /// Relevance score (only for subgraph extraction)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relevance: Option<f64>,

    /// Node attributes
    pub attributes: serde_json::Value,

    /// Business context (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub business_context: Option<serde_json::Value>,
}

/// An edge in JSON format.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonEdge {
    /// Source node ID
    pub source: String,

    /// Target node ID
    pub target: String,

    /// Edge type
    #[serde(rename = "type")]
    pub edge_type: String,

    /// Edge metadata
    pub metadata: serde_json::Value,
}

/// Summary statistics for the output.
#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    /// Total number of nodes
    pub total_nodes: usize,

    /// Total number of edges
    pub total_edges: usize,

    /// Node counts by type
    pub by_type: HashMap<String, usize>,
}

/// JSON serializer for knowledge graphs.
#[derive(Debug, Clone, Default)]
pub struct JsonSerializer;

impl JsonSerializer {
    /// Create a new JsonSerializer.
    pub fn new() -> Self {
        Self
    }

    /// Serialize an entire graph to JSON.
    pub fn serialize_graph(&self, graph: &ForgeGraph) -> String {
        let output = self.build_graph_output(graph);
        serde_json::to_string_pretty(&output)
            .unwrap_or_else(|e| format!("{{\"error\": \"Failed to serialize: {}\"}}", e))
    }

    /// Serialize an extracted subgraph to JSON.
    pub fn serialize_subgraph(
        &self,
        subgraph: &ExtractedSubgraph<'_>,
        query_info: Option<QueryInfo>,
    ) -> String {
        let output = self.build_subgraph_output(subgraph, query_info);
        serde_json::to_string_pretty(&output)
            .unwrap_or_else(|e| format!("{{\"error\": \"Failed to serialize: {}\"}}", e))
    }

    /// Build JSON output for a full graph.
    fn build_graph_output(&self, graph: &ForgeGraph) -> JsonOutput {
        let mut by_type: HashMap<String, usize> = HashMap::new();

        let nodes: Vec<JsonNode> = graph
            .nodes()
            .map(|node| {
                let type_str = node_type_to_string(node.node_type);
                *by_type.entry(type_str.clone()).or_insert(0) += 1;
                self.node_to_json(node, None)
            })
            .collect();

        let edges: Vec<JsonEdge> = graph.edges().map(|edge| self.edge_to_json(edge)).collect();

        JsonOutput {
            schema: "https://forge.dev/schemas/graph-v1.json".to_string(),
            version: "1.0.0".to_string(),
            generated_at: Utc::now().to_rfc3339(),
            query: Some(QueryInfo {
                query_type: "full".to_string(),
                seeds: None,
                max_depth: None,
            }),
            summary: Summary {
                total_nodes: nodes.len(),
                total_edges: edges.len(),
                by_type,
            },
            nodes,
            edges,
        }
    }

    /// Build JSON output for an extracted subgraph.
    fn build_subgraph_output(
        &self,
        subgraph: &ExtractedSubgraph<'_>,
        query_info: Option<QueryInfo>,
    ) -> JsonOutput {
        let mut by_type: HashMap<String, usize> = HashMap::new();

        let nodes: Vec<JsonNode> = subgraph
            .nodes
            .iter()
            .map(|scored| {
                let type_str = node_type_to_string(scored.node.node_type);
                *by_type.entry(type_str.clone()).or_insert(0) += 1;
                self.node_to_json(scored.node, Some(scored.score))
            })
            .collect();

        let edges: Vec<JsonEdge> = subgraph
            .edges
            .iter()
            .map(|edge| self.edge_to_json(edge))
            .collect();

        JsonOutput {
            schema: "https://forge.dev/schemas/graph-v1.json".to_string(),
            version: "1.0.0".to_string(),
            generated_at: Utc::now().to_rfc3339(),
            query: query_info.or_else(|| {
                Some(QueryInfo {
                    query_type: "subgraph".to_string(),
                    seeds: None,
                    max_depth: None,
                })
            }),
            summary: Summary {
                total_nodes: nodes.len(),
                total_edges: edges.len(),
                by_type,
            },
            nodes,
            edges,
        }
    }

    /// Convert a Node to JsonNode.
    fn node_to_json(&self, node: &Node, relevance: Option<f64>) -> JsonNode {
        let business_context = node
            .business_context
            .as_ref()
            .and_then(|bc| serde_json::to_value(bc).ok());

        JsonNode {
            id: node.id.as_str().to_string(),
            node_type: node_type_to_string(node.node_type),
            name: node.display_name.clone(),
            relevance,
            attributes: serde_json::to_value(&node.attributes).unwrap_or(serde_json::Value::Null),
            business_context,
        }
    }

    /// Convert an Edge to JsonEdge.
    fn edge_to_json(&self, edge: &forge_graph::Edge) -> JsonEdge {
        JsonEdge {
            source: edge.source.as_str().to_string(),
            target: edge.target.as_str().to_string(),
            edge_type: edge_type_to_string(edge.edge_type),
            metadata: serde_json::to_value(&edge.metadata).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// Convert NodeType to string representation.
fn node_type_to_string(node_type: NodeType) -> String {
    match node_type {
        NodeType::Service => "service".to_string(),
        NodeType::Api => "api".to_string(),
        NodeType::Database => "database".to_string(),
        NodeType::Queue => "queue".to_string(),
        NodeType::CloudResource => "cloud_resource".to_string(),
    }
}

/// Convert EdgeType to string representation.
fn edge_type_to_string(edge_type: EdgeType) -> String {
    match edge_type {
        EdgeType::Calls => "CALLS".to_string(),
        EdgeType::Owns => "OWNS".to_string(),
        EdgeType::Reads => "READS".to_string(),
        EdgeType::Writes => "WRITES".to_string(),
        EdgeType::Publishes => "PUBLISHES".to_string(),
        EdgeType::Subscribes => "SUBSCRIBES".to_string(),
        EdgeType::Uses => "USES".to_string(),
        EdgeType::ReadsShared => "READS_SHARED".to_string(),
        EdgeType::WritesShared => "WRITES_SHARED".to_string(),
        EdgeType::ImplicitlyCoupled => "IMPLICITLY_COUPLED".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_graph::{DiscoverySource, Edge, NodeBuilder, NodeId, SubgraphConfig};

    fn create_test_service(namespace: &str, name: &str, display: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, namespace, name).unwrap())
            .node_type(NodeType::Service)
            .display_name(display)
            .attribute("language", "typescript")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    fn create_test_database(namespace: &str, name: &str, display: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, namespace, name).unwrap())
            .node_type(NodeType::Database)
            .display_name(display)
            .attribute("db_type", "dynamodb")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    fn create_test_queue(namespace: &str, name: &str, display: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Queue, namespace, name).unwrap())
            .node_type(NodeType::Queue)
            .display_name(display)
            .attribute("queue_type", "sqs")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    fn create_test_graph() -> ForgeGraph {
        let mut graph = ForgeGraph::new();

        // Add services
        graph
            .add_node(create_test_service("ns", "user-api", "User API"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "order-api", "Order API"))
            .unwrap();

        // Add database
        graph
            .add_node(create_test_database("ns", "users-table", "Users Table"))
            .unwrap();

        // Add queue
        graph
            .add_node(create_test_queue("ns", "order-events", "Order Events"))
            .unwrap();

        // Add edges
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "user-api").unwrap(),
                    NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
                    EdgeType::Reads,
                )
                .unwrap(),
            )
            .unwrap();

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "user-api").unwrap(),
                    NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
                    EdgeType::Writes,
                )
                .unwrap(),
            )
            .unwrap();

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "order-api").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "user-api").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "order-api").unwrap(),
                    NodeId::new(NodeType::Queue, "ns", "order-events").unwrap(),
                    EdgeType::Publishes,
                )
                .unwrap(),
            )
            .unwrap();

        graph
    }

    #[test]
    fn test_serialize_graph_basic() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // Should have required fields
        assert!(parsed.get("$schema").is_some());
        assert!(parsed.get("version").is_some());
        assert!(parsed.get("generated_at").is_some());
        assert!(parsed.get("nodes").is_some());
        assert!(parsed.get("edges").is_some());
        assert!(parsed.get("summary").is_some());
    }

    #[test]
    fn test_serialize_graph_nodes() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.nodes.len(), 4);

        // Check node types are present
        let types: Vec<&str> = parsed.nodes.iter().map(|n| n.node_type.as_str()).collect();
        assert!(types.contains(&"service"));
        assert!(types.contains(&"database"));
        assert!(types.contains(&"queue"));

        // Check a specific node
        let user_api = parsed.nodes.iter().find(|n| n.name == "User API").unwrap();
        assert_eq!(user_api.id, "service:ns:user-api");
        assert_eq!(user_api.node_type, "service");
        assert!(user_api.relevance.is_none()); // Full graph has no relevance scores
    }

    #[test]
    fn test_serialize_graph_edges() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.edges.len(), 4);

        // Check edge types
        let types: Vec<&str> = parsed.edges.iter().map(|e| e.edge_type.as_str()).collect();
        assert!(types.contains(&"READS"));
        assert!(types.contains(&"WRITES"));
        assert!(types.contains(&"CALLS"));
        assert!(types.contains(&"PUBLISHES"));
    }

    #[test]
    fn test_serialize_graph_summary() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.summary.total_nodes, 4);
        assert_eq!(parsed.summary.total_edges, 4);
        assert_eq!(parsed.summary.by_type.get("service"), Some(&2));
        assert_eq!(parsed.summary.by_type.get("database"), Some(&1));
        assert_eq!(parsed.summary.by_type.get("queue"), Some(&1));
    }

    #[test]
    fn test_serialize_subgraph() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let output = serializer.serialize_subgraph(&subgraph, None);

        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        // Should have nodes with relevance scores
        assert!(!parsed.nodes.is_empty());
        let user_api = parsed.nodes.iter().find(|n| n.name == "User API").unwrap();
        assert!(user_api.relevance.is_some());
        assert_eq!(user_api.relevance.unwrap(), 1.0); // Seed has full relevance
    }

    #[test]
    fn test_serialize_subgraph_with_query_info() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let seed_ids = vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()];
        let config = SubgraphConfig {
            seed_nodes: seed_ids.clone(),
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        let query_info = QueryInfo {
            query_type: "service_filter".to_string(),
            seeds: Some(seed_ids.iter().map(|id| id.as_str().to_string()).collect()),
            max_depth: Some(2),
        };

        let output = serializer.serialize_subgraph(&subgraph, Some(query_info));
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        assert!(parsed.query.is_some());
        let query = parsed.query.unwrap();
        assert_eq!(query.query_type, "service_filter");
        assert!(query.seeds.is_some());
        assert_eq!(query.max_depth, Some(2));
    }

    #[test]
    fn test_serialize_empty_graph() {
        let graph = ForgeGraph::new();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.summary.total_nodes, 0);
        assert_eq!(parsed.summary.total_edges, 0);
        assert!(parsed.nodes.is_empty());
        assert!(parsed.edges.is_empty());
    }

    #[test]
    fn test_node_type_to_string() {
        assert_eq!(node_type_to_string(NodeType::Service), "service");
        assert_eq!(node_type_to_string(NodeType::Api), "api");
        assert_eq!(node_type_to_string(NodeType::Database), "database");
        assert_eq!(node_type_to_string(NodeType::Queue), "queue");
        assert_eq!(
            node_type_to_string(NodeType::CloudResource),
            "cloud_resource"
        );
    }

    #[test]
    fn test_edge_type_to_string() {
        assert_eq!(edge_type_to_string(EdgeType::Calls), "CALLS");
        assert_eq!(edge_type_to_string(EdgeType::Owns), "OWNS");
        assert_eq!(edge_type_to_string(EdgeType::Reads), "READS");
        assert_eq!(edge_type_to_string(EdgeType::Writes), "WRITES");
        assert_eq!(edge_type_to_string(EdgeType::Publishes), "PUBLISHES");
        assert_eq!(edge_type_to_string(EdgeType::Subscribes), "SUBSCRIBES");
        assert_eq!(edge_type_to_string(EdgeType::Uses), "USES");
        assert_eq!(edge_type_to_string(EdgeType::ReadsShared), "READS_SHARED");
        assert_eq!(edge_type_to_string(EdgeType::WritesShared), "WRITES_SHARED");
        assert_eq!(
            edge_type_to_string(EdgeType::ImplicitlyCoupled),
            "IMPLICITLY_COUPLED"
        );
    }

    #[test]
    fn test_serialize_graph_with_business_context() {
        let mut graph = ForgeGraph::new();

        let mut service = create_test_service("ns", "auth-api", "Auth API");
        service.business_context = Some(forge_graph::BusinessContext {
            purpose: Some("Handles authentication".to_string()),
            owner: Some("Platform Team".to_string()),
            history: Some("Migrated in 2023".to_string()),
            gotchas: vec!["Rate limited".to_string()],
            notes: Default::default(),
        });

        graph.add_node(service).unwrap();

        let serializer = JsonSerializer::new();
        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        let auth_api = parsed.nodes.iter().find(|n| n.name == "Auth API").unwrap();
        assert!(auth_api.business_context.is_some());

        let bc = auth_api.business_context.as_ref().unwrap();
        assert_eq!(
            bc.get("purpose"),
            Some(&serde_json::json!("Handles authentication"))
        );
        assert_eq!(bc.get("owner"), Some(&serde_json::json!("Platform Team")));
    }

    #[test]
    fn test_json_output_is_deserializable() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should be able to deserialize back to JsonOutput
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        // And serialize again
        let output2 = serde_json::to_string_pretty(&parsed).unwrap();
        let parsed2: JsonOutput = serde_json::from_str(&output2).unwrap();

        assert_eq!(parsed.nodes.len(), parsed2.nodes.len());
        assert_eq!(parsed.edges.len(), parsed2.edges.len());
    }

    #[test]
    fn test_schema_and_version() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.schema, "https://forge.dev/schemas/graph-v1.json");
        assert_eq!(parsed.version, "1.0.0");
    }

    #[test]
    fn test_generated_at_is_valid_timestamp() {
        let graph = create_test_graph();
        let serializer = JsonSerializer::new();

        let output = serializer.serialize_graph(&graph);
        let parsed: JsonOutput = serde_json::from_str(&output).unwrap();

        // Should be able to parse as RFC3339 timestamp
        let result = chrono::DateTime::parse_from_rfc3339(&parsed.generated_at);
        assert!(result.is_ok());
    }
}
