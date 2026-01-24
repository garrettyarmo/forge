//! LLM CLI adapter layer for Forge.
//!
//! This crate provides adapters for shelling out to coding agent CLIs
//! (Claude, Gemini, Codex) for business context interviews.
//!
//! # Architecture
//!
//! Forge integrates with LLMs by **shelling out to coding agent CLIs** rather
//! than making direct API calls. This approach:
//!
//! - Leverages user's existing CLI authentication
//! - No API keys stored in forge.yaml
//! - Provider-agnostic design
//! - Works with any CLI that accepts stdin/stdout
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_llm::{LLMConfig, create_provider};
//!
//! let config = LLMConfig {
//!     provider: "claude".to_string(),
//!     cli_path: None,
//! };
//!
//! let provider = create_provider(&config)?;
//! let response = provider.prompt("You are helpful.", "What is Rust?").await?;
//! ```

pub mod provider;

// Re-export main types for convenience
pub use provider::{LLMError, LLMProvider, LLMResult, Message, Role};
