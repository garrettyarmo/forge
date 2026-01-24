//! Token counting and budget management for LLM output serialization.
//!
//! This module provides token counting using tiktoken-rs (cl100k_base encoding)
//! for accurate estimation of output size when serializing graphs for LLM consumption.
//!
//! ## Design
//!
//! The token counter uses OpenAI's cl100k_base tokenizer (same as GPT-4 and Claude).
//! While Claude's exact tokenizer differs slightly, cl100k_base provides a close
//! approximation that is accurate within ±5%.
//!
//! ## Example
//!
//! ```ignore
//! use forge_cli::token_budget::{TokenCounter, DetailLevel};
//!
//! let counter = TokenCounter::new().unwrap();
//!
//! // Count tokens in text
//! let tokens = counter.count("Hello, world!");
//!
//! // Estimate tokens for a node
//! let node_tokens = counter.estimate_node_tokens(&node, DetailLevel::Full);
//! ```

use crate::serializers::DetailLevel;
use forge_graph::{Edge, ExtractedSubgraph, Node, ScoredNode};
use std::collections::HashSet;
use std::fmt::Write;
use thiserror::Error;
use tiktoken_rs::CoreBPE;

/// Errors related to token counting operations.
#[derive(Debug, Error)]
pub enum TokenBudgetError {
    #[error("Failed to initialize tokenizer: {0}")]
    TokenizerInit(String),

    #[error("Budget exceeded: required {required} tokens but only {available} available")]
    BudgetExceeded { required: usize, available: usize },
}

/// Token counter using tiktoken-rs cl100k_base encoding.
///
/// This provides accurate token counting compatible with GPT-4 and
/// approximately accurate for Claude models.
pub struct TokenCounter {
    bpe: CoreBPE,
}

impl TokenCounter {
    /// Create a new TokenCounter with cl100k_base encoding.
    ///
    /// # Errors
    ///
    /// Returns an error if the tokenizer fails to initialize.
    pub fn new() -> Result<Self, TokenBudgetError> {
        let bpe = tiktoken_rs::cl100k_base()
            .map_err(|e| TokenBudgetError::TokenizerInit(e.to_string()))?;
        Ok(Self { bpe })
    }

    /// Count the number of tokens in a string.
    ///
    /// Uses cl100k_base encoding which is accurate within ±5% of Claude's tokenizer.
    pub fn count(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    /// Estimate the number of tokens for serializing a node.
    ///
    /// The estimate varies based on detail level:
    /// - Full: 100 base + 2× name tokens (includes all attributes, evidence, context)
    /// - Summary: 50 base + name tokens (key attributes only)
    /// - Minimal: 10 base + name tokens (just name and type)
    pub fn estimate_node_tokens(&self, node: &Node, detail_level: DetailLevel) -> usize {
        let name_tokens = self.count(&node.display_name);

        match detail_level {
            DetailLevel::Full => {
                // Full detail includes: name, type, all attributes, business context,
                // dependency tables, evidence, markdown formatting
                let base = 100;
                let attribute_estimate = node.attributes.len() * 15; // ~15 tokens per attr
                let context_estimate = if node.business_context.is_some() {
                    50
                } else {
                    0
                };
                base + (name_tokens * 2) + attribute_estimate + context_estimate
            }
            DetailLevel::Summary => {
                // Summary includes: name, type, key attributes (language, framework)
                50 + name_tokens
            }
            DetailLevel::Minimal => {
                // Minimal: just name and basic type info
                10 + name_tokens
            }
        }
    }

    /// Estimate the number of tokens for serializing an edge.
    ///
    /// Returns approximately 30 tokens per edge, accounting for:
    /// - Source and target names (~10 tokens)
    /// - Edge type (~5 tokens)
    /// - Evidence entries (~10 tokens)
    /// - Markdown formatting (~5 tokens)
    pub fn estimate_edge_tokens(&self) -> usize {
        30
    }

    /// Estimate tokens for a complete subgraph serialization.
    ///
    /// This provides a rough estimate based on the number of nodes and edges.
    pub fn estimate_subgraph_tokens(&self, subgraph: &ExtractedSubgraph<'_>) -> usize {
        let mut total = 100; // Header/structure overhead

        for scored in &subgraph.nodes {
            let detail = detail_level_for_relevance(scored.score);
            total += self.estimate_node_tokens(scored.node, detail);
        }

        total += subgraph.edges.len() * self.estimate_edge_tokens();

        total
    }
}

/// Output format for budget-constrained serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Markdown format optimized for LLM context
    Markdown,
    /// Structured JSON format
    Json,
    /// Mermaid diagram syntax
    Mermaid,
}

