# LLM Instruction Generation - Specification

## Purpose

Generate actionable, LLM-consumable instructions from Forge's knowledge graph to help coding agents write better code. Transform graph knowledge (business context, code patterns, deployment metadata, dependencies) into explicit DO/DON'T guidelines, coding style rules, testing requirements, and deployment commands.

## Overview

**The Problem**: LLMs need explicit, actionable instructions - not just data.

**Current State** (M1-M7): Forge provides rich data:
- Business context (purpose, gotchas, owner)
- Code patterns (language, framework, dependencies)
- Deployment metadata (Terraform workspace, SAM stack)
- Dependency graph (calls, reads, writes)

**M8 Addition**: Transform this data into LLM-friendly instructions:
- "Use Pydantic models for validation" (from language=python, framework=fastapi)
- "DO NOT exceed 1000 req/sec rate limit" (from gotcha)
- "cd terraform/service && terraform apply" (from deployment_method=terraform)
- "Must call auth-service before user operations" (from dependency graph)

## Instruction Categories

### 1. Code Style

**Source**: Infer from language + framework + detected patterns

**Purpose**: Help LLM write code that matches existing codebase conventions

**Examples**:

| Language | Framework | Generated Instruction |
|----------|-----------|----------------------|
| python | fastapi | "FastAPI framework. Use Pydantic models for validation. Type hints required. Use async/await patterns." |
| python | flask | "Flask framework. Use blueprints for routing. Type hints recommended." |
| python | django | "Django framework. Follow MTV pattern. Use Django ORM. Don't bypass model validation." |
| python | (none) | "Python. Use type hints. Follow PEP 8 style guide." |
| typescript | express | "Express.js with TypeScript. Use middleware pattern. Strong typing. Async error handling." |
| javascript | express | "Express.js. Use middleware pattern. Handle async errors with try/catch." |
| typescript | nestjs | "NestJS framework. Use decorators. Dependency injection. DTOs for validation." |
| javascript | react | "React. Use functional components with hooks. PropTypes or TypeScript for types." |

**Detection**:
- Language: from node attributes (`language: "python"`)
- Framework: from node attributes (`framework: "fastapi"`) or detected in parser
- Pattern detection: Can be enhanced in future (detect decorators, async/await usage)

### 2. Testing

**Source**: Detect test frameworks from dependencies/file patterns

**Purpose**: Guide LLM on testing approach and coverage requirements

**Examples**:

| Test Framework | Generated Instruction |
|----------------|----------------------|
| pytest | "pytest with >80% coverage. Use fixtures for setup. Mock external dependencies with pytest-mock." |
| jest | "Jest tests. Use describe/it blocks. Mock with jest.mock(). Aim for >80% coverage." |
| mocha | "Mocha tests with Chai assertions. Use describe/it structure." |
| unittest | "Python unittest. Use setUp/tearDown. Mock with unittest.mock." |
| vitest | "Vitest tests. Fast unit tests. Use vi.mock() for mocking." |

**Detection**:
- From dependencies in package.json (jest, mocha, vitest)
- From dependencies in requirements.txt / pyproject.toml (pytest, unittest)
- Store in node attributes: `test_framework: "pytest"`

**Future Enhancement**: Detect coverage requirements from CI config or coverage.yml

### 3. Deployment

**Source**: Deployment metadata (deployment_method, workspace, stack_name)

**Purpose**: Provide exact deployment commands for LLM to reference or execute

**Examples**:

| Deployment Method | Workspace/Stack | Generated Instruction |
|-------------------|----------------|----------------------|
| terraform | production | `cd terraform/{service} && terraform plan -var-file=production.tfvars && terraform apply -var-file=production.tfvars` |
| terraform | staging | `cd terraform/{service} && terraform plan -var-file=staging.tfvars && terraform apply -var-file=staging.tfvars` |
| sam | user-api-stack | `sam build && sam deploy --stack-name user-api-stack` |
| cloudformation | infra-stack | `aws cloudformation deploy --template-file template.yaml --stack-name infra-stack` |
| (unknown) | - | "Deployment method unknown. Check with team for deployment process." |

**Template Variables**:
- `{service}`: Replace with node display_name
- `{workspace}`: Replace with terraform_workspace or environment
- `{stack}`: Replace with stack_name

