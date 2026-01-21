//! Parser module for Forge survey.
//!
//! This module provides the framework for language-specific code parsers that
//! extract information from source code to build the knowledge graph.
//!
//! # Architecture
//!
//! The parser architecture is built around the [`Parser`] trait, which all
//! language-specific parsers implement. Each parser is responsible for:
//!
//! 1. Identifying which file extensions it handles
//! 2. Parsing files to extract [`Discovery`] items
//! 3. Walking repository directories (with a sensible default implementation)
//!
//! # Key Principle: Deterministic Execution
//!
//! All parsers in this module use tree-sitter AST parsing only - no LLM calls.
//! This ensures:
//! - **Reproducibility**: Same input code always produces the same discoveries
//! - **Speed**: No API latency or rate limits
//! - **Offline capability**: Works without network access
//!
//! # Available Parsers
//!
//! - [`JavaScriptParser`] - JavaScript/TypeScript (Milestone 2)
//! - [`PythonParser`] - Python (Milestone 3)
//! - [`TerraformParser`] - Terraform/HCL (Milestone 3)
//!
//! # Adding a New Parser
//!
//! To add support for a new language:
//!
//! 1. Create a new file: `parser/{language}.rs`
//! 2. Implement the [`Parser`] trait
//! 3. Add the module to this file and export the parser
//! 4. Register the parser in the parser registry (coming in M3)
//!
//! See the extension guide in `docs/extending-parsers.md` for detailed instructions.

pub mod javascript;
pub mod python;
pub mod terraform;
mod traits;

// Re-export all public types from traits
pub use traits::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError, QueueOperationDiscovery, QueueOperationType,
    ServiceDiscovery,
};

// Re-export parsers
pub use javascript::JavaScriptParser;
pub use python::PythonParser;
pub use terraform::TerraformParser;
