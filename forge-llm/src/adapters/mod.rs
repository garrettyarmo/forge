//! LLM CLI adapters for various coding agent tools.
//!
//! This module provides adapters that shell out to coding agent CLIs
//! (Claude, Gemini, Codex) rather than making direct API calls.
//!
//! # Available Adapters
//!
//! - [`ClaudeAdapter`] - Adapter for the Claude Code CLI (`claude`)
//!
//! # Architecture
//!
//! All adapters share a common base implementation ([`base::CliAdapter`])
//! that handles:
//! - CLI availability checking
//! - Subprocess spawning and stdio handling
//! - Timeout management
//! - Error handling
//!
//! Each concrete adapter implements the [`LLMProvider`](crate::provider::LLMProvider)
//! trait and handles provider-specific prompt formatting.
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_llm::adapters::claude::ClaudeAdapter;
//! use forge_llm::provider::LLMProvider;
//!
//! let adapter = ClaudeAdapter::new(None);
//!
//! if adapter.is_available().await {
//!     let response = adapter.prompt("Be helpful", "What is Rust?").await?;
//! }
//! ```

pub mod base;
pub mod claude;

// Re-export adapters for convenience
pub use claude::ClaudeAdapter;
