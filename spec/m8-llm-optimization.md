# Milestone 8: LLM Optimization - Specification

## Overview

Transform Forge from human-readable documentation to LLM-optimized context generation for coding agents. This milestone adds deployment metadata extraction, environment mapping, and actionable instruction generation to make Forge the perfect context provider for LLM coding agents building application and infrastructure code.

## Goals

1. **Extract deployment metadata** from Infrastructure as Code (IaC) source files
2. **Map repositories to environments** and AWS accounts
3. **Generate actionable LLM instructions** from business context, code patterns, and deployment metadata
4. **Optimize JSON output** for LLM consumption with structured, predictable schema

## Non-Goals

- Runtime AWS scanning (defer to future Cloud Mapper project)
- Human-facing output improvements (deprioritized)
- Real-time context updates (incremental survey already handles freshness)

## Context: Why LLM Optimization?

**Critical Insight**: LLM coding agents are the primary consumers of Forge output, not humans.

LLM agents need:
- ⭐⭐⭐⭐⭐ **Architecture context** - What services exist, how they connect
- ⭐⭐⭐⭐⭐ **Business intent** - WHY services exist, constraints, gotchas
- ⭐⭐⭐⭐⭐ **Deployment context** - HOW to deploy (Terraform/SAM/CDK commands)
- ⭐⭐⭐⭐ **Code patterns** - Framework conventions, testing requirements
- ⭐⭐⭐ **Freshness** - Is this context stale?
- ⭐⭐⭐ **Token budgeting** - Fit within context windows

Forge M1-M7 provides items 1, 2, 5, and 6. **M8 adds items 3 and 4.**

## Architecture Changes

### New Parsers

1. **CloudFormation/SAM Parser** (`forge-survey/src/parser/cloudformation.rs`)
   - Parses YAML/JSON CloudFormation templates
   - Extracts AWS::Serverless::* resources (SAM)
   - Extracts Parameters, Outputs, and stack metadata

2. **Enhanced Terraform Parser** (`forge-survey/src/parser/terraform.rs`)
   - Existing parser enhanced with tag extraction
   - Backend configuration parsing for workspace detection
   - Deployment method inference from tag patterns

### New Modules

1. **LLM Instruction Generator** (`forge-cli/src/llm_instructions.rs`)
   - Converts business context gotchas to DO NOT statements
   - Infers code style from language/framework attributes
   - Generates deployment commands from metadata
   - Extracts dependency descriptions with purpose context

2. **Enhanced Config Schema** (`forge-cli/src/config.rs`)
   - Environment definitions (dev/staging/prod)
   - Environment-to-repo mappings
   - AWS account ID mappings

### Enhanced Output

**JSON Serializer** (`forge-cli/src/serializers/json.rs`) now includes:
- `llm_instructions` field in node output
- Deployment metadata attributes (`deployment_method`, `stack_name`, `terraform_workspace`)
- Environment attributes (`environment`, `aws_account_id`)

## Task Breakdown

### M8-T1: Enhanced Terraform Parser

**Goal**: Extract deployment metadata from Terraform files to understand HOW resources are deployed.

**Files**: `forge-survey/src/parser/terraform.rs`

**Changes**:
1. Add `parse_tags()` method to extract tags from HCL resource blocks
2. Add `parse_backend()` method to extract workspace from backend config
3. Add `infer_deployment_method()` to detect deployment method from tags
4. Store metadata in node attributes: `deployment_method`, `terraform_workspace`, `environment`

**Implementation Details**:

```rust
impl TerraformParser {
    /// Extract tags from HCL resource block
    fn parse_tags(&self, block: &hcl::Block) -> HashMap<String, String> {
        // Look for 'tags' attribute in block
        // Return key-value pairs
    }

    /// Infer deployment method from tag patterns
    fn infer_deployment_method(&self, tags: &HashMap<String, String>) -> String {
        // Check for common patterns:
        // - "ManagedBy" = "Terraform" → "terraform"
        // - "aws:cloudformation:stack-name" → "cloudformation"
        // Default: "terraform"
    }

    /// Extract workspace from backend configuration
    fn parse_backend_workspace(&self, body: &hcl::Body) -> Option<String> {
        // Parse terraform { backend "s3" { key = "env/terraform.tfstate" } }
        // Extract "env" from key path
    }
}
```

