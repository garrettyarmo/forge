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
                format!("Implicitly coupled with: {}", coupled_services.join(", "))
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
    let mut result: Vec<_> = scores.into_values().filter(|s| s.score > 0.0).collect();
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
        let centrality_score = config.max_centrality_score * (total_edges as f64 / 10.0).min(1.0);
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
                questions.push(generate_shared_resource_question(node, accessor_services));
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

// ============================================================================
// Interview Flow (M6-T8)
// ============================================================================

use crate::provider::{LLMProvider, LLMResult};
use std::collections::HashMap as StdHashMap;
use std::io::{self, Write};

/// Update to apply to a node's business context annotation.
#[derive(Debug, Clone)]
pub struct AnnotationUpdate {
    /// Type of annotation being updated.
    pub annotation_type: AnnotationType,
    /// Value to set.
    pub value: String,
}

/// Interactive interview session state.
///
/// Manages the flow of asking questions about nodes in the graph
/// and collecting answers to update business context annotations.
pub struct InterviewSession {
    /// Questions to ask during this session
    questions: Vec<InterviewQuestion>,

    /// Current question index
    current_index: usize,

    /// Collected answers keyed by node ID
    answers: StdHashMap<NodeId, Vec<AnnotationUpdate>>,

    /// LLM provider for suggestions (optional)
    provider: Option<Box<dyn LLMProvider>>,
}

impl InterviewSession {
    /// Create a new interview session from a graph.
    ///
    /// Analyzes the graph for context gaps and generates questions
    /// sorted by priority.
    pub fn new(graph: &ForgeGraph) -> Self {
        let questions = generate_all_questions(graph);

        Self {
            questions,
            current_index: 0,
            answers: StdHashMap::new(),
            provider: None,
        }
    }

    /// Create an interview session with LLM support for answer suggestions.
    pub fn with_provider(graph: &ForgeGraph, provider: Box<dyn LLMProvider>) -> Self {
        let questions = generate_all_questions(graph);

        Self {
            questions,
            current_index: 0,
            answers: StdHashMap::new(),
            provider: Some(provider),
        }
    }

    /// Total number of questions in this session.
    pub fn total_questions(&self) -> usize {
        self.questions.len()
    }

    /// Current question number (1-based for display).
    pub fn current_question_number(&self) -> usize {
        self.current_index + 1
    }

    /// Check if the interview is complete.
    pub fn is_complete(&self) -> bool {
        self.current_index >= self.questions.len()
    }

    /// Get the current question.
    pub fn current_question(&self) -> Option<&InterviewQuestion> {
        self.questions.get(self.current_index)
    }

    /// Submit an answer for the current question.
    pub fn submit_answer(&mut self, answer: &str) {
        if let Some(question) = self.questions.get(self.current_index) {
            let update = AnnotationUpdate {
                annotation_type: question.annotation_type,
                value: answer.to_string(),
            };

            self.answers
                .entry(question.node_id.clone())
                .or_default()
                .push(update);
        }

        self.current_index += 1;
    }

    /// Skip the current question without answering.
    pub fn skip(&mut self) {
        self.current_index += 1;
    }

    /// Get the number of answers collected so far.
    pub fn answer_count(&self) -> usize {
        self.answers.values().map(|v| v.len()).sum()
    }

    /// Get the collected answers.
    pub fn answers(&self) -> &StdHashMap<NodeId, Vec<AnnotationUpdate>> {
        &self.answers
    }

