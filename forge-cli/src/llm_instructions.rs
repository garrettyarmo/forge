//! LLM Instruction Generation Module
//!
//! This module transforms Forge's knowledge graph data into actionable, LLM-consumable
//! instructions. It converts business context, code patterns, deployment metadata, and
//! dependency graphs into explicit guidelines that help LLM coding agents write better code.
//!
//! # Overview
//!
//! The `InstructionGenerator` produces `LlmInstructions` containing:
//! - **Code style**: Inferred from language and framework (e.g., "FastAPI with Pydantic models")
//! - **Testing**: Requirements inferred from detected test frameworks
//! - **Deployment**: Commands generated from deployment metadata (Terraform, SAM, CloudFormation)
//! - **Gotchas**: Business context warnings converted to DO NOT/MUST statements
//! - **Dependencies**: Service/database/queue relationships with purpose context
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_cli::llm_instructions::InstructionGenerator;
//! use forge_graph::ForgeGraph;
//!
//! let graph = ForgeGraph::new();
//! // ... add nodes ...
//! let generator = InstructionGenerator::new(&graph);
//! let instructions = generator.generate(&node_id).unwrap();
//! ```

use forge_graph::{EdgeType, ForgeGraph, Node, NodeId, NodeType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// === Code Style Templates ===
// These templates guide LLMs on language/framework-specific conventions.

const PYTHON_FASTAPI: &str = "FastAPI framework. Use Pydantic models for validation. Type hints required. Use async/await patterns.";
const PYTHON_FLASK: &str = "Flask framework. Use blueprints for routing. Type hints recommended.";
const PYTHON_DJANGO: &str =
    "Django framework. Follow MTV pattern. Use Django ORM. Don't bypass model validation.";
const PYTHON_CHALICE: &str =
    "AWS Chalice framework. Use decorators for routing. Follow AWS Lambda best practices.";
const PYTHON_STARLETTE: &str =
    "Starlette framework. Use async/await patterns. Type hints recommended.";
const PYTHON_GENERIC: &str = "Python. Use type hints. Follow PEP 8 style guide.";

const TS_EXPRESS: &str =
    "Express.js with TypeScript. Use middleware pattern. Strong typing. Async error handling.";
const JS_EXPRESS: &str = "Express.js. Use middleware pattern. Handle async errors with try/catch.";
const TS_NESTJS: &str =
    "NestJS framework. Use decorators. Dependency injection. DTOs for validation.";
const TS_REACT: &str =
    "React with TypeScript. Use functional components with hooks. Strong typing for props.";
const JS_REACT: &str =
    "React. Use functional components with hooks. PropTypes or TypeScript for types.";
const TS_GENERIC: &str = "TypeScript. Use strict mode. Prefer interfaces over type aliases.";
const JS_GENERIC: &str = "JavaScript. Use ES6+ features. Consider adding TypeScript.";

// === Testing Templates ===

const PYTEST: &str = "pytest with >80% coverage. Use fixtures for setup. Mock external dependencies with pytest-mock.";
const JEST: &str =
    "Jest tests. Use describe/it blocks. Mock with jest.mock(). Aim for >80% coverage.";
const MOCHA: &str = "Mocha tests with Chai assertions. Use describe/it structure.";
const VITEST: &str = "Vitest tests. Fast unit tests. Use vi.mock() for mocking.";
const UNITTEST: &str = "Python unittest. Use setUp/tearDown. Mock with unittest.mock.";

/// Errors that can occur during instruction generation.
#[derive(Debug, Error)]
pub enum InstructionError {
    /// The requested node was not found in the graph.
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    /// An error occurred while querying the graph.
    #[error("Graph error: {0}")]
    GraphError(String),
}

/// Generated instructions for LLM agents.
///
/// This struct contains actionable guidelines derived from the knowledge graph
/// that help LLMs write code matching the codebase conventions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LlmInstructions {
    /// Code style guidelines inferred from language/framework.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_style: Option<String>,

    /// Testing requirements inferred from test frameworks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testing: Option<String>,

    /// Deployment commands generated from metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment: Option<String>,

    /// Critical DO NOT/MUST statements from business context gotchas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gotchas: Vec<String>,

    /// Dependency context with purpose descriptions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<DependencyInstructions>,
}

