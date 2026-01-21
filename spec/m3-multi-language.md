# Milestone 3: Multi-Language Parser Specification

> **Spec Version**: 1.0
> **Status**: Draft
> **Implements**: IMPLEMENTATION_PLAN.md § Milestone 3
> **Depends On**: [M2 Survey Core](./m2-survey-core.md)

---

## 1. Overview

### 1.1 Purpose

Extend the survey engine with Python and Terraform parsers, and implement automatic language detection. This milestone completes the V1 language support matrix, enabling Forge to survey typical enterprise polyglot environments.

### 1.2 Language Support Matrix (V1)

| Language | Parser | File Extensions | Detection Signals |
|----------|--------|-----------------|-------------------|
| JavaScript | tree-sitter-javascript | `.js`, `.jsx`, `.mjs`, `.cjs` | `package.json` |
| TypeScript | tree-sitter-typescript | `.ts`, `.tsx` | `package.json` with TypeScript deps |
| Python | tree-sitter-python | `.py` | `requirements.txt`, `pyproject.toml`, `setup.py` |
| Terraform | hcl2 crate | `.tf`, `.tfvars` | `*.tf` files in directory |

### 1.3 Success Criteria

1. Python parser detects boto3 DynamoDB, S3, SQS, SNS, and Lambda operations
2. Terraform parser extracts `aws_dynamodb_table`, `aws_sqs_queue`, and other resource definitions
3. Languages are auto-detected - no manual configuration required
4. `languages.exclude` config works to skip specific languages
5. Adding a new parser requires only implementing the `Parser` trait
6. Mixed-language repos produce unified graphs

### 1.4 Non-Goals

- Go, Java, or Rust parsers (future milestone)
- Runtime language detection (compile-time/static only)
- Support for non-AWS cloud providers (Azure, GCP)

---

## 2. Python Parser

### 2.1 Detection Targets

| Pattern | Discovery Type | Example |
|---------|---------------|---------|
| `import boto3` | Cloud SDK | AWS SDK usage |
| `boto3.client('dynamodb')` | Database | DynamoDB access |
| `boto3.resource('s3')` | CloudResource | S3 bucket usage |
| `boto3.client('sqs')` | Queue | SQS queue access |
| `boto3.client('sns')` | Queue | SNS topic usage |
| `boto3.client('lambda')` | Service | Lambda invocation |
| `requests.get/post/...` | ApiCall | HTTP client usage |
| `httpx.get/post/...` | ApiCall | HTTP client usage |
| `import service_name` | Import | Internal service dependency |

### 2.2 Implementation