    /// Apply collected answers to the graph.
    ///
    /// Updates business context annotations on nodes based on
    /// the answers collected during the interview.
    pub fn apply_to_graph(&self, graph: &mut ForgeGraph) {
        use forge_graph::BusinessContext;

        for (node_id, updates) in &self.answers {
            if let Some(node) = graph.get_node_mut(node_id) {
                let bc = node
                    .business_context
                    .get_or_insert_with(BusinessContext::default);

                for update in updates {
                    match update.annotation_type {
                        AnnotationType::Purpose => {
                            bc.purpose = Some(update.value.clone());
                        }
                        AnnotationType::Owner => {
                            bc.owner = Some(update.value.clone());
                        }
                        AnnotationType::History => {
                            bc.history = Some(update.value.clone());
                        }
                        AnnotationType::Gotcha => {
                            // Avoid duplicate gotchas
                            if !bc.gotchas.contains(&update.value) {
                                bc.gotchas.push(update.value.clone());
                            }
                        }
                        AnnotationType::Note => {
                            // Generate unique key for notes
                            let key = format!("note_{}", bc.notes.len() + 1);
                            bc.notes.insert(key, update.value.clone());
                        }
                    }
                }
            }
        }
    }

    /// Generate an LLM-assisted answer suggestion for a question.
    ///
    /// Returns `None` if no LLM provider is configured.
    pub async fn suggest_answer(&self, question: &InterviewQuestion) -> Option<LLMResult<String>> {
        let provider = self.provider.as_ref()?;

        let system = r#"You are helping document a software ecosystem. Based on the context provided, suggest a concise answer to the question. If you cannot determine the answer from context alone, say "Unable to determine from available context - please provide this information manually."

Keep answers brief (1-3 sentences) and focused."#;

        let user = format!(
            "Context: {}\n\nQuestion: {}",
            question.context, question.question
        );

        Some(provider.prompt(system, &user).await)
    }

    /// Check if LLM suggestions are available.
    pub fn has_llm_support(&self) -> bool {
        self.provider.is_some()
    }
}

/// Result of an interactive interview.
#[derive(Debug)]
pub struct InterviewResult {
    /// Number of questions asked.
    pub questions_asked: usize,
    /// Number of questions answered.
    pub questions_answered: usize,
    /// Number of questions skipped.
    pub questions_skipped: usize,
    /// Whether the interview was completed or quit early.
    pub completed: bool,
}