impl LlmInstructions {
    /// Check if this instruction set is empty (no useful instructions generated).
    pub fn is_empty(&self) -> bool {
        self.code_style.is_none()
            && self.testing.is_none()
            && self.deployment.is_none()
            && self.gotchas.is_empty()
            && self.dependencies.is_none()
    }
}

/// Dependency instructions organized by category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DependencyInstructions {
    /// Services this node calls (with purpose context).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,

    /// Databases this node reads/writes (with ownership context).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<String>,

    /// Queues this node publishes/subscribes (with event context).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queues: Vec<String>,
}

impl DependencyInstructions {
    /// Check if there are no dependencies.
    pub fn is_empty(&self) -> bool {
        self.services.is_empty() && self.databases.is_empty() && self.queues.is_empty()
    }
}

/// Generator for LLM instructions from the knowledge graph.
///
/// The `InstructionGenerator` transforms graph data into actionable instructions
/// that guide LLM coding agents on how to work with a service.
pub struct InstructionGenerator<'a> {
    graph: &'a ForgeGraph,
}

impl<'a> InstructionGenerator<'a> {
    /// Create a new instruction generator for the given graph.
    pub fn new(graph: &'a ForgeGraph) -> Self {
        Self { graph }
    }

    /// Generate instructions for a single node.
    ///
    /// # Arguments
    /// * `node_id` - The ID of the node to generate instructions for
    ///
    /// # Returns
    /// `LlmInstructions` containing all applicable guidelines, or an error if the node is not found.
    ///
    /// # Example
    /// ```rust,ignore
    /// let instructions = generator.generate(&node_id)?;
    /// if let Some(style) = &instructions.code_style {
    ///     println!("Code style: {}", style);
    /// }
    /// ```
    pub fn generate(&self, node_id: &NodeId) -> Result<LlmInstructions, InstructionError> {
        let node = self
            .graph
            .get_node(node_id)
            .ok_or_else(|| InstructionError::NodeNotFound(node_id.to_string()))?;

        // Only generate instructions for Service nodes
        if node.node_type != NodeType::Service {
            return Ok(LlmInstructions::default());
        }

        let code_style = self.infer_code_style(node);
        let testing = self.infer_testing_requirements(node);
        let deployment = self.generate_deployment_command(node);
        let gotchas = self.convert_gotchas(node);
        let dependencies = self.generate_dependency_instructions(node_id);

        Ok(LlmInstructions {
            code_style,
            testing,
            deployment,
            gotchas,
            dependencies,
        })
    }

    /// Infer code style guidelines from language and framework attributes.
    ///
    /// Uses a decision tree based on (language, framework) pairs to select
    /// appropriate style guidelines.
    fn infer_code_style(&self, node: &Node) -> Option<String> {
        let language = self.get_string_attribute(node, "language")?;
        let framework = self.get_string_attribute(node, "framework");

        let language_lower = language.to_lowercase();
        let framework_lower = framework.as_ref().map(|f| f.to_lowercase());

        match (language_lower.as_str(), framework_lower.as_deref()) {
            // Python frameworks
            ("python", Some("fastapi")) => Some(PYTHON_FASTAPI.to_string()),
            ("python", Some("flask")) => Some(PYTHON_FLASK.to_string()),
            ("python", Some("django")) => Some(PYTHON_DJANGO.to_string()),
            ("python", Some("chalice")) => Some(PYTHON_CHALICE.to_string()),
            ("python", Some("starlette")) => Some(PYTHON_STARLETTE.to_string()),
            ("python", _) => Some(PYTHON_GENERIC.to_string()),

            // TypeScript frameworks
            ("typescript", Some("express")) => Some(TS_EXPRESS.to_string()),
            ("typescript", Some("nestjs")) => Some(TS_NESTJS.to_string()),
            ("typescript", Some("react")) => Some(TS_REACT.to_string()),
            ("typescript", _) => Some(TS_GENERIC.to_string()),

            // JavaScript frameworks
            ("javascript", Some("express")) => Some(JS_EXPRESS.to_string()),
            ("javascript", Some("react")) => Some(JS_REACT.to_string()),
            ("javascript", _) => Some(JS_GENERIC.to_string()),

            // Unknown language
            _ => None,
        }
    }

