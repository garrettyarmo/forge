# Extending Forge: Adding a New Language Parser

This guide walks you through adding support for a new programming language to Forge's survey system.

## Overview

Forge uses a trait-based parser architecture where each language parser implements the [`Parser`](../forge-survey/src/parser/traits.rs) trait. The parser system is:

- **Deterministic**: Uses tree-sitter AST parsing only—no LLM calls
- **Extensible**: New languages require implementing a single trait
- **Resilient**: Parser failures don't crash the entire survey

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     ParserRegistry                               │
│  ┌─────────┐ ┌─────────┐ ┌───────────┐ ┌────────────────────┐  │
│  │JavaScript│ │ Python  │ │ Terraform │ │ CloudFormation/SAM │  │
│  │  Parser │ │ Parser  │ │  Parser   │ │      Parser        │  │
│  └────┬────┘ └────┬────┘ └─────┬─────┘ └─────────┬──────────┘  │
└───────┼───────────┼────────────┼─────────────────┼─────────────┘
        │           │            │                 │
        ▼           ▼            ▼                 ▼
   ┌─────────────────────────────────────────────────────┐
   │                    Parser Trait                      │
   │  - supported_extensions()                            │
   │  - parse_file(path, content) -> Vec<Discovery>       │
   │  - parse_repo(repo_path) -> Vec<Discovery>  (default)│
   └─────────────────────────────────────────────────────┘
                            │
                            ▼
   ┌─────────────────────────────────────────────────────┐
   │                   Discovery Types                    │
   │  - Service         - DatabaseAccess                  │
   │  - Import          - QueueOperation                  │
   │  - ApiCall         - CloudResourceUsage              │
   └─────────────────────────────────────────────────────┘
```

---

## Prerequisites

Before adding a new parser, you'll need:

1. **Rust development environment** (1.85+)
2. **tree-sitter grammar** for your language (if using AST parsing)
3. **Understanding of your language's patterns** for AWS SDK, HTTP clients, etc.

---

## Step-by-Step Guide

### Step 1: Add Dependencies

Add the tree-sitter grammar for your language to `forge-survey/Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...
tree-sitter = "0.24"
tree-sitter-your-language = "0.x"  # Add your language grammar
```

**Note**: If a tree-sitter grammar doesn't exist for your language, you can still implement a parser using regex or other parsing approaches. The `Parser` trait doesn't require tree-sitter—it just needs to return `Vec<Discovery>` from `parse_file()`.

### Step 2: Create the Parser File

Create a new file at `forge-survey/src/parser/your_language.rs`:

```rust
//! Your Language parser for Forge survey.
//!
//! This parser uses tree-sitter to analyze YourLanguage files and detect:
//! - Import statements
//! - AWS SDK usage patterns
//! - HTTP client calls
//! - Database operations
//!
//! The parser is deterministic - it uses only AST analysis with no LLM calls.

use super::traits::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError, QueueOperationDiscovery, QueueOperationType,
    ServiceDiscovery,
};
use std::any::Any;
use std::path::Path;
use tree_sitter::{Language, Parser as TSParser};

/// Parser for YourLanguage files.
pub struct YourLanguageParser {
    language: Language,
}

impl YourLanguageParser {
    /// Create a new parser instance.
    ///
    /// # Errors
    /// Returns an error if tree-sitter initialization fails.
    pub fn new() -> Result<Self, ParserError> {
        let language = tree_sitter_your_language::LANGUAGE.into();

        // Verify the language is valid
        let mut parser = TSParser::new();
        parser
            .set_language(&language)
            .map_err(|e| ParserError::TreeSitterError(format!("Failed to set language: {}", e)))?;

        Ok(Self { language })
    }

    /// Parse project configuration to extract service metadata.
    ///
    /// For example, package.json for JavaScript or pyproject.toml for Python.
    pub fn parse_project_config(&self, repo_path: &Path) -> Option<ServiceDiscovery> {
        // Implement language-specific config parsing
        // Look for your language's package manifest
        let config_path = repo_path.join("your-config.yaml");
        if !config_path.exists() {
            return None;
        }

        // Parse config and extract service name, entry point, etc.
        let content = std::fs::read_to_string(&config_path).ok()?;

        // Extract service metadata...
        Some(ServiceDiscovery {
            name: "extracted-name".to_string(),
            language: "your-language".to_string(),
            framework: self.detect_framework(&content),
            entry_point: "main.yl".to_string(),
            source_file: config_path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: None,
        })
    }

    fn detect_framework(&self, _content: &str) -> Option<String> {
        // Detect frameworks from dependencies
        None
    }

