# Deployment Metadata Extraction - Specification

## Purpose

Extract deployment context from Infrastructure as Code (IaC) source files to help LLM coding agents understand **HOW code is deployed**, not just WHAT is deployed. This enables LLMs to generate correct deployment commands and understand the operational context of services.

## Context Sources

Deployment metadata is extracted from IaC files in the codebase itself, **not from AWS runtime state**. This approach:
- Works offline (no AWS API calls needed)
- Is faster (no network latency)
- Is cheaper (no Lambda/API costs)
- Provides "source of truth" from version-controlled files

### Terraform

Terraform files (`.tf`) contain deployment metadata in:
1. **Resource tags** - Indicate deployment method, environment, ownership
2. **Backend configuration** - Reveals workspace/environment
3. **Resource definitions** - Service names, runtimes, configurations

**Tag Patterns to Detect**:

| Tag Key | Example Value | Extracted Attribute |
|---------|--------------|---------------------|
| `ManagedBy` | "Terraform" | deployment_method = "terraform" |
| `Environment` | "production" | environment = "production" |
| `terraform:workspace` | "prod" | terraform_workspace = "prod" |
| `aws:cloudformation:stack-name` | "my-stack" | Indicates CloudFormation management |

**Backend Configuration Example**:

```hcl
terraform {
  backend "s3" {
    bucket = "my-terraform-state"
    key    = "production/terraform.tfstate"
    region = "us-east-1"
  }
}
```

Extract: `terraform_workspace = "production"` (from key path)

**Resource Example**:

```hcl
resource "aws_lambda_function" "user_api" {
  function_name = "user-api"
  runtime       = "python3.11"
  handler       = "app.handler"

  tags = {
    Environment        = "production"
    ManagedBy         = "Terraform"
    terraform:workspace = "prod"
    Owner             = "Platform Team"
  }
}
```

**Extracted Attributes**:
- `deployment_method`: "terraform"
- `terraform_workspace`: "prod"
- `environment`: "production"
- Additional context: Owner tag can inform business context

### SAM (Serverless Application Model)

SAM templates (`template.yaml`, `template.yml`) are CloudFormation templates with serverless transform.

**Template Structure**:

```yaml
AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Description: User API Stack

Parameters:
  Environment:
    Type: String
    Default: production
    Description: Deployment environment

Globals:
  Function:
    Runtime: python3.11
    Timeout: 30
    MemorySize: 512

Resources:
  UserApiFunction:
    Type: AWS::Serverless::Function
    Properties:
      FunctionName: user-api
      CodeUri: ./src
      Handler: app.handler
      Environment:
        Variables:
          ENV: !Ref Environment
          TABLE_NAME: !Ref UsersTable

  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: !Sub '${Environment}-users'
      AttributeDefinitions:
        - AttributeName: userId
          AttributeType: S
      KeySchema:
        - AttributeName: userId
          KeyType: HASH

  UserApiGateway:
    Type: AWS::Serverless::Api
    Properties:
      StageName: !Ref Environment
```

**Extracted Attributes**:
- `deployment_method`: "sam"
- `stack_name`: from filename or metadata (e.g., "user-api-stack")
- `environment`: from Parameters (e.g., "production")
- Resources extracted: UserApiFunction (Service), UsersTable (Database), UserApiGateway (API)

**SAM Detection**:
- Presence of `Transform: AWS::Serverless-*` indicates SAM
- Resource types start with `AWS::Serverless::*`

### CloudFormation

Raw CloudFormation templates (no SAM Transform) follow similar structure but with different resource types.

**Template Example**:

```yaml
AWSTemplateFormatVersion: '2010-09-09'
Description: User Infrastructure

Parameters:
  EnvironmentName:
    Type: String
    Default: production

Resources:
  UsersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: !Sub '${EnvironmentName}-users'
      BillingMode: PAY_PER_REQUEST

  OrdersQueue:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: !Sub '${EnvironmentName}-orders-queue'
      VisibilityTimeout: 300

  UsersBucket:
    Type: AWS::S3::Bucket
    Properties:
      BucketName: !Sub '${EnvironmentName}-users-data'
```