    /// Infer testing requirements from detected test frameworks.
    fn infer_testing_requirements(&self, node: &Node) -> Option<String> {
        let test_framework = self.get_string_attribute(node, "test_framework")?;
        let framework_lower = test_framework.to_lowercase();

        match framework_lower.as_str() {
            "pytest" => Some(PYTEST.to_string()),
            "jest" => Some(JEST.to_string()),
            "mocha" => Some(MOCHA.to_string()),
            "vitest" => Some(VITEST.to_string()),
            "unittest" => Some(UNITTEST.to_string()),
            _ => None,
        }
    }

    /// Generate deployment command from deployment metadata.
    ///
    /// Supports Terraform, SAM, and CloudFormation deployment methods.
    fn generate_deployment_command(&self, node: &Node) -> Option<String> {
        let deployment_method = self.get_string_attribute(node, "deployment_method")?;
        let method_lower = deployment_method.to_lowercase();

        match method_lower.as_str() {
            "terraform" => Some(self.generate_terraform_command(node)),
            "sam" => Some(self.generate_sam_command(node)),
            "cloudformation" => Some(self.generate_cloudformation_command(node)),
            _ => None,
        }
    }

    /// Generate Terraform deployment command.
    fn generate_terraform_command(&self, node: &Node) -> String {
        let workspace = self
            .get_string_attribute(node, "terraform_workspace")
            .or_else(|| self.get_string_attribute(node, "environment"))
            .unwrap_or_else(|| "default".to_string());

        let service_name = &node.display_name;

        // If workspace is "default", don't use var-file
        if workspace == "default" {
            format!(
                "cd terraform/{} && terraform plan && terraform apply",
                service_name
            )
        } else {
            format!(
                "cd terraform/{} && terraform plan -var-file={}.tfvars && terraform apply -var-file={}.tfvars",
                service_name, workspace, workspace
            )
        }
    }

    /// Generate SAM deployment command.
    fn generate_sam_command(&self, node: &Node) -> String {
        let stack_name = self
            .get_string_attribute(node, "stack_name")
            .unwrap_or_else(|| format!("{}-stack", node.display_name));

        format!("sam build && sam deploy --stack-name {}", stack_name)
    }

    /// Generate CloudFormation deployment command.
    fn generate_cloudformation_command(&self, node: &Node) -> String {
        let stack_name = self
            .get_string_attribute(node, "stack_name")
            .unwrap_or_else(|| format!("{}-stack", node.display_name));

        format!(
            "aws cloudformation deploy --template-file template.yaml --stack-name {} --capabilities CAPABILITY_IAM",
            stack_name
        )
    }

    /// Convert business context gotchas to DO NOT/MUST statements.
    ///
    /// Transformation rules:
    /// - Contains "must", "required", "always" → "MUST {action}"
    /// - Contains "don't", "never", "avoid" → "DO NOT {action}"
    /// - Otherwise → "DO NOT violate: {gotcha}"
    fn convert_gotchas(&self, node: &Node) -> Vec<String> {
        let business_context = match &node.business_context {
            Some(ctx) => ctx,
            None => return Vec::new(),
        };

        business_context
            .gotchas
            .iter()
            .map(|gotcha| self.normalize_gotcha(gotcha))
            .collect()
    }

    /// Normalize a single gotcha to an actionable instruction.
    fn normalize_gotcha(&self, gotcha: &str) -> String {
        let lower = gotcha.to_lowercase();

        // Check for "must", "required", "always" patterns
        if lower.contains("must")
            || lower.contains("required")
            || lower.contains("always")
            || lower.contains("ensure")
        {
            return self.format_must_statement(gotcha);
        }

        // Check for "don't", "never", "avoid" patterns
        if lower.contains("don't")
            || lower.contains("dont")
            || lower.contains("never")
            || lower.contains("avoid")
            || lower.contains("do not")
        {
            return self.format_do_not_statement(gotcha);
        }

        // Default: wrap as informational constraint
        format!("DO NOT violate: {}", gotcha)
    }

    /// Format a gotcha as a MUST statement.
    fn format_must_statement(&self, gotcha: &str) -> String {
        // If it already starts with a directive-like word, just prefix MUST
        let trimmed = gotcha.trim();
        let lower = trimmed.to_lowercase();

        // Remove existing "must" to avoid "MUST must..."
        if lower.starts_with("must ") {
            format!("MUST {}", &trimmed[5..].trim())
        } else if lower.starts_with("always ") {
            format!("MUST {}", &trimmed[7..].trim())
        } else if lower.contains("is required") || lower.contains("are required") {
            // Convert "X is required" to "MUST have X"
            let action = trimmed
                .replace(" is required", "")
                .replace(" are required", "");
            format!("MUST ensure {}", action)
        } else {
            format!("MUST {}", trimmed)
        }
    }