```rust
// forge-survey/src/parser/python.rs

use super::traits::*;
use std::path::Path;
use tree_sitter::{Parser as TSParser, Query, QueryCursor, Node};

/// Parser for Python files
pub struct PythonParser {
    parser: TSParser,
    import_query: Query,
    boto3_client_query: Query,
    boto3_resource_query: Query,
    http_client_query: Query,
    dynamodb_method_query: Query,
}

impl PythonParser {
    pub fn new() -> Result<Self, ParserError> {
        let mut parser = TSParser::new();
        let language = tree_sitter_python::language();
        parser.set_language(&language)
            .map_err(|e| ParserError::TreeSitterError(e.to_string()))?;

        // Import statements: import X, from X import Y
        let import_query = Query::new(
            &language,
            r#"
            (import_statement
              name: (dotted_name) @module)

            (import_from_statement
              module_name: (dotted_name) @module)
            "#,
        ).map_err(|e| ParserError::TreeSitterError(e.to_string()))?;

        // boto3.client('service') pattern
        let boto3_client_query = Query::new(
            &language,
            r#"
            (call
              function: (attribute
                object: (identifier) @obj
                attribute: (identifier) @method)
              arguments: (argument_list
                (string) @service)
              (#eq? @obj "boto3")
              (#eq? @method "client"))
            "#,
        ).map_err(|e| ParserError::TreeSitterError(e.to_string()))?;

        // boto3.resource('service') pattern
        let boto3_resource_query = Query::new(
            &language,
            r#"
            (call
              function: (attribute
                object: (identifier) @obj
                attribute: (identifier) @method)
              arguments: (argument_list
                (string) @service)
              (#eq? @obj "boto3")
              (#eq? @method "resource"))
            "#,
        ).map_err(|e| ParserError::TreeSitterError(e.to_string()))?;

        // requests.get/post/etc and httpx.get/post/etc
        let http_client_query = Query::new(
            &language,
            r#"
            (call
              function: (attribute
                object: (identifier) @client
                attribute: (identifier) @method)
              arguments: (argument_list
                (string)? @url)
              (#match? @client "(requests|httpx)")
              (#match? @method "(get|post|put|delete|patch|head|options)"))
            "#,
        ).map_err(|e| ParserError::TreeSitterError(e.to_string()))?;

        // DynamoDB Table methods
        let dynamodb_method_query = Query::new(
            &language,
            r#"
            (call
              function: (attribute
                object: (_) @table
                attribute: (identifier) @method)
              (#match? @method "(get_item|put_item|update_item|delete_item|query|scan|batch_get_item|batch_write_item)"))
            "#,
        ).map_err(|e| ParserError::TreeSitterError(e.to_string()))?;

        Ok(Self {
            parser,
            import_query,
            boto3_client_query,
            boto3_resource_query,
            http_client_query,
            dynamodb_method_query,
        })
    }

    fn detect_imports(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();
        let mut cursor = QueryCursor::new();

        for match_ in cursor.matches(&self.import_query, tree.root_node(), content.as_bytes()) {
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

    fn detect_boto3_clients(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();
        let mut cursor = QueryCursor::new();

        // Detect boto3.client() calls
        for match_ in cursor.matches(&self.boto3_client_query, tree.root_node(), content.as_bytes()) {
            if let Some(service_capture) = match_.captures.iter().find(|c| c.index == 2) {
                let node = service_capture.node;
                let service_str = &content[node.byte_range()];
                let service = service_str.trim_matches(|c| c == '"' || c == '\'');

                self.add_service_discovery(&mut discoveries, service, node, path, content);
            }
        }

        // Detect boto3.resource() calls
        for match_ in cursor.matches(&self.boto3_resource_query, tree.root_node(), content.as_bytes()) {
            if let Some(service_capture) = match_.captures.iter().find(|c| c.index == 2) {
                let node = service_capture.node;
                let service_str = &content[node.byte_range()];
                let service = service_str.trim_matches(|c| c == '"' || c == '\'');

                self.add_service_discovery(&mut discoveries, service, node, path, content);
            }
        }

        discoveries
    }

    fn add_service_discovery(
        &self,
        discoveries: &mut Vec<Discovery>,
        service: &str,
        node: Node,
        path: &Path,
        _content: &str,
    ) {
        let line = node.start_position().row as u32 + 1;
        let source_file = path.to_string_lossy().to_string();

        match service {
            "dynamodb" => {
                discoveries.push(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
                    db_type: "dynamodb".to_string(),
                    table_name: None, // Will be extracted from method calls
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
                    operation: QueueOperation::Unknown,
                    source_file,
                    source_line: line,
                }));
            }
            "sns" => {
                discoveries.push(Discovery::QueueOperation(QueueOperationDiscovery {
                    queue_type: "sns".to_string(),
                    queue_name: None,
                    operation: QueueOperation::Publish, // SNS is typically publish-only from code
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
                    operation: QueueOperation::Unknown,
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

    fn detect_http_clients(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();
        let mut cursor = QueryCursor::new();

        for match_ in cursor.matches(&self.http_client_query, tree.root_node(), content.as_bytes()) {
            let client = match_.captures.iter()
                .find(|c| c.index == 0)
                .map(|c| &content[c.node.byte_range()])
                .unwrap_or("unknown");

            let method = match_.captures.iter()
                .find(|c| c.index == 1)
                .map(|c| &content[c.node.byte_range()])
                .unwrap_or("unknown");

            let url = match_.captures.iter()
                .find(|c| c.index == 2)
                .map(|c| {
                    let text = &content[c.node.byte_range()];
                    text.trim_matches(|c| c == '"' || c == '\'').to_string()
                });

            let node = match_.captures[0].node;

            discoveries.push(Discovery::ApiCall(ApiCallDiscovery {
                target: url.unwrap_or_else(|| "unknown".to_string()),
                method: Some(method.to_uppercase()),
                detection_method: client.to_string(),
                source_file: path.to_string_lossy().to_string(),
                source_line: node.start_position().row as u32 + 1,
            }));
        }

        discoveries
    }

    fn detect_dynamodb_methods(&self, tree: &tree_sitter::Tree, content: &str, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();
        let mut cursor = QueryCursor::new();

        for match_ in cursor.matches(&self.dynamodb_method_query, tree.root_node(), content.as_bytes()) {
            if let Some(method_capture) = match_.captures.iter().find(|c| c.index == 1) {
                let method = &content[method_capture.node.byte_range()];
                let node = method_capture.node;

                let operation = match method {
                    "get_item" | "query" | "scan" | "batch_get_item" => DatabaseOperation::Read,
                    "put_item" | "delete_item" | "batch_write_item" => DatabaseOperation::Write,
                    "update_item" => DatabaseOperation::ReadWrite,
                    _ => DatabaseOperation::Unknown,
                };

                // Try to extract table name from the call
                let table_name = self.extract_table_name_from_call(node.parent(), content);

                discoveries.push(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
                    db_type: "dynamodb".to_string(),
                    table_name,
                    operation,
                    detection_method: format!("boto3.{}", method),
                    source_file: path.to_string_lossy().to_string(),
                    source_line: node.start_position().row as u32 + 1,
                }));
            }
        }

        discoveries
    }

    fn extract_table_name_from_call(&self, call_node: Option<Node>, content: &str) -> Option<String> {
        // Look for TableName='xxx' or TableName="xxx" in the call arguments
        let call = call_node?;
        let args = call.child_by_field_name("arguments")?;

        for i in 0..args.child_count() {
            if let Some(arg) = args.child(i) {
                let arg_text = &content[arg.byte_range()];
                if arg_text.contains("TableName") {
                    // Extract the value after TableName=
                    if let Some(start) = arg_text.find('=') {
                        let value_part = &arg_text[start + 1..];
                        let value = value_part.trim()
                            .trim_matches(|c| c == '"' || c == '\'' || c == ',' || c == ')');
                        if !value.is_empty() && !value.contains('{') {
                            return Some(value.to_string());
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
            if let Some(service) = self.parse_pyproject_toml(&pyproject_path) {
                return Some(service);
            }
        }

        // Try setup.py
        let setup_path = repo_path.join("setup.py");
        if setup_path.exists() {
            if let Some(service) = self.parse_setup_py(&setup_path) {
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

    fn parse_pyproject_toml(&self, path: &Path) -> Option<ServiceDiscovery> {
        let content = std::fs::read_to_string(path).ok()?;

        // Simple TOML parsing for name field
        let name = content.lines()
            .find(|line| line.starts_with("name"))
            .and_then(|line| {
                let parts: Vec<&str> = line.split('=').collect();
                parts.get(1).map(|v| v.trim().trim_matches('"').to_string())
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
            entry_point: self.find_entry_point(path.parent()?).unwrap_or_else(|| "main.py".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    fn parse_setup_py(&self, path: &Path) -> Option<ServiceDiscovery> {
        let content = std::fs::read_to_string(path).ok()?;

        // Look for name= in setup()
        let name = content.lines()
            .find(|line| line.contains("name=") || line.contains("name ="))
            .and_then(|line| {
                let start = line.find('=')? + 1;
                let value = line[start..].trim()
                    .trim_matches(|c| c == '"' || c == '\'' || c == ',');
                Some(value.to_string())
            })?;

        Some(ServiceDiscovery {
            name,
            language: "python".to_string(),
            framework: None,
            entry_point: self.find_entry_point(path.parent()?).unwrap_or_else(|| "main.py".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        })
    }

    fn detect_framework_from_requirements(&self, path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;

        if content.lines().any(|l| l.starts_with("fastapi") || l.starts_with("FastAPI")) {
            Some("fastapi".to_string())
        } else if content.lines().any(|l| l.starts_with("flask") || l.starts_with("Flask")) {
            Some("flask".to_string())
        } else if content.lines().any(|l| l.starts_with("django") || l.starts_with("Django")) {
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
        let tree = self.parser.parse(content, None)
            .ok_or_else(|| ParserError::ParseFailed {
                path: path.to_string_lossy().to_string()
            })?;

        let mut discoveries = Vec::new();

        discoveries.extend(self.detect_imports(&tree, content, path));
        discoveries.extend(self.detect_boto3_clients(&tree, content, path));
        discoveries.extend(self.detect_http_clients(&tree, content, path));
        discoveries.extend(self.detect_dynamodb_methods(&tree, content, path));

        Ok(discoveries)
    }
}
```