**Command Structure**:
1. **Terraform**: Include workspace-specific var file
2. **SAM**: Include stack name and optional parameters
3. **CloudFormation**: Include template file and stack name
4. **Unknown**: Provide helpful fallback message

### 4. Gotchas (Critical)

**Source**: Business context gotchas + dependency constraints

**Purpose**: Convert human-written warnings into explicit DO NOT statements

**Transformation Rules**:

| Input Gotcha | Output Instruction |
|--------------|-------------------|
| "Rate limit is 1000 req/sec" | "DO NOT exceed 1000 requests/second" |
| "Email validation required" | "MUST validate email format before database writes" |
| "Cache invalidation on update" | "DO NOT forget to invalidate cache after updates" |
| "Transactions must be idempotent" | "MUST ensure all transactions are idempotent (safe to retry)" |
| "No direct DB access" | "DO NOT bypass service layer - always use API" |

**Transformation Logic**:
1. If gotcha contains "must", "required", "always": → "MUST {action}"
2. If gotcha contains "don't", "never", "avoid": → "DO NOT {action}"
3. If gotcha is informational: → "DO NOT violate: {gotcha}"
4. Preserve specifics (numbers, limits, constraints)

**Priority**: CRITICAL - These prevent bugs and outages

### 5. Dependencies

**Source**: Graph edges (CALLS, READS, WRITES, PUBLISHES, SUBSCRIBES)

**Purpose**: Explain WHY dependencies exist and HOW to use them

**Examples**:

**Services (CALLS edges)**:
- "Must call auth-service for token validation before user operations"
- "Calls payment-service (handles payment processing)"
- "Depends on email-service (sends notification emails)"

**Databases (READS/WRITES edges)**:
- "users-table (primary data store)"
- "sessions-cache (Redis - stores session tokens)"
- "orders-table (read-only access - owned by order-service)"

**Queues (PUBLISHES/SUBSCRIBES edges)**:
- "user-events-queue (publish on create/update/delete)"
- "order-notifications-queue (subscribe for order updates)"

**Extraction Logic**:
```
For each CALLS edge:
  target_service = edge.target
  purpose = target.business_context.purpose OR "purpose unknown"
  instruction = f"{target.display_name} ({purpose})"

For each READS/WRITES edge:
  target_db = edge.target
  operation = "primary data store" if WRITES else "read-only access"
  owner = infer_owner(target_db) OR "ownership unclear"
  instruction = f"{target.display_name} ({operation})"

For each PUBLISHES edge:
  target_queue = edge.target
  instruction = f"{target.display_name} (publish on events)"

For each SUBSCRIBES edge:
  target_queue = edge.target
  instruction = f"{target.display_name} (subscribe for updates)"
```

## Implementation

### Data Structures

**File**: `forge-cli/src/llm_instructions.rs` (NEW)

```rust
use serde::{Serialize, Deserialize};

/// Generated instructions for LLM agents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyInstructions {
    /// Services this node calls (with purpose context)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,

    /// Databases this node reads/writes (with ownership context)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<String>,

    /// Queues this node publishes/subscribes (with event context)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queues: Vec<String>,
}

pub struct InstructionGenerator<'a> {
    graph: &'a ForgeGraph,
}

#[derive(Debug, thiserror::Error)]
pub enum InstructionError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Graph error: {0}")]
    GraphError(String),
}
```

### API

**Main Method**:

```rust
impl<'a> InstructionGenerator<'a> {
    pub fn new(graph: &'a ForgeGraph) -> Self

    /// Generate instructions for a single node
    pub fn generate(&self, node_id: &NodeId) -> Result<LlmInstructions, InstructionError>
}
```

**Private Inference Methods**:

```rust
impl<'a> InstructionGenerator<'a> {
    /// Infer code style from language/framework attributes
    fn infer_code_style(&self, node: &Node) -> Option<String>

    /// Infer testing requirements from detected test frameworks
    fn infer_testing_requirements(&self, node: &Node) -> Option<String>

    /// Generate deployment command from deployment metadata
    fn generate_deployment_command(&self, node: &Node) -> Option<String>

    /// Convert business context gotchas to DO NOT statements
    fn convert_gotchas(&self, node: &Node) -> Vec<String>

    /// Generate dependency descriptions with context
    fn generate_dependency_instructions(&self, node_id: &NodeId) -> Option<DependencyInstructions>

    /// Helper: Normalize gotcha to instruction format
    fn normalize_gotcha(&self, gotcha: &str) -> String

    /// Helper: Extract purpose from target node
    fn extract_purpose(&self, node_id: &NodeId) -> String

    /// Helper: Replace template variables in deployment command
    fn replace_template_vars(&self, template: &str, node: &Node) -> String
}
```

