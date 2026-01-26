//! Mermaid serializer for knowledge graphs.
//!
//! Produces Mermaid flowchart diagram syntax for visual representation
//! of knowledge graphs. Optimized for documentation and human review.
//!
//! ## Output Format
//!
//! The output follows Mermaid's flowchart syntax:
//!
//! ```mermaid
//! flowchart LR
//!     subgraph Services
//!         svc_user[user-service<br/>TypeScript/Express]
//!     end
//!     subgraph Databases
//!         db_users[(users-table<br/>DynamoDB)]
//!     end
//!     svc_user -->|READS| db_users
//! ```
//!
//! ## Node Shapes
//!
//! - **Services**: Rectangle `[name]`
//! - **Databases**: Cylinder `[(name)]`
//! - **Queues**: Asymmetric `>name]`
//! - **Cloud Resources**: Hexagon `{{name}}`
//! - **APIs**: Stadium `([name])`
//!
//! ## Edge Styles
//!
//! - Normal edges: `-->` (solid line with arrow)
//! - Implicit couplings: `-.->` (dotted line with arrow)

use forge_graph::{EdgeType, ExtractedSubgraph, ForgeGraph, Node, NodeType};
use std::fmt::Write;

/// Direction for the flowchart layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Left to Right
    #[default]
    LR,
    /// Right to Left
    RL,
    /// Top to Bottom
    TB,
    /// Bottom to Top
    BT,
}

impl Direction {
    /// Convert to Mermaid direction string.
    fn as_str(&self) -> &'static str {
        match self {
            Direction::LR => "LR",
            Direction::RL => "RL",
            Direction::TB => "TB",
            Direction::BT => "BT",
        }
    }
}

/// Mermaid serializer for knowledge graphs.
///
/// Converts `ForgeGraph` and `ExtractedSubgraph` instances into
/// Mermaid flowchart diagram syntax.
#[derive(Debug, Clone)]
pub struct MermaidSerializer {
    /// Flow direction
    direction: Direction,

    /// Include node attributes in labels
    include_attributes: bool,

    /// Maximum nodes before simplifying output
    max_nodes: usize,

    /// Include style classes
    include_styles: bool,

    /// Number of days after which a node is considered stale (0 = disabled)
    staleness_days: u32,
}

impl Default for MermaidSerializer {
    fn default() -> Self {
        Self {
            direction: Direction::LR,
            include_attributes: true,
            max_nodes: 50,
            include_styles: true,
            staleness_days: 7,
        }
    }
}

impl MermaidSerializer {
    /// Create a new MermaidSerializer with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the flow direction.
    pub fn with_direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Set whether to include attributes in node labels.
    pub fn with_attributes(mut self, include: bool) -> Self {
        self.include_attributes = include;
        self
    }

    /// Set the maximum number of nodes before simplifying.
    pub fn with_max_nodes(mut self, max: usize) -> Self {
        self.max_nodes = max;
        self
    }

    /// Set whether to include style classes.
    pub fn with_styles(mut self, include: bool) -> Self {
        self.include_styles = include;
        self
    }

    /// Set the staleness threshold in days (0 to disable staleness indicators).
    pub fn with_staleness_days(mut self, days: u32) -> Self {
        self.staleness_days = days;
        self
    }

    /// Serialize an entire graph to Mermaid syntax.
    pub fn serialize_graph(&self, graph: &ForgeGraph) -> String {
        let mut output = String::new();

        writeln!(output, "flowchart {}", self.direction.as_str()).unwrap();

        // Group nodes by type into subgraphs
        self.write_services_subgraph(&mut output, graph);
        self.write_databases_subgraph(&mut output, graph);
        self.write_queues_subgraph(&mut output, graph);
        self.write_cloud_resources_subgraph(&mut output, graph);
        self.write_apis_subgraph(&mut output, graph);

        // Write edges
        writeln!(output).unwrap();
        self.write_edges(&mut output, graph);

        // Write style classes
        if self.include_styles {
            writeln!(output).unwrap();
            self.write_style_classes(&mut output, graph);
        }

        output
    }