---

## 3. Terraform Parser

### 3.1 Detection Targets

| Resource Type | Node Type | Key Attributes |
|--------------|-----------|----------------|
| `aws_dynamodb_table` | Database | name, hash_key, range_key, arn |
| `aws_sqs_queue` | Queue | name, arn, fifo |
| `aws_sns_topic` | Queue | name, arn |
| `aws_s3_bucket` | CloudResource | bucket, arn |
| `aws_lambda_function` | Service | function_name, handler, runtime |
| `aws_iam_role` | (metadata) | Permissions for cross-service access |
| `aws_iam_policy` | (metadata) | Resource access patterns |

### 3.2 Implementation

```rust
// forge-survey/src/parser/terraform.rs

use super::traits::*;
use std::path::Path;
use std::collections::HashMap;

/// Parser for Terraform HCL files
pub struct TerraformParser {
    // We use the hcl2 crate for parsing instead of tree-sitter
}

impl TerraformParser {
    pub fn new() -> Result<Self, ParserError> {
        Ok(Self {})
    }

    fn parse_hcl(&self, content: &str) -> Result<hcl::Body, ParserError> {
        hcl::from_str(content)
            .map_err(|e| ParserError::TreeSitterError(format!("HCL parse error: {}", e)))
    }

    fn extract_resources(&self, body: &hcl::Body, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        for block in &body.blocks {
            if block.identifier.as_str() == "resource" {
                if let Some(discovery) = self.process_resource_block(block, path) {
                    discoveries.push(discovery);
                }
            }
        }

        discoveries
    }

    fn process_resource_block(&self, block: &hcl::Block, path: &Path) -> Option<Discovery> {
        let labels: Vec<&str> = block.labels.iter()
            .map(|l| l.as_str())
            .collect();

        if labels.len() < 2 {
            return None;
        }

        let resource_type = labels[0];
        let resource_name = labels[1];

        match resource_type {
            "aws_dynamodb_table" => self.parse_dynamodb_table(block, resource_name, path),
            "aws_sqs_queue" => self.parse_sqs_queue(block, resource_name, path),
            "aws_sns_topic" => self.parse_sns_topic(block, resource_name, path),
            "aws_s3_bucket" => self.parse_s3_bucket(block, resource_name, path),
            "aws_lambda_function" => self.parse_lambda_function(block, resource_name, path),
            _ => None,
        }
    }

    fn parse_dynamodb_table(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let attrs = self.extract_attributes(&block.body);

        let name = attrs.get("name")
            .or_else(|| Some(&tf_name.to_string()))
            .cloned()?;

        let hash_key = attrs.get("hash_key").cloned();
        let range_key = attrs.get("range_key").cloned();

        Some(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some(name),
            operation: DatabaseOperation::Unknown, // Terraform defines the table, not operations
            detection_method: "terraform".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1, // HCL parser doesn't give line numbers easily
        }))
    }

    fn parse_sqs_queue(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let attrs = self.extract_attributes(&block.body);

        let name = attrs.get("name")
            .or_else(|| Some(&tf_name.to_string()))
            .cloned()?;

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sqs".to_string(),
            queue_name: Some(name),
            operation: QueueOperation::Unknown, // Terraform defines the queue, not operations
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_sns_topic(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let attrs = self.extract_attributes(&block.body);

        let name = attrs.get("name")
            .or_else(|| Some(&tf_name.to_string()))
            .cloned()?;

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sns".to_string(),
            queue_name: Some(name),
            operation: QueueOperation::Unknown,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_s3_bucket(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let attrs = self.extract_attributes(&block.body);

        let name = attrs.get("bucket")
            .or_else(|| Some(&tf_name.to_string()))
            .cloned()?;

        Some(Discovery::CloudResourceUsage(CloudResourceDiscovery {
            resource_type: "s3".to_string(),
            resource_name: Some(name),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_lambda_function(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let attrs = self.extract_attributes(&block.body);

        let name = attrs.get("function_name")
            .or_else(|| Some(&tf_name.to_string()))
            .cloned()?;

        let runtime = attrs.get("runtime").cloned();
        let handler = attrs.get("handler").cloned();

        // Lambda functions are services
        Some(Discovery::Service(ServiceDiscovery {
            name,
            language: runtime.map(|r| {
                if r.starts_with("python") { "python".to_string() }
                else if r.starts_with("nodejs") { "javascript".to_string() }
                else if r.starts_with("go") { "go".to_string() }
                else if r.starts_with("java") { "java".to_string() }
                else { r }
            }).unwrap_or_else(|| "unknown".to_string()),
            framework: Some("aws-lambda".to_string()),
            entry_point: handler.unwrap_or_else(|| "index.handler".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn extract_attributes(&self, body: &hcl::Body) -> HashMap<String, String> {
        let mut attrs = HashMap::new();

        for attr in &body.attributes {
            if let Some(value) = self.expr_to_string(&attr.expr) {
                attrs.insert(attr.key.to_string(), value);
            }
        }

        attrs
    }

    fn expr_to_string(&self, expr: &hcl::Expression) -> Option<String> {
        match expr {
            hcl::Expression::String(s) => Some(s.clone()),
            hcl::Expression::Number(n) => Some(n.to_string()),
            hcl::Expression::Bool(b) => Some(b.to_string()),
            hcl::Expression::Variable(v) => Some(format!("${{{}}}", v)),
            _ => None,
        }
    }

    /// Extract IAM policy statements to understand service permissions
    pub fn extract_iam_permissions(&self, body: &hcl::Body) -> Vec<IamPermission> {
        let mut permissions = Vec::new();

        for block in &body.blocks {
            if block.identifier.as_str() == "resource" {
                let labels: Vec<&str> = block.labels.iter()
                    .map(|l| l.as_str())
                    .collect();

                if labels.first() == Some(&"aws_iam_role_policy")
                    || labels.first() == Some(&"aws_iam_policy")
                {
                    permissions.extend(self.parse_iam_policy(&block.body));
                }
            }
        }

        permissions
    }

    fn parse_iam_policy(&self, body: &hcl::Body) -> Vec<IamPermission> {
        // This would parse the policy JSON to extract resource access patterns
        // Simplified implementation - real implementation would parse the policy document
        vec![]
    }
}

/// Represents an IAM permission extracted from Terraform
#[derive(Debug, Clone)]
pub struct IamPermission {
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub effect: String,
}

impl Parser for TerraformParser {
    fn supported_extensions(&self) -> &[&str] {
        &["tf"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        let body = self.parse_hcl(content)?;
        Ok(self.extract_resources(&body, path))
    }

    fn parse_repo(&self, repo_path: &Path) -> Result<Vec<Discovery>, ParserError> {
        let mut all_discoveries = Vec::new();

        // Find all .tf files
        for entry in walkdir::WalkDir::new(repo_path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_str().unwrap_or("");
                !matches!(name, ".git" | ".terraform" | "node_modules")
            })
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            if path.extension().map(|e| e == "tf").unwrap_or(false) {
                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                match self.parse_file(path, &content) {
                    Ok(discoveries) => all_discoveries.extend(discoveries),
                    Err(e) => {
                        tracing::warn!("Failed to parse Terraform file {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(all_discoveries)
    }
}
```