## Inference Rules

### Code Style Inference

**Decision Tree**:

```
language = node.attributes["language"]
framework = node.attributes["framework"]

match (language, framework):
    ("python", Some("fastapi")) → FastAPI instruction
    ("python", Some("flask")) → Flask instruction
    ("python", Some("django")) → Django instruction
    ("python", Some("chalice")) → Chalice instruction
    ("python", None) → Generic Python instruction
    ("typescript", Some("express")) → Express + TS instruction
    ("javascript", Some("express")) → Express instruction
    ("typescript", Some("nestjs")) → NestJS instruction
    ("typescript", Some("react")) → React + TS instruction
    ("javascript", Some("react")) → React instruction
    (_, _) → None
```

**Instruction Templates** (defined as constants):

```rust
const PYTHON_FASTAPI: &str = "FastAPI framework. Use Pydantic models for validation. Type hints required. Use async/await patterns.";
const PYTHON_FLASK: &str = "Flask framework. Use blueprints for routing. Type hints recommended.";
const PYTHON_DJANGO: &str = "Django framework. Follow MTV pattern. Use Django ORM. Don't bypass model validation.";
const PYTHON_GENERIC: &str = "Python. Use type hints. Follow PEP 8 style guide.";
const TS_EXPRESS: &str = "Express.js with TypeScript. Use middleware pattern. Strong typing. Async error handling.";
const JS_EXPRESS: &str = "Express.js. Use middleware pattern. Handle async errors with try/catch.";
const TS_NESTJS: &str = "NestJS framework. Use decorators. Dependency injection. DTOs for validation.";
```

### Testing Inference

**Decision Tree**:

```
test_framework = node.attributes["test_framework"]

match test_framework:
    Some("pytest") → Pytest instruction
    Some("jest") → Jest instruction
    Some("mocha") → Mocha instruction
    Some("vitest") → Vitest instruction
    Some("unittest") → unittest instruction
    None → None
```

**Instruction Templates**:

```rust
const PYTEST: &str = "pytest with >80% coverage. Use fixtures for setup. Mock external dependencies with pytest-mock.";
const JEST: &str = "Jest tests. Use describe/it blocks. Mock with jest.mock(). Aim for >80% coverage.";
const MOCHA: &str = "Mocha tests with Chai assertions. Use describe/it structure.";
```

### Deployment Command Generation

**Logic**:

```
deployment_method = node.attributes["deployment_method"]

match deployment_method:
    Some("terraform") → generate_terraform_command()
    Some("sam") → generate_sam_command()
    Some("cloudformation") → generate_cloudformation_command()
    None → None

generate_terraform_command():
    workspace = node.attributes["terraform_workspace"] OR "default"
    template = "cd terraform/{service} && terraform plan -var-file={workspace}.tfvars && terraform apply -var-file={workspace}.tfvars"
    replace_vars(template, node)

generate_sam_command():
    stack_name = node.attributes["stack_name"] OR "{service}-stack"
    template = "sam build && sam deploy --stack-name {stack_name}"
    replace_vars(template, node)

replace_vars(template, node):
    template.replace("{service}", node.display_name)
    template.replace("{workspace}", workspace)
    template.replace("{stack_name}", stack_name)
```

### Gotcha Transformation

**Normalization Rules**:

```
fn normalize_gotcha(gotcha: &str) -> String:
    lower = gotcha.to_lowercase()

    if "must" in lower or "required" in lower or "always" in lower:
        # Transform to MUST statement
        return extract_action_and_format_as_must(gotcha)

    if "don't" in lower or "never" in lower or "avoid" in lower:
        # Transform to DO NOT statement
        return extract_action_and_format_as_do_not(gotcha)

    # Default: prefix with "DO NOT violate:"
    return f"DO NOT violate: {gotcha}"
```

**Examples**:

