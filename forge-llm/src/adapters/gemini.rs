//! Gemini CLI adapter for Forge.
//!
//! This adapter shells out to the `gemini` CLI tool to interact with
//! Google's Gemini AI. It leverages the user's existing CLI authentication
//! rather than requiring API keys in forge.yaml.
//!
//! # Requirements
//!
//! The `gemini` CLI must be installed and available in PATH.
//! Install via: `npm install -g @anthropic-ai/gemini` (or similar)
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_llm::adapters::gemini::GeminiAdapter;
//! use forge_llm::provider::LLMProvider;
//!
//! let adapter = GeminiAdapter::new(None);
//!
//! if adapter.is_available().await {
//!     let response = adapter.prompt(
//!         "You are a helpful assistant.",
//!         "What is the capital of France?"
//!     ).await?;
//!     println!("{}", response);
//! }
//! ```

use super::base::CliAdapter;
use crate::provider::{LLMError, LLMProvider, LLMResult, Message, Role};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

/// Default timeout in seconds for Gemini CLI operations.
const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// Adapter for the Google Gemini CLI.
///
/// This adapter communicates with Gemini through a command-line tool,
/// which handles authentication and API communication internally.
///
/// # Features
///
/// - Supports system prompts combined with user prompts
/// - Handles conversation history for multi-turn interactions
/// - Configurable timeout (default: 180 seconds)
///
/// # CLI Arguments
///
/// The adapter formats prompts for stdin/stdout communication with
/// the Gemini CLI tool.
#[derive(Debug, Clone)]
pub struct GeminiAdapter {
    /// The underlying CLI adapter with common functionality.
    base: CliAdapter,
}

impl GeminiAdapter {
    /// Create a new Gemini adapter.
    ///
    /// # Arguments
    /// * `cli_path` - Optional custom path to the `gemini` CLI.
    ///   If `None`, uses "gemini" (must be in PATH).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Use default 'gemini' command from PATH
    /// let adapter = GeminiAdapter::new(None);
    ///
    /// // Use custom path
    /// let adapter = GeminiAdapter::new(Some("/usr/local/bin/gemini".to_string()));
    /// ```
    pub fn new(cli_path: Option<String>) -> Self {
        let cmd = cli_path.unwrap_or_else(|| "gemini".to_string());
        Self {
            base: CliAdapter::new(cmd).with_timeout(DEFAULT_TIMEOUT_SECS),
        }
    }

    /// Create a new Gemini adapter with custom timeout.
    ///
    /// # Arguments
    /// * `cli_path` - Optional custom path to the `gemini` CLI
    /// * `timeout_secs` - Timeout in seconds for CLI operations
    pub fn with_timeout(cli_path: Option<String>, timeout_secs: u64) -> Self {
        let mut adapter = Self::new(cli_path);
        adapter.base.timeout_secs = timeout_secs;
        adapter
    }

    /// Execute the Gemini CLI with a prompt.
    ///
    /// This method handles Gemini-specific prompt formatting and
    /// CLI argument handling.
    async fn execute_prompt(&self, system: &str, user: &str) -> LLMResult<String> {
        // Build command with Gemini-specific arguments
        let mut cmd = Command::new(&self.base.cli_command);

        // Add base arguments
        for arg in &self.base.extra_args {
            cmd.arg(arg);
        }

        // Configure stdio
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn process
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                LLMError::CliNotFound(self.base.cli_command.clone())
            } else {
                LLMError::ProcessFailed {
                    cmd: self.base.cli_command.clone(),
                    message: e.to_string(),
                }
            }
        })?;

        // Format and write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let full_prompt = self.format_gemini_prompt(system, user);
            stdin
                .write_all(full_prompt.as_bytes())
                .await
                .map_err(|e| LLMError::ProcessFailed {
                    cmd: self.base.cli_command.clone(),
                    message: format!("Failed to write to stdin: {}", e),
                })?;
            // Close stdin to signal end of input
            drop(stdin);
        }

        // Wait for output with timeout
        let output = timeout(
            Duration::from_secs(self.base.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| LLMError::Timeout(self.base.timeout_secs))?
        .map_err(|e| LLMError::ProcessFailed {
            cmd: self.base.cli_command.clone(),
            message: e.to_string(),
        })?;

        // Check exit status
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LLMError::NonZeroExit {
                code: output.status.code(),
                stderr: stderr.to_string(),
            });
        }

        // Parse and clean output
        let response =
            String::from_utf8(output.stdout).map_err(|e| LLMError::InvalidOutput(e.to_string()))?;

        Ok(self.clean_response(&response))
    }

    /// Format the prompt for Gemini CLI.
    ///
    /// Gemini CLI accepts prompts via stdin. The system prompt is
    /// combined with the user message for context.
    fn format_gemini_prompt(&self, system: &str, user: &str) -> String {
        if system.is_empty() {
            user.to_string()
        } else {
            // Gemini uses a simpler format: combine system and user context
            format!("{}\n\n{}", system.trim(), user.trim())
        }
    }

    /// Format conversation history for multi-turn prompts.
    fn format_history(&self, history: &[Message]) -> String {
        let mut context = String::new();

        for msg in history {
            match msg.role {
                Role::User => {
                    context.push_str("User: ");
                    context.push_str(&msg.content);
                    context.push_str("\n\n");
                }
                Role::Assistant => {
                    context.push_str("Model: ");
                    context.push_str(&msg.content);
                    context.push_str("\n\n");
                }
            }
        }

        context
    }

    /// Clean up the response from Gemini CLI.
    ///
    /// Removes any CLI-specific formatting, trailing whitespace, etc.
    fn clean_response(&self, response: &str) -> String {
        response.trim().to_string()
    }
}

