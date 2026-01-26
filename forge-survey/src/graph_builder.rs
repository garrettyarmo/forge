//! GraphBuilder - Converts parser discoveries into a knowledge graph.
//!
//! This module provides the bridge between code analysis (parser discoveries)
//! and the knowledge graph representation. It handles:
//!
//! - Service node creation from package.json and similar files
//! - Database, queue, and cloud resource node creation
//! - Edge creation for relationships (reads, writes, calls, etc.)
//! - Deduplication of nodes across repositories
//! - Incremental graph building (can start from existing graph)

use crate::parser::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, QueueOperationDiscovery, QueueOperationType, ServiceDiscovery,
};
use forge_graph::{
    AttributeValue, DiscoverySource, Edge, EdgeType, ForgeGraph, NodeBuilder, NodeId, NodeType,
};
use std::collections::HashMap;

/// Builds a knowledge graph from parser discoveries.
///
/// The GraphBuilder maintains internal state to deduplicate nodes and track
/// relationships between services and resources across multiple repositories.
///
/// # Example
///
/// ```rust,ignore
/// use forge_survey::GraphBuilder;
/// use forge_survey::parser::javascript::JavaScriptParser;
///
/// let mut builder = GraphBuilder::new();
/// builder.set_repo_context("my-org/user-service", Some("abc123"));
///
/// let parser = JavaScriptParser::new().unwrap();
/// if let Some(service) = parser.parse_package_json(&repo_path) {
///     let service_id = builder.add_service(service);
///     let discoveries = parser.parse_repo(&repo_path).unwrap();
///     builder.process_discoveries(discoveries, &service_id);
/// }
///
/// let graph = builder.build();
/// ```
pub struct GraphBuilder {
    /// The knowledge graph being constructed
    graph: ForgeGraph,

    /// Map from discovered service names to their NodeIds (for deduplication)
    service_map: HashMap<String, NodeId>,

    /// Map from resource identifiers to NodeIds (for deduplication)
    /// Key format: "{resource_type}:{name}" (e.g., "dynamodb:users-table")
    resource_map: HashMap<String, NodeId>,

    /// Current repo being processed (e.g., "my-org/user-service")
    current_repo: Option<String>,

    /// Current commit SHA being processed
    current_commit: Option<String>,

    /// Current environment name (e.g., "production", "staging")
    /// Injected from forge.yaml environment mapping
    current_environment: Option<String>,

    /// Current AWS account ID for this environment
    /// Injected from forge.yaml environment mapping
    current_aws_account_id: Option<String>,
}

impl GraphBuilder {
    /// Create a new GraphBuilder with an empty graph.
    pub fn new() -> Self {
        Self {
            graph: ForgeGraph::new(),
            service_map: HashMap::new(),
            resource_map: HashMap::new(),
            current_repo: None,
            current_commit: None,
            current_environment: None,
            current_aws_account_id: None,
        }
    }

    /// Load an existing graph to update (for incremental survey).
    ///
    /// This rebuilds the internal indexes from the existing graph,
    /// allowing new discoveries to be merged with existing nodes.
    pub fn from_graph(graph: ForgeGraph) -> Self {
        let mut builder = Self {
            graph,
            service_map: HashMap::new(),
            resource_map: HashMap::new(),
            current_repo: None,
            current_commit: None,
            current_environment: None,
            current_aws_account_id: None,
        };

        // Rebuild indexes from existing graph
        for node in builder.graph.nodes() {
            match node.node_type {
                NodeType::Service => {
                    builder
                        .service_map
                        .insert(node.display_name.clone(), node.id.clone());
                }
                NodeType::Database | NodeType::Queue | NodeType::CloudResource => {
                    builder
                        .resource_map
                        .insert(node.display_name.clone(), node.id.clone());
                }
                _ => {}
            }
        }

        builder
    }

