# Milestone 6: Business Context Specification

> **Spec Version**: 1.0
> **Status**: Draft
> **Implements**: IMPLEMENTATION_PLAN.md § Milestone 6
> **Depends On**: [M5 Serialization](./m5-serialization.md)

---

## 1. Overview

### 1.1 Purpose

Implement LLM-assisted business context interviews to capture the "why" behind technical structures. Code analysis reveals **what** exists; interviews capture **why** it exists, **who** owns it, and **what** business processes it supports.

### 1.2 Architecture: Coding Agent CLI Adapters

Forge integrates with LLMs by **shelling out to coding agent CLIs** rather than making direct API calls:

```
┌─────────────────────────────────────────────────────────────────┐
│                        Forge Interview                           │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    LLMProvider Trait                         ││
│  │     async fn prompt(&self, system: &str, user: &str)        ││
│  └─────────────────────────────────────────────────────────────┘│
│           │                    │                    │            │
│           ▼                    ▼                    ▼            │
│  ┌─────────────┐      ┌─────────────┐      ┌─────────────┐     │
│  │   Claude    │      │   Gemini    │      │   Codex     │     │
│  │   Adapter   │      │   Adapter   │      │   Adapter   │     │
│  └──────┬──────┘      └──────┬──────┘      └──────┬──────┘     │
│         │                    │                    │              │
│         ▼                    ▼                    ▼              │
│    subprocess            subprocess            subprocess        │
│    `claude`              `gemini`              `codex`          │
└─────────────────────────────────────────────────────────────────┘
```

**Benefits**:
- Leverages user's existing CLI authentication
- No API keys stored in forge.yaml
- Provider-agnostic design
- Works with any CLI that accepts stdin/stdout

### 1.3 Success Criteria

1. `forge survey --business-context` launches interactive interview
2. Questions are contextual and based on graph structure
3. Annotations persist across survey re-runs
4. Can switch LLM providers via forge.yaml config
5. Works gracefully when LLM CLI is unavailable (skip with warning)

---

## 2. LLM Provider Trait

### 2.1 Trait Definition

```rust
// forge-llm/src/provider.rs

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LLMError {
    #[error("Failed to spawn process '{cmd}': {message}")]
    ProcessFailed { cmd: String, message: String },

    #[error("Process exited with code {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },

    #[error("Invalid output from LLM: {0}")]
    InvalidOutput(String),

    #[error("LLM CLI not found: {0}. Is it installed and in your PATH?")]
    CliNotFound(String),

    #[error("Timeout waiting for LLM response after {0} seconds")]
    Timeout(u64),

    #[error("Provider not configured: {0}")]
    NotConfigured(String),
}

/// Result type for LLM operations
pub type LLMResult<T> = Result<T, LLMError>;

/// Trait for LLM providers
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Get the provider name
    fn name(&self) -> &str;

    /// Check if the CLI is available
    async fn is_available(&self) -> bool;

    /// Send a prompt and get a response
    ///
    /// # Arguments
    /// * `system` - System prompt setting context
    /// * `user` - User message/question
    ///
    /// # Returns
    /// The LLM's response text
    async fn prompt(&self, system: &str, user: &str) -> LLMResult<String>;

    /// Send a prompt with conversation history
    async fn prompt_with_history(
        &self,
        system: &str,
        history: &[Message],
        user: &str,
    ) -> LLMResult<String> {
        // Default: just use user message (stateless)
        self.prompt(system, user).await
    }
}

/// A message in a conversation
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}
```

### 2.2 Base CLI Adapter