#[async_trait]
impl LLMProvider for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn is_available(&self) -> bool {
        self.base.check_available().await
    }

    async fn prompt(&self, system: &str, user: &str) -> LLMResult<String> {
        self.execute_prompt(system, user).await
    }

    async fn prompt_with_history(
        &self,
        system: &str,
        history: &[Message],
        user: &str,
    ) -> LLMResult<String> {
        // Build conversation context from history
        let history_context = self.format_history(history);

        // Combine history with current message
        let full_user = if history_context.is_empty() {
            user.to_string()
        } else {
            format!("{}\nUser: {}", history_context.trim(), user)
        };

        self.execute_prompt(system, &full_user).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default_command() {
        let adapter = GeminiAdapter::new(None);
        assert_eq!(adapter.base.cli_command, "gemini");
        assert_eq!(adapter.base.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn test_new_custom_path() {
        let adapter = GeminiAdapter::new(Some("/custom/path/gemini".to_string()));
        assert_eq!(adapter.base.cli_command, "/custom/path/gemini");
    }

    #[test]
    fn test_with_timeout() {
        let adapter = GeminiAdapter::with_timeout(None, 300);
        assert_eq!(adapter.base.timeout_secs, 300);
    }

    #[test]
    fn test_format_prompt_with_system() {
        let adapter = GeminiAdapter::new(None);
        let formatted = adapter.format_gemini_prompt("Be helpful", "Hello");
        assert!(formatted.contains("Be helpful"));
        assert!(formatted.contains("Hello"));
        // System and user should be separated
        assert!(formatted.contains("\n\n"));
    }

    #[test]
    fn test_format_prompt_without_system() {
        let adapter = GeminiAdapter::new(None);
        let formatted = adapter.format_gemini_prompt("", "Hello");
        assert_eq!(formatted, "Hello");
    }

    #[test]
    fn test_format_history_empty() {
        let adapter = GeminiAdapter::new(None);
        let history: Vec<Message> = vec![];
        let formatted = adapter.format_history(&history);
        assert!(formatted.is_empty());
    }

    #[test]
    fn test_format_history_with_messages() {
        let adapter = GeminiAdapter::new(None);
        let history = vec![
            Message::user("What is 2+2?"),
            Message::assistant("2+2 equals 4."),
            Message::user("And 3+3?"),
            Message::assistant("3+3 equals 6."),
        ];

        let formatted = adapter.format_history(&history);

        assert!(formatted.contains("User: What is 2+2?"));
        assert!(formatted.contains("Model: 2+2 equals 4."));
        assert!(formatted.contains("User: And 3+3?"));
        assert!(formatted.contains("Model: 3+3 equals 6."));
    }

    #[test]
    fn test_clean_response() {
        let adapter = GeminiAdapter::new(None);

        assert_eq!(adapter.clean_response("  Hello  "), "Hello");
        assert_eq!(adapter.clean_response("\n\nHello\n\n"), "Hello");
        assert_eq!(adapter.clean_response("Hello World"), "Hello World");
    }

    #[test]
    fn test_provider_name() {
        let adapter = GeminiAdapter::new(None);
        assert_eq!(adapter.name(), "gemini");
    }

    #[tokio::test]
    async fn test_is_available_not_installed() {
        // Test with a fake command that shouldn't exist
        let adapter = GeminiAdapter::new(Some("nonexistent-gemini-fake-12345".to_string()));
        assert!(!adapter.is_available().await);
    }

    #[tokio::test]
    async fn test_prompt_cli_not_found() {
        let adapter = GeminiAdapter::new(Some("nonexistent-gemini-fake-12345".to_string()));

        let result = adapter.prompt("system", "user").await;

        // Should return CliNotFound or ProcessFailed
        assert!(matches!(
            result,
            Err(LLMError::CliNotFound(_)) | Err(LLMError::ProcessFailed { .. })
        ));
    }
}
