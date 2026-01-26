# Extending Forge: Adding a New LLM Provider

This guide walks you through adding support for a new LLM CLI adapter to Forge's business context interview system.

## Overview

Forge uses a trait-based adapter architecture where each LLM provider implements the [`LLMProvider`](../forge-llm/src/provider.rs) trait. The adapter system is:

- **CLI-Based**: Shells out to coding agent CLIs—no direct API calls
- **Auth-Free**: Leverages user's existing CLI authentication (no API keys in forge.yaml)
- **Extensible**: New providers require implementing a single trait
- **Timeout-Safe**: Built-in timeout management for long-running requests

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Provider Factory                             │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐            │
│  │ ClaudeAdapter│ │ GeminiAdapter│ │ CodexAdapter │  + Yours   │
│  └──────┬───────┘ └──────┬───────┘ └──────┬───────┘            │
└─────────┼────────────────┼────────────────┼─────────────────────┘
          │                │                │
          ▼                ▼                ▼
   ┌─────────────────────────────────────────────────────┐
   │                    LLMProvider Trait                 │
   │  - name() -> &str                                   │
   │  - is_available() -> bool                           │
   │  - prompt(system, user) -> Result<String>           │
   │  - prompt_with_history(system, history, user)       │
   └─────────────────────────────────────────────────────┘
                            │
                            ▼
   ┌─────────────────────────────────────────────────────┐
   │                    CliAdapter Base                   │
   │  - check_available() - CLI existence via 'which'    │
   │  - execute() - Subprocess stdin/stdout piping       │
   │  - format_prompt() - Default prompt formatting      │
   │  - Timeout management                               │
   └─────────────────────────────────────────────────────┘
                            │
                            ▼
   ┌─────────────────────────────────────────────────────┐
   │                  Coding Agent CLI                    │
   │  (claude, gemini, codex, etc.)                      │
   │  - Handles API authentication                        │
   │  - Manages model selection                          │
   │  - Processes prompts via stdin/stdout               │
   └─────────────────────────────────────────────────────┘
```

---

## Prerequisites

Before adding a new provider, you'll need:

1. **Rust development environment** (1.85+)
2. **Understanding of your CLI's interface**:
   - How to invoke the CLI in non-interactive mode
   - How prompts are passed (stdin, arguments, etc.)
   - How responses are returned (stdout, JSON, etc.)
3. **Access to the CLI for testing** (or ability to mock it)

---

## Step-by-Step Guide

### Step 1: Create the Adapter File

Create a new file at `forge-llm/src/adapters/your_provider.rs`:

```rust
//! YourProvider CLI adapter for Forge.
//!
//! This adapter shells out to the `your-cli` command-line tool
//! to interact with YourProvider AI. It leverages the user's existing CLI
//! authentication rather than requiring API keys in forge.yaml.
//!
//! # Requirements
//!
//! The `your-cli` CLI must be installed and available in PATH.
//! Install via: `npm install -g @your-org/cli` (or similar)
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_llm::adapters::your_provider::YourProviderAdapter;
//! use forge_llm::provider::LLMProvider;
//!
//! let adapter = YourProviderAdapter::new(None);
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

/// Default timeout in seconds for YourProvider CLI operations.
const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// Adapter for the YourProvider CLI.
///
/// This adapter communicates with YourProvider through a command-line tool,
/// which handles authentication and API communication internally.
///
/// # Features
///
/// - Supports system prompts combined with user prompts
/// - Handles conversation history for multi-turn interactions
/// - Configurable timeout (default: 180 seconds)
#[derive(Debug, Clone)]
pub struct YourProviderAdapter {
    /// The underlying CLI adapter with common functionality.
    base: CliAdapter,
}

impl YourProviderAdapter {
    /// Create a new YourProvider adapter.
    ///
    /// # Arguments
    /// * `cli_path` - Optional custom path to the CLI.
    ///   If `None`, uses "your-cli" (must be in PATH).
    pub fn new(cli_path: Option<String>) -> Self {
        let cmd = cli_path.unwrap_or_else(|| "your-cli".to_string());
        Self {
            base: CliAdapter::new(cmd)
                .with_timeout(DEFAULT_TIMEOUT_SECS)
                // Add any provider-specific CLI flags here:
                // .with_args(vec!["--non-interactive".to_string()]),
        }
    }

    /// Create a new adapter with custom timeout.
    pub fn with_timeout(cli_path: Option<String>, timeout_secs: u64) -> Self {
        let mut adapter = Self::new(cli_path);
        adapter.base.timeout_secs = timeout_secs;
        adapter
    }