/// Run an interactive terminal interview.
///
/// This function provides a terminal-based UI for conducting business
/// context interviews. It displays questions one at a time and collects
/// answers from the user.
///
/// # Arguments
/// * `graph` - The knowledge graph to update with collected answers
/// * `provider` - Optional LLM provider for answer suggestions
///
/// # Returns
/// An `InterviewResult` summarizing the interview session.
///
/// # Commands
/// During the interview, users can:
/// - Type an answer directly to submit it
/// - `s` or `suggest` - Get an LLM-suggested answer (if provider available)
/// - `k` or `skip` - Skip the current question
/// - `q` or `quit` - End the interview early
///
/// # Example
///
/// ```rust,ignore
/// use forge_llm::{LLMConfig, create_provider, run_interactive_interview};
/// use forge_graph::ForgeGraph;
///
/// let mut graph = ForgeGraph::load_from_file("graph.json")?;
/// let provider = create_provider(&LLMConfig::new("claude"))?;
///
/// let result = run_interactive_interview(&mut graph, Some(provider)).await?;
/// println!("Answered {} questions", result.questions_answered);
///
/// graph.save_to_file("graph.json")?;
/// ```
pub async fn run_interactive_interview(
    graph: &mut ForgeGraph,
    provider: Option<Box<dyn LLMProvider>>,
) -> Result<InterviewResult, InterviewError> {
    let mut session = if let Some(p) = provider {
        InterviewSession::with_provider(graph, p)
    } else {
        InterviewSession::new(graph)
    };

    if session.total_questions() == 0 {
        println!("No context gaps detected - graph is well-documented!");
        return Ok(InterviewResult {
            questions_asked: 0,
            questions_answered: 0,
            questions_skipped: 0,
            completed: true,
        });
    }

    println!();
    println!("Business Context Interview");
    println!("==========================");
    println!(
        "Found {} questions to help document your ecosystem.",
        session.total_questions()
    );
    println!();

    if session.has_llm_support() {
        println!("Commands: [s]uggest (LLM), [k]skip, [q]uit, or type answer directly");
    } else {
        println!("Commands: [k]skip, [q]uit, or type answer directly");
    }
    println!();

    let mut questions_answered = 0;
    let mut questions_skipped = 0;
    let mut completed = true;

    while !session.is_complete() {
        let question = session.current_question().unwrap();

        println!(
            "Question {}/{}",
            session.current_question_number(),
            session.total_questions()
        );
        println!("About: {}", question.node_id.name());
        println!("Type: {}", question.annotation_type.display_name());
        println!();
        println!("Context: {}", question.context);
        println!();
        println!("{}", question.question);
        println!();

        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        match input.to_lowercase().as_str() {
            "s" | "suggest" => {
                if session.has_llm_support() {
                    println!("Getting suggestion from LLM...");
                    match session.suggest_answer(question).await {
                        Some(Ok(suggestion)) => {
                            println!();
                            println!("Suggested answer: {}", suggestion);
                            println!();
                            println!("Accept? [y]es, [n]o, [e]dit");
                            print!("> ");
                            io::stdout().flush()?;

                            let mut response = String::new();
                            io::stdin().read_line(&mut response)?;
                            let response = response.trim().to_lowercase();

                            match response.as_str() {
                                "y" | "yes" => {
                                    session.submit_answer(&suggestion);
                                    questions_answered += 1;
                                    println!("Answer accepted.");
                                }
                                "e" | "edit" => {
                                    print!("Your edited answer: ");
                                    io::stdout().flush()?;
                                    let mut edited = String::new();
                                    io::stdin().read_line(&mut edited)?;
                                    let edited = edited.trim();
                                    if !edited.is_empty() {
                                        session.submit_answer(edited);
                                        questions_answered += 1;
                                        println!("Answer saved.");
                                    } else {
                                        session.skip();
                                        questions_skipped += 1;
                                        println!("Skipped.");
                                    }
                                }
                                _ => {
                                    session.skip();
                                    questions_skipped += 1;
                                    println!("Skipped.");
                                }
                            }
                        }
                        Some(Err(e)) => {
                            println!("LLM error: {}. Please answer manually.", e);
                        }
                        None => {
                            println!("LLM not available.");
                        }
                    }
                } else {
                    println!("LLM suggestions not available. Please type your answer.");
                }
            }
            "k" | "skip" => {
                session.skip();
                questions_skipped += 1;
                println!("Skipped.");
            }
            "q" | "quit" => {
                println!("Interview ended early. Saving collected answers...");
                completed = false;
                break;
            }
            "" => {
                println!("Please enter an answer, or use [s]uggest/[k]skip/[q]uit.");
            }
            _ => {
                // Treat as direct answer
                session.submit_answer(input);
                questions_answered += 1;
                println!("Answer saved.");
            }
        }

        println!();
    }

    // Apply answers to graph
    session.apply_to_graph(graph);

    if completed {
        println!("Interview complete!");
    }
    println!(
        "Answered {} questions, skipped {}.",
        questions_answered, questions_skipped
    );

    Ok(InterviewResult {
        questions_asked: session.current_index,
        questions_answered,
        questions_skipped,
        completed,
    })
}

