//! Business context interview module for Forge.
//!
//! This module provides:
//! - Gap analysis to identify nodes lacking business context
//! - Question generation for interview sessions
//! - Interview flow management
//!
//! # Gap Analysis
//!
//! Gap analysis identifies nodes that need business context annotations.
//! Nodes are scored based on:
//! - Missing purpose or owner annotations
//! - High centrality (many connections)
//! - Implicit couplings with other services
//! - Shared resources without clear ownership
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_llm::interview::analyze_gaps;
//! use forge_graph::ForgeGraph;
//!
//! let graph = ForgeGraph::load_from_file("graph.json")?;
//! let gaps = analyze_gaps(&graph);
//!
//! for gap in gaps {
//!     println!("Node {} has gap score {:.2}", gap.node_id, gap.score);
//!     for reason in &gap.reasons {
//!         println!("  - {:?}", reason);
//!     }
//! }
//! ```

use forge_graph::{EdgeType, ForgeGraph, Node, NodeId, NodeType};
use std::collections::HashMap;

/// Score representing the need for business context on a node.
///
/// Higher scores indicate greater need for context annotation.
/// Scores range from 0.0 (fully documented) to 1.0 (needs immediate attention).
#[derive(Debug, Clone)]
pub struct ContextGapScore {
    /// The node this score applies to
    pub node_id: NodeId,

    /// Overall gap score (0.0 to 1.0)
    pub score: f64,

    /// Reasons contributing to the gap score
    pub reasons: Vec<GapReason>,
}

impl ContextGapScore {
    /// Create a new gap score for a node.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            score: 0.0,
            reasons: vec![],
        }
    }

    /// Add a reason and increment the score.
    pub fn add_reason(&mut self, reason: GapReason, score_contribution: f64) {
        self.reasons.push(reason);
        self.score += score_contribution;
        // Cap at 1.0
        if self.score > 1.0 {
            self.score = 1.0;
        }
    }
}

/// Reasons why a node needs business context.
#[derive(Debug, Clone, PartialEq)]
pub enum GapReason {
    /// No business purpose documented
    MissingPurpose,

    /// No owner documented
    MissingOwner,

    /// High connectivity (central to architecture)
    HighCentrality {
        /// Total number of edges (incoming + outgoing)
        edge_count: usize,
    },

    /// Has implicit couplings (needs explanation)
    ImplicitCoupling {
        /// Names of coupled services
        coupled_services: Vec<String>,
    },

    /// Shared resource without clear ownership
    SharedResourceWithoutOwner {
        /// Names of services accessing this resource
        accessor_services: Vec<String>,
    },

    /// No gotchas documented for complex service
    ComplexWithoutGotchas {
        /// Signals indicating complexity
        complexity_signals: Vec<String>,
    },
}

impl GapReason {
    /// Get a human-readable description of this gap reason.
    pub fn description(&self) -> String {
        match self {
            GapReason::MissingPurpose => "No business purpose documented".to_string(),
            GapReason::MissingOwner => "No owner documented".to_string(),
            GapReason::HighCentrality { edge_count } => {
                format!("High centrality with {} connections", edge_count)
            }
            GapReason::ImplicitCoupling { coupled_services } => {
                format!(
                    "Implicitly coupled with: {}",
                    coupled_services.join(", ")
                )
            }
            GapReason::SharedResourceWithoutOwner { accessor_services } => {
                format!(
                    "Shared resource accessed by {} services without clear owner",
                    accessor_services.len()
                )
            }
            GapReason::ComplexWithoutGotchas { complexity_signals } => {
                format!(
                    "Complex service without documented gotchas: {}",
                    complexity_signals.join(", ")
                )
            }
        }
    }
}

/// Configuration for gap analysis.
#[derive(Debug, Clone)]
pub struct GapAnalysisConfig {
    /// Minimum edge count to consider a node "high centrality"
    pub high_centrality_threshold: usize,

    /// Minimum edge count to consider a service "complex" (for gotchas check)
    pub complexity_threshold: usize,

    /// Score contribution for missing purpose
    pub missing_purpose_score: f64,

    /// Score contribution for missing owner
    pub missing_owner_score: f64,

