//! LLM provider trait and error types.
//!
//! This module defines the core trait for LLM providers that shell out to
//! coding agent CLIs (Claude, Gemini, Codex) rather than making direct API calls.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during LLM operations.
#[derive(Debug, Error)]
pub enum LLMError {
    /// Failed to spawn the CLI process.
    #[error("Failed to spawn process '{cmd}': {message}")]
    ProcessFailed { cmd: String, message: String },

    /// Process exited with non-zero status.
    #[error("Process exited with code {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },

    /// Invalid output from the LLM.
    #[error("Invalid output from LLM: {0}")]
    InvalidOutput(String),

    /// CLI command not found in PATH.
    #[error("LLM CLI not found: {0}. Is it installed and in your PATH?")]
    CliNotFound(String),

    /// Timeout waiting for LLM response.
    #[error("Timeout waiting for LLM response after {0} seconds")]
    Timeout(u64),

    /// Provider not configured or unknown.
    #[error("Provider not configured: {0}")]
    NotConfigured(String),

    /// I/O error during process communication.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for LLM operations.
pub type LLMResult<T> = Result<T, LLMError>;

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender.
    pub role: Role,
    /// The content of the message.
    pub content: String,
}

impl Message {
    /// Create a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Create a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Role of a message sender in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User/human message.
    User,
    /// Assistant/LLM message.
    Assistant,
}

/// Trait for LLM providers.
///
/// Implementations shell out to coding agent CLIs (claude, gemini, codex)
/// rather than making direct API calls. This leverages the user's existing
/// CLI authentication and avoids storing API keys in forge.yaml.
///
/// # Example
///
/// ```rust,ignore
/// use forge_llm::provider::{LLMProvider, LLMResult};
///
/// async fn example(provider: &dyn LLMProvider) -> LLMResult<String> {
///     let system = "You are a helpful assistant.";
///     let user = "What is the capital of France?";
///     provider.prompt(system, user).await
/// }
/// ```
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Get the provider name (e.g., "claude", "gemini", "codex").
    fn name(&self) -> &str;

    /// Check if the CLI is available in the system PATH.
    ///
    /// This checks whether the command can be executed, not whether
    /// authentication is valid.
    async fn is_available(&self) -> bool;

    /// Send a prompt and get a response.
    ///
    /// # Arguments
    /// * `system` - System prompt setting context and instructions
    /// * `user` - User message/question
    ///
    /// # Returns
    /// The LLM's response text.
    ///
    /// # Errors
    /// Returns an error if the CLI is not available, times out, or
    /// returns an error.
    async fn prompt(&self, system: &str, user: &str) -> LLMResult<String>;

    /// Send a prompt with conversation history.
    ///
    /// The default implementation ignores history and just uses the user message.
    /// Providers that support conversation context should override this.
    ///
    /// # Arguments
    /// * `system` - System prompt setting context and instructions
    /// * `history` - Previous messages in the conversation
    /// * `user` - Current user message/question
    ///
    /// # Returns
    /// The LLM's response text.
    async fn prompt_with_history(
        &self,
        system: &str,
        _history: &[Message],
        user: &str,
    ) -> LLMResult<String> {
        // Default: just use user message (stateless)
        self.prompt(system, user).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock provider for testing.
    pub struct MockProvider {
        pub name: String,
        pub available: bool,
        pub response: String,
    }

    impl MockProvider {
        pub fn new(response: impl Into<String>) -> Self {
            Self {
                name: "mock".to_string(),
                available: true,
                response: response.into(),
            }
        }

        pub fn unavailable() -> Self {
            Self {
                name: "mock".to_string(),
                available: false,
                response: String::new(),
            }
        }
    }

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn is_available(&self) -> bool {
            self.available
        }

        async fn prompt(&self, _system: &str, _user: &str) -> LLMResult<String> {
            if !self.available {
                return Err(LLMError::CliNotFound(self.name.clone()));
            }
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockProvider::new("Test response");

        assert_eq!(provider.name(), "mock");
        assert!(provider.is_available().await);

        let result = provider.prompt("system", "user").await.unwrap();
        assert_eq!(result, "Test response");
    }

    #[tokio::test]
    async fn test_mock_provider_unavailable() {
        let provider = MockProvider::unavailable();

        assert!(!provider.is_available().await);

        let result = provider.prompt("system", "user").await;
        assert!(matches!(result, Err(LLMError::CliNotFound(_))));
    }

    #[tokio::test]
    async fn test_prompt_with_history_default() {
        let provider = MockProvider::new("Response with history");

        let history = vec![
            Message::user("First message"),
            Message::assistant("First response"),
        ];

        let result = provider
            .prompt_with_history("system", &history, "Current question")
            .await
            .unwrap();
        assert_eq!(result, "Response with history");
    }

    #[test]
    fn test_message_constructors() {
        let user_msg = Message::user("Hello");
        assert_eq!(user_msg.role, Role::User);
        assert_eq!(user_msg.content, "Hello");

        let assistant_msg = Message::assistant("Hi there");
        assert_eq!(assistant_msg.role, Role::Assistant);
        assert_eq!(assistant_msg.content, "Hi there");
    }

    #[test]
    fn test_error_display() {
        let err = LLMError::ProcessFailed {
            cmd: "claude".to_string(),
            message: "not found".to_string(),
        };
        assert!(err.to_string().contains("claude"));
        assert!(err.to_string().contains("not found"));

        let err = LLMError::NonZeroExit {
            code: Some(1),
            stderr: "error output".to_string(),
        };
        assert!(err.to_string().contains("1"));
        assert!(err.to_string().contains("error output"));

        let err = LLMError::Timeout(120);
        assert!(err.to_string().contains("120"));
    }
}
