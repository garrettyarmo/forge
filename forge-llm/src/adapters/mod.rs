//! LLM CLI adapters for various coding agent tools.
//!
//! This module provides adapters that shell out to coding agent CLIs
//! (Claude, Gemini, Codex) rather than making direct API calls.
//!
//! # Available Adapters
//!
//! - [`ClaudeAdapter`] - Adapter for the Claude Code CLI (`claude`)
//! - [`GeminiAdapter`] - Adapter for the Google Gemini CLI (`gemini`)
//! - [`CodexAdapter`] - Adapter for the OpenAI Codex CLI (`codex`)
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
//! use forge_llm::adapters::gemini::GeminiAdapter;
//! use forge_llm::adapters::codex::CodexAdapter;
//! use forge_llm::provider::LLMProvider;
//!
//! let claude = ClaudeAdapter::new(None);
//! let gemini = GeminiAdapter::new(None);
//! let codex = CodexAdapter::new(None);
//!
//! if claude.is_available().await {
//!     let response = claude.prompt("Be helpful", "What is Rust?").await?;
//! }
//! ```

pub mod base;
pub mod claude;
pub mod codex;
pub mod gemini;

// Re-export adapters for convenience
pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use gemini::GeminiAdapter;