---

## 4. Language Auto-Detection

### 4.1 Detection Strategy

```rust
// forge-survey/src/detection.rs

use std::path::Path;
use std::collections::HashSet;

/// Detected languages in a repository
#[derive(Debug, Clone, Default)]
pub struct DetectedLanguages {
    pub languages: HashSet<String>,
    pub signals: Vec<LanguageSignal>,
}

/// Evidence for language detection
#[derive(Debug, Clone)]
pub struct LanguageSignal {
    pub language: String,
    pub signal_type: SignalType,
    pub file_path: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub enum SignalType {
    /// Config file indicates language (package.json, requirements.txt)
    ConfigFile,
    /// File extension indicates language
    FileExtension,
    /// Directory structure indicates language
    DirectoryStructure,
}

/// Detect languages used in a repository
pub fn detect_languages(repo_path: &Path) -> DetectedLanguages {
    let mut result = DetectedLanguages::default();

    // Check for config files (highest confidence)
    check_config_files(repo_path, &mut result);

    // Scan for file extensions (medium confidence)
    scan_file_extensions(repo_path, &mut result);

    result
}

fn check_config_files(repo_path: &Path, result: &mut DetectedLanguages) {
    // JavaScript/TypeScript indicators
    let package_json = repo_path.join("package.json");
    if package_json.exists() {
        result.languages.insert("javascript".to_string());
        result.signals.push(LanguageSignal {
            language: "javascript".to_string(),
            signal_type: SignalType::ConfigFile,
            file_path: package_json.to_string_lossy().to_string(),
            confidence: 0.95,
        });

        // Check for TypeScript in package.json
        if let Ok(content) = std::fs::read_to_string(&package_json) {
            if content.contains("typescript") || content.contains("\"ts-") {
                result.languages.insert("typescript".to_string());
                result.signals.push(LanguageSignal {
                    language: "typescript".to_string(),
                    signal_type: SignalType::ConfigFile,
                    file_path: package_json.to_string_lossy().to_string(),
                    confidence: 0.9,
                });
            }
        }
    }

    // Python indicators
    let python_configs = [
        "requirements.txt",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "Pipfile",
    ];

    for config in &python_configs {
        let path = repo_path.join(config);
        if path.exists() {
            result.languages.insert("python".to_string());
            result.signals.push(LanguageSignal {
                language: "python".to_string(),
                signal_type: SignalType::ConfigFile,
                file_path: path.to_string_lossy().to_string(),
                confidence: 0.95,
            });
            break;
        }
    }

    // Terraform indicators
    let has_tf_files = walkdir::WalkDir::new(repo_path)
        .max_depth(2) // Don't scan too deep
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path().extension()
                .map(|ext| ext == "tf")
                .unwrap_or(false)
        });

    if has_tf_files {
        result.languages.insert("terraform".to_string());
        result.signals.push(LanguageSignal {
            language: "terraform".to_string(),
            signal_type: SignalType::FileExtension,
            file_path: repo_path.to_string_lossy().to_string(),
            confidence: 0.9,
        });
    }
}

fn scan_file_extensions(repo_path: &Path, result: &mut DetectedLanguages) {
    // Quick scan of top-level and src/ directories
    let scan_dirs = [
        repo_path.to_path_buf(),
        repo_path.join("src"),
        repo_path.join("lib"),
        repo_path.join("app"),
    ];

    let mut js_count = 0;
    let mut ts_count = 0;
    let mut py_count = 0;

    for dir in &scan_dirs {
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .max_depth(3)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_str().unwrap_or("");
                !matches!(name, "node_modules" | ".git" | "__pycache__" | "venv" | ".venv")
            })
            .filter_map(|e| e.ok())
        {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                match ext {
                    "js" | "jsx" | "mjs" | "cjs" => js_count += 1,
                    "ts" | "tsx" => ts_count += 1,
                    "py" => py_count += 1,
                    _ => {}
                }
            }
        }
    }

    // Add languages if we found significant code
    if js_count >= 3 && !result.languages.contains("javascript") {
        result.languages.insert("javascript".to_string());
        result.signals.push(LanguageSignal {
            language: "javascript".to_string(),
            signal_type: SignalType::FileExtension,
            file_path: repo_path.to_string_lossy().to_string(),
            confidence: 0.7,
        });
    }

    if ts_count >= 3 && !result.languages.contains("typescript") {
        result.languages.insert("typescript".to_string());
        result.signals.push(LanguageSignal {
            language: "typescript".to_string(),
            signal_type: SignalType::FileExtension,
            file_path: repo_path.to_string_lossy().to_string(),
            confidence: 0.7,
        });
    }

    if py_count >= 3 && !result.languages.contains("python") {
        result.languages.insert("python".to_string());
        result.signals.push(LanguageSignal {
            language: "python".to_string(),
            signal_type: SignalType::FileExtension,
            file_path: repo_path.to_string_lossy().to_string(),
            confidence: 0.7,
        });
    }
}
```