    /// Format a gotcha as a DO NOT statement.
    fn format_do_not_statement(&self, gotcha: &str) -> String {
        let trimmed = gotcha.trim();
        let lower = trimmed.to_lowercase();

        // Remove existing negative prefix to avoid "DO NOT never..."
        // "don't", "dont", "never", "avoid" are all 6 chars (including space)
        if lower.starts_with("don't ")
            || lower.starts_with("dont ")
            || lower.starts_with("never ")
            || lower.starts_with("avoid ")
        {
            format!("DO NOT {}", &trimmed[6..].trim())
        } else if lower.starts_with("do not ") {
            format!("DO NOT {}", &trimmed[7..].trim())
        } else {
            format!("DO NOT {}", trimmed)
        }
    }

    /// Generate dependency instructions from graph edges.
    fn generate_dependency_instructions(&self, node_id: &NodeId) -> Option<DependencyInstructions> {
        let mut deps = DependencyInstructions::default();

        // Service dependencies (CALLS edges)
        for edge in self.graph.edges_from_by_type(node_id, EdgeType::Calls) {
            if let Some(target) = self.graph.get_node(&edge.target) {
                if target.node_type == NodeType::Service {
                    let purpose = self.extract_purpose(target);
                    deps.services
                        .push(format!("{} ({})", target.display_name, purpose));
                }
            }
        }

        // Database dependencies (READS/WRITES edges)
        for edge in self.graph.edges_from_by_type(node_id, EdgeType::Reads) {
            if let Some(target) = self.graph.get_node(&edge.target) {
                if target.node_type == NodeType::Database {
                    deps.databases
                        .push(format!("{} (read access)", target.display_name));
                }
            }
        }

        for edge in self.graph.edges_from_by_type(node_id, EdgeType::Writes) {
            if let Some(target) = self.graph.get_node(&edge.target) {
                if target.node_type == NodeType::Database {
                    // Check if we already have this as read, upgrade to read/write
                    let existing = deps
                        .databases
                        .iter()
                        .position(|d| d.starts_with(&target.display_name));
                    if let Some(pos) = existing {
                        deps.databases[pos] =
                            format!("{} (read/write access)", target.display_name);
                    } else {
                        deps.databases
                            .push(format!("{} (write access)", target.display_name));
                    }
                }
            }
        }

        // Queue dependencies (PUBLISHES/SUBSCRIBES edges)
        for edge in self.graph.edges_from_by_type(node_id, EdgeType::Publishes) {
            if let Some(target) = self.graph.get_node(&edge.target) {
                if target.node_type == NodeType::Queue {
                    deps.queues
                        .push(format!("{} (publish on events)", target.display_name));
                }
            }
        }

        for edge in self.graph.edges_from_by_type(node_id, EdgeType::Subscribes) {
            if let Some(target) = self.graph.get_node(&edge.target) {
                if target.node_type == NodeType::Queue {
                    deps.queues
                        .push(format!("{} (consume messages)", target.display_name));
                }
            }
        }

        // Return None if no dependencies found
        if deps.is_empty() { None } else { Some(deps) }
    }

    /// Extract purpose from a node's business context.
    fn extract_purpose(&self, node: &Node) -> String {
        node.business_context
            .as_ref()
            .and_then(|ctx| ctx.purpose.clone())
            .unwrap_or_else(|| "purpose unknown".to_string())
    }