/// Budget-constrained serializer that respects token limits.
///
/// Prioritizes nodes by relevance score, including as many high-relevance
/// nodes as fit within the budget, with detail level adjusted based on
/// available space.
pub struct BudgetedSerializer {
    counter: TokenCounter,
    budget: usize,
}

impl BudgetedSerializer {
    /// Create a new BudgetedSerializer with the specified token budget.
    ///
    /// # Arguments
    ///
    /// * `budget` - Maximum number of tokens for the output
    ///
    /// # Errors
    ///
    /// Returns an error if the token counter fails to initialize.
    pub fn new(budget: usize) -> Result<Self, TokenBudgetError> {
        Ok(Self {
            counter: TokenCounter::new()?,
            budget,
        })
    }

    /// Get the configured token budget.
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Get a reference to the token counter.
    pub fn counter(&self) -> &TokenCounter {
        &self.counter
    }

    /// Serialize a subgraph within the token budget.
    ///
    /// Nodes are included in order of relevance score (highest first).
    /// Detail level is adjusted based on relevance:
    /// - Score > 0.7: Full detail
    /// - Score 0.4-0.7: Summary detail
    /// - Score < 0.4: Minimal detail
    ///
    /// Edges are included if both their source and target nodes are included.
    pub fn serialize_within_budget(
        &self,
        subgraph: &ExtractedSubgraph<'_>,
        format: OutputFormat,
    ) -> String {
        // Start with highest relevance nodes and add until budget reached
        let mut included_nodes: Vec<&ScoredNode<'_>> = vec![];
        let mut estimated_tokens: usize = 0;
        let header_tokens: usize = 100; // Reserve for headers/structure

        for scored_node in &subgraph.nodes {
            let node_tokens = self.counter.estimate_node_tokens(
                scored_node.node,
                detail_level_for_relevance(scored_node.score),
            );

            if estimated_tokens + node_tokens + header_tokens > self.budget {
                break;
            }

            included_nodes.push(scored_node);
            estimated_tokens += node_tokens;
        }

        // Collect IDs of included nodes
        let included_ids: HashSet<_> = included_nodes.iter().map(|n| &n.node.id).collect();

        // Add edges for included nodes, respecting remaining budget
        let remaining_budget = self.budget.saturating_sub(estimated_tokens + header_tokens);
        let max_edges = remaining_budget / self.counter.estimate_edge_tokens();

        let included_edges: Vec<_> = subgraph
            .edges
            .iter()
            .filter(|e| included_ids.contains(&e.source) && included_ids.contains(&e.target))
            .take(max_edges)
            .collect();

        // Serialize based on format
        match format {
            OutputFormat::Markdown => {
                self.serialize_markdown_budgeted(&included_nodes, &included_edges, subgraph)
            }
            OutputFormat::Json => {
                // JSON has fixed structure, just serialize the included nodes
                self.serialize_json_budgeted(&included_nodes, &included_edges)
            }
            OutputFormat::Mermaid => {
                // Mermaid diagrams: serialize only included nodes
                self.serialize_mermaid_budgeted(&included_nodes, &included_edges)
            }
        }
    }

    /// Check if a subgraph fits within the budget.
    pub fn fits_within_budget(&self, subgraph: &ExtractedSubgraph<'_>) -> bool {
        self.counter.estimate_subgraph_tokens(subgraph) <= self.budget
    }

    /// Get the estimated token count for serializing a subgraph.
    pub fn estimate_tokens(&self, subgraph: &ExtractedSubgraph<'_>) -> usize {
        self.counter.estimate_subgraph_tokens(subgraph)
    }