    /// Set the current repository context for subsequent discoveries.
    ///
    /// This context is used to populate the `repo_url` attribute and
    /// namespace for NodeIds.
    pub fn set_repo_context(&mut self, repo_name: &str, commit_sha: Option<&str>) {
        self.current_repo = Some(repo_name.to_string());
        self.current_commit = commit_sha.map(|s| s.to_string());
    }

    /// Set the environment context for subsequent discoveries.
    ///
    /// This context is used to inject `environment` and `aws_account_id`
    /// attributes into graph nodes. Call this after `set_repo_context()`
    /// and before processing discoveries.
    ///
    /// # Arguments
    ///
    /// * `env_name` - Environment name (e.g., "production", "staging")
    /// * `aws_account_id` - Optional AWS account ID for this environment
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.set_repo_context("my-org/api-service", Some("abc123"));
    /// builder.set_environment("production", Some("123456789012"));
    /// // Now all discoveries will have environment and aws_account_id attributes
    /// ```
    pub fn set_environment(&mut self, env_name: &str, aws_account_id: Option<&str>) {
        self.current_environment = Some(env_name.to_string());
        self.current_aws_account_id = aws_account_id.map(|s| s.to_string());
    }

    /// Clear the environment context.
    ///
    /// Call this when switching to a repo that doesn't belong to any environment.
    pub fn clear_environment(&mut self) {
        self.current_environment = None;
        self.current_aws_account_id = None;
    }

    /// Process a service discovery and return its NodeId.
    ///
    /// If a service with the same name already exists, returns the existing
    /// NodeId. Otherwise creates a new Service node.
    pub fn add_service(&mut self, discovery: ServiceDiscovery) -> NodeId {
        let namespace = self
            .current_repo
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let id = NodeId::new(NodeType::Service, &namespace, &discovery.name)
            .expect("Failed to create service NodeId");

        // Check if service already exists
        if let Some(existing_id) = self.service_map.get(&discovery.name) {
            // Update the timestamp to mark it as recently surveyed
            if let Some(node) = self.graph.get_node_mut(existing_id) {
                node.metadata.updated_at = chrono::Utc::now();
            }
            return existing_id.clone();
        }

        // Build the service node
        let mut builder = NodeBuilder::new()
            .id(id.clone())
            .node_type(NodeType::Service)
            .display_name(&discovery.name)
            .attribute("language", discovery.language)
            .attribute("entry_point", discovery.entry_point)
            .source(DiscoverySource::JavaScriptParser);

        if let Some(repo) = &self.current_repo {
            builder = builder.attribute("repo_url", repo.clone());
        }

        if let Some(commit) = &self.current_commit {
            builder = builder.commit_sha(commit);
        }

        builder = builder
            .source_file(discovery.source_file)
            .source_line(discovery.source_line);

        let mut node = builder.build().expect("Failed to build service node");

        // Add framework as attribute if present
        if let Some(framework) = discovery.framework {
            node.attributes
                .insert("framework".to_string(), AttributeValue::String(framework));
        }

        // Add deployment metadata if present
        if let Some(metadata) = discovery.deployment_metadata {
            node.attributes.insert(
                "deployment_method".to_string(),
                AttributeValue::String(metadata.deployment_method),
            );
            if let Some(workspace) = metadata.terraform_workspace {
                node.attributes.insert(
                    "terraform_workspace".to_string(),
                    AttributeValue::String(workspace),
                );
            }
            if let Some(env) = metadata.environment {
                node.attributes
                    .insert("environment".to_string(), AttributeValue::String(env));
            }
            if let Some(stack_name) = metadata.stack_name {
                node.attributes
                    .insert("stack_name".to_string(), AttributeValue::String(stack_name));
            }
            // Store non-empty tags as a map attribute
            if !metadata.tags.is_empty() {
                let tags_map: HashMap<String, AttributeValue> = metadata
                    .tags
                    .into_iter()
                    .map(|(k, v)| (k, AttributeValue::String(v)))
                    .collect();
                node.attributes
                    .insert("tags".to_string(), AttributeValue::Map(tags_map));
            }
        }

        // Inject environment context from forge.yaml if not already set by deployment_metadata
        if !node.attributes.contains_key("environment") {
            if let Some(env) = &self.current_environment {
                node.attributes.insert(
                    "environment".to_string(),
                    AttributeValue::String(env.clone()),
                );
            }
        }
        if let Some(account_id) = &self.current_aws_account_id {
            node.attributes.insert(
                "aws_account_id".to_string(),
                AttributeValue::String(account_id.clone()),
            );
        }

        self.graph.upsert_node(node);
        self.service_map.insert(discovery.name, id.clone());
        id
    }