    /// Get a string attribute from a node.
    fn get_string_attribute(&self, node: &Node, key: &str) -> Option<String> {
        node.attributes.get(key).and_then(|v| match v {
            forge_graph::AttributeValue::String(s) => Some(s.clone()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_graph::{BusinessContext, DiscoverySource, Edge, NodeBuilder};
    use pretty_assertions::assert_eq;

    /// Helper to create a test graph.
    fn create_test_graph() -> ForgeGraph {
        ForgeGraph::new()
    }

    /// Helper to create a service node with given attributes.
    fn create_service_with_attrs(
        graph: &mut ForgeGraph,
        namespace: &str,
        name: &str,
        attrs: &[(&str, &str)],
    ) -> NodeId {
        let id = NodeId::new(NodeType::Service, namespace, name).unwrap();
        let mut builder = NodeBuilder::new()
            .id(id.clone())
            .node_type(NodeType::Service)
            .display_name(name)
            .source(DiscoverySource::Manual);

        for (key, value) in attrs {
            builder = builder.attribute(*key, *value);
        }

        graph.add_node(builder.build().unwrap()).unwrap();
        id
    }

    /// Helper to create a service with business context.
    fn create_service_with_context(
        graph: &mut ForgeGraph,
        namespace: &str,
        name: &str,
        context: BusinessContext,
    ) -> NodeId {
        let id = NodeId::new(NodeType::Service, namespace, name).unwrap();
        let node = NodeBuilder::new()
            .id(id.clone())
            .node_type(NodeType::Service)
            .display_name(name)
            .source(DiscoverySource::Manual)
            .business_context(context)
            .build()
            .unwrap();

        graph.add_node(node).unwrap();
        id
    }

    // === Code Style Inference Tests ===

    #[test]
    fn test_infer_python_fastapi_style() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "api",
            &[("language", "python"), ("framework", "fastapi")],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let style = instructions.code_style.unwrap();
        assert!(style.contains("FastAPI"));
        assert!(style.contains("Pydantic"));
        assert!(style.contains("async/await"));
    }

    #[test]
    fn test_infer_python_flask_style() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "api",
            &[("language", "python"), ("framework", "flask")],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let style = instructions.code_style.unwrap();
        assert!(style.contains("Flask"));
        assert!(style.contains("blueprints"));
    }

    #[test]
    fn test_infer_python_generic_style() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "api", &[("language", "python")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let style = instructions.code_style.unwrap();
        assert!(style.contains("Python"));
        assert!(style.contains("PEP 8"));
    }

    #[test]
    fn test_infer_typescript_express_style() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "api",
            &[("language", "typescript"), ("framework", "express")],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let style = instructions.code_style.unwrap();
        assert!(style.contains("Express.js"));
        assert!(style.contains("TypeScript"));
        assert!(style.contains("middleware"));
    }

    #[test]
    fn test_infer_javascript_react_style() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "ui",
            &[("language", "javascript"), ("framework", "react")],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let style = instructions.code_style.unwrap();
        assert!(style.contains("React"));
        assert!(style.contains("functional components"));
    }

    #[test]
    fn test_infer_style_unknown_language() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "api", &[("language", "rust")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert!(instructions.code_style.is_none());
    }

