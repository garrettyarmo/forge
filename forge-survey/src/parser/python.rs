//! Python parser for Forge survey.
//!
//! This parser uses tree-sitter to analyze Python files and detect:
//! - Import statements (`import X` and `from X import Y`)
//! - boto3 client/resource patterns for AWS services (DynamoDB, S3, SQS, SNS, Lambda, EventBridge)
//! - DynamoDB method calls (get_item, put_item, query, scan, etc.)
//! - HTTP client usage (requests, httpx)
//! - Service metadata from pyproject.toml, setup.py, requirements.txt
//!
//! The parser is deterministic - it uses only AST analysis with no LLM calls.

use super::traits::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError, QueueOperationDiscovery, QueueOperationType,
    ServiceDiscovery,
};
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser as TSParser, Query, QueryCursor};

/// Parser for Python files.
///
/// Uses tree-sitter queries to detect:
/// - Import statements (`import` and `from...import`)
/// - boto3 client/resource usage patterns
/// - HTTP client calls (requests, httpx)
/// - DynamoDB operations
pub struct PythonParser {
    language: Language,
}

impl PythonParser {
    /// Create a new Python parser.
    ///
    /// # Errors
    /// Returns an error if tree-sitter initialization fails.
    pub fn new() -> Result<Self, ParserError> {
        let language = tree_sitter_python::LANGUAGE.into();

        // Verify the language is valid by trying to set it on a parser
        let mut parser = TSParser::new();
        parser
            .set_language(&language)
            .map_err(|e| ParserError::TreeSitterError(format!("Failed to set language: {}", e)))?;

        Ok(Self { language })
    }

