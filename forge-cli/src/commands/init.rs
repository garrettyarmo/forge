//! Implementation of the `forge init` command.
//!
//! This command generates a default `forge.yaml` configuration file with
//! helpful comments explaining each section.
//!
//! # Usage
//!
//! ```bash
//! # Create forge.yaml in current directory
//! forge init
//!
//! # Pre-fill organization name
//! forge init --org my-company
//!
//! # Specify output path
//! forge init --output config/forge.yaml
//!
//! # Overwrite existing file
//! forge init --force
//! ```

use std::io::Write;
use std::path::Path;
use thiserror::Error;

use crate::output;

/// Errors that can occur during initialization.
#[derive(Debug, Error)]
pub enum InitError {
    /// Configuration file already exists and --force was not specified.
    #[error("Configuration file already exists: {path}. Use --force to overwrite.")]
    FileExists { path: String },

    /// Failed to write the configuration file.
    #[error("Failed to write configuration file: {0}")]
    WriteError(#[from] std::io::Error),
}

/// Default configuration template with comprehensive comments.
const DEFAULT_CONFIG_TEMPLATE: &str = r#"# forge.yaml - Forge configuration file
# Documentation: https://forge.dev/docs/configuration

# ===============================================================================
# REPOSITORY SOURCES
# ===============================================================================
# Define where Forge should discover repositories. You can combine multiple sources.

repos:
  # Option 1: GitHub organization (discovers all repos)
  # Forge will use the GitHub API to list all repositories
  github_org: "{org}"

  # Option 2: Explicit list of GitHub repositories
  # Use this for selective surveying or when you don't want all org repos
  # github_repos:
  #   - "owner/repo-a"
  #   - "owner/repo-b"

  # Option 3: Local filesystem paths
  # Use for testing, air-gapped environments, or monorepos
  # local_paths:
  #   - "./my-local-repo"
  #   - "/absolute/path/to/repo"

  # Exclude patterns (applied to all sources)
  # Glob patterns matched against repo names
  # exclude:
  #   - "*-deprecated"
  #   - "*-archive"
  #   - "fork-*"

# ===============================================================================
# GITHUB CONFIGURATION
# ===============================================================================

github:
  # Environment variable containing your GitHub Personal Access Token
  # The token needs 'repo' scope for private repos, or 'public_repo' for public only
  token_env: "GITHUB_TOKEN"

  # For GitHub Enterprise, set the API base URL
  # api_url: "https://github.mycompany.com/api/v3"

  # Clone method: "https" (default) or "ssh"
  clone_method: "https"

  # Number of concurrent clone operations (default: 4)
  clone_concurrency: 4

# ===============================================================================
# LANGUAGE DETECTION
# ===============================================================================
# Languages are AUTO-DETECTED from file extensions and config files.
# You do NOT need to configure this section for normal usage.
#
# Detection rules:
#   package.json        -> JavaScript/TypeScript
#   requirements.txt    -> Python
#   pyproject.toml      -> Python
#   *.tf                -> Terraform
#
# Only configure if you need to exclude specific languages:

languages:
  exclude: []
  # Example: exclude terraform parsing
  # exclude:
  #   - terraform

# ===============================================================================
# OUTPUT CONFIGURATION
# ===============================================================================

output:
  # Where to save the knowledge graph (relative or absolute path)
  graph_path: ".forge/graph.json"

  # Where to cache cloned repositories
  # Supports ~ for home directory
  cache_path: "~/.forge/repos"

# ===============================================================================
# LLM CONFIGURATION (for business context interview)
# ===============================================================================
# Only used when running: forge survey --business-context

llm:
  # LLM provider CLI to use: claude | gemini | codex
  provider: "claude"

  # Override CLI path if not in PATH
  # cli_path: "/usr/local/bin/claude"

# ===============================================================================
# TOKEN BUDGET
# ===============================================================================
# Default token budget for `forge map` output

token_budget: 8000
"#;

/// Options for the `forge init` command.
#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    /// GitHub organization to pre-fill in the configuration.
    pub org: Option<String>,
    /// Output path for the configuration file.
    pub output: Option<String>,
    /// Whether to overwrite an existing file.
    pub force: bool,
}