### 4.2 Parser Registry

```rust
// forge-survey/src/parser/mod.rs

pub mod traits;
pub mod javascript;
pub mod python;
pub mod terraform;

use traits::{Parser, ParserError};
use std::sync::Arc;
use std::collections::HashMap;

/// Registry of available parsers
pub struct ParserRegistry {
    parsers: HashMap<String, Arc<dyn Parser>>,
}

impl ParserRegistry {
    /// Create a new registry with all built-in parsers
    pub fn new() -> Result<Self, ParserError> {
        let mut parsers: HashMap<String, Arc<dyn Parser>> = HashMap::new();

        // JavaScript/TypeScript parser
        let js_parser = Arc::new(javascript::JavaScriptParser::new()?);
        parsers.insert("javascript".to_string(), js_parser.clone());
        parsers.insert("typescript".to_string(), js_parser);

        // Python parser
        parsers.insert("python".to_string(), Arc::new(python::PythonParser::new()?));

        // Terraform parser
        parsers.insert("terraform".to_string(), Arc::new(terraform::TerraformParser::new()?));

        Ok(Self { parsers })
    }

    /// Get a parser by language name
    pub fn get(&self, language: &str) -> Option<Arc<dyn Parser>> {
        self.parsers.get(language).cloned()
    }

    /// Get parsers for detected languages, excluding specified ones
    pub fn get_for_languages(
        &self,
        languages: &[String],
        exclude: &[String],
    ) -> Vec<(String, Arc<dyn Parser>)> {
        languages
            .iter()
            .filter(|lang| !exclude.contains(lang))
            .filter_map(|lang| {
                self.get(lang).map(|p| (lang.clone(), p))
            })
            .collect()
    }

    /// List all available languages
    pub fn available_languages(&self) -> Vec<&str> {
        self.parsers.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new().expect("Failed to create parser registry")
    }
}
```

