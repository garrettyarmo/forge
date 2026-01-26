//! Markdown serializer for knowledge graphs.
//!
//! Produces human-readable, LLM-optimized markdown documentation
//! from knowledge graphs and extracted subgraphs.
//!
//! ## Output Structure
//!
//! The markdown output is organized into sections:
//! 1. **Services**: All service nodes with dependencies and business context
//! 2. **Databases**: Database nodes with access patterns
//! 3. **Queues**: Message queues with publishers/subscribers
//! 4. **Cloud Resources**: Other cloud resources with usage patterns
//! 5. **Implicit Couplings**: Risk summary for shared resource couplings
//!
//! ## Example Output
//!
//! ```markdown
//! # Ecosystem Knowledge Graph
//!
//! ## Services
//!
//! ### user-service
//! **Type**: Service | **Language**: TypeScript | **Framework**: Express
//!
//! **Purpose**: Handles user authentication and profile management
//!
//! **Dependencies**:
//! | Target | Relationship | Evidence |
//! |--------|--------------|----------|
//! | auth-service | Calls | src/auth.ts:42 |
//! | users-table | Reads, Writes | src/db/users.ts:15 |
//! ```

use forge_graph::{EdgeType, ExtractedSubgraph, ForgeGraph, Node, NodeType, ScoredNode};
use std::fmt::Write;

/// Detail level for markdown output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailLevel {
    /// Full details including all attributes and evidence
    #[default]
    Full,
    /// Summary with key attributes only
    Summary,
    /// Minimal: just names and relationships
    Minimal,
}

/// Markdown serializer for knowledge graphs.
///
/// Converts `ForgeGraph` and `ExtractedSubgraph` instances into
/// human-readable markdown documentation.
#[derive(Debug, Clone)]
pub struct MarkdownSerializer {
    /// Include business context annotations
    include_business_context: bool,

    /// Include edge evidence (file:line)
    include_evidence: bool,

    /// Maximum evidence items to show per relationship
    max_evidence_items: usize,

    /// Detail level for output
    detail_level: DetailLevel,

    /// Number of days after which a node is considered stale (0 = disabled)
    staleness_days: u32,
}

impl Default for MarkdownSerializer {
    fn default() -> Self {
        Self {
            include_business_context: true,
            include_evidence: true,
            max_evidence_items: 3,
            detail_level: DetailLevel::Full,
            staleness_days: 7,
        }
    }
}

impl MarkdownSerializer {
    /// Create a new MarkdownSerializer with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the detail level.
    pub fn with_detail_level(mut self, level: DetailLevel) -> Self {
        self.detail_level = level;
        self
    }

    /// Set whether to include business context.
    pub fn with_business_context(mut self, include: bool) -> Self {
        self.include_business_context = include;
        self
    }

    /// Set whether to include evidence.
    pub fn with_evidence(mut self, include: bool) -> Self {
        self.include_evidence = include;
        self
    }

    /// Set the maximum number of evidence items to show.
    pub fn with_max_evidence(mut self, max: usize) -> Self {
        self.max_evidence_items = max;
        self
    }

    /// Set the staleness threshold in days (0 to disable staleness indicators).
    pub fn with_staleness_days(mut self, days: u32) -> Self {
        self.staleness_days = days;
        self
    }

    /// Serialize an entire graph to markdown.
    pub fn serialize_graph(&self, graph: &ForgeGraph) -> String {
        let mut output = String::new();

        writeln!(output, "# Ecosystem Knowledge Graph\n").unwrap();

        // Services section
        self.write_services_section(&mut output, graph);

        // Databases section
        self.write_databases_section(&mut output, graph);

        // Queues section
        self.write_queues_section(&mut output, graph);

        // Cloud Resources section
        self.write_cloud_resources_section(&mut output, graph);

        // APIs section (if any)
        self.write_apis_section(&mut output, graph);

        // Couplings summary
        self.write_couplings_summary(&mut output, graph);

        output
    }

