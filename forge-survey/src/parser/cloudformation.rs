//! CloudFormation and SAM parser for detecting AWS resource definitions and deployment metadata.
//!
//! Detects resources from:
//! - SAM templates (AWS::Serverless::Function, AWS::Serverless::Api, etc.)
//! - CloudFormation templates (AWS::Lambda::Function, AWS::DynamoDB::Table, etc.)
//!
//! Extracts deployment metadata from:
//! - Template Transform field (SAM vs CloudFormation detection)
//! - Parameters section (environment detection)
//! - Resource properties (names, configurations)

use super::traits::*;
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;

/// Parser for CloudFormation and SAM YAML/JSON templates.
pub struct CloudFormationParser {}

impl CloudFormationParser {
    pub fn new() -> Result<Self, ParserError> {
        Ok(Self {})
    }

    /// Parse a YAML template file.
    fn parse_yaml(&self, content: &str) -> Result<serde_yaml::Value, ParserError> {
        serde_yaml::from_str(content)
            .map_err(|e| ParserError::TreeSitterError(format!("YAML parse error: {}", e)))
    }

    /// Parse a JSON template file.
    fn parse_json(&self, content: &str) -> Result<serde_json::Value, ParserError> {
        serde_json::from_str(content)
            .map_err(|e| ParserError::TreeSitterError(format!("JSON parse error: {}", e)))
    }

    /// Check if this is a valid CloudFormation/SAM template.
    ///
    /// Valid templates must have:
    /// - AWSTemplateFormatVersion field, OR
    /// - Be named template.yaml/yml/json (checked by caller)
    fn is_template(&self, value: &serde_yaml::Value) -> bool {
        if let serde_yaml::Value::Mapping(map) = value {
            // Check for AWSTemplateFormatVersion
            if map.contains_key("AWSTemplateFormatVersion") {
                return true;
            }
            // Check for Transform (SAM templates may have this without AWSTemplateFormatVersion)
            if map.contains_key("Transform") {
                return true;
            }
            // Check for Resources section (minimal valid template)
            if map.contains_key("Resources") {
                return true;
            }
        }
        false
    }

