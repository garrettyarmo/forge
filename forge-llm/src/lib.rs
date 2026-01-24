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
//! # Available Providers
//!
//! - `claude` - Claude Code CLI adapter (see [`adapters::ClaudeAdapter`])
//! - `gemini` - Google Gemini CLI adapter (see [`adapters::GeminiAdapter`])
//! - `codex` - OpenAI Codex CLI adapter (see [`adapters::CodexAdapter`])
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
//!
//! # Provider Factory
//!
//! Use [`create_provider`] to instantiate a provider based on configuration:
//!
//! ```rust,ignore
//! use forge_llm::{LLMConfig, create_provider, create_and_verify_provider};
//!
//! // Create without checking availability
//! let provider = create_provider(&config)?;
//!
//! // Create and verify CLI is installed
//! let provider = create_and_verify_provider(&config).await?;
//! ```

pub mod adapters;
pub mod interview;
pub mod provider;

// Re-export main types for convenience
pub use provider::{LLMError, LLMProvider, LLMResult, Message, Role};

// Re-export adapters
pub use adapters::ClaudeAdapter;
pub use adapters::CodexAdapter;
pub use adapters::GeminiAdapter;

// Re-export interview types
pub use interview::{
    AnnotationType,
    // Interview flow (M6-T8)
    AnnotationUpdate,
    ContextGapScore,
    GapAnalysisConfig,
    GapReason,
    InterviewError,
    InterviewQuestion,
    InterviewResult,
    InterviewSession,
    // Gap analysis
    analyze_gaps,
    analyze_gaps_with_config,
    // Question generation
    generate_all_questions,
    generate_questions,
    merge_business_context,
    run_interactive_interview,
};

/// Configuration for creating an LLM provider.
#[derive(Debug, Clone, Default)]
pub struct LLMConfig {
    /// Provider name: "claude", "gemini", or "codex".
    pub provider: String,

    /// Optional custom path to the CLI executable.
    /// If `None`, the default command name is used (must be in PATH).
    pub cli_path: Option<String>,
}

impl LLMConfig {
    /// Create a new LLM config for the given provider.
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            cli_path: None,
        }
    }

    /// Set a custom CLI path (builder pattern).
    pub fn with_cli_path(mut self, path: impl Into<String>) -> Self {
        self.cli_path = Some(path.into());
        self
    }
}

/// Create an LLM provider based on configuration.
///
/// This creates the provider without checking if the CLI is actually installed.
/// Use [`create_and_verify_provider`] if you need to verify availability first.
///
/// # Arguments
/// * `config` - Configuration specifying which provider to create
///
/// # Returns
/// A boxed trait object implementing [`LLMProvider`]
///
/// # Errors
/// Returns [`LLMError::NotConfigured`] if the provider name is unknown.
///
/// # Supported Providers
/// - `"claude"` - Claude Code CLI
/// - `"gemini"` - Google Gemini CLI
/// - `"codex"` - OpenAI Codex CLI
///
/// # Example
///
/// ```rust
/// use forge_llm::{LLMConfig, create_provider};
///
/// let config = LLMConfig::new("claude");
/// let provider = create_provider(&config).unwrap();
/// assert_eq!(provider.name(), "claude");
/// ```
pub fn create_provider(config: &LLMConfig) -> Result<Box<dyn LLMProvider>, LLMError> {
    match config.provider.as_str() {
        "claude" => Ok(Box::new(ClaudeAdapter::new(config.cli_path.clone()))),
        "gemini" => Ok(Box::new(GeminiAdapter::new(config.cli_path.clone()))),
        "codex" => Ok(Box::new(CodexAdapter::new(config.cli_path.clone()))),
        other => Err(LLMError::NotConfigured(format!(
            "Unknown provider: '{}'. Supported providers: claude, gemini, codex",
            other
        ))),
    }
}

