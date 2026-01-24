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

// ============================================================================
// Question Generation
// ============================================================================

/// A question to ask during the business context interview.
///
/// Questions are generated based on gap analysis results and are designed
/// to elicit useful business context annotations for nodes in the graph.
#[derive(Debug, Clone)]
pub struct InterviewQuestion {
    /// Node this question is about
    pub node_id: NodeId,

    /// The question text
    pub question: String,

    /// What type of annotation this fills
    pub annotation_type: AnnotationType,

    /// Priority (1-10, higher = more important)
    pub priority: u8,

    /// Context to help answer the question
    pub context: String,
}

impl InterviewQuestion {
    /// Create a new interview question.
    pub fn new(
        node_id: NodeId,
        question: impl Into<String>,
        annotation_type: AnnotationType,
        priority: u8,
        context: impl Into<String>,
    ) -> Self {
        Self {
            node_id,
            question: question.into(),
            annotation_type,
            priority: priority.clamp(1, 10),
            context: context.into(),
        }
    }
}

/// Type of annotation that an interview question fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationType {
    /// Business purpose / what problem does it solve
    Purpose,
    /// Who owns or is responsible for this component
    Owner,
    /// Historical context / why was it built this way
    History,
    /// Known issues, gotchas, operational learnings
    Gotcha,
    /// Free-form additional notes
    Note,
}

impl AnnotationType {
    /// Get a human-readable name for this annotation type.
    pub fn display_name(&self) -> &'static str {
        match self {
            AnnotationType::Purpose => "Purpose",
            AnnotationType::Owner => "Owner",
            AnnotationType::History => "History",
            AnnotationType::Gotcha => "Gotcha",
            AnnotationType::Note => "Note",
        }
    }
}

/// Generate interview questions for a node based on gap analysis.
///
/// This function examines the gap reasons for a node and generates appropriate
/// questions to gather the missing business context.
///
/// # Arguments
/// * `node` - The node to generate questions for
/// * `graph` - The knowledge graph (for context gathering)
/// * `gap` - The gap analysis result for this node
///
/// # Returns
/// A vector of `InterviewQuestion` to ask about this node.
///
/// # Example
///
/// ```rust,ignore
/// let gaps = analyze_gaps(&graph);
/// for gap in &gaps {
///     if let Some(node) = graph.get_node(&gap.node_id) {
///         let questions = generate_questions(node, &graph, gap);
///         for q in questions {
///             println!("{}", q.question);
///         }
///     }
/// }
/// ```
pub fn generate_questions(
    node: &Node,
    graph: &ForgeGraph,
    gap: &ContextGapScore,
) -> Vec<InterviewQuestion> {
    let mut questions = vec![];

    for reason in &gap.reasons {
        match reason {
            GapReason::MissingPurpose => {
                questions.push(generate_purpose_question(node, graph));
            }
            GapReason::MissingOwner => {
                questions.push(generate_owner_question(node));
            }
            GapReason::HighCentrality { edge_count } => {
                questions.push(generate_centrality_question(node, *edge_count, graph));
            }
            GapReason::ImplicitCoupling { coupled_services } => {
                questions.push(generate_coupling_question(node, coupled_services, graph));
            }
            GapReason::SharedResourceWithoutOwner { accessor_services } => {
                questions.push(generate_shared_resource_question(
                    node,
                    accessor_services,
                ));
            }
            GapReason::ComplexWithoutGotchas { .. } => {
                questions.push(generate_gotcha_question(node, graph));
            }
        }
    }

    questions
}