    /// Serialize an extracted subgraph to Mermaid syntax.
    pub fn serialize_subgraph(&self, subgraph: &ExtractedSubgraph<'_>) -> String {
        let mut output = String::new();

        writeln!(output, "flowchart {}", self.direction.as_str()).unwrap();

        // Collect nodes by type
        let mut services: Vec<&Node> = vec![];
        let mut databases: Vec<&Node> = vec![];
        let mut queues: Vec<&Node> = vec![];
        let mut resources: Vec<&Node> = vec![];
        let mut apis: Vec<&Node> = vec![];

        for scored in &subgraph.nodes {
            match scored.node.node_type {
                NodeType::Service => services.push(scored.node),
                NodeType::Database => databases.push(scored.node),
                NodeType::Queue => queues.push(scored.node),
                NodeType::CloudResource => resources.push(scored.node),
                NodeType::Api => apis.push(scored.node),
            }
        }

        // Write subgraphs
        if !services.is_empty() {
            self.write_node_subgraph(&mut output, "Services", &services);
        }
        if !databases.is_empty() {
            self.write_node_subgraph(&mut output, "Databases", &databases);
        }
        if !queues.is_empty() {
            self.write_node_subgraph(&mut output, "Queues", &queues);
        }
        if !resources.is_empty() {
            self.write_node_subgraph(&mut output, "Resources", &resources);
        }
        if !apis.is_empty() {
            self.write_node_subgraph(&mut output, "APIs", &apis);
        }

        // Write edges
        writeln!(output).unwrap();
        for edge in &subgraph.edges {
            let source_id = sanitize_id(edge.source.as_str());
            let target_id = sanitize_id(edge.target.as_str());
            let label = edge_type_label(edge.edge_type);

            let line_style = if edge.edge_type == EdgeType::ImplicitlyCoupled {
                "-.->"
            } else {
                "-->"
            };

            writeln!(
                output,
                "    {} {}|{}| {}",
                source_id, line_style, label, target_id
            )
            .unwrap();
        }

        // Write style classes
        if self.include_styles {
            writeln!(output).unwrap();
            self.write_style_classes_for_nodes(
                &mut output,
                &services,
                &databases,
                &queues,
                &resources,
            );
        }

        output
    }

    fn write_services_subgraph(&self, output: &mut String, graph: &ForgeGraph) {
        let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
        if services.is_empty() {
            return;
        }

        writeln!(output, "    subgraph Services").unwrap();

        for service in &services {
            let id = sanitize_id(service.id.as_str());
            let label = self.build_service_label(service);
            writeln!(output, "        {}[{}]", id, label).unwrap();
        }

        writeln!(output, "    end").unwrap();
    }

    fn write_databases_subgraph(&self, output: &mut String, graph: &ForgeGraph) {
        let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
        if databases.is_empty() {
            return;
        }

        writeln!(output, "    subgraph Databases").unwrap();

        for db in &databases {
            let id = sanitize_id(db.id.as_str());
            let label = self.build_database_label(db);
            // Use cylinder shape for databases
            writeln!(output, "        {}[(\"{}\")]", id, label).unwrap();
        }

        writeln!(output, "    end").unwrap();
    }

    fn write_queues_subgraph(&self, output: &mut String, graph: &ForgeGraph) {
        let queues: Vec<_> = graph.nodes_by_type(NodeType::Queue).collect();
        if queues.is_empty() {
            return;
        }

        writeln!(output, "    subgraph Queues").unwrap();

        for queue in &queues {
            let id = sanitize_id(queue.id.as_str());
            let label = self.build_queue_label(queue);
            // Use asymmetric shape for queues
            writeln!(output, "        {}>{}]", id, label).unwrap();
        }

        writeln!(output, "    end").unwrap();
    }

    fn write_cloud_resources_subgraph(&self, output: &mut String, graph: &ForgeGraph) {
        let resources: Vec<_> = graph.nodes_by_type(NodeType::CloudResource).collect();
        if resources.is_empty() {
            return;
        }

        writeln!(output, "    subgraph Resources").unwrap();

        for resource in &resources {
            let id = sanitize_id(resource.id.as_str());
            let label = &resource.display_name;
            // Use hexagon shape for cloud resources
            writeln!(output, "        {}{{{{{}}}}}", id, label).unwrap();
        }

        writeln!(output, "    end").unwrap();
    }