```rust
// forge-llm/src/adapters/base.rs

use tokio::process::Command;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::time::{timeout, Duration};

/// Base implementation for CLI-based LLM adapters
pub struct CliAdapter {
    /// CLI command name or path
    pub cli_command: String,

    /// Default timeout in seconds
    pub timeout_secs: u64,

    /// Additional arguments to pass
    pub extra_args: Vec<String>,
}

impl CliAdapter {
    pub fn new(cli_command: impl Into<String>) -> Self {
        Self {
            cli_command: cli_command.into(),
            timeout_secs: 120,
            extra_args: vec![],
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    /// Check if the CLI command exists
    pub async fn check_available(&self) -> bool {
        Command::new("which")
            .arg(&self.cli_command)
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Execute the CLI with a prompt
    pub async fn execute(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> LLMResult<String> {
        // Build command
        let mut cmd = Command::new(&self.cli_command);

        // Add extra arguments
        for arg in &self.extra_args {
            cmd.arg(arg);
        }

        // Configure stdio
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Spawn process
        let mut child = cmd.spawn().map_err(|e| LLMError::ProcessFailed {
            cmd: self.cli_command.clone(),
            message: e.to_string(),
        })?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let full_prompt = self.format_prompt(system_prompt, user_prompt);
            stdin.write_all(full_prompt.as_bytes()).await
                .map_err(|e| LLMError::ProcessFailed {
                    cmd: self.cli_command.clone(),
                    message: format!("Failed to write to stdin: {}", e),
                })?;
            stdin.shutdown().await.ok();
        }

        // Wait for output with timeout
        let output = timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output()
        ).await
            .map_err(|_| LLMError::Timeout(self.timeout_secs))?
            .map_err(|e| LLMError::ProcessFailed {
                cmd: self.cli_command.clone(),
                message: e.to_string(),
            })?;

        // Check exit status
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LLMError::NonZeroExit {
                code: output.status.code(),
                stderr: stderr.to_string(),
            });
        }

        // Parse output
        String::from_utf8(output.stdout)
            .map_err(|e| LLMError::InvalidOutput(e.to_string()))
    }

    fn format_prompt(&self, system: &str, user: &str) -> String {
        format!(
            "System: {}\n\nHuman: {}\n\nAssistant:",
            system,
            user
        )
    }
}
```

---

## 3. Concrete Adapters

### 3.1 Claude Adapter

```rust
// forge-llm/src/adapters/claude.rs

use super::super::provider::{LLMProvider, LLMResult, LLMError, Message, Role};
use super::base::CliAdapter;
use async_trait::async_trait;

/// Adapter for Claude Code CLI
pub struct ClaudeAdapter {
    base: CliAdapter,
}

impl ClaudeAdapter {
    pub fn new(cli_path: Option<String>) -> Self {
        let cmd = cli_path.unwrap_or_else(|| "claude".to_string());
        Self {
            base: CliAdapter::new(cmd)
                .with_timeout(180)
                .with_args(vec![
                    "--print".to_string(), // Non-interactive mode
                ]),
        }
    }
}

#[async_trait]
impl LLMProvider for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    async fn is_available(&self) -> bool {
        self.base.check_available().await
    }

    async fn prompt(&self, system: &str, user: &str) -> LLMResult<String> {
        self.base.execute(system, user).await
    }

    async fn prompt_with_history(
        &self,
        system: &str,
        history: &[Message],
        user: &str,
    ) -> LLMResult<String> {
        // Build conversation context
        let mut context = String::new();

        for msg in history {
            match msg.role {
                Role::User => context.push_str(&format!("Human: {}\n\n", msg.content)),
                Role::Assistant => context.push_str(&format!("Assistant: {}\n\n", msg.content)),
            }
        }

        let full_user = format!("{}\n\nHuman: {}", context, user);
        self.base.execute(system, &full_user).await
    }
}
```

### 3.2 Gemini Adapter

```rust
// forge-llm/src/adapters/gemini.rs

use super::super::provider::{LLMProvider, LLMResult, LLMError};
use super::base::CliAdapter;
use async_trait::async_trait;

/// Adapter for Google Gemini CLI
pub struct GeminiAdapter {
    base: CliAdapter,
}

impl GeminiAdapter {
    pub fn new(cli_path: Option<String>) -> Self {
        let cmd = cli_path.unwrap_or_else(|| "gemini".to_string());
        Self {
            base: CliAdapter::new(cmd).with_timeout(180),
        }
    }
}

#[async_trait]
impl LLMProvider for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn is_available(&self) -> bool {
        self.base.check_available().await
    }

    async fn prompt(&self, system: &str, user: &str) -> LLMResult<String> {
        // Gemini may have different prompt format
        let combined = format!("{}\n\n{}", system, user);
        self.base.execute("", &combined).await
    }
}
```

### 3.3 Codex Adapter

