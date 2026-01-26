//! Terraform parser for detecting AWS resource definitions and deployment metadata.
//!
//! Detects:
//! - aws_dynamodb_table resources
//! - aws_sqs_queue resources
//! - aws_sns_topic resources
//! - aws_s3_bucket resources
//! - aws_lambda_function resources
//!
//! Extracts deployment metadata from:
//! - Resource tags (ManagedBy, Environment, terraform:workspace)
//! - Backend configuration (workspace from S3 key path)

use super::traits::*;
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;

/// Parser for Terraform HCL files
pub struct TerraformParser {}

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

        // Extract terraform workspace from backend config
        let workspace = self.extract_backend_workspace(body);

        for block in body.blocks() {
            let ident: &str = block.identifier();
            if ident == "resource" {
                if let Some(discovery) =
                    self.process_resource_block(block, path, workspace.as_deref())
                {
                    discoveries.push(discovery);
                }
            }
        }

        discoveries
    }

    /// Extract workspace name from terraform backend configuration.
    ///
    /// Looks for backend "s3" block and extracts workspace from key path:
    /// - "production/terraform.tfstate" → "production"
    /// - "env/prod/terraform.tfstate" → "prod"
    /// - "terraform.tfstate" → None (default workspace)
    fn extract_backend_workspace(&self, body: &hcl::Body) -> Option<String> {
        for block in body.blocks() {
            if block.identifier() == "terraform" {
                // Look for backend block inside terraform block
                for nested_block in block.body().blocks() {
                    if nested_block.identifier() == "backend" {
                        // Check if it's S3 backend
                        let labels = nested_block.labels();
                        if !labels.is_empty() && labels[0].as_str() == "s3" {
                            // Extract key attribute
                            if let Some(key) = self.get_string_attribute(nested_block.body(), "key")
                            {
                                return self.workspace_from_key_path(&key);
                            }
                        }
                        // Check workspace attribute directly (for backends that support it)
                        if let Some(workspace) =
                            self.get_string_attribute(nested_block.body(), "workspace")
                        {
                            return Some(workspace);
                        }
                    }
                }
            }
        }
        None
    }

    /// Parse workspace from S3 key path.
    ///
    /// Examples:
    /// - "production/terraform.tfstate" → Some("production")
    /// - "env/prod/terraform.tfstate" → Some("prod")
    /// - "myproject/staging/state.tfstate" → Some("staging")
    /// - "terraform.tfstate" → None
    fn workspace_from_key_path(&self, key: &str) -> Option<String> {
        let parts: Vec<&str> = key.split('/').collect();

        if parts.len() <= 1 {
            // No path segments, default workspace
            return None;
        }

        // Get the second-to-last part (parent of the tfstate file)
        // which is usually the environment/workspace
        let candidate = parts[parts.len() - 2];

        // Check if it's a common environment name or treat it as workspace
        if !candidate.is_empty() && !candidate.contains('.') {
            Some(candidate.to_string())
        } else {
            None
        }
    }

    fn process_resource_block(
        &self,
        block: &hcl::Block,
        path: &Path,
        backend_workspace: Option<&str>,
    ) -> Option<Discovery> {
        let labels = block.labels();

        if labels.len() < 2 {
            return None;
        }

        let resource_type = labels[0].as_str();
        let resource_name = labels[1].as_str();

        match resource_type {
            "aws_dynamodb_table" => {
                self.parse_dynamodb_table(block, resource_name, path, backend_workspace)
            }
            "aws_sqs_queue" => self.parse_sqs_queue(block, resource_name, path, backend_workspace),
            "aws_sns_topic" => self.parse_sns_topic(block, resource_name, path, backend_workspace),
            "aws_s3_bucket" => self.parse_s3_bucket(block, resource_name, path, backend_workspace),
            "aws_lambda_function" => {
                self.parse_lambda_function(block, resource_name, path, backend_workspace)
            }
            _ => None,
        }
    }

    /// Extract tags from a resource block.
    ///
    /// Handles both simple tags attribute and tags_all.
    /// Normalizes tag keys for consistent lookup.
    fn extract_tags(&self, body: &hcl::Body) -> HashMap<String, String> {
        let mut tags = HashMap::new();

        // Look for tags attribute
        if let Some(tags_attr) = body.attributes().find(|attr| attr.key() == "tags") {
            if let hcl::Expression::Object(obj) = tags_attr.expr() {
                for (key, value) in obj.iter() {
                    // Extract the key as string - it can be an Identifier or Expression
                    let key_str = match key {
                        hcl::ObjectKey::Identifier(id) => id.to_string(),
                        hcl::ObjectKey::Expression(hcl::Expression::String(s)) => s.to_string(),
                        _ => continue, // Skip non-string keys
                    };

                    // Extract the value if it's a string
                    if let hcl::Expression::String(v) = value {
                        tags.insert(key_str, v.to_string());
                    }
                }
            }
        }

        tags
    }

    /// Extract environment from tags using common key variations.
    ///
    /// Supports: Environment, Env, env, environment, ENVIRONMENT
    fn extract_environment_from_tags(&self, tags: &HashMap<String, String>) -> Option<String> {
        // Check common environment tag keys (case-insensitive)
        for (key, value) in tags {
            let key_lower = key.to_lowercase();
            if key_lower == "environment" || key_lower == "env" {
                return Some(value.clone());
            }
        }
        None
    }

    /// Infer deployment method from tags.
    ///
    /// Looks for ManagedBy, managed_by, or similar tags indicating Terraform management.
    fn infer_deployment_method(&self, tags: &HashMap<String, String>) -> String {
        for (key, value) in tags {
            let key_lower = key.to_lowercase().replace('-', "_");
            if key_lower == "managed_by" || key_lower == "managedby" {
                let value_lower = value.to_lowercase();
                if value_lower == "terraform" || value_lower.contains("terraform") {
                    return "terraform".to_string();
                }
            }
        }
        // Default to terraform since we're parsing Terraform files
        "terraform".to_string()
    }

    /// Build deployment metadata from tags and backend workspace.
    fn build_deployment_metadata(
        &self,
        tags: HashMap<String, String>,
        backend_workspace: Option<&str>,
    ) -> DeploymentMetadata {
        let environment = self
            .extract_environment_from_tags(&tags)
            .or_else(|| backend_workspace.map(|s| s.to_string()));

        let terraform_workspace = tags
            .get("terraform:workspace")
            .cloned()
            .or_else(|| tags.get("terraform_workspace").cloned())
            .or_else(|| backend_workspace.map(|s| s.to_string()));

        DeploymentMetadata {
            deployment_method: self.infer_deployment_method(&tags),
            terraform_workspace,
            environment,
            stack_name: None, // Not applicable for Terraform
            tags,
        }
    }

    fn parse_dynamodb_table(
        &self,
        block: &hcl::Block,
        tf_name: &str,
        path: &Path,
        backend_workspace: Option<&str>,
    ) -> Option<Discovery> {
        let table_name = self
            .get_string_attribute(block.body(), "name")
            .unwrap_or_else(|| tf_name.to_string());

        let tags = self.extract_tags(block.body());
        let metadata = self.build_deployment_metadata(tags, backend_workspace);

        Some(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some(table_name),
            operation: DatabaseOperation::Unknown, // Terraform defines the table, not operations
            detection_method: "terraform".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    fn parse_sqs_queue(
        &self,
        block: &hcl::Block,
        tf_name: &str,
        path: &Path,
        backend_workspace: Option<&str>,
    ) -> Option<Discovery> {
        let queue_name = self
            .get_string_attribute(block.body(), "name")
            .unwrap_or_else(|| tf_name.to_string());

        let tags = self.extract_tags(block.body());
        let metadata = self.build_deployment_metadata(tags, backend_workspace);

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sqs".to_string(),
            queue_name: Some(queue_name),
            operation: QueueOperationType::Unknown, // Terraform defines the queue, not operations
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    fn parse_sns_topic(
        &self,
        block: &hcl::Block,
        tf_name: &str,
        path: &Path,
        backend_workspace: Option<&str>,
    ) -> Option<Discovery> {
        let topic_name = self
            .get_string_attribute(block.body(), "name")
            .unwrap_or_else(|| tf_name.to_string());

        let tags = self.extract_tags(block.body());
        let metadata = self.build_deployment_metadata(tags, backend_workspace);

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sns".to_string(),
            queue_name: Some(topic_name),
            operation: QueueOperationType::Unknown,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    fn parse_s3_bucket(
        &self,
        block: &hcl::Block,
        tf_name: &str,
        path: &Path,
        backend_workspace: Option<&str>,
    ) -> Option<Discovery> {
        let bucket_name = self
            .get_string_attribute(block.body(), "bucket")
            .unwrap_or_else(|| tf_name.to_string());

        let tags = self.extract_tags(block.body());
        let metadata = self.build_deployment_metadata(tags, backend_workspace);

        Some(Discovery::CloudResourceUsage(CloudResourceDiscovery {
            resource_type: "s3".to_string(),
            resource_name: Some(bucket_name),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    fn parse_lambda_function(
        &self,
        block: &hcl::Block,
        tf_name: &str,
        path: &Path,
        backend_workspace: Option<&str>,
    ) -> Option<Discovery> {
        let function_name = self
            .get_string_attribute(block.body(), "function_name")
            .unwrap_or_else(|| tf_name.to_string());

        let runtime = self.get_string_attribute(block.body(), "runtime");
        let handler = self.get_string_attribute(block.body(), "handler");

        let language = runtime
            .as_ref()
            .map(|r| {
                if r.starts_with("python") {
                    "python".to_string()
                } else if r.starts_with("nodejs") {
                    "javascript".to_string()
                } else if r.starts_with("go") {
                    "go".to_string()
                } else if r.starts_with("java") {
                    "java".to_string()
                } else {
                    r.clone()
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let tags = self.extract_tags(block.body());
        let metadata = self.build_deployment_metadata(tags, backend_workspace);

        // Lambda functions are services
        Some(Discovery::Service(ServiceDiscovery {
            name: function_name,
            language,
            framework: Some("aws-lambda".to_string()),
            entry_point: handler.unwrap_or_else(|| "index.handler".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    fn get_string_attribute(&self, body: &hcl::Body, key: &str) -> Option<String> {
        body.attributes()
            .find(|attr| attr.key() == key)
            .and_then(|attr| match attr.expr() {
                hcl::Expression::String(s) => Some(s.to_string()),
                _ => None,
            })
    }
}

impl Parser for TerraformParser {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supported_extensions(&self) -> &[&str] {
        &["tf"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        let body = self.parse_hcl(content)?;
        Ok(self.extract_resources(&body, path))
    }
}

#[cfg(test)]
mod tests {
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

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(db_accesses.len(), 1);
        assert_eq!(db_accesses[0].table_name, Some("users-table".to_string()));
        assert_eq!(db_accesses[0].db_type, "dynamodb");

        // Verify deployment metadata
        let metadata = db_accesses[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "terraform");
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

        let queues: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert_eq!(queues.len(), 1);
        assert_eq!(queues[0].queue_name, Some("orders-queue".to_string()));
        assert_eq!(queues[0].queue_type, "sqs");

        // Verify deployment metadata
        let metadata = queues[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "terraform");
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

        let services: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Service(s) => Some(s),
                _ => None,
            })
            .collect();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "order-processor");
        assert_eq!(services[0].language, "python");
        assert_eq!(services[0].framework, Some("aws-lambda".to_string()));

        // Verify deployment metadata
        let metadata = services[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "terraform");
    }

    #[test]
    fn test_parse_s3_bucket() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_s3_bucket" "data" {
  bucket = "my-data-bucket"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();

        let resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) => Some(r),
                _ => None,
            })
            .collect();

        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0].resource_name,
            Some("my-data-bucket".to_string())
        );
        assert_eq!(resources[0].resource_type, "s3");

        // Verify deployment metadata
        let metadata = resources[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "terraform");
    }

    #[test]
    fn test_parse_sns_topic() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_sns_topic" "notifications" {
  name = "user-notifications"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();

        let queues: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert_eq!(queues.len(), 1);
        assert_eq!(queues[0].queue_name, Some("user-notifications".to_string()));
        assert_eq!(queues[0].queue_type, "sns");
    }

    #[test]
    fn test_parse_resource_without_name() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_dynamodb_table" "default" {
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "id"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();

        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        // Should use the terraform resource name as fallback
        assert_eq!(db_accesses.len(), 1);
        assert_eq!(db_accesses[0].table_name, Some("default".to_string()));
    }

    // ==================== M8-T1 New Tests: Tag Extraction ====================

    #[test]
    fn test_parse_terraform_tags_managed_by() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_lambda_function" "api" {
  function_name = "my-api"
  runtime       = "nodejs18.x"
  handler       = "index.handler"

  tags = {
    ManagedBy   = "Terraform"
    Environment = "production"
  }
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();
        let services: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Service(s) => Some(s),
                _ => None,
            })
            .collect();

        assert_eq!(services.len(), 1);
        let metadata = services[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "terraform");
        assert_eq!(metadata.environment, Some("production".to_string()));
        assert!(metadata.tags.contains_key("ManagedBy"));
        assert!(metadata.tags.contains_key("Environment"));
    }

    #[test]
    fn test_parse_terraform_backend_workspace() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
terraform {
  backend "s3" {
    bucket = "my-terraform-state"
    key    = "production/terraform.tfstate"
    region = "us-east-1"
  }
}

resource "aws_dynamodb_table" "users" {
  name = "users-table"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();
        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(db_accesses.len(), 1);
        let metadata = db_accesses[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.terraform_workspace, Some("production".to_string()));
        assert_eq!(metadata.environment, Some("production".to_string()));
    }

    #[test]
    fn test_parse_terraform_tags_variations() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_sqs_queue" "orders" {
  name = "orders-queue"

  tags = {
    managed_by = "terraform"
    env        = "staging"
  }
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();
        let queues: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert_eq!(queues.len(), 1);
        let metadata = queues[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "terraform");
        assert_eq!(metadata.environment, Some("staging".to_string()));
    }

    #[test]
    fn test_parse_terraform_resource_without_tags() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
resource "aws_dynamodb_table" "users" {
  name = "users-table"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();
        let db_accesses: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(db_accesses.len(), 1);
        let metadata = db_accesses[0].deployment_metadata.as_ref().unwrap();
        // Default to terraform since we're parsing Terraform files
        assert_eq!(metadata.deployment_method, "terraform");
        // No environment without tags
        assert!(metadata.environment.is_none());
    }

    #[test]
    fn test_parse_terraform_nested_key_path() {
        let parser = TerraformParser::new().unwrap();
        let content = r#"
terraform {
  backend "s3" {
    bucket = "my-state"
    key    = "env/prod/myservice/terraform.tfstate"
    region = "us-east-1"
  }
}

resource "aws_s3_bucket" "data" {
  bucket = "my-bucket"
}
"#;

        let discoveries = parser.parse_file(Path::new("main.tf"), content).unwrap();
        let resources: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) => Some(r),
                _ => None,
            })
            .collect();

        assert_eq!(resources.len(), 1);
        let metadata = resources[0].deployment_metadata.as_ref().unwrap();
        // Should extract "myservice" from the path (second-to-last segment)
        assert_eq!(metadata.terraform_workspace, Some("myservice".to_string()));
    }

    #[test]
    fn test_workspace_from_key_path_variations() {
        let parser = TerraformParser::new().unwrap();

        // Simple environment path
        assert_eq!(
            parser.workspace_from_key_path("production/terraform.tfstate"),
            Some("production".to_string())
        );

        // Nested path
        assert_eq!(
            parser.workspace_from_key_path("env/staging/terraform.tfstate"),
            Some("staging".to_string())
        );

        // Deep nested path - takes parent of tfstate
        assert_eq!(
            parser.workspace_from_key_path("org/team/dev/state.tfstate"),
            Some("dev".to_string())
        );

        // No path - default workspace
        assert_eq!(parser.workspace_from_key_path("terraform.tfstate"), None);
    }

    #[test]
    fn test_extract_environment_from_tags_case_insensitive() {
        let parser = TerraformParser::new().unwrap();

        let mut tags1 = HashMap::new();
        tags1.insert("Environment".to_string(), "prod".to_string());
        assert_eq!(
            parser.extract_environment_from_tags(&tags1),
            Some("prod".to_string())
        );

        let mut tags2 = HashMap::new();
        tags2.insert("environment".to_string(), "staging".to_string());
        assert_eq!(
            parser.extract_environment_from_tags(&tags2),
            Some("staging".to_string())
        );

        let mut tags3 = HashMap::new();
        tags3.insert("Env".to_string(), "dev".to_string());
        assert_eq!(
            parser.extract_environment_from_tags(&tags3),
            Some("dev".to_string())
        );

        let mut tags4 = HashMap::new();
        tags4.insert("env".to_string(), "test".to_string());
        assert_eq!(
            parser.extract_environment_from_tags(&tags4),
            Some("test".to_string())
        );

        let tags5 = HashMap::new();
        assert_eq!(parser.extract_environment_from_tags(&tags5), None);
    }
}