/// Generate all questions for a graph based on gap analysis.
///
/// This is a convenience function that runs gap analysis and generates
/// questions for all nodes with gaps, sorted by question priority.
///
/// # Arguments
/// * `graph` - The knowledge graph to analyze
///
/// # Returns
/// A vector of all questions, sorted by priority (highest first).
pub fn generate_all_questions(graph: &ForgeGraph) -> Vec<InterviewQuestion> {
    let gaps = analyze_gaps(graph);
    let mut all_questions: Vec<InterviewQuestion> = gaps
        .iter()
        .filter_map(|gap| graph.get_node(&gap.node_id).map(|node| (node, gap)))
        .flat_map(|(node, gap)| generate_questions(node, graph, gap))
        .collect();

    // Sort by priority (highest first)
    all_questions.sort_by(|a, b| b.priority.cmp(&a.priority));

    all_questions
}

/// Generate a question about a node's business purpose.
fn generate_purpose_question(node: &Node, graph: &ForgeGraph) -> InterviewQuestion {
    // Gather dependency context to help answer the question
    let deps = graph.edges_from(&node.id);
    let dep_names: Vec<_> = deps
        .iter()
        .filter_map(|e| graph.get_node(&e.target).map(|n| n.display_name.clone()))
        .take(5)
        .collect();

    let context = if dep_names.is_empty() {
        format!(
            "'{}' exists in your ecosystem but has no documented dependencies.",
            node.display_name
        )
    } else {
        format!(
            "'{}' depends on: {}",
            node.display_name,
            dep_names.join(", ")
        )
    };

    InterviewQuestion::new(
        node.id.clone(),
        format!(
            "What is the business purpose of '{}'? What problem does it solve or what capability does it provide?",
            node.display_name
        ),
        AnnotationType::Purpose,
        9, // High priority - purpose is fundamental
        context,
    )
}

/// Generate a question about a node's owner.
fn generate_owner_question(node: &Node) -> InterviewQuestion {
    InterviewQuestion::new(
        node.id.clone(),
        format!(
            "Who owns or is responsible for '{}'? (Team name, person, or group)",
            node.display_name
        ),
        AnnotationType::Owner,
        7, // Medium-high priority
        "Ownership helps route questions and on-call responsibilities.".to_string(),
    )
}

/// Generate a question about why a central service is so connected.
fn generate_centrality_question(
    node: &Node,
    edge_count: usize,
    graph: &ForgeGraph,
) -> InterviewQuestion {
    // Find services that call this one
    let callers: Vec<_> = graph
        .edges_to(&node.id)
        .iter()
        .filter(|e| e.edge_type == EdgeType::Calls)
        .filter_map(|e| graph.get_node(&e.source).map(|n| n.display_name.clone()))
        .collect();

    let context = format!(
        "'{}' has {} connections and is called by: {}",
        node.display_name,
        edge_count,
        if callers.is_empty() {
            "no direct callers".to_string()
        } else {
            callers.join(", ")
        }
    );

    InterviewQuestion::new(
        node.id.clone(),
        format!(
            "'{}' appears to be a central service with many dependencies. Why is it so connected? What core capability does it provide?",
            node.display_name
        ),
        AnnotationType::Purpose,
        8, // High priority - central services are important
        context,
    )
}

/// Generate a question about implicit coupling between services.
fn generate_coupling_question(
    node: &Node,
    coupled_services: &[String],
    graph: &ForgeGraph,
) -> InterviewQuestion {
    // Find the shared resources causing the coupling
    let shared_resources: Vec<_> = graph
        .edges_from(&node.id)
        .iter()
        .chain(graph.edges_to(&node.id).iter())
        .filter(|e| e.edge_type == EdgeType::ImplicitlyCoupled)
        .filter_map(|e| e.metadata.reason.clone())
        .collect();

    let context = format!(
        "'{}' is implicitly coupled with {} via shared resources. Reasons: {}",
        node.display_name,
        coupled_services.join(", "),
        if shared_resources.is_empty() {
            "unknown".to_string()
        } else {
            shared_resources.join("; ")
        }
    );

    InterviewQuestion::new(
        node.id.clone(),
        format!(
            "'{}' shares resources with {}. Is this intentional? What coordination, if any, exists between these services?",
            node.display_name,
            coupled_services.join(", ")
        ),
        AnnotationType::Note,
        7, // Medium-high priority - couplings can cause issues
        context,
    )
}