```rust
// forge-llm/src/adapters/codex.rs

use super::super::provider::{LLMProvider, LLMResult};
use super::base::CliAdapter;
use async_trait::async_trait;

/// Adapter for OpenAI Codex CLI
pub struct CodexAdapter {
    base: CliAdapter,
}

impl CodexAdapter {
    pub fn new(cli_path: Option<String>) -> Self {
        let cmd = cli_path.unwrap_or_else(|| "codex".to_string());
        Self {
            base: CliAdapter::new(cmd).with_timeout(180),
        }
    }
}

#[async_trait]
impl LLMProvider for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    async fn is_available(&self) -> bool {
        self.base.check_available().await
    }

    async fn prompt(&self, system: &str, user: &str) -> LLMResult<String> {
        self.base.execute(system, user).await
    }
}
```

### 3.4 Provider Factory

```rust
// forge-llm/src/lib.rs

pub mod provider;
pub mod adapters;
pub mod interview;

use provider::{LLMProvider, LLMError};
use adapters::{claude::ClaudeAdapter, gemini::GeminiAdapter, codex::CodexAdapter};

/// Configuration for LLM provider
#[derive(Debug, Clone)]
pub struct LLMConfig {
    pub provider: String,
    pub cli_path: Option<String>,
}

/// Create an LLM provider based on configuration
pub fn create_provider(config: &LLMConfig) -> Result<Box<dyn LLMProvider>, LLMError> {
    match config.provider.as_str() {
        "claude" => Ok(Box::new(ClaudeAdapter::new(config.cli_path.clone()))),
        "gemini" => Ok(Box::new(GeminiAdapter::new(config.cli_path.clone()))),
        "codex" => Ok(Box::new(CodexAdapter::new(config.cli_path.clone()))),
        other => Err(LLMError::NotConfigured(format!(
            "Unknown provider: {}. Supported: claude, gemini, codex",
            other
        ))),
    }
}

/// Create provider and verify it's available
pub async fn create_and_verify_provider(
    config: &LLMConfig,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let provider = create_provider(config)?;

    if !provider.is_available().await {
        return Err(LLMError::CliNotFound(config.provider.clone()));
    }

    Ok(provider)
}
```

---

## 4. Gap Analysis

### 4.1 Node Scoring for Interview Priority

