//! Parser trait and discovery types for Forge survey.
//!
//! This module defines the core abstractions for language-specific parsers:
//! - [`Parser`] trait that all language parsers implement
//! - [`Discovery`] enum representing different types of code patterns found
//! - Supporting types for service, import, API, database, queue, and cloud resource discoveries
//!
//! The parser architecture is designed to be:
//! - **Deterministic**: Uses tree-sitter AST parsing only, no LLM calls
//! - **Extensible**: New languages can be added by implementing the Parser trait
//! - **Resilient**: Parser failures don't crash the entire survey

use std::any::Any;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during parsing.
#[derive(Debug, Error)]
pub enum ParserError {
    /// Failed to read a file from disk.
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to parse a file (e.g., syntax error).
    #[error("Failed to parse file: {path}")]
    ParseFailed {
        /// The path to the file that failed to parse.
        path: String,
    },

    /// The file type is not supported by this parser.
    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(String),

    /// An error occurred in the tree-sitter parsing library.
    #[error("Tree-sitter error: {0}")]
    TreeSitterError(String),
}

/// A discovery made by a parser during code analysis.
///
/// Each variant represents a different type of code pattern that Forge
/// tracks to build the knowledge graph.
#[derive(Debug, Clone, PartialEq)]
pub enum Discovery {
    /// A service entry point was found (e.g., from package.json or main.py).
    Service(ServiceDiscovery),

    /// An import/require statement was found.
    Import(ImportDiscovery),

    /// An HTTP API call was detected (e.g., axios, fetch, requests).
    ApiCall(ApiCallDiscovery),

    /// A database access was detected (e.g., DynamoDB, PostgreSQL).
    DatabaseAccess(DatabaseAccessDiscovery),

    /// A queue/message operation was detected (e.g., SQS, SNS).
    QueueOperation(QueueOperationDiscovery),

    /// A cloud resource usage was detected (e.g., S3, Lambda).
    CloudResourceUsage(CloudResourceDiscovery),
}

/// Details about a discovered service entry point.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServiceDiscovery {
    /// Service name (usually from package.json name field or directory name).
    pub name: String,

    /// Programming language (e.g., "javascript", "python", "typescript").
    pub language: String,

    /// Framework detected (e.g., "express", "fastify", "flask").
    pub framework: Option<String>,

    /// Entry point file (e.g., "index.js", "main.py").
    pub entry_point: String,

    /// Source file where the service was detected.
    pub source_file: String,

    /// Line number in the source file.
    pub source_line: u32,
}

/// Details about an import/require statement.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDiscovery {
    /// The module being imported (e.g., "express", "@aws-sdk/client-dynamodb").
    pub module: String,

    /// Whether this is a relative import (starts with "./" or "../").
    pub is_relative: bool,

    /// Specific items imported if destructured (e.g., ["DynamoDB", "S3"]).
    pub imported_items: Vec<String>,

    /// Source file containing the import.
    pub source_file: String,

    /// Line number of the import statement.
    pub source_line: u32,
}

/// Details about an HTTP API call.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiCallDiscovery {
    /// The target URL, service name, or endpoint pattern.
    pub target: String,

    /// HTTP method if known (GET, POST, PUT, DELETE, etc.).
    pub method: Option<String>,

    /// How the call was detected (e.g., "axios", "fetch", "requests").
    pub detection_method: String,

    /// Source file containing the API call.
    pub source_file: String,

    /// Line number of the API call.
    pub source_line: u32,
}

/// Details about a database access pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseAccessDiscovery {
    /// Database type (e.g., "dynamodb", "postgresql", "mongodb").
    pub db_type: String,

    /// Table or collection name if detected.
    pub table_name: Option<String>,

    /// The type of database operation.
    pub operation: DatabaseOperation,

    /// How the access was detected (e.g., "aws-sdk", "boto3", "pg").
    pub detection_method: String,

    /// Source file containing the database access.
    pub source_file: String,

    /// Line number of the database access.
    pub source_line: u32,
}