    /// Process all discoveries from a repository for a given service.
    ///
    /// This is the main entry point for converting parser output into graph
    /// nodes and edges.
    pub fn process_discoveries(&mut self, discoveries: Vec<Discovery>, service_id: &NodeId) {
        for discovery in discoveries {
            match discovery {
                Discovery::Service(svc) => {
                    self.add_service(svc);
                }
                Discovery::Import(import) => {
                    // Track imports for dependency analysis
                    // External imports might indicate service calls
                    if !import.is_relative && self.is_known_service(&import.module) {
                        self.add_service_call(
                            service_id,
                            &import.module,
                            &import.source_file,
                            import.source_line,
                        );
                    }
                }
                Discovery::ApiCall(call) => {
                    self.add_api_call(service_id, call);
                }
                Discovery::DatabaseAccess(db) => {
                    self.add_database_access(service_id, db);
                }
                Discovery::QueueOperation(queue) => {
                    self.add_queue_operation(service_id, queue);
                }
                Discovery::CloudResourceUsage(resource) => {
                    self.add_cloud_resource(service_id, resource);
                }
            }
        }
    }

    /// Check if a module name matches a known service.
    fn is_known_service(&self, module: &str) -> bool {
        self.service_map.contains_key(module)
    }

    /// Add a service-to-service call edge.
    fn add_service_call(
        &mut self,
        from: &NodeId,
        to_name: &str,
        source_file: &str,
        source_line: u32,
    ) {
        if let Some(to_id) = self.service_map.get(to_name) {
            let mut edge = Edge::new(from.clone(), to_id.clone(), EdgeType::Calls)
                .expect("Failed to create CALLS edge");
            edge.metadata
                .evidence
                .push(format!("{}:{}", source_file, source_line));
            edge.metadata.discovered_at = chrono::Utc::now();
            let _ = self.graph.upsert_edge(edge);
        }
    }

    /// Add an API call discovery.
    ///
    /// For now, we record API calls as attributes on the service node.
    /// In the future, this could create API nodes and CALLS edges if we can
    /// resolve the target service.
    fn add_api_call(&mut self, service_id: &NodeId, call: ApiCallDiscovery) {
        if let Some(node) = self.graph.get_node_mut(service_id) {
            let calls = node
                .attributes
                .entry("api_calls".to_string())
                .or_insert_with(|| AttributeValue::List(vec![]));

            if let AttributeValue::List(list) = calls {
                let mut call_map = HashMap::new();
                call_map.insert("target".to_string(), AttributeValue::String(call.target));

                if let Some(method) = call.method {
                    call_map.insert("method".to_string(), AttributeValue::String(method));
                }

                call_map.insert(
                    "source".to_string(),
                    AttributeValue::String(format!("{}:{}", call.source_file, call.source_line)),
                );

                list.push(AttributeValue::Map(call_map));
            }
        }
    }

