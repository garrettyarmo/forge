//! Python parser for detecting AWS SDK usage, HTTP clients, and imports.

use super::traits::*;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser as TSParser, Query, QueryCursor};

/// Parser for Python files using tree-sitter
pub struct PythonParser {
    language: Language,
}

impl PythonParser {
    pub fn new() -> Result<Self, ParserError> {
        let language = tree_sitter_python::LANGUAGE.into();

        // Verify language validity
        let mut parser = TSParser::new();
        parser
            .set_language(&language)
            .map_err(|e| ParserError::TreeSitterError(format!("Failed to set Python language: {}", e)))?;

        Ok(Self { language })
    }

    /// Detect import statements (import X, from X import Y)
    fn detect_imports(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        let import_query = match Query::new(
            &self.language,
            r#"
            (import_statement
              name: (dotted_name) @module)

            (import_from_statement
              module_name: (dotted_name) @module)
            "#,
        ) {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("Failed to create import query: {}", e);
                return discoveries;
            }
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&import_query, tree.root_node(), content.as_bytes());

        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let node = capture.node;
                let module = &content[node.byte_range()];

                discoveries.push(Discovery::Import(ImportDiscovery {
                    module: module.to_string(),
                    is_relative: module.starts_with('.'),
                    imported_items: vec![],
                    source_file: path.to_string_lossy().to_string(),
                    source_line: node.start_position().row as u32 + 1,
                }));
            }
        }

        discoveries
    }

    /// Detect boto3.client() and boto3.resource() calls
    fn detect_boto3_clients(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Walk the tree to find boto3.client() and boto3.resource() calls
        self.walk_for_boto3_calls(tree.root_node(), content, path, &mut discoveries);

        discoveries
    }

    fn walk_for_boto3_calls(
        &self,
        node: Node,
        content: &str,
        path: &Path,
        discoveries: &mut Vec<Discovery>,
    ) {
        if node.kind() == "call" {
            // Check if this is boto3.client() or boto3.resource()
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "attribute" {
                    if let Some(object_node) = function_node.child_by_field_name("object") {
                        if let Some(attribute_node) = function_node.child_by_field_name("attribute") {
                            let object_text = &content[object_node.byte_range()];
                            let attribute_text = &content[attribute_node.byte_range()];

                            if object_text == "boto3" && (attribute_text == "client" || attribute_text == "resource") {
                                // Extract the service name from the arguments
                                if let Some(args_node) = node.child_by_field_name("arguments") {
                                    if let Some(service_name) = self.extract_first_string_arg(args_node, content) {
                                        self.add_service_discovery(
                                            discoveries,
                                            &service_name,
                                            node,
                                            path,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Recursively walk children
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                self.walk_for_boto3_calls(child, content, path, discoveries);
            }
        }
    }

    fn extract_first_string_arg(&self, args_node: Node, content: &str) -> Option<String> {
        for i in 0..args_node.named_child_count() {
            if let Some(child) = args_node.named_child(i) {
                if child.kind() == "string" {
                    let text = &content[child.byte_range()];
                    let trimmed = text.trim_matches(|c| c == '"' || c == '\'');
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }

    fn add_service_discovery(
        &self,
        discoveries: &mut Vec<Discovery>,
        service: &str,
        node: Node,
        path: &Path,
    ) {
        let line = node.start_position().row as u32 + 1;
        let source_file = path.to_string_lossy().to_string();

        match service {
            "dynamodb" => {
                discoveries.push(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
                    db_type: "dynamodb".to_string(),
                    table_name: None,
                    operation: DatabaseOperation::Unknown,
                    detection_method: "boto3.client".to_string(),
                    source_file,
                    source_line: line,
                }));
            }
            "s3" => {
                discoveries.push(Discovery::CloudResourceUsage(CloudResourceDiscovery {
                    resource_type: "s3".to_string(),
                    resource_name: None,
                    source_file,
                    source_line: line,
                }));
            }
            "sqs" => {
                discoveries.push(Discovery::QueueOperation(QueueOperationDiscovery {
                    queue_type: "sqs".to_string(),
                    queue_name: None,
                    operation: QueueOperationType::Unknown,
                    source_file,
                    source_line: line,
                }));
            }
            "sns" => {
                discoveries.push(Discovery::QueueOperation(QueueOperationDiscovery {
                    queue_type: "sns".to_string(),
                    queue_name: None,
                    operation: QueueOperationType::Publish,
                    source_file,
                    source_line: line,
                }));
            }
            "lambda" => {
                discoveries.push(Discovery::CloudResourceUsage(CloudResourceDiscovery {
                    resource_type: "lambda".to_string(),
                    resource_name: None,
                    source_file,
                    source_line: line,
                }));
            }
            "events" | "eventbridge" => {
                discoveries.push(Discovery::QueueOperation(QueueOperationDiscovery {
                    queue_type: "eventbridge".to_string(),
                    queue_name: None,
                    operation: QueueOperationType::Unknown,
                    source_file,
                    source_line: line,
                }));
            }
            _ => {
                // Generic AWS service
                discoveries.push(Discovery::CloudResourceUsage(CloudResourceDiscovery {
                    resource_type: service.to_string(),
                    resource_name: None,
                    source_file,
                    source_line: line,
                }));
            }
        }
    }

    /// Detect requests.get/post and httpx.get/post calls
    fn detect_http_clients(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        self.walk_for_http_calls(tree.root_node(), content, path, &mut discoveries);

        discoveries
    }

    fn walk_for_http_calls(
        &self,
        node: Node,
        content: &str,
        path: &Path,
        discoveries: &mut Vec<Discovery>,
    ) {
        if node.kind() == "call" {
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "attribute" {
                    if let Some(object_node) = function_node.child_by_field_name("object") {
                        if let Some(method_node) = function_node.child_by_field_name("attribute") {
                            let client = &content[object_node.byte_range()];
                            let method = &content[method_node.byte_range()];

                            let http_methods = ["get", "post", "put", "delete", "patch", "head", "options"];
                            if (client == "requests" || client == "httpx") && http_methods.contains(&method) {
                                // Extract URL from arguments
                                let url = if let Some(args_node) = node.child_by_field_name("arguments") {
                                    self.extract_first_string_arg(args_node, content)
                                        .unwrap_or_else(|| "unknown".to_string())
                                } else {
                                    "unknown".to_string()
                                };

                                discoveries.push(Discovery::ApiCall(ApiCallDiscovery {
                                    target: url,
                                    method: Some(method.to_uppercase()),
                                    detection_method: client.to_string(),
                                    source_file: path.to_string_lossy().to_string(),
                                    source_line: node.start_position().row as u32 + 1,
                                }));
                            }
                        }
                    }
                }
            }
        }

        // Recursively walk children
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                self.walk_for_http_calls(child, content, path, discoveries);
            }
        }
    }

    /// Detect DynamoDB method calls (get_item, put_item, etc.)
    fn detect_dynamodb_methods(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        self.walk_for_dynamodb_methods(tree.root_node(), content, path, &mut discoveries);

        discoveries
    }

    fn walk_for_dynamodb_methods(
        &self,
        node: Node,
        content: &str,
        path: &Path,
        discoveries: &mut Vec<Discovery>,
    ) {
        if node.kind() == "call" {
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "attribute" {
                    if let Some(method_node) = function_node.child_by_field_name("attribute") {
                        let method = &content[method_node.byte_range()];

                        let dynamodb_methods = [
                            ("get_item", DatabaseOperation::Read),
                            ("put_item", DatabaseOperation::Write),
                            ("update_item", DatabaseOperation::ReadWrite),
                            ("delete_item", DatabaseOperation::Write),
                            ("query", DatabaseOperation::Read),
                            ("scan", DatabaseOperation::Read),
                            ("batch_get_item", DatabaseOperation::Read),
                            ("batch_write_item", DatabaseOperation::Write),
                        ];

                        for (method_name, operation) in &dynamodb_methods {
                            if method == *method_name {
                                // Try to extract table name from arguments
                                let table_name = if let Some(args_node) = node.child_by_field_name("arguments") {
                                    self.extract_table_name_from_args(args_node, content)
                                } else {
                                    None
                                };

                                discoveries.push(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
                                    db_type: "dynamodb".to_string(),
                                    table_name,
                                    operation: *operation,
                                    detection_method: format!("boto3.{}", method),
                                    source_file: path.to_string_lossy().to_string(),
                                    source_line: method_node.start_position().row as u32 + 1,
                                }));

                                break;
                            }
                        }
                    }
                }
            }
        }

        // Recursively walk children
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                self.walk_for_dynamodb_methods(child, content, path, discoveries);
            }
        }
    }

    fn extract_table_name_from_args(&self, args_node: Node, content: &str) -> Option<String> {
        // Look for TableName='xxx' or TableName="xxx" in keyword arguments
        for i in 0..args_node.named_child_count() {
            if let Some(child) = args_node.named_child(i) {
                if child.kind() == "keyword_argument" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = &content[name_node.byte_range()];
                        if name == "TableName" {
                            if let Some(value_node) = child.child_by_field_name("value") {
                                if value_node.kind() == "string" {
                                    let text = &content[value_node.byte_range()];
                                    let trimmed = text.trim_matches(|c| c == '"' || c == '\'');
                                    return Some(trimmed.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Parse requirements.txt or pyproject.toml for service detection
    pub fn parse_project_config(&self, repo_path: &Path) -> Option<ServiceDiscovery> {
        // Try pyproject.toml first
        let pyproject_path = repo_path.join("pyproject.toml");
        if pyproject_path.exists() {
            if let Some(service) = self.parse_pyproject_toml(&pyproject_path, repo_path) {
                return Some(service);
            }
        }

        // Try setup.py
        let setup_path = repo_path.join("setup.py");
        if setup_path.exists() {
            if let Some(service) = self.parse_setup_py(&setup_path, repo_path) {
                return Some(service);
            }
        }

        // Fall back to directory name if requirements.txt exists
        let requirements_path = repo_path.join("requirements.txt");
        if requirements_path.exists() {
            let name = repo_path.file_name()?.to_str()?.to_string();
            let framework = self.detect_framework_from_requirements(&requirements_path);

            return Some(ServiceDiscovery {
                name,
                language: "python".to_string(),
                framework,
                entry_point: self.find_entry_point(repo_path).unwrap_or_else(|| "main.py".to_string()),
                source_file: requirements_path.to_string_lossy().to_string(),
                source_line: 1,
            });
        }

        None
    }

    fn parse_pyproject_toml(&self, path: &Path, repo_path: &Path) -> Option<ServiceDiscovery> {
        let content = std::fs::read_to_string(path).ok()?;

        // Simple TOML parsing for name field
        let name = content
            .lines()
            .find(|line| line.trim().starts_with("name"))
            .and_then(|line| {
                let parts: Vec<&str> = line.split('=').collect();
                parts.get(1).map(|v| {
                    v.trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string()
                })
            })?;

        // Detect framework from dependencies
        let framework = if content.contains("fastapi") {
            Some("fastapi".to_string())
        } else if content.contains("flask") {
            Some("flask".to_string())
        } else if content.contains("django") {
            Some("django".to_string())
        } else if content.contains("starlette") {
            Some("starlette".to_string())
        } else {
            None
        };

        Some(ServiceDiscovery {
            name,
            language: "python".to_string(),
            framework,
            entry_point: self.find_entry_point(repo_path).unwrap_or_else(|| "main.py".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    fn parse_setup_py(&self, path: &Path, repo_path: &Path) -> Option<ServiceDiscovery> {
        let content = std::fs::read_to_string(path).ok()?;

        // Look for name= in setup()
        let name = content
            .lines()
            .find(|line| line.contains("name=") || line.contains("name ="))
            .and_then(|line| {
                let start = line.find('=')? + 1;
                let value = line[start..]
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'' || c == ',');
                Some(value.to_string())
            })?;

        Some(ServiceDiscovery {
            name,
            language: "python".to_string(),
            framework: None,
            entry_point: self.find_entry_point(repo_path).unwrap_or_else(|| "main.py".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    fn detect_framework_from_requirements(&self, path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;

        if content
            .lines()
            .any(|l| l.starts_with("fastapi") || l.starts_with("FastAPI"))
        {
            Some("fastapi".to_string())
        } else if content
            .lines()
            .any(|l| l.starts_with("flask") || l.starts_with("Flask"))
        {
            Some("flask".to_string())
        } else if content
            .lines()
            .any(|l| l.starts_with("django") || l.starts_with("Django"))
        {
            Some("django".to_string())
        } else if content.lines().any(|l| l.starts_with("chalice")) {
            Some("chalice".to_string())
        } else {
            None
        }
    }

    fn find_entry_point(&self, repo_path: &Path) -> Option<String> {
        // Common entry points
        let candidates = ["main.py", "app.py", "run.py", "server.py", "__main__.py"];

        for candidate in &candidates {
            if repo_path.join(candidate).exists() {
                return Some(candidate.to_string());
            }
        }

        // Check src/ directory
        let src_path = repo_path.join("src");
        if src_path.exists() {
            for candidate in &candidates {
                if src_path.join(candidate).exists() {
                    return Some(format!("src/{}", candidate));
                }
            }
        }

        None
    }
}

impl Parser for PythonParser {
    fn supported_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        // Create a new parser for thread safety
        let mut parser = TSParser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| ParserError::TreeSitterError(format!("Failed to set language: {}", e)))?;

        let tree = parser.parse(content, None).ok_or_else(|| ParserError::ParseFailed {
            path: path.to_string_lossy().to_string(),
        })?;

        let mut discoveries = Vec::new();

        discoveries.extend(self.detect_imports(&tree, content, path));
        discoveries.extend(self.detect_boto3_clients(&tree, content, path));
        discoveries.extend(self.detect_http_clients(&tree, content, path));
        discoveries.extend(self.detect_dynamodb_methods(&tree, content, path));

        Ok(discoveries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_boto3_dynamodb() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import boto3

dynamodb = boto3.client('dynamodb')
result = dynamodb.get_item(TableName='users', Key={'id': {'S': '123'}})
"#;

        let discoveries = parser
            .parse_file(Path::new("test.py"), content)
            .unwrap();

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(db_accesses.iter().any(|d| d.db_type == "dynamodb"));
        // Should have at least 2: one from boto3.client(), one from get_item()
        assert!(db_accesses.len() >= 1);
    }

    #[test]
    fn test_detect_boto3_s3() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import boto3

s3 = boto3.client('s3')
s3.upload_file('local.txt', 'my-bucket', 'remote.txt')
"#;

        let discoveries = parser
            .parse_file(Path::new("test.py"), content)
            .unwrap();

        let resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) => Some(r),
                _ => None,
            })
            .collect();

        assert!(resources.iter().any(|r| r.resource_type == "s3"));
    }

    #[test]
    fn test_detect_requests() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import requests

response = requests.get('https://api.example.com/users')
requests.post('https://api.example.com/orders', json={'item': 'test'})
"#;

        let discoveries = parser
            .parse_file(Path::new("test.py"), content)
            .unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert_eq!(api_calls.len(), 2);
        assert!(api_calls
            .iter()
            .any(|a| a.method == Some("GET".to_string())));
        assert!(api_calls
            .iter()
            .any(|a| a.method == Some("POST".to_string())));
    }

    #[test]
    fn test_parse_pyproject_toml() {
        let parser = PythonParser::new().unwrap();
        let dir = tempdir().unwrap();

        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[project]
name = "my-service"
version = "1.0.0"

[project.dependencies]
fastapi = ">=0.100.0"
boto3 = ">=1.28.0"
"#,
        )
        .unwrap();

        let service = parser.parse_project_config(dir.path()).unwrap();
        assert_eq!(service.name, "my-service");
        assert_eq!(service.framework, Some("fastapi".to_string()));
    }

    #[test]
    fn test_detect_imports() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import boto3
import requests
from datetime import datetime
"#;

        let discoveries = parser
            .parse_file(Path::new("test.py"), content)
            .unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(imports.iter().any(|i| i.module == "boto3"));
        assert!(imports.iter().any(|i| i.module == "requests"));
        assert!(imports.iter().any(|i| i.module == "datetime"));
    }

    #[test]
    fn test_detect_httpx() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import httpx

client = httpx.get('https://api.example.com/data')
"#;

        let discoveries = parser
            .parse_file(Path::new("test.py"), content)
            .unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert_eq!(api_calls.len(), 1);
        assert_eq!(api_calls[0].detection_method, "httpx");
    }

    #[test]
    fn test_detect_dynamodb_table_name() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import boto3

dynamodb = boto3.client('dynamodb')
response = dynamodb.put_item(
    TableName='users',
    Item={'id': {'S': '123'}, 'name': {'S': 'John'}}
)
"#;

        let discoveries = parser
            .parse_file(Path::new("test.py"), content)
            .unwrap();

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        // Find the put_item operation
        let put_item = db_accesses
            .iter()
            .find(|d| matches!(d.operation, DatabaseOperation::Write));
        assert!(put_item.is_some());

        if let Some(put) = put_item {
            assert_eq!(put.table_name, Some("users".to_string()));
        }
    }
}
