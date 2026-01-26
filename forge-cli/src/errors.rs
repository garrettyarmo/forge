use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("Configuration file not found at {path}")]
    ConfigNotFound { path: String },

    #[error("GitHub token not found in environment")]
    GitHubTokenMissing,

    #[error("Failed to clone repository {repo}: {reason}")]
    CloneFailed { repo: String, reason: String },

    #[error("Parser failed for {file}: {reason}")]
    ParseError { file: String, reason: String },

    #[error("Graph file not found: {path}")]
    GraphNotFound { path: String },
}

impl ForgeError {
    /// Get a suggestion for how to fix this error
    #[allow(dead_code)]
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            ForgeError::ConfigNotFound { .. } => Some(
                "Run 'forge init' to create a configuration file, or use --config to specify a path",
            ),
            ForgeError::GitHubTokenMissing => Some(
                "Set the GITHUB_TOKEN environment variable with a personal access token:\n  export GITHUB_TOKEN=ghp_xxxx",
            ),
            ForgeError::CloneFailed { .. } => Some(
                "Check your network connection and GitHub token permissions.\nFor private repos, ensure your token has 'repo' scope.",
            ),
            ForgeError::ParseError { .. } => {
                Some("This file may have syntax errors. The survey will continue with other files.")
            }
            ForgeError::GraphNotFound { .. } => {
                Some("Run 'forge survey' first to build the knowledge graph.")
            }
        }
    }

    /// Format error with suggestion for CLI output
    #[allow(dead_code)]
    pub fn format_for_cli(&self) -> String {
        let mut output = format!("Error: {}", self);

        if let Some(suggestion) = self.suggestion() {
            output.push_str(&format!("\n\nSuggestion: {}", suggestion));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_not_found_error() {
        let error = ForgeError::ConfigNotFound {
            path: "./forge.yaml".to_string(),
        };
        assert!(error.to_string().contains("forge.yaml"));
        assert!(error.suggestion().unwrap().contains("forge init"));
    }

    #[test]
    fn test_github_token_missing_error() {
        let error = ForgeError::GitHubTokenMissing;
        assert!(error.to_string().contains("token"));
        assert!(error.suggestion().unwrap().contains("GITHUB_TOKEN"));
    }

    #[test]
    fn test_clone_failed_error() {
        let error = ForgeError::CloneFailed {
            repo: "org/repo".to_string(),
            reason: "network timeout".to_string(),
        };
        assert!(error.to_string().contains("org/repo"));
        assert!(error.suggestion().unwrap().contains("network"));
    }

    #[test]
    fn test_parse_error() {
        let error = ForgeError::ParseError {
            file: "test.js".to_string(),
            reason: "syntax error".to_string(),
        };
        assert!(error.to_string().contains("test.js"));
        assert!(error.suggestion().unwrap().contains("syntax"));
    }

    #[test]
    fn test_graph_not_found_error() {
        let error = ForgeError::GraphNotFound {
            path: ".forge/graph.json".to_string(),
        };
        assert!(error.to_string().contains("graph.json"));
        assert!(error.suggestion().unwrap().contains("forge survey"));
    }

    #[test]
    fn test_format_for_cli() {
        let error = ForgeError::GitHubTokenMissing;
        let formatted = error.format_for_cli();
        assert!(formatted.contains("Error:"));
        assert!(formatted.contains("Suggestion:"));
        assert!(formatted.contains("GITHUB_TOKEN"));
    }
}