    /// Serialize an extracted subgraph to markdown.
    ///
    /// Includes relevance indicators for nodes based on their scores.
    pub fn serialize_subgraph(&self, subgraph: &ExtractedSubgraph<'_>) -> String {
        let mut output = String::new();

        writeln!(output, "# Relevant Context\n").unwrap();

        // Group nodes by type
        let mut services: Vec<&ScoredNode> = vec![];
        let mut databases: Vec<&ScoredNode> = vec![];
        let mut queues: Vec<&ScoredNode> = vec![];
        let mut resources: Vec<&ScoredNode> = vec![];

        for scored_node in &subgraph.nodes {
            match scored_node.node.node_type {
                NodeType::Service => services.push(scored_node),
                NodeType::Database => databases.push(scored_node),
                NodeType::Queue => queues.push(scored_node),
                NodeType::CloudResource => resources.push(scored_node),
                NodeType::Api => {} // APIs are documented with their owning service
            }
        }

        // Write sections
        if !services.is_empty() {
            self.write_scored_section(&mut output, "Services", &services, subgraph);
        }

        if !databases.is_empty() {
            self.write_scored_section(&mut output, "Databases", &databases, subgraph);
        }

        if !queues.is_empty() {
            self.write_scored_section(&mut output, "Queues", &queues, subgraph);
        }

        if !resources.is_empty() {
            self.write_scored_section(&mut output, "Cloud Resources", &resources, subgraph);
        }

        output
    }

    fn write_services_section(&self, output: &mut String, graph: &ForgeGraph) {
        let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
        if services.is_empty() {
            return;
        }

        writeln!(output, "## Services\n").unwrap();

        for service in services {
            self.write_service_node(output, service, graph);
        }
    }