    /// Add a database access discovery, creating a Database node and edge.
    fn add_database_access(&mut self, service_id: &NodeId, db: DatabaseAccessDiscovery) {
        // Create or get database node
        let db_name = db
            .table_name
            .clone()
            .unwrap_or_else(|| format!("{}-unknown", db.db_type));
        let namespace = self
            .current_repo
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let db_id = if let Some(id) = self.resource_map.get(&db_name) {
            // Update the timestamp to mark it as recently surveyed
            if let Some(node) = self.graph.get_node_mut(id) {
                node.metadata.updated_at = chrono::Utc::now();
            }
            id.clone()
        } else {
            let id = NodeId::new(NodeType::Database, &namespace, &db_name)
                .expect("Failed to create database NodeId");

            let mut node = NodeBuilder::new()
                .id(id.clone())
                .node_type(NodeType::Database)
                .display_name(&db_name)
                .attribute("db_type", db.db_type.clone())
                .source(DiscoverySource::JavaScriptParser)
                .source_file(db.source_file.clone())
                .source_line(db.source_line)
                .build()
                .expect("Failed to build database node");

            // Add deployment metadata if present
            if let Some(ref metadata) = db.deployment_metadata {
                node.attributes.insert(
                    "deployment_method".to_string(),
                    AttributeValue::String(metadata.deployment_method.clone()),
                );
                if let Some(ref workspace) = metadata.terraform_workspace {
                    node.attributes.insert(
                        "terraform_workspace".to_string(),
                        AttributeValue::String(workspace.clone()),
                    );
                }
                if let Some(ref env) = metadata.environment {
                    node.attributes.insert(
                        "environment".to_string(),
                        AttributeValue::String(env.clone()),
                    );
                }
                if let Some(ref stack_name) = metadata.stack_name {
                    node.attributes.insert(
                        "stack_name".to_string(),
                        AttributeValue::String(stack_name.clone()),
                    );
                }
            }

            // Inject environment context from forge.yaml if not already set
            if !node.attributes.contains_key("environment") {
                if let Some(env) = &self.current_environment {
                    node.attributes.insert(
                        "environment".to_string(),
                        AttributeValue::String(env.clone()),
                    );
                }
            }
            if let Some(account_id) = &self.current_aws_account_id {
                node.attributes.insert(
                    "aws_account_id".to_string(),
                    AttributeValue::String(account_id.clone()),
                );
            }

            self.graph.upsert_node(node);
            self.resource_map.insert(db_name, id.clone());
            id
        };

        // Create edge based on operation type
        let edge_type = match db.operation {
            DatabaseOperation::Read => EdgeType::Reads,
            DatabaseOperation::Write => EdgeType::Writes,
            DatabaseOperation::ReadWrite => EdgeType::Reads, // Add both
            DatabaseOperation::Unknown => EdgeType::Reads,   // Default to read
        };

        let mut edge = Edge::new(service_id.clone(), db_id.clone(), edge_type)
            .expect("Failed to create database edge");
        edge.metadata
            .evidence
            .push(format!("{}:{}", db.source_file, db.source_line));
        edge.metadata.discovered_at = chrono::Utc::now();
        let _ = self.graph.upsert_edge(edge);

        // If ReadWrite, add write edge too
        if db.operation == DatabaseOperation::ReadWrite {
            let mut write_edge = Edge::new(service_id.clone(), db_id, EdgeType::Writes)
                .expect("Failed to create write edge");
            write_edge
                .metadata
                .evidence
                .push(format!("{}:{}", db.source_file, db.source_line));
            write_edge.metadata.discovered_at = chrono::Utc::now();
            let _ = self.graph.upsert_edge(write_edge);
        }
    }

