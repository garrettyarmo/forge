//! Forge CLI library - Serializers and LLM instruction generation.
//!
//! This library provides the serialization and LLM instruction generation
//! functionality used by the forge CLI tool. It's exposed as a library to
//! enable integration testing.
//!
//! # Modules
//!
//! - [`serializers`]: Convert knowledge graphs to Markdown, JSON, and Mermaid formats
//! - [`llm_instructions`]: Generate LLM-optimized instructions from graph data
//! - [`token_budget`]: Token counting and budget management for LLM context
//! - [`config`]: Configuration loading and management

pub mod config;
pub mod llm_instructions;
pub mod serializers;
pub mod token_budget;

// Re-export commonly used types for convenience
pub use config::{Environment, ForgeConfig, OutputConfig, RepoConfig};
pub use llm_instructions::{
    DependencyInstructions, InstructionError, InstructionGenerator, LlmInstructions,
};
pub use serializers::{
    json::{JsonOutput, JsonSerializer, QueryInfo},
    markdown::{DetailLevel, MarkdownSerializer},
    mermaid::{Direction, MermaidSerializer},
};
pub use token_budget::{BudgetedSerializer, TokenCounter};