**Extracted Attributes**:
- `deployment_method`: "cloudformation"
- `stack_name`: from filename or metadata
- `environment`: from Parameters
- Resources: DynamoDB tables, SQS queues, S3 buckets

**CloudFormation vs SAM**:
- CloudFormation: No Transform field, uses `AWS::*` resource types (not Serverless)
- SAM: Has Transform field, uses `AWS::Serverless::*` types

## Node Attribute Schema

### Standard Deployment Attributes

All resource nodes should include deployment metadata attributes:

```rust
// Attributes added to Node.attributes HashMap

/// How this resource is deployed
deployment_method: "terraform" | "sam" | "cloudformation" | "unknown"

/// CloudFormation/SAM stack name
stack_name: Option<String>

/// Terraform workspace (maps to environment usually)
terraform_workspace: Option<String>

/// Environment name (dev/staging/prod)
environment: Option<String>

/// AWS account ID (injected from forge.yaml environment mapping)
aws_account_id: Option<String>
```

### Attribute Priority

When multiple sources provide the same attribute:

1. **Explicit tags** (highest priority) - `Environment` tag
2. **Parameters** - CloudFormation Parameters section
3. **Backend config** - Terraform backend key path
4. **Naming conventions** - Extract from resource names (e.g., "prod-users-table")
5. **forge.yaml mapping** - Environment section (lowest priority, fallback)

## Parser Enhancements

### TerraformParser Enhancement

**File**: `forge-survey/src/parser/terraform.rs`

**New Methods**:

```rust
/// Extract tags from HCL resource block
fn parse_tags(&self, block: &hcl::Block) -> HashMap<String, String>

/// Infer deployment method from tag patterns
fn infer_deployment_method(&self, tags: &HashMap<String, String>) -> String

/// Extract workspace from backend configuration
fn parse_backend_workspace(&self, body: &hcl::Body) -> Option<String>

/// Extract environment from tags (supports common variations)
fn extract_environment(&self, tags: &HashMap<String, String>) -> Option<String>
```

**Tag Extraction Logic**:

```
For each resource block:
  1. Find 'tags' attribute
  2. Parse key-value pairs
  3. Normalize keys (lowercase, handle underscores vs hyphens)
  4. Check for deployment indicators:
     - ManagedBy, managed_by, managedBy → deployment method
     - Environment, Env, env → environment name
     - terraform:workspace → workspace name
  5. Store in Discovery metadata
```

**Backend Extraction Logic**:

```
For terraform block:
  1. Find backend configuration (s3, remote, etc.)
  2. Extract 'key' attribute value
  3. Parse workspace from path:
     - "production/terraform.tfstate" → "production"
     - "env/prod/terraform.tfstate" → "prod"
     - "terraform.tfstate" → "default"
```

### CloudFormationParser (NEW)

**File**: `forge-survey/src/parser/cloudformation.rs`

**Parser Structure**:

```rust
pub struct CloudFormationParser {
    // No internal state needed - stateless parser
}

impl CloudFormationParser {
    pub fn new() -> Result<Self, ParserError>

    /// Determine if template is SAM or raw CloudFormation
    fn is_sam_template(&self, template: &serde_yaml::Value) -> bool

    /// Parse AWS::Serverless::Function resources → Service discoveries
    fn parse_sam_function(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery>

    /// Parse AWS::Lambda::Function resources → Service discoveries
    fn parse_lambda_function(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery>

    /// Parse AWS::DynamoDB::Table resources → Database discoveries
    fn parse_dynamodb_table(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery>

    /// Parse AWS::SQS::Queue resources → Queue discoveries
    fn parse_sqs_queue(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery>

    /// Parse AWS::SNS::Topic resources → Queue discoveries
    fn parse_sns_topic(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery>

    /// Parse AWS::S3::Bucket resources → CloudResource discoveries
    fn parse_s3_bucket(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery>

    /// Extract Parameters section for environment detection
    fn parse_parameters(&self, template: &serde_yaml::Value) -> HashMap<String, String>
}
```

**File Detection**:

