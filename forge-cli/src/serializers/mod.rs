//! Serializers for converting knowledge graphs to various output formats.
//!
//! This module provides serializers for the `forge map` command:
//!
//! - **Markdown**: Human-readable documentation optimized for LLM context consumption
//! - **JSON**: Structured format for programmatic access (planned)
//! - **Mermaid**: Visual diagram syntax for documentation (planned)
//!
//! ## Design Philosophy
//!
//! Serializers transform the knowledge graph into LLM-consumable formats.
//! LLMs consume text, not graph structures—these serializers build the critical
//! bridge between Forge's internal representation and formats optimized for
//! AI comprehension and human review.

pub mod markdown;

pub use markdown::{DetailLevel, MarkdownSerializer};