    fn write_apis_subgraph(&self, output: &mut String, graph: &ForgeGraph) {
        let apis: Vec<_> = graph.nodes_by_type(NodeType::Api).collect();
        if apis.is_empty() {
            return;
        }

        writeln!(output, "    subgraph APIs").unwrap();

        for api in &apis {
            let id = sanitize_id(api.id.as_str());
            let label = &api.display_name;
            // Use stadium shape for APIs
            writeln!(output, "        {}([{}])", id, label).unwrap();
        }

        writeln!(output, "    end").unwrap();
    }

    fn write_node_subgraph(&self, output: &mut String, name: &str, nodes: &[&Node]) {
        writeln!(output, "    subgraph {}", name).unwrap();

        for node in nodes {
            let id = sanitize_id(node.id.as_str());
            let label = self.build_node_label(node);
            let shape = self.get_node_shape(node, &label);
            writeln!(output, "        {}{}", id, shape).unwrap();
        }

        writeln!(output, "    end").unwrap();
    }

    fn write_edges(&self, output: &mut String, graph: &ForgeGraph) {
        for edge in graph.edges() {
            let source_id = sanitize_id(edge.source.as_str());
            let target_id = sanitize_id(edge.target.as_str());
            let label = edge_type_label(edge.edge_type);

            let line_style = if edge.edge_type == EdgeType::ImplicitlyCoupled {
                "-.->"
            } else {
                "-->"
            };

            writeln!(
                output,
                "    {} {}|{}| {}",
                source_id, line_style, label, target_id
            )
            .unwrap();
        }
    }

    fn write_style_classes(&self, output: &mut String, graph: &ForgeGraph) {
        // Collect all node IDs by type
        let service_ids: Vec<_> = graph
            .nodes_by_type(NodeType::Service)
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();
        let database_ids: Vec<_> = graph
            .nodes_by_type(NodeType::Database)
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();
        let queue_ids: Vec<_> = graph
            .nodes_by_type(NodeType::Queue)
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();
        let resource_ids: Vec<_> = graph
            .nodes_by_type(NodeType::CloudResource)
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();

        self.write_style_definitions(
            output,
            &service_ids,
            &database_ids,
            &queue_ids,
            &resource_ids,
        );
    }

    fn write_style_classes_for_nodes(
        &self,
        output: &mut String,
        services: &[&Node],
        databases: &[&Node],
        queues: &[&Node],
        resources: &[&Node],
    ) {
        let service_ids: Vec<_> = services
            .iter()
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();
        let database_ids: Vec<_> = databases
            .iter()
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();
        let queue_ids: Vec<_> = queues.iter().map(|n| sanitize_id(n.id.as_str())).collect();
        let resource_ids: Vec<_> = resources
            .iter()
            .map(|n| sanitize_id(n.id.as_str()))
            .collect();

        self.write_style_definitions(
            output,
            &service_ids,
            &database_ids,
            &queue_ids,
            &resource_ids,
        );
    }

    fn write_style_definitions(
        &self,
        output: &mut String,
        service_ids: &[String],
        database_ids: &[String],
        queue_ids: &[String],
        resource_ids: &[String],
    ) {
        // Define style classes
        writeln!(
            output,
            "    classDef service fill:#4a86e8,stroke:#333,stroke-width:2px,color:white"
        )
        .unwrap();
        writeln!(
            output,
            "    classDef database fill:#f1c232,stroke:#333,stroke-width:2px"
        )
        .unwrap();
        writeln!(
            output,
            "    classDef queue fill:#6aa84f,stroke:#333,stroke-width:2px,color:white"
        )
        .unwrap();
        writeln!(
            output,
            "    classDef resource fill:#9fc5e8,stroke:#333,stroke-width:2px"
        )
        .unwrap();

        // Apply classes to nodes
        if !service_ids.is_empty() {
            writeln!(output, "    class {} service", service_ids.join(",")).unwrap();
        }
        if !database_ids.is_empty() {
            writeln!(output, "    class {} database", database_ids.join(",")).unwrap();
        }
        if !queue_ids.is_empty() {
            writeln!(output, "    class {} queue", queue_ids.join(",")).unwrap();
        }
        if !resource_ids.is_empty() {
            writeln!(output, "    class {} resource", resource_ids.join(",")).unwrap();
        }
    }