    /// Add a queue operation discovery, creating a Queue node and edge.
    fn add_queue_operation(&mut self, service_id: &NodeId, queue: QueueOperationDiscovery) {
        let queue_name = queue
            .queue_name
            .clone()
            .unwrap_or_else(|| format!("{}-unknown", queue.queue_type));
        let namespace = self
            .current_repo
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let queue_id = if let Some(id) = self.resource_map.get(&queue_name) {
            // Update the timestamp to mark it as recently surveyed
            if let Some(node) = self.graph.get_node_mut(id) {
                node.metadata.updated_at = chrono::Utc::now();
            }
            id.clone()
        } else {
            let id = NodeId::new(NodeType::Queue, &namespace, &queue_name)
                .expect("Failed to create queue NodeId");

            let mut node = NodeBuilder::new()
                .id(id.clone())
                .node_type(NodeType::Queue)
                .display_name(&queue_name)
                .attribute("queue_type", queue.queue_type.clone())
                .source(DiscoverySource::JavaScriptParser)
                .source_file(queue.source_file.clone())
                .source_line(queue.source_line)
                .build()
                .expect("Failed to build queue node");

            // Add deployment metadata if present
            if let Some(ref metadata) = queue.deployment_metadata {
                node.attributes.insert(
                    "deployment_method".to_string(),
                    AttributeValue::String(metadata.deployment_method.clone()),
                );
                if let Some(ref workspace) = metadata.terraform_workspace {
                    node.attributes.insert(
                        "terraform_workspace".to_string(),
                        AttributeValue::String(workspace.clone()),
                    );
                }
                if let Some(ref env) = metadata.environment {
                    node.attributes.insert(
                        "environment".to_string(),
                        AttributeValue::String(env.clone()),
                    );
                }
                if let Some(ref stack_name) = metadata.stack_name {
                    node.attributes.insert(
                        "stack_name".to_string(),
                        AttributeValue::String(stack_name.clone()),
                    );
                }
            }

            // Inject environment context from forge.yaml if not already set
            if !node.attributes.contains_key("environment") {
                if let Some(env) = &self.current_environment {
                    node.attributes.insert(
                        "environment".to_string(),
                        AttributeValue::String(env.clone()),
                    );
                }
            }
            if let Some(account_id) = &self.current_aws_account_id {
                node.attributes.insert(
                    "aws_account_id".to_string(),
                    AttributeValue::String(account_id.clone()),
                );
            }

            self.graph.upsert_node(node);
            self.resource_map.insert(queue_name, id.clone());
            id
        };

        let edge_type = match queue.operation {
            QueueOperationType::Publish => EdgeType::Publishes,
            QueueOperationType::Subscribe => EdgeType::Subscribes,
            QueueOperationType::Unknown => EdgeType::Publishes, // Default to publish for unknown
        };

        let mut edge = Edge::new(service_id.clone(), queue_id, edge_type)
            .expect("Failed to create queue edge");
        edge.metadata
            .evidence
            .push(format!("{}:{}", queue.source_file, queue.source_line));
        edge.metadata.discovered_at = chrono::Utc::now();
        let _ = self.graph.upsert_edge(edge);
    }

    /// Add a cloud resource usage discovery, creating a CloudResource node and edge.
    fn add_cloud_resource(&mut self, service_id: &NodeId, resource: CloudResourceDiscovery) {
        let resource_name = resource
            .resource_name
            .clone()
            .unwrap_or_else(|| format!("{}-unknown", resource.resource_type));
        let namespace = self
            .current_repo
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let resource_id = if let Some(id) = self.resource_map.get(&resource_name) {
            // Update the timestamp to mark it as recently surveyed
            if let Some(node) = self.graph.get_node_mut(id) {
                node.metadata.updated_at = chrono::Utc::now();
            }
            id.clone()
        } else {
            let id = NodeId::new(NodeType::CloudResource, &namespace, &resource_name)
                .expect("Failed to create cloud resource NodeId");

            let mut node = NodeBuilder::new()
                .id(id.clone())
                .node_type(NodeType::CloudResource)
                .display_name(&resource_name)
                .attribute("resource_type", resource.resource_type.clone())
                .source(DiscoverySource::JavaScriptParser)
                .source_file(resource.source_file.clone())
                .source_line(resource.source_line)
                .build()
                .expect("Failed to build cloud resource node");

            // Add deployment metadata if present
            if let Some(ref metadata) = resource.deployment_metadata {
                node.attributes.insert(
                    "deployment_method".to_string(),
                    AttributeValue::String(metadata.deployment_method.clone()),
                );
                if let Some(ref workspace) = metadata.terraform_workspace {
                    node.attributes.insert(
                        "terraform_workspace".to_string(),
                        AttributeValue::String(workspace.clone()),
                    );
                }
                if let Some(ref env) = metadata.environment {
                    node.attributes.insert(
                        "environment".to_string(),
                        AttributeValue::String(env.clone()),
                    );
                }
                if let Some(ref stack_name) = metadata.stack_name {
                    node.attributes.insert(
                        "stack_name".to_string(),
                        AttributeValue::String(stack_name.clone()),
                    );
                }
            }

            // Inject environment context from forge.yaml if not already set
            if !node.attributes.contains_key("environment") {
                if let Some(env) = &self.current_environment {
                    node.attributes.insert(
                        "environment".to_string(),
                        AttributeValue::String(env.clone()),
                    );
                }
            }
            if let Some(account_id) = &self.current_aws_account_id {
                node.attributes.insert(
                    "aws_account_id".to_string(),
                    AttributeValue::String(account_id.clone()),
                );
            }

            self.graph.upsert_node(node);
            self.resource_map.insert(resource_name, id.clone());
            id
        };

        let mut edge = Edge::new(service_id.clone(), resource_id, EdgeType::Uses)
            .expect("Failed to create resource edge");
        edge.metadata
            .evidence
            .push(format!("{}:{}", resource.source_file, resource.source_line));
        edge.metadata.discovered_at = chrono::Utc::now();
        let _ = self.graph.upsert_edge(edge);
    }

