//! Code analysis and discovery for Forge.
//!
//! This crate provides functionality for surveying codebases
//! and building knowledge graphs.
//!
//! # Overview
//!
//! The forge-survey crate is responsible for:
//!
//! - **GitHub Integration**: Discovering and cloning repositories from GitHub organizations
//! - **Code Parsing**: Analyzing source code using tree-sitter AST parsing
//! - **Discovery**: Detecting services, APIs, databases, and their relationships
//!
//! # Architecture
//!
//! The survey phase is **purely deterministic** - it uses only tree-sitter AST parsing
//! with no LLM calls. This ensures:
//!
//! - Reproducibility: Same input code always produces the same graph
//! - Speed: No API latency or rate limits
//! - Offline capability: Works without network for local repos
//! - Predictable costs: Zero token usage during survey
//!
//! # Modules
//!
//! - [`github`]: GitHub API client and repository caching
//! - [`parser`]: Language-specific code parsers and discovery types
//! - [`graph_builder`]: Converts parser discoveries into a knowledge graph

pub mod detection;
pub mod github;
pub mod graph_builder;
pub mod parser;

pub use detection::{detect_languages, DetectedLanguage, DetectedLanguages, DetectionMethod};
pub use github::{CloneMethod, GitHubClient, GitHubError, RepoCache, RepoInfo};
pub use graph_builder::GraphBuilder;

// Re-export commonly used parser types for convenience
pub use parser::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError, ParserRegistry, QueueOperationDiscovery,
    QueueOperationType, ServiceDiscovery,
};