    fn serialize_markdown_budgeted(
        &self,
        nodes: &[&ScoredNode<'_>],
        edges: &[&&Edge],
        subgraph: &ExtractedSubgraph<'_>,
    ) -> String {
        let mut output = String::new();

        writeln!(output, "# Context (Budget: {} tokens)\n", self.budget).unwrap();

        // Group nodes by type for organized output
        let mut services: Vec<&ScoredNode<'_>> = vec![];
        let mut databases: Vec<&ScoredNode<'_>> = vec![];
        let mut queues: Vec<&ScoredNode<'_>> = vec![];
        let mut others: Vec<&ScoredNode<'_>> = vec![];

        for node in nodes {
            match node.node.node_type {
                forge_graph::NodeType::Service => services.push(node),
                forge_graph::NodeType::Database => databases.push(node),
                forge_graph::NodeType::Queue => queues.push(node),
                _ => others.push(node),
            }
        }

        // Write sections
        if !services.is_empty() {
            writeln!(output, "## Services\n").unwrap();
            for scored in services {
                self.write_budgeted_node(&mut output, scored, edges, subgraph);
            }
        }

        if !databases.is_empty() {
            writeln!(output, "## Databases\n").unwrap();
            for scored in databases {
                self.write_budgeted_node(&mut output, scored, edges, subgraph);
            }
        }

        if !queues.is_empty() {
            writeln!(output, "## Queues\n").unwrap();
            for scored in queues {
                self.write_budgeted_node(&mut output, scored, edges, subgraph);
            }
        }

        if !others.is_empty() {
            writeln!(output, "## Other Resources\n").unwrap();
            for scored in others {
                self.write_budgeted_node(&mut output, scored, edges, subgraph);
            }
        }

        output
    }

    fn write_budgeted_node(
        &self,
        output: &mut String,
        scored: &ScoredNode<'_>,
        edges: &[&&Edge],
        subgraph: &ExtractedSubgraph<'_>,
    ) {
        let detail = detail_level_for_relevance(scored.score);
        let relevance_pct = (scored.score * 100.0) as u32;

        match detail {
            DetailLevel::Full => {
                writeln!(
                    output,
                    "### {} ({}% relevance)\n",
                    scored.node.display_name, relevance_pct
                )
                .unwrap();

                // Type info
                if let Some(lang) = scored
                    .node
                    .attributes
                    .get("language")
                    .and_then(|v| match v {
                        forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                        _ => None,
                    })
                {
                    writeln!(output, "**Language**: {}", lang).unwrap();
                }

                // Business context
                if let Some(ctx) = &scored.node.business_context {
                    if let Some(purpose) = &ctx.purpose {
                        writeln!(output, "**Purpose**: {}", purpose).unwrap();
                    }
                }

                // Related edges
                let node_edges: Vec<_> = edges
                    .iter()
                    .filter(|e| e.source == scored.node.id || e.target == scored.node.id)
                    .collect();

                if !node_edges.is_empty() {
                    writeln!(output, "\n**Relationships**:").unwrap();
                    for edge in node_edges {
                        let other_id = if edge.source == scored.node.id {
                            &edge.target
                        } else {
                            &edge.source
                        };
                        let direction = if edge.source == scored.node.id {
                            "→"
                        } else {
                            "←"
                        };

                        if let Some(other) = subgraph.graph().get_node(other_id) {
                            writeln!(
                                output,
                                "- {} {:?} {}",
                                direction, edge.edge_type, other.display_name
                            )
                            .unwrap();
                        }
                    }
                }

                writeln!(output).unwrap();
            }
            DetailLevel::Summary => {
                writeln!(
                    output,
                    "### {} ({}% relevance) [summary]\n",
                    scored.node.display_name, relevance_pct
                )
                .unwrap();

                // Just type info
                if let Some(lang) = scored
                    .node
                    .attributes
                    .get("language")
                    .and_then(|v| match v {
                        forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                        _ => None,
                    })
                {
                    writeln!(output, "**Language**: {}\n", lang).unwrap();
                }
            }
            DetailLevel::Minimal => {
                writeln!(
                    output,
                    "- {} ({}% relevance)",
                    scored.node.display_name, relevance_pct
                )
                .unwrap();
            }
        }
    }

    fn serialize_json_budgeted(&self, nodes: &[&ScoredNode<'_>], edges: &[&&Edge]) -> String {
        use serde_json::json;

        let nodes_json: Vec<_> = nodes
            .iter()
            .map(|scored| {
                json!({
                    "id": scored.node.id.as_str(),
                    "type": format!("{:?}", scored.node.node_type).to_lowercase(),
                    "name": scored.node.display_name,
                    "relevance": scored.score
                })
            })
            .collect();

        let edges_json: Vec<_> = edges
            .iter()
            .map(|edge| {
                json!({
                    "source": edge.source.as_str(),
                    "target": edge.target.as_str(),
                    "type": format!("{:?}", edge.edge_type)
                })
            })
            .collect();

        let output = json!({
            "budget": self.budget,
            "nodes": nodes_json,
            "edges": edges_json,
            "summary": {
                "included_nodes": nodes.len(),
                "included_edges": edges.len()
            }
        });

        serde_json::to_string_pretty(&output).unwrap_or_default()
    }

    fn serialize_mermaid_budgeted(&self, nodes: &[&ScoredNode<'_>], edges: &[&&Edge]) -> String {
        let mut output = String::new();

        writeln!(output, "flowchart LR").unwrap();
        writeln!(output, "    %% Budget: {} tokens", self.budget).unwrap();

        // Write nodes
        for scored in nodes {
            let id = sanitize_mermaid_id(scored.node.id.as_str());
            let label = &scored.node.display_name;
            let shape = match scored.node.node_type {
                forge_graph::NodeType::Service => format!("[{}]", label),
                forge_graph::NodeType::Database => format!("[({})]", label),
                forge_graph::NodeType::Queue => format!("[>{}]", label),
                _ => format!("{{{{{}}}}}", label),
            };
            writeln!(output, "    {}{}", id, shape).unwrap();
        }

        writeln!(output).unwrap();

        // Write edges
        for edge in edges {
            let source_id = sanitize_mermaid_id(edge.source.as_str());
            let target_id = sanitize_mermaid_id(edge.target.as_str());
            let label = format!("{:?}", edge.edge_type);
            writeln!(output, "    {} -->|{}| {}", source_id, label, target_id).unwrap();
        }

        output
    }
}

/// Determine detail level based on relevance score.
pub fn detail_level_for_relevance(score: f64) -> DetailLevel {
    if score > 0.7 {
        DetailLevel::Full
    } else if score > 0.4 {
        DetailLevel::Summary
    } else {
        DetailLevel::Minimal
    }
}

/// Sanitize a string for use as a Mermaid node ID.
fn sanitize_mermaid_id(id: &str) -> String {
    id.replace([':', '-', '/', '.'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_graph::{DiscoverySource, ForgeGraph, NodeBuilder, NodeId, NodeType, SubgraphConfig};

    fn create_test_service(namespace: &str, name: &str, display: &str) -> forge_graph::Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, namespace, name).unwrap())
            .node_type(NodeType::Service)
            .display_name(display)
            .attribute("language", "typescript")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    fn create_test_database(namespace: &str, name: &str, display: &str) -> forge_graph::Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, namespace, name).unwrap())
            .node_type(NodeType::Database)
            .display_name(display)
            .attribute("db_type", "dynamodb")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    #[test]
    fn test_token_counter_new() {
        let counter = TokenCounter::new();
        assert!(counter.is_ok());
    }

    #[test]
    fn test_token_counter_count() {
        let counter = TokenCounter::new().unwrap();

        // Simple text
        let count = counter.count("Hello, world!");
        assert!(count > 0);
        assert!(count < 10); // Should be around 4 tokens

        // Empty string
        assert_eq!(counter.count(""), 0);

        // Longer text
        let long_text = "This is a longer piece of text that should have more tokens.";
        assert!(counter.count(long_text) > counter.count("Hello"));
    }

    #[test]
    fn test_token_counter_consistency() {
        let counter = TokenCounter::new().unwrap();

        // Same input should give same output
        let text = "Test consistency";
        let count1 = counter.count(text);
        let count2 = counter.count(text);
        assert_eq!(count1, count2);
    }

    #[test]
    fn test_estimate_node_tokens_full() {
        let counter = TokenCounter::new().unwrap();
        let node = create_test_service("ns", "user-api", "User API Service");

        let tokens = counter.estimate_node_tokens(&node, DetailLevel::Full);

        // Full detail should have significant tokens
        assert!(tokens > 100);
    }

    #[test]
    fn test_estimate_node_tokens_summary() {
        let counter = TokenCounter::new().unwrap();
        let node = create_test_service("ns", "user-api", "User API Service");

        let tokens = counter.estimate_node_tokens(&node, DetailLevel::Summary);

        // Summary should be less than full
        let full_tokens = counter.estimate_node_tokens(&node, DetailLevel::Full);
        assert!(tokens < full_tokens);
        assert!(tokens >= 50); // At least base amount
    }

    #[test]
    fn test_estimate_node_tokens_minimal() {
        let counter = TokenCounter::new().unwrap();
        let node = create_test_service("ns", "user-api", "User API Service");

        let tokens = counter.estimate_node_tokens(&node, DetailLevel::Minimal);

        // Minimal should be least
        let summary_tokens = counter.estimate_node_tokens(&node, DetailLevel::Summary);
        assert!(tokens < summary_tokens);
        assert!(tokens >= 10); // At least base amount
    }

    #[test]
    fn test_estimate_node_tokens_with_business_context() {
        let counter = TokenCounter::new().unwrap();

        let mut node = create_test_service("ns", "user-api", "User API");
        node.business_context = Some(forge_graph::BusinessContext {
            purpose: Some("Handles user authentication".to_string()),
            owner: Some("Platform Team".to_string()),
            ..Default::default()
        });

        let with_context = counter.estimate_node_tokens(&node, DetailLevel::Full);

        let mut node_no_context = create_test_service("ns", "user-api", "User API");
        node_no_context.business_context = None;

        let without_context = counter.estimate_node_tokens(&node_no_context, DetailLevel::Full);

        // Node with context should estimate more tokens
        assert!(with_context > without_context);
    }

    #[test]
    fn test_estimate_edge_tokens() {
        let counter = TokenCounter::new().unwrap();
        let tokens = counter.estimate_edge_tokens();

        // Should be approximately 30
        assert_eq!(tokens, 30);
    }

    #[test]
    fn test_detail_level_for_relevance() {
        assert_eq!(detail_level_for_relevance(1.0), DetailLevel::Full);
        assert_eq!(detail_level_for_relevance(0.8), DetailLevel::Full);
        assert_eq!(detail_level_for_relevance(0.71), DetailLevel::Full);

        assert_eq!(detail_level_for_relevance(0.7), DetailLevel::Summary);
        assert_eq!(detail_level_for_relevance(0.5), DetailLevel::Summary);
        assert_eq!(detail_level_for_relevance(0.41), DetailLevel::Summary);

        assert_eq!(detail_level_for_relevance(0.4), DetailLevel::Minimal);
        assert_eq!(detail_level_for_relevance(0.2), DetailLevel::Minimal);
        assert_eq!(detail_level_for_relevance(0.0), DetailLevel::Minimal);
    }

    #[test]
    fn test_budgeted_serializer_new() {
        let serializer = BudgetedSerializer::new(4000);
        assert!(serializer.is_ok());

        let serializer = serializer.unwrap();
        assert_eq!(serializer.budget(), 4000);
    }

    #[test]
    fn test_budgeted_serializer_markdown() {
        let mut graph = ForgeGraph::new();
        graph
            .add_node(create_test_service("ns", "api", "API Service"))
            .unwrap();
        graph
            .add_node(create_test_database("ns", "db", "Database"))
            .unwrap();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "api").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let serializer = BudgetedSerializer::new(2000).unwrap();

        let output = serializer.serialize_within_budget(&subgraph, OutputFormat::Markdown);

        assert!(output.contains("Budget: 2000 tokens"));
        assert!(output.contains("API Service"));
    }