    /// Get the built graph, consuming the builder.
    pub fn build(self) -> ForgeGraph {
        self.graph
    }

    /// Get a reference to the graph without consuming the builder.
    pub fn graph(&self) -> &ForgeGraph {
        &self.graph
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ImportDiscovery, ServiceDiscovery};

    #[test]
    fn test_new_graph_builder() {
        let builder = GraphBuilder::new();
        assert_eq!(builder.graph().node_count(), 0);
        assert_eq!(builder.graph().edge_count(), 0);
    }

    #[test]
    fn test_add_service() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", Some("abc123"));

        let discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "src/index.ts".to_string(),
            framework: Some("express".to_string()),
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(discovery);
        assert_eq!(builder.graph().node_count(), 1);

        let node = builder.graph().get_node(&service_id).unwrap();
        assert_eq!(node.display_name, "user-service");
        assert_eq!(node.node_type, NodeType::Service);
    }

    #[test]
    fn test_add_service_deduplication() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);

        let discovery1 = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let discovery2 = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "javascript".to_string(),
            entry_point: "index.js".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let id1 = builder.add_service(discovery1);
        let id2 = builder.add_service(discovery2);

        // Should return the same ID
        assert_eq!(id1, id2);
        // Should only have one node
        assert_eq!(builder.graph().node_count(), 1);
    }

    #[test]
    fn test_add_database_access() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);

        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(service_discovery);

        let db_discovery = DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some("users-table".to_string()),
            operation: DatabaseOperation::Read,
            detection_method: "aws-sdk".to_string(),
            source_file: "src/db.ts".to_string(),
            source_line: 42,
            deployment_metadata: None,
        };

        builder.add_database_access(&service_id, db_discovery);

        // Should have service + database nodes
        assert_eq!(builder.graph().node_count(), 2);
        // Should have one edge
        assert_eq!(builder.graph().edge_count(), 1);

        // Verify edge type
        let edges = builder.graph().edges_from(&service_id);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::Reads);
    }

    #[test]
    fn test_database_readwrite_creates_two_edges() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);

        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(service_discovery);

        let db_discovery = DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some("users-table".to_string()),
            operation: DatabaseOperation::ReadWrite,
            detection_method: "aws-sdk".to_string(),
            source_file: "src/db.ts".to_string(),
            source_line: 42,
            deployment_metadata: None,
        };

        builder.add_database_access(&service_id, db_discovery);

        // Should have 2 edges (Read + Write)
        assert_eq!(builder.graph().edge_count(), 2);
    }

    #[test]
    fn test_process_discoveries() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);

        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(service_discovery.clone());

        let discoveries = vec![
            Discovery::Service(service_discovery),
            Discovery::Import(ImportDiscovery {
                module: "express".to_string(),
                is_relative: false,
                imported_items: vec![],
                source_file: "src/index.ts".to_string(),
                source_line: 1,
            }),
            Discovery::DatabaseAccess(DatabaseAccessDiscovery {
                db_type: "dynamodb".to_string(),
                table_name: Some("users-table".to_string()),
                operation: DatabaseOperation::Write,
                detection_method: "aws-sdk".to_string(),
                source_file: "src/db.ts".to_string(),
                source_line: 10,
                deployment_metadata: None,
            }),
        ];

        builder.process_discoveries(discoveries, &service_id);

        // Should have service + database
        assert_eq!(builder.graph().node_count(), 2);
        // Should have one edge (service -> database)
        assert_eq!(builder.graph().edge_count(), 1);
    }

    #[test]
    fn test_set_environment_injects_attributes() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);
        builder.set_environment("production", Some("123456789012"));

        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(service_discovery);

        let node = builder.graph().get_node(&service_id).unwrap();

        // Check environment attribute was injected
        assert_eq!(
            node.attributes.get("environment"),
            Some(&AttributeValue::String("production".to_string()))
        );

        // Check AWS account ID was injected
        assert_eq!(
            node.attributes.get("aws_account_id"),
            Some(&AttributeValue::String("123456789012".to_string()))
        );
    }

    #[test]
    fn test_environment_does_not_override_deployment_metadata() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);
        builder.set_environment("production", Some("123456789012"));

        // Service discovery with existing deployment metadata environment
        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: Some(crate::parser::DeploymentMetadata {
                deployment_method: "terraform".to_string(),
                terraform_workspace: Some("staging".to_string()),
                environment: Some("staging".to_string()), // This should be preserved
                stack_name: None,
                tags: HashMap::new(),
            }),
        };

        let service_id = builder.add_service(service_discovery);

        let node = builder.graph().get_node(&service_id).unwrap();

        // Environment from deployment_metadata should be preserved, not overwritten
        assert_eq!(
            node.attributes.get("environment"),
            Some(&AttributeValue::String("staging".to_string()))
        );

        // AWS account ID should still be injected
        assert_eq!(
            node.attributes.get("aws_account_id"),
            Some(&AttributeValue::String("123456789012".to_string()))
        );
    }

    #[test]
    fn test_clear_environment() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);
        builder.set_environment("production", Some("123456789012"));

        // Clear environment
        builder.clear_environment();

        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(service_discovery);

        let node = builder.graph().get_node(&service_id).unwrap();

        // No environment attribute should be set
        assert!(node.attributes.get("environment").is_none());
        assert!(node.attributes.get("aws_account_id").is_none());
    }

    #[test]
    fn test_environment_injected_into_database() {
        let mut builder = GraphBuilder::new();
        builder.set_repo_context("test-org/test-repo", None);
        builder.set_environment("production", Some("123456789012"));

        let service_discovery = ServiceDiscovery {
            name: "user-service".to_string(),
            language: "typescript".to_string(),
            entry_point: "index.ts".to_string(),
            framework: None,
            source_file: "package.json".to_string(),
            source_line: 1,
            deployment_metadata: None,
        };

        let service_id = builder.add_service(service_discovery);

        let db_discovery = DatabaseAccessDiscovery {
            db_type: "dynamodb".to_string(),
            table_name: Some("users-table".to_string()),
            operation: DatabaseOperation::Read,
            detection_method: "aws-sdk".to_string(),
            source_file: "src/db.ts".to_string(),
            source_line: 42,
            deployment_metadata: None,
        };

        builder.add_database_access(&service_id, db_discovery);

        // Find the database node
        let db_id = NodeId::new(NodeType::Database, "test-org/test-repo", "users-table").unwrap();
        let db_node = builder.graph().get_node(&db_id).unwrap();

        // Check environment was injected
        assert_eq!(
            db_node.attributes.get("environment"),
            Some(&AttributeValue::String("production".to_string()))
        );
        assert_eq!(
            db_node.attributes.get("aws_account_id"),
            Some(&AttributeValue::String("123456789012".to_string()))
        );
    }
}