CloudFormation parser should only parse files that:
1. Have `.yaml`, `.yml`, or `.json` extensions, AND
2. Contain `AWSTemplateFormatVersion` field, OR
3. Are named `template.yaml`, `template.yml`, `template.json`

**Resource Type Mapping**:

| CloudFormation Type | Forge Node Type | Attributes Extracted |
|---------------------|-----------------|----------------------|
| `AWS::Serverless::Function` | Service | FunctionName, Runtime, Handler |
| `AWS::Lambda::Function` | Service | FunctionName, Runtime, Handler |
| `AWS::DynamoDB::Table` | Database | TableName, BillingMode |
| `AWS::SQS::Queue` | Queue | QueueName |
| `AWS::SNS::Topic` | Queue | TopicName |
| `AWS::S3::Bucket` | CloudResource | BucketName |
| `AWS::Serverless::Api` | API | StageName, DefinitionUri |

**Parameter Extraction**:

```
For Parameters section:
  1. Iterate over each parameter
  2. Check for common environment indicators:
     - "Environment", "Env", "Stage"
  3. Extract Default value
  4. Store in metadata for environment attribute
```

## Test Scenarios

### Unit Tests

**TerraformParser Tag Extraction**:

```
Test: parse_terraform_tags_managed_by
Input:
  resource "aws_lambda_function" "api" {
    tags = {
      ManagedBy = "Terraform"
      Environment = "production"
    }
  }
Expected:
  deployment_method = "terraform"
  environment = "production"
```

```
Test: parse_terraform_backend_workspace
Input:
  terraform {
    backend "s3" {
      key = "production/terraform.tfstate"
    }
  }
Expected:
  terraform_workspace = "production"
```

```
Test: parse_terraform_tags_variations
Input:
  tags = {
    managed_by = "terraform"
    env = "staging"
  }
Expected:
  deployment_method = "terraform"
  environment = "staging"
```

```
Test: parse_terraform_resource_without_tags
Input:
  resource "aws_dynamodb_table" "users" {
    name = "users-table"
  }
Expected:
  deployment_method = "terraform" (default)
  environment = None
```

**CloudFormationParser SAM Template**:

```
Test: parse_sam_template_lambda
Input:
  AWSTemplateFormatVersion: '2010-09-09'
  Transform: AWS::Serverless-2016-10-31
  Resources:
    ApiFunction:
      Type: AWS::Serverless::Function
      Properties:
        FunctionName: user-api
        Runtime: python3.11
Expected:
  Discovery::Service {
    name: "user-api",
    attributes: {
      deployment_method: "sam",
      runtime: "python3.11"
    }
  }
```

```
Test: parse_cloudformation_dynamodb
Input:
  AWSTemplateFormatVersion: '2010-09-09'
  Resources:
    UsersTable:
      Type: AWS::DynamoDB::Table
      Properties:
        TableName: users-table
Expected:
  Discovery::DatabaseAccess {
    operation: ReadWrite,
    table_name: "users-table",
    attributes: {
      deployment_method: "cloudformation"
    }
  }
```

```
Test: detect_sam_vs_cloudformation
Input (SAM):
  Transform: AWS::Serverless-2016-10-31
Expected:
  is_sam_template = true

Input (CloudFormation):
  # No Transform field
Expected:
  is_sam_template = false
```

```
Test: parse_parameters_for_environment
Input:
  Parameters:
    Environment:
      Type: String
      Default: production
Expected:
  environment = "production"
```

```
Test: ignore_non_template_yaml
Input:
  # Regular YAML file without AWSTemplateFormatVersion
  config:
    database: postgres
Expected:
  discoveries = []
```

### Integration Tests

**Multi-IaC Repository**:

```
Test: test_survey_mixed_iac_repo

Fixture structure:
  infrastructure/
    main.tf                # Terraform Lambda
    template.yaml          # SAM API Gateway
    dynamodb.yaml          # CloudFormation DynamoDB

Expected graph:
  1. Lambda function node:
     - deployment_method = "terraform"
     - terraform_workspace = extracted from backend

  2. API Gateway node:
     - deployment_method = "sam"
     - stack_name = "user-api-stack"

  3. DynamoDB table node:
     - deployment_method = "cloudformation"
     - environment = from Parameters
```

