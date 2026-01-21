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
//! - **Code Parsing**: Analyzing source code using tree-sitter AST parsing (future milestone)
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

pub mod github;

pub use github::{CloneMethod, GitHubClient, GitHubError, RepoCache, RepoInfo};