    fn build_service_label(&self, node: &Node) -> String {
        if !self.include_attributes {
            let mut label = escape_label(&node.display_name);
            if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
                label.push_str(" ⚠️");
            }
            return label;
        }

        let mut label = escape_label(&node.display_name);

        // Add staleness indicator
        if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
            label.push_str(" ⚠️");
        }

        let lang = node
            .attributes
            .get("language")
            .and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("");

        let framework = node.attributes.get("framework").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        });

        if !lang.is_empty() {
            label.push_str("<br/>");
            if let Some(fw) = framework {
                label.push_str(&format!("{}/{}", lang, fw));
            } else {
                label.push_str(lang);
            }
        }

        label
    }

    fn build_database_label(&self, node: &Node) -> String {
        if !self.include_attributes {
            let mut label = escape_label(&node.display_name);
            if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
                label.push_str(" ⚠️");
            }
            return label;
        }

        let mut label = escape_label(&node.display_name);

        // Add staleness indicator
        if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
            label.push_str(" ⚠️");
        }

        if let Some(db_type) = node.attributes.get("db_type").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            label.push_str(&format!("<br/>{}", db_type));
        }

        label
    }

    fn build_queue_label(&self, node: &Node) -> String {
        if !self.include_attributes {
            let mut label = escape_label(&node.display_name);
            if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
                label.push_str(" ⚠️");
            }
            return label;
        }

        let mut label = escape_label(&node.display_name);

        // Add staleness indicator
        if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
            label.push_str(" ⚠️");
        }

        if let Some(q_type) = node.attributes.get("queue_type").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            label.push_str(&format!("<br/>{}", q_type));
        }

        label
    }

    fn build_node_label(&self, node: &Node) -> String {
        match node.node_type {
            NodeType::Service => self.build_service_label(node),
            NodeType::Database => self.build_database_label(node),
            NodeType::Queue => self.build_queue_label(node),
            _ => {
                let mut label = escape_label(&node.display_name);
                if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
                    label.push_str(" ⚠️");
                }
                label
            }
        }
    }

    fn get_node_shape(&self, node: &Node, label: &str) -> String {
        match node.node_type {
            NodeType::Service => format!("[{}]", label),
            NodeType::Database => format!("[({})]", label),
            NodeType::Queue => format!("[>{}]", label),
            NodeType::CloudResource => format!("{{{{{}}}}}", label),
            NodeType::Api => format!("([{}])", label),
        }
    }
}

/// Sanitize a node ID for use in Mermaid.
///
/// Mermaid node IDs cannot contain colons, dashes, or slashes.
fn sanitize_id(id: &str) -> String {
    id.replace([':', '-', '/'], "_")
}

