//! # forge-graph
//!
//! Knowledge graph data structures for Forge.
//!
//! This crate provides the core graph infrastructure for the Forge ecosystem mapper.
//! It includes:
//!
//! - **Node types**: Service, API, Database, Queue, CloudResource
//! - **Edge types**: Calls, Owns, Reads, Writes, Publishes, Subscribes, Uses, etc.
//! - **ForgeGraph**: The main graph container with full CRUD operations
//! - **Query interface**: Traversal, path finding, subgraph extraction
//! - **Serialization**: JSON persistence for graphs
//!
//! ## Example
//!
//! ```rust
//! use forge_graph::{ForgeGraph, Node, NodeId, NodeType, Edge, EdgeType, NodeBuilder, DiscoverySource};
//!
//! // Create a new graph
//! let mut graph = ForgeGraph::new();
//!
//! // Add a service node
//! let service = NodeBuilder::new()
//!     .id(NodeId::new(NodeType::Service, "acme", "user-api").unwrap())
//!     .node_type(NodeType::Service)
//!     .display_name("User API")
//!     .attribute("language", "typescript")
//!     .source(DiscoverySource::Manual)
//!     .build()
//!     .unwrap();
//!
//! graph.add_node(service).unwrap();
//!
//! // Add a database node
//! let database = NodeBuilder::new()
//!     .id(NodeId::new(NodeType::Database, "acme", "users-db").unwrap())
//!     .node_type(NodeType::Database)
//!     .display_name("Users Database")
//!     .source(DiscoverySource::Manual)
//!     .build()
//!     .unwrap();
//!
//! graph.add_node(database).unwrap();
//!
//! // Add an edge
//! let edge = Edge::new(
//!     NodeId::new(NodeType::Service, "acme", "user-api").unwrap(),
//!     NodeId::new(NodeType::Database, "acme", "users-db").unwrap(),
//!     EdgeType::Reads,
//! ).unwrap();
//!
//! graph.add_edge(edge).unwrap();
//!
//! // Query the graph
//! assert_eq!(graph.node_count(), 2);
//! assert_eq!(graph.edge_count(), 1);
//! ```

// Module declarations - order matters due to dependencies
pub mod edge;
pub mod error;
pub mod graph;
pub mod node;
pub mod query;

// Re-exports for convenient access
pub use edge::{Edge, EdgeMetadata, EdgeType};
pub use error::{EdgeError, GraphError};
pub use graph::{ForgeGraph, GraphMetadata, GraphSnapshot};
pub use node::{
    AttributeValue, BusinessContext, DiscoverySource, Node, NodeBuilder, NodeBuilderError, NodeId,
    NodeIdError, NodeMetadata, NodeType,
};
pub use query::TraversalDirection;