| Input | Output |
|-------|--------|
| "Rate limit is 1000 req/sec" | "DO NOT violate: Rate limit is 1000 req/sec" |
| "Email validation required before writes" | "MUST validate email before database writes" |
| "Never bypass the cache layer" | "DO NOT bypass the cache layer" |
| "Always use transactions for multi-table updates" | "MUST use transactions for multi-table updates" |

### Dependency Instruction Generation

**Algorithm**:

```
fn generate_dependency_instructions(node_id: &NodeId) -> DependencyInstructions:
    services = []
    databases = []
    queues = []

    # Service dependencies (CALLS edges)
    for edge in graph.get_outgoing_edges(node_id, EdgeType::Calls):
        target = graph.get_node(edge.target)
        purpose = target.business_context?.purpose OR "purpose unknown"
        services.push(f"{target.display_name} ({purpose})")

    # Database dependencies (READS/WRITES edges)
    for edge in graph.get_outgoing_edges(node_id, [EdgeType::Reads, EdgeType::Writes]):
        target = graph.get_node(edge.target)
        access_type = if edge.type == Writes { "primary data store" } else { "read access" }
        databases.push(f"{target.display_name} ({access_type})")

    # Queue dependencies (PUBLISHES/SUBSCRIBES edges)
    for edge in graph.get_outgoing_edges(node_id, EdgeType::Publishes):
        target = graph.get_node(edge.target)
        queues.push(f"{target.display_name} (publish on events)")

    for edge in graph.get_outgoing_edges(node_id, EdgeType::Subscribes):
        target = graph.get_node(edge.target)
        queues.push(f"{target.display_name} (consume messages)")

    return DependencyInstructions { services, databases, queues }
```

## Test Scenarios

### Unit Tests

**Code Style Inference**:

```rust
#[test]
fn test_infer_python_fastapi_style() {
    let node = create_node_with_attrs([
        ("language", "python"),
        ("framework", "fastapi"),
    ]);

    let style = generator.infer_code_style(&node).unwrap();

    assert!(style.contains("FastAPI"));
    assert!(style.contains("Pydantic"));
    assert!(style.contains("async/await"));
}

#[test]
fn test_infer_typescript_express_style() {
    let node = create_node_with_attrs([
        ("language", "typescript"),
        ("framework", "express"),
    ]);

    let style = generator.infer_code_style(&node).unwrap();

    assert!(style.contains("Express.js"));
    assert!(style.contains("TypeScript"));
    assert!(style.contains("middleware"));
}

#[test]
fn test_infer_style_unknown_framework() {
    let node = create_node_with_attrs([
        ("language", "rust"),
    ]);

    let style = generator.infer_code_style(&node);

    assert!(style.is_none());
}
```

**Testing Inference**:

```rust
#[test]
fn test_infer_pytest_requirements() {
    let node = create_node_with_attrs([
        ("test_framework", "pytest"),
    ]);

    let testing = generator.infer_testing_requirements(&node).unwrap();

    assert!(testing.contains("pytest"));
    assert!(testing.contains("fixtures"));
    assert!(testing.contains("coverage"));
}

#[test]
fn test_infer_jest_requirements() {
    let node = create_node_with_attrs([
        ("test_framework", "jest"),
    ]);

    let testing = generator.infer_testing_requirements(&node).unwrap();

    assert!(testing.contains("Jest"));
    assert!(testing.contains("describe/it"));
    assert!(testing.contains("mock"));
}
```

**Deployment Command Generation**:

```rust
#[test]
fn test_generate_terraform_deployment() {
    let node = create_node_with_attrs([
        ("deployment_method", "terraform"),
        ("terraform_workspace", "production"),
        ("display_name", "user-api"),
    ]);

    let deployment = generator.generate_deployment_command(&node).unwrap();

    assert!(deployment.contains("terraform apply"));
    assert!(deployment.contains("production.tfvars"));
    assert!(deployment.contains("terraform/"));
}

#[test]
fn test_generate_sam_deployment() {
    let node = create_node_with_attrs([
        ("deployment_method", "sam"),
        ("stack_name", "user-api-stack"),
    ]);

    let deployment = generator.generate_deployment_command(&node).unwrap();

    assert!(deployment.contains("sam build"));
    assert!(deployment.contains("sam deploy"));
    assert!(deployment.contains("user-api-stack"));
}

#[test]
fn test_generate_deployment_no_metadata() {
    let node = create_node_with_attrs([]);

    let deployment = generator.generate_deployment_command(&node);

    assert!(deployment.is_none());
}
```