    /// Maximum score contribution from centrality
    pub max_centrality_score: f64,

    /// Score contribution for having implicit couplings
    pub implicit_coupling_score: f64,

    /// Score contribution for shared resource without owner
    pub shared_resource_score: f64,

    /// Score contribution for complex service without gotchas
    pub complex_without_gotchas_score: f64,
}

impl Default for GapAnalysisConfig {
    fn default() -> Self {
        Self {
            high_centrality_threshold: 5,
            complexity_threshold: 3,
            missing_purpose_score: 0.3,
            missing_owner_score: 0.2,
            max_centrality_score: 0.2,
            implicit_coupling_score: 0.15,
            shared_resource_score: 0.25,
            complex_without_gotchas_score: 0.1,
        }
    }
}

/// Analyze a graph for context gaps.
///
/// Returns a list of nodes that need business context, sorted by gap score
/// (highest first). Only nodes with a positive gap score are returned.
///
/// # Arguments
/// * `graph` - The knowledge graph to analyze
///
/// # Returns
/// A vector of `ContextGapScore` sorted by score descending.
///
/// # Example
///
/// ```rust,ignore
/// let gaps = analyze_gaps(&graph);
/// println!("Found {} nodes needing context", gaps.len());
/// ```
pub fn analyze_gaps(graph: &ForgeGraph) -> Vec<ContextGapScore> {
    analyze_gaps_with_config(graph, &GapAnalysisConfig::default())
}

