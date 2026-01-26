//! The `forge map` command.
//!
//! Serializes the knowledge graph to various output formats:
//! - Markdown: Human-readable documentation optimized for LLM context
//! - JSON: Structured format for programmatic access
//! - Mermaid: Visual diagram syntax for documentation

use crate::config::ForgeConfig;
use crate::output;
use crate::serializers::{JsonSerializer, MarkdownSerializer, MermaidSerializer, QueryInfo};
use forge_graph::{ForgeGraph, NodeId, NodeType, SubgraphConfig};
use std::path::PathBuf;
use thiserror::Error;

/// Options for the map command.
#[derive(Debug)]
pub struct MapOptions {
    /// Path to the configuration file
    pub config: Option<String>,
    /// Override input graph path
    pub input: Option<String>,
    /// Output format
    pub format: String,
    /// Filter to specific services
    pub service: Option<String>,
    /// Token budget limit
    pub budget: Option<u32>,
    /// Output file (None = stdout)
    pub output: Option<String>,
}

/// Errors that can occur during the map command.
#[derive(Debug, Error)]
pub enum MapError {
    #[error("Failed to load configuration: {0}")]
    ConfigError(String),

    #[error("Failed to load graph: {0}")]
    GraphLoadError(String),

    #[error("Unknown format: {0}. Valid formats: markdown, json, mermaid")]
    UnknownFormat(String),

    #[error("Failed to write output: {0}")]
    WriteError(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),
}

/// Output format for the map command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Markdown,
    Json,
    Mermaid,
}

impl OutputFormat {
    /// Parse a format string.
    pub fn from_str(s: &str) -> Result<Self, MapError> {
        match s.to_lowercase().as_str() {
            "markdown" | "md" => Ok(OutputFormat::Markdown),
            "json" => Ok(OutputFormat::Json),
            "mermaid" | "mmd" => Ok(OutputFormat::Mermaid),
            _ => Err(MapError::UnknownFormat(s.to_string())),
        }
    }
}

/// Run the map command.
pub fn run_map(options: MapOptions) -> Result<(), MapError> {
    // Load config for graph path and staleness_days
    let config = if let Some(config_path) = &options.config {
        Some(
            ForgeConfig::load_from_path(std::path::Path::new(config_path))
                .map_err(|e| MapError::ConfigError(e.to_string()))?,
        )
    } else {
        // Try to load default config
        ForgeConfig::load_default().ok()
    };

    // Determine graph path - use input override, config, or default
    let graph_path = if let Some(input) = &options.input {
        PathBuf::from(input)
    } else if let Some(cfg) = &config {
        cfg.output.graph_path.clone()
    } else {
        PathBuf::from(".forge/graph.json")
    };

    // Get staleness_days from config or use default
    let staleness_days = config.as_ref().map(|c| c.staleness_days).unwrap_or(7);

    // Load the graph
    let graph = ForgeGraph::load_from_file(&graph_path)
        .map_err(|e| MapError::GraphLoadError(format!("{}: {}", graph_path.display(), e)))?;

    // Parse format
    let format = OutputFormat::from_str(&options.format)?;

    // Generate output
    let output = if let Some(services) = &options.service {
        // Extract subgraph for specified services
        let seed_ids = parse_service_filter(services, &graph)?;
        serialize_subgraph(&graph, &seed_ids, format, options.budget, staleness_days)?
    } else {
        // Serialize entire graph
        serialize_graph(&graph, format, options.budget, staleness_days)?
    };

    // Write output
    if let Some(output_path) = &options.output {
        std::fs::write(output_path, &output)
            .map_err(|e| MapError::WriteError(format!("{}: {}", output_path, e)))?;
        output::success(&format!("Output written to: {}", output_path));
    } else {
        // Write to stdout
        output::info(&output);
    }

    Ok(())
}

/// Parse a comma-separated service filter into node IDs.
fn parse_service_filter(filter: &str, graph: &ForgeGraph) -> Result<Vec<NodeId>, MapError> {
    let mut seed_ids = Vec::new();

    for name in filter.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        // Try to find a service with this name
        let found = graph
            .nodes_by_type(NodeType::Service)
            .find(|n| n.display_name.eq_ignore_ascii_case(name) || n.id.name() == name);

        if let Some(node) = found {
            seed_ids.push(node.id.clone());
        } else {
            return Err(MapError::ServiceNotFound(name.to_string()));
        }
    }

    Ok(seed_ids)
}