/// Escape a label for use in Mermaid.
///
/// Handles special characters that could break Mermaid syntax.
fn escape_label(label: &str) -> String {
    label
        .replace('"', "'")
        .replace('[', "(")
        .replace(']', ")")
        .replace('{', "(")
        .replace('}', ")")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Get a human-readable label for an edge type.
fn edge_type_label(edge_type: EdgeType) -> &'static str {
    match edge_type {
        EdgeType::Calls => "CALLS",
        EdgeType::Owns => "OWNS",
        EdgeType::Reads => "READS",
        EdgeType::Writes => "WRITES",
        EdgeType::Publishes => "PUBLISHES",
        EdgeType::Subscribes => "SUBSCRIBES",
        EdgeType::Uses => "USES",
        EdgeType::ReadsShared => "READS_SHARED",
        EdgeType::WritesShared => "WRITES_SHARED",
        EdgeType::ImplicitlyCoupled => "COUPLED",
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
        let serializer = MermaidSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should start with flowchart declaration
        assert!(output.starts_with("flowchart LR"));

        // Should contain subgraphs
        assert!(output.contains("subgraph Services"));
        assert!(output.contains("subgraph Databases"));
        assert!(output.contains("subgraph Queues"));

        // Should contain end tags
        assert!(output.contains("    end"));
    }

    #[test]
    fn test_serialize_graph_nodes() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should contain service nodes
        assert!(output.contains("service_ns_user_api"));
        assert!(output.contains("service_ns_order_api"));

        // Should contain database node with cylinder shape
        assert!(output.contains("database_ns_users_table"));

        // Should contain queue node
        assert!(output.contains("queue_ns_order_events"));
    }

    #[test]
    fn test_serialize_graph_edges() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should contain edge relationships
        assert!(output.contains("-->|READS|"));
        assert!(output.contains("-->|WRITES|"));
        assert!(output.contains("-->|CALLS|"));
        assert!(output.contains("-->|PUBLISHES|"));
    }

    #[test]
    fn test_serialize_graph_styles() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new().with_styles(true);

        let output = serializer.serialize_graph(&graph);

        // Should contain style definitions
        assert!(output.contains("classDef service"));
        assert!(output.contains("classDef database"));
        assert!(output.contains("classDef queue"));

        // Should apply classes to nodes
        assert!(output.contains("class ") && output.contains(" service"));
        assert!(output.contains("class ") && output.contains(" database"));
        assert!(output.contains("class ") && output.contains(" queue"));
    }

    #[test]
    fn test_serialize_graph_no_styles() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new().with_styles(false);

        let output = serializer.serialize_graph(&graph);

        // Should not contain style definitions
        assert!(!output.contains("classDef"));
        assert!(!output.contains("class ") || !output.contains(" service"));
    }

    #[test]
    fn test_serialize_graph_with_attributes() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new().with_attributes(true);

        let output = serializer.serialize_graph(&graph);

        // Should contain language info
        assert!(output.contains("typescript"));

        // Should contain db_type
        assert!(output.contains("dynamodb"));

        // Should contain queue_type
        assert!(output.contains("sqs"));
    }

    #[test]
    fn test_serialize_graph_without_attributes() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new().with_attributes(false);

        let output = serializer.serialize_graph(&graph);

        // Should not contain detailed attributes
        assert!(!output.contains("<br/>typescript"));
        assert!(!output.contains("<br/>dynamodb"));
    }

    #[test]
    fn test_serialize_empty_graph() {
        let graph = ForgeGraph::new();
        let serializer = MermaidSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should still have flowchart declaration
        assert!(output.starts_with("flowchart LR"));

        // Should not contain subgraphs (no nodes)
        assert!(!output.contains("subgraph Services"));
        assert!(!output.contains("subgraph Databases"));
    }

    #[test]
    fn test_serialize_subgraph() {
        let graph = create_test_graph();
        let serializer = MermaidSerializer::new();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let output = serializer.serialize_subgraph(&subgraph);

        // Should have flowchart declaration
        assert!(output.starts_with("flowchart LR"));

        // Should have nodes in subgraph
        assert!(output.contains("service_ns_user_api"));
    }

    #[test]
    fn test_implicit_coupling_edge_style() {
        let mut graph = ForgeGraph::new();

        // Add two services
        graph
            .add_node(create_test_service("ns", "svc-a", "Service A"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "svc-b", "Service B"))
            .unwrap();

        // Add implicit coupling
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

        let serializer = MermaidSerializer::new();
        let output = serializer.serialize_graph(&graph);

        // Should use dotted line for implicit coupling
        assert!(output.contains("-.->|COUPLED|"));
    }

    #[test]
    fn test_direction() {
        let graph = create_test_graph();

        let serializer_lr = MermaidSerializer::new().with_direction(Direction::LR);
        let output_lr = serializer_lr.serialize_graph(&graph);
        assert!(output_lr.starts_with("flowchart LR"));

        let serializer_tb = MermaidSerializer::new().with_direction(Direction::TB);
        let output_tb = serializer_tb.serialize_graph(&graph);
        assert!(output_tb.starts_with("flowchart TB"));

        let serializer_rl = MermaidSerializer::new().with_direction(Direction::RL);
        let output_rl = serializer_rl.serialize_graph(&graph);
        assert!(output_rl.starts_with("flowchart RL"));

        let serializer_bt = MermaidSerializer::new().with_direction(Direction::BT);
        let output_bt = serializer_bt.serialize_graph(&graph);
        assert!(output_bt.starts_with("flowchart BT"));
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("service:ns:user-api"), "service_ns_user_api");
        assert_eq!(sanitize_id("database:ns:users"), "database_ns_users");
        assert_eq!(sanitize_id("queue:ns:events"), "queue_ns_events");
    }

    #[test]
    fn test_escape_label() {
        assert_eq!(escape_label("Hello World"), "Hello World");
        assert_eq!(escape_label("user-api"), "user-api");
        assert_eq!(escape_label("test[1]"), "test(1)");
        assert_eq!(escape_label("test{a}"), "test(a)");
        assert_eq!(escape_label("test\"quote\""), "test'quote'");
    }

    #[test]
    fn test_edge_type_label() {
        assert_eq!(edge_type_label(EdgeType::Calls), "CALLS");
        assert_eq!(edge_type_label(EdgeType::Reads), "READS");
        assert_eq!(edge_type_label(EdgeType::Writes), "WRITES");
        assert_eq!(edge_type_label(EdgeType::Publishes), "PUBLISHES");
        assert_eq!(edge_type_label(EdgeType::Subscribes), "SUBSCRIBES");
        assert_eq!(edge_type_label(EdgeType::Uses), "USES");
        assert_eq!(edge_type_label(EdgeType::ReadsShared), "READS_SHARED");
        assert_eq!(edge_type_label(EdgeType::WritesShared), "WRITES_SHARED");
        assert_eq!(edge_type_label(EdgeType::ImplicitlyCoupled), "COUPLED");
    }

    #[test]
    fn test_builder_pattern() {
        let serializer = MermaidSerializer::new()
            .with_direction(Direction::TB)
            .with_attributes(false)
            .with_max_nodes(100)
            .with_styles(false);

        assert_eq!(serializer.direction, Direction::TB);
        assert!(!serializer.include_attributes);
        assert_eq!(serializer.max_nodes, 100);
        assert!(!serializer.include_styles);
    }

    #[test]
    fn test_cloud_resource_node() {
        let mut graph = ForgeGraph::new();

        let resource = NodeBuilder::new()
            .id(NodeId::new(NodeType::CloudResource, "ns", "my-bucket").unwrap())
            .node_type(NodeType::CloudResource)
            .display_name("My Bucket")
            .attribute("resource_type", "S3")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(resource).unwrap();

        let serializer = MermaidSerializer::new();
        let output = serializer.serialize_graph(&graph);

        // Should have Resources subgraph
        assert!(output.contains("subgraph Resources"));

        // Should use hexagon shape (double braces)
        // Note: cloud_resource type becomes cloud_resource_ns_my_bucket after sanitization
        assert!(output.contains("cloud_resource_ns_my_bucket{{My Bucket}}"));
    }

    #[test]
    fn test_api_node() {
        let mut graph = ForgeGraph::new();

        let api = NodeBuilder::new()
            .id(NodeId::new(NodeType::Api, "ns", "get-users").unwrap())
            .node_type(NodeType::Api)
            .display_name("GET /users")
            .attribute("method", "GET")
            .attribute("path", "/users")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(api).unwrap();

        let serializer = MermaidSerializer::new();
        let output = serializer.serialize_graph(&graph);

        // Should have APIs subgraph
        assert!(output.contains("subgraph APIs"));

        // Should use stadium shape (rounded rectangle)
        assert!(output.contains("api_ns_get_users(["));
    }

    #[test]
    fn test_service_with_framework() {
        let mut graph = ForgeGraph::new();

        let service = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "web-app").unwrap())
            .node_type(NodeType::Service)
            .display_name("Web App")
            .attribute("language", "typescript")
            .attribute("framework", "express")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(service).unwrap();

        let serializer = MermaidSerializer::new().with_attributes(true);
        let output = serializer.serialize_graph(&graph);

        // Should contain language/framework combo
        assert!(output.contains("typescript/express"));
    }
}