    /// Detect import statements in the source code.
    fn detect_imports(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Use tree-sitter queries to find imports
        // Example query for a hypothetical language:
        let import_query = match tree_sitter::Query::new(
            &self.language,
            r#"
            (import_statement
              module: (identifier) @module)
            "#,
        ) {
            Ok(q) => q,
            Err(_) => return discoveries,
        };

        // Process query matches...
        // (See Python or JavaScript parser for full examples)

        discoveries
    }

    /// Detect AWS SDK usage patterns.
    fn detect_aws_sdk(
        &self,
        _tree: &tree_sitter::Tree,
        _content: &str,
        _path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Look for patterns like:
        // - AWS SDK client instantiation
        // - DynamoDB operations (get, put, query, scan)
        // - S3 operations
        // - SQS/SNS operations

        discoveries
    }

    /// Detect HTTP client usage.
    fn detect_http_clients(
        &self,
        _tree: &tree_sitter::Tree,
        _content: &str,
        _path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Look for HTTP client patterns specific to your language

        discoveries
    }
}

impl Parser for YourLanguageParser {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supported_extensions(&self) -> &[&str] {
        &["yl", "ylx"]  // Your language's file extensions
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        // Create a new parser instance for thread safety
        let mut parser = TSParser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| ParserError::TreeSitterError(format!("Failed to set language: {}", e)))?;

        // Parse the file
        let tree = parser
            .parse(content, None)
            .ok_or_else(|| ParserError::ParseFailed {
                path: path.to_string_lossy().to_string(),
            })?;

        let mut discoveries = Vec::new();

        // Run all detectors
        discoveries.extend(self.detect_imports(&tree, content, path));
        discoveries.extend(self.detect_aws_sdk(&tree, content, path));
        discoveries.extend(self.detect_http_clients(&tree, content, path));

        Ok(discoveries)
    }
}

impl Default for YourLanguageParser {
    fn default() -> Self {
        Self::new().expect("Failed to create default YourLanguageParser")
    }
}
```

### Step 3: Register the Parser

Update `forge-survey/src/parser/mod.rs` to include and register your parser:

```rust
// Add module declaration
pub mod your_language;

// Add re-export
pub use your_language::YourLanguageParser;

// In ParserRegistry::new(), add your parser:
impl ParserRegistry {
    pub fn new() -> Result<Self, ParserError> {
        let mut parsers: HashMap<String, Arc<dyn Parser>> = HashMap::new();

        // ... existing parsers ...

        // Register your language parser
        let your_language_parser: Arc<dyn Parser> = Arc::new(YourLanguageParser::new()?);
        parsers.insert("your-language".to_string(), your_language_parser);

        Ok(Self { parsers })
    }
}
```

### Step 4: Add Language Detection

Update `forge-survey/src/detection.rs` to auto-detect your language:

```rust
// In the detect_languages() function, add detection for your language:

// Check for your language's config files
if repo_path.join("your-config.yaml").exists() {
    languages.add(DetectedLanguage {
        name: "your-language".to_string(),
        confidence: 0.95,
        detection_method: DetectionMethod::ConfigFile,
    });
}

// Check for file extensions
let yl_count = count_files_with_extension(repo_path, &["yl", "ylx"]);
if yl_count >= 3 {
    languages.add(DetectedLanguage {
        name: "your-language".to_string(),
        confidence: calculate_confidence(yl_count),
        detection_method: DetectionMethod::FileExtension,
    });
}
```

### Step 5: Write Tests

Add comprehensive tests to your parser file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_parser() -> YourLanguageParser {
        YourLanguageParser::new().expect("Failed to create parser")
    }

    // ===================
    // Import Detection Tests
    // ===================

    #[test]
    fn test_detect_simple_imports() {
        let parser = create_parser();
        let content = r#"
import standard_lib
import my_module
"#;

        let discoveries = parser.parse_file(Path::new("test.yl"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(imports.len() >= 2, "Expected at least 2 imports");
    }

    // ===================
    // AWS SDK Detection Tests
    // ===================

    #[test]
    fn test_detect_dynamodb_operations() {
        let parser = create_parser();
        let content = r#"
client = aws.dynamodb.client()
result = client.get_item("users", key)
"#;

        let discoveries = parser.parse_file(Path::new("test.yl"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(!db_ops.is_empty(), "Should detect DynamoDB access");
        assert!(db_ops.iter().any(|d| d.db_type == "dynamodb"));
    }

    // ===================
    // HTTP Client Detection Tests
    // ===================

    #[test]
    fn test_detect_http_calls() {
        let parser = create_parser();
        let content = r#"
response = http.get("https://api.example.com/users")
"#;

        let discoveries = parser.parse_file(Path::new("test.yl"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(!api_calls.is_empty(), "Should detect HTTP call");
    }

    // ===================
    // Edge Cases
    // ===================

    #[test]
    fn test_empty_file() {
        let parser = create_parser();
        let discoveries = parser.parse_file(Path::new("test.yl"), "").unwrap();
        assert!(discoveries.is_empty());
    }

    #[test]
    fn test_supported_extensions() {
        let parser = create_parser();
        let extensions = parser.supported_extensions();
        assert!(extensions.contains(&"yl"));
    }
}
```