    fn write_service_node(&self, output: &mut String, node: &Node, graph: &ForgeGraph) {
        writeln!(output, "### {}\n", node.display_name).unwrap();

        // Basic info
        let language = node
            .attributes
            .get("language")
            .and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("unknown");

        let framework = node.attributes.get("framework").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        });

        write!(output, "**Type**: Service | **Language**: {}", language).unwrap();
        if let Some(fw) = framework {
            write!(output, " | **Framework**: {}", fw).unwrap();
        }
        writeln!(output, "\n").unwrap();

        // Repo URL if present
        if let Some(repo_url) = node.attributes.get("repo_url").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            writeln!(output, "**Repository**: {}\n", repo_url).unwrap();
        }

        // Staleness indicator
        if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
            let age_desc = node.metadata.staleness_description();
            writeln!(
                output,
                "⚠️ **Status**: Stale ({}) - May be outdated\n",
                age_desc
            )
            .unwrap();
        }

        // Business context
        if self.include_business_context {
            if let Some(ctx) = &node.business_context {
                if let Some(purpose) = &ctx.purpose {
                    writeln!(output, "**Purpose**: {}\n", purpose).unwrap();
                }
                if let Some(owner) = &ctx.owner {
                    writeln!(output, "**Owner**: {}\n", owner).unwrap();
                }
                if let Some(history) = &ctx.history {
                    writeln!(output, "**History**: {}\n", history).unwrap();
                }
                if !ctx.gotchas.is_empty() {
                    writeln!(output, "**Gotchas**:").unwrap();
                    for gotcha in &ctx.gotchas {
                        writeln!(output, "- {}", gotcha).unwrap();
                    }
                    writeln!(output).unwrap();
                }
            }
        }

        // Dependencies (outgoing edges)
        let deps: Vec<_> = graph.edges_from(&node.id);
        if !deps.is_empty() {
            writeln!(output, "**Dependencies**:").unwrap();
            writeln!(output, "| Target | Relationship | Evidence |").unwrap();
            writeln!(output, "|--------|--------------|----------|").unwrap();

            for edge in deps {
                if let Some(target) = graph.get_node(&edge.target) {
                    let evidence = self.format_evidence(&edge.metadata.evidence);
                    writeln!(
                        output,
                        "| {} | {} | {} |",
                        target.display_name,
                        format_edge_type(edge.edge_type),
                        evidence
                    )
                    .unwrap();
                }
            }
            writeln!(output).unwrap();
        }

        // Dependents (incoming CALLS edges)
        let dependents: Vec<_> = graph
            .edges_to(&node.id)
            .into_iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();

        if !dependents.is_empty() {
            writeln!(output, "**Dependents**:").unwrap();
            writeln!(output, "| Source | Relationship |").unwrap();
            writeln!(output, "|--------|--------------|").unwrap();

            for edge in dependents {
                if let Some(source) = graph.get_node(&edge.source) {
                    writeln!(output, "| {} | Calls |", source.display_name).unwrap();
                }
            }
            writeln!(output).unwrap();
        }

        // Implicit couplings
        let couplings: Vec<_> = graph
            .edges_from(&node.id)
            .into_iter()
            .chain(graph.edges_to(&node.id))
            .filter(|e| e.edge_type == EdgeType::ImplicitlyCoupled)
            .collect();

        if !couplings.is_empty() {
            writeln!(output, "**Implicit Couplings**:").unwrap();
            for edge in couplings {
                let other_id = if edge.source == node.id {
                    &edge.target
                } else {
                    &edge.source
                };
                if let Some(other) = graph.get_node(other_id) {
                    let reason = edge.metadata.reason.as_deref().unwrap_or("shared resource");
                    writeln!(output, "- `{}` - {}", other.display_name, reason).unwrap();
                }
            }
            writeln!(output).unwrap();
        }

        writeln!(output, "---\n").unwrap();
    }

    fn write_databases_section(&self, output: &mut String, graph: &ForgeGraph) {
        let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
        if databases.is_empty() {
            return;
        }

        writeln!(output, "## Databases\n").unwrap();

        for db in databases {
            self.write_database_node(output, db, graph);
        }
    }

    fn write_database_node(&self, output: &mut String, node: &Node, graph: &ForgeGraph) {
        writeln!(output, "### {}\n", node.display_name).unwrap();

        let db_type = node
            .attributes
            .get("db_type")
            .and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("unknown");

        writeln!(output, "**Type**: {}\n", db_type).unwrap();

        // Table name if different from display name
        if let Some(table_name) = node.attributes.get("table_name").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            if table_name != node.display_name {
                writeln!(output, "**Table Name**: {}\n", table_name).unwrap();
            }
        }

        // ARN if present
        if let Some(arn) = node.attributes.get("arn").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            writeln!(output, "**ARN**: `{}`\n", arn).unwrap();
        }

        // Staleness indicator
        if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
            let age_desc = node.metadata.staleness_description();
            writeln!(
                output,
                "⚠️ **Status**: Stale ({}) - May be outdated\n",
                age_desc
            )
            .unwrap();
        }

        // Find owner (service that OWNS this database)
        let owner = graph
            .edges_to(&node.id)
            .into_iter()
            .find(|e| e.edge_type == EdgeType::Owns)
            .and_then(|e| graph.get_node(&e.source));

        if let Some(owner_node) = owner {
            writeln!(output, "**Owner**: {}\n", owner_node.display_name).unwrap();
        }

        // Accessing services
        let accessors: Vec<_> = graph
            .edges_to(&node.id)
            .into_iter()
            .filter(|e| {
                matches!(
                    e.edge_type,
                    EdgeType::Reads
                        | EdgeType::Writes
                        | EdgeType::ReadsShared
                        | EdgeType::WritesShared
                )
            })
            .collect();

        if !accessors.is_empty() {
            writeln!(output, "**Accessed By**:").unwrap();
            writeln!(output, "| Service | Access Type | Evidence |").unwrap();
            writeln!(output, "|---------|-------------|----------|").unwrap();

            for edge in accessors {
                if let Some(service) = graph.get_node(&edge.source) {
                    let access_type = match edge.edge_type {
                        EdgeType::Reads | EdgeType::ReadsShared => "READ",
                        EdgeType::Writes | EdgeType::WritesShared => "WRITE",
                        _ => "UNKNOWN",
                    };
                    let evidence = self.format_evidence(&edge.metadata.evidence);
                    writeln!(
                        output,
                        "| {} | {} | {} |",
                        service.display_name, access_type, evidence
                    )
                    .unwrap();
                }
            }
            writeln!(output).unwrap();
        }

        writeln!(output, "---\n").unwrap();
    }

    fn write_queues_section(&self, output: &mut String, graph: &ForgeGraph) {
        let queues: Vec<_> = graph.nodes_by_type(NodeType::Queue).collect();
        if queues.is_empty() {
            return;
        }

        writeln!(output, "## Queues\n").unwrap();

        for queue in queues {
            self.write_queue_node(output, queue, graph);
        }
    }

    fn write_queue_node(&self, output: &mut String, node: &Node, graph: &ForgeGraph) {
        writeln!(output, "### {}\n", node.display_name).unwrap();

        let queue_type = node
            .attributes
            .get("queue_type")
            .and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("unknown");

        writeln!(output, "**Type**: {}\n", queue_type).unwrap();

        // ARN if present
        if let Some(arn) = node.attributes.get("arn").and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.as_str()),
            _ => None,
        }) {
            writeln!(output, "**ARN**: `{}`\n", arn).unwrap();
        }

        // Staleness indicator
        if self.staleness_days > 0 && node.metadata.is_stale(self.staleness_days) {
            let age_desc = node.metadata.staleness_description();
            writeln!(
                output,
                "⚠️ **Status**: Stale ({}) - May be outdated\n",
                age_desc
            )
            .unwrap();
        }

        // Publishers
        let publishers: Vec<_> = graph
            .edges_to(&node.id)
            .into_iter()
            .filter(|e| e.edge_type == EdgeType::Publishes)
            .filter_map(|e| graph.get_node(&e.source))
            .collect();

        if !publishers.is_empty() {
            let names: Vec<_> = publishers.iter().map(|n| n.display_name.as_str()).collect();
            writeln!(output, "**Publishers**: {}\n", names.join(", ")).unwrap();
        }

        // Subscribers
        let subscribers: Vec<_> = graph
            .edges_to(&node.id)
            .into_iter()
            .filter(|e| e.edge_type == EdgeType::Subscribes)
            .filter_map(|e| graph.get_node(&e.source))
            .collect();

        if !subscribers.is_empty() {
            let names: Vec<_> = subscribers
                .iter()
                .map(|n| n.display_name.as_str())
                .collect();
            writeln!(output, "**Subscribers**: {}\n", names.join(", ")).unwrap();
        }

        writeln!(output, "---\n").unwrap();
    }

    fn write_cloud_resources_section(&self, output: &mut String, graph: &ForgeGraph) {
        let resources: Vec<_> = graph.nodes_by_type(NodeType::CloudResource).collect();
        if resources.is_empty() {
            return;
        }

        writeln!(output, "## Cloud Resources\n").unwrap();

        for resource in resources {
            let resource_type = resource
                .attributes
                .get("resource_type")
                .and_then(|v| match v {
                    forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("unknown");

            writeln!(
                output,
                "### {} ({})\n",
                resource.display_name, resource_type
            )
            .unwrap();

            // ARN if present
            if let Some(arn) = resource.attributes.get("arn").and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            }) {
                writeln!(output, "**ARN**: `{}`\n", arn).unwrap();
            }

            // Staleness indicator
            if self.staleness_days > 0 && resource.metadata.is_stale(self.staleness_days) {
                let age_desc = resource.metadata.staleness_description();
                writeln!(
                    output,
                    "⚠️ **Status**: Stale ({}) - May be outdated\n",
                    age_desc
                )
                .unwrap();
            }

            let users: Vec<_> = graph
                .edges_to(&resource.id)
                .into_iter()
                .filter(|e| e.edge_type == EdgeType::Uses)
                .filter_map(|e| graph.get_node(&e.source))
                .collect();

            if !users.is_empty() {
                let names: Vec<_> = users.iter().map(|n| n.display_name.as_str()).collect();
                writeln!(output, "**Used By**: {}\n", names.join(", ")).unwrap();
            }

            writeln!(output, "---\n").unwrap();
        }
    }

    fn write_apis_section(&self, output: &mut String, graph: &ForgeGraph) {
        let apis: Vec<_> = graph.nodes_by_type(NodeType::Api).collect();
        if apis.is_empty() {
            return;
        }

        writeln!(output, "## APIs\n").unwrap();

        for api in apis {
            writeln!(output, "### {}\n", api.display_name).unwrap();

            // Path if present
            if let Some(path) = api.attributes.get("path").and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            }) {
                writeln!(output, "**Path**: `{}`\n", path).unwrap();
            }

            // Method if present
            if let Some(method) = api.attributes.get("method").and_then(|v| match v {
                forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                _ => None,
            }) {
                writeln!(output, "**Method**: {}\n", method).unwrap();
            }

            // Find owner
            let owner = graph
                .edges_to(&api.id)
                .into_iter()
                .find(|e| e.edge_type == EdgeType::Owns)
                .and_then(|e| graph.get_node(&e.source));

            if let Some(owner_node) = owner {
                writeln!(output, "**Owner**: {}\n", owner_node.display_name).unwrap();
            }

            writeln!(output, "---\n").unwrap();
        }
    }

    fn write_couplings_summary(&self, output: &mut String, graph: &ForgeGraph) {
        let couplings: Vec<_> = graph
            .edges()
            .filter(|e| e.edge_type == EdgeType::ImplicitlyCoupled)
            .collect();

        if couplings.is_empty() {
            return;
        }

        writeln!(output, "## Implicit Couplings (Risk Summary)\n").unwrap();
        writeln!(output, "| Services | Shared Resource | Risk | Reason |").unwrap();
        writeln!(output, "|----------|-----------------|------|--------|").unwrap();

        for edge in couplings {
            let service_a = graph
                .get_node(&edge.source)
                .map(|n| n.display_name.as_str())
                .unwrap_or("?");
            let service_b = graph
                .get_node(&edge.target)
                .map(|n| n.display_name.as_str())
                .unwrap_or("?");

            let reason = edge.metadata.reason.as_deref().unwrap_or("shared resource");
            let risk = classify_risk(reason);

            // Try to extract shared resource from reason
            let shared_resource = extract_shared_resource(reason);

            writeln!(
                output,
                "| {} ↔ {} | {} | {} | {} |",
                service_a, service_b, shared_resource, risk, reason
            )
            .unwrap();
        }

        writeln!(output).unwrap();
    }

    fn write_scored_section(
        &self,
        output: &mut String,
        title: &str,
        nodes: &[&ScoredNode<'_>],
        subgraph: &ExtractedSubgraph<'_>,
    ) {
        writeln!(output, "## {}\n", title).unwrap();

        for scored in nodes {
            let relevance_indicator = if scored.score > 0.8 {
                "[HIGH]"
            } else if scored.score > 0.5 {
                "[MEDIUM]"
            } else {
                "[LOW]"
            };

            writeln!(
                output,
                "### {} {} (relevance: {:.0}%)\n",
                scored.node.display_name,
                relevance_indicator,
                scored.score * 100.0
            )
            .unwrap();

            // Write details based on relevance
            if scored.score > 0.5 {
                self.write_service_details(output, scored.node, subgraph);
            } else {
                // Just show existence for low-relevance nodes
                writeln!(output, "*Exists in ecosystem*\n").unwrap();
            }

            writeln!(output, "---\n").unwrap();
        }
    }

    fn write_service_details(
        &self,
        output: &mut String,
        node: &Node,
        subgraph: &ExtractedSubgraph<'_>,
    ) {
        // Type info based on node type
        match node.node_type {
            NodeType::Service => {
                if let Some(lang) = node.attributes.get("language").and_then(|v| match v {
                    forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                    _ => None,
                }) {
                    writeln!(output, "**Language**: {}", lang).unwrap();
                }
            }
            NodeType::Database => {
                if let Some(db_type) = node.attributes.get("db_type").and_then(|v| match v {
                    forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                    _ => None,
                }) {
                    writeln!(output, "**Type**: {}", db_type).unwrap();
                }
            }
            NodeType::Queue => {
                if let Some(q_type) = node.attributes.get("queue_type").and_then(|v| match v {
                    forge_graph::AttributeValue::String(s) => Some(s.as_str()),
                    _ => None,
                }) {
                    writeln!(output, "**Type**: {}", q_type).unwrap();
                }
            }
            _ => {}
        }

        // Business context
        if self.include_business_context {
            if let Some(ctx) = &node.business_context {
                if let Some(purpose) = &ctx.purpose {
                    writeln!(output, "**Purpose**: {}", purpose).unwrap();
                }
            }
        }

        // Related edges in subgraph
        let related_edges: Vec<_> = subgraph
            .edges
            .iter()
            .filter(|e| e.source == node.id || e.target == node.id)
            .collect();

        if !related_edges.is_empty() {
            writeln!(output, "\n**Relationships in context**:").unwrap();
            for edge in related_edges {
                let other_id = if edge.source == node.id {
                    &edge.target
                } else {
                    &edge.source
                };
                let direction = if edge.source == node.id { "→" } else { "←" };

                if let Some(other) = subgraph.graph().get_node(other_id) {
                    writeln!(
                        output,
                        "- {} {} {}",
                        direction,
                        format_edge_type(edge.edge_type),
                        other.display_name
                    )
                    .unwrap();
                }
            }
        }

        writeln!(output).unwrap();
    }

    fn format_evidence(&self, evidence: &[String]) -> String {
        if !self.include_evidence || evidence.is_empty() {
            return "-".to_string();
        }

        let items: Vec<_> = evidence
            .iter()
            .take(self.max_evidence_items)
            .map(|e| format!("`{}`", e))
            .collect();

        let mut result = items.join(", ");

        if evidence.len() > self.max_evidence_items {
            result.push_str(&format!(
                " +{} more",
                evidence.len() - self.max_evidence_items
            ));
        }

        result
    }
}