/// Error type for interview operations.
#[derive(Debug, thiserror::Error)]
pub enum InterviewError {
    /// IO error during terminal interaction.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

// ============================================================================
// Annotation Persistence (M6-T9)
// ============================================================================

/// Merge business context from an existing graph into a new graph.
///
/// This function preserves business context annotations when re-surveying.
/// Nodes in the new graph will have their business context updated with
/// annotations from matching nodes in the existing graph.
///
/// # Arguments
/// * `new_graph` - The newly surveyed graph to update
/// * `existing_graph` - The existing graph with business context annotations
pub fn merge_business_context(new_graph: &mut ForgeGraph, existing_graph: &ForgeGraph) {
    for existing_node in existing_graph.nodes() {
        if let Some(existing_bc) = &existing_node.business_context {
            if let Some(new_node) = new_graph.get_node_mut(&existing_node.id) {
                let bc = new_node
                    .business_context
                    .get_or_insert_with(forge_graph::BusinessContext::default);
                bc.merge(existing_bc);
            }
        }
    }
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
        assert!(
            gaps[0]
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::MissingPurpose))
        );
    }

    #[test]
    fn test_detect_missing_owner() {
        let mut graph = ForgeGraph::new();

        let node = create_test_service("ns", "svc", "Test Service");
        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);

        assert!(!gaps.is_empty());
        assert!(
            gaps[0]
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::MissingOwner))
        );
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
            assert!(
                !gaps[0]
                    .reasons
                    .iter()
                    .any(|r| matches!(r, GapReason::MissingPurpose))
            );
            assert!(
                !gaps[0]
                    .reasons
                    .iter()
                    .any(|r| matches!(r, GapReason::MissingOwner))
            );
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
        assert!(
            central_gap
                .unwrap()
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::HighCentrality { edge_count: 6 }))
        );
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
            assert!(
                !gap.reasons
                    .iter()
                    .any(|r| matches!(r, GapReason::SharedResourceWithoutOwner { .. }))
            );
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
        assert!(
            complex_gap
                .unwrap()
                .reasons
                .iter()
                .any(|r| matches!(r, GapReason::ComplexWithoutGotchas { .. }))
        );
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
            assert!(
                !gap.reasons
                    .iter()
                    .any(|r| matches!(r, GapReason::ComplexWithoutGotchas { .. }))
            );
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
        let mut gap_score =
            ContextGapScore::new(NodeId::new(NodeType::Service, "ns", "test").unwrap());

        // Add reasons that would sum to more than 1.0
        gap_score.add_reason(GapReason::MissingPurpose, 0.5);
        gap_score.add_reason(GapReason::MissingOwner, 0.5);
        gap_score.add_reason(GapReason::HighCentrality { edge_count: 10 }, 0.5);

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
        assert!(
            !doc_questions
                .iter()
                .any(|q| q.annotation_type == AnnotationType::Purpose
                    && q.question.contains("business purpose"))
        );
        assert!(
            !doc_questions
                .iter()
                .any(|q| q.annotation_type == AnnotationType::Owner && q.question.contains("owns"))
        );
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
        let purpose_q = questions.iter().find(|q| {
            q.annotation_type == AnnotationType::Purpose && q.question.contains("business purpose")
        });
        assert!(purpose_q.is_some());
        assert!(purpose_q.unwrap().context.contains("Users Database"));
    }

    // ========================================================================
    // Interview Session Tests (M6-T8)
    // ========================================================================

    #[test]
    fn test_interview_session_creation() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "test-svc", "Test Service");
        graph.add_node(node).unwrap();

        let session = InterviewSession::new(&graph);

        assert!(session.total_questions() > 0);
        assert_eq!(session.current_question_number(), 1);
        assert!(!session.is_complete());
        assert!(!session.has_llm_support());
    }

    #[test]
    fn test_interview_session_empty_graph() {
        let graph = ForgeGraph::new();
        let session = InterviewSession::new(&graph);

        assert_eq!(session.total_questions(), 0);
        assert!(session.is_complete());
    }

    #[test]
    fn test_interview_session_submit_answer() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "test-svc", "Test Service");
        graph.add_node(node).unwrap();

        let mut session = InterviewSession::new(&graph);
        let initial_index = session.current_question_number();

        session.submit_answer("This service handles authentication");

        assert_eq!(session.current_question_number(), initial_index + 1);
        assert_eq!(session.answer_count(), 1);
    }

    #[test]
    fn test_interview_session_skip() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "test-svc", "Test Service");
        graph.add_node(node).unwrap();

        let mut session = InterviewSession::new(&graph);
        let initial_index = session.current_question_number();

        session.skip();

        assert_eq!(session.current_question_number(), initial_index + 1);
        assert_eq!(session.answer_count(), 0);
    }

    #[test]
    fn test_interview_session_apply_to_graph() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "test-svc", "Test Service");
        graph.add_node(node).unwrap();

        let mut session = InterviewSession::new(&graph);

        // Answer the purpose question
        if let Some(q) = session.current_question() {
            if q.annotation_type == AnnotationType::Purpose {
                session.submit_answer("Handles user authentication");
            } else {
                session.skip();
            }
        }

        // Answer the owner question
        while !session.is_complete() {
            if let Some(q) = session.current_question() {
                if q.annotation_type == AnnotationType::Owner {
                    session.submit_answer("Auth Team");
                    break;
                } else {
                    session.skip();
                }
            }
        }

        // Apply to graph
        session.apply_to_graph(&mut graph);

        let node_id = NodeId::new(NodeType::Service, "ns", "test-svc").unwrap();
        let updated_node = graph.get_node(&node_id).unwrap();

        assert!(updated_node.business_context.is_some());
        let bc = updated_node.business_context.as_ref().unwrap();

        // At least one of these should be set
        let has_annotations = bc.purpose.is_some() || bc.owner.is_some();
        assert!(has_annotations);
    }

    #[test]
    fn test_interview_session_multiple_answers_same_node() {
        let mut graph = ForgeGraph::new();
        let node = create_test_service("ns", "test-svc", "Test Service");
        graph.add_node(node).unwrap();

        let mut session = InterviewSession::new(&graph);

        // Answer multiple questions
        while !session.is_complete() {
            if let Some(q) = session.current_question() {
                match q.annotation_type {
                    AnnotationType::Purpose => {
                        session.submit_answer("Handles authentication");
                    }
                    AnnotationType::Owner => {
                        session.submit_answer("Auth Team");
                    }
                    AnnotationType::Gotcha => {
                        session.submit_answer("Rate limited to 100 req/s");
                    }
                    _ => session.skip(),
                }
            }
        }

        session.apply_to_graph(&mut graph);

        let node_id = NodeId::new(NodeType::Service, "ns", "test-svc").unwrap();
        let updated_node = graph.get_node(&node_id).unwrap();
        let bc = updated_node.business_context.as_ref().unwrap();

        // Verify annotations were applied
        if bc.purpose.is_some() {
            assert_eq!(bc.purpose.as_ref().unwrap(), "Handles authentication");
        }
        if bc.owner.is_some() {
            assert_eq!(bc.owner.as_ref().unwrap(), "Auth Team");
        }
    }

    #[test]
    fn test_annotation_update() {
        let update = AnnotationUpdate {
            annotation_type: AnnotationType::Purpose,
            value: "Test purpose".to_string(),
        };

        assert_eq!(update.annotation_type, AnnotationType::Purpose);
        assert_eq!(update.value, "Test purpose");
    }

    // ========================================================================
    // Annotation Persistence Tests (M6-T9)
    // ========================================================================

    #[test]
    fn test_business_context_merge_preserves_existing() {
        let mut bc1 = BusinessContext {
            purpose: Some("Original purpose".to_string()),
            owner: None,
            history: None,
            gotchas: vec!["Gotcha 1".to_string()],
            notes: Default::default(),
        };

        let bc2 = BusinessContext {
            purpose: Some("New purpose".to_string()), // Should NOT overwrite
            owner: Some("New Team".to_string()),      // Should be added
            history: Some("History info".to_string()), // Should be added
            gotchas: vec!["Gotcha 2".to_string()],    // Should be merged
            notes: Default::default(),
        };

        bc1.merge(&bc2);

        // Original purpose should be preserved
        assert_eq!(bc1.purpose, Some("Original purpose".to_string()));

        // New owner should be added
        assert_eq!(bc1.owner, Some("New Team".to_string()));

        // History should be added
        assert_eq!(bc1.history, Some("History info".to_string()));

        // Gotchas should be merged (both should exist)
        assert!(bc1.gotchas.contains(&"Gotcha 1".to_string()));
        assert!(bc1.gotchas.contains(&"Gotcha 2".to_string()));
    }

    #[test]
    fn test_business_context_merge_deduplicates_gotchas() {
        let mut bc1 = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec!["Same gotcha".to_string()],
            notes: Default::default(),
        };

        let bc2 = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec!["Same gotcha".to_string(), "Different gotcha".to_string()],
            notes: Default::default(),
        };

        bc1.merge(&bc2);

        // Should have exactly 2 gotchas (no duplicate)
        assert_eq!(bc1.gotchas.len(), 2);
        assert!(bc1.gotchas.contains(&"Same gotcha".to_string()));
        assert!(bc1.gotchas.contains(&"Different gotcha".to_string()));
    }

    #[test]
    fn test_business_context_merge_notes() {
        let mut bc1 = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec![],
            notes: [("key1".to_string(), "value1".to_string())]
                .into_iter()
                .collect(),
        };

        let bc2 = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec![],
            notes: [
                ("key1".to_string(), "new_value1".to_string()), // Should NOT overwrite
                ("key2".to_string(), "value2".to_string()),     // Should be added
            ]
            .into_iter()
            .collect(),
        };

        bc1.merge(&bc2);

        // Original note should be preserved
        assert_eq!(bc1.notes.get("key1"), Some(&"value1".to_string()));

        // New note should be added
        assert_eq!(bc1.notes.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_merge_business_context_across_graphs() {
        // Create existing graph with annotations
        let mut existing_graph = ForgeGraph::new();
        let mut existing_node = create_test_service("ns", "test-svc", "Test Service");
        existing_node.business_context = Some(BusinessContext {
            purpose: Some("Original purpose".to_string()),
            owner: Some("Original Team".to_string()),
            history: None,
            gotchas: vec!["Original gotcha".to_string()],
            notes: Default::default(),
        });
        existing_graph.add_node(existing_node).unwrap();

        // Create new graph from re-survey (no annotations)
        let mut new_graph = ForgeGraph::new();
        let new_node = create_test_service("ns", "test-svc", "Test Service");
        new_graph.add_node(new_node).unwrap();

        // Merge annotations from existing to new
        merge_business_context(&mut new_graph, &existing_graph);

        // Verify annotations were preserved
        let node_id = NodeId::new(NodeType::Service, "ns", "test-svc").unwrap();
        let merged_node = new_graph.get_node(&node_id).unwrap();

        assert!(merged_node.business_context.is_some());
        let bc = merged_node.business_context.as_ref().unwrap();
        assert_eq!(bc.purpose, Some("Original purpose".to_string()));
        assert_eq!(bc.owner, Some("Original Team".to_string()));
        assert!(bc.gotchas.contains(&"Original gotcha".to_string()));
    }

    #[test]
    fn test_merge_business_context_only_matching_nodes() {
        // Create existing graph with annotations
        let mut existing_graph = ForgeGraph::new();
        let mut existing_node = create_test_service("ns", "svc-a", "Service A");
        existing_node.business_context = Some(BusinessContext {
            purpose: Some("Purpose A".to_string()),
            owner: None,
            history: None,
            gotchas: vec![],
            notes: Default::default(),
        });
        existing_graph.add_node(existing_node).unwrap();

        // Create new graph with different node
        let mut new_graph = ForgeGraph::new();
        let new_node = create_test_service("ns", "svc-b", "Service B");
        new_graph.add_node(new_node).unwrap();

        // Merge - should not affect new_graph since nodes don't match
        merge_business_context(&mut new_graph, &existing_graph);

        let node_id = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();
        let node = new_graph.get_node(&node_id).unwrap();

        // Should have no business context (node IDs don't match)
        assert!(node.business_context.is_none());
    }
}