**Gotcha Transformation**:

```rust
#[test]
fn test_convert_gotchas_to_do_not() {
    let mut node = create_test_node();
    node.business_context = Some(BusinessContext {
        gotchas: vec![
            "Rate limit is 1000 req/sec".to_string(),
            "Email validation required".to_string(),
        ],
        ..Default::default()
    });

    let gotchas = generator.convert_gotchas(&node);

    assert_eq!(gotchas.len(), 2);
    assert!(gotchas[0].starts_with("DO NOT"));
    assert!(gotchas[1].starts_with("MUST") || gotchas[1].starts_with("DO NOT"));
}

#[test]
fn test_convert_gotcha_with_must() {
    let gotcha = "Email validation is required before writes";
    let normalized = generator.normalize_gotcha(gotcha);

    assert!(normalized.starts_with("MUST"));
    assert!(normalized.contains("validation"));
}

#[test]
fn test_convert_gotcha_with_never() {
    let gotcha = "Never bypass the cache layer";
    let normalized = generator.normalize_gotcha(gotcha);

    assert!(normalized.starts_with("DO NOT"));
    assert!(normalized.contains("bypass"));
    assert!(normalized.contains("cache"));
}
```

**Dependency Instructions**:

```rust
#[test]
fn test_generate_service_dependencies() {
    // Setup graph with:
    // api-service CALLS auth-service
    // auth-service has purpose = "Token validation"

    let deps = generator.generate_dependency_instructions(&api_id).unwrap();

    assert_eq!(deps.services.len(), 1);
    assert!(deps.services[0].contains("auth-service"));
    assert!(deps.services[0].contains("Token validation"));
}

#[test]
fn test_generate_database_dependencies() {
    // Setup graph with:
    // api-service READS users-table
    // api-service WRITES sessions-cache

    let deps = generator.generate_dependency_instructions(&api_id).unwrap();

    assert_eq!(deps.databases.len(), 2);
    assert!(deps.databases.iter().any(|d| d.contains("users-table")));
    assert!(deps.databases.iter().any(|d| d.contains("sessions-cache")));
}

#[test]
fn test_generate_queue_dependencies() {
    // Setup graph with:
    // api-service PUBLISHES user-events-queue

    let deps = generator.generate_dependency_instructions(&api_id).unwrap();

    assert_eq!(deps.queues.len(), 1);
    assert!(deps.queues[0].contains("user-events-queue"));
    assert!(deps.queues[0].contains("publish"));
}

#[test]
fn test_generate_dependencies_no_edges() {
    // Node with no outgoing edges

    let deps = generator.generate_dependency_instructions(&isolated_node_id);

    assert!(deps.is_none() || deps.services.is_empty());
}
```

### Integration Tests

**Full Instruction Generation**:

```rust
#[test]
fn test_full_llm_instruction_generation() {
    // Create fixture:
    // - Python FastAPI service
    // - Terraform deployment (production workspace)
    // - Business context with gotchas
    // - Dependencies: auth-service, users-table, events-queue

    let graph = create_fixture_graph();
    let generator = InstructionGenerator::new(&graph);
    let api_node_id = graph.get_node_by_name("user-api").unwrap().id;

    let instructions = generator.generate(&api_node_id).unwrap();

    // Verify code style
    assert!(instructions.code_style.is_some());
    assert!(instructions.code_style.unwrap().contains("FastAPI"));

    // Verify testing
    assert!(instructions.testing.is_some());
    assert!(instructions.testing.unwrap().contains("pytest"));

    // Verify deployment
    assert!(instructions.deployment.is_some());
    assert!(instructions.deployment.unwrap().contains("terraform apply"));
    assert!(instructions.deployment.unwrap().contains("production.tfvars"));

    // Verify gotchas
    assert!(!instructions.gotchas.is_empty());
    assert!(instructions.gotchas.iter().all(|g| g.starts_with("DO NOT") || g.starts_with("MUST")));

    // Verify dependencies
    assert!(instructions.dependencies.is_some());
    let deps = instructions.dependencies.unwrap();
    assert!(!deps.services.is_empty());
    assert!(!deps.databases.is_empty());
    assert!(!deps.queues.is_empty());
}
```