    /// Parse pyproject.toml, setup.py, or requirements.txt to extract service metadata.
    ///
    /// Detects:
    /// - Service name from pyproject.toml or setup.py
    /// - Framework from dependencies (FastAPI, Flask, Django, Chalice, Starlette)
    /// - Entry point (main.py, app.py, etc.)
    pub fn parse_project_config(&self, repo_path: &Path) -> Option<ServiceDiscovery> {
        // Try pyproject.toml first (modern Python packaging)
        let pyproject_path = repo_path.join("pyproject.toml");
        if pyproject_path.exists() {
            if let Some(service) = self.parse_pyproject_toml(&pyproject_path, repo_path) {
                return Some(service);
            }
        }

        // Try setup.py (traditional packaging)
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
                entry_point: self
                    .find_entry_point(repo_path)
                    .unwrap_or_else(|| "main.py".to_string()),
                source_file: requirements_path.to_string_lossy().to_string(),
                source_line: 1,
            });
        }

        None
    }

    /// Parse pyproject.toml to extract service metadata.
    fn parse_pyproject_toml(&self, path: &Path, repo_path: &Path) -> Option<ServiceDiscovery> {
        let content = std::fs::read_to_string(path).ok()?;

        // Simple TOML parsing for name field
        // Look for [project] section first (PEP 621), then [tool.poetry] as fallback
        let name = self
            .extract_toml_name(&content, "[project]")
            .or_else(|| self.extract_toml_name(&content, "[tool.poetry]"))?;

        // Detect framework from dependencies
        let framework = self.detect_framework_from_toml_content(&content);

        Some(ServiceDiscovery {
            name,
            language: "python".to_string(),
            framework,
            entry_point: self
                .find_entry_point(repo_path)
                .unwrap_or_else(|| "main.py".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    /// Extract the name field from a TOML section.
    fn extract_toml_name(&self, content: &str, section: &str) -> Option<String> {
        let section_start = content.find(section)?;
        let section_content = &content[section_start..];

        // Find the next section start (or end of file)
        let section_end = section_content[1..]
            .find("\n[")
            .map(|i| i + 1)
            .unwrap_or(section_content.len());
        let section_text = &section_content[..section_end];

        // Look for name = "..." or name = '...'
        for line in section_text.lines() {
            let line = line.trim();
            if line.starts_with("name") && line.contains('=') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let value = parts[1]
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string();
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
        None
    }

    /// Detect framework from pyproject.toml content.
    fn detect_framework_from_toml_content(&self, content: &str) -> Option<String> {
        let content_lower = content.to_lowercase();

        if content_lower.contains("fastapi") {
            Some("fastapi".to_string())
        } else if content_lower.contains("flask") {
            Some("flask".to_string())
        } else if content_lower.contains("django") {
            Some("django".to_string())
        } else if content_lower.contains("chalice") {
            Some("chalice".to_string())
        } else if content_lower.contains("starlette") {
            Some("starlette".to_string())
        } else {
            None
        }
    }

    /// Parse setup.py to extract service metadata.
    fn parse_setup_py(&self, path: &Path, repo_path: &Path) -> Option<ServiceDiscovery> {
        let content = std::fs::read_to_string(path).ok()?;

        // Look for name= in setup() call
        let name = self.extract_setup_py_name(&content)?;

        Some(ServiceDiscovery {
            name,
            language: "python".to_string(),
            framework: None,
            entry_point: self
                .find_entry_point(repo_path)
                .unwrap_or_else(|| "main.py".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    /// Extract the name parameter from setup.py.
    fn extract_setup_py_name(&self, content: &str) -> Option<String> {
        for line in content.lines() {
            let line = line.trim();
            if (line.contains("name=") || line.contains("name =")) && !line.starts_with('#') {
                if let Some(start) = line.find('=') {
                    let value = line[start + 1..]
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'' || c == ',');
                    if !value.is_empty() && !value.starts_with("os.") && !value.contains('(') {
                        return Some(value.to_string());
                    }
                }
            }
        }
        None
    }

    /// Detect framework from requirements.txt.
    fn detect_framework_from_requirements(&self, path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;

        for line in content.lines() {
            let line = line.trim().to_lowercase();
            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Extract package name (before any version specifier)
            let package = line
                .split(['=', '<', '>', '[', ';'])
                .next()
                .unwrap_or("")
                .trim();

            match package {
                "fastapi" => return Some("fastapi".to_string()),
                "flask" => return Some("flask".to_string()),
                "django" => return Some("django".to_string()),
                "chalice" => return Some("chalice".to_string()),
                "starlette" => return Some("starlette".to_string()),
                _ => {}
            }
        }
        None
    }

    /// Find the entry point file in a Python project.
    fn find_entry_point(&self, repo_path: &Path) -> Option<String> {
        // Common entry points in order of preference
        let candidates = [
            "main.py",
            "app.py",
            "run.py",
            "server.py",
            "__main__.py",
            "wsgi.py",
            "asgi.py",
        ];

        // Check root directory
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

        // Check for app/ directory (common in Django/Flask)
        let app_path = repo_path.join("app");
        if app_path.exists() {
            for candidate in &candidates {
                if app_path.join(candidate).exists() {
                    return Some(format!("app/{}", candidate));
                }
            }
        }

        None
    }

    /// Detect import statements (`import X` and `from X import Y`).
    fn detect_imports(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Query for regular import statements: import X
        let import_query = match Query::new(
            &self.language,
            r#"
            (import_statement
              name: (dotted_name) @module)
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
                let module = node.utf8_text(content.as_bytes()).unwrap_or("");

                if !module.is_empty() {
                    discoveries.push(Discovery::Import(ImportDiscovery {
                        module: module.to_string(),
                        is_relative: false,
                        imported_items: vec![],
                        source_file: path.to_string_lossy().to_string(),
                        source_line: node.start_position().row as u32 + 1,
                    }));
                }
            }
        }

        // Query for from...import statements: from X import Y
        let from_import_query = match Query::new(
            &self.language,
            r#"
            (import_from_statement
              module_name: (dotted_name) @module)
            "#,
        ) {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("Failed to create from-import query: {}", e);
                return discoveries;
            }
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&from_import_query, tree.root_node(), content.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let node = capture.node;
                let module_text = node.utf8_text(content.as_bytes()).unwrap_or("");

                if !module_text.is_empty() {
                    let imported_items = self.extract_imported_names(node.parent(), content);

                    discoveries.push(Discovery::Import(ImportDiscovery {
                        module: module_text.to_string(),
                        is_relative: false,
                        imported_items,
                        source_file: path.to_string_lossy().to_string(),
                        source_line: node.start_position().row as u32 + 1,
                    }));
                }
            }
        }

        // Handle relative imports: from . import X, from .. import Y, from .module import Z
        let relative_import_query = match Query::new(
            &self.language,
            r#"
            (import_from_statement
              module_name: (relative_import) @relative)
            "#,
        ) {
            Ok(q) => q,
            Err(_) => return discoveries,
        };

        let mut cursor = QueryCursor::new();
        let mut matches =
            cursor.matches(&relative_import_query, tree.root_node(), content.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let node = capture.node;
                let module_text = node.utf8_text(content.as_bytes()).unwrap_or("");

                if !module_text.is_empty() {
                    let imported_items = self.extract_imported_names(node.parent(), content);

                    discoveries.push(Discovery::Import(ImportDiscovery {
                        module: module_text.to_string(),
                        is_relative: true,
                        imported_items,
                        source_file: path.to_string_lossy().to_string(),
                        source_line: node.start_position().row as u32 + 1,
                    }));
                }
            }
        }

        discoveries
    }

    /// Extract imported names from a from...import statement.
    fn extract_imported_names(&self, import_node: Option<Node>, content: &str) -> Vec<String> {
        let mut items = Vec::new();

        if let Some(stmt) = import_node {
            // Walk children to find aliased_import or identifier nodes after the module
            for i in 0..stmt.named_child_count() {
                if let Some(child) = stmt.named_child(i) {
                    match child.kind() {
                        "aliased_import" => {
                            // Get the original name (first child)
                            if let Some(name_node) = child.named_child(0) {
                                if let Ok(text) = name_node.utf8_text(content.as_bytes()) {
                                    items.push(text.to_string());
                                }
                            }
                        }
                        "dotted_name" => {
                            // Skip the first dotted_name which is the module name
                            if i > 0 {
                                if let Ok(text) = child.utf8_text(content.as_bytes()) {
                                    items.push(text.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        items
    }

    /// Detect boto3 client and resource patterns.
    fn detect_boto3_clients(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Walk the tree to find boto3.client() and boto3.resource() calls
        self.walk_for_boto3_calls(tree.root_node(), content, path, &mut discoveries);

        discoveries
    }

    /// Walk the AST recursively looking for boto3 client/resource calls.
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
                        if let Some(attribute_node) = function_node.child_by_field_name("attribute")
                        {
                            let object_text =
                                object_node.utf8_text(content.as_bytes()).unwrap_or("");
                            let attribute_text =
                                attribute_node.utf8_text(content.as_bytes()).unwrap_or("");

                            if object_text == "boto3"
                                && (attribute_text == "client" || attribute_text == "resource")
                            {
                                // Extract the service name from the arguments
                                if let Some(args_node) = node.child_by_field_name("arguments") {
                                    if let Some(service_name) =
                                        self.extract_first_string_arg(args_node, content)
                                    {
                                        self.add_service_discovery(
                                            discoveries,
                                            &service_name,
                                            node.start_position().row as u32 + 1,
                                            path,
                                            attribute_text,
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

    /// Extract the first string argument from an argument list.
    fn extract_first_string_arg(&self, args_node: Node, content: &str) -> Option<String> {
        for i in 0..args_node.named_child_count() {
            if let Some(child) = args_node.named_child(i) {
                if child.kind() == "string" {
                    let text = child.utf8_text(content.as_bytes()).unwrap_or("");
                    // Remove quotes and f-string prefix
                    let cleaned = text
                        .trim_start_matches('f')
                        .trim_start_matches('r')
                        .trim_start_matches('b')
                        .trim_matches(|c| c == '"' || c == '\'');
                    return Some(cleaned.to_string());
                }
            }
        }
        None
    }

    /// Add a discovery for a specific AWS service.
    fn add_service_discovery(
        &self,
        discoveries: &mut Vec<Discovery>,
        service: &str,
        line: u32,
        path: &Path,
        detection_method: &str,
    ) {
        let source_file = path.to_string_lossy().to_string();
        let _detection = format!("boto3.{}", detection_method);

        match service {
            "dynamodb" => {
                // NOTE: Don't create a DatabaseAccessDiscovery here!
                // DynamoDB discoveries are handled by detect_dynamodb_methods() which
                // extracts the actual table name from method calls or Table() assignments.
                // Creating one here with table_name: None would result in a "dynamodb-unknown"
                // node that can't be properly deduplicated with actual table operations.
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
                    operation: QueueOperationType::Publish, // SNS is typically publish-only from code
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

    /// Detect HTTP client usage (requests and httpx).
    fn detect_http_clients(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        self.walk_for_http_calls(tree.root_node(), content, path, &mut discoveries);

        discoveries
    }

    /// Walk AST looking for HTTP client calls.
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
                            let client = object_node.utf8_text(content.as_bytes()).unwrap_or("");
                            let method = method_node.utf8_text(content.as_bytes()).unwrap_or("");

                            let http_methods =
                                ["get", "post", "put", "delete", "patch", "head", "options"];
                            if (client == "requests" || client == "httpx")
                                && http_methods.contains(&method)
                            {
                                // Extract URL from arguments
                                let url = if let Some(args_node) =
                                    node.child_by_field_name("arguments")
                                {
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

    /// Detect DynamoDB method calls (get_item, put_item, query, scan, etc.).
    fn detect_dynamodb_methods(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // First pass: collect variable -> table name mappings
        // e.g., `table = dynamodb.Table('my-table')` maps "table" -> "my-table"
        let table_mappings = self.collect_table_assignments(tree.root_node(), content);

        self.walk_for_dynamodb_methods(
            tree.root_node(),
            content,
            path,
            &mut discoveries,
            &table_mappings,
        );

        discoveries
    }

    /// Collect table variable assignments (e.g., `table = dynamodb.Table('name')`).
    /// Returns a mapping of variable name to table name.
    fn collect_table_assignments(&self, node: Node, content: &str) -> HashMap<String, String> {
        let mut mappings = HashMap::new();
        self.walk_for_table_assignments(node, content, &mut mappings);
        mappings
    }

    /// Walk AST looking for table variable assignments.
    fn walk_for_table_assignments(
        &self,
        node: Node,
        content: &str,
        mappings: &mut HashMap<String, String>,
    ) {
        // Look for assignment patterns:
        // - expression_statement containing assignment (Python 3)
        // - assignment itself
        if node.kind() == "expression_statement" || node.kind() == "assignment" {
            self.check_table_assignment(node, content, mappings);
        }

        // Recursively walk children
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                self.walk_for_table_assignments(child, content, mappings);
            }
        }
    }

    /// Check if a node is a table assignment and extract the mapping.
    fn check_table_assignment(
        &self,
        node: Node,
        content: &str,
        mappings: &mut HashMap<String, String>,
    ) {
        // Find the assignment node (may be direct or child of expression_statement)
        let assignment = if node.kind() == "assignment" {
            Some(node)
        } else {
            node.named_child(0).filter(|c| c.kind() == "assignment")
        };

        if let Some(assignment) = assignment {
            // Get the left side (variable name)
            let left = assignment.child_by_field_name("left");
            // Get the right side (should be a call to .Table('name'))
            let right = assignment.child_by_field_name("right");

            if let (Some(left_node), Some(right_node)) = (left, right) {
                // Left should be an identifier
                if left_node.kind() == "identifier" {
                    let var_name = left_node.utf8_text(content.as_bytes()).unwrap_or("");

                    // Right should be a call expression
                    if right_node.kind() == "call" {
                        if let Some(table_name) =
                            self.extract_table_name_from_call(right_node, content)
                        {
                            mappings.insert(var_name.to_string(), table_name);
                        }
                    }
                }
            }
        }
    }

    /// Extract table name from a call like `dynamodb.Table('my-table')` or `resource.Table('my-table')`.
    fn extract_table_name_from_call(&self, call_node: Node, content: &str) -> Option<String> {
        // Check if this is a .Table() call
        if let Some(function_node) = call_node.child_by_field_name("function") {
            if function_node.kind() == "attribute" {
                if let Some(attr_node) = function_node.child_by_field_name("attribute") {
                    let method = attr_node.utf8_text(content.as_bytes()).unwrap_or("");
                    if method == "Table" {
                        // Extract the first argument as the table name
                        if let Some(args_node) = call_node.child_by_field_name("arguments") {
                            return self.extract_first_string_arg(args_node, content);
                        }
                    }
                }
            }
        }
        None
    }

    /// Walk AST looking for DynamoDB method calls.
    fn walk_for_dynamodb_methods(
        &self,
        node: Node,
        content: &str,
        path: &Path,
        discoveries: &mut Vec<Discovery>,
        table_mappings: &HashMap<String, String>,
    ) {
        if node.kind() == "call" {
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "attribute" {
                    if let Some(method_node) = function_node.child_by_field_name("attribute") {
                        let method = method_node.utf8_text(content.as_bytes()).unwrap_or("");

                        // DynamoDB method names and their operation types
                        let dynamodb_methods = [
                            ("get_item", DatabaseOperation::Read),
                            ("put_item", DatabaseOperation::Write),
                            ("update_item", DatabaseOperation::ReadWrite),
                            ("delete_item", DatabaseOperation::Write),
                            ("query", DatabaseOperation::Read),
                            ("scan", DatabaseOperation::Read),
                            ("batch_get_item", DatabaseOperation::Read),
                            ("batch_write_item", DatabaseOperation::Write),
                            ("transact_get_items", DatabaseOperation::Read),
                            ("transact_write_items", DatabaseOperation::Write),
                        ];

                        for (method_name, operation) in &dynamodb_methods {
                            if method == *method_name {
                                // Try to extract table name from arguments first
                                let mut table_name = if let Some(args_node) =
                                    node.child_by_field_name("arguments")
                                {
                                    self.extract_table_name(args_node, content)
                                } else {
                                    None
                                };

                                // If no table name from arguments, try to get it from
                                // the object variable (e.g., `table` in `table.get_item(...)`)
                                if table_name.is_none() {
                                    if let Some(object_node) =
                                        function_node.child_by_field_name("object")
                                    {
                                        if object_node.kind() == "identifier" {
                                            let var_name = object_node
                                                .utf8_text(content.as_bytes())
                                                .unwrap_or("");
                                            table_name = table_mappings.get(var_name).cloned();
                                        }
                                    }
                                }

                                discoveries.push(Discovery::DatabaseAccess(
                                    DatabaseAccessDiscovery {
                                        db_type: "dynamodb".to_string(),
                                        table_name,
                                        operation: *operation,
                                        detection_method: format!("boto3.{}", method),
                                        source_file: path.to_string_lossy().to_string(),
                                        source_line: method_node.start_position().row as u32 + 1,
                                    },
                                ));
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
                self.walk_for_dynamodb_methods(child, content, path, discoveries, table_mappings);
            }
        }
    }

    /// Extract table name from DynamoDB method call arguments.
    fn extract_table_name(&self, args_node: Node, content: &str) -> Option<String> {
        // Look for TableName='xxx' or TableName="xxx" in the arguments
        // This can be a keyword argument or part of a dict
        for i in 0..args_node.named_child_count() {
            if let Some(child) = args_node.named_child(i) {
                match child.kind() {
                    "keyword_argument" => {
                        // Check if this is TableName=...
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = name_node.utf8_text(content.as_bytes()).unwrap_or("");
                            if name == "TableName" {
                                if let Some(value_node) = child.child_by_field_name("value") {
                                    if value_node.kind() == "string" {
                                        let value =
                                            value_node.utf8_text(content.as_bytes()).unwrap_or("");
                                        return Some(
                                            value
                                                .trim_matches(|c| c == '"' || c == '\'')
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    "dictionary" => {
                        // Look for 'TableName': '...' in a dict
                        if let Some(table_name) = self.find_table_name_in_dict(child, content) {
                            return Some(table_name);
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// Find TableName in a dictionary literal.
    fn find_table_name_in_dict(&self, dict_node: Node, content: &str) -> Option<String> {
        for i in 0..dict_node.named_child_count() {
            if let Some(child) = dict_node.named_child(i) {
                if child.kind() == "pair" {
                    if let Some(key_node) = child.child_by_field_name("key") {
                        let key = key_node
                            .utf8_text(content.as_bytes())
                            .unwrap_or("")
                            .trim_matches(|c| c == '"' || c == '\'');
                        if key == "TableName" {
                            if let Some(value_node) = child.child_by_field_name("value") {
                                if value_node.kind() == "string" {
                                    let value =
                                        value_node.utf8_text(content.as_bytes()).unwrap_or("");
                                    return Some(
                                        value.trim_matches(|c| c == '"' || c == '\'').to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

impl Parser for PythonParser {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supported_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        // Create a new parser instance for thread safety (tree-sitter parsers are not thread-safe)
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
        discoveries.extend(self.detect_boto3_clients(&tree, content, path));
        discoveries.extend(self.detect_http_clients(&tree, content, path));
        discoveries.extend(self.detect_dynamodb_methods(&tree, content, path));

        Ok(discoveries)
    }
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new().expect("Failed to create default PythonParser")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_parser() -> PythonParser {
        PythonParser::new().expect("Failed to create parser")
    }

    // ===================
    // Import Detection Tests
    // ===================

    #[test]
    fn test_detect_simple_imports() {
        let parser = create_parser();
        let content = r#"
import os
import sys
import boto3
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(
            imports.len() >= 3,
            "Expected at least 3 imports, got {}",
            imports.len()
        );
        assert!(
            imports.iter().any(|i| i.module == "os"),
            "Should detect os import"
        );
        assert!(
            imports.iter().any(|i| i.module == "boto3"),
            "Should detect boto3 import"
        );
    }

    #[test]
    fn test_detect_from_imports() {
        let parser = create_parser();
        let content = r#"
from typing import Dict, List
from os.path import join
from boto3.dynamodb.conditions import Key, Attr
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(
            imports.iter().any(|i| i.module == "typing"),
            "Should detect typing import"
        );
        assert!(
            imports.iter().any(|i| i.module == "os.path"),
            "Should detect os.path import"
        );
        assert!(
            imports
                .iter()
                .any(|i| i.module == "boto3.dynamodb.conditions"),
            "Should detect boto3.dynamodb.conditions import"
        );
    }

    #[test]
    fn test_detect_relative_imports() {
        let parser = create_parser();
        let content = r#"
from . import utils
from .. import config
from .models import User
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        let relative_imports: Vec<_> = imports.iter().filter(|i| i.is_relative).collect();
        assert!(
            !relative_imports.is_empty(),
            "Should detect relative imports"
        );
    }

    // ===================
    // boto3 Client Detection Tests
    // ===================

    #[test]
    fn test_detect_boto3_dynamodb_client() {
        let parser = create_parser();
        let content = r#"
import boto3

dynamodb = boto3.client('dynamodb')
result = dynamodb.get_item(TableName='users', Key={'id': {'S': '123'}})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_accesses.iter().any(|d| d.db_type == "dynamodb"),
            "Should detect DynamoDB access"
        );
    }

    #[test]
    fn test_detect_boto3_dynamodb_resource() {
        let parser = create_parser();
        // DynamoDB detection requires actual method calls (get_item, put_item, etc.)
        // not just boto3.resource('dynamodb') - this avoids "dynamodb-unknown" nodes
        let content = r#"
import boto3

dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table('users')
result = table.get_item(Key={'id': '123'})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_accesses
                .iter()
                .any(|d| d.db_type == "dynamodb" && d.table_name == Some("users".to_string())),
            "Should detect DynamoDB table access with table name 'users'"
        );
    }

    #[test]
    fn test_detect_boto3_s3() {
        let parser = create_parser();
        let content = r#"
import boto3

s3 = boto3.client('s3')
s3.upload_file('local.txt', 'my-bucket', 'remote.txt')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) => Some(r),
                _ => None,
            })
            .collect();

        assert!(
            resources.iter().any(|r| r.resource_type == "s3"),
            "Should detect S3 usage"
        );
    }

    #[test]
    fn test_detect_boto3_sqs() {
        let parser = create_parser();
        let content = r#"
import boto3

sqs = boto3.client('sqs')
sqs.send_message(QueueUrl='https://sqs...', MessageBody='test')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let queue_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert!(
            queue_ops.iter().any(|q| q.queue_type == "sqs"),
            "Should detect SQS usage"
        );
    }

    #[test]
    fn test_detect_boto3_sns() {
        let parser = create_parser();
        let content = r#"
import boto3

sns = boto3.client('sns')
sns.publish(TopicArn='arn:aws:sns:...', Message='hello')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let queue_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert!(
            queue_ops.iter().any(|q| q.queue_type == "sns"),
            "Should detect SNS usage"
        );
        assert!(
            queue_ops
                .iter()
                .any(|q| q.operation == QueueOperationType::Publish),
            "SNS should be detected as Publish operation"
        );
    }

    #[test]
    fn test_detect_boto3_lambda() {
        let parser = create_parser();
        let content = r#"
import boto3

lambda_client = boto3.client('lambda')
lambda_client.invoke(FunctionName='my-function')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) => Some(r),
                _ => None,
            })
            .collect();

        assert!(
            resources.iter().any(|r| r.resource_type == "lambda"),
            "Should detect Lambda usage"
        );
    }

    #[test]
    fn test_detect_boto3_eventbridge() {
        let parser = create_parser();
        let content = r#"
import boto3

events = boto3.client('events')
events.put_events(Entries=[{}])
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let queue_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert!(
            queue_ops.iter().any(|q| q.queue_type == "eventbridge"),
            "Should detect EventBridge usage"
        );
    }

    // ===================
    // DynamoDB Method Detection Tests
    // ===================

    #[test]
    fn test_detect_dynamodb_get_item() {
        let parser = create_parser();
        let content = r#"
result = table.get_item(TableName='users', Key={'id': {'S': '123'}})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Read),
            "get_item should be Read operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_put_item() {
        let parser = create_parser();
        let content = r#"
table.put_item(TableName='users', Item={'id': {'S': '123'}, 'name': {'S': 'John'}})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Write),
            "put_item should be Write operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_update_item() {
        let parser = create_parser();
        let content = r#"
table.update_item(TableName='users', Key={'id': {'S': '123'}}, UpdateExpression='SET #n = :v')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::ReadWrite),
            "update_item should be ReadWrite operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_query() {
        let parser = create_parser();
        let content = r#"
response = table.query(TableName='orders', KeyConditionExpression='user_id = :uid')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Read),
            "query should be Read operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_scan() {
        let parser = create_parser();
        let content = r#"
response = table.scan(TableName='users')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Read),
            "scan should be Read operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_batch_operations() {
        let parser = create_parser();
        let content = r#"
response = client.batch_get_item(RequestItems={'users': {'Keys': []}})
client.batch_write_item(RequestItems={'users': {'DeleteRequest': []}})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Read),
            "batch_get_item should be Read operation"
        );
        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Write),
            "batch_write_item should be Write operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_transact_operations() {
        let parser = create_parser();
        let content = r#"
response = client.transact_get_items(TransactItems=[])
client.transact_write_items(TransactItems=[])
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Read),
            "transact_get_items should be Read operation"
        );
        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Write),
            "transact_write_items should be Write operation"
        );
    }

    #[test]
    fn test_extract_dynamodb_table_name() {
        let parser = create_parser();
        let content = r#"
result = table.get_item(TableName='users-table', Key={'id': {'S': '123'}})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.table_name.as_deref() == Some("users-table")),
            "Should extract table name"
        );
    }

    // ===================
    // HTTP Client Detection Tests
    // ===================

    #[test]
    fn test_detect_requests_get() {
        let parser = create_parser();
        let content = r#"
import requests

response = requests.get('https://api.example.com/users')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(!api_calls.is_empty(), "Should detect requests.get call");
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("GET".to_string())),
            "Should detect GET method"
        );
        assert!(
            api_calls.iter().any(|a| a.detection_method == "requests"),
            "Should identify requests as detection method"
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.target.contains("api.example.com")),
            "Should extract URL"
        );
    }

    #[test]
    fn test_detect_requests_post() {
        let parser = create_parser();
        let content = r#"
import requests

response = requests.post('https://api.example.com/users', json={'name': 'John'})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("POST".to_string())),
            "Should detect POST method"
        );
    }

    #[test]
    fn test_detect_requests_multiple_methods() {
        let parser = create_parser();
        let content = r#"
import requests

requests.get('https://api.example.com/users')
requests.post('https://api.example.com/users', json={})
requests.put('https://api.example.com/users/1', json={})
requests.delete('https://api.example.com/users/1')
requests.patch('https://api.example.com/users/1', json={})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(api_calls.len() >= 5, "Should detect all HTTP method calls");
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("GET".to_string()))
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("POST".to_string()))
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("PUT".to_string()))
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("DELETE".to_string()))
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("PATCH".to_string()))
        );
    }

    #[test]
    fn test_detect_httpx_calls() {
        let parser = create_parser();
        let content = r#"
import httpx

response = httpx.get('https://api.example.com/users')
response = httpx.post('https://api.example.com/orders', json={'item': 'test'})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(api_calls.len() >= 2, "Should detect httpx calls");
        assert!(
            api_calls.iter().any(|a| a.detection_method == "httpx"),
            "Should identify httpx as detection method"
        );
    }

    // ===================
    // Project Configuration Tests
    // ===================

    #[test]
    fn test_parse_pyproject_toml() {
        let dir = tempfile::tempdir().unwrap();

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

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.name, "my-service");
        assert_eq!(service.framework, Some("fastapi".to_string()));
        assert_eq!(service.language, "python");
    }

    #[test]
    fn test_parse_pyproject_toml_poetry() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[tool.poetry]
name = "poetry-service"
version = "1.0.0"

[tool.poetry.dependencies]
flask = "^2.0.0"
"#,
        )
        .unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.name, "poetry-service");
        assert_eq!(service.framework, Some("flask".to_string()));
    }

    #[test]
    fn test_parse_setup_py() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("setup.py"),
            r#"
from setuptools import setup

setup(
    name='my-setup-service',
    version='1.0.0',
    install_requires=['django'],
)
"#,
        )
        .unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.name, "my-setup-service");
    }

    #[test]
    fn test_parse_requirements_txt() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("requirements.txt"),
            r#"
boto3==1.28.0
fastapi>=0.100.0
uvicorn
"#,
        )
        .unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.framework, Some("fastapi".to_string()));
    }

    #[test]
    fn test_detect_django_framework() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("requirements.txt"),
            r#"