/// Serialize an entire graph.
fn serialize_graph(
    graph: &ForgeGraph,
    format: OutputFormat,
    _budget: Option<u32>,
    staleness_days: u32,
) -> Result<String, MapError> {
    match format {
        OutputFormat::Markdown => {
            let serializer = MarkdownSerializer::new().with_staleness_days(staleness_days);
            Ok(serializer.serialize_graph(graph))
        }
        OutputFormat::Json => {
            let serializer = JsonSerializer::new().with_staleness_days(staleness_days);
            Ok(serializer.serialize_graph(graph))
        }
        OutputFormat::Mermaid => {
            let serializer = MermaidSerializer::new().with_staleness_days(staleness_days);
            Ok(serializer.serialize_graph(graph))
        }
    }
}

/// Serialize a subgraph filtered to specific services.
fn serialize_subgraph(
    graph: &ForgeGraph,
    seed_ids: &[NodeId],
    format: OutputFormat,
    _budget: Option<u32>,
    staleness_days: u32,
) -> Result<String, MapError> {
    let config = SubgraphConfig {
        seed_nodes: seed_ids.to_vec(),
        max_depth: 2,
        include_implicit_couplings: true,
        min_relevance: 0.1,
        edge_types: None,
    };

    let subgraph = graph.extract_subgraph(&config);

    match format {
        OutputFormat::Markdown => {
            let serializer = MarkdownSerializer::new().with_staleness_days(staleness_days);
            Ok(serializer.serialize_subgraph(&subgraph))
        }
        OutputFormat::Json => {
            let serializer = JsonSerializer::new().with_staleness_days(staleness_days);
            let query_info = QueryInfo {
                query_type: "service_filter".to_string(),
                seeds: Some(seed_ids.iter().map(|id| id.as_str().to_string()).collect()),
                max_depth: Some(2),
            };
            Ok(serializer.serialize_subgraph(&subgraph, Some(query_info)))
        }
        OutputFormat::Mermaid => {
            let serializer = MermaidSerializer::new().with_staleness_days(staleness_days);
            Ok(serializer.serialize_subgraph(&subgraph))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_graph::{DiscoverySource, Edge, EdgeType, NodeBuilder};
    use tempfile::tempdir;

    fn create_test_graph() -> ForgeGraph {
        let mut graph = ForgeGraph::new();

        // Add services
        let user_api = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "user-api").unwrap())
            .node_type(NodeType::Service)
            .display_name("User API")
            .attribute("language", "typescript")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        let order_api = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "order-api").unwrap())
            .node_type(NodeType::Service)
            .display_name("Order API")
            .attribute("language", "python")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        // Add database
        let users_db = NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, "ns", "users-table").unwrap())
            .node_type(NodeType::Database)
            .display_name("Users Table")
            .attribute("db_type", "dynamodb")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(user_api).unwrap();
        graph.add_node(order_api).unwrap();
        graph.add_node(users_db).unwrap();

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
                    NodeId::new(NodeType::Service, "ns", "order-api").unwrap(),
                    NodeId::new(NodeType::Service, "ns", "user-api").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        graph
    }

    #[test]
    fn test_output_format_parsing() {
        assert_eq!(
            OutputFormat::from_str("markdown").unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!(
            OutputFormat::from_str("md").unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!(OutputFormat::from_str("json").unwrap(), OutputFormat::Json);
        assert_eq!(
            OutputFormat::from_str("mermaid").unwrap(),
            OutputFormat::Mermaid
        );
        assert_eq!(
            OutputFormat::from_str("mmd").unwrap(),
            OutputFormat::Mermaid
        );

        assert!(OutputFormat::from_str("unknown").is_err());
    }

    #[test]
    fn test_parse_service_filter() {
        let graph = create_test_graph();

        // Single service
        let ids = parse_service_filter("User API", &graph).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].name(), "user-api");

        // Multiple services
        let ids = parse_service_filter("User API, Order API", &graph).unwrap();
        assert_eq!(ids.len(), 2);

        // Case insensitive
        let ids = parse_service_filter("user api", &graph).unwrap();
        assert_eq!(ids.len(), 1);

        // By node ID name
        let ids = parse_service_filter("user-api", &graph).unwrap();
        assert_eq!(ids.len(), 1);

        // Non-existent service
        assert!(parse_service_filter("Non-existent", &graph).is_err());
    }

    #[test]
    fn test_serialize_graph_markdown() {
        let graph = create_test_graph();

        let output = serialize_graph(&graph, OutputFormat::Markdown, None, 7).unwrap();

        assert!(output.contains("# Ecosystem Knowledge Graph"));
        assert!(output.contains("User API"));
        assert!(output.contains("Order API"));
        assert!(output.contains("Users Table"));
    }

    #[test]
    fn test_serialize_subgraph_markdown() {
        let graph = create_test_graph();
        let seed_ids = vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()];

        let output =
            serialize_subgraph(&graph, &seed_ids, OutputFormat::Markdown, None, 7).unwrap();

        assert!(output.contains("# Relevant Context"));
        assert!(output.contains("User API"));
    }

    #[test]
    fn test_run_map_with_file_output() {
        let graph = create_test_graph();
        let temp_dir = tempdir().unwrap();

        // Save graph
        let graph_path = temp_dir.path().join("graph.json");
        graph.save_to_file(&graph_path).unwrap();

        // Output path
        let output_path = temp_dir.path().join("output.md");

        let options = MapOptions {
            config: None,
            input: Some(graph_path.to_string_lossy().to_string()),
            format: "markdown".to_string(),
            service: None,
            budget: None,
            output: Some(output_path.to_string_lossy().to_string()),
        };

        run_map(options).unwrap();

        // Verify output file exists
        assert!(output_path.exists());

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("# Ecosystem Knowledge Graph"));
    }

    #[test]
    fn test_run_map_with_service_filter() {
        let graph = create_test_graph();
        let temp_dir = tempdir().unwrap();

        // Save graph
        let graph_path = temp_dir.path().join("graph.json");
        graph.save_to_file(&graph_path).unwrap();

        // Output path
        let output_path = temp_dir.path().join("output.md");

        let options = MapOptions {
            config: None,
            input: Some(graph_path.to_string_lossy().to_string()),
            format: "markdown".to_string(),
            service: Some("User API".to_string()),
            budget: None,
            output: Some(output_path.to_string_lossy().to_string()),
        };

        run_map(options).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("# Relevant Context"));
        assert!(content.contains("User API"));
    }

    #[test]
    fn test_serialize_graph_json() {
        let graph = create_test_graph();

        let output = serialize_graph(&graph, OutputFormat::Json, None, 7).unwrap();

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // Should have required fields
        assert!(parsed.get("$schema").is_some());
        assert!(parsed.get("version").is_some());
        assert!(parsed.get("nodes").is_some());
        assert!(parsed.get("edges").is_some());
        assert!(parsed.get("summary").is_some());

        // Check nodes
        let nodes = parsed.get("nodes").unwrap().as_array().unwrap();
        assert_eq!(nodes.len(), 3); // 2 services + 1 database
    }

    #[test]
    fn test_serialize_subgraph_json() {
        let graph = create_test_graph();
        let seed_ids = vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()];

        let output = serialize_subgraph(&graph, &seed_ids, OutputFormat::Json, None, 7).unwrap();

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // Should have query info
        let query = parsed.get("query").unwrap();
        assert_eq!(
            query.get("type").unwrap().as_str().unwrap(),
            "service_filter"
        );

        // Seeds should be present
        let seeds = query.get("seeds").unwrap().as_array().unwrap();
        assert!(!seeds.is_empty());
    }

    #[test]
    fn test_run_map_with_json_format() {
        let graph = create_test_graph();
        let temp_dir = tempdir().unwrap();

        // Save graph
        let graph_path = temp_dir.path().join("graph.json");
        graph.save_to_file(&graph_path).unwrap();

        // Output path
        let output_path = temp_dir.path().join("output.json");

        let options = MapOptions {
            config: None,
            input: Some(graph_path.to_string_lossy().to_string()),
            format: "json".to_string(),
            service: None,
            budget: None,
            output: Some(output_path.to_string_lossy().to_string()),
        };

        run_map(options).unwrap();

        // Verify output file exists and contains valid JSON
        assert!(output_path.exists());

        let content = std::fs::read_to_string(&output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed.get("nodes").is_some());
    }

    #[test]
    fn test_run_map_json_with_service_filter() {
        let graph = create_test_graph();
        let temp_dir = tempdir().unwrap();

        // Save graph
        let graph_path = temp_dir.path().join("graph.json");
        graph.save_to_file(&graph_path).unwrap();

        // Output path
        let output_path = temp_dir.path().join("output.json");

        let options = MapOptions {
            config: None,
            input: Some(graph_path.to_string_lossy().to_string()),
            format: "json".to_string(),
            service: Some("User API".to_string()),
            budget: None,
            output: Some(output_path.to_string_lossy().to_string()),
        };

        run_map(options).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should have query info with service_filter type
        let query = parsed.get("query").unwrap();
        assert_eq!(
            query.get("type").unwrap().as_str().unwrap(),
            "service_filter"
        );

        // Nodes should have relevance scores
        let nodes = parsed.get("nodes").unwrap().as_array().unwrap();
        assert!(!nodes.is_empty());
        let first_node = &nodes[0];
        assert!(first_node.get("relevance").is_some());
    }

    #[test]
    fn test_serialize_graph_mermaid() {
        let graph = create_test_graph();

        let output = serialize_graph(&graph, OutputFormat::Mermaid, None, 7).unwrap();

        // Should start with flowchart declaration
        assert!(output.starts_with("flowchart LR"));

        // Should contain subgraphs
        assert!(output.contains("subgraph Services"));
        assert!(output.contains("subgraph Databases"));

        // Should contain nodes
        assert!(output.contains("service_ns_user_api"));
        assert!(output.contains("service_ns_order_api"));
        assert!(output.contains("database_ns_users_table"));

        // Should contain edges
        assert!(output.contains("-->|READS|"));
        assert!(output.contains("-->|CALLS|"));
    }

    #[test]
    fn test_serialize_subgraph_mermaid() {
        let graph = create_test_graph();
        let seed_ids = vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()];

        let output = serialize_subgraph(&graph, &seed_ids, OutputFormat::Mermaid, None, 7).unwrap();

        // Should start with flowchart declaration
        assert!(output.starts_with("flowchart LR"));

        // Should contain nodes in subgraph
        assert!(output.contains("service_ns_user_api"));
    }

    #[test]
    fn test_run_map_with_mermaid_format() {
        let graph = create_test_graph();
        let temp_dir = tempdir().unwrap();

        // Save graph
        let graph_path = temp_dir.path().join("graph.json");
        graph.save_to_file(&graph_path).unwrap();

        // Output path
        let output_path = temp_dir.path().join("output.mmd");

        let options = MapOptions {
            config: None,
            input: Some(graph_path.to_string_lossy().to_string()),
            format: "mermaid".to_string(),
            service: None,
            budget: None,
            output: Some(output_path.to_string_lossy().to_string()),
        };

        run_map(options).unwrap();

        // Verify output file exists and contains Mermaid syntax
        assert!(output_path.exists());

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.starts_with("flowchart LR"));
        assert!(content.contains("subgraph Services"));
    }

    #[test]
    fn test_run_map_mermaid_with_service_filter() {
        let graph = create_test_graph();
        let temp_dir = tempdir().unwrap();

        // Save graph
        let graph_path = temp_dir.path().join("graph.json");
        graph.save_to_file(&graph_path).unwrap();

        // Output path
        let output_path = temp_dir.path().join("output.mmd");

        let options = MapOptions {
            config: None,
            input: Some(graph_path.to_string_lossy().to_string()),
            format: "mmd".to_string(), // Test mmd alias
            service: Some("User API".to_string()),
            budget: None,
            output: Some(output_path.to_string_lossy().to_string()),
        };

        run_map(options).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.starts_with("flowchart LR"));
        assert!(content.contains("service_ns_user_api"));
    }
}