### Step 6: Write Integration Tests

Create integration tests in `forge-survey/tests/integration_your_language.rs`:

```rust
//! Integration tests for YourLanguage parser.

use forge_graph::ForgeGraph;
use forge_survey::parser::{YourLanguageParser, Parser};
use forge_survey::graph_builder::GraphBuilder;
use std::path::Path;
use tempfile::tempdir;

fn create_test_repo(path: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(path).unwrap();
    for (name, content) in files {
        let file_path = path.join(name);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(file_path, content).unwrap();
    }
}

#[test]
fn test_survey_your_language_repo() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("test-service");

    create_test_repo(&repo_path, &[
        ("your-config.yaml", r#"
name: test-service
entry: main.yl
"#),
        ("main.yl", r#"
import aws.dynamodb
import http

client = aws.dynamodb.client()
table = client.table("users")
result = table.get_item({"id": "123"})

response = http.get("https://api.example.com/data")
"#),
    ]);

    let parser = YourLanguageParser::new().unwrap();
    let discoveries = parser.parse_repo(&repo_path).unwrap();

    // Verify discoveries
    assert!(!discoveries.is_empty(), "Should find discoveries");

    // Build graph
    let mut builder = GraphBuilder::new();
    // ... process discoveries into graph ...
}
```

---

## Discovery Types Reference

Your parser should emit appropriate `Discovery` variants:

### ServiceDiscovery

Emitted when a service entry point is found (e.g., from package manifests):

```rust
Discovery::Service(ServiceDiscovery {
    name: "my-service".to_string(),
    language: "your-language".to_string(),
    framework: Some("your-framework".to_string()),
    entry_point: "main.yl".to_string(),
    source_file: "your-config.yaml".to_string(),
    source_line: 1,
    deployment_metadata: None,
})
```

### ImportDiscovery

Emitted for import/require statements:

```rust
Discovery::Import(ImportDiscovery {
    module: "aws.dynamodb".to_string(),
    is_relative: false,
    imported_items: vec!["Client".to_string()],
    source_file: path.to_string_lossy().to_string(),
    source_line: 5,
})
```

### DatabaseAccessDiscovery

Emitted for database operations:

```rust
Discovery::DatabaseAccess(DatabaseAccessDiscovery {
    db_type: "dynamodb".to_string(),
    table_name: Some("users".to_string()),
    operation: DatabaseOperation::Read,  // Read, Write, ReadWrite, or Unknown
    detection_method: "aws-sdk".to_string(),
    source_file: path.to_string_lossy().to_string(),
    source_line: 10,
    deployment_metadata: None,
})
```

### QueueOperationDiscovery

Emitted for message queue operations (SQS, SNS, etc.):

```rust
Discovery::QueueOperation(QueueOperationDiscovery {
    queue_type: "sqs".to_string(),
    queue_name: Some("order-events".to_string()),
    operation: QueueOperationType::Publish,  // Publish, Subscribe, or Unknown
    source_file: path.to_string_lossy().to_string(),
    source_line: 15,
    deployment_metadata: None,
})
```

### CloudResourceDiscovery

Emitted for other AWS resource usage (S3, Lambda, etc.):

```rust
Discovery::CloudResourceUsage(CloudResourceDiscovery {
    resource_type: "s3".to_string(),
    resource_name: Some("my-bucket".to_string()),
    source_file: path.to_string_lossy().to_string(),
    source_line: 20,
    deployment_metadata: None,
})
```

### ApiCallDiscovery

Emitted for HTTP API calls:

```rust
Discovery::ApiCall(ApiCallDiscovery {
    target: "https://api.example.com/users".to_string(),
    method: Some("GET".to_string()),
    detection_method: "http-client".to_string(),
    source_file: path.to_string_lossy().to_string(),
    source_line: 25,
})
```

---

## Best Practices