---

## 5. Updated Survey Pipeline

```rust
// forge-survey/src/lib.rs (updated for M3)

use crate::detection::detect_languages;
use crate::parser::{ParserRegistry, Discovery};
use forge_graph::ForgeGraph;

/// Survey a repository with automatic language detection
pub fn survey_repo(
    repo_path: &Path,
    builder: &mut GraphBuilder,
    registry: &ParserRegistry,
    exclude_languages: &[String],
) -> Result<(), SurveyError> {
    // Step 1: Detect languages in the repository
    let detected = detect_languages(repo_path);

    tracing::info!(
        "Detected languages in {}: {:?}",
        repo_path.display(),
        detected.languages
    );

    // Step 2: Get applicable parsers
    let parsers = registry.get_for_languages(
        &detected.languages.iter().cloned().collect::<Vec<_>>(),
        exclude_languages,
    );

    if parsers.is_empty() {
        tracing::warn!(
            "No parsers available for {} (detected: {:?}, excluded: {:?})",
            repo_path.display(),
            detected.languages,
            exclude_languages
        );
        return Ok(());
    }

    // Step 3: Detect service identity
    let service_id = detect_service(repo_path, &parsers, builder)?;

    // Step 4: Run all applicable parsers
    let mut all_discoveries = Vec::new();

    for (lang, parser) in &parsers {
        tracing::debug!("Running {} parser on {}", lang, repo_path.display());

        match parser.parse_repo(repo_path) {
            Ok(discoveries) => {
                tracing::debug!("Found {} discoveries from {} parser", discoveries.len(), lang);
                all_discoveries.extend(discoveries);
            }
            Err(e) => {
                tracing::warn!("Parser {} failed for {}: {}", lang, repo_path.display(), e);
                // Continue with other parsers
            }
        }
    }

    // Step 5: Process discoveries into graph
    builder.process_discoveries(all_discoveries, &service_id);

    Ok(())
}

fn detect_service(
    repo_path: &Path,
    parsers: &[(String, Arc<dyn Parser>)],
    builder: &mut GraphBuilder,
) -> Result<NodeId, SurveyError> {
    // Try JavaScript parser for package.json
    if let Some((_, parser)) = parsers.iter().find(|(l, _)| l == "javascript") {
        if let Some(js_parser) = parser.as_any().downcast_ref::<javascript::JavaScriptParser>() {
            if let Some(service) = js_parser.parse_package_json(repo_path) {
                return Ok(builder.add_service(service));
            }
        }
    }

    // Try Python parser for pyproject.toml/requirements.txt
    if let Some((_, parser)) = parsers.iter().find(|(l, _)| l == "python") {
        if let Some(py_parser) = parser.as_any().downcast_ref::<python::PythonParser>() {
            if let Some(service) = py_parser.parse_project_config(repo_path) {
                return Ok(builder.add_service(service));
            }
        }
    }

    // Fall back to directory name
    let name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let primary_lang = parsers
        .first()
        .map(|(l, _)| l.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let service = ServiceDiscovery {
        name: name.clone(),
        language: primary_lang,
        framework: None,
        entry_point: "unknown".to_string(),
        source_file: repo_path.to_string_lossy().to_string(),
        source_line: 1,
    };

    Ok(builder.add_service(service))
}
```