/// Analyze a graph for context gaps with custom configuration.
///
/// # Arguments
/// * `graph` - The knowledge graph to analyze
/// * `config` - Configuration for scoring thresholds
///
/// # Returns
/// A vector of `ContextGapScore` sorted by score descending.
pub fn analyze_gaps_with_config(
    graph: &ForgeGraph,
    config: &GapAnalysisConfig,
) -> Vec<ContextGapScore> {
    let mut scores: HashMap<NodeId, ContextGapScore> = HashMap::new();

    // Analyze each service
    for service in graph.nodes_by_type(NodeType::Service) {
        analyze_service_gaps(graph, service, config, &mut scores);
    }

    // Analyze shared resources (databases and queues)
    for db in graph.nodes_by_type(NodeType::Database) {
        analyze_shared_resource_gaps(graph, db, config, &mut scores);
    }
    for queue in graph.nodes_by_type(NodeType::Queue) {
        analyze_shared_resource_gaps(graph, queue, config, &mut scores);
    }

    // Convert to sorted vector
    let mut result: Vec<_> = scores
        .into_values()
        .filter(|s| s.score > 0.0)
        .collect();
    result.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

/// Analyze gaps for a service node.
fn analyze_service_gaps(
    graph: &ForgeGraph,
    service: &Node,
    config: &GapAnalysisConfig,
    scores: &mut HashMap<NodeId, ContextGapScore>,
) {
    let mut gap_score = ContextGapScore::new(service.id.clone());

    // Check for missing purpose
    let has_purpose = service
        .business_context
        .as_ref()
        .and_then(|bc| bc.purpose.as_ref())
        .map(|p| !p.is_empty())
        .unwrap_or(false);

    if !has_purpose {
        gap_score.add_reason(GapReason::MissingPurpose, config.missing_purpose_score);
    }

    // Check for missing owner
    let has_owner = service
        .business_context
        .as_ref()
        .and_then(|bc| bc.owner.as_ref())
        .map(|o| !o.is_empty())
        .unwrap_or(false);

    if !has_owner {
        gap_score.add_reason(GapReason::MissingOwner, config.missing_owner_score);
    }

    // Check centrality (edge count)
    let outgoing = graph.edges_from(&service.id).len();
    let incoming = graph.edges_to(&service.id).len();
    let total_edges = outgoing + incoming;

    if total_edges >= config.high_centrality_threshold {
        // Scale score based on how many edges above threshold
        let centrality_score =
            config.max_centrality_score * (total_edges as f64 / 10.0).min(1.0);
        gap_score.add_reason(
            GapReason::HighCentrality {
                edge_count: total_edges,
            },
            centrality_score,
        );
    }

    // Check for implicit couplings
    let coupled_services = find_implicit_couplings(graph, &service.id);
    if !coupled_services.is_empty() {
        gap_score.add_reason(
            GapReason::ImplicitCoupling {
                coupled_services: coupled_services.clone(),
            },
            config.implicit_coupling_score,
        );
    }

    // Check for gotchas in complex services
    let has_gotchas = service
        .business_context
        .as_ref()
        .map(|bc| !bc.gotchas.is_empty())
        .unwrap_or(false);

    if !has_gotchas && total_edges >= config.complexity_threshold {
        let complexity_signals = vec![format!("{} dependencies", total_edges)];
        gap_score.add_reason(
            GapReason::ComplexWithoutGotchas { complexity_signals },
            config.complex_without_gotchas_score,
        );
    }

    if gap_score.score > 0.0 {
        scores.insert(service.id.clone(), gap_score);
    }
}

/// Analyze gaps for a shared resource (database or queue).
fn analyze_shared_resource_gaps(
    graph: &ForgeGraph,
    resource: &Node,
    config: &GapAnalysisConfig,
    scores: &mut HashMap<NodeId, ContextGapScore>,
) {
    // Find all services that access this resource
    let accessor_services = find_resource_accessors(graph, &resource.id);

    // Only flag as shared if multiple services access it
    if accessor_services.len() > 1 {
        // Check if there's a clear owner (via OWNS edge)
        let has_owner = graph
            .edges_to(&resource.id)
            .iter()
            .any(|e| e.edge_type == EdgeType::Owns);

        if !has_owner {
            let gap_score = scores
                .entry(resource.id.clone())
                .or_insert_with(|| ContextGapScore::new(resource.id.clone()));

            gap_score.add_reason(
                GapReason::SharedResourceWithoutOwner {
                    accessor_services: accessor_services.clone(),
                },
                config.shared_resource_score,
            );
        }
    }
}

/// Find services implicitly coupled to this service.
fn find_implicit_couplings(graph: &ForgeGraph, service_id: &NodeId) -> Vec<String> {
    let mut coupled = Vec::new();

    // Check outgoing implicit coupling edges
    for edge in graph.edges_from(service_id) {
        if edge.edge_type == EdgeType::ImplicitlyCoupled {
            if let Some(target_node) = graph.get_node(&edge.target) {
                coupled.push(target_node.display_name.clone());
            }
        }
    }

    // Check incoming implicit coupling edges
    for edge in graph.edges_to(service_id) {
        if edge.edge_type == EdgeType::ImplicitlyCoupled {
            if let Some(source_node) = graph.get_node(&edge.source) {
                // Avoid duplicates
                if !coupled.contains(&source_node.display_name) {
                    coupled.push(source_node.display_name.clone());
                }
            }
        }
    }

    coupled
}

/// Find all services that access a resource.
fn find_resource_accessors(graph: &ForgeGraph, resource_id: &NodeId) -> Vec<String> {
    let mut accessors = Vec::new();

    for edge in graph.edges_to(resource_id) {
        // Check for any access edge type
        let is_access = matches!(
            edge.edge_type,
            EdgeType::Reads
                | EdgeType::Writes
                | EdgeType::ReadsShared
                | EdgeType::WritesShared
                | EdgeType::Publishes
                | EdgeType::Subscribes
        );

        if is_access {
            if let Some(source_node) = graph.get_node(&edge.source) {
                if !accessors.contains(&source_node.display_name) {
                    accessors.push(source_node.display_name.clone());
                }
            }
        }
    }

    accessors
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_graph::{
        BusinessContext, DiscoverySource, Edge, Node, NodeBuilder, NodeId, NodeType,
    };

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

    fn create_test_queue(namespace: &str, name: &str, display: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Queue, namespace, name).unwrap())
            .node_type(NodeType::Queue)
            .display_name(display)
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    #[test]
    fn test_detect_missing_purpose() {
        let mut graph = ForgeGraph::new();

        let node = create_test_service("ns", "svc", "Test Service");
        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);

        assert!(!gaps.is_empty());
        assert!(gaps[0]
            .reasons
            .iter()
            .any(|r| matches!(r, GapReason::MissingPurpose)));
    }

    #[test]
    fn test_detect_missing_owner() {
        let mut graph = ForgeGraph::new();

        let node = create_test_service("ns", "svc", "Test Service");
        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);

        assert!(!gaps.is_empty());
        assert!(gaps[0]
            .reasons
            .iter()
            .any(|r| matches!(r, GapReason::MissingOwner)));
    }

    #[test]
    fn test_no_gap_when_fully_annotated() {
        let mut graph = ForgeGraph::new();

        let mut node = create_test_service("ns", "svc", "Test Service");
        node.business_context = Some(BusinessContext {
            purpose: Some("Handles authentication".to_string()),
            owner: Some("Auth Team".to_string()),
            history: None,
            gotchas: vec!["Rate limited".to_string()],
            notes: Default::default(),
        });

        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);

        // Should have no gaps for purpose/owner
        if !gaps.is_empty() {
            assert!(!gaps[0]
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::MissingPurpose)));
            assert!(!gaps[0]
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::MissingOwner)));
        }
    }

    #[test]
    fn test_detect_high_centrality() {
        let mut graph = ForgeGraph::new();

        // Create a central service with many connections
        let central = create_test_service("ns", "central", "Central Service");
        graph.add_node(central).unwrap();

        // Create 6 other services that connect to central
        for i in 0..6 {
            let svc = create_test_service("ns", &format!("svc-{}", i), &format!("Service {}", i));
            graph.add_node(svc).unwrap();

            // Each service calls central
            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", &format!("svc-{}", i)).unwrap(),
                NodeId::new(NodeType::Service, "ns", "central").unwrap(),
                EdgeType::Calls,
            )
            .unwrap();
            graph.add_edge(edge).unwrap();
        }

        let gaps = analyze_gaps(&graph);

        // Find central service's gap
        let central_id = NodeId::new(NodeType::Service, "ns", "central").unwrap();
        let central_gap = gaps.iter().find(|g| g.node_id == central_id);

        assert!(central_gap.is_some());
        assert!(central_gap
            .unwrap()
            .reasons
            .iter()
            .any(|r| matches!(r, GapReason::HighCentrality { edge_count: 6 })));
    }

    #[test]
    fn test_detect_implicit_coupling() {
        let mut graph = ForgeGraph::new();

        // Create two services
        let svc_a = create_test_service("ns", "svc-a", "Service A");
        let svc_b = create_test_service("ns", "svc-b", "Service B");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        // Add implicit coupling edge
        let edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
            NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
            EdgeType::ImplicitlyCoupled,
        )
        .unwrap();
        graph.add_edge(edge).unwrap();

        let gaps = analyze_gaps(&graph);

        // Service A should have implicit coupling reason
        let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
        let svc_a_gap = gaps.iter().find(|g| g.node_id == svc_a_id);

        assert!(svc_a_gap.is_some());
        let has_coupling_reason = svc_a_gap.unwrap().reasons.iter().any(|r| {
            matches!(r, GapReason::ImplicitCoupling { coupled_services } if coupled_services.contains(&"Service B".to_string()))
        });
        assert!(has_coupling_reason);
    }

    #[test]
    fn test_detect_shared_resource_without_owner() {
        let mut graph = ForgeGraph::new();

        // Create a database
        let db = create_test_database("ns", "shared-db", "Shared Database");
        graph.add_node(db).unwrap();

        // Create two services that access it
        let svc_a = create_test_service("ns", "svc-a", "Service A");
        let svc_b = create_test_service("ns", "svc-b", "Service B");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        // Both services read from the database
        let edge_a = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();
        let edge_b = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
            NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();
        graph.add_edge(edge_a).unwrap();
        graph.add_edge(edge_b).unwrap();

        let gaps = analyze_gaps(&graph);

        // Database should have shared resource without owner reason
        let db_id = NodeId::new(NodeType::Database, "ns", "shared-db").unwrap();
        let db_gap = gaps.iter().find(|g| g.node_id == db_id);

        assert!(db_gap.is_some());
        let has_shared_reason = db_gap.unwrap().reasons.iter().any(|r| {
            matches!(r, GapReason::SharedResourceWithoutOwner { accessor_services } if accessor_services.len() == 2)
        });
        assert!(has_shared_reason);
    }

    #[test]
    fn test_no_shared_resource_gap_when_owned() {
        let mut graph = ForgeGraph::new();

        // Create a database
        let db = create_test_database("ns", "owned-db", "Owned Database");
        graph.add_node(db).unwrap();

        // Create two services
        let svc_a = create_test_service("ns", "svc-a", "Service A");
        let svc_b = create_test_service("ns", "svc-b", "Service B");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        // Service A owns the database
        let owns_edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "owned-db").unwrap(),
            EdgeType::Owns,
        )
        .unwrap();
        graph.add_edge(owns_edge).unwrap();

        // Both services read from the database
        let edge_a = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "owned-db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();
        let edge_b = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
            NodeId::new(NodeType::Database, "ns", "owned-db").unwrap(),
            EdgeType::ReadsShared,
        )
        .unwrap();
        graph.add_edge(edge_a).unwrap();
        graph.add_edge(edge_b).unwrap();

        let gaps = analyze_gaps(&graph);

        // Database should NOT have shared resource without owner reason
        let db_id = NodeId::new(NodeType::Database, "ns", "owned-db").unwrap();
        let db_gap = gaps.iter().find(|g| g.node_id == db_id);

        // Either no gap for database, or no SharedResourceWithoutOwner reason
        if let Some(gap) = db_gap {
            assert!(!gap
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::SharedResourceWithoutOwner { .. })));
        }
    }

    #[test]
    fn test_detect_complex_without_gotchas() {
        let mut graph = ForgeGraph::new();

        // Create a service with 4 connections (above complexity threshold of 3)
        let complex = create_test_service("ns", "complex", "Complex Service");
        graph.add_node(complex).unwrap();

        // Create databases it connects to
        for i in 0..4 {
            let db = create_test_database("ns", &format!("db-{}", i), &format!("Database {}", i));
            graph.add_node(db).unwrap();

            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", "complex").unwrap(),
                NodeId::new(NodeType::Database, "ns", &format!("db-{}", i)).unwrap(),
                EdgeType::Reads,
            )
            .unwrap();
            graph.add_edge(edge).unwrap();
        }

        let gaps = analyze_gaps(&graph);

        let complex_id = NodeId::new(NodeType::Service, "ns", "complex").unwrap();
        let complex_gap = gaps.iter().find(|g| g.node_id == complex_id);

        assert!(complex_gap.is_some());
        assert!(complex_gap
            .unwrap()
            .reasons
            .iter()
            .any(|r| matches!(r, GapReason::ComplexWithoutGotchas { .. })));
    }

    #[test]
    fn test_no_complex_gap_when_gotchas_documented() {
        let mut graph = ForgeGraph::new();

        // Create a service with gotchas documented
        let mut complex = create_test_service("ns", "complex", "Complex Service");
        complex.business_context = Some(BusinessContext {
            purpose: Some("Does complex things".to_string()),
            owner: Some("Team X".to_string()),
            history: None,
            gotchas: vec!["Watch out for X".to_string()],
            notes: Default::default(),
        });
        graph.add_node(complex).unwrap();

        // Create databases it connects to
        for i in 0..4 {
            let db = create_test_database("ns", &format!("db-{}", i), &format!("Database {}", i));
            graph.add_node(db).unwrap();

            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", "complex").unwrap(),
                NodeId::new(NodeType::Database, "ns", &format!("db-{}", i)).unwrap(),
                EdgeType::Reads,
            )
            .unwrap();
            graph.add_edge(edge).unwrap();
        }

        let gaps = analyze_gaps(&graph);

        let complex_id = NodeId::new(NodeType::Service, "ns", "complex").unwrap();
        let complex_gap = gaps.iter().find(|g| g.node_id == complex_id);

        // Should not have ComplexWithoutGotchas reason
        if let Some(gap) = complex_gap {
            assert!(!gap
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::ComplexWithoutGotchas { .. })));
        }
    }

    #[test]
    fn test_gaps_sorted_by_score() {
        let mut graph = ForgeGraph::new();

        // Create service with high gap (missing purpose, owner)
        let high_gap = create_test_service("ns", "high-gap", "High Gap Service");
        graph.add_node(high_gap).unwrap();

        // Create service with low gap (has purpose, missing owner)
        let mut low_gap = create_test_service("ns", "low-gap", "Low Gap Service");
        low_gap.business_context = Some(BusinessContext {
            purpose: Some("Has a purpose".to_string()),
            owner: None,
            history: None,
            gotchas: vec![],
            notes: Default::default(),
        });
        graph.add_node(low_gap).unwrap();

        let gaps = analyze_gaps(&graph);

        assert!(gaps.len() >= 2);
        // First gap should have higher score
        assert!(gaps[0].score >= gaps[1].score);
    }

    #[test]
    fn test_custom_config() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "svc", "Test Service");
        graph.add_node(node).unwrap();

        // Use custom config with higher thresholds
        let config = GapAnalysisConfig {
            missing_purpose_score: 0.5, // Higher than default
            missing_owner_score: 0.4,   // Higher than default
            ..Default::default()
        };

        let gaps = analyze_gaps_with_config(&graph, &config);

        assert!(!gaps.is_empty());
        // Score should be higher due to custom config
        assert!(gaps[0].score >= 0.9); // 0.5 + 0.4 = 0.9
    }

    #[test]
    fn test_gap_reason_description() {
        let reason = GapReason::MissingPurpose;
        assert_eq!(reason.description(), "No business purpose documented");

        let reason = GapReason::HighCentrality { edge_count: 10 };
        assert_eq!(reason.description(), "High centrality with 10 connections");

        let reason = GapReason::ImplicitCoupling {
            coupled_services: vec!["A".to_string(), "B".to_string()],
        };
        assert_eq!(reason.description(), "Implicitly coupled with: A, B");
    }

    #[test]
    fn test_empty_graph() {
        let graph = ForgeGraph::new();
        let gaps = analyze_gaps(&graph);
        assert!(gaps.is_empty());
    }

    #[test]
    fn test_shared_queue_without_owner() {
        let mut graph = ForgeGraph::new();

        // Create a queue
        let queue = create_test_queue("ns", "shared-queue", "Shared Queue");
        graph.add_node(queue).unwrap();

        // Create two services that access it
        let svc_a = create_test_service("ns", "publisher", "Publisher Service");
        let svc_b = create_test_service("ns", "subscriber", "Subscriber Service");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        // Publisher publishes, subscriber subscribes
        let pub_edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "publisher").unwrap(),
            NodeId::new(NodeType::Queue, "ns", "shared-queue").unwrap(),
            EdgeType::Publishes,
        )
        .unwrap();
        let sub_edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "subscriber").unwrap(),
            NodeId::new(NodeType::Queue, "ns", "shared-queue").unwrap(),
            EdgeType::Subscribes,
        )
        .unwrap();
        graph.add_edge(pub_edge).unwrap();
        graph.add_edge(sub_edge).unwrap();

        let gaps = analyze_gaps(&graph);

        // Queue should have shared resource without owner reason
        let queue_id = NodeId::new(NodeType::Queue, "ns", "shared-queue").unwrap();
        let queue_gap = gaps.iter().find(|g| g.node_id == queue_id);

        assert!(queue_gap.is_some());
        let has_shared_reason = queue_gap.unwrap().reasons.iter().any(|r| {
            matches!(r, GapReason::SharedResourceWithoutOwner { accessor_services } if accessor_services.len() == 2)
        });
        assert!(has_shared_reason);
    }

    #[test]
    fn test_score_capped_at_one() {
        let mut gap_score = ContextGapScore::new(
            NodeId::new(NodeType::Service, "ns", "test").unwrap(),
        );

        // Add reasons that would sum to more than 1.0
        gap_score.add_reason(GapReason::MissingPurpose, 0.5);
        gap_score.add_reason(GapReason::MissingOwner, 0.5);
        gap_score.add_reason(
            GapReason::HighCentrality { edge_count: 10 },
            0.5,
        );

        // Score should be capped at 1.0
        assert_eq!(gap_score.score, 1.0);
    }
}