    #[test]
    fn test_budgeted_serializer_json() {
        let mut graph = ForgeGraph::new();
        graph
            .add_node(create_test_service("ns", "api", "API Service"))
            .unwrap();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "api").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let serializer = BudgetedSerializer::new(1000).unwrap();

        let output = serializer.serialize_within_budget(&subgraph, OutputFormat::Json);

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.get("budget").is_some());
        assert!(parsed.get("nodes").is_some());
        assert!(parsed.get("edges").is_some());
    }

    #[test]
    fn test_budgeted_serializer_mermaid() {
        let mut graph = ForgeGraph::new();
        graph
            .add_node(create_test_service("ns", "api", "API Service"))
            .unwrap();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "api").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let serializer = BudgetedSerializer::new(1000).unwrap();

        let output = serializer.serialize_within_budget(&subgraph, OutputFormat::Mermaid);

        assert!(output.contains("flowchart LR"));
        assert!(output.contains("Budget: 1000 tokens"));
    }

    #[test]
    fn test_budgeted_serializer_respects_budget() {
        let mut graph = ForgeGraph::new();

        // Add many services to potentially exceed budget
        for i in 0..20 {
            graph
                .add_node(create_test_service(
                    "ns",
                    &format!("service-{}", i),
                    &format!("Service {} with a longer display name", i),
                ))
                .unwrap();
        }

        let seed_nodes: Vec<_> = (0..20)
            .map(|i| NodeId::new(NodeType::Service, "ns", &format!("service-{}", i)).unwrap())
            .collect();

        let config = SubgraphConfig {
            seed_nodes,
            max_depth: 0,
            include_implicit_couplings: false,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Very small budget - should limit output
        let serializer = BudgetedSerializer::new(500).unwrap();
        let output = serializer.serialize_within_budget(&subgraph, OutputFormat::Markdown);

        // Output should be limited (not all 20 services)
        let service_count = output.matches("Service").count();
        assert!(
            service_count < 20,
            "Expected fewer than 20 services due to budget, got {}",
            service_count
        );
    }

    #[test]
    fn test_fits_within_budget() {
        let mut graph = ForgeGraph::new();
        graph
            .add_node(create_test_service("ns", "api", "API"))
            .unwrap();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "api").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);

        // Large budget should fit
        let large_serializer = BudgetedSerializer::new(10000).unwrap();
        assert!(large_serializer.fits_within_budget(&subgraph));

        // Tiny budget might not fit
        let tiny_serializer = BudgetedSerializer::new(10).unwrap();
        assert!(!tiny_serializer.fits_within_budget(&subgraph));
    }

    #[test]
    fn test_estimate_tokens() {
        let mut graph = ForgeGraph::new();
        graph
            .add_node(create_test_service("ns", "api", "API Service"))
            .unwrap();
        graph
            .add_node(create_test_database("ns", "db", "Database"))
            .unwrap();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "api").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let serializer = BudgetedSerializer::new(5000).unwrap();

        let estimate = serializer.estimate_tokens(&subgraph);
        assert!(estimate > 0);
    }

    #[test]
    fn test_sanitize_mermaid_id() {
        assert_eq!(
            sanitize_mermaid_id("service:ns:user-api"),
            "service_ns_user_api"
        );
        assert_eq!(
            sanitize_mermaid_id("database:ns:users.table"),
            "database_ns_users_table"
        );
        assert_eq!(sanitize_mermaid_id("simple"), "simple");
    }

    #[test]
    fn test_token_count_accuracy() {
        // Test that token counting is reasonably accurate
        let counter = TokenCounter::new().unwrap();

        // Known approximate token counts for cl100k_base
        // "Hello" is about 1 token
        assert!(counter.count("Hello") <= 2);

        // A typical sentence should be 10-20 tokens
        let sentence = "The quick brown fox jumps over the lazy dog.";
        let count = counter.count(sentence);
        assert!(count >= 8 && count <= 15, "Sentence token count: {}", count);

        // Code-like content
        let code = "function getData() { return fetch('/api/data'); }";
        let code_count = counter.count(code);
        assert!(
            code_count >= 10 && code_count <= 25,
            "Code token count: {}",
            code_count
        );
    }

    #[test]
    fn test_output_format_enum() {
        // Test that OutputFormat values are distinct
        assert_ne!(OutputFormat::Markdown, OutputFormat::Json);
        assert_ne!(OutputFormat::Json, OutputFormat::Mermaid);
        assert_ne!(OutputFormat::Markdown, OutputFormat::Mermaid);
    }
}