```rust
// forge-llm/src/interview.rs

use forge_graph::{ForgeGraph, Node, NodeType, NodeId};
use std::collections::HashMap;

/// Score representing need for business context
#[derive(Debug, Clone)]
pub struct ContextGapScore {
    pub node_id: NodeId,
    pub score: f64,
    pub reasons: Vec<GapReason>,
}

#[derive(Debug, Clone)]
pub enum GapReason {
    /// No business purpose documented
    MissingPurpose,

    /// No owner documented
    MissingOwner,

    /// High connectivity (central to architecture)
    HighCentrality { edge_count: usize },

    /// Has implicit couplings (needs explanation)
    ImplicitCoupling { coupled_services: Vec<String> },

    /// Shared resource without clear ownership
    SharedResourceWithoutOwner,

    /// No gotchas documented for complex service
    ComplexWithoutGotchas { complexity_signals: Vec<String> },
}

/// Analyze graph for context gaps
pub fn analyze_gaps(graph: &ForgeGraph) -> Vec<ContextGapScore> {
    let mut scores: HashMap<NodeId, ContextGapScore> = HashMap::new();

    // Analyze each service
    for service in graph.nodes_by_type(NodeType::Service) {
        let mut gap_score = ContextGapScore {
            node_id: service.id.clone(),
            score: 0.0,
            reasons: vec![],
        };

        // Check for missing purpose
        let has_purpose = service.business_context
            .as_ref()
            .and_then(|bc| bc.purpose.as_ref())
            .map(|p| !p.is_empty())
            .unwrap_or(false);

        if !has_purpose {
            gap_score.score += 0.3;
            gap_score.reasons.push(GapReason::MissingPurpose);
        }

        // Check for missing owner
        let has_owner = service.business_context
            .as_ref()
            .and_then(|bc| bc.owner.as_ref())
            .map(|o| !o.is_empty())
            .unwrap_or(false);

        if !has_owner {
            gap_score.score += 0.2;
            gap_score.reasons.push(GapReason::MissingOwner);
        }

        // Check centrality (edge count)
        let outgoing = graph.edges_from(&service.id).len();
        let incoming = graph.edges_to(&service.id).len();
        let total_edges = outgoing + incoming;

        if total_edges > 5 {
            gap_score.score += 0.2 * (total_edges as f64 / 10.0).min(1.0);
            gap_score.reasons.push(GapReason::HighCentrality {
                edge_count: total_edges,
            });
        }

        // Check for implicit couplings
        let coupled: Vec<String> = graph.edges_from(&service.id)
            .iter()
            .chain(graph.edges_to(&service.id).iter())
            .filter(|e| e.edge_type == forge_graph::EdgeType::ImplicitlyCoupled)
            .filter_map(|e| {
                let other = if e.source == service.id { &e.target } else { &e.source };
                graph.get_node(other).map(|n| n.display_name.clone())
            })
            .collect();

        if !coupled.is_empty() {
            gap_score.score += 0.15;
            gap_score.reasons.push(GapReason::ImplicitCoupling {
                coupled_services: coupled,
            });
        }

        // Check for gotchas in complex services
        let has_gotchas = service.business_context
            .as_ref()
            .map(|bc| !bc.gotchas.is_empty())
            .unwrap_or(false);

        if !has_gotchas && total_edges > 3 {
            let complexity_signals = vec![
                format!("{} dependencies", total_edges),
            ];
            gap_score.score += 0.1;
            gap_score.reasons.push(GapReason::ComplexWithoutGotchas {
                complexity_signals,
            });
        }

        if gap_score.score > 0.0 {
            scores.insert(service.id.clone(), gap_score);
        }
    }

    // Analyze shared resources
    for db in graph.nodes_by_type(NodeType::Database) {
        let accessors: Vec<_> = graph.edges_to(&db.id)
            .iter()
            .filter(|e| matches!(e.edge_type,
                forge_graph::EdgeType::Reads |
                forge_graph::EdgeType::Writes |
                forge_graph::EdgeType::ReadsShared |
                forge_graph::EdgeType::WritesShared
            ))
            .map(|e| e.source.clone())
            .collect();

        // Shared resource without clear owner
        if accessors.len() > 1 {
            let has_owner = graph.edges_to(&db.id)
                .iter()
                .any(|e| e.edge_type == forge_graph::EdgeType::Owns);

            if !has_owner {
                let gap_score = scores.entry(db.id.clone()).or_insert_with(|| {
                    ContextGapScore {
                        node_id: db.id.clone(),
                        score: 0.0,
                        reasons: vec![],
                    }
                });

                gap_score.score += 0.25;
                gap_score.reasons.push(GapReason::SharedResourceWithoutOwner);
            }
        }
    }

    // Sort by score (highest first)
    let mut result: Vec<_> = scores.into_values().collect();
    result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    result
}
```

---

## 5. Question Generation

### 5.1 Question Templates