**Minimal Instructions (No Metadata)**:

```rust
#[test]
fn test_minimal_instructions_no_business_context() {
    // Node with only basic attributes (language, no framework)

    let instructions = generator.generate(&basic_node_id).unwrap();

    // Should have code style (generic)
    assert!(instructions.code_style.is_some());

    // No testing, deployment, gotchas, dependencies
    assert!(instructions.testing.is_none());
    assert!(instructions.deployment.is_none());
    assert!(instructions.gotchas.is_empty());
    assert!(instructions.dependencies.is_none());
}
```

## JSON Output Integration

**Enhanced JSON Serializer** (`forge-cli/src/serializers/json.rs`):

```rust
// In serialize_node() method:

let instruction_gen = InstructionGenerator::new(graph);
let instructions = instruction_gen.generate(&node.id).ok();

json!({
    "id": node.id.to_string(),
    "type": format!("{:?}", node.node_type),
    "display_name": node.display_name,
    "attributes": node.attributes,
    "business_context": node.business_context,
    "llm_instructions": instructions,  // NEW FIELD
    "metadata": { /* ... */ }
})
```

**Example Output**:

```json
{
  "id": "service:my-org:user-api",
  "type": "Service",
  "display_name": "user-api",
  "llm_instructions": {
    "code_style": "FastAPI framework. Use Pydantic models for validation. Type hints required. Use async/await patterns.",
    "testing": "pytest with >80% coverage. Use fixtures for setup. Mock external dependencies with pytest-mock.",
    "deployment": "cd terraform/user-api && terraform plan -var-file=production.tfvars && terraform apply -var-file=production.tfvars",
    "gotchas": [
      "DO NOT violate: Rate limit is 1000 req/sec",
      "MUST validate email before database writes"
    ],
    "dependencies": {
      "services": ["auth-service (token validation)"],
      "databases": ["users-table (primary data store)"],
      "queues": ["user-events-queue (publish on events)"]
    }
  }
}
```

## Acceptance Criteria

### Functionality

- [ ] Code style inferred for Python (FastAPI, Flask, Django, generic)
- [ ] Code style inferred for TypeScript/JavaScript (Express, NestJS, React)
- [ ] Testing requirements inferred from test_framework attribute
- [ ] Deployment commands generated for Terraform (with workspace)
- [ ] Deployment commands generated for SAM (with stack name)
- [ ] Deployment commands generated for CloudFormation
- [ ] Gotchas converted to DO NOT or MUST statements
- [ ] Dependency instructions include purpose context from business_context
- [ ] Template variables replaced ({service}, {workspace}, {stack_name})

### Code Quality

- [ ] 15+ unit tests covering all inference rules
- [ ] Integration test with full instruction generation
- [ ] All tests pass (>540 workspace tests)
- [ ] Clippy clean with no warnings
- [ ] No panics on missing attributes (graceful None returns)

### Documentation

- [ ] InstructionGenerator API documented
- [ ] Inference rules documented in spec
- [ ] Examples of generated instructions in spec
- [ ] JSON schema updated to include llm_instructions field

## Future Enhancements (Post-V1)

1. **Pattern-based style inference**: Detect async/await usage, decorator patterns, etc.
2. **Coverage requirement detection**: Parse coverage.yml or CI config for actual thresholds
3. **Custom instruction templates**: User-defined templates in forge.yaml
4. **LLM-powered instruction refinement**: Use LLM to improve instruction quality
5. **Framework version-specific guidance**: Different instructions for FastAPI 0.95 vs 0.100
6. **CI/CD command generation**: Include build, test, deploy pipeline commands
7. **Security guidance**: "Use parameterized queries to prevent SQL injection"

---

## Summary

LLM instruction generation is the final piece that transforms Forge from a data provider into an intelligent assistant. By converting graph knowledge into actionable instructions, Forge enables LLM coding agents to:

- Write code that matches codebase conventions
- Follow testing requirements
- Execute correct deployment commands
- Avoid known gotchas and constraints
- Understand dependency purposes and usage patterns

**The Result**: LLMs can build great code without asking humans for clarification.

**Estimated Effort**: 2 days
- Day 1: Core InstructionGenerator implementation + unit tests
- Day 2: JSON integration + integration tests + documentation