    /// Execute the CLI with a prompt.
    async fn execute_prompt(&self, system: &str, user: &str) -> LLMResult<String> {
        let mut cmd = Command::new(&self.base.cli_command);

        // Add base arguments
        for arg in &self.base.extra_args {
            cmd.arg(arg);
        }

        // Configure stdio for stdin/stdout communication
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
            let full_prompt = self.format_prompt(system, user);
            stdin
                .write_all(full_prompt.as_bytes())
                .await
                .map_err(|e| LLMError::ProcessFailed {
                    cmd: self.base.cli_command.clone(),
                    message: format!("Failed to write to stdin: {}", e),
                })?;
            drop(stdin); // Close stdin to signal end of input
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

    /// Format the prompt for YourProvider CLI.
    ///
    /// Customize this method based on your CLI's expected input format.
    fn format_prompt(&self, system: &str, user: &str) -> String {
        if system.is_empty() {
            user.to_string()
        } else {
            // Adjust format to match your CLI's expectations
            format!("System: {}\n\nUser: {}", system.trim(), user.trim())
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
                    context.push_str("Assistant: ");
                    context.push_str(&msg.content);
                    context.push_str("\n\n");
                }
            }
        }

        context
    }

    /// Clean up the response from the CLI.
    fn clean_response(&self, response: &str) -> String {
        response.trim().to_string()
    }
}

#[async_trait]
impl LLMProvider for YourProviderAdapter {
    fn name(&self) -> &str {
        "your-provider"  // This must match the name used in forge.yaml
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
        let history_context = self.format_history(history);

        let full_user = if history_context.is_empty() {
            user.to_string()
        } else {
            format!("{}\nUser: {}", history_context.trim(), user)
        };

        self.execute_prompt(system, &full_user).await
    }
}
```

### Step 2: Register the Adapter

Update `forge-llm/src/adapters/mod.rs` to include and export your adapter:

```rust
// Add module declaration
pub mod your_provider;

// Add re-export
pub use your_provider::YourProviderAdapter;
```

### Step 3: Update the Provider Factory

Update `forge-llm/src/lib.rs` to support your provider:

```rust
// Add re-export at the top with other adapters
pub use adapters::YourProviderAdapter;

// In create_provider(), add your provider case:
pub fn create_provider(config: &LLMConfig) -> Result<Box<dyn LLMProvider>, LLMError> {
    match config.provider.as_str() {
        "claude" => Ok(Box::new(ClaudeAdapter::new(config.cli_path.clone()))),
        "gemini" => Ok(Box::new(GeminiAdapter::new(config.cli_path.clone()))),
        "codex" => Ok(Box::new(CodexAdapter::new(config.cli_path.clone()))),
        "your-provider" => Ok(Box::new(YourProviderAdapter::new(config.cli_path.clone()))),  // Add this
        other => Err(LLMError::NotConfigured(format!(
            "Unknown provider: '{}'. Supported providers: claude, gemini, codex, your-provider",
            other
        ))),
    }
}
```

### Step 4: Write Tests

Add comprehensive tests to your adapter file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default_command() {
        let adapter = YourProviderAdapter::new(None);
        assert_eq!(adapter.base.cli_command, "your-cli");
        assert_eq!(adapter.base.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn test_new_custom_path() {
        let adapter = YourProviderAdapter::new(Some("/custom/path/your-cli".to_string()));
        assert_eq!(adapter.base.cli_command, "/custom/path/your-cli");
    }

    #[test]
    fn test_with_timeout() {
        let adapter = YourProviderAdapter::with_timeout(None, 300);
        assert_eq!(adapter.base.timeout_secs, 300);
    }

    #[test]
    fn test_format_prompt_with_system() {
        let adapter = YourProviderAdapter::new(None);
        let formatted = adapter.format_prompt("Be helpful", "Hello");
        assert!(formatted.contains("System: Be helpful"));
        assert!(formatted.contains("User: Hello"));
    }

    #[test]
    fn test_format_prompt_without_system() {
        let adapter = YourProviderAdapter::new(None);
        let formatted = adapter.format_prompt("", "Hello");
        assert_eq!(formatted, "Hello");
    }

    #[test]
    fn test_format_history_empty() {
        let adapter = YourProviderAdapter::new(None);
        let history: Vec<Message> = vec![];
        let formatted = adapter.format_history(&history);
        assert!(formatted.is_empty());
    }

    #[test]
    fn test_format_history_with_messages() {
        let adapter = YourProviderAdapter::new(None);
        let history = vec![
            Message::user("What is 2+2?"),
            Message::assistant("2+2 equals 4."),
        ];

        let formatted = adapter.format_history(&history);
        assert!(formatted.contains("User: What is 2+2?"));
        assert!(formatted.contains("Assistant: 2+2 equals 4."));
    }

    #[test]
    fn test_clean_response() {
        let adapter = YourProviderAdapter::new(None);
        assert_eq!(adapter.clean_response("  Hello  "), "Hello");
        assert_eq!(adapter.clean_response("\n\nHello\n\n"), "Hello");
    }

    #[test]
    fn test_provider_name() {
        let adapter = YourProviderAdapter::new(None);
        assert_eq!(adapter.name(), "your-provider");
    }

    #[tokio::test]
    async fn test_is_available_not_installed() {
        let adapter = YourProviderAdapter::new(Some("nonexistent-cli-12345".to_string()));
        assert!(!adapter.is_available().await);
    }

    #[tokio::test]
    async fn test_prompt_cli_not_found() {
        let adapter = YourProviderAdapter::new(Some("nonexistent-cli-12345".to_string()));

        let result = adapter.prompt("system", "user").await;
        assert!(matches!(
            result,
            Err(LLMError::CliNotFound(_)) | Err(LLMError::ProcessFailed { .. })
        ));
    }
}
```