/// Format an edge type for display.
fn format_edge_type(edge_type: EdgeType) -> &'static str {
    match edge_type {
        EdgeType::Calls => "Calls",
        EdgeType::Owns => "Owns",
        EdgeType::Reads => "Reads",
        EdgeType::Writes => "Writes",
        EdgeType::Publishes => "Publishes",
        EdgeType::Subscribes => "Subscribes",
        EdgeType::Uses => "Uses",
        EdgeType::ReadsShared => "Reads (shared)",
        EdgeType::WritesShared => "Writes (shared)",
        EdgeType::ImplicitlyCoupled => "Implicitly Coupled",
    }
}

/// Classify risk level from a coupling reason.
fn classify_risk(reason: &str) -> &'static str {
    let reason_lower = reason.to_lowercase();
    if reason_lower.contains("high") || reason_lower.contains("multiple writers") {
        "High"
    } else if reason_lower.contains("medium") || reason_lower.contains("write") {
        "Medium"
    } else {
        "Low"
    }
}

/// Extract shared resource name from a coupling reason if possible.
fn extract_shared_resource(reason: &str) -> String {
    // Try to find patterns like "via shared X" or "shared X"
    if let Some(idx) = reason.find("shared ") {
        let rest = &reason[idx + 7..];
        // Take until space, comma, or end
        let end = rest.find([' ', ',', ')']).unwrap_or(rest.len());
        if end > 0 {
            return rest[..end].to_string();
        }
    }

    // Try to find patterns like "via X"
    if let Some(idx) = reason.find("via ") {
        let rest = &reason[idx + 4..];
        let end = rest.find([' ', ',', ')']).unwrap_or(rest.len());
        if end > 0 {
            return rest[..end].to_string();
        }
    }

    "-".to_string()
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
        // user-api reads users-table
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

        // user-api writes users-table
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

        // order-api calls user-api
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

        // order-api publishes to order-events
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
        let serializer = MarkdownSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should contain main header
        assert!(output.contains("# Ecosystem Knowledge Graph"));

        // Should contain services section
        assert!(output.contains("## Services"));
        assert!(output.contains("### User API"));
        assert!(output.contains("### Order API"));

        // Should contain database section
        assert!(output.contains("## Databases"));
        assert!(output.contains("### Users Table"));

        // Should contain queue section
        assert!(output.contains("## Queues"));
        assert!(output.contains("### Order Events"));
    }

    #[test]
    fn test_serialize_graph_with_relationships() {
        let graph = create_test_graph();
        let serializer = MarkdownSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should show dependencies
        assert!(output.contains("**Dependencies**:"));

        // Should show service calling another
        assert!(output.contains("Calls"));

        // Should show database access
        assert!(output.contains("Reads"));
        assert!(output.contains("Writes"));

        // Should show publisher
        assert!(output.contains("**Publishers**:"));
        assert!(output.contains("Order API"));
    }

    #[test]
    fn test_serialize_graph_empty() {
        let graph = ForgeGraph::new();
        let serializer = MarkdownSerializer::new();

        let output = serializer.serialize_graph(&graph);

        // Should still have header
        assert!(output.contains("# Ecosystem Knowledge Graph"));

        // Should not have any sections (no nodes)
        assert!(!output.contains("## Services"));
        assert!(!output.contains("## Databases"));
    }

    #[test]
    fn test_serialize_subgraph() {
        let graph = create_test_graph();
        let serializer = MarkdownSerializer::new();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()],
            max_depth: 1,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let output = serializer.serialize_subgraph(&subgraph);

        // Should contain relevant context header
        assert!(output.contains("# Relevant Context"));

        // Should include user-api (seed) with high relevance
        assert!(output.contains("User API"));
        assert!(output.contains("[HIGH]"));

        // Should include relevance percentages
        assert!(output.contains("relevance:"));
    }

    #[test]
    fn test_detail_level() {
        let serializer_full = MarkdownSerializer::new().with_detail_level(DetailLevel::Full);
        let serializer_minimal = MarkdownSerializer::new().with_detail_level(DetailLevel::Minimal);

        assert_eq!(serializer_full.detail_level, DetailLevel::Full);
        assert_eq!(serializer_minimal.detail_level, DetailLevel::Minimal);
    }

    #[test]
    fn test_format_evidence() {
        let serializer = MarkdownSerializer::new().with_max_evidence(2);

        let evidence = vec![
            "src/a.ts:10".to_string(),
            "src/b.ts:20".to_string(),
            "src/c.ts:30".to_string(),
        ];

        let formatted = serializer.format_evidence(&evidence);

        assert!(formatted.contains("`src/a.ts:10`"));
        assert!(formatted.contains("`src/b.ts:20`"));
        assert!(!formatted.contains("`src/c.ts:30`"));
        assert!(formatted.contains("+1 more"));
    }

    #[test]
    fn test_format_evidence_empty() {
        let serializer = MarkdownSerializer::new();

        let evidence: Vec<String> = vec![];
        let formatted = serializer.format_evidence(&evidence);

        assert_eq!(formatted, "-");
    }

    #[test]
    fn test_format_evidence_disabled() {
        let serializer = MarkdownSerializer::new().with_evidence(false);

        let evidence = vec!["src/a.ts:10".to_string()];
        let formatted = serializer.format_evidence(&evidence);

        assert_eq!(formatted, "-");
    }

    #[test]
    fn test_format_edge_type() {
        assert_eq!(format_edge_type(EdgeType::Calls), "Calls");
        assert_eq!(format_edge_type(EdgeType::Reads), "Reads");
        assert_eq!(format_edge_type(EdgeType::Writes), "Writes");
        assert_eq!(format_edge_type(EdgeType::ReadsShared), "Reads (shared)");
        assert_eq!(
            format_edge_type(EdgeType::ImplicitlyCoupled),
            "Implicitly Coupled"
        );
    }

    #[test]
    fn test_classify_risk() {
        assert_eq!(classify_risk("High risk: multiple writers"), "High");
        assert_eq!(
            classify_risk("Medium risk: one writes, one reads"),
            "Medium"
        );
        assert_eq!(classify_risk("Both services read data"), "Low");
        assert_eq!(classify_risk("write to same table"), "Medium");
    }

    #[test]
    fn test_extract_shared_resource() {
        assert_eq!(
            extract_shared_resource("via shared users-table"),
            "users-table"
        );
        assert_eq!(extract_shared_resource("via orders-db"), "orders-db");
        assert_eq!(extract_shared_resource("no resource info"), "-");
    }

    #[test]
    fn test_builder_pattern() {
        let serializer = MarkdownSerializer::new()
            .with_detail_level(DetailLevel::Summary)
            .with_business_context(false)
            .with_evidence(false)
            .with_max_evidence(5);

        assert_eq!(serializer.detail_level, DetailLevel::Summary);
        assert!(!serializer.include_business_context);
        assert!(!serializer.include_evidence);
        assert_eq!(serializer.max_evidence_items, 5);
    }

    #[test]
    fn test_implicit_couplings_section() {
        let mut graph = ForgeGraph::new();

        // Add two services
        graph
            .add_node(create_test_service("ns", "svc-a", "Service A"))
            .unwrap();
        graph
            .add_node(create_test_service("ns", "svc-b", "Service B"))
            .unwrap();

        // Add implicit coupling
        let mut edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
            NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
            EdgeType::ImplicitlyCoupled,
        )
        .unwrap();
        edge.metadata.reason = Some("High risk: both write to users-table".to_string());
        graph.add_edge(edge).unwrap();

        let serializer = MarkdownSerializer::new();
        let output = serializer.serialize_graph(&graph);

        assert!(output.contains("## Implicit Couplings (Risk Summary)"));
        assert!(output.contains("Service A ↔ Service B"));
        assert!(output.contains("High"));
    }

    #[test]
    fn test_service_with_business_context() {
        let mut graph = ForgeGraph::new();

        let mut service = create_test_service("ns", "auth-api", "Auth API");
        service.business_context = Some(forge_graph::BusinessContext {
            purpose: Some("Handles authentication and authorization".to_string()),
            owner: Some("Platform Team".to_string()),
            history: Some("Migrated from monolith in 2023".to_string()),
            gotchas: vec!["Rate limited to 100 req/s".to_string()],
            notes: Default::default(),
        });

        graph.add_node(service).unwrap();

        let serializer = MarkdownSerializer::new().with_business_context(true);
        let output = serializer.serialize_graph(&graph);

        assert!(output.contains("**Purpose**: Handles authentication"));
        assert!(output.contains("**Owner**: Platform Team"));
        assert!(output.contains("**History**: Migrated from monolith"));
        assert!(output.contains("**Gotchas**:"));
        assert!(output.contains("Rate limited to 100 req/s"));
    }

    #[test]
    fn test_service_without_business_context() {
        let mut graph = ForgeGraph::new();

        let mut service = create_test_service("ns", "auth-api", "Auth API");
        service.business_context = Some(forge_graph::BusinessContext {
            purpose: Some("Handles authentication".to_string()),
            ..Default::default()
        });

        graph.add_node(service).unwrap();

        let serializer = MarkdownSerializer::new().with_business_context(false);
        let output = serializer.serialize_graph(&graph);

        assert!(!output.contains("**Purpose**:"));
    }

    #[test]
    fn test_scored_nodes_output() {
        let graph = create_test_graph();
        let serializer = MarkdownSerializer::new();

        let config = SubgraphConfig {
            seed_nodes: vec![NodeId::new(NodeType::Service, "ns", "user-api").unwrap()],
            max_depth: 2,
            include_implicit_couplings: true,
            min_relevance: 0.0,
            edge_types: None,
        };

        let subgraph = graph.extract_subgraph(&config);
        let output = serializer.serialize_subgraph(&subgraph);

        // Seed should have highest relevance
        assert!(output.contains("[HIGH]") || output.contains("100%"));

        // Should have relevance indicators
        assert!(
            output.contains("[HIGH]") || output.contains("[MEDIUM]") || output.contains("[LOW]")
        );
    }
}