    /// Determine if template is SAM or raw CloudFormation.
    ///
    /// SAM templates have a Transform field containing "AWS::Serverless-*".
    fn is_sam_template(&self, template: &serde_yaml::Value) -> bool {
        if let Some(transform) = template.get("Transform") {
            // Handle string transform
            if let Some(s) = transform.as_str() {
                return s.contains("AWS::Serverless");
            }
            // Handle array of transforms
            if let Some(arr) = transform.as_sequence() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if s.contains("AWS::Serverless") {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Extract resources from a parsed template.
    fn extract_resources(&self, template: &serde_yaml::Value, path: &Path) -> Vec<Discovery> {
        let mut discoveries = Vec::new();

        let is_sam = self.is_sam_template(template);
        let deployment_method = if is_sam { "sam" } else { "cloudformation" };

        // Extract environment from Parameters
        let environment = self.extract_environment_from_parameters(template);

        // Extract stack name from metadata or filename
        let stack_name = self.extract_stack_name(template, path);

        // Process Resources section
        if let Some(resources) = template.get("Resources") {
            if let Some(resources_map) = resources.as_mapping() {
                for (key, resource) in resources_map {
                    if let Some(logical_id) = key.as_str() {
                        if let Some(discovery) = self.process_resource(
                            logical_id,
                            resource,
                            path,
                            deployment_method,
                            environment.as_deref(),
                            stack_name.as_deref(),
                        ) {
                            discoveries.push(discovery);
                        }
                    }
                }
            }
        }

        discoveries
    }

    /// Process a single resource and convert it to a Discovery.
    fn process_resource(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let resource_type = resource.get("Type")?.as_str()?;

        match resource_type {
            // SAM function types
            "AWS::Serverless::Function" => self.parse_serverless_function(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            // CloudFormation Lambda
            "AWS::Lambda::Function" => self.parse_lambda_function(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            // DynamoDB
            "AWS::DynamoDB::Table" => self.parse_dynamodb_table(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            // SQS
            "AWS::SQS::Queue" => self.parse_sqs_queue(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            // SNS
            "AWS::SNS::Topic" => self.parse_sns_topic(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            // S3
            "AWS::S3::Bucket" => self.parse_s3_bucket(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            // SAM API
            "AWS::Serverless::Api" => self.parse_serverless_api(
                logical_id,
                resource,
                path,
                deployment_method,
                environment,
                stack_name,
            ),
            _ => None,
        }
    }

    /// Parse AWS::Serverless::Function resource.
    fn parse_serverless_function(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties")?;

        // Get function name (use logical ID as fallback)
        let function_name = properties
            .get("FunctionName")
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Get runtime
        let runtime = properties
            .get("Runtime")
            .and_then(|v| self.extract_string_value(v));

        // Get handler
        let handler = properties
            .get("Handler")
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| "index.handler".to_string());

        // Infer language from runtime
        let language = self.infer_language_from_runtime(runtime.as_deref());

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        Some(Discovery::Service(ServiceDiscovery {
            name: function_name,
            language,
            framework: Some("aws-lambda".to_string()),
            entry_point: handler,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Parse AWS::Lambda::Function resource.
    fn parse_lambda_function(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties")?;

        // Get function name (use logical ID as fallback)
        let function_name = properties
            .get("FunctionName")
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Get runtime
        let runtime = properties
            .get("Runtime")
            .and_then(|v| self.extract_string_value(v));

        // Get handler
        let handler = properties
            .get("Handler")
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| "index.handler".to_string());

        // Infer language from runtime
        let language = self.infer_language_from_runtime(runtime.as_deref());

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        Some(Discovery::Service(ServiceDiscovery {
            name: function_name,
            language,
            framework: Some("aws-lambda".to_string()),
            entry_point: handler,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Parse AWS::DynamoDB::Table resource.
    fn parse_dynamodb_table(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties")?;

        // Get table name (use logical ID as fallback)
        let table_name = properties
            .get("TableName")
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        Some(Discovery::DatabaseAccess(DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some(table_name),
            operation: DatabaseOperation::Unknown, // Template defines table, not operations
            detection_method: deployment_method.to_string(),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Parse AWS::SQS::Queue resource.
    fn parse_sqs_queue(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties");

        // Get queue name (use logical ID as fallback)
        let queue_name = properties
            .and_then(|p| p.get("QueueName"))
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sqs".to_string(),
            queue_name: Some(queue_name),
            operation: QueueOperationType::Unknown,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Parse AWS::SNS::Topic resource.
    fn parse_sns_topic(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties");

        // Get topic name (use logical ID as fallback)
        let topic_name = properties
            .and_then(|p| p.get("TopicName"))
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        Some(Discovery::QueueOperation(QueueOperationDiscovery {
            queue_type: "sns".to_string(),
            queue_name: Some(topic_name),
            operation: QueueOperationType::Unknown,
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Parse AWS::S3::Bucket resource.
    fn parse_s3_bucket(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties");

        // Get bucket name (use logical ID as fallback)
        let bucket_name = properties
            .and_then(|p| p.get("BucketName"))
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        Some(Discovery::CloudResourceUsage(CloudResourceDiscovery {
            resource_type: "s3".to_string(),
            resource_name: Some(bucket_name),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Parse AWS::Serverless::Api resource.
    fn parse_serverless_api(
        &self,
        logical_id: &str,
        resource: &serde_yaml::Value,
        path: &Path,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> Option<Discovery> {
        let properties = resource.get("Properties");

        // Get API name (use logical ID as fallback)
        let api_name = properties
            .and_then(|p| p.get("Name"))
            .and_then(|v| self.extract_string_value(v))
            .unwrap_or_else(|| logical_id.to_string());

        // Get stage name if available
        let _stage_name = properties
            .and_then(|p| p.get("StageName"))
            .and_then(|v| self.extract_string_value(v));

        // Build deployment metadata
        let metadata = self.build_deployment_metadata(deployment_method, environment, stack_name);

        // APIs are represented as CloudResources with type "apigateway"
        Some(Discovery::CloudResourceUsage(CloudResourceDiscovery {
            resource_type: "apigateway".to_string(),
            resource_name: Some(api_name),
            source_file: path.to_string_lossy().to_string(),
            source_line: 1,
            deployment_metadata: Some(metadata),
        }))
    }

    /// Extract string value, handling intrinsic functions.
    ///
    /// This extracts the value if it's a simple string.
    /// For intrinsic functions like !Ref or !Sub, returns the reference name
    /// as a best-effort extraction.
    fn extract_string_value(&self, value: &serde_yaml::Value) -> Option<String> {
        // Simple string
        if let Some(s) = value.as_str() {
            return Some(s.to_string());
        }

        // Handle !Ref intrinsic function
        if let Some(map) = value.as_mapping() {
            // CloudFormation intrinsic functions in YAML
            if let Some(ref_value) = map.get("Ref") {
                if let Some(s) = ref_value.as_str() {
                    return Some(format!("${{Ref:{}}}", s));
                }
            }
            // !Sub intrinsic function
            if let Some(sub_value) = map.get("Fn::Sub") {
                if let Some(s) = sub_value.as_str() {
                    // Return the template string as-is
                    return Some(s.to_string());
                }
                // Handle array form: [template, {var1: val1}]
                if let Some(arr) = sub_value.as_sequence() {
                    if let Some(first) = arr.first() {
                        if let Some(s) = first.as_str() {
                            return Some(s.to_string());
                        }
                    }
                }
            }
            // !GetAtt
            if let Some(getatt_value) = map.get("Fn::GetAtt") {
                if let Some(arr) = getatt_value.as_sequence() {
                    let parts: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !parts.is_empty() {
                        return Some(format!("${{GetAtt:{}}}", parts.join(".")));
                    }
                }
            }
        }

        None
    }

    /// Extract environment from Parameters section.
    ///
    /// Looks for common environment parameter names and extracts the default value.
    fn extract_environment_from_parameters(&self, template: &serde_yaml::Value) -> Option<String> {
        let parameters = template.get("Parameters")?;
        let params_map = parameters.as_mapping()?;

        // Common environment parameter names
        let env_param_names = ["Environment", "Env", "Stage", "environment", "env", "stage"];

        for param_name in env_param_names {
            if let Some(param) = params_map.get(param_name) {
                // Try to get Default value
                if let Some(default) = param.get("Default") {
                    if let Some(s) = default.as_str() {
                        return Some(s.to_string());
                    }
                }
            }
        }

        None
    }

    /// Extract stack name from template metadata or filename.
    fn extract_stack_name(&self, template: &serde_yaml::Value, path: &Path) -> Option<String> {
        // Try to get from Metadata section
        if let Some(metadata) = template.get("Metadata") {
            // Check for custom stack name in metadata
            if let Some(stack_name) = metadata.get("StackName") {
                if let Some(s) = stack_name.as_str() {
                    return Some(s.to_string());
                }
            }
        }

        // Try to get from Description (often contains stack name)
        // This is a best-effort heuristic

        // Use filename without extension as stack name
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }

    /// Infer programming language from AWS Lambda runtime.
    fn infer_language_from_runtime(&self, runtime: Option<&str>) -> String {
        match runtime {
            Some(r) if r.starts_with("python") => "python".to_string(),
            Some(r) if r.starts_with("nodejs") => "javascript".to_string(),
            Some(r) if r.starts_with("java") => "java".to_string(),
            Some(r) if r.starts_with("go") => "go".to_string(),
            Some(r) if r.starts_with("ruby") => "ruby".to_string(),
            Some(r) if r.starts_with("dotnet") => "csharp".to_string(),
            Some(r) if r.starts_with("provided") => "custom".to_string(),
            Some(r) => r.to_string(),
            None => "unknown".to_string(),
        }
    }

    /// Build deployment metadata struct.
    fn build_deployment_metadata(
        &self,
        deployment_method: &str,
        environment: Option<&str>,
        stack_name: Option<&str>,
    ) -> DeploymentMetadata {
        DeploymentMetadata {
            deployment_method: deployment_method.to_string(),
            terraform_workspace: None, // Not applicable for CloudFormation/SAM
            environment: environment.map(|s| s.to_string()),
            stack_name: stack_name.map(|s| s.to_string()),
            tags: HashMap::new(),
        }
    }

    /// Check if a file is a CloudFormation/SAM template by filename.
    fn is_template_filename(&self, path: &Path) -> bool {
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let filename_lower = filename.to_lowercase();

        // Common SAM/CloudFormation template names
        filename_lower == "template.yaml"
            || filename_lower == "template.yml"
            || filename_lower == "template.json"
            || filename_lower == "samconfig.yaml"
            || filename_lower == "samconfig.yml"
    }
}

impl Parser for CloudFormationParser {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supported_extensions(&self) -> &[&str] {
        // CloudFormation/SAM templates use YAML or JSON
        &["yaml", "yml", "json"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        // Determine file format
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        // Parse based on extension
        let template: serde_yaml::Value = if ext == "json" {
            // Parse JSON and convert to YAML Value for uniform processing
            let json_value = self.parse_json(content)?;
            serde_json::from_value(json_value).map_err(|e| {
                ParserError::TreeSitterError(format!("JSON to YAML conversion error: {}", e))
            })?
        } else {
            self.parse_yaml(content)?
        };

        // Check if this is a valid CloudFormation/SAM template
        if !self.is_template(&template) && !self.is_template_filename(path) {
            // Not a CloudFormation template, return empty (no error)
            return Ok(Vec::new());
        }

        // Extract resources
        Ok(self.extract_resources(&template, path))
    }

    /// Custom repository parsing to filter for CloudFormation/SAM templates.
    ///
    /// Unlike other parsers that process all files with matching extensions,
    /// this parser only processes files that are valid CloudFormation/SAM templates.
    fn parse_repo(&self, repo_path: &Path) -> Result<Vec<Discovery>, ParserError> {
        let mut all_discoveries = Vec::new();
        let extensions = self.supported_extensions();

        for entry in walkdir::WalkDir::new(repo_path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| !is_ignored_cloudformation_dir(e.file_name().to_str().unwrap_or("")))
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
                    continue;
                }
            };

            match self.parse_file(path, &content) {
                Ok(discoveries) => all_discoveries.extend(discoveries),
                Err(e) => {
                    // Log but continue - don't fail entire survey for one file
                    tracing::debug!("Failed to parse {}: {}", path.display(), e);
                }
            }
        }

        Ok(all_discoveries)
    }
}

/// Directories to skip during CloudFormation template scanning.
fn is_ignored_cloudformation_dir(name: &str) -> bool {
    matches!(
        name,
        // General build/cache directories
        "node_modules"
            | ".git"
            | "target"
            | "dist"
            | "build"
            | "__pycache__"
            | ".pytest_cache"
            | "venv"
            | ".venv"
            // CloudFormation/SAM specific
            | ".aws-sam"
            | ".serverless"
            // IDE/Editor
            | ".idea"
            | ".vscode"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Template Detection Tests ====================

    #[test]
    fn test_is_sam_template_with_transform() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Resources:
  MyFunction:
    Type: AWS::Serverless::Function
"#;
        let template: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        assert!(parser.is_sam_template(&template));
    }

    #[test]
    fn test_is_cloudformation_without_transform() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  MyTable:
    Type: AWS::DynamoDB::Table
"#;
        let template: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        assert!(!parser.is_sam_template(&template));
    }

    #[test]
    fn test_is_template_with_aws_version() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources: {}
"#;
        let template: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        assert!(parser.is_template(&template));
    }

    #[test]
    fn test_is_not_template_regular_yaml() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
config:
  database: postgres
  host: localhost
"#;
        let template: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        assert!(!parser.is_template(&template));
    }

    // ==================== SAM Function Parsing Tests ====================

    #[test]
    fn test_parse_sam_serverless_function() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Resources:
  UserApiFunction:
    Type: AWS::Serverless::Function
    Properties:
      FunctionName: user-api
      Runtime: python3.11
      Handler: app.handler
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        let services: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Service(s) => Some(s),
                _ => None,
            })
            .collect();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "user-api");
        assert_eq!(services[0].language, "python");
        assert_eq!(services[0].framework, Some("aws-lambda".to_string()));
        assert_eq!(services[0].entry_point, "app.handler");

        let metadata = services[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "sam");
    }

    #[test]
    fn test_parse_sam_function_without_name() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Resources:
  MyFunction:
    Type: AWS::Serverless::Function
    Properties:
      Runtime: nodejs18.x
      Handler: index.handler
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        let services: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Service(s) => Some(s),
                _ => None,
            })
            .collect();

        assert_eq!(services.len(), 1);
        // Should use logical ID as fallback
        assert_eq!(services[0].name, "MyFunction");
        assert_eq!(services[0].language, "javascript");
    }

    // ==================== CloudFormation Resource Tests ====================

    #[test]
    fn test_parse_cloudformation_lambda() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  ProcessorFunction:
    Type: AWS::Lambda::Function
    Properties:
      FunctionName: order-processor
      Runtime: python3.9
      Handler: main.handler
"#;

        let discoveries = parser
            .parse_file(Path::new("lambda.yaml"), content)
            .unwrap();

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

        let metadata = services[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "cloudformation");
    }

    #[test]
    fn test_parse_cloudformation_dynamodb() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: users-table
      BillingMode: PAY_PER_REQUEST
"#;

        let discoveries = parser
            .parse_file(Path::new("database.yaml"), content)
            .unwrap();

        let databases: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(databases.len(), 1);
        assert_eq!(databases[0].table_name, Some("users-table".to_string()));
        assert_eq!(databases[0].db_type, "dynamodb");

        let metadata = databases[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.deployment_method, "cloudformation");
    }

    #[test]
    fn test_parse_cloudformation_sqs() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  OrdersQueue:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: orders-queue
"#;

        let discoveries = parser.parse_file(Path::new("queue.yaml"), content).unwrap();

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
    fn test_parse_cloudformation_sns() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  NotificationsTopic:
    Type: AWS::SNS::Topic
    Properties:
      TopicName: notifications
"#;

        let discoveries = parser
            .parse_file(Path::new("notifications.yaml"), content)
            .unwrap();

        let queues: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert_eq!(queues.len(), 1);
        assert_eq!(queues[0].queue_name, Some("notifications".to_string()));
        assert_eq!(queues[0].queue_type, "sns");
    }

    #[test]
    fn test_parse_cloudformation_s3() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  DataBucket:
    Type: AWS::S3::Bucket
    Properties:
      BucketName: my-data-bucket
"#;

        let discoveries = parser
            .parse_file(Path::new("storage.yaml"), content)
            .unwrap();

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
    }

    #[test]
    fn test_parse_serverless_api() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Resources:
  UserApi:
    Type: AWS::Serverless::Api
    Properties:
      Name: user-api
      StageName: prod
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        let apis: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::CloudResourceUsage(r) if r.resource_type == "apigateway" => Some(r),
                _ => None,
            })
            .collect();

        assert_eq!(apis.len(), 1);
        assert_eq!(apis[0].resource_name, Some("user-api".to_string()));
    }

    // ==================== Parameter Extraction Tests ====================

    #[test]
    fn test_extract_environment_from_parameters() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Parameters:
  Environment:
    Type: String
    Default: production
Resources:
  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: users
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        let databases: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(databases.len(), 1);
        let metadata = databases[0].deployment_metadata.as_ref().unwrap();
        assert_eq!(metadata.environment, Some("production".to_string()));
    }

    #[test]
    fn test_extract_environment_from_stage_parameter() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Parameters:
  Stage:
    Type: String
    Default: staging
Resources:
  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: users
"#;

        let template: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        let env = parser.extract_environment_from_parameters(&template);

        assert_eq!(env, Some("staging".to_string()));
    }

    // ==================== Edge Cases Tests ====================

    #[test]
    fn test_ignore_non_template_yaml() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
config:
  database: postgres
  port: 5432
"#;

        let discoveries = parser
            .parse_file(Path::new("config.yaml"), content)
            .unwrap();
        assert!(discoveries.is_empty());
    }

    #[test]
    fn test_handle_malformed_yaml() {
        let parser = CloudFormationParser::new().unwrap();
        let content = "not: valid: yaml: content: [";

        let result = parser.parse_file(Path::new("bad.yaml"), content);
        assert!(result.is_err());
    }

    #[test]
    fn test_resource_without_properties() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Resources:
  MyQueue:
    Type: AWS::SQS::Queue
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        let queues: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();

        assert_eq!(queues.len(), 1);
        // Should use logical ID as fallback
        assert_eq!(queues[0].queue_name, Some("MyQueue".to_string()));
    }

    #[test]
    fn test_multiple_resources_same_template() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Resources:
  ApiFunction:
    Type: AWS::Serverless::Function
    Properties:
      FunctionName: api
      Runtime: nodejs18.x
      Handler: index.handler
  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: users
  OrdersQueue:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: orders
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        assert_eq!(discoveries.len(), 3);

        let services: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::Service(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(services.len(), 1);

        let databases: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();
        assert_eq!(databases.len(), 1);

        let queues: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::QueueOperation(q) => Some(q),
                _ => None,
            })
            .collect();
        assert_eq!(queues.len(), 1);
    }

    #[test]
    fn test_intrinsic_function_sub() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"
AWSTemplateFormatVersion: '2010-09-09'
Parameters:
  Environment:
    Type: String
    Default: prod
Resources:
  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: !Sub '${Environment}-users'
"#;

        let discoveries = parser
            .parse_file(Path::new("template.yaml"), content)
            .unwrap();

        let databases: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(databases.len(), 1);
        // The !Sub template string should be extracted
        assert_eq!(
            databases[0].table_name,
            Some("${Environment}-users".to_string())
        );
    }

    #[test]
    fn test_json_template() {
        let parser = CloudFormationParser::new().unwrap();
        let content = r#"{
  "AWSTemplateFormatVersion": "2010-09-09",
  "Resources": {
    "UsersTable": {
      "Type": "AWS::DynamoDB::Table",
      "Properties": {
        "TableName": "users-table"
      }
    }
  }
}"#;

        let discoveries = parser
            .parse_file(Path::new("template.json"), content)
            .unwrap();

        let databases: Vec<_> = discoveries
            .iter()
            .filter_map(|d| match d {
                Discovery::DatabaseAccess(db) => Some(db),
                _ => None,
            })
            .collect();

        assert_eq!(databases.len(), 1);
        assert_eq!(databases[0].table_name, Some("users-table".to_string()));
    }

    #[test]
    fn test_runtime_language_inference() {
        let parser = CloudFormationParser::new().unwrap();

        assert_eq!(
            parser.infer_language_from_runtime(Some("python3.11")),
            "python"
        );
        assert_eq!(
            parser.infer_language_from_runtime(Some("nodejs18.x")),
            "javascript"
        );
        assert_eq!(parser.infer_language_from_runtime(Some("java11")), "java");
        assert_eq!(parser.infer_language_from_runtime(Some("go1.x")), "go");
        assert_eq!(parser.infer_language_from_runtime(Some("ruby2.7")), "ruby");
        assert_eq!(
            parser.infer_language_from_runtime(Some("dotnetcore3.1")),
            "csharp"
        );
        assert_eq!(
            parser.infer_language_from_runtime(Some("provided.al2")),
            "custom"
        );
        assert_eq!(parser.infer_language_from_runtime(None), "unknown");
    }

    #[test]
    fn test_is_template_filename() {
        let parser = CloudFormationParser::new().unwrap();

        assert!(parser.is_template_filename(Path::new("template.yaml")));
        assert!(parser.is_template_filename(Path::new("template.yml")));
        assert!(parser.is_template_filename(Path::new("template.json")));
        assert!(parser.is_template_filename(Path::new("Template.YAML"))); // case-insensitive
        assert!(!parser.is_template_filename(Path::new("config.yaml")));
        assert!(!parser.is_template_filename(Path::new("app.json")));
    }
}
