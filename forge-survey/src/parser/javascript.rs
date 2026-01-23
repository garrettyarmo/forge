//! JavaScript and TypeScript parser for Forge survey.
//!
//! This parser uses tree-sitter to analyze JavaScript/TypeScript files and detect:
//! - ES6 imports (`import X from 'Y'`)
//! - CommonJS requires (`require('Y')`)
//! - AWS SDK v2 and v3 usage
//! - DynamoDB operations (get, put, update, delete, query, scan)
//! - HTTP client usage (axios, fetch)
//! - Service metadata from package.json
//!
//! The parser is deterministic - it uses only AST analysis with no LLM calls.

use super::traits::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError,
    ServiceDiscovery,
};
use std::any::Any;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser as TSParser, Query, QueryCursor};

/// Parser for JavaScript and TypeScript files.
///
/// Uses tree-sitter queries to detect:
/// - Import statements (ES6 and CommonJS)
/// - AWS SDK usage patterns
/// - HTTP client calls
/// - DynamoDB operations
pub struct JavaScriptParser {
    language: Language,
}

impl JavaScriptParser {
    /// Create a new JavaScript parser.
    ///
    /// # Errors
    /// Returns an error if tree-sitter initialization fails.
    pub fn new() -> Result<Self, ParserError> {
        let language = tree_sitter_javascript::LANGUAGE.into();

        // Verify the language is valid by trying to set it on a parser
        let mut parser = TSParser::new();
        parser
            .set_language(&language)
            .map_err(|e| ParserError::TreeSitterError(format!("Failed to set language: {}", e)))?;

        Ok(Self { language })
    }

    /// Parse package.json to extract service metadata.
    ///
    /// Detects:
    /// - Service name from the "name" field
    /// - Entry point from the "main" field
    /// - Framework from dependencies (express, fastify, koa, etc.)
    pub fn parse_package_json(&self, repo_path: &Path) -> Option<ServiceDiscovery> {
        let package_json_path = repo_path.join("package.json");
        if !package_json_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&package_json_path).ok()?;
        let package: serde_json::Value = serde_json::from_str(&content).ok()?;

        let name = package.get("name")?.as_str()?.to_string();

        // Detect framework from dependencies
        let framework = self.detect_framework_from_package(&package);

        // Find entry point
        let entry_point = package
            .get("main")
            .and_then(|m| m.as_str())
            .unwrap_or("index.js")
            .to_string();

        // Detect language based on TypeScript dependency or tsconfig presence
        let language = if package
            .get("devDependencies")
            .and_then(|d| d.get("typescript"))
            .is_some()
            || repo_path.join("tsconfig.json").exists()
        {
            "typescript"
        } else {
            "javascript"
        };