---

## LLMProvider Trait Reference

Your adapter must implement the `LLMProvider` trait:

### name()

Returns the provider identifier used in `forge.yaml`:

```rust
fn name(&self) -> &str {
    "your-provider"  // Users will specify llm.provider: "your-provider"
}
```

### is_available()

Checks if the CLI exists in the system PATH:

```rust
async fn is_available(&self) -> bool {
    self.base.check_available().await
}
```

This uses `which` (Unix) or `where` (Windows) to verify the CLI exists.

### prompt()

Sends a single prompt and returns the response:

```rust
async fn prompt(&self, system: &str, user: &str) -> LLMResult<String> {
    self.execute_prompt(system, user).await
}
```

**Arguments:**
- `system` - System prompt setting context and instructions
- `user` - User message/question

### prompt_with_history()

Sends a prompt with conversation history for multi-turn interactions:

```rust
async fn prompt_with_history(
    &self,
    system: &str,
    history: &[Message],
    user: &str,
) -> LLMResult<String>
```

The default implementation ignores history. Override this if your provider supports conversation context.

---

## Error Types

Your adapter should use the appropriate `LLMError` variants:

| Error Type | When to Use |
|------------|-------------|
| `LLMError::CliNotFound(String)` | CLI command not found in PATH |
| `LLMError::ProcessFailed { cmd, message }` | Failed to spawn or communicate with CLI |
| `LLMError::NonZeroExit { code, stderr }` | CLI returned non-zero exit code |
| `LLMError::Timeout(u64)` | CLI didn't respond within timeout |
| `LLMError::InvalidOutput(String)` | Response couldn't be parsed (e.g., not UTF-8) |

---

## CliAdapter Base Class

The `CliAdapter` base class provides common functionality:

### Creating an Instance

```rust
use crate::adapters::base::CliAdapter;

let adapter = CliAdapter::new("your-cli")
    .with_timeout(180)
    .with_args(vec!["--flag1".to_string(), "--flag2".to_string()]);
```

### Methods