**Attributes Added to Nodes**:
- `deployment_method`: "terraform" | "sam" | "cloudformation" | "unknown"
- `terraform_workspace`: workspace name from backend or tags
- `environment`: extracted from tags (Environment, Env, etc.)

**Test Scenarios**:
1. Parse resource with `ManagedBy = "Terraform"` tag → deployment_method = "terraform"
2. Parse resource without tags → deployment_method = "terraform" (default)
3. Extract workspace from backend s3 key `production/terraform.tfstate` → "production"
4. Extract environment from `Environment = "staging"` tag
5. Handle multiple tag formats (ManagedBy vs managed_by vs managedBy)
6. Handle resources without backend config (no workspace)

**Acceptance Criteria**:
- [ ] Terraform resources include `deployment_method` attribute
- [ ] Workspace extracted when backend config present
- [ ] Environment extracted from common tag keys
- [ ] 6+ unit tests covering all scenarios

---

### M8-T2: SAM/CloudFormation Parser

**Goal**: Parse SAM and CloudFormation templates to extract serverless resources and deployment metadata.

**Files**: `forge-survey/src/parser/cloudformation.rs` (NEW)

**Implementation**:

```rust
use serde_yaml;
use super::traits::{Parser, Discovery, ParserError};

pub struct CloudFormationParser {}

impl CloudFormationParser {
    pub fn new() -> Result<Self, ParserError> {
        Ok(Self {})
    }

    /// Determine if template is SAM or raw CloudFormation
    fn is_sam_template(&self, template: &serde_yaml::Value) -> bool {
        template.get("Transform")
            .and_then(|t| t.as_str())
            .map(|s| s.contains("AWS::Serverless"))
            .unwrap_or(false)
    }

    /// Parse AWS::Serverless::Function resources
    fn parse_sam_function(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery> {
        // Extract FunctionName, Runtime, Handler
        // Create Service discovery
    }

    /// Parse AWS::DynamoDB::Table resources
    fn parse_dynamodb_table(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery> {
        // Extract TableName
        // Create Database discovery
    }

    /// Parse AWS::SQS::Queue resources
    fn parse_sqs_queue(&self, name: &str, resource: &serde_yaml::Value) -> Option<Discovery> {
        // Extract QueueName
        // Create Queue discovery
    }

    /// Extract Parameters section
    fn parse_parameters(&self, template: &serde_yaml::Value) -> HashMap<String, String> {
        // Extract default values from Parameters
    }
}

impl Parser for CloudFormationParser {
    fn supported_extensions(&self) -> &[&str] {
        &["yaml", "yml", "json"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        // Only parse files named 'template.*' or containing 'AWSTemplateFormatVersion'

        let template: serde_yaml::Value = serde_yaml::from_str(content)?;

        // Verify it's a CloudFormation template
        if !template.get("AWSTemplateFormatVersion").is_some() {
            return Ok(Vec::new());
        }

        let mut discoveries = Vec::new();
        let is_sam = self.is_sam_template(&template);
        let resources = template.get("Resources")?;

        for (name, resource) in resources.as_mapping()? {
            let resource_type = resource.get("Type")?.as_str()?;

            match resource_type {
                "AWS::Serverless::Function" => {
                    if let Some(d) = self.parse_sam_function(name.as_str()?, resource) {
                        discoveries.push(d);
                    }
                }
                "AWS::DynamoDB::Table" => {
                    if let Some(d) = self.parse_dynamodb_table(name.as_str()?, resource) {
                        discoveries.push(d);
                    }
                }
                "AWS::SQS::Queue" => {
                    if let Some(d) = self.parse_sqs_queue(name.as_str()?, resource) {
                        discoveries.push(d);
                    }
                }
                _ => {}
            }
        }

        Ok(discoveries)
    }
}
```

**Attributes Added to Nodes**:
- `deployment_method`: "sam" | "cloudformation"
- `stack_name`: extracted from template metadata or filename
- `environment`: from Parameters section

