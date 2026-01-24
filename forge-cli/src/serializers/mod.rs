//! Serializers for converting knowledge graphs to various output formats.
//!
//! This module provides serializers for the `forge map` command:
//!
//! - **Markdown**: Human-readable documentation optimized for LLM context consumption
//! - **JSON**: Structured format for programmatic access
//! - **Mermaid**: Visual diagram syntax for documentation
//!
//! ## Design Philosophy
//!
//! Serializers transform the knowledge graph into LLM-consumable formats.
//! LLMs consume text, not graph structures—these serializers build the critical
//! bridge between Forge's internal representation and formats optimized for
//! AI comprehension and human review.

pub mod json;
pub mod markdown;
pub mod mermaid;

pub use json::{JsonOutput, JsonSerializer, QueryInfo};
pub use markdown::{DetailLevel, MarkdownSerializer};
pub use mermaid::{Direction, MermaidSerializer};