---

## 6. Test Specifications

### 6.1 Python Parser Tests

```rust
#[cfg(test)]
mod python_parser_tests {
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

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let db_accesses: Vec<_> = discoveries.iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert!(db_accesses.iter().any(|d| d.db_type == "dynamodb"));
    }

    #[test]
    fn test_detect_boto3_s3() {
        let parser = PythonParser::new().unwrap();
        let content = r#"
import boto3

s3 = boto3.client('s3')
s3.upload_file('local.txt', 'my-bucket', 'remote.txt')
"#;

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let resources: Vec<_> = discoveries.iter()
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

        let discoveries = parser.parse_file(Path::new("test.py"), content).unwrap();

        let api_calls: Vec<_> = discoveries.iter()
            .filter_map(|d| match d {
                Discovery::ApiCall(a) => Some(a),
                _ => None,
            })
            .collect();

        assert_eq!(api_calls.len(), 2);
        assert!(api_calls.iter().any(|a| a.method == Some("GET".to_string())));
        assert!(api_calls.iter().any(|a| a.method == Some("POST".to_string())));
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
        ).unwrap();

        let service = parser.parse_project_config(dir.path()).unwrap();
        assert_eq!(service.name, "my-service");
        assert_eq!(service.framework, Some("fastapi".to_string()));
    }
}
```