**Test Scenarios**:
1. Parse SAM template with AWS::Serverless::Function → Service discovery
2. Parse SAM template with AWS::Serverless::Api → API discovery
3. Parse CloudFormation with AWS::DynamoDB::Table → Database discovery
4. Detect SAM vs CloudFormation from Transform field
5. Extract Parameters with default values
6. Handle malformed YAML gracefully
7. Ignore non-template YAML files

**Acceptance Criteria**:
- [ ] SAM templates parsed correctly
- [ ] CloudFormation templates parsed correctly
- [ ] Lambda functions extracted as Service nodes
- [ ] DynamoDB tables extracted as Database nodes
- [ ] SQS queues extracted as Queue nodes
- [ ] 7+ unit tests covering all scenarios
- [ ] Parser registered in ParserRegistry

---

### M8-T3: Environment and Account Mapping

**Goal**: Map repositories to environments (dev/staging/prod) and AWS accounts for deployment context.

**Files**:
- `forge-cli/src/config.rs` - Add Environment struct
- `forge-survey/src/graph_builder.rs` - Inject environment context
- `forge-cli/src/commands/map.rs` - Add --env filter

**Config Schema Extension**:

```yaml
# forge.yaml additions

# Environment definitions
environments:
  - name: production
    aws_account_id: "123456789012"
    repos:
      - "my-org/api-gateway"
      - "my-org/user-service"

  - name: staging
    aws_account_id: "987654321098"
    repos:
      - "my-org/*-staging"  # Glob pattern support

  - name: development
    aws_account_id: "555555555555"
    local_only: true  # Not deployed to AWS
```

**Implementation**:

```rust
// forge-cli/src/config.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub aws_account_id: Option<String>,
    pub repos: Vec<String>,  // Repo names or glob patterns
    pub local_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeConfig {
    pub repos: RepoConfig,
    pub github: Option<GitHubConfig>,
    pub output: OutputConfig,
    pub llm: Option<LLMConfig>,
    pub token_budget: Option<usize>,
    pub environments: Option<Vec<Environment>>,  // NEW
}

impl ForgeConfig {
    /// Resolve which environment a repo belongs to
    pub fn resolve_environment(&self, repo_name: &str) -> Option<&Environment> {
        if let Some(envs) = &self.environments {
            for env in envs {
                for pattern in &env.repos {
                    if glob_match(pattern, repo_name) {
                        return Some(env);
                    }
                }
            }
        }
        None
    }
}
```

```rust
// forge-survey/src/graph_builder.rs

impl GraphBuilder {
    /// Inject environment attributes into nodes
    pub fn set_environment(&mut self, env_name: &str, aws_account_id: Option<&str>) {
        self.current_environment = Some(env_name.to_string());
        self.current_aws_account = aws_account_id.map(|s| s.to_string());
    }

    // Modified build method to inject environment attributes
    fn build_internal(&mut self) -> ForgeGraph {
        // When creating nodes, add environment attributes
        if let Some(env) = &self.current_environment {
            node.attributes.insert("environment".to_string(), env.clone());
        }
        if let Some(account) = &self.current_aws_account {
            node.attributes.insert("aws_account_id".to_string(), account.clone());
        }
    }
}
```

```rust
// forge-cli/src/commands/map.rs

#[derive(Parser)]
pub struct MapArgs {
    // ... existing fields ...

    /// Filter to specific environment
    #[arg(long)]
    pub env: Option<String>,
}

pub async fn execute(args: MapArgs) -> Result<(), Box<dyn Error>> {
    // ... load graph ...

    // Filter by environment if specified
    if let Some(env_name) = args.env {
        subgraph = subgraph.filter_by_attribute("environment", &env_name);
    }

    // ... serialize ...
}
```

**Test Scenarios**:
1. Load config with environments → resolves repo to environment
2. Survey repo in "production" environment → nodes have environment="production"
3. Map with --env production → filters to production nodes only
4. Repo matches multiple environment patterns → first match wins
5. Repo matches no environments → no environment attribute (null)
6. Glob pattern matching for repo names