django>=4.0
djangorestframework
"#,
        )
        .unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.framework, Some("django".to_string()));
    }

    #[test]
    fn test_detect_chalice_framework() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("requirements.txt"),
            r#"
chalice
boto3
"#,
        )
        .unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.framework, Some("chalice".to_string()));
    }

    #[test]
    fn test_detect_starlette_framework() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("requirements.txt"),
            r#"
starlette
uvicorn
"#,
        )
        .unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.framework, Some("starlette".to_string()));
    }

    // ===================
    // Entry Point Detection Tests
    // ===================

    #[test]
    fn test_find_entry_point_main_py() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.py"), "# main").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.entry_point, "main.py");
    }

    #[test]
    fn test_find_entry_point_app_py() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.py"), "# app").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.entry_point, "app.py");
    }

    #[test]
    fn test_find_entry_point_in_src() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.py"), "# main").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path()).unwrap();

        assert_eq!(service.entry_point, "src/main.py");
    }

    // ===================
    // Edge Case Tests
    // ===================

    #[test]
    fn test_empty_file() {
        let parser = create_parser();
        let discoveries = parser.parse_file(Path::new("test.py"), "").unwrap();

        assert!(
            discoveries.is_empty(),
            "Empty file should produce no discoveries"
        );
    }

    #[test]
    fn test_file_with_comments_only() {
        let parser = create_parser();
        let content = r#"
# This is a comment
"""
Multi-line
docstring
"""
"#;
        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        assert!(
            discoveries.is_empty(),
            "File with only comments should produce no discoveries"
        );
    }

    #[test]
    fn test_supported_extensions() {
        let parser = create_parser();
        let extensions = parser.supported_extensions();

        assert!(extensions.contains(&"py"));
        assert_eq!(extensions.len(), 1);
    }

    #[test]
    fn test_mixed_code() {
        let parser = create_parser();
        let content = r#"
import boto3
import requests
from typing import Dict

dynamodb = boto3.client('dynamodb')
s3 = boto3.client('s3')

def get_user(user_id: str) -> Dict:
    response = dynamodb.get_item(TableName='users', Key={'id': {'S': user_id}})
    return response['Item']

def notify_external_service(data: Dict):
    requests.post('https://webhook.example.com/notify', json=data)
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        // Should have imports
        let imports: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::Import(_)))
            .collect();
        assert!(imports.len() >= 3, "Should have at least 3 imports");

        // Should have database access
        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::DatabaseAccess(_)))
            .collect();
        assert!(!db_accesses.is_empty(), "Should have database access");

        // Should have cloud resource usage (S3)
        let cloud_resources: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::CloudResourceUsage(_)))
            .collect();
        assert!(
            !cloud_resources.is_empty(),
            "Should have cloud resource usage"
        );

        // Should have API call
        let api_calls: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::ApiCall(_)))
            .collect();
        assert!(!api_calls.is_empty(), "Should have API call");
    }

    #[test]
    fn test_no_project_config() {
        let dir = tempfile::tempdir().unwrap();

        let parser = create_parser();
        let service = parser.parse_project_config(dir.path());

        assert!(
            service.is_none(),
            "Should return None when no project config exists"
        );
    }

    #[test]
    fn test_boto3_with_session() {
        let parser = create_parser();
        // This pattern shouldn't match our simple query, but shouldn't crash either
        let content = r#"
import boto3

session = boto3.Session(profile_name='myprofile')
client = session.client('dynamodb')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        // Should at least detect the import
        let imports: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::Import(_)))
            .collect();
        assert!(!imports.is_empty(), "Should detect boto3 import");
    }

    #[test]
    fn test_line_numbers_are_correct() {
        let parser = create_parser();
        // DynamoDB detection requires actual method calls, not just boto3.client()
        let content = r#"import boto3

dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table('users')
result = table.get_item(Key={'id': '123'})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(!imports.is_empty());
        assert_eq!(imports[0].source_line, 1, "Import should be on line 1");

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(!db_accesses.is_empty());
        assert_eq!(
            db_accesses[0].source_line, 5,
            "table.get_item should be on line 5"
        );
    }

    #[test]
    fn test_default_parser() {
        // Test that Default implementation works
        let parser = PythonParser::default();
        assert!(parser.supported_extensions().contains(&"py"));
    }

    #[test]
    fn test_generic_aws_service() {
        let parser = create_parser();
        let content = r#"
import boto3

secretsmanager = boto3.client('secretsmanager')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) => Some(r),
                _ => None,
            })
            .collect();

        assert!(
            resources
                .iter()
                .any(|r| r.resource_type == "secretsmanager"),
            "Should detect generic AWS service"
        );
    }

    #[test]
    fn test_delete_item_operation() {
        let parser = create_parser();
        let content = r#"
table.delete_item(TableName='users', Key={'id': {'S': '123'}})
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Write
                    && d.detection_method.contains("delete_item")),
            "delete_item should be Write operation"
        );
    }

    #[test]
    fn test_http_head_and_options() {
        let parser = create_parser();
        let content = r#"
import requests

requests.head('https://api.example.com/health')
requests.options('https://api.example.com/users')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("HEAD".to_string()))
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("OPTIONS".to_string()))
        );
    }
}