```rust
// forge-llm/src/interview.rs (continued)

/// A question to ask during the interview
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationType {
    Purpose,
    Owner,
    History,
    Gotcha,
    Note,
}

/// Generate questions for a node based on gap analysis
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
            GapReason::SharedResourceWithoutOwner => {
                questions.push(generate_shared_resource_question(node, graph));
            }
            GapReason::ComplexWithoutGotchas { .. } => {
                questions.push(generate_gotcha_question(node, graph));
            }
        }
    }

    questions
}

fn generate_purpose_question(node: &Node, graph: &ForgeGraph) -> InterviewQuestion {
    let deps = graph.edges_from(&node.id);
    let dep_names: Vec<_> = deps.iter()
        .filter_map(|e| graph.get_node(&e.target).map(|n| n.display_name.clone()))
        .take(5)
        .collect();

    let context = if dep_names.is_empty() {
        format!("Service '{}' exists in your ecosystem.", node.display_name)
    } else {
        format!(
            "Service '{}' depends on: {}",
            node.display_name,
            dep_names.join(", ")
        )
    };

    InterviewQuestion {
        node_id: node.id.clone(),
        question: format!(
            "What is the business purpose of '{}'? What problem does it solve or what capability does it provide?",
            node.display_name
        ),
        annotation_type: AnnotationType::Purpose,
        priority: 9,
        context,
    }
}

fn generate_owner_question(node: &Node) -> InterviewQuestion {
    InterviewQuestion {
        node_id: node.id.clone(),
        question: format!(
            "Who owns or is responsible for '{}'? (Team name, person, or group)",
            node.display_name
        ),
        annotation_type: AnnotationType::Owner,
        priority: 7,
        context: format!("Ownership helps route questions and on-call responsibilities."),
    }
}

fn generate_centrality_question(node: &Node, edge_count: usize, graph: &ForgeGraph) -> InterviewQuestion {
    let callers: Vec<_> = graph.edges_to(&node.id)
        .iter()
        .filter(|e| e.edge_type == forge_graph::EdgeType::Calls)
        .filter_map(|e| graph.get_node(&e.source).map(|n| n.display_name.clone()))
        .collect();

    let context = format!(
        "'{}' has {} connections and is called by: {}",
        node.display_name,
        edge_count,
        if callers.is_empty() { "no direct callers".to_string() } else { callers.join(", ") }
    );

    InterviewQuestion {
        node_id: node.id.clone(),
        question: format!(
            "'{}' appears to be a central service with many dependencies. Why is it so connected? What core capability does it provide?",
            node.display_name
        ),
        annotation_type: AnnotationType::Purpose,
        priority: 8,
        context,
    }
}

fn generate_coupling_question(
    node: &Node,
    coupled_services: &[String],
    graph: &ForgeGraph,
) -> InterviewQuestion {
    // Find the shared resources
    let shared_resources: Vec<_> = graph.edges_from(&node.id)
        .iter()
        .chain(graph.edges_to(&node.id).iter())
        .filter(|e| e.edge_type == forge_graph::EdgeType::ImplicitlyCoupled)
        .filter_map(|e| e.metadata.reason.clone())
        .collect();

    let context = format!(
        "'{}' is implicitly coupled with {} via shared resources. Reasons: {}",
        node.display_name,
        coupled_services.join(", "),
        if shared_resources.is_empty() { "unknown".to_string() } else { shared_resources.join("; ") }
    );

    InterviewQuestion {
        node_id: node.id.clone(),
        question: format!(
            "'{}' shares resources with {}. Is this intentional? What coordination, if any, exists between these services?",
            node.display_name,
            coupled_services.join(", ")
        ),
        annotation_type: AnnotationType::Note,
        priority: 7,
        context,
    }
}

fn generate_shared_resource_question(node: &Node, graph: &ForgeGraph) -> InterviewQuestion {
    let accessors: Vec<_> = graph.edges_to(&node.id)
        .iter()
        .filter(|e| matches!(e.edge_type,
            forge_graph::EdgeType::Reads |
            forge_graph::EdgeType::Writes |
            forge_graph::EdgeType::ReadsShared |
            forge_graph::EdgeType::WritesShared
        ))
        .filter_map(|e| graph.get_node(&e.source).map(|n| n.display_name.clone()))
        .collect();

    InterviewQuestion {
        node_id: node.id.clone(),
        question: format!(
            "Database/resource '{}' is accessed by multiple services ({}). Which service owns this resource and is responsible for its schema?",
            node.display_name,
            accessors.join(", ")
        ),
        annotation_type: AnnotationType::Owner,
        priority: 8,
        context: format!("Shared resources need clear ownership for schema changes and data governance."),
    }
}

fn generate_gotcha_question(node: &Node, graph: &ForgeGraph) -> InterviewQuestion {
    InterviewQuestion {
        node_id: node.id.clone(),
        question: format!(
            "Are there any gotchas, known issues, or operational concerns with '{}'? Things a new team member should know?",
            node.display_name
        ),
        annotation_type: AnnotationType::Gotcha,
        priority: 5,
        context: format!("Service has multiple dependencies and may have non-obvious operational concerns."),
    }
}
```

---

## 6. Interview Flow

### 6.1 Interactive Interview