**Acceptance Criteria**:
- [ ] forge.yaml supports `environments` section
- [ ] Repos resolved to environments using glob patterns
- [ ] Survey injects environment and aws_account_id attributes
- [ ] Map command supports --env filter
- [ ] 6+ unit tests covering resolution and filtering

---

### M8-T4: LLM Instruction Generation

**Goal**: Generate actionable, LLM-consumable instructions from graph knowledge.

**Files**: `forge-cli/src/llm_instructions.rs` (NEW)

**Data Structures**:

```rust
use serde::{Serialize, Deserialize};
use forge_graph::{ForgeGraph, Node, NodeId};

/// Generated instructions for LLM agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInstructions {
    /// Code style guidelines inferred from language/framework
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_style: Option<String>,

    /// Testing requirements inferred from test frameworks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testing: Option<String>,

    /// Deployment commands generated from metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment: Option<String>,

    /// Critical DO NOT statements from business context gotchas
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gotchas: Vec<String>,

    /// Dependency context with purpose descriptions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<DependencyInstructions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInstructions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queues: Vec<String>,
}

pub struct InstructionGenerator<'a> {
    graph: &'a ForgeGraph,
}

impl<'a> InstructionGenerator<'a> {
    pub fn new(graph: &'a ForgeGraph) -> Self {
        Self { graph }
    }

    /// Generate instructions for a node
    pub fn generate(&self, node_id: &NodeId) -> Result<LlmInstructions, InstructionError> {
        let node = self.graph.get_node(node_id)?;

        Ok(LlmInstructions {
            code_style: self.infer_code_style(node),
            testing: self.infer_testing_requirements(node),
            deployment: self.generate_deployment_command(node),
            gotchas: self.convert_gotchas(node),
            dependencies: self.generate_dependency_instructions(node_id),
        })
    }

    /// Infer code style from language/framework
    fn infer_code_style(&self, node: &Node) -> Option<String> {
        let language = node.attributes.get("language")?;
        let framework = node.attributes.get("framework");

        match (language.as_str(), framework.as_deref()) {
            ("python", Some("fastapi")) => {
                Some("FastAPI framework. Use Pydantic models for validation. Type hints required. Use async/await patterns.".to_string())
            }
            ("python", Some("flask")) => {
                Some("Flask framework. Use blueprints for routing. Type hints recommended.".to_string())
            }
            ("python", Some("django")) => {
                Some("Django framework. Follow MTV pattern. Use Django ORM.".to_string())
            }
            ("python", _) => {
                Some("Python. Use type hints. Follow PEP 8 style guide.".to_string())
            }
            ("typescript", Some("express")) => {
                Some("Express.js with TypeScript. Use middleware pattern. Strong typing. Async error handling.".to_string())
            }
            ("javascript", Some("express")) => {
                Some("Express.js. Use middleware pattern. Handle async errors with try/catch.".to_string())
            }
            ("typescript", Some("nestjs")) => {
                Some("NestJS framework. Use decorators. Dependency injection. DTOs for validation.".to_string())
            }
            _ => None,
        }
    }

    /// Infer testing requirements from framework
    fn infer_testing_requirements(&self, node: &Node) -> Option<String> {
        let test_framework = node.attributes.get("test_framework")?;

        match test_framework.as_str() {
            "pytest" => Some("pytest with >80% coverage. Use fixtures for setup. Mock external dependencies with pytest-mock.".to_string()),
            "jest" => Some("Jest tests. Use describe/it blocks. Mock with jest.mock(). Aim for >80% coverage.".to_string()),
            "mocha" => Some("Mocha tests with Chai assertions. Use describe/it structure.".to_string()),
            _ => None,
        }
    }

    /// Generate deployment command from metadata
    fn generate_deployment_command(&self, node: &Node) -> Option<String> {
        let deployment_method = node.attributes.get("deployment_method")?;

        match deployment_method.as_str() {
            "terraform" => {
                let workspace = node.attributes.get("terraform_workspace").map(|s| s.as_str()).unwrap_or("default");
                Some(format!("cd terraform/{{service}} && terraform plan -var-file={}.tfvars && terraform apply -var-file={}.tfvars", workspace, workspace))
            }
            "sam" => {
                let stack_name = node.attributes.get("stack_name").map(|s| s.as_str()).unwrap_or("{{service}}-stack");
                Some(format!("sam build && sam deploy --stack-name {}", stack_name))
            }
            "cloudformation" => {
                let stack_name = node.attributes.get("stack_name").map(|s| s.as_str()).unwrap_or("{{service}}-stack");
                Some(format!("aws cloudformation deploy --template-file template.yaml --stack-name {}", stack_name))
            }
            _ => None,
        }
    }

    /// Convert business context gotchas to DO NOT statements
    fn convert_gotchas(&self, node: &Node) -> Vec<String> {
        node.business_context
            .as_ref()
            .map(|bc| {
                bc.gotchas.iter().map(|gotcha| {
                    format!("DO NOT violate: {}", gotcha)
                }).collect()
            })
            .unwrap_or_default()
    }

    /// Generate dependency instructions with context
    fn generate_dependency_instructions(&self, node_id: &NodeId) -> Option<DependencyInstructions> {
        let mut services = Vec::new();
        let mut databases = Vec::new();
        let mut queues = Vec::new();

        // Find CALLS edges
        for edge in self.graph.get_outgoing_edges(node_id, Some(EdgeType::Calls)) {
            let target = self.graph.get_node(&edge.target).ok()?;
            let purpose = target.business_context.as_ref()
                .and_then(|bc| bc.purpose.clone())
                .unwrap_or_else(|| "purpose unknown".to_string());
            services.push(format!("{} ({})", target.display_name, purpose));
        }

        // Find READS/WRITES edges
        for edge in self.graph.get_outgoing_edges(node_id, Some(EdgeType::Reads)) {
            let target = self.graph.get_node(&edge.target).ok()?;
            databases.push(format!("{} (primary data store)", target.display_name));
        }

        // Find PUBLISHES edges
        for edge in self.graph.get_outgoing_edges(node_id, Some(EdgeType::Publishes)) {
            let target = self.graph.get_node(&edge.target).ok()?;
            queues.push(format!("{} (publish on events)", target.display_name));
        }

        Some(DependencyInstructions { services, databases, queues })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstructionError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Graph error: {0}")]
    GraphError(String),
}
```