        Some(ServiceDiscovery {
            name,
            language: language.to_string(),
            framework,
            entry_point,
            source_file: package_json_path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    /// Detect the web framework from package.json dependencies.
    fn detect_framework_from_package(&self, package: &serde_json::Value) -> Option<String> {
        let deps = package.get("dependencies")?;

        // Check for various frameworks in order of specificity
        if deps.get("@nestjs/core").is_some() {
            Some("nestjs".to_string())
        } else if deps.get("next").is_some() {
            Some("next.js".to_string())
        } else if deps.get("nuxt").is_some() {
            Some("nuxt".to_string())
        } else if deps.get("express").is_some() {
            Some("express".to_string())
        } else if deps.get("fastify").is_some() {
            Some("fastify".to_string())
        } else if deps.get("koa").is_some() {
            Some("koa".to_string())
        } else if deps.get("hapi").is_some() || deps.get("@hapi/hapi").is_some() {
            Some("hapi".to_string())
        } else {
            None
        }
    }

    /// Detect import statements (ES6 and CommonJS).
    fn detect_imports(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Query for ES6 imports: import X from 'Y'
        let import_query = match Query::new(
            &self.language,
            r#"
            (import_statement
              source: (string) @source)
            "#,
        ) {
            Ok(q) => q,
            Err(_) => return discoveries,
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&import_query, tree.root_node(), content.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let node = capture.node;
                let text = node.utf8_text(content.as_bytes()).unwrap_or("");
                // Remove quotes from string
                let module = text
                    .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                    .to_string();

                if !module.is_empty() {
                    discoveries.push(Discovery::Import(ImportDiscovery {
                        module: module.clone(),
                        is_relative: module.starts_with('.'),
                        imported_items: self.extract_import_specifiers(tree, node, content),
                        source_file: path.to_string_lossy().to_string(),
                        source_line: node.start_position().row as u32 + 1,
                    }));
                }
            }
        }

        // Query for CommonJS requires: require('Y')
        let require_query = match Query::new(
            &self.language,
            r#"
            (call_expression
              function: (identifier) @func
              arguments: (arguments (string) @source)
              (#eq? @func "require"))
            "#,
        ) {
            Ok(q) => q,
            Err(_) => return discoveries,
        };

        let source_index = require_query
            .capture_names()
            .iter()
            .position(|n| *n == "source")
            .unwrap_or(1);

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&require_query, tree.root_node(), content.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                if capture.index as usize == source_index {
                    let node = capture.node;
                    let text = node.utf8_text(content.as_bytes()).unwrap_or("");
                    let module = text
                        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                        .to_string();

                    if !module.is_empty() {
                        discoveries.push(Discovery::Import(ImportDiscovery {
                            module: module.clone(),
                            is_relative: module.starts_with('.'),
                            imported_items: vec![],
                            source_file: path.to_string_lossy().to_string(),
                            source_line: node.start_position().row as u32 + 1,
                        }));
                    }
                }
            }
        }

        discoveries
    }

    /// Extract named import specifiers from an import statement.
    fn extract_import_specifiers(
        &self,
        _tree: &tree_sitter::Tree,
        source_node: Node,
        content: &str,
    ) -> Vec<String> {
        let mut items = Vec::new();

        // Walk up to the import_statement and find import_clause
        if let Some(import_stmt) = source_node.parent() {
            // Find the import_clause child
            for i in 0..import_stmt.named_child_count() {
                if let Some(child) = import_stmt.named_child(i) {
                    if child.kind() == "import_clause" {
                        // Check for named_imports
                        for j in 0..child.named_child_count() {
                            if let Some(named) = child.named_child(j) {
                                if named.kind() == "named_imports" {
                                    // Extract import specifiers
                                    for k in 0..named.named_child_count() {
                                        if let Some(spec) = named.named_child(k) {
                                            if spec.kind() == "import_specifier" {
                                                if let Some(name) = spec.named_child(0) {
                                                    if let Ok(text) =
                                                        name.utf8_text(content.as_bytes())
                                                    {
                                                        items.push(text.to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else if named.kind() == "identifier" {
                                    // Default import
                                    if let Ok(text) = named.utf8_text(content.as_bytes()) {
                                        items.push(text.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        items
    }

    /// Detect AWS SDK usage patterns.
    fn detect_aws_sdk(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // Find all import statements and require calls that reference AWS SDK
        let root = tree.root_node();
        self.walk_for_aws_imports(&root, content, path, &mut discoveries);

        discoveries
    }

    /// Walk the AST looking for AWS SDK imports.
    fn walk_for_aws_imports(
        &self,
        node: &Node,
        content: &str,
        path: &Path,
        discoveries: &mut Vec<Discovery>,
    ) {
        // Check if this node is an import or require of AWS SDK
        if node.kind() == "import_statement" {
            if let Some(source) = node.child_by_field_name("source") {
                let text = source.utf8_text(content.as_bytes()).unwrap_or("");
                let module = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');

                self.process_aws_import(
                    module,
                    source.start_position().row as u32 + 1,
                    path,
                    discoveries,
                );
            }
        } else if node.kind() == "call_expression" {
            // Check for require('aws-sdk') or require('@aws-sdk/...')
            if let Some(func) = node.child_by_field_name("function") {
                if func.utf8_text(content.as_bytes()).unwrap_or("") == "require" {
                    if let Some(args) = node.child_by_field_name("arguments") {
                        if let Some(first_arg) = args.named_child(0) {
                            let text = first_arg.utf8_text(content.as_bytes()).unwrap_or("");
                            let module = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');

                            self.process_aws_import(
                                module,
                                first_arg.start_position().row as u32 + 1,
                                path,
                                discoveries,
                            );
                        }
                    }
                }
            }
        }

        // Recursively walk children
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                self.walk_for_aws_imports(&child, content, path, discoveries);
            }
        }
    }

    /// Process an AWS SDK import and create appropriate discoveries.
    fn process_aws_import(
        &self,
        module: &str,
        line: u32,
        path: &Path,
        discoveries: &mut Vec<Discovery>,
    ) {
        let module_lower = module.to_lowercase();

        // Check for AWS SDK imports
        if !module_lower.contains("aws-sdk") && !module_lower.contains("@aws-sdk") {
            return;
        }

        // Determine the AWS service
        if module_lower.contains("dynamodb") {
            discoveries.push(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
                db_type: "dynamodb".to_string(),
                table_name: None,
                operation: DatabaseOperation::Unknown,
                detection_method: if module.contains("@aws-sdk") {
                    "aws-sdk-v3".to_string()
                } else {
                    "aws-sdk-v2".to_string()
                },
                source_file: path.to_string_lossy().to_string(),
                source_line: line,
            }));
        } else if module_lower.contains("sqs") {
            // NOTE: Don't create a QueueOperation here!
            // SQS queue discoveries should be created from actual sendMessage/receiveMessage calls
            // where we can extract the queue name from QueueUrl.
            // Creating one here with queue_name: None leads to "sqs-unknown" nodes
            // that can't be properly deduplicated.
        } else if module_lower.contains("sns") {
            // NOTE: Don't create a QueueOperation here for the same reason as SQS.
            // SNS topic discoveries should be created from actual publish calls.
        } else if module_lower.contains("s3") {
            discoveries.push(Discovery::CloudResourceUsage(CloudResourceDiscovery {
                resource_type: "s3".to_string(),
                resource_name: None,
                source_file: path.to_string_lossy().to_string(),
                source_line: line,
            }));
        } else if module_lower.contains("lambda") {
            discoveries.push(Discovery::CloudResourceUsage(CloudResourceDiscovery {
                resource_type: "lambda".to_string(),
                resource_name: None,
                source_file: path.to_string_lossy().to_string(),
                source_line: line,
            }));
        } else if module == "aws-sdk" || module == "@aws-sdk/client-sts" {
            // Generic AWS SDK import - don't create specific discovery
            // This is usually followed by specific service client creation
        }
    }

    /// Detect DynamoDB operation calls.
    fn detect_dynamodb_operations(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        // DynamoDB method names to detect
        let dynamodb_methods = [
            ("get", DatabaseOperation::Read),
            ("getItem", DatabaseOperation::Read),
            ("query", DatabaseOperation::Read),
            ("scan", DatabaseOperation::Read),
            ("batchGet", DatabaseOperation::Read),
            ("batchGetItem", DatabaseOperation::Read),
            ("put", DatabaseOperation::Write),
            ("putItem", DatabaseOperation::Write),
            ("delete", DatabaseOperation::Write),
            ("deleteItem", DatabaseOperation::Write),
            ("batchWrite", DatabaseOperation::Write),
            ("batchWriteItem", DatabaseOperation::Write),
            ("update", DatabaseOperation::ReadWrite),
            ("updateItem", DatabaseOperation::ReadWrite),
            ("transactGet", DatabaseOperation::Read),
            ("transactGetItems", DatabaseOperation::Read),
            ("transactWrite", DatabaseOperation::Write),
            ("transactWriteItems", DatabaseOperation::Write),
        ];

        // Walk the AST looking for method calls
        self.walk_for_method_calls(
            tree.root_node(),
            content,
            path,
            &dynamodb_methods,
            &mut discoveries,
        );

        discoveries
    }

    /// Walk AST looking for specific method calls on DynamoDB-like objects.
    fn walk_for_method_calls(
        &self,
        node: Node,
        content: &str,
        path: &Path,
        methods: &[(&str, DatabaseOperation)],
        discoveries: &mut Vec<Discovery>,
    ) {
        if node.kind() == "call_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                if func.kind() == "member_expression" {
                    if let Some(prop) = func.child_by_field_name("property") {
                        let method_name = prop.utf8_text(content.as_bytes()).unwrap_or("");

                        // Check if the object looks like a DynamoDB client
                        // to avoid false positives like axios.get() being detected as DynamoDB
                        if let Some(obj) = func.child_by_field_name("object") {
                            if !self.is_dynamodb_like_object(obj, content) {
                                // Skip - not a DynamoDB client
                            } else {
                                for (method, operation) in methods {
                                    if method_name == *method {
                                        // Try to extract table name from arguments
                                        let table_name =
                                            self.extract_table_name_from_call(&node, content);

                                        discoveries.push(Discovery::DatabaseAccess(
                                            DatabaseAccessDiscovery {
                                                db_type: "dynamodb".to_string(),
                                                table_name,
                                                operation: *operation,
                                                detection_method: "method-call".to_string(),
                                                source_file: path.to_string_lossy().to_string(),
                                                source_line: node.start_position().row as u32 + 1,
                                            },
                                        ));
                                        break;
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
                self.walk_for_method_calls(child, content, path, methods, discoveries);
            }
        }
    }

    /// Check if an AST node represents a DynamoDB-like object.
    /// This helps avoid false positives like axios.get() being detected as DynamoDB.
    fn is_dynamodb_like_object(&self, node: Node, content: &str) -> bool {
        let text = node.utf8_text(content.as_bytes()).unwrap_or("").to_lowercase();

        // Common DynamoDB client variable names
        let dynamodb_names = [
            "dynamodb",
            "ddb",
            "dynamo",
            "docclient",
            "doc_client",
            "docClient",
            "documentclient",
            "document_client",
            "documentClient",
            "dynamodbclient",
            "dynamodb_client",
            "dynamoDBClient",
            "table",
        ];

        // Check if the identifier contains any DynamoDB-related names
        for name in &dynamodb_names {
            if text.contains(&name.to_lowercase()) {
                return true;
            }
        }

        // Also check for new DynamoDB() or new DocumentClient() patterns
        if node.kind() == "new_expression"
            && (text.contains("dynamodb") || text.contains("documentclient"))
        {
            return true;
        }

        false
    }

    /// Try to extract table name from a DynamoDB call.
    fn extract_table_name_from_call(&self, call_node: &Node, content: &str) -> Option<String> {
        // Look for TableName in the arguments
        if let Some(args) = call_node.child_by_field_name("arguments") {
            for i in 0..args.named_child_count() {
                if let Some(arg) = args.named_child(i) {
                    // Check if it's an object with TableName property
                    if arg.kind() == "object" {
                        return self.find_table_name_in_object(arg, content);
                    }
                }
            }
        }
        None
    }

    /// Find TableName property in an object literal.
    fn find_table_name_in_object(&self, obj_node: Node, content: &str) -> Option<String> {
        for i in 0..obj_node.named_child_count() {
            if let Some(child) = obj_node.named_child(i) {
                if child.kind() == "pair" {
                    if let Some(key) = child.child_by_field_name("key") {
                        let key_text = key.utf8_text(content.as_bytes()).unwrap_or("");
                        if key_text == "TableName" {
                            if let Some(value) = child.child_by_field_name("value") {
                                let value_text = value.utf8_text(content.as_bytes()).unwrap_or("");
                                return Some(
                                    value_text
                                        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                                        .to_string(),
                                );
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Detect HTTP client usage (axios, fetch).
    fn detect_http_calls(
        &self,
        tree: &tree_sitter::Tree,
        content: &str,
        path: &Path,
    ) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        let root = tree.root_node();
        self.walk_for_http_calls(root, content, path, &mut discoveries);

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
        if node.kind() == "call_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                let (is_http_call, method, detection_method) = self.check_http_call(&func, content);

                if is_http_call {
                    // Try to extract URL from arguments
                    let target = self.extract_url_from_call(&node, content);

                    discoveries.push(Discovery::ApiCall(ApiCallDiscovery {
                        target: target.unwrap_or_else(|| "unknown".to_string()),
                        method,
                        detection_method,
                        source_file: path.to_string_lossy().to_string(),
                        source_line: node.start_position().row as u32 + 1,
                    }));
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

    /// Check if a function node represents an HTTP call.
    fn check_http_call(&self, func: &Node, content: &str) -> (bool, Option<String>, String) {
        // Check for direct fetch() call
        if func.kind() == "identifier" {
            let name = func.utf8_text(content.as_bytes()).unwrap_or("");
            if name == "fetch" {
                return (true, None, "fetch".to_string());
            }
        }

        // Check for axios calls: axios.get(), axios.post(), etc.
        if func.kind() == "member_expression" {
            if let Some(obj) = func.child_by_field_name("object") {
                let obj_name = obj.utf8_text(content.as_bytes()).unwrap_or("");
                if obj_name == "axios" {
                    if let Some(prop) = func.child_by_field_name("property") {
                        let method_name = prop.utf8_text(content.as_bytes()).unwrap_or("");
                        let http_method = match method_name {
                            "get" => Some("GET".to_string()),
                            "post" => Some("POST".to_string()),
                            "put" => Some("PUT".to_string()),
                            "delete" => Some("DELETE".to_string()),
                            "patch" => Some("PATCH".to_string()),
                            "head" => Some("HEAD".to_string()),
                            "options" => Some("OPTIONS".to_string()),
                            "request" => None,
                            _ => return (false, None, String::new()),
                        };
                        return (true, http_method, "axios".to_string());
                    }
                }
            }
        }

        // Check for direct axios() call
        if func.kind() == "identifier" {
            let name = func.utf8_text(content.as_bytes()).unwrap_or("");
            if name == "axios" {
                return (true, None, "axios".to_string());
            }
        }

        (false, None, String::new())
    }

    /// Extract URL from HTTP call arguments.
    fn extract_url_from_call(&self, call_node: &Node, content: &str) -> Option<String> {
        if let Some(args) = call_node.child_by_field_name("arguments") {
            if let Some(first_arg) = args.named_child(0) {
                // Check if first argument is a string literal
                if first_arg.kind() == "string" {
                    let text = first_arg.utf8_text(content.as_bytes()).unwrap_or("");
                    return Some(
                        text.trim_matches(|c| c == '"' || c == '\'' || c == '`')
                            .to_string(),
                    );
                }
                // Check if it's a template literal
                if first_arg.kind() == "template_string" {
                    let text = first_arg.utf8_text(content.as_bytes()).unwrap_or("");
                    // Return the template string as-is (includes ${} expressions)
                    return Some(text.trim_matches('`').to_string());
                }
            }
        }
        None
    }
}

impl Parser for JavaScriptParser {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supported_extensions(&self) -> &[&str] {
        &["js", "jsx", "ts", "tsx", "mjs", "cjs"]
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
        discoveries.extend(self.detect_aws_sdk(&tree, content, path));
        discoveries.extend(self.detect_dynamodb_operations(&tree, content, path));
        discoveries.extend(self.detect_http_calls(&tree, content, path));

        Ok(discoveries)
    }
}

impl Default for JavaScriptParser {
    fn default() -> Self {
        Self::new().expect("Failed to create default JavaScriptParser")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_parser() -> JavaScriptParser {
        JavaScriptParser::new().expect("Failed to create parser")
    }

    #[test]
    fn test_detect_es6_imports() {
        let parser = create_parser();
        let content = r#"
import express from 'express';
import { DynamoDB } from '@aws-sdk/client-dynamodb';
import axios from 'axios';
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

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
            imports.iter().any(|i| i.module == "express"),
            "Should detect express import"
        );
        assert!(
            imports
                .iter()
                .any(|i| i.module == "@aws-sdk/client-dynamodb"),
            "Should detect AWS SDK import"
        );
        assert!(
            imports.iter().any(|i| i.module == "axios"),
            "Should detect axios import"
        );
    }

    #[test]
    fn test_detect_commonjs_requires() {
        let parser = create_parser();
        let content = r#"
const express = require('express');
const AWS = require('aws-sdk');
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(
            imports.iter().any(|i| i.module == "express"),
            "Should detect express require"
        );
        assert!(
            imports.iter().any(|i| i.module == "aws-sdk"),
            "Should detect aws-sdk require"
        );
    }

    #[test]
    fn test_detect_relative_imports() {
        let parser = create_parser();
        let content = r#"
import { helper } from './utils/helper';
import config from '../config';
import data from 'data-package';
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        let relative: Vec<_> = imports.iter().filter(|i| i.is_relative).collect();
        let non_relative: Vec<_> = imports.iter().filter(|i| !i.is_relative).collect();

        assert_eq!(relative.len(), 2, "Should have 2 relative imports");
        assert_eq!(non_relative.len(), 1, "Should have 1 non-relative import");
    }

    #[test]
    fn test_detect_aws_sdk_v3_dynamodb() {
        let parser = create_parser();
        // AWS SDK v3 imports - tests that imports are correctly detected
        // Note: Resource discoveries (DynamoDB, SQS) require actual method calls,
        // not just imports, to avoid creating "unknown" nodes that can't be deduplicated.
        let content = r#"
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { S3Client } from '@aws-sdk/client-s3';
import { SQSClient } from '@aws-sdk/client-sqs';
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        // AWS SDK imports should be detected
        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(
            imports.iter().any(|i| i.module == "@aws-sdk/client-dynamodb"),
            "Should detect DynamoDB client import"
        );
        assert!(
            imports.iter().any(|i| i.module == "@aws-sdk/client-s3"),
            "Should detect S3 client import"
        );
        assert!(
            imports.iter().any(|i| i.module == "@aws-sdk/client-sqs"),
            "Should detect SQS client import"
        );

        // S3 still creates a CloudResourceUsage from import (unlike DynamoDB/SQS which need method calls)
        let cloud_resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(c) => Some(c),
                _ => None,
            })
            .collect();

        assert!(
            cloud_resources.iter().any(|c| c.resource_type == "s3"),
            "Should detect S3 usage"
        );
    }

    #[test]
    fn test_detect_aws_sdk_v2() {
        let parser = create_parser();
        let content = r#"
const AWS = require('aws-sdk');
const dynamodb = new AWS.DynamoDB();
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        // Should detect the aws-sdk import
        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(
            imports.iter().any(|i| i.module == "aws-sdk"),
            "Should detect aws-sdk import"
        );
    }

    #[test]
    fn test_detect_dynamodb_operations() {
        let parser = create_parser();
        // Use DynamoDB-like variable name so detection recognizes this as DynamoDB
        let content = r#"
const result = await dynamoClient.get({ TableName: 'users', Key: { id } });
await dynamoClient.put({ TableName: 'users', Item: user });
const items = await dynamoClient.query({ TableName: 'orders' });
await dynamoClient.delete({ TableName: 'users', Key: { id } });
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

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
            "Should detect read operation"
        );
        assert!(
            db_ops
                .iter()
                .any(|d| d.operation == DatabaseOperation::Write),
            "Should detect write operation"
        );
    }

    #[test]
    fn test_detect_dynamodb_table_name() {
        let parser = create_parser();
        // Use DynamoDB-like variable name so detection recognizes this as DynamoDB
        let content = r#"
await docClient.get({ TableName: 'users-table', Key: { id: '123' } });
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        let db_ops: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(!db_ops.is_empty(), "Should detect database access");
        assert!(
            db_ops
                .iter()
                .any(|d| d.table_name.as_deref() == Some("users-table")),
            "Should extract table name"
        );
    }

    #[test]
    fn test_detect_fetch_calls() {
        let parser = create_parser();
        let content = r#"
const response = await fetch('https://api.example.com/users');
const data = await fetch('/api/data');
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(api_calls.len() >= 2, "Should detect both fetch calls");
        assert!(
            api_calls.iter().any(|a| a.detection_method == "fetch"),
            "Should identify fetch as detection method"
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.target.contains("api.example.com")),
            "Should extract URL"
        );
    }

    #[test]
    fn test_detect_axios_calls() {
        let parser = create_parser();
        let content = r#"
const users = await axios.get('https://api.example.com/users');
await axios.post('/api/users', { name: 'John' });
await axios.delete('/api/users/123');
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        let api_calls: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert!(api_calls.len() >= 3, "Should detect all axios calls");
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("GET".to_string())),
            "Should detect GET method"
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("POST".to_string())),
            "Should detect POST method"
        );
        assert!(
            api_calls
                .iter()
                .any(|a| a.method == Some("DELETE".to_string())),
            "Should detect DELETE method"
        );
    }

    #[test]
    fn test_package_json_parsing() {
        let dir = tempfile::tempdir().unwrap();

        let package_json = r#"
{
  "name": "user-service",
  "main": "dist/index.js",
  "dependencies": {
    "express": "^4.18.0",
    "@aws-sdk/client-dynamodb": "^3.0.0"
  }
}
"#;
        std::fs::write(dir.path().join("package.json"), package_json).unwrap();

        let parser = create_parser();
        let service = parser.parse_package_json(dir.path()).unwrap();

        assert_eq!(service.name, "user-service");
        assert_eq!(service.framework, Some("express".to_string()));
        assert_eq!(service.entry_point, "dist/index.js");
        assert_eq!(service.language, "javascript");
    }

    #[test]
    fn test_package_json_typescript_detection() {
        let dir = tempfile::tempdir().unwrap();

        let package_json = r#"
{
  "name": "ts-service",
  "main": "dist/index.js",
  "devDependencies": {
    "typescript": "^5.0.0"
  },
  "dependencies": {
    "fastify": "^4.0.0"
  }
}
"#;
        std::fs::write(dir.path().join("package.json"), package_json).unwrap();

        let parser = create_parser();
        let service = parser.parse_package_json(dir.path()).unwrap();

        assert_eq!(service.name, "ts-service");
        assert_eq!(service.framework, Some("fastify".to_string()));
        assert_eq!(service.language, "typescript");
    }

    #[test]
    fn test_package_json_missing() {
        let dir = tempfile::tempdir().unwrap();

        let parser = create_parser();
        let service = parser.parse_package_json(dir.path());

        assert!(
            service.is_none(),
            "Should return None for missing package.json"
        );
    }

    #[test]
    fn test_empty_file() {
        let parser = create_parser();
        let discoveries = parser.parse_file(Path::new("test.js"), "").unwrap();

        assert!(
            discoveries.is_empty(),
            "Empty file should produce no discoveries"
        );
    }

    #[test]
    fn test_file_with_comments_only() {
        let parser = create_parser();
        let content = r#"
// This is a comment
/* Multi-line
   comment */
"#;
        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        assert!(
            discoveries.is_empty(),
            "File with only comments should produce no discoveries"
        );
    }

    #[test]
    fn test_mixed_imports_and_code() {
        let parser = create_parser();
        let content = r#"
import express from 'express';
import { DynamoDB } from '@aws-sdk/client-dynamodb';

const app = express();
const client = new DynamoDB({});

app.get('/users/:id', async (req, res) => {
    const result = await client.get({ TableName: 'users', Key: { id: req.params.id } });
    res.json(result.Item);
});

const data = await fetch('https://api.example.com/data');
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        // Should have imports
        let imports: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::Import(_)))
            .collect();
        assert!(imports.len() >= 2, "Should have at least 2 imports");

        // Should have database access
        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::DatabaseAccess(_)))
            .collect();
        assert!(!db_accesses.is_empty(), "Should have database access");

        // Should have API call
        let api_calls: Vec<_> = discoveries
            .iter()
            .filter(|d| matches!(d, Discovery::ApiCall(_)))
            .collect();
        assert!(!api_calls.is_empty(), "Should have API call");
    }

    #[test]
    fn test_nestjs_framework_detection() {
        let dir = tempfile::tempdir().unwrap();

        let package_json = r#"
{
  "name": "nest-service",
  "dependencies": {
    "@nestjs/core": "^10.0.0",
    "express": "^4.18.0"
  }
}
"#;
        std::fs::write(dir.path().join("package.json"), package_json).unwrap();

        let parser = create_parser();
        let service = parser.parse_package_json(dir.path()).unwrap();

        // NestJS should take precedence over express
        assert_eq!(service.framework, Some("nestjs".to_string()));
    }

    #[test]
    fn test_nextjs_framework_detection() {
        let dir = tempfile::tempdir().unwrap();

        let package_json = r#"
{
  "name": "next-app",
  "dependencies": {
    "next": "^14.0.0",
    "react": "^18.0.0"
  }
}
"#;
        std::fs::write(dir.path().join("package.json"), package_json).unwrap();

        let parser = create_parser();
        let service = parser.parse_package_json(dir.path()).unwrap();

        assert_eq!(service.framework, Some("next.js".to_string()));
    }

    #[test]
    fn test_supported_extensions() {
        let parser = create_parser();
        let extensions = parser.supported_extensions();

        assert!(extensions.contains(&"js"));
        assert!(extensions.contains(&"jsx"));
        assert!(extensions.contains(&"ts"));
        assert!(extensions.contains(&"tsx"));
        assert!(extensions.contains(&"mjs"));
        assert!(extensions.contains(&"cjs"));
    }

    #[test]
    fn test_sns_detection() {
        let parser = create_parser();
        // SNS import detection - actual publish operations would be needed
        // to create QueueOperation discoveries with topic names.
        let content = r#"
import { SNSClient } from '@aws-sdk/client-sns';
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        // Should detect the SNS client import
        let imports: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Import(i) => Some(i),
                _ => None,
            })
            .collect();

        assert!(
            imports.iter().any(|i| i.module == "@aws-sdk/client-sns"),
            "Should detect SNS client import"
        );
    }

    #[test]
    fn test_lambda_detection() {
        let parser = create_parser();
        let content = r#"
import { LambdaClient } from '@aws-sdk/client-lambda';
"#;

        let discoveries = parser.parse_file(Path::new("test.js"), content).unwrap();

        let cloud_resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(c) => Some(c),
                _ => None,
            })
            .collect();

        assert!(
            cloud_resources.iter().any(|c| c.resource_type == "lambda"),
            "Should detect Lambda usage"
        );
    }
}