| Method | Description |
|--------|-------------|
| `new(cmd)` | Create adapter for given CLI command |
| `with_timeout(secs)` | Set timeout in seconds (builder pattern) |
| `with_args(args)` | Set additional CLI arguments (builder pattern) |
| `check_available()` | Check if CLI exists in PATH |
| `execute(system, user)` | Execute CLI with prompts (used by base, but you'll likely override) |

---

## Best Practices

### 1. Handle CLI-Specific Requirements

Different CLIs have different requirements. Document them clearly:

```rust
//! # Requirements
//!
//! The `your-cli` CLI must be installed and available in PATH.
//! - Requires authentication via `your-cli login` first
//! - Requires `--non-interactive` flag for piped input
```

### 2. Format Prompts Appropriately

Match your CLI's expected input format:

```rust
// Claude style: [System: ...] followed by user message
fn format_prompt(&self, system: &str, user: &str) -> String {
    if system.is_empty() {
        user.to_string()
    } else {
        format!("[System: {}]\n\n{}", system.trim(), user.trim())
    }
}

// Gemini style: Simple concatenation
fn format_prompt(&self, system: &str, user: &str) -> String {
    if system.is_empty() {
        user.to_string()
    } else {
        format!("{}\n\n{}", system.trim(), user.trim())
    }
}

// Codex style: Labeled sections
fn format_prompt(&self, system: &str, user: &str) -> String {
    if system.is_empty() {
        user.to_string()
    } else {
        format!("System: {}\n\nUser: {}", system.trim(), user.trim())
    }
}
```

### 3. Clean Response Output

Remove CLI-specific formatting from responses:

```rust
fn clean_response(&self, response: &str) -> String {
    let cleaned = response.trim();

    // Remove common prefixes your CLI might add
    let cleaned = cleaned.strip_prefix("Assistant: ").unwrap_or(cleaned);

    // Remove trailing markers
    let cleaned = cleaned.strip_suffix("<|end|>").unwrap_or(cleaned);

    cleaned.to_string()
}
```

### 4. Use Appropriate Timeouts

LLM operations can be slow. Set generous but reasonable timeouts:

```rust
const DEFAULT_TIMEOUT_SECS: u64 = 180;  // 3 minutes is reasonable

// For complex operations, allow custom timeouts
pub fn with_timeout(cli_path: Option<String>, timeout_secs: u64) -> Self {
    let mut adapter = Self::new(cli_path);
    adapter.base.timeout_secs = timeout_secs;
    adapter
}
```

### 5. Handle Non-Interactive Mode

Most CLIs need special flags for non-interactive use:

```rust
Self {
    base: CliAdapter::new(cmd)
        .with_args(vec![
            "--print".to_string(),           // Claude: print output only
            "--non-interactive".to_string(), // Common flag
            "--no-pager".to_string(),        // Disable paging
        ]),
}
```

### 6. Test Without the Actual CLI

Use mock tests that don't require the real CLI:

```rust
#[tokio::test]
async fn test_cli_not_found() {
    let adapter = YourProviderAdapter::new(Some("nonexistent-cli-12345".to_string()));
    let result = adapter.prompt("system", "user").await;
    assert!(matches!(result, Err(LLMError::CliNotFound(_)) | Err(LLMError::ProcessFailed { .. })));
}

// For integration tests, check availability first
#[tokio::test]
#[ignore = "requires your-cli to be installed"]
async fn test_real_prompt() {
    let adapter = YourProviderAdapter::new(None);
    if !adapter.is_available().await {
        return;  // Skip if not installed
    }
    let result = adapter.prompt("Be brief", "Say hello").await;
    assert!(result.is_ok());
}
```

---

## Testing Guidelines

### Unit Test Coverage

Your adapter should have tests for:

1. **Construction** - Default command, custom path, timeout
2. **Prompt formatting** - With/without system prompt
3. **History formatting** - Empty history, multiple messages
4. **Response cleaning** - Whitespace, prefixes, markers
5. **Error handling** - CLI not found, process failures
6. **Provider name** - Returns correct identifier

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Run only your adapter's tests
cargo test --package forge-llm your_provider

# Run with output
cargo test --package forge-llm your_provider -- --nocapture
```

---

## Configuration

Once your adapter is registered, users can configure it in `forge.yaml`:

```yaml
# forge.yaml
llm:
  provider: "your-provider"  # Matches adapter.name()

  # Optional: custom CLI path
  # cli_path: "/custom/path/your-cli"
```

Or via environment variable:

```bash
export FORGE_LLM_PROVIDER=your-provider
```

---

## Checklist

Before submitting your adapter:

- [ ] Adapter implements `LLMProvider` trait correctly
- [ ] Adapter is registered in `adapters/mod.rs`
- [ ] Provider factory updated in `lib.rs`
- [ ] Provider name is documented and consistent
- [ ] Unit tests cover all methods
- [ ] Error handling uses appropriate `LLMError` variants
- [ ] CLI-specific flags documented
- [ ] Module documentation explains requirements
- [ ] All tests pass: `cargo test --workspace`
- [ ] Code is formatted: `cargo fmt`
- [ ] No clippy warnings: `cargo clippy --workspace -- -D warnings`

---

## Examples

For complete implementation examples, see:

- **Claude CLI**: [`forge-llm/src/adapters/claude.rs`](../forge-llm/src/adapters/claude.rs) - Uses `--print` flag, Claude-style formatting
- **Gemini CLI**: [`forge-llm/src/adapters/gemini.rs`](../forge-llm/src/adapters/gemini.rs) - Simple prompt format
- **Codex CLI**: [`forge-llm/src/adapters/codex.rs`](../forge-llm/src/adapters/codex.rs) - OpenAI-style formatting
- **Base Adapter**: [`forge-llm/src/adapters/base.rs`](../forge-llm/src/adapters/base.rs) - Common functionality

---

## See Also

- [CLI Reference](cli-reference.md) - Command-line options including `--business-context`
- [Configuration Reference](configuration.md) - Full `forge.yaml` schema including `llm` section
- [Extending Parsers](extending-parsers.md) - Adding new language parsers