**Test Scenarios**:
1. Infer Python FastAPI style → includes "Pydantic", "async/await"
2. Infer TypeScript Express style → includes "middleware", "async error handling"
3. Generate pytest testing requirements → includes "fixtures", "mock"
4. Generate Terraform deployment command with workspace → includes var-file
5. Generate SAM deployment command → includes sam build
6. Convert gotcha "Rate limit is 1000 req/sec" → "DO NOT violate: Rate limit is 1000 req/sec"
7. Generate dependency instructions with purpose context
8. Handle node without business context → minimal instructions
9. Handle node without deployment metadata → no deployment command

**Acceptance Criteria**:
- [ ] Code style inferred for Python, TypeScript, JavaScript
- [ ] Testing requirements inferred from test frameworks
- [ ] Deployment commands generated for Terraform, SAM, CloudFormation
- [ ] Gotchas converted to DO NOT statements
- [ ] Dependency instructions include purpose context
- [ ] 15+ unit tests covering all inference rules
- [ ] Handles missing data gracefully (returns None/empty)

---

### M8-T5: Enhanced JSON Output

**Goal**: Add llm_instructions field to JSON serializer output.

**Files**: `forge-cli/src/serializers/json.rs`

**Changes**:

```rust
use crate::llm_instructions::{InstructionGenerator, LlmInstructions};

impl JsonSerializer {
    pub fn serialize_graph(&self, graph: &ForgeGraph) -> Result<String, SerializerError> {
        // ... existing code ...

        // Generate LLM instructions for each node
        let instruction_gen = InstructionGenerator::new(graph);

        let nodes_with_instructions: Vec<_> = graph.nodes().map(|node| {
            let instructions = instruction_gen.generate(&node.id).ok();

            json!({
                "id": node.id.to_string(),
                "type": format!("{:?}", node.node_type),
                "display_name": node.display_name,
                "relevance_score": 1.0,
                "attributes": node.attributes,
                "business_context": node.business_context,
                "llm_instructions": instructions,  // NEW
                "metadata": {
                    "created_at": node.created_at,
                    "updated_at": node.updated_at,
                    // ...
                }
            })
        }).collect();

        // ... rest of serialization ...
    }
}
```