/// Run the `forge init` command.
///
/// Creates a new `forge.yaml` configuration file with helpful comments.
///
/// # Arguments
///
/// * `options` - The options for the init command
///
/// # Returns
///
/// Returns `Ok(())` if the file was created successfully, or an error if
/// the file already exists (and `--force` was not specified) or if writing failed.
pub fn run_init(options: InitOptions) -> Result<(), InitError> {
    let output_path = options.output.unwrap_or_else(|| "forge.yaml".to_string());
    let path = Path::new(&output_path);

    // Check if file exists
    if path.exists() && !options.force {
        return Err(InitError::FileExists { path: output_path });
    }

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Generate config content
    let org = options.org.unwrap_or_else(|| "my-org".to_string());
    let content = DEFAULT_CONFIG_TEMPLATE.replace("{org}", &org);

    // Write file
    let mut file = std::fs::File::create(path)?;
    file.write_all(content.as_bytes())?;

    output::success(&format!("Created configuration file: {}", output_path));
    output::info("");
    output::info("Next steps:");
    output::info(&format!(
        "  1. Edit {} to configure your repositories",
        output_path
    ));
    output::info("  2. Set your GitHub token: export GITHUB_TOKEN=<your-token>");
    output::info("  3. Run: forge survey");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_init_creates_file() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("forge.yaml");

        let options = InitOptions {
            org: Some("test-org".to_string()),
            output: Some(output_path.to_string_lossy().to_string()),
            force: false,
        };

        run_init(options).unwrap();

        assert!(output_path.exists());

        // Read and verify content
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("github_org: \"test-org\""));
        assert!(content.contains("token_env: \"GITHUB_TOKEN\""));
        assert!(content.contains("graph_path: \".forge/graph.json\""));
    }

    #[test]
    fn test_init_default_org() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("forge.yaml");

        let options = InitOptions {
            org: None,
            output: Some(output_path.to_string_lossy().to_string()),
            force: false,
        };

        run_init(options).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("github_org: \"my-org\""));
    }

    #[test]
    fn test_init_fails_if_exists() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("forge.yaml");

        // Create existing file
        std::fs::write(&output_path, "existing content").unwrap();

        let options = InitOptions {
            org: None,
            output: Some(output_path.to_string_lossy().to_string()),
            force: false,
        };

        let result = run_init(options);
        assert!(matches!(result, Err(InitError::FileExists { .. })));
    }

    #[test]
    fn test_init_force_overwrites() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("forge.yaml");

        // Create existing file
        std::fs::write(&output_path, "existing content").unwrap();

        let options = InitOptions {
            org: Some("new-org".to_string()),
            output: Some(output_path.to_string_lossy().to_string()),
            force: true,
        };

        run_init(options).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("github_org: \"new-org\""));
        assert!(!content.contains("existing content"));
    }

    #[test]
    fn test_init_creates_parent_directories() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("subdir").join("nested").join("forge.yaml");

        let options = InitOptions {
            org: None,
            output: Some(output_path.to_string_lossy().to_string()),
            force: false,
        };

        run_init(options).unwrap();
        assert!(output_path.exists());
    }

    #[test]
    fn test_generated_config_is_valid_yaml() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("forge.yaml");

        let options = InitOptions {
            org: Some("valid-org".to_string()),
            output: Some(output_path.to_string_lossy().to_string()),
            force: false,
        };

        run_init(options).unwrap();

        // Try to parse as YAML
        let content = std::fs::read_to_string(&output_path).unwrap();
        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&content);
        assert!(parsed.is_ok(), "Generated config should be valid YAML");
    }

    #[test]
    fn test_generated_config_loads_as_forge_config() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("forge.yaml");

        let options = InitOptions {
            org: Some("loadable-org".to_string()),
            output: Some(output_path.to_string_lossy().to_string()),
            force: false,
        };

        run_init(options).unwrap();

        // Try to load as ForgeConfig
        let config = crate::config::ForgeConfig::load_from_path(&output_path);
        assert!(
            config.is_ok(),
            "Generated config should load as ForgeConfig"
        );

        let config = config.unwrap();
        assert_eq!(config.repos.github_org, Some("loadable-org".to_string()));
    }
}