/// Create an LLM provider and verify it's available.
///
/// This creates the provider and checks that the CLI is actually installed
/// and accessible in the system PATH.
///
/// # Arguments
/// * `config` - Configuration specifying which provider to create
///
/// # Returns
/// A boxed trait object implementing [`LLMProvider`]
///
/// # Errors
/// - [`LLMError::NotConfigured`] if the provider name is unknown
/// - [`LLMError::CliNotFound`] if the CLI is not installed or not in PATH
///
/// # Example
///
/// ```rust,ignore
/// use forge_llm::{LLMConfig, create_and_verify_provider};
///
/// let config = LLMConfig::new("claude");
/// match create_and_verify_provider(&config).await {
///     Ok(provider) => {
///         let response = provider.prompt("system", "hello").await?;
///     }
///     Err(e) => {
///         println!("Provider not available: {}", e);
///     }
/// }
/// ```
pub async fn create_and_verify_provider(
    config: &LLMConfig,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let provider = create_provider(config)?;

    if !provider.is_available().await {
        return Err(LLMError::CliNotFound(format!(
            "{} (provider: {})",
            config.cli_path.as_deref().unwrap_or(&config.provider),
            config.provider
        )));
    }

    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_new() {
        let config = LLMConfig::new("claude");
        assert_eq!(config.provider, "claude");
        assert!(config.cli_path.is_none());
    }

    #[test]
    fn test_llm_config_with_cli_path() {
        let config = LLMConfig::new("claude").with_cli_path("/custom/path/claude");
        assert_eq!(config.provider, "claude");
        assert_eq!(config.cli_path, Some("/custom/path/claude".to_string()));
    }

    #[test]
    fn test_create_provider_claude() {
        let config = LLMConfig::new("claude");
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "claude");
    }

    #[test]
    fn test_create_provider_gemini() {
        let config = LLMConfig::new("gemini");
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "gemini");
    }

    #[test]
    fn test_create_provider_gemini_with_custom_path() {
        let config = LLMConfig::new("gemini").with_cli_path("/custom/gemini");
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "gemini");
    }

    #[test]
    fn test_create_provider_codex() {
        let config = LLMConfig::new("codex");
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "codex");
    }

    #[test]
    fn test_create_provider_codex_with_custom_path() {
        let config = LLMConfig::new("codex").with_cli_path("/custom/codex");
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "codex");
    }

    #[test]
    fn test_create_provider_unknown() {
        let config = LLMConfig::new("unknown-provider");
        let result = create_provider(&config);
        assert!(matches!(result, Err(LLMError::NotConfigured(_))));
    }

    #[test]
    fn test_create_provider_with_custom_path() {
        let config = LLMConfig::new("claude").with_cli_path("/custom/claude");
        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "claude");
    }

    #[tokio::test]
    async fn test_create_and_verify_provider_unavailable() {
        // Use a fake path that doesn't exist
        let config = LLMConfig::new("claude").with_cli_path("/nonexistent/path/claude");
        let result = create_and_verify_provider(&config).await;
        assert!(matches!(result, Err(LLMError::CliNotFound(_))));
    }

    #[tokio::test]
    async fn test_create_and_verify_gemini_unavailable() {
        // Use a fake path that doesn't exist
        let config = LLMConfig::new("gemini").with_cli_path("/nonexistent/path/gemini");
        let result = create_and_verify_provider(&config).await;
        assert!(matches!(result, Err(LLMError::CliNotFound(_))));
    }

    #[tokio::test]
    async fn test_create_and_verify_codex_unavailable() {
        // Use a fake path that doesn't exist
        let config = LLMConfig::new("codex").with_cli_path("/nonexistent/path/codex");
        let result = create_and_verify_provider(&config).await;
        assert!(matches!(result, Err(LLMError::CliNotFound(_))));
    }

    #[tokio::test]
    async fn test_create_and_verify_unknown_provider() {
        let config = LLMConfig::new("unknown-provider");
        let result = create_and_verify_provider(&config).await;
        assert!(matches!(result, Err(LLMError::NotConfigured(_))));
    }
}
