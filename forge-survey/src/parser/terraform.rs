//! Terraform parser for detecting AWS resource definitions.
//!
//! Detects:
//! - aws_dynamodb_table resources
//! - aws_sqs_queue resources
//! - aws_sns_topic resources
//! - aws_s3_bucket resources
//! - aws_lambda_function resources

use super::traits::*;
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

        for block in body.blocks() {
            let ident: &str = block.identifier();
            if ident == "resource" {
                if let Some(discovery) = self.process_resource_block(&block, path) {
                    discoveries.push(discovery);
                }
            }
        }

        discoveries
    }

    fn process_resource_block(&self, block: &hcl::Block, path: &Path) -> Option<Discovery> {
        let labels = block.labels();

        if labels.len() < 2 {
            return None;
        }

        let resource_type = labels[0].as_str();
        let resource_name = labels[1].as_str();

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
        let table_name = self.get_string_attribute(block.body(), "name")
            .unwrap_or_else(|| tf_name.to_string());

        Some(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some(table_name),
            operation: DatabaseOperation::Unknown, // Terraform defines the table, not operations
            detection_method: "terraform".to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_sqs_queue(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let queue_name = self.get_string_attribute(block.body(), "name")
            .unwrap_or_else(|| tf_name.to_string());

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sqs".to_string(),
            queue_name: Some(queue_name),
            operation: QueueOperationType::Unknown, // Terraform defines the queue, not operations
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_sns_topic(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let topic_name = self.get_string_attribute(block.body(), "name")
            .unwrap_or_else(|| tf_name.to_string());

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sns".to_string(),
            queue_name: Some(topic_name),
            operation: QueueOperationType::Unknown,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_s3_bucket(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let bucket_name = self.get_string_attribute(block.body(), "bucket")
            .unwrap_or_else(|| tf_name.to_string());

        Some(Discovery::CloudResourceUsage(CloudResourceDiscovery {
            resource_type: "s3".to_string(),
            resource_name: Some(bucket_name),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn parse_lambda_function(&self, block: &hcl::Block, tf_name: &str, path: &Path) -> Option<Discovery> {
        let function_name = self.get_string_attribute(block.body(), "function_name")
            .unwrap_or_else(|| tf_name.to_string());

        let runtime = self.get_string_attribute(block.body(), "runtime");
        let handler = self.get_string_attribute(block.body(), "handler");

        let language = runtime.as_ref().map(|r| {
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
        }).unwrap_or_else(|| "unknown".to_string());

        // Lambda functions are services
        Some(Discovery::Service(ServiceDiscovery {
            name: function_name,
            language,
            framework: Some("aws-lambda".to_string()),
            entry_point: handler.unwrap_or_else(|| "index.handler".to_string()),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
        }))
    }

    fn get_string_attribute(&self, body: &hcl::Body, key: &str) -> Option<String> {
        body.attributes()
            .find(|attr| attr.key() == key)
            .and_then(|attr| {
                match attr.expr() {
                    hcl::Expression::String(s) => Some(s.to_string()),
                    _ => None,
                }
            })
    }
}

impl Parser for TerraformParser {
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
        assert_eq!(resources[0].resource_name, Some("my-data-bucket".to_string()));
        assert_eq!(resources[0].resource_type, "s3");
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
}
