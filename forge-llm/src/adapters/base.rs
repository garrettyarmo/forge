//! Base implementation for CLI-based LLM adapters.
//!
//! This module provides a reusable base for building adapters that shell out
//! to coding agent CLIs (claude, gemini, codex) rather than making direct API calls.

use crate::provider::{LLMError, LLMResult};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

/// Base implementation for CLI-based LLM adapters.
///
/// Handles common functionality like:
/// - Checking CLI availability via `which`
/// - Spawning subprocesses with proper stdio handling
/// - Timeout management
/// - Error handling for process failures
///
/// # Example
///
/// ```rust,ignore
/// use forge_llm::adapters::base::CliAdapter;
///
/// let adapter = CliAdapter::new("claude")
///     .with_timeout(180)
///     .with_args(vec!["--print".to_string()]);
///
/// let response = adapter.execute("system prompt", "user message").await?;
/// ```
#[derive(Debug, Clone)]
pub struct CliAdapter {
    /// CLI command name or full path to executable.
    pub cli_command: String,

    /// Timeout in seconds for waiting on LLM response.
    pub timeout_secs: u64,

    /// Additional arguments to pass to the CLI.
    pub extra_args: Vec<String>,
}

impl CliAdapter {
    /// Create a new CLI adapter with the given command.
    ///
    /// # Arguments
    /// * `cli_command` - The CLI command name (e.g., "claude") or full path
    pub fn new(cli_command: impl Into<String>) -> Self {
        Self {
            cli_command: cli_command.into(),
            timeout_secs: 120,
            extra_args: vec![],
        }
    }

    /// Set the timeout in seconds (builder pattern).
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set additional CLI arguments (builder pattern).
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    /// Check if the CLI command is available in the system PATH.
    ///
    /// Uses `which` on Unix systems to verify the command exists.
    /// This checks whether the command can be executed, not whether
    /// authentication is valid.
    pub async fn check_available(&self) -> bool {
        // Try using `which` on Unix or `where` on Windows
        #[cfg(unix)]
        let check_cmd = "which";
        #[cfg(windows)]
        let check_cmd = "where";

        Command::new(check_cmd)
            .arg(&self.cli_command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Execute the CLI with system and user prompts.
    ///
    /// The prompts are formatted and written to stdin, and the response
    /// is read from stdout.
    ///
    /// # Arguments
    /// * `system_prompt` - System prompt setting context
    /// * `user_prompt` - User message/question
    ///
    /// # Returns
    /// The LLM's response text.
    ///
    /// # Errors
    /// - `LLMError::ProcessFailed` if the CLI cannot be spawned
    /// - `LLMError::Timeout` if the response takes too long
    /// - `LLMError::NonZeroExit` if the CLI returns an error
    /// - `LLMError::InvalidOutput` if the output is not valid UTF-8
    pub async fn execute(&self, system_prompt: &str, user_prompt: &str) -> LLMResult<String> {
        // Build command
        let mut cmd = Command::new(&self.cli_command);

        // Add extra arguments
        for arg in &self.extra_args {
            cmd.arg(arg);
        }

        // Configure stdio
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn process
        let mut child = cmd.spawn().map_err(|e| LLMError::ProcessFailed {
            cmd: self.cli_command.clone(),
            message: e.to_string(),
        })?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            let full_prompt = self.format_prompt(system_prompt, user_prompt);
            stdin
                .write_all(full_prompt.as_bytes())
                .await
                .map_err(|e| LLMError::ProcessFailed {
                    cmd: self.cli_command.clone(),
                    message: format!("Failed to write to stdin: {}", e),
                })?;
            // Close stdin to signal end of input
            drop(stdin);
        }

        // Wait for output with timeout
        let output = timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| LLMError::Timeout(self.timeout_secs))?
        .map_err(|e| LLMError::ProcessFailed {
            cmd: self.cli_command.clone(),
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

        // Parse output
        String::from_utf8(output.stdout).map_err(|e| LLMError::InvalidOutput(e.to_string()))
    }

    /// Format the system and user prompts into a single input string.
    ///
    /// The default format is compatible with Claude-style prompting:
    /// ```text
    /// System: {system_prompt}
    ///
    /// Human: {user_prompt}
    ///
    /// Assistant:
    /// ```
    fn format_prompt(&self, system: &str, user: &str) -> String {
        if system.is_empty() {
            format!("Human: {}\n\nAssistant:", user)
        } else {
            format!("System: {}\n\nHuman: {}\n\nAssistant:", system, user)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_adapter_builder() {
        let adapter = CliAdapter::new("test-cli")
            .with_timeout(300)
            .with_args(vec!["--arg1".to_string(), "--arg2".to_string()]);

        assert_eq!(adapter.cli_command, "test-cli");
        assert_eq!(adapter.timeout_secs, 300);
        assert_eq!(adapter.extra_args, vec!["--arg1", "--arg2"]);
    }

    #[test]
    fn test_default_timeout() {
        let adapter = CliAdapter::new("test-cli");
        assert_eq!(adapter.timeout_secs, 120);
    }

    #[test]
    fn test_format_prompt_with_system() {
        let adapter = CliAdapter::new("test");
        let formatted = adapter.format_prompt("Be helpful", "Hello");
        assert!(formatted.contains("System: Be helpful"));
        assert!(formatted.contains("Human: Hello"));
        assert!(formatted.contains("Assistant:"));
    }

    #[test]
    fn test_format_prompt_without_system() {
        let adapter = CliAdapter::new("test");
        let formatted = adapter.format_prompt("", "Hello");
        assert!(!formatted.contains("System:"));
        assert!(formatted.contains("Human: Hello"));
        assert!(formatted.contains("Assistant:"));
    }

    #[tokio::test]
    async fn test_check_available_nonexistent_command() {
        let adapter = CliAdapter::new("nonexistent-command-that-does-not-exist-12345");
        assert!(!adapter.check_available().await);
    }

    #[tokio::test]
    async fn test_check_available_existing_command() {
        // `echo` should exist on all systems
        let adapter = CliAdapter::new("echo");
        assert!(adapter.check_available().await);
    }

    #[tokio::test]
    async fn test_execute_with_echo() {
        // Use `cat` to test stdin/stdout piping
        let adapter = CliAdapter::new("cat").with_timeout(5);

        let result = adapter.execute("System prompt", "User message").await;
        assert!(result.is_ok());

        let output = result.unwrap();
        // cat should echo back what we write to stdin
        assert!(output.contains("System prompt"));
        assert!(output.contains("User message"));
    }

    #[tokio::test]
    async fn test_execute_nonexistent_command() {
        let adapter = CliAdapter::new("nonexistent-command-12345");

        let result = adapter.execute("system", "user").await;
        assert!(matches!(result, Err(LLMError::ProcessFailed { .. })));
    }
}