**Deployment Metadata Completeness**:

```
Test: test_deployment_metadata_attributes

Fixture: Terraform Lambda with full tags

Expected node attributes:
  {
    "deployment_method": "terraform",
    "terraform_workspace": "production",
    "environment": "production",
    "language": "python",
    "runtime": "python3.11"
  }
```

**SAM Stack Detection**:

```
Test: test_sam_stack_resources

Fixture: SAM template with Lambda, DynamoDB, SQS

Expected graph:
  - 1 Service node (Lambda)
  - 1 Database node (DynamoDB)
  - 1 Queue node (SQS)
  - All have deployment_method = "sam"
```

## Edge Cases

### Terraform Edge Cases

1. **No backend configuration**: terraform_workspace = None
2. **No tags**: deployment_method = "terraform" (default), environment = None
3. **Conflicting tags**: Environment="prod" but terraform:workspace="staging" → prefer terraform:workspace
4. **Mixed case tags**: ManagedBy vs managedBy vs managed_by → normalize to lowercase
5. **Backend without key**: Extract from workspace attribute if present

### CloudFormation Edge Cases

1. **Template without Parameters**: environment = None
2. **SAM template without Transform field**: Treat as CloudFormation
3. **Resource without explicit name**: Use logical ID (resource key)
4. **Intrinsic functions (!Ref, !Sub)**: Extract parameter references but don't evaluate
5. **Nested stacks**: Parse parent template only (don't follow TemplateURL)

### File Detection Edge Cases

1. **YAML file without AWSTemplateFormatVersion**: Skip (not a template)
2. **JSON CloudFormation template**: Parse using serde_json
3. **terraform.tfvars files**: Ignore (not resource definitions)
4. **terraform.tfstate files**: Ignore (state, not source)

## Acceptance Criteria

### Terraform Parser Enhancement

- [ ] Terraform resources include `deployment_method` attribute (always "terraform")
- [ ] Tags extracted correctly (case-insensitive key matching)
- [ ] Workspace extracted from backend s3 key path
- [ ] Environment extracted from common tag keys (Environment, Env, env)
- [ ] 6+ unit tests covering all extraction scenarios
- [ ] Edge cases handled gracefully (no panics)

### CloudFormation Parser

- [ ] SAM templates detected correctly (Transform field)
- [ ] AWS::Serverless::Function creates Service nodes
- [ ] AWS::DynamoDB::Table creates Database nodes
- [ ] AWS::SQS::Queue creates Queue nodes
- [ ] Parameters section parsed for environment
- [ ] Non-template YAML files ignored
- [ ] 7+ unit tests covering SAM, CloudFormation, edge cases
- [ ] Parser registered in ParserRegistry

### Integration

- [ ] Multi-IaC repos surveyed correctly (Terraform + SAM + CloudFormation)
- [ ] All resources have deployment_method attribute
- [ ] Environment attributes consistent across parsers
- [ ] 3+ integration tests with realistic fixtures

### Documentation

- [ ] Parser extension guide updated with IaC examples
- [ ] forge.yaml schema documented with deployment metadata
- [ ] Examples of extracted attributes in spec

## Future Enhancements (Post-V1)

1. **CDK Parser**: Parse TypeScript/Python CDK code for stack definitions
2. **Workspace variable extraction**: Parse `terraform.workspace` variable references
3. **Stack outputs**: Extract CloudFormation Outputs section for dependency mapping
4. **IAM policy analysis**: Infer permissions from IAM roles (future coupling detection)
5. **Backend variable interpolation**: Resolve workspace from Terraform variables

---

## Summary

Deployment metadata extraction transforms Forge from "what exists in code" to "how to deploy code." By parsing Terraform tags, CloudFormation parameters, and SAM templates, Forge provides LLM coding agents with the operational context they need to generate correct deployment commands.

**Key Insight**: Extract deployment context from **source-controlled IaC files**, not runtime AWS state. This is simpler, faster, cheaper, and provides the "source of truth" for deployments.

**Estimated Effort**: 3 days
- Day 1: Terraform parser enhancement + tests
- Day 2: CloudFormation parser + tests
- Day 3: Integration tests + documentation