### 1. Be Conservative with Discoveries

Only emit discoveries for patterns you're confident about. It's better to miss some patterns than to create false positives that pollute the knowledge graph.

### 2. Include Source Location

Always populate `source_file` and `source_line` fields—they're crucial for traceability and debugging.

### 3. Extract Resource Names When Possible

Try to extract actual table names, queue names, and bucket names rather than just detecting "some DynamoDB access exists."

### 4. Handle Edge Cases Gracefully

```rust
fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
    // Handle empty files
    if content.trim().is_empty() {
        return Ok(vec![]);
    }

    // Parse the file
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => {
            // Log but don't fail
            tracing::warn!("Failed to parse {}", path.display());
            return Ok(vec![]);
        }
    };

    // Continue processing...
}
```

### 5. Test Against Real-World Patterns

Look at how your language's ecosystem actually uses AWS SDKs, HTTP clients, etc. Test against realistic code patterns.

### 6. Document Detection Patterns

Add comments explaining what patterns your parser detects:

```rust
/// Detects boto3-style DynamoDB client instantiation:
/// ```python
/// dynamodb = boto3.client('dynamodb')
/// dynamodb = boto3.resource('dynamodb')
/// table = dynamodb.Table('users')
/// ```
fn detect_boto3_clients(&self, ...) -> Vec<Discovery> {
```

---

## Tree-Sitter Query Tips

### Finding Query Patterns

Use the tree-sitter playground to explore your language's AST:

1. Visit https://tree-sitter.github.io/tree-sitter/playground
2. Select your language grammar
3. Paste sample code
4. Examine the AST structure

### Query Syntax

```rust
// Match any import statement
"(import_statement) @import"

// Match import with captured module name
"(import_statement module: (identifier) @module)"

// Match specific function calls
"(call_expression
  function: (identifier) @func
  (#eq? @func \"require\"))"

// Match method calls on specific objects
"(call_expression
  function: (member_expression
    object: (identifier) @obj
    property: (property_identifier) @method)
  (#eq? @obj \"dynamodb\"))"
```

### Processing Query Results

```rust
use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor};

let query = Query::new(&self.language, "(import_statement module: (identifier) @module)")?;
let mut cursor = QueryCursor::new();
let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

while let Some(match_) = matches.next() {
    for capture in match_.captures {
        let node = capture.node;
        let text = node.utf8_text(content.as_bytes()).unwrap_or("");
        let line = node.start_position().row as u32 + 1;

        // Process the captured text...
    }
}
```

---

## Testing Guidelines

### Unit Test Coverage

Your parser should have tests for:

1. **Import detection** - Various import syntax forms
2. **AWS SDK patterns** - Client creation, operations
3. **HTTP client detection** - GET, POST, etc.
4. **Database operations** - Read, write, read-write operations
5. **Queue operations** - Publish, subscribe
6. **Edge cases** - Empty files, syntax errors, comments-only files
7. **Source line accuracy** - Verify line numbers are correct
8. **Project config parsing** - Service name extraction

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Run only your parser's tests
cargo test --package forge-survey your_language

# Run with output
cargo test --package forge-survey your_language -- --nocapture
```

---

## Checklist

Before submitting your parser:

- [ ] Parser implements `Parser` trait correctly
- [ ] Parser is registered in `ParserRegistry`
- [ ] Language detection is added to `detection.rs`
- [ ] Module is exported in `parser/mod.rs`
- [ ] Unit tests cover all detection patterns
- [ ] Integration test verifies full repo parsing
- [ ] Documentation in parser file explains patterns detected
- [ ] All tests pass: `cargo test --workspace`
- [ ] Code is formatted: `cargo fmt`
- [ ] No clippy warnings: `cargo clippy --workspace -- -D warnings`

---

## Examples

For complete implementation examples, see:

- **JavaScript/TypeScript**: [`forge-survey/src/parser/javascript.rs`](../forge-survey/src/parser/javascript.rs)
- **Python**: [`forge-survey/src/parser/python.rs`](../forge-survey/src/parser/python.rs)
- **Terraform**: [`forge-survey/src/parser/terraform.rs`](../forge-survey/src/parser/terraform.rs)
- **CloudFormation/SAM**: [`forge-survey/src/parser/cloudformation.rs`](../forge-survey/src/parser/cloudformation.rs)

---

## See Also

- [CLI Reference](cli-reference.md) - Command-line options
- [Configuration Reference](configuration.md) - Full `forge.yaml` schema
- [Extending LLM Providers](extending-llm-providers.md) - Adding new LLM CLI adapters