    #[test]
    fn test_infer_style_case_insensitive() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "api",
            &[("language", "PYTHON"), ("framework", "FastAPI")],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert!(instructions.code_style.is_some());
        assert!(instructions.code_style.unwrap().contains("FastAPI"));
    }

    // === Testing Inference Tests ===

    #[test]
    fn test_infer_pytest_requirements() {
        let mut graph = create_test_graph();
        let id =
            create_service_with_attrs(&mut graph, "ns", "api", &[("test_framework", "pytest")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let testing = instructions.testing.unwrap();
        assert!(testing.contains("pytest"));
        assert!(testing.contains("fixtures"));
        assert!(testing.contains("coverage"));
    }

    #[test]
    fn test_infer_jest_requirements() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "api", &[("test_framework", "jest")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let testing = instructions.testing.unwrap();
        assert!(testing.contains("Jest"));
        assert!(testing.contains("describe/it"));
        assert!(testing.contains("mock"));
    }

    #[test]
    fn test_infer_no_test_framework() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "api", &[("language", "python")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert!(instructions.testing.is_none());
    }

    // === Deployment Command Tests ===

    #[test]
    fn test_generate_terraform_deployment() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "user-api",
            &[
                ("deployment_method", "terraform"),
                ("terraform_workspace", "production"),
            ],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let deployment = instructions.deployment.unwrap();
        assert!(deployment.contains("terraform apply"));
        assert!(deployment.contains("production.tfvars"));
        assert!(deployment.contains("terraform/user-api"));
    }

    #[test]
    fn test_generate_terraform_default_workspace() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "user-api",
            &[("deployment_method", "terraform")],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let deployment = instructions.deployment.unwrap();
        assert!(deployment.contains("terraform apply"));
        // Should not have var-file for default workspace
        assert!(!deployment.contains(".tfvars"));
    }

    #[test]
    fn test_generate_sam_deployment() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "user-api",
            &[
                ("deployment_method", "sam"),
                ("stack_name", "user-api-prod-stack"),
            ],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let deployment = instructions.deployment.unwrap();
        assert!(deployment.contains("sam build"));
        assert!(deployment.contains("sam deploy"));
        assert!(deployment.contains("user-api-prod-stack"));
    }

    #[test]
    fn test_generate_cloudformation_deployment() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(
            &mut graph,
            "ns",
            "infra",
            &[
                ("deployment_method", "cloudformation"),
                ("stack_name", "infra-stack"),
            ],
        );

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        let deployment = instructions.deployment.unwrap();
        assert!(deployment.contains("aws cloudformation deploy"));
        assert!(deployment.contains("infra-stack"));
        assert!(deployment.contains("CAPABILITY_IAM"));
    }

    #[test]
    fn test_generate_deployment_no_metadata() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "api", &[("language", "python")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert!(instructions.deployment.is_none());
    }

    // === Gotcha Transformation Tests ===

    #[test]
    fn test_convert_gotchas_to_do_not() {
        let mut graph = create_test_graph();
        let context = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec![
                "Rate limit is 1000 req/sec".to_string(),
                "Cache TTL is 5 minutes".to_string(),
            ],
            notes: Default::default(),
        };
        let id = create_service_with_context(&mut graph, "ns", "api", context);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert_eq!(instructions.gotchas.len(), 2);
        assert!(instructions.gotchas[0].starts_with("DO NOT"));
        assert!(instructions.gotchas[1].starts_with("DO NOT"));
    }

    #[test]
    fn test_convert_gotcha_with_must() {
        let mut graph = create_test_graph();
        let context = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec!["Email validation is required before writes".to_string()],
            notes: Default::default(),
        };
        let id = create_service_with_context(&mut graph, "ns", "api", context);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert_eq!(instructions.gotchas.len(), 1);
        assert!(instructions.gotchas[0].starts_with("MUST"));
        assert!(instructions.gotchas[0].contains("validation"));
    }

    #[test]
    fn test_convert_gotcha_with_never() {
        let mut graph = create_test_graph();
        let context = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec!["Never bypass the cache layer".to_string()],
            notes: Default::default(),
        };
        let id = create_service_with_context(&mut graph, "ns", "api", context);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert_eq!(instructions.gotchas.len(), 1);
        assert!(instructions.gotchas[0].starts_with("DO NOT"));
        assert!(instructions.gotchas[0].contains("bypass"));
        assert!(instructions.gotchas[0].contains("cache"));
    }

    #[test]
    fn test_convert_gotcha_with_always() {
        let mut graph = create_test_graph();
        let context = BusinessContext {
            purpose: None,
            owner: None,
            history: None,
            gotchas: vec!["Always use transactions for multi-table updates".to_string()],
            notes: Default::default(),
        };
        let id = create_service_with_context(&mut graph, "ns", "api", context);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert_eq!(instructions.gotchas.len(), 1);
        assert!(instructions.gotchas[0].starts_with("MUST"));
        assert!(instructions.gotchas[0].contains("transactions"));
    }

    #[test]
    fn test_no_gotchas_without_business_context() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "api", &[("language", "python")]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert!(instructions.gotchas.is_empty());
    }

    // === Dependency Instruction Tests ===

    #[test]
    fn test_generate_service_dependencies() {
        let mut graph = create_test_graph();

        // Create auth-service with purpose
        let auth_context = BusinessContext {
            purpose: Some("Token validation".to_string()),
            owner: None,
            history: None,
            gotchas: vec![],
            notes: Default::default(),
        };
        create_service_with_context(&mut graph, "ns", "auth-service", auth_context);

        // Create api-service
        let api_id = create_service_with_attrs(&mut graph, "ns", "api-service", &[]);

        // Create CALLS edge
        let edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "api-service").unwrap(),
            NodeId::new(NodeType::Service, "ns", "auth-service").unwrap(),
            EdgeType::Calls,
        )
        .unwrap();
        graph.add_edge(edge).unwrap();

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&api_id).unwrap();

        let deps = instructions.dependencies.unwrap();
        assert_eq!(deps.services.len(), 1);
        assert!(deps.services[0].contains("auth-service"));
        assert!(deps.services[0].contains("Token validation"));
    }

    #[test]
    fn test_generate_database_dependencies() {
        let mut graph = create_test_graph();

        // Create database
        let db_id = NodeId::new(NodeType::Database, "ns", "users-table").unwrap();
        let db_node = NodeBuilder::new()
            .id(db_id.clone())
            .node_type(NodeType::Database)
            .display_name("users-table")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();
        graph.add_node(db_node).unwrap();

        // Create service
        let svc_id = create_service_with_attrs(&mut graph, "ns", "api-service", &[]);

        // Create READS and WRITES edges
        let read_edge = Edge::new(svc_id.clone(), db_id.clone(), EdgeType::Reads).unwrap();
        let write_edge = Edge::new(svc_id.clone(), db_id.clone(), EdgeType::Writes).unwrap();
        graph.add_edge(read_edge).unwrap();
        graph.add_edge(write_edge).unwrap();

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&svc_id).unwrap();

        let deps = instructions.dependencies.unwrap();
        assert_eq!(deps.databases.len(), 1);
        assert!(deps.databases[0].contains("users-table"));
        assert!(deps.databases[0].contains("read/write"));
    }

    #[test]
    fn test_generate_queue_dependencies() {
        let mut graph = create_test_graph();

        // Create queue
        let queue_id = NodeId::new(NodeType::Queue, "ns", "user-events").unwrap();
        let queue_node = NodeBuilder::new()
            .id(queue_id.clone())
            .node_type(NodeType::Queue)
            .display_name("user-events")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();
        graph.add_node(queue_node).unwrap();

        // Create service
        let svc_id = create_service_with_attrs(&mut graph, "ns", "api-service", &[]);

        // Create PUBLISHES edge
        let edge = Edge::new(svc_id.clone(), queue_id.clone(), EdgeType::Publishes).unwrap();
        graph.add_edge(edge).unwrap();

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&svc_id).unwrap();

        let deps = instructions.dependencies.unwrap();
        assert_eq!(deps.queues.len(), 1);
        assert!(deps.queues[0].contains("user-events"));
        assert!(deps.queues[0].contains("publish"));
    }

    #[test]
    fn test_generate_dependencies_no_edges() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "isolated-service", &[]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        assert!(instructions.dependencies.is_none());
    }

    // === Integration Tests ===

    #[test]
    fn test_full_llm_instruction_generation() {
        let mut graph = create_test_graph();

        // Create auth-service with purpose
        let auth_context = BusinessContext {
            purpose: Some("Token validation".to_string()),
            owner: None,
            history: None,
            gotchas: vec![],
            notes: Default::default(),
        };
        create_service_with_context(&mut graph, "ns", "auth-service", auth_context);

        // Create database
        let db_id = NodeId::new(NodeType::Database, "ns", "users-table").unwrap();
        let db_node = NodeBuilder::new()
            .id(db_id.clone())
            .node_type(NodeType::Database)
            .display_name("users-table")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();
        graph.add_node(db_node).unwrap();

        // Create queue
        let queue_id = NodeId::new(NodeType::Queue, "ns", "events-queue").unwrap();
        let queue_node = NodeBuilder::new()
            .id(queue_id.clone())
            .node_type(NodeType::Queue)
            .display_name("events-queue")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();
        graph.add_node(queue_node).unwrap();

        // Create main service with all attributes
        let main_context = BusinessContext {
            purpose: Some("Main API service".to_string()),
            owner: Some("Platform Team".to_string()),
            history: None,
            gotchas: vec![
                "Rate limit is 1000 req/sec".to_string(),
                "Must validate email before writes".to_string(),
            ],
            notes: Default::default(),
        };
        let main_id = NodeId::new(NodeType::Service, "ns", "user-api").unwrap();
        let main_node = NodeBuilder::new()
            .id(main_id.clone())
            .node_type(NodeType::Service)
            .display_name("user-api")
            .attribute("language", "python")
            .attribute("framework", "fastapi")
            .attribute("test_framework", "pytest")
            .attribute("deployment_method", "terraform")
            .attribute("terraform_workspace", "production")
            .source(DiscoverySource::Manual)
            .business_context(main_context)
            .build()
            .unwrap();
        graph.add_node(main_node).unwrap();

        // Add edges
        graph
            .add_edge(Edge::new(main_id.clone(), db_id.clone(), EdgeType::Reads).unwrap())
            .unwrap();
        graph
            .add_edge(Edge::new(main_id.clone(), db_id.clone(), EdgeType::Writes).unwrap())
            .unwrap();
        graph
            .add_edge(Edge::new(main_id.clone(), queue_id.clone(), EdgeType::Publishes).unwrap())
            .unwrap();
        graph
            .add_edge(
                Edge::new(
                    main_id.clone(),
                    NodeId::new(NodeType::Service, "ns", "auth-service").unwrap(),
                    EdgeType::Calls,
                )
                .unwrap(),
            )
            .unwrap();

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&main_id).unwrap();

        // Verify code style
        assert!(instructions.code_style.is_some());
        assert!(
            instructions
                .code_style
                .as_ref()
                .unwrap()
                .contains("FastAPI")
        );

        // Verify testing
        assert!(instructions.testing.is_some());
        assert!(instructions.testing.as_ref().unwrap().contains("pytest"));

        // Verify deployment
        assert!(instructions.deployment.is_some());
        assert!(
            instructions
                .deployment
                .as_ref()
                .unwrap()
                .contains("terraform apply")
        );
        assert!(
            instructions
                .deployment
                .as_ref()
                .unwrap()
                .contains("production.tfvars")
        );

        // Verify gotchas
        assert_eq!(instructions.gotchas.len(), 2);
        assert!(
            instructions
                .gotchas
                .iter()
                .all(|g| g.starts_with("DO NOT") || g.starts_with("MUST"))
        );

        // Verify dependencies
        assert!(instructions.dependencies.is_some());
        let deps = instructions.dependencies.unwrap();
        assert!(!deps.services.is_empty());
        assert!(!deps.databases.is_empty());
        assert!(!deps.queues.is_empty());
    }

    #[test]
    fn test_minimal_instructions_no_metadata() {
        let mut graph = create_test_graph();
        let id = create_service_with_attrs(&mut graph, "ns", "basic-service", &[]);

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&id).unwrap();

        // Should have no instructions
        assert!(instructions.code_style.is_none());
        assert!(instructions.testing.is_none());
        assert!(instructions.deployment.is_none());
        assert!(instructions.gotchas.is_empty());
        assert!(instructions.dependencies.is_none());
        assert!(instructions.is_empty());
    }

    #[test]
    fn test_node_not_found_error() {
        let graph = create_test_graph();
        let generator = InstructionGenerator::new(&graph);

        let fake_id = NodeId::new(NodeType::Service, "ns", "nonexistent").unwrap();
        let result = generator.generate(&fake_id);

        assert!(matches!(result, Err(InstructionError::NodeNotFound(_))));
    }

    #[test]
    fn test_non_service_node_returns_empty() {
        let mut graph = create_test_graph();

        // Create a database node
        let db_id = NodeId::new(NodeType::Database, "ns", "users-db").unwrap();
        let db_node = NodeBuilder::new()
            .id(db_id.clone())
            .node_type(NodeType::Database)
            .display_name("users-db")
            .attribute("db_type", "dynamodb")
            .source(DiscoverySource::Manual)
            .build()
            .unwrap();
        graph.add_node(db_node).unwrap();

        let generator = InstructionGenerator::new(&graph);
        let instructions = generator.generate(&db_id).unwrap();

        // Should return empty instructions for non-service nodes
        assert!(instructions.is_empty());
    }

    #[test]
    fn test_llm_instructions_serialization() {
        let instructions = LlmInstructions {
            code_style: Some("FastAPI framework".to_string()),
            testing: Some("pytest".to_string()),
            deployment: Some("terraform apply".to_string()),
            gotchas: vec!["DO NOT exceed rate limit".to_string()],
            dependencies: Some(DependencyInstructions {
                services: vec!["auth-service (token validation)".to_string()],
                databases: vec!["users-table (read/write)".to_string()],
                queues: vec![],
            }),
        };

        let json = serde_json::to_string_pretty(&instructions).unwrap();
        let deserialized: LlmInstructions = serde_json::from_str(&json).unwrap();

        assert_eq!(instructions, deserialized);
    }
}