#[cfg(test)]
mod integration_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Test that demonstrates the complete workflow from the M1 spec.
    #[test]
    fn test_complete_workflow() {
        // Create graph
        let mut graph = ForgeGraph::new();

        // Add services
        let user_api = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "acme", "user-api").unwrap())
            .node_type(NodeType::Service)
            .display_name("User API")
            .attribute("repo_url", "https://github.com/acme/user-api")
            .attribute("language", "typescript")
            .attribute("framework", "express")
            .source(DiscoverySource::JavaScriptParser)
            .source_file("src/index.ts")
            .build()
            .unwrap();

        let order_api = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "acme", "order-api").unwrap())
            .node_type(NodeType::Service)
            .display_name("Order API")
            .attribute("language", "python")
            .source(DiscoverySource::PythonParser)
            .build()
            .unwrap();

        // Add database
        let users_db = NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, "acme", "users-table").unwrap())
            .node_type(NodeType::Database)
            .display_name("Users Table")
            .attribute("db_type", "dynamodb")
            .attribute("table_name", "acme-users")
            .source(DiscoverySource::TerraformParser)
            .source_file("terraform/dynamodb.tf")
            .build()
            .unwrap();

        // Add queue
        let order_events = NodeBuilder::new()
            .id(NodeId::new(NodeType::Queue, "acme", "order-events").unwrap())
            .node_type(NodeType::Queue)
            .display_name("Order Events")
            .attribute("queue_type", "sqs")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap();

        graph.add_node(user_api).unwrap();
        graph.add_node(order_api).unwrap();
        graph.add_node(users_db).unwrap();
        graph.add_node(order_events).unwrap();

        // Add edges
        // user-api reads users-table
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "acme", "user-api").unwrap(),
                    NodeId::new(NodeType::Database, "acme", "users-table").unwrap(),
                    EdgeType::Reads,
                )
                .unwrap(),
            )
            .unwrap();

        // user-api writes users-table
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "acme", "user-api").unwrap(),
                    NodeId::new(NodeType::Database, "acme", "users-table").unwrap(),
                    EdgeType::Writes,
                )
                .unwrap(),
            )
            .unwrap();

        // order-api calls user-api
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "acme", "order-api").unwrap(),
                    NodeId::new(NodeType::Service, "acme", "user-api").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        // order-api publishes to order-events
        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "acme", "order-api").unwrap(),
                    NodeId::new(NodeType::Queue, "acme", "order-events").unwrap(),
                    EdgeType::Publishes,
                )
                .unwrap(),
            )
            .unwrap();

        // Verify graph structure
        assert_eq!(graph.node_count(), 4);
        assert_eq!(graph.edge_count(), 4);

        // Test queries
        let user_api_id = NodeId::new(NodeType::Service, "acme", "user-api").unwrap();

        // Dependencies of user-api
        let deps = graph.dependencies(&user_api_id);
        assert_eq!(deps.len(), 2); // users-table (reads) and users-table (writes)

        // Dependents of user-api
        let dependents = graph.dependents(&user_api_id);
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].display_name, "Order API");

        // Services accessing users-table
        let db_id = NodeId::new(NodeType::Database, "acme", "users-table").unwrap();
        let accessors = graph.services_accessing_resource(&db_id);
        assert_eq!(accessors.len(), 2); // user-api reads and writes

        // Test serialization roundtrip
        let json = graph.to_json().unwrap();
        let loaded = ForgeGraph::from_json(&json).unwrap();
        assert_eq!(graph.node_count(), loaded.node_count());
        assert_eq!(graph.edge_count(), loaded.edge_count());

        // Verify node attributes preserved
        let loaded_user_api = loaded.get_node(&user_api_id).unwrap();
        assert_eq!(
            loaded_user_api.attributes.get("language"),
            Some(&AttributeValue::String("typescript".to_string()))
        );
    }

    /// Test subgraph extraction.
    #[test]
    fn test_subgraph_extraction() {
        let mut graph = ForgeGraph::new();

        // Create a chain: A -> B -> C
        for name in ["a", "b", "c"] {
            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", name).unwrap())
                .node_type(NodeType::Service)
                .display_name(format!("Service {}", name.to_uppercase()))
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();
            graph.add_node(node).unwrap();
        }

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "a").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "b").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        graph
            .add_edge(
                Edge::new(
                    NodeId::new(NodeType::Service, "ns", "b").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "c").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        // Extract subgraph with A and B only
        let subgraph = graph.get_subgraph(&[
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::Service, "ns", "b").unwrap(),
        ]);

        assert_eq!(subgraph.node_count(), 2);
        assert_eq!(subgraph.edge_count(), 1); // Only A -> B, not B -> C
    }

    /// Test path finding.
    #[test]
    fn test_path_finding() {
        let mut graph = ForgeGraph::new();

        // Create diamond: A -> B, A -> C, B -> D, C -> D
        for name in ["a", "b", "c", "d"] {
            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", name).unwrap())
                .node_type(NodeType::Service)
                .display_name(format!("Service {}", name.to_uppercase()))
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();
            graph.add_node(node).unwrap();
        }

        let edges = [("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")];
        for (src, tgt) in edges {
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", src).unwrap(),
                        NodeId::new(NodeType::Service, "ns", tgt).unwrap(),
                        EdgeType::Calls,
                    )
                    .unwrap(),
                )
                .unwrap();
        }

        let a = NodeId::new(NodeType::Service, "ns", "a").unwrap();
        let d = NodeId::new(NodeType::Service, "ns", "d").unwrap();

        let path = graph.find_path(&a, &d).unwrap();
        assert_eq!(path.len(), 3); // A -> B -> D or A -> C -> D
        assert_eq!(path[0].display_name, "Service A");
        assert_eq!(path[2].display_name, "Service D");
    }

    /// Test file persistence.
    #[test]
    fn test_file_persistence() {
        let mut graph = ForgeGraph::new();

        let node = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
            .node_type(NodeType::Service)
            .display_name("Test Service")
            .attribute("key", "value")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(node).unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.json");

        graph.save_to_file(&path).unwrap();

        // Verify file exists and contains valid JSON
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"forge_version\""));
        assert!(contents.contains("\"Test Service\""));

        // Load and verify
        let loaded = ForgeGraph::load_from_file(&path).unwrap();
        assert_eq!(loaded.node_count(), 1);
    }

    /// Test business context.
    #[test]
    fn test_business_context() {
        let mut graph = ForgeGraph::new();

        let business_ctx = BusinessContext {
            purpose: Some("Handles user authentication".to_string()),
            owner: Some("Platform Team".to_string()),
            history: Some("Migrated from monolith in 2023".to_string()),
            gotchas: vec!["Rate limited to 100 req/s".to_string()],
            notes: Default::default(),
        };

        let node = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "auth").unwrap())
            .node_type(NodeType::Service)
            .display_name("Auth Service")
            .business_context(business_ctx)
            .source(DiscoverySource::Interview)
            .build()
            .unwrap();

        graph.add_node(node).unwrap();

        let id = NodeId::new(NodeType::Service, "ns", "auth").unwrap();
        let retrieved = graph.get_node(&id).unwrap();

        let ctx = retrieved.business_context.as_ref().unwrap();
        assert_eq!(ctx.purpose, Some("Handles user authentication".to_string()));
        assert_eq!(ctx.owner, Some("Platform Team".to_string()));
        assert_eq!(ctx.gotchas.len(), 1);
    }
}