/// Types of database operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseOperation {
    /// Read-only operation (SELECT, get, query, scan).
    Read,
    /// Write-only operation (INSERT, put, delete).
    Write,
    /// Both read and write (UPDATE, upsert).
    ReadWrite,
    /// Operation type could not be determined.
    Unknown,
}

/// Details about a queue/message operation.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueOperationDiscovery {
    /// Queue type (e.g., "sqs", "sns", "eventbridge", "rabbitmq").
    pub queue_type: String,

    /// Queue or topic name/ARN if detected.
    pub queue_name: Option<String>,

    /// The type of queue operation.
    pub operation: QueueOperationType,

    /// Source file containing the queue operation.
    pub source_file: String,

    /// Line number of the queue operation.
    pub source_line: u32,
}

/// Types of queue/message operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOperationType {
    /// Publishing/sending a message.
    Publish,
    /// Subscribing/receiving messages.
    Subscribe,
    /// Operation type could not be determined.
    Unknown,
}

/// Details about cloud resource usage.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudResourceDiscovery {
    /// Resource type (e.g., "s3", "lambda", "secretsmanager").
    pub resource_type: String,

    /// Resource name or ARN if detected.
    pub resource_name: Option<String>,

    /// Source file containing the resource usage.
    pub source_file: String,

    /// Line number of the resource usage.
    pub source_line: u32,
}

/// Trait for language-specific parsers.
///
/// Implement this trait to add support for a new programming language.
/// The parser is responsible for:
/// 1. Identifying which file extensions it can handle
/// 2. Parsing individual files to extract discoveries
/// 3. Optionally overriding the default repository walking behavior
///
/// # Example
///
/// ```ignore
/// pub struct MyLangParser {
///     // parser state
/// }
///
/// impl Parser for MyLangParser {
///     fn supported_extensions(&self) -> &[&str] {
///         &["ml", "mli"]
///     }
///
///     fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
///         // Parse the file and return discoveries
///         Ok(vec![])
///     }
/// }
/// ```
pub trait Parser: Send + Sync {
    /// Returns a reference to `Any` for downcasting to concrete parser types.
    ///
    /// This is needed for accessing parser-specific methods like
    /// `JavaScriptParser::parse_package_json` or `PythonParser::parse_project_config`.
    fn as_any(&self) -> &dyn Any;

    /// Returns the file extensions this parser handles (without the dot).
    ///
    /// For example: `&["js", "jsx", "ts", "tsx"]` for JavaScript/TypeScript.
    fn supported_extensions(&self) -> &[&str];

    /// Parse a single file and return all discoveries found.
    ///
    /// # Arguments
    /// * `path` - Path to the file being parsed (for error messages and source tracking)
    /// * `content` - The file content as a string
    ///
    /// # Returns
    /// A vector of discoveries found in the file, or an error if parsing failed.
    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError>;

    /// Parse an entire repository and return all discoveries.
    ///
    /// The default implementation walks the directory tree, filters by supported
    /// extensions, and calls `parse_file` for each matching file. It skips
    /// common directories like `node_modules`, `.git`, `target`, etc.
    ///
    /// Override this method if you need custom repository traversal logic.
    ///
    /// # Arguments
    /// * `repo_path` - Path to the root of the repository
    ///
    /// # Returns
    /// A vector of all discoveries found in the repository.
    fn parse_repo(&self, repo_path: &Path) -> Result<Vec<Discovery>, ParserError> {
        let mut all_discoveries = Vec::new();
        let extensions = self.supported_extensions();

        for entry in walkdir::WalkDir::new(repo_path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e.file_name().to_str().unwrap_or("")))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::debug!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            if !extensions.contains(&ext) {
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("Failed to read file {}: {}", path.display(), e);
                    continue; // Skip unreadable files (binary, permissions, etc.)
                }
            };

            match self.parse_file(path, &content) {
                Ok(discoveries) => all_discoveries.extend(discoveries),
                Err(e) => {
                    // Log but continue - don't fail entire survey for one file
                    tracing::warn!("Failed to parse {}: {}", path.display(), e);
                }
            }
        }

        Ok(all_discoveries)
    }
}