**Output Example**:

```json
{
  "$schema": "https://forge.io/schemas/graph-v1.json",
  "version": "1.0.0",
  "generated_at": "2026-01-25T15:30:00Z",
  "nodes": [
    {
      "id": "service:my-org:user-api",
      "type": "Service",
      "display_name": "user-api",
      "relevance_score": 1.0,
      "attributes": {
        "language": "python",
        "framework": "fastapi",
        "deployment_method": "terraform",
        "terraform_workspace": "production",
        "environment": "production",
        "aws_account_id": "123456789012"
      },
      "business_context": {
        "purpose": "Handles user CRUD operations and authentication",
        "owner": "Platform Team",
        "gotchas": [
          "Rate limit is 1000 requests/second",
          "Email validation required before DB writes"
        ]
      },
      "llm_instructions": {
        "code_style": "FastAPI framework. Use Pydantic models for validation. Type hints required. Use async/await patterns.",
        "testing": "pytest with >80% coverage. Use fixtures for setup. Mock external dependencies with pytest-mock.",
        "deployment": "cd terraform/user-api && terraform plan -var-file=production.tfvars && terraform apply -var-file=production.tfvars",
        "gotchas": [
          "DO NOT violate: Rate limit is 1000 requests/second",
          "DO NOT violate: Email validation required before DB writes"
        ],
        "dependencies": {
          "services": ["auth-service (token validation)"],
          "databases": ["users-table (primary data store)"],
          "queues": ["user-events-queue (publish on events)"]
        }
      }
    }
  ]
}
```

**Test Scenarios**:
1. Serialize graph with LLM instructions → JSON includes llm_instructions field
2. Serialize node without business context → llm_instructions present but minimal
3. Serialize node without deployment metadata → deployment field is null
4. Validate JSON schema compliance
5. Ensure token budgeting still works with new field
6. Test subgraph serialization with instructions

**Acceptance Criteria**:
- [ ] JSON output includes llm_instructions for all nodes
- [ ] Instructions follow schema (code_style, testing, deployment, gotchas, dependencies)
- [ ] Null/empty fields are omitted (skip_serializing_if)
- [ ] Existing JSON tests still pass
- [ ] 6+ new tests for instruction serialization

---

### M8-T6: Integration Tests

**Goal**: End-to-end testing of LLM-optimized output.

**Files**: `forge-survey/tests/integration_llm.rs` (NEW)

**Test Cases**:

1. **test_survey_with_terraform_metadata**
   - Create fixture with Terraform Lambda, DynamoDB, SQS
   - Survey repo
   - Verify deployment_method="terraform" on nodes
   - Verify workspace extracted from backend

2. **test_survey_with_sam_template**
   - Create fixture with SAM template.yaml
   - Survey repo
   - Verify deployment_method="sam" on Lambda nodes
   - Verify stack_name extracted

3. **test_environment_mapping**
   - Create forge.yaml with environments
   - Survey repos in different environments
   - Verify environment and aws_account_id attributes
   - Test map --env filter

4. **test_llm_instructions_generation**
   - Create fixture with Python FastAPI service
   - Add business context with gotchas
   - Survey and map to JSON
   - Verify llm_instructions field populated
   - Verify code_style includes "FastAPI"
   - Verify gotchas converted to DO NOT statements
   - Verify deployment command includes terraform

5. **test_mixed_iac_deployment_metadata**
   - Create repo with Terraform + SAM templates
   - Survey repo
   - Verify different deployment methods on different resources

6. **test_llm_json_output_complete**
   - Full end-to-end: survey → map --format json
   - Verify JSON includes all LLM-optimized fields
   - Verify token budgeting respects llm_instructions size