/// Generate a question about shared resource ownership.
fn generate_shared_resource_question(
    node: &Node,
    accessor_services: &[String],
) -> InterviewQuestion {
    InterviewQuestion::new(
        node.id.clone(),
        format!(
            "Resource '{}' is accessed by multiple services ({}). Which service owns this resource and is responsible for its schema?",
            node.display_name,
            accessor_services.join(", ")
        ),
        AnnotationType::Owner,
        8, // High priority - ownership clarity is important
        "Shared resources need clear ownership for schema changes and data governance.".to_string(),
    )
}

/// Generate a question about gotchas for a complex service.
fn generate_gotcha_question(node: &Node, graph: &ForgeGraph) -> InterviewQuestion {
    // Gather some context about the service's complexity
    let outgoing = graph.edges_from(&node.id).len();
    let incoming = graph.edges_to(&node.id).len();

    let context = format!(
        "'{}' has {} outgoing and {} incoming connections, indicating significant complexity.",
        node.display_name, outgoing, incoming
    );

    InterviewQuestion::new(
        node.id.clone(),
        format!(
            "Are there any gotchas, known issues, or operational concerns with '{}'? Things a new team member should know?",
            node.display_name
        ),
        AnnotationType::Gotcha,
        5, // Medium priority - nice to have
        context,
    )
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

    // ========================================================================
    // Question Generation Tests
    // ========================================================================

    #[test]
    fn test_generate_purpose_question() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "user-api", "User API");
        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);
        let gap = &gaps[0];
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Should have a purpose question
        let purpose_q = questions
            .iter()
            .find(|q| q.annotation_type == AnnotationType::Purpose);
        assert!(purpose_q.is_some());

        let q = purpose_q.unwrap();
        assert!(q.question.contains("User API"));
        assert!(q.question.contains("business purpose"));
        assert_eq!(q.priority, 9);
    }

    #[test]
    fn test_generate_owner_question() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "auth-service", "Auth Service");
        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);
        let gap = &gaps[0];
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Should have an owner question
        let owner_q = questions
            .iter()
            .find(|q| q.annotation_type == AnnotationType::Owner);
        assert!(owner_q.is_some());

        let q = owner_q.unwrap();
        assert!(q.question.contains("Auth Service"));
        assert!(q.question.contains("owns") || q.question.contains("responsible"));
        assert_eq!(q.priority, 7);
    }

    #[test]
    fn test_generate_centrality_question() {
        let mut graph = ForgeGraph::new();

        // Create a central service with many connections
        let central = create_test_service("ns", "gateway", "API Gateway");
        graph.add_node(central).unwrap();

        // Create services that connect to it
        for i in 0..6 {
            let svc = create_test_service("ns", &format!("svc-{}", i), &format!("Service {}", i));
            graph.add_node(svc).unwrap();

            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", &format!("svc-{}", i)).unwrap(),
                NodeId::new(NodeType::Service, "ns", "gateway").unwrap(),
                EdgeType::Calls,
            )
            .unwrap();
            graph.add_edge(edge).unwrap();
        }

        let gaps = analyze_gaps(&graph);
        let gateway_id = NodeId::new(NodeType::Service, "ns", "gateway").unwrap();
        let gap = gaps.iter().find(|g| g.node_id == gateway_id).unwrap();
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Should have centrality-related question about purpose
        let centrality_q = questions
            .iter()
            .find(|q| q.question.contains("central service"));
        assert!(centrality_q.is_some());
        assert!(centrality_q.unwrap().context.contains("6 connections"));
    }

    #[test]
    fn test_generate_coupling_question() {
        let mut graph = ForgeGraph::new();

        let svc_a = create_test_service("ns", "order-svc", "Order Service");
        let svc_b = create_test_service("ns", "inventory-svc", "Inventory Service");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        // Add implicit coupling edge
        let edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "order-svc").unwrap(),
            NodeId::new(NodeType::Service, "ns", "inventory-svc").unwrap(),
            EdgeType::ImplicitlyCoupled,
        )
        .unwrap();
        graph.add_edge(edge).unwrap();

        let gaps = analyze_gaps(&graph);
        let order_id = NodeId::new(NodeType::Service, "ns", "order-svc").unwrap();
        let gap = gaps.iter().find(|g| g.node_id == order_id).unwrap();
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Should have coupling question
        let coupling_q = questions
            .iter()
            .find(|q| q.question.contains("shares resources"));
        assert!(coupling_q.is_some());

        let q = coupling_q.unwrap();
        assert_eq!(q.annotation_type, AnnotationType::Note);
        assert!(q.question.contains("Inventory Service"));
    }

    #[test]
    fn test_generate_shared_resource_question() {
        let mut graph = ForgeGraph::new();

        let db = create_test_database("ns", "users-db", "Users Database");
        graph.add_node(db).unwrap();

        let svc_a = create_test_service("ns", "user-svc", "User Service");
        let svc_b = create_test_service("ns", "admin-svc", "Admin Service");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        // Both services access the database
        let edge_a = Edge::new(
            NodeId::new(NodeType::Service, "ns", "user-svc").unwrap(),
            NodeId::new(NodeType::Database, "ns", "users-db").unwrap(),
            EdgeType::Writes,
        )
        .unwrap();
        let edge_b = Edge::new(
            NodeId::new(NodeType::Service, "ns", "admin-svc").unwrap(),
            NodeId::new(NodeType::Database, "ns", "users-db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();
        graph.add_edge(edge_a).unwrap();
        graph.add_edge(edge_b).unwrap();

        let gaps = analyze_gaps(&graph);
        let db_id = NodeId::new(NodeType::Database, "ns", "users-db").unwrap();
        let gap = gaps.iter().find(|g| g.node_id == db_id).unwrap();
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Should have shared resource question
        let shared_q = questions
            .iter()
            .find(|q| q.question.contains("accessed by multiple services"));
        assert!(shared_q.is_some());

        let q = shared_q.unwrap();
        assert_eq!(q.annotation_type, AnnotationType::Owner);
        assert!(q.question.contains("User Service") || q.question.contains("Admin Service"));
    }

    #[test]
    fn test_generate_gotcha_question() {
        let mut graph = ForgeGraph::new();

        // Create a complex service with multiple connections
        let complex = create_test_service("ns", "payment-svc", "Payment Service");
        graph.add_node(complex).unwrap();

        // Add several connections
        for i in 0..4 {
            let db = create_test_database("ns", &format!("db-{}", i), &format!("Database {}", i));
            graph.add_node(db).unwrap();

            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", "payment-svc").unwrap(),
                NodeId::new(NodeType::Database, "ns", &format!("db-{}", i)).unwrap(),
                EdgeType::Reads,
            )
            .unwrap();
            graph.add_edge(edge).unwrap();
        }

        let gaps = analyze_gaps(&graph);
        let payment_id = NodeId::new(NodeType::Service, "ns", "payment-svc").unwrap();
        let gap = gaps.iter().find(|g| g.node_id == payment_id).unwrap();
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Should have gotcha question
        let gotcha_q = questions
            .iter()
            .find(|q| q.annotation_type == AnnotationType::Gotcha);
        assert!(gotcha_q.is_some());

        let q = gotcha_q.unwrap();
        assert!(q.question.contains("gotchas") || q.question.contains("known issues"));
        assert_eq!(q.priority, 5);
    }

    #[test]
    fn test_generate_all_questions() {
        let mut graph = ForgeGraph::new();

        // Create several services with different gap levels
        let svc_a = create_test_service("ns", "svc-a", "Service A");
        let svc_b = create_test_service("ns", "svc-b", "Service B");
        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        let all_questions = generate_all_questions(&graph);

        // Should have questions for both services
        assert!(all_questions.len() >= 2);

        // Questions should be sorted by priority (highest first)
        for i in 1..all_questions.len() {
            assert!(all_questions[i - 1].priority >= all_questions[i].priority);
        }
    }

    #[test]
    fn test_no_questions_when_fully_documented() {
        let mut graph = ForgeGraph::new();

        let mut node = create_test_service("ns", "documented", "Documented Service");
        node.business_context = Some(BusinessContext {
            purpose: Some("Handles user authentication".to_string()),
            owner: Some("Auth Team".to_string()),
            history: Some("Built in 2020".to_string()),
            gotchas: vec!["Rate limited to 100 req/s".to_string()],
            notes: Default::default(),
        });
        graph.add_node(node).unwrap();

        let all_questions = generate_all_questions(&graph);

        // Should have no questions (or very few from other reasons)
        // Service is fully documented, so no MissingPurpose, MissingOwner, or ComplexWithoutGotchas
        let doc_id = NodeId::new(NodeType::Service, "ns", "documented").unwrap();
        let doc_questions: Vec<_> = all_questions
            .iter()
            .filter(|q| q.node_id == doc_id)
            .collect();

        // Should not have purpose or owner questions
        assert!(!doc_questions
            .iter()
            .any(|q| q.annotation_type == AnnotationType::Purpose
                && q.question.contains("business purpose")));
        assert!(!doc_questions
            .iter()
            .any(|q| q.annotation_type == AnnotationType::Owner
                && q.question.contains("owns")));
    }

    #[test]
    fn test_annotation_type_display_name() {
        assert_eq!(AnnotationType::Purpose.display_name(), "Purpose");
        assert_eq!(AnnotationType::Owner.display_name(), "Owner");
        assert_eq!(AnnotationType::History.display_name(), "History");
        assert_eq!(AnnotationType::Gotcha.display_name(), "Gotcha");
        assert_eq!(AnnotationType::Note.display_name(), "Note");
    }

    #[test]
    fn test_interview_question_priority_clamped() {
        let node_id = NodeId::new(NodeType::Service, "ns", "test").unwrap();

        // Priority should be clamped to 1-10
        let q1 = InterviewQuestion::new(
            node_id.clone(),
            "Test question",
            AnnotationType::Purpose,
            0, // Below minimum
            "context",
        );
        assert_eq!(q1.priority, 1);

        let q2 = InterviewQuestion::new(
            node_id.clone(),
            "Test question",
            AnnotationType::Purpose,
            15, // Above maximum
            "context",
        );
        assert_eq!(q2.priority, 10);

        let q3 = InterviewQuestion::new(
            node_id,
            "Test question",
            AnnotationType::Purpose,
            5, // Within range
            "context",
        );
        assert_eq!(q3.priority, 5);
    }

    #[test]
    fn test_purpose_question_includes_dependency_context() {
        let mut graph = ForgeGraph::new();

        let svc = create_test_service("ns", "api-gateway", "API Gateway");
        graph.add_node(svc).unwrap();

        let db = create_test_database("ns", "users-db", "Users Database");
        graph.add_node(db).unwrap();

        // Add dependency edge
        let edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "api-gateway").unwrap(),
            NodeId::new(NodeType::Database, "ns", "users-db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();
        graph.add_edge(edge).unwrap();

        let gaps = analyze_gaps(&graph);
        let gateway_id = NodeId::new(NodeType::Service, "ns", "api-gateway").unwrap();
        let gap = gaps.iter().find(|g| g.node_id == gateway_id).unwrap();
        let node = graph.get_node(&gap.node_id).unwrap();
        let questions = generate_questions(node, &graph, gap);

        // Purpose question should include dependency context
        let purpose_q = questions
            .iter()
            .find(|q| q.annotation_type == AnnotationType::Purpose
                && q.question.contains("business purpose"));
        assert!(purpose_q.is_some());
        assert!(purpose_q.unwrap().context.contains("Users Database"));
    }
}