/// Directories to skip during repository traversal.
///
/// These are common directories that don't contain source code we want to analyze,
/// or that would significantly slow down parsing.
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        // JavaScript/Node.js
        "node_modules"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | "coverage"
            | ".turbo"
            | ".parcel-cache"
            // Python
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | "venv"
            | ".venv"
            | "env"
            | ".tox"
            | ".nox"
            | "*.egg-info"
            // Rust
            | "target"
            // General
            | ".git"
            | ".svn"
            | ".hg"
            | "vendor"
            | ".idea"
            | ".vscode"
            | ".github"
            // Build outputs
            | "out"
            | "output"
            | "bin"
            | "obj"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ignored_dir() {
        assert!(is_ignored_dir("node_modules"));
        assert!(is_ignored_dir(".git"));
        assert!(is_ignored_dir("target"));
        assert!(is_ignored_dir("__pycache__"));
        assert!(is_ignored_dir("venv"));

        assert!(!is_ignored_dir("src"));
        assert!(!is_ignored_dir("lib"));
        assert!(!is_ignored_dir("app"));
        assert!(!is_ignored_dir("services"));
    }

    #[test]
    fn test_discovery_equality() {
        let d1 = Discovery::Import(ImportDiscovery {
            module: "express".to_string(),
            is_relative: false,
            imported_items: vec![],
            source_file: "test.js".to_string(),
            source_line: 1,
        });

        let d2 = Discovery::Import(ImportDiscovery {
            module: "express".to_string(),
            is_relative: false,
            imported_items: vec![],
            source_file: "test.js".to_string(),
            source_line: 1,
        });

        assert_eq!(d1, d2);
    }

    #[test]
    fn test_database_operation_enum() {
        assert_ne!(DatabaseOperation::Read, DatabaseOperation::Write);
        assert_eq!(DatabaseOperation::Unknown, DatabaseOperation::Unknown);
    }

    #[test]
    fn test_queue_operation_enum() {
        assert_ne!(QueueOperationType::Publish, QueueOperationType::Subscribe);
        assert_eq!(QueueOperationType::Unknown, QueueOperationType::Unknown);
    }

    #[test]
    fn test_service_discovery_creation() {
        let service = ServiceDiscovery {
            name: "my-service".to_string(),
            language: "javascript".to_string(),
            framework: Some("express".to_string()),
            entry_point: "src/index.js".to_string(),
            source_file: "package.json".to_string(),
            source_line: 1,
        };

        assert_eq!(service.name, "my-service");
        assert_eq!(service.framework, Some("express".to_string()));
    }

    #[test]
    fn test_parser_error_display() {
        let err = ParserError::ParseFailed {
            path: "/some/file.js".to_string(),
        };
        assert!(err.to_string().contains("file.js"));

        let err = ParserError::TreeSitterError("query failed".to_string());
        assert!(err.to_string().contains("query failed"));
    }

    // Test that a mock parser can implement the trait
    struct MockParser;

    impl Parser for MockParser {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn supported_extensions(&self) -> &[&str] {
            &["mock"]
        }

        fn parse_file(&self, path: &Path, _content: &str) -> Result<Vec<Discovery>, ParserError> {
            Ok(vec![Discovery::Import(ImportDiscovery {
                module: "mock-module".to_string(),
                is_relative: false,
                imported_items: vec![],
                source_file: path.to_string_lossy().to_string(),
                source_line: 1,
            })])
        }
    }

    #[test]
    fn test_mock_parser_implements_trait() {
        let parser = MockParser;
        assert_eq!(parser.supported_extensions(), &["mock"]);

        let discoveries = parser
            .parse_file(Path::new("test.mock"), "content")
            .unwrap();
        assert_eq!(discoveries.len(), 1);
        assert!(matches!(discoveries[0], Discovery::Import(_)));
    }
}