```rust
// forge-llm/src/interview.rs (continued)

use std::io::{self, Write};
use forge_graph::{ForgeGraph, BusinessContext};

/// Interview session state
pub struct InterviewSession<'a> {
    graph: &'a mut ForgeGraph,
    provider: Box<dyn LLMProvider>,
    questions: Vec<InterviewQuestion>,
    current_index: usize,
    answers: HashMap<NodeId, Vec<AnnotationUpdate>>,
}

#[derive(Debug, Clone)]
pub struct AnnotationUpdate {
    pub annotation_type: AnnotationType,
    pub value: String,
}

impl<'a> InterviewSession<'a> {
    pub fn new(
        graph: &'a mut ForgeGraph,
        provider: Box<dyn LLMProvider>,
    ) -> Self {
        // Analyze gaps and generate questions
        let gaps = analyze_gaps(graph);
        let mut questions: Vec<InterviewQuestion> = gaps.iter()
            .filter_map(|gap| graph.get_node(&gap.node_id).map(|n| (n, gap)))
            .flat_map(|(node, gap)| generate_questions(node, graph, gap))
            .collect();

        // Sort by priority
        questions.sort_by(|a, b| b.priority.cmp(&a.priority));

        Self {
            graph,
            provider,
            questions,
            current_index: 0,
            answers: HashMap::new(),
        }
    }

    /// Total number of questions
    pub fn total_questions(&self) -> usize {
        self.questions.len()
    }

    /// Current question index (1-based for display)
    pub fn current_question_number(&self) -> usize {
        self.current_index + 1
    }

    /// Is the interview complete?
    pub fn is_complete(&self) -> bool {
        self.current_index >= self.questions.len()
    }

    /// Get the current question
    pub fn current_question(&self) -> Option<&InterviewQuestion> {
        self.questions.get(self.current_index)
    }

    /// Submit answer for current question
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

    /// Skip current question
    pub fn skip(&mut self) {
        self.current_index += 1;
    }

    /// Apply all collected answers to the graph
    pub fn apply_to_graph(&mut self) {
        for (node_id, updates) in &self.answers {
            if let Some(node) = self.graph.get_node_mut(node_id) {
                let bc = node.business_context.get_or_insert_with(BusinessContext::default);

                for update in updates {
                    match update.annotation_type {
                        AnnotationType::Purpose => bc.purpose = Some(update.value.clone()),
                        AnnotationType::Owner => bc.owner = Some(update.value.clone()),
                        AnnotationType::History => bc.history = Some(update.value.clone()),
                        AnnotationType::Gotcha => bc.gotchas.push(update.value.clone()),
                        AnnotationType::Note => {
                            bc.notes.insert(
                                format!("note_{}", bc.notes.len() + 1),
                                update.value.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Generate LLM-assisted answer suggestion
    pub async fn suggest_answer(&self, question: &InterviewQuestion) -> LLMResult<String> {
        let system = r#"You are helping document a software ecosystem. Based on the context provided, suggest a concise answer to the question. If you cannot determine the answer from context alone, say "Unable to determine from available context - please provide this information manually."

Keep answers brief (1-3 sentences) and focused."#;

        let user = format!(
            "Context: {}\n\nQuestion: {}",
            question.context,
            question.question
        );

        self.provider.prompt(system, &user).await
    }
}

/// Run interactive terminal interview
pub async fn run_interactive_interview(
    graph: &mut ForgeGraph,
    provider: Box<dyn LLMProvider>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = InterviewSession::new(graph, provider);

    if session.total_questions() == 0 {
        println!("No context gaps detected - graph is well-documented!");
        return Ok(());
    }

    println!("Business Context Interview");
    println!("==========================");
    println!("Found {} questions to help document your ecosystem.\n", session.total_questions());
    println!("Commands: [a]nswer, [s]uggest (use LLM), [k]skip, [q]uit\n");

    while !session.is_complete() {
        let question = session.current_question().unwrap();

        println!("Question {}/{}", session.current_question_number(), session.total_questions());
        println!("About: {}", question.node_id.name());
        println!("Context: {}", question.context);
        println!();
        println!("{}", question.question);
        println!();

        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        match input.chars().next() {
            Some('s') | Some('S') => {
                // Use LLM to suggest
                println!("Getting suggestion from LLM...");
                match session.suggest_answer(question).await {
                    Ok(suggestion) => {
                        println!("\nSuggested answer: {}", suggestion);
                        println!("\nAccept? [y/n/edit]");
                        print!("> ");
                        io::stdout().flush()?;

                        let mut response = String::new();
                        io::stdin().read_line(&mut response)?;
                        let response = response.trim().to_lowercase();

                        if response == "y" || response == "yes" {
                            session.submit_answer(&suggestion);
                        } else if response.starts_with("edit") {
                            print!("Your answer: ");
                            io::stdout().flush()?;
                            let mut edited = String::new();
                            io::stdin().read_line(&mut edited)?;
                            session.submit_answer(edited.trim());
                        } else {
                            println!("Skipped.");
                            session.skip();
                        }
                    }
                    Err(e) => {
                        println!("LLM error: {}. Please answer manually.", e);
                    }
                }
            }
            Some('k') | Some('K') => {
                session.skip();
                println!("Skipped.\n");
            }
            Some('q') | Some('Q') => {
                println!("Interview ended early. Saving progress...");
                break;
            }
            Some('a') | Some('A') => {
                print!("Your answer: ");
                io::stdout().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                session.submit_answer(answer.trim());
            }
            _ if !input.is_empty() => {
                // Treat as direct answer
                session.submit_answer(input);
            }
            _ => {
                println!("Please enter a command or answer.");
            }
        }

        println!();
    }

    // Apply answers to graph
    session.apply_to_graph();
    println!("\nInterview complete! Annotations have been added to the graph.");

    Ok(())
}
```