**Acceptance Criteria**:
- [ ] 6+ integration tests all passing
- [ ] Tests cover Terraform, SAM, and environment mapping
- [ ] Tests verify LLM instructions in JSON output
- [ ] Tests use realistic fixture repos
- [ ] All workspace tests pass (>540 tests)

---

## Dependencies

```toml
# forge-survey/Cargo.toml
[dependencies]
serde_yaml = "0.9"  # Already present, used for SAM/CloudFormation

# forge-cli/Cargo.toml
[dependencies]
# No new dependencies needed
```

## Testing Strategy

### Unit Tests

Each component must have comprehensive unit tests:

- **TerraformParser enhancement**: 6+ tests for tag extraction, backend parsing
- **CloudFormationParser**: 7+ tests for SAM/CF resource extraction
- **Environment mapping**: 6+ tests for resolution and filtering
- **InstructionGenerator**: 15+ tests for all inference rules
- **JSON serializer**: 6+ tests for llm_instructions serialization

**Target**: >80% line coverage on new code

### Integration Tests

- **integration_llm.rs**: 6+ end-to-end tests for LLM-optimized workflow
- Test realistic scenarios: multi-language repos with IaC
- Verify complete JSON output with all LLM fields

## Acceptance Criteria

- [ ] **Terraform resources** include `deployment_method`, `terraform_workspace` attributes
- [ ] **SAM templates** parsed and Lambda/DynamoDB/SQS extracted correctly
- [ ] **forge.yaml** supports `environments` section with repo mappings
- [ ] **forge map --env production** filters to production environment nodes
- [ ] **JSON output** includes `llm_instructions` field with all sections
- [ ] **LLM instructions** include code_style, testing, deployment, gotchas, dependencies
- [ ] **All tests pass** (>540 tests total, +40 new tests from M8)
- [ ] **Clippy clean** with no warnings
- [ ] **Documentation updated** in relevant spec files

## Success Metrics

An LLM coding agent using Forge is successful when it can:

1. Query "What does service X do?" → Get purpose, constraints, gotchas
2. Query "How do I deploy service X?" → Get Terraform/SAM commands
3. Query "What are the dependencies of service X?" → Get full dependency graph with context
4. Receive actionable DO/DON'T instructions automatically
5. Know which environment/account to target
6. Get framework-specific coding guidance (FastAPI, Express, etc.)
7. Understand testing requirements for the service

**The LLM should have everything it needs to write great code without asking the human for clarification.**

## Open Questions

1. **Tag format standardization**: Should we enforce specific tag formats or detect common variations?
   - **Decision**: Detect common variations (ManagedBy vs managed_by vs managedBy)

2. **SAM vs CloudFormation detection**: Should we treat them differently?
   - **Decision**: Detect Transform field - SAM is superset of CloudFormation

3. **Workspace extraction heuristics**: What if backend config doesn't have clear workspace?
   - **Decision**: Try key path first, fall back to terraform.workspace variable, default to "default"

4. **Environment conflict resolution**: What if repo matches multiple environments?
   - **Decision**: First match wins (order in forge.yaml matters)

5. **Instruction verbosity**: How detailed should generated instructions be?
   - **Decision**: Keep concise (1-2 sentences per section), LLMs prefer density

## Future Enhancements (Post-V1)

- **CDK Parser**: Parse TypeScript/Python CDK code for stack definitions
- **Runtime state integration**: Future Cloud Mapper integration for drift detection
- **Custom instruction templates**: User-defined instruction generation rules
- **Multi-cloud support**: Azure ARM templates, GCP Deployment Manager
- **Instruction refinement**: LLM-powered instruction quality improvement

---

## Conclusion

Milestone 8 transforms Forge from a documentation tool into the definitive context provider for LLM coding agents. By extracting deployment metadata from source code and generating actionable instructions, Forge gives LLMs everything they need to write, test, and deploy code confidently.

**Estimated effort**: 2 weeks (1 week implementation, 1 week testing and polish)

**Priority**: HIGH - This is the differentiator that makes Forge invaluable for LLM-assisted development.