### 6.2 Terraform Parser Tests

```rust
#[cfg(test)]
mod terraform_parser_tests {
    use super::*;

    #[test]
    fn test_parse_dynamodb_table() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_dynamodb_table" "users" {
  name           = "users-table"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "id"

  attribute {
    name = "id"
    type = "S"
  }
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();

        let db_accesses: Vec<_> = discoveries.iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(db_accesses.len(), 1);
        assert_eq!(db_accesses[0].table_name, Some("users-table".to_string()));
        assert_eq!(db_accesses[0].db_type, "dynamodb");
    }

    #[test]
    fn test_parse_sqs_queue() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_sqs_queue" "orders" {
  name = "orders-queue"
  fifo_queue = false
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();

        let queues: Vec<_> = discoveries.iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert_eq!(queues.len(), 1);
        assert_eq!(queues[0].queue_name, Some("orders-queue".to_string()));
        assert_eq!(queues[0].queue_type, "sqs");
    }

    #[test]
    fn test_parse_lambda_function() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_lambda_function" "processor" {
  function_name = "order-processor"
  runtime       = "python3.9"
  handler       = "main.handler"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();

        let services: Vec<_> = discoveries.iter()
            .filter_map(|d| match d {
                Discovery::Service(s) => Some(s),
                _ => None,
            })
            .collect();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "order-processor");
        assert_eq!(services[0].language, "python");
        assert_eq!(services[0].framework, Some("aws-lambda".to_string()));
    }
}
```

### 6.3 Language Detection Tests

```rust
#[cfg(test)]
mod detection_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_javascript_from_package_json() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();

        let result = detect_languages(dir.path());
        assert!(result.languages.contains("javascript"));
    }

    #[test]
    fn test_detect_python_from_requirements() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "boto3==1.28.0").unwrap();

        let result = detect_languages(dir.path());
        assert!(result.languages.contains("python"));
    }

    #[test]
    fn test_detect_terraform_from_tf_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("main.tf"), "resource \"aws_s3_bucket\" \"test\" {}").unwrap();

        let result = detect_languages(dir.path());
        assert!(result.languages.contains("terraform"));
    }

    #[test]
    fn test_detect_multiple_languages() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        std::fs::write(dir.path().join("main.tf"), "").unwrap();

        let result = detect_languages(dir.path());

        assert!(result.languages.contains("javascript"));
        assert!(result.languages.contains("python"));
        assert!(result.languages.contains("terraform"));
    }
}
```

---

## 7. Implementation Checklist

| Task ID | Description | Files |
|---------|-------------|-------|
| M3-T1 | Implement Python parser | `forge-survey/src/parser/python.rs` |
| M3-T2 | Implement Terraform parser | `forge-survey/src/parser/terraform.rs` |
| M3-T3 | Implement parser registry | `forge-survey/src/parser/mod.rs` |
| M3-T4 | Implement language auto-detection | `forge-survey/src/detection.rs` |
| M3-T5 | Write Python parser tests | `forge-survey/src/parser/python.rs` |
| M3-T6 | Write Terraform parser tests | `forge-survey/src/parser/terraform.rs` |
| M3-T7 | Write integration tests | `forge-survey/tests/integration_multi.rs` |

---

## 8. Dependencies

```toml
# Additional dependencies for forge-survey/Cargo.toml
[dependencies]
tree-sitter-python = "0.20"
hcl-rs = "0.16"  # or hcl2 = "0.4"
```

---

## 9. Acceptance Criteria

- [ ] Python parser detects `import boto3` statements
- [ ] Python parser detects `boto3.client('dynamodb')` calls
- [ ] Python parser detects `boto3.client('s3')` calls
- [ ] Python parser detects `boto3.client('sqs')` and `boto3.client('sns')` calls
- [ ] Python parser detects `requests.get/post` calls
- [ ] Python parser detects `httpx.get/post` calls
- [ ] Python parser extracts service info from `pyproject.toml`
- [ ] Terraform parser extracts `aws_dynamodb_table` resources
- [ ] Terraform parser extracts `aws_sqs_queue` resources
- [ ] Terraform parser extracts `aws_sns_topic` resources
- [ ] Terraform parser extracts `aws_s3_bucket` resources
- [ ] Terraform parser extracts `aws_lambda_function` resources
- [ ] Languages are auto-detected from config files
- [ ] Languages are auto-detected from file extensions
- [ ] `languages.exclude` config prevents parsing
- [ ] Mixed-language repos produce unified graphs
- [ ] Parser failures don't crash the survey