---

## 7. Annotation Persistence

### 7.1 Merge Strategy

```rust
// forge-graph/src/node.rs (extended)

impl BusinessContext {
    /// Merge another BusinessContext into this one (preserving existing values)
    pub fn merge(&mut self, other: &BusinessContext) {
        // Only update if we don't have a value
        if self.purpose.is_none() {
            self.purpose = other.purpose.clone();
        }

        if self.owner.is_none() {
            self.owner = other.owner.clone();
        }

        if self.history.is_none() {
            self.history = other.history.clone();
        }

        // Merge gotchas (deduplicate)
        for gotcha in &other.gotchas {
            if !self.gotchas.contains(gotcha) {
                self.gotchas.push(gotcha.clone());
            }
        }

        // Merge notes
        for (key, value) in &other.notes {
            if !self.notes.contains_key(key) {
                self.notes.insert(key.clone(), value.clone());
            }
        }
    }
}

// forge-survey/src/lib.rs (updated for annotation preservation)

impl GraphBuilder {
    /// Merge with existing graph, preserving business context
    pub fn merge_from_existing(&mut self, existing: &ForgeGraph) {
        for existing_node in existing.nodes() {
            if let Some(node) = self.graph.get_node_mut(&existing_node.id) {
                // Preserve business context from existing graph
                if let Some(existing_bc) = &existing_node.business_context {
                    let bc = node.business_context.get_or_insert_with(BusinessContext::default);
                    bc.merge(existing_bc);
                }
            }
        }
    }
}
```

### 7.2 Survey Integration

```rust
// forge-cli/src/commands/survey.rs (updated)

pub async fn run_survey(options: SurveyOptions) -> Result<(), Box<dyn std::error::Error>> {
    // ... existing survey logic ...

    // After survey, if --business-context flag is set
    if options.business_context {
        let llm_config = LLMConfig {
            provider: config.llm.provider.clone(),
            cli_path: config.llm.cli_path.clone(),
        };

        match forge_llm::create_and_verify_provider(&llm_config).await {
            Ok(provider) => {
                println!("\nStarting business context interview...\n");
                forge_llm::interview::run_interactive_interview(&mut graph, provider).await?;
            }
            Err(e) => {
                println!(
                    "Warning: LLM provider '{}' not available ({}). Skipping interview.",
                    config.llm.provider, e
                );
                println!("Install the CLI or change llm.provider in forge.yaml to enable interviews.");
            }
        }
    }

    // Save graph with annotations
    graph.save_to_file(&output_path)?;

    Ok(())
}
```

---

## 8. Test Specifications

### 8.1 Provider Tests

```rust
#[cfg(test)]
mod provider_tests {
    use super::*;

    struct MockProvider {
        response: String,
    }

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str { "mock" }

        async fn is_available(&self) -> bool { true }

        async fn prompt(&self, _system: &str, _user: &str) -> LLMResult<String> {
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockProvider {
            response: "Test response".to_string(),
        };

        let result = provider.prompt("system", "user").await.unwrap();
        assert_eq!(result, "Test response");
    }
}
```

### 8.2 Gap Analysis Tests

```rust
#[cfg(test)]
mod gap_analysis_tests {
    use super::*;

    #[test]
    fn test_detect_missing_purpose() {
        let mut graph = ForgeGraph::new();

        let node = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "svc").unwrap())
            .node_type(NodeType::Service)
            .display_name("svc")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);

        assert!(!gaps.is_empty());
        assert!(gaps[0].reasons.iter().any(|r| matches!(r, GapReason::MissingPurpose)));
    }

    #[test]
    fn test_no_gap_when_annotated() {
        let mut graph = ForgeGraph::new();

        let mut node = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "svc").unwrap())
            .node_type(NodeType::Service)
            .display_name("svc")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        node.business_context = Some(BusinessContext {
            purpose: Some("Handles authentication".to_string()),
            owner: Some("Auth Team".to_string()),
            ..Default::default()
        });

        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);

        // Should have lower score or no gap for purpose/owner
        if !gaps.is_empty() {
            assert!(!gaps[0].reasons.iter().any(|r| matches!(r, GapReason::MissingPurpose)));
            assert!(!gaps[0].reasons.iter().any(|r| matches!(r, GapReason::MissingOwner)));
        }
    }
}
```

### 8.3 Interview Tests

```rust
#[cfg(test)]
mod interview_tests {
    use super::*;

    #[test]
    fn test_generate_questions() {
        let mut graph = ForgeGraph::new();

        let node = NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, "ns", "user-api").unwrap())
            .node_type(NodeType::Service)
            .display_name("user-api")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();

        graph.add_node(node).unwrap();

        let gaps = analyze_gaps(&graph);
        let node = graph.get_node(&gaps[0].node_id).unwrap();
        let questions = generate_questions(node, &graph, &gaps[0]);

        assert!(!questions.is_empty());
        assert!(questions.iter().any(|q| q.annotation_type == AnnotationType::Purpose));
    }

    #[test]
    fn test_annotation_persistence() {
        let mut bc1 = BusinessContext {
            purpose: Some("Original purpose".to_string()),
            owner: None,
            ..Default::default()
        };

        let bc2 = BusinessContext {
            purpose: Some("New purpose".to_string()),
            owner: Some("New Team".to_string()),
            ..Default::default()
        };

        bc1.merge(&bc2);

        // Original purpose should be preserved
        assert_eq!(bc1.purpose, Some("Original purpose".to_string()));
        // New owner should be added
        assert_eq!(bc1.owner, Some("New Team".to_string()));
    }
}
```

---

## 9. Implementation Checklist

| Task ID | Description | Files |
|---------|-------------|-------|
| M6-T1 | Define LLM provider trait | `forge-llm/src/provider.rs` |
| M6-T2 | Implement Claude CLI adapter | `forge-llm/src/adapters/claude.rs` |
| M6-T3 | Implement Gemini CLI adapter | `forge-llm/src/adapters/gemini.rs` |
| M6-T4 | Implement Codex CLI adapter | `forge-llm/src/adapters/codex.rs` |
| M6-T5 | Implement provider factory | `forge-llm/src/lib.rs` |
| M6-T6 | Implement gap analysis | `forge-llm/src/interview.rs` |
| M6-T7 | Implement question generation | `forge-llm/src/interview.rs` |
| M6-T8 | Implement interview flow | `forge-llm/src/interview.rs` |
| M6-T9 | Implement annotation persistence | `forge-graph/src/node.rs` |
| M6-T10 | Add `--business-context` flag | `forge-cli/src/commands/survey.rs` |
| M6-T11 | Write tests with mocked LLM | `forge-llm/tests/` |

---

## 10. Acceptance Criteria

- [ ] `forge survey --business-context` launches interactive interview
- [ ] Interview questions are generated from gap analysis
- [ ] Questions prioritize high-centrality services
- [ ] Questions address implicit couplings
- [ ] LLM can suggest answers when requested
- [ ] Annotations persist across survey re-runs
- [ ] Can switch providers via `llm.provider` config
- [ ] Graceful degradation when CLI unavailable
- [ ] No API keys stored in forge.yaml
