# Milestone 1: Foundation Specification

> **Spec Version**: 1.0
> **Status**: Draft
> **Implements**: IMPLEMENTATION_PLAN.md § Milestone 1

---

## 1. Overview

### 1.1 Purpose

Establish the foundational project structure and implement the core knowledge graph data structures that will underpin all subsequent Forge functionality. This milestone delivers a working Cargo workspace with the `forge-graph` crate fully implemented and tested.

### 1.2 Success Criteria

1. `cargo build --workspace` compiles without errors or warnings
2. `cargo test --workspace` passes all unit tests with >90% coverage on forge-graph
3. Graph can store 10,000+ nodes with sub-second query performance
4. JSON serialization produces deterministic, human-readable output
5. CI pipeline runs on every push/PR

### 1.3 Non-Goals

- CLI implementation (Milestone 2)
- Any parsing or survey logic (Milestone 2+)
- LLM integration (Milestone 6)
- Performance optimization beyond baseline correctness

---

## 2. Project Structure

### 2.1 Cargo Workspace Layout

```
forge/
├── Cargo.toml                    # Workspace root
├── .github/
│   └── workflows/
│       └── ci.yml                # GitHub Actions CI
├── forge-graph/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                # Crate root, re-exports
│       ├── node.rs               # Node types and structures
│       ├── edge.rs               # Edge types and structures
│       ├── graph.rs              # ForgeGraph wrapper
│       ├── query.rs              # Query interface
│       └── error.rs              # Error types
├── forge-cli/
│   ├── Cargo.toml                # Stub only in M1
│   └── src/
│       └── main.rs               # Stub: prints "forge v0.1.0"
├── forge-survey/
│   ├── Cargo.toml                # Stub only in M1
│   └── src/
│       └── lib.rs                # Stub: empty module
└── forge-llm/
    ├── Cargo.toml                # Stub only in M1
    └── src/
        └── lib.rs                # Stub: empty module
```

### 2.2 Root Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "forge-cli",
    "forge-graph",
    "forge-survey",
    "forge-llm",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.75"
authors = ["Forge Contributors"]
license = "MIT OR Apache-2.0"
repository = "https://github.com/your-org/forge"

[workspace.dependencies]
# Shared dependencies with pinned versions
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
uuid = { version = "1.0", features = ["v4", "serde"] }
petgraph = "0.6"
tokio = { version = "1.0", features = ["full"] }
```

### 2.3 forge-graph/Cargo.toml

```toml
[package]
name = "forge-graph"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
description = "Knowledge graph data structures for Forge"

[dependencies]
petgraph = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
pretty_assertions = "1.4"
tempfile = "3.10"
```

---

## 3. Data Structures

### 3.1 Node Types

#### 3.1.1 NodeType Enum

```rust
/// The category of a node in the knowledge graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    /// A deployable service (Lambda, container, server process)
    Service,
    /// An HTTP endpoint or RPC interface
    Api,
    /// A database or table (DynamoDB, PostgreSQL, etc.)
    Database,
    /// A message queue or topic (SQS, SNS, EventBridge)
    Queue,
    /// Other cloud resources (S3 bucket, etc.)
    CloudResource,
}
```

#### 3.1.2 NodeId

```rust
/// Unique identifier for a node.
/// Format: "{type}:{namespace}:{name}" e.g., "service:my-org:user-api"
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    /// Create a new NodeId with validation.
    ///
    /// # Format
    /// `{type}:{namespace}:{name}` where:
    /// - type: one of service, api, database, queue, cloud_resource
    /// - namespace: typically org name or repo name
    /// - name: the resource identifier
    ///
    /// # Examples
    /// - `service:acme:user-api`
    /// - `database:acme:users-table`
    /// - `api:acme:user-api:/users/GET`
    pub fn new(node_type: NodeType, namespace: &str, name: &str) -> Result<Self, NodeIdError> {
        validate_segment(namespace)?;
        validate_segment(name)?;
        let type_str = match node_type {
            NodeType::Service => "service",
            NodeType::Api => "api",
            NodeType::Database => "database",
            NodeType::Queue => "queue",
            NodeType::CloudResource => "cloud_resource",
        };
        Ok(Self(format!("{}:{}:{}", type_str, namespace, name)))
    }

    /// Parse an existing NodeId string.
    pub fn parse(s: &str) -> Result<Self, NodeIdError> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(NodeIdError::InvalidFormat(s.to_string()));
        }
        // Validate the type portion
        match parts[0] {
            "service" | "api" | "database" | "queue" | "cloud_resource" => {}
            _ => return Err(NodeIdError::InvalidType(parts[0].to_string())),
        }
        Ok(Self(s.to_string()))
    }

    /// Extract the node type from the ID.
    pub fn node_type(&self) -> NodeType {
        match self.0.split(':').next().unwrap() {
            "service" => NodeType::Service,
            "api" => NodeType::Api,
            "database" => NodeType::Database,
            "queue" => NodeType::Queue,
            "cloud_resource" => NodeType::CloudResource,
            _ => unreachable!("NodeId invariant violated"),
        }
    }

    /// Get the namespace portion.
    pub fn namespace(&self) -> &str {
        self.0.split(':').nth(1).unwrap()
    }

    /// Get the name portion.
    pub fn name(&self) -> &str {
        self.0.split(':').nth(2).unwrap()
    }

    /// Get the full ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_segment(s: &str) -> Result<(), NodeIdError> {
    if s.is_empty() {
        return Err(NodeIdError::EmptySegment);
    }
    if s.contains(':') {
        return Err(NodeIdError::InvalidCharacter(':'));
    }
    if s.len() > 256 {
        return Err(NodeIdError::TooLong(s.len()));
    }
    Ok(())
}
```

#### 3.1.3 Node Structure

```rust
/// A node in the knowledge graph representing an entity in the ecosystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier
    pub id: NodeId,

    /// Node category
    #[serde(rename = "type")]
    pub node_type: NodeType,

    /// Human-readable display name
    pub display_name: String,

    /// Arbitrary key-value attributes
    /// Common keys by type:
    /// - Service: repo_url, language, framework, entry_point, owner
    /// - Api: path, method, request_schema, response_schema
    /// - Database: db_type, table_name, arn, region
    /// - Queue: queue_type, arn, region
    /// - CloudResource: resource_type, arn, region
    #[serde(default)]
    pub attributes: HashMap<String, AttributeValue>,

    /// Business context annotations (filled by interview)
    #[serde(default)]
    pub business_context: Option<BusinessContext>,

    /// Metadata about when this node was discovered/updated
    pub metadata: NodeMetadata,
}

/// Typed attribute values for node properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<AttributeValue>),
    Map(HashMap<String, AttributeValue>),
    Null,
}

/// Business context annotations from LLM interview.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BusinessContext {
    /// What business function does this serve?
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,

    /// Who owns this component?
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,

    /// Historical context / why was it built this way?
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,

    /// Known issues, gotchas, operational learnings
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gotchas: Vec<String>,

    /// Free-form additional notes
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub notes: HashMap<String, String>,
}

/// Metadata tracking node discovery and updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    /// When this node was first discovered
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When this node was last updated by survey
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// Source that discovered this node
    pub source: DiscoverySource,

    /// Git commit SHA when discovered (if from repo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,

    /// File path where discovered (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,

    /// Line number in source file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
}

/// Where a node was discovered from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    /// Discovered from JavaScript/TypeScript code
    JavaScriptParser,
    /// Discovered from Python code
    PythonParser,
    /// Discovered from Terraform HCL
    TerraformParser,
    /// Manually added by user
    Manual,
    /// Inferred from coupling analysis
    CouplingAnalysis,
    /// Added during business context interview
    Interview,
}
```

#### 3.1.4 Node Builder Pattern

```rust
/// Builder for constructing Node instances.
pub struct NodeBuilder {
    id: Option<NodeId>,
    node_type: Option<NodeType>,
    display_name: Option<String>,
    attributes: HashMap<String, AttributeValue>,
    business_context: Option<BusinessContext>,
    source: DiscoverySource,
    commit_sha: Option<String>,
    source_file: Option<String>,
    source_line: Option<u32>,
}

impl NodeBuilder {
    pub fn new() -> Self { ... }

    pub fn id(mut self, id: NodeId) -> Self { ... }
    pub fn node_type(mut self, t: NodeType) -> Self { ... }
    pub fn display_name(mut self, name: impl Into<String>) -> Self { ... }
    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<AttributeValue>) -> Self { ... }
    pub fn source(mut self, source: DiscoverySource) -> Self { ... }
    pub fn commit_sha(mut self, sha: impl Into<String>) -> Self { ... }
    pub fn source_file(mut self, path: impl Into<String>) -> Self { ... }
    pub fn source_line(mut self, line: u32) -> Self { ... }

    pub fn build(self) -> Result<Node, NodeBuilderError> { ... }
}
```

### 3.2 Edge Types

#### 3.2.1 EdgeType Enum

```rust
/// The type of relationship between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EdgeType {
    /// Service invokes another service via HTTP/RPC
    /// Direction: caller → callee
    Calls,

    /// Service defines/manages an API endpoint
    /// Direction: service → api
    Owns,

    /// Service reads from a database/table
    /// Direction: service → database
    Reads,

    /// Service writes to a database/table
    /// Direction: service → database
    Writes,

    /// Service publishes messages to a queue/topic
    /// Direction: service → queue
    Publishes,

    /// Service subscribes to messages from a queue/topic
    /// Direction: service → queue
    Subscribes,

    /// Service uses a cloud resource
    /// Direction: service → cloud_resource
    Uses,

    /// Service reads from a shared resource (owned by another service)
    /// Direction: service → database/queue (where owner != service)
    ReadsShared,

    /// Service writes to a shared resource (owned by another service)
    /// Direction: service → database/queue (where owner != service)
    WritesShared,

    /// Two services are coupled via shared resource without explicit contract
    /// Direction: bidirectional (service ↔ service)
    ImplicitlyCoupled,
}

impl EdgeType {
    /// Whether this edge type is directional (true) or bidirectional (false).
    pub fn is_directional(&self) -> bool {
        !matches!(self, EdgeType::ImplicitlyCoupled)
    }

    /// Valid source node types for this edge type.
    pub fn valid_source_types(&self) -> &[NodeType] {
        match self {
            EdgeType::Calls => &[NodeType::Service],
            EdgeType::Owns => &[NodeType::Service],
            EdgeType::Reads | EdgeType::Writes => &[NodeType::Service],
            EdgeType::Publishes | EdgeType::Subscribes => &[NodeType::Service],
            EdgeType::Uses => &[NodeType::Service],
            EdgeType::ReadsShared | EdgeType::WritesShared => &[NodeType::Service],
            EdgeType::ImplicitlyCoupled => &[NodeType::Service],
        }
    }

    /// Valid target node types for this edge type.
    pub fn valid_target_types(&self) -> &[NodeType] {
        match self {
            EdgeType::Calls => &[NodeType::Service, NodeType::Api],
            EdgeType::Owns => &[NodeType::Api, NodeType::Database, NodeType::Queue],
            EdgeType::Reads | EdgeType::Writes => &[NodeType::Database],
            EdgeType::Publishes | EdgeType::Subscribes => &[NodeType::Queue],
            EdgeType::Uses => &[NodeType::CloudResource],
            EdgeType::ReadsShared | EdgeType::WritesShared => &[NodeType::Database, NodeType::Queue],
            EdgeType::ImplicitlyCoupled => &[NodeType::Service],
        }
    }
}
```

#### 3.2.2 Edge Structure

```rust
/// An edge representing a relationship between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Source node ID
    pub source: NodeId,

    /// Target node ID
    pub target: NodeId,

    /// Type of relationship
    #[serde(rename = "type")]
    pub edge_type: EdgeType,

    /// Additional metadata about the relationship
    #[serde(default)]
    pub metadata: EdgeMetadata,
}

/// Metadata about an edge relationship.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeMetadata {
    /// Confidence score (0.0 to 1.0) for inferred relationships
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,

    /// Human-readable reason for this relationship
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Source evidence (file:line where detected)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,

    /// For CALLS edges: the HTTP method if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_method: Option<String>,

    /// For CALLS edges: the endpoint path if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_path: Option<String>,

    /// When this edge was discovered
    pub discovered_at: chrono::DateTime<chrono::Utc>,

    /// Whether this edge was manually confirmed
    #[serde(default)]
    pub confirmed: bool,
}

impl Edge {
    /// Create a new edge with validation.
    pub fn new(
        source: NodeId,
        target: NodeId,
        edge_type: EdgeType,
    ) -> Result<Self, EdgeError> {
        // Validate source/target types match edge type constraints
        let source_type = source.node_type();
        let target_type = target.node_type();

        if !edge_type.valid_source_types().contains(&source_type) {
            return Err(EdgeError::InvalidSourceType {
                edge_type,
                actual: source_type,
                expected: edge_type.valid_source_types().to_vec(),
            });
        }

        if !edge_type.valid_target_types().contains(&target_type) {
            return Err(EdgeError::InvalidTargetType {
                edge_type,
                actual: target_type,
                expected: edge_type.valid_target_types().to_vec(),
            });
        }

        Ok(Self {
            source,
            target,
            edge_type,
            metadata: EdgeMetadata::default(),
        })
    }

    /// Set metadata on this edge.
    pub fn with_metadata(mut self, metadata: EdgeMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}
```

### 3.3 Graph Structure

#### 3.3.1 ForgeGraph

```rust
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use std::collections::HashMap;

/// The main knowledge graph container.
pub struct ForgeGraph {
    /// Underlying directed graph from petgraph
    inner: DiGraph<Node, Edge>,

    /// Index from NodeId to petgraph NodeIndex for O(1) lookup
    node_index: HashMap<NodeId, NodeIndex>,

    /// Graph metadata
    metadata: GraphMetadata,
}

/// Metadata about the graph itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    /// Forge version that created this graph
    pub forge_version: String,

    /// When the graph was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// When the graph was last modified
    pub modified_at: chrono::DateTime<chrono::Utc>,

    /// Number of surveys that have updated this graph
    pub survey_count: u32,

    /// Configuration used for last survey
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_survey_config: Option<serde_json::Value>,
}

impl ForgeGraph {
    /// Create a new empty graph.
    pub fn new() -> Self {
        Self {
            inner: DiGraph::new(),
            node_index: HashMap::new(),
            metadata: GraphMetadata {
                forge_version: env!("CARGO_PKG_VERSION").to_string(),
                created_at: chrono::Utc::now(),
                modified_at: chrono::Utc::now(),
                survey_count: 0,
                last_survey_config: None,
            },
        }
    }

    // === Node Operations ===

    /// Add a node to the graph.
    /// Returns error if a node with the same ID already exists.
    pub fn add_node(&mut self, node: Node) -> Result<NodeIndex, GraphError> {
        if self.node_index.contains_key(&node.id) {
            return Err(GraphError::DuplicateNode(node.id.clone()));
        }

        let id = node.id.clone();
        let idx = self.inner.add_node(node);
        self.node_index.insert(id, idx);
        self.metadata.modified_at = chrono::Utc::now();
        Ok(idx)
    }

    /// Add or update a node (upsert semantics).
    /// If node exists, merges attributes and updates metadata.
    pub fn upsert_node(&mut self, node: Node) -> NodeIndex {
        if let Some(&idx) = self.node_index.get(&node.id) {
            // Merge with existing node
            let existing = &mut self.inner[idx];
            for (k, v) in node.attributes {
                existing.attributes.insert(k, v);
            }
            existing.metadata.updated_at = chrono::Utc::now();
            if node.business_context.is_some() {
                existing.business_context = node.business_context;
            }
            idx
        } else {
            // Insert new node
            self.add_node(node).unwrap() // Safe: we just checked it doesn't exist
        }
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &NodeId) -> Option<&Node> {
        self.node_index.get(id).map(|&idx| &self.inner[idx])
    }

    /// Get a mutable reference to a node by ID.
    pub fn get_node_mut(&mut self, id: &NodeId) -> Option<&mut Node> {
        self.node_index.get(id).map(|&idx| &mut self.inner[idx])
    }

    /// Remove a node and all its edges.
    pub fn remove_node(&mut self, id: &NodeId) -> Option<Node> {
        if let Some(idx) = self.node_index.remove(id) {
            self.metadata.modified_at = chrono::Utc::now();
            self.inner.remove_node(idx)
        } else {
            None
        }
    }

    /// Check if a node exists.
    pub fn contains_node(&self, id: &NodeId) -> bool {
        self.node_index.contains_key(id)
    }

    /// Get count of nodes.
    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    /// Iterate over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.inner.node_weights()
    }

    /// Get all nodes of a specific type.
    pub fn nodes_by_type(&self, node_type: NodeType) -> impl Iterator<Item = &Node> {
        self.inner.node_weights().filter(move |n| n.node_type == node_type)
    }

    // === Edge Operations ===

    /// Add an edge to the graph.
    /// Validates that source and target nodes exist.
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), GraphError> {
        let source_idx = self.node_index.get(&edge.source)
            .ok_or_else(|| GraphError::NodeNotFound(edge.source.clone()))?;
        let target_idx = self.node_index.get(&edge.target)
            .ok_or_else(|| GraphError::NodeNotFound(edge.target.clone()))?;

        // Check for duplicate edge
        for existing_edge in self.inner.edges_connecting(*source_idx, *target_idx) {
            if existing_edge.weight().edge_type == edge.edge_type {
                return Err(GraphError::DuplicateEdge {
                    source: edge.source,
                    target: edge.target,
                    edge_type: edge.edge_type,
                });
            }
        }

        self.inner.add_edge(*source_idx, *target_idx, edge);
        self.metadata.modified_at = chrono::Utc::now();
        Ok(())
    }

    /// Add or update an edge (upsert semantics).
    pub fn upsert_edge(&mut self, edge: Edge) -> Result<(), GraphError> {
        let source_idx = self.node_index.get(&edge.source)
            .ok_or_else(|| GraphError::NodeNotFound(edge.source.clone()))?;
        let target_idx = self.node_index.get(&edge.target)
            .ok_or_else(|| GraphError::NodeNotFound(edge.target.clone()))?;

        // Find and update existing or add new
        let mut found = false;
        for existing_edge in self.inner.edges_connecting_mut(*source_idx, *target_idx) {
            if existing_edge.weight().edge_type == edge.edge_type {
                *existing_edge.weight_mut() = edge.clone();
                found = true;
                break;
            }
        }

        if !found {
            self.inner.add_edge(*source_idx, *target_idx, edge);
        }

        self.metadata.modified_at = chrono::Utc::now();
        Ok(())
    }

    /// Get all edges from a node.
    pub fn edges_from(&self, id: &NodeId) -> Vec<&Edge> {
        self.node_index.get(id)
            .map(|&idx| {
                self.inner.edges_directed(idx, Direction::Outgoing)
                    .map(|e| e.weight())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all edges to a node.
    pub fn edges_to(&self, id: &NodeId) -> Vec<&Edge> {
        self.node_index.get(id)
            .map(|&idx| {
                self.inner.edges_directed(idx, Direction::Incoming)
                    .map(|e| e.weight())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all edges of a specific type from a node.
    pub fn edges_from_by_type(&self, id: &NodeId, edge_type: EdgeType) -> Vec<&Edge> {
        self.edges_from(id)
            .into_iter()
            .filter(|e| e.edge_type == edge_type)
            .collect()
    }

    /// Get edge count.
    pub fn edge_count(&self) -> usize {
        self.inner.edge_count()
    }

    /// Iterate over all edges.
    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.inner.edge_weights()
    }
}
```

#### 3.3.2 Serialization

```rust
/// JSON-serializable representation of the graph.
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// Metadata about the graph
    pub metadata: GraphMetadata,

    /// All nodes in the graph
    pub nodes: Vec<Node>,

    /// All edges in the graph
    pub edges: Vec<Edge>,
}

impl ForgeGraph {
    /// Serialize the graph to a JSON file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), GraphError> {
        let snapshot = GraphSnapshot {
            metadata: self.metadata.clone(),
            nodes: self.inner.node_weights().cloned().collect(),
            edges: self.inner.edge_weights().cloned().collect(),
        };

        let file = std::fs::File::create(path.as_ref())
            .map_err(|e| GraphError::IoError(e))?;

        let writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &snapshot)
            .map_err(|e| GraphError::SerializationError(e.to_string()))?;

        Ok(())
    }

    /// Load a graph from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, GraphError> {
        let file = std::fs::File::open(path.as_ref())
            .map_err(|e| GraphError::IoError(e))?;

        let reader = std::io::BufReader::new(file);
        let snapshot: GraphSnapshot = serde_json::from_reader(reader)
            .map_err(|e| GraphError::DeserializationError(e.to_string()))?;

        let mut graph = Self::new();
        graph.metadata = snapshot.metadata;

        // Add all nodes first
        for node in snapshot.nodes {
            graph.add_node(node)?;
        }

        // Then add all edges
        for edge in snapshot.edges {
            graph.add_edge(edge)?;
        }

        Ok(graph)
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> Result<String, GraphError> {
        let snapshot = GraphSnapshot {
            metadata: self.metadata.clone(),
            nodes: self.inner.node_weights().cloned().collect(),
            edges: self.inner.edge_weights().cloned().collect(),
        };

        serde_json::to_string_pretty(&snapshot)
            .map_err(|e| GraphError::SerializationError(e.to_string()))
    }

    /// Deserialize from JSON string.
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let snapshot: GraphSnapshot = serde_json::from_str(json)
            .map_err(|e| GraphError::DeserializationError(e.to_string()))?;

        let mut graph = Self::new();
        graph.metadata = snapshot.metadata;

        for node in snapshot.nodes {
            graph.add_node(node)?;
        }

        for edge in snapshot.edges {
            graph.add_edge(edge)?;
        }

        Ok(graph)
    }
}
```

---

## 4. Query Interface

### 4.1 Query Functions

```rust
// In forge-graph/src/query.rs

impl ForgeGraph {
    /// Find all nodes connected to a given node via specific edge types.
    ///
    /// # Arguments
    /// * `node_id` - Starting node
    /// * `edge_types` - Edge types to follow (None = all types)
    /// * `direction` - Outgoing, Incoming, or Both
    pub fn traverse_edges(
        &self,
        node_id: &NodeId,
        edge_types: Option<&[EdgeType]>,
        direction: TraversalDirection,
    ) -> Vec<&Node> {
        let Some(&idx) = self.node_index.get(node_id) else {
            return vec![];
        };

        let mut result = Vec::new();

        let directions = match direction {
            TraversalDirection::Outgoing => vec![Direction::Outgoing],
            TraversalDirection::Incoming => vec![Direction::Incoming],
            TraversalDirection::Both => vec![Direction::Outgoing, Direction::Incoming],
        };

        for dir in directions {
            for edge_ref in self.inner.edges_directed(idx, dir) {
                let edge = edge_ref.weight();

                // Filter by edge type if specified
                if let Some(types) = edge_types {
                    if !types.contains(&edge.edge_type) {
                        continue;
                    }
                }

                // Get the connected node
                let connected_idx = match dir {
                    Direction::Outgoing => edge_ref.target(),
                    Direction::Incoming => edge_ref.source(),
                };

                result.push(&self.inner[connected_idx]);
            }
        }

        result
    }

    /// Find the shortest path between two nodes.
    /// Returns None if no path exists.
    pub fn find_path(
        &self,
        from: &NodeId,
        to: &NodeId,
    ) -> Option<Vec<&Node>> {
        use petgraph::algo::astar;

        let start_idx = *self.node_index.get(from)?;
        let goal_idx = *self.node_index.get(to)?;

        let result = astar(
            &self.inner,
            start_idx,
            |n| n == goal_idx,
            |_| 1, // uniform edge weight
            |_| 0, // no heuristic
        );

        result.map(|(_, path)| {
            path.iter().map(|&idx| &self.inner[idx]).collect()
        })
    }

    /// Extract a subgraph containing the specified nodes and all edges between them.
    pub fn get_subgraph(&self, node_ids: &[NodeId]) -> ForgeGraph {
        let mut subgraph = ForgeGraph::new();

        // Collect valid indices
        let indices: HashSet<_> = node_ids
            .iter()
            .filter_map(|id| self.node_index.get(id).copied())
            .collect();

        // Add nodes
        for &idx in &indices {
            subgraph.add_node(self.inner[idx].clone()).ok();
        }

        // Add edges where both endpoints are in the subgraph
        for edge in self.inner.edge_weights() {
            if let (Some(&src_idx), Some(&tgt_idx)) = (
                self.node_index.get(&edge.source),
                self.node_index.get(&edge.target),
            ) {
                if indices.contains(&src_idx) && indices.contains(&tgt_idx) {
                    subgraph.add_edge(edge.clone()).ok();
                }
            }
        }

        subgraph
    }

    /// Find all services that access a shared resource.
    pub fn services_accessing_resource(&self, resource_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(
            resource_id,
            Some(&[
                EdgeType::Reads,
                EdgeType::Writes,
                EdgeType::ReadsShared,
                EdgeType::WritesShared,
                EdgeType::Publishes,
                EdgeType::Subscribes,
                EdgeType::Uses,
            ]),
            TraversalDirection::Incoming,
        )
    }

    /// Get all services that a given service depends on (calls or uses).
    pub fn dependencies(&self, service_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(
            service_id,
            Some(&[EdgeType::Calls, EdgeType::Reads, EdgeType::Publishes, EdgeType::Uses]),
            TraversalDirection::Outgoing,
        )
    }

    /// Get all services that depend on a given service.
    pub fn dependents(&self, service_id: &NodeId) -> Vec<&Node> {
        self.traverse_edges(
            service_id,
            Some(&[EdgeType::Calls]),
            TraversalDirection::Incoming,
        )
    }

    /// Find all IMPLICITLY_COUPLED edges in the graph.
    pub fn implicit_couplings(&self) -> Vec<(&Node, &Node, &Edge)> {
        self.inner
            .edge_references()
            .filter(|e| e.weight().edge_type == EdgeType::ImplicitlyCoupled)
            .map(|e| {
                let source = &self.inner[e.source()];
                let target = &self.inner[e.target()];
                (source, target, e.weight())
            })
            .collect()
    }

    /// Search nodes by attribute value.
    pub fn find_nodes_by_attribute(
        &self,
        key: &str,
        value: &AttributeValue,
    ) -> Vec<&Node> {
        self.inner
            .node_weights()
            .filter(|n| n.attributes.get(key) == Some(value))
            .collect()
    }

    /// Search nodes by display name (case-insensitive substring).
    pub fn find_nodes_by_name(&self, query: &str) -> Vec<&Node> {
        let query_lower = query.to_lowercase();
        self.inner
            .node_weights()
            .filter(|n| n.display_name.to_lowercase().contains(&query_lower))
            .collect()
    }
}

/// Direction for edge traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Follow outgoing edges only
    Outgoing,
    /// Follow incoming edges only
    Incoming,
    /// Follow both directions
    Both,
}
```

---

## 5. Error Types

### 5.1 Error Definitions

```rust
// In forge-graph/src/error.rs

use thiserror::Error;

/// Errors related to NodeId operations.
#[derive(Debug, Error)]
pub enum NodeIdError {
    #[error("Invalid NodeId format: {0}")]
    InvalidFormat(String),

    #[error("Invalid node type: {0}")]
    InvalidType(String),

    #[error("NodeId segment cannot be empty")]
    EmptySegment,

    #[error("NodeId segment cannot contain character: {0}")]
    InvalidCharacter(char),

    #[error("NodeId segment too long: {0} characters (max 256)")]
    TooLong(usize),
}

/// Errors related to Node building.
#[derive(Debug, Error)]
pub enum NodeBuilderError {
    #[error("Node ID is required")]
    MissingId,

    #[error("Node type is required")]
    MissingType,

    #[error("Display name is required")]
    MissingDisplayName,

    #[error("NodeId error: {0}")]
    NodeId(#[from] NodeIdError),
}

/// Errors related to Edge operations.
#[derive(Debug, Error)]
pub enum EdgeError {
    #[error("Invalid source type for {edge_type:?}: got {actual:?}, expected one of {expected:?}")]
    InvalidSourceType {
        edge_type: EdgeType,
        actual: NodeType,
        expected: Vec<NodeType>,
    },

    #[error("Invalid target type for {edge_type:?}: got {actual:?}, expected one of {expected:?}")]
    InvalidTargetType {
        edge_type: EdgeType,
        actual: NodeType,
        expected: Vec<NodeType>,
    },
}

/// Errors related to Graph operations.
#[derive(Debug, Error)]
pub enum GraphError {
    #[error("Node already exists: {0:?}")]
    DuplicateNode(NodeId),

    #[error("Node not found: {0:?}")]
    NodeNotFound(NodeId),

    #[error("Edge already exists: {source:?} --{edge_type:?}--> {target:?}")]
    DuplicateEdge {
        source: NodeId,
        target: NodeId,
        edge_type: EdgeType,
    },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Edge error: {0}")]
    EdgeError(#[from] EdgeError),

    #[error("Node builder error: {0}")]
    NodeBuilderError(#[from] NodeBuilderError),
}
```

---

## 6. JSON Schema

### 6.1 Output Format Example

```json
{
  "metadata": {
    "forge_version": "0.1.0",
    "created_at": "2024-01-15T10:30:00Z",
    "modified_at": "2024-01-15T14:22:33Z",
    "survey_count": 3,
    "last_survey_config": null
  },
  "nodes": [
    {
      "id": "service:acme:user-api",
      "type": "service",
      "display_name": "User API",
      "attributes": {
        "repo_url": "https://github.com/acme/user-api",
        "language": "typescript",
        "framework": "express",
        "entry_point": "src/index.ts"
      },
      "business_context": {
        "purpose": "Handles user authentication and profile management",
        "owner": "Platform Team",
        "gotchas": [
          "Rate limited to 1000 req/min per user"
        ]
      },
      "metadata": {
        "created_at": "2024-01-15T10:30:00Z",
        "updated_at": "2024-01-15T14:22:33Z",
        "source": "javascript_parser",
        "commit_sha": "abc123def",
        "source_file": "src/index.ts",
        "source_line": 1
      }
    },
    {
      "id": "database:acme:users-table",
      "type": "database",
      "display_name": "Users Table",
      "attributes": {
        "db_type": "dynamodb",
        "table_name": "acme-users",
        "arn": "arn:aws:dynamodb:us-east-1:123456789:table/acme-users",
        "region": "us-east-1"
      },
      "business_context": null,
      "metadata": {
        "created_at": "2024-01-15T10:30:00Z",
        "updated_at": "2024-01-15T10:30:00Z",
        "source": "terraform_parser",
        "source_file": "terraform/dynamodb.tf",
        "source_line": 15
      }
    }
  ],
  "edges": [
    {
      "source": "service:acme:user-api",
      "target": "database:acme:users-table",
      "type": "READS",
      "metadata": {
        "confidence": 0.95,
        "evidence": [
          "src/db/users.ts:42"
        ],
        "discovered_at": "2024-01-15T10:30:00Z",
        "confirmed": false
      }
    },
    {
      "source": "service:acme:user-api",
      "target": "database:acme:users-table",
      "type": "WRITES",
      "metadata": {
        "confidence": 0.95,
        "evidence": [
          "src/db/users.ts:87"
        ],
        "discovered_at": "2024-01-15T10:30:00Z",
        "confirmed": false
      }
    }
  ]
}
```

---

## 7. CI/CD Pipeline

### 7.1 GitHub Actions Workflow

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  check:
    name: Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy lints
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Build
        run: cargo build --workspace --all-targets

      - name: Run tests
        run: cargo test --workspace --all-targets

      - name: Run doc tests
        run: cargo test --workspace --doc

  coverage:
    name: Coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install cargo-llvm-cov
        uses: taiki-e/install-action@cargo-llvm-cov

      - name: Generate coverage report
        run: cargo llvm-cov --workspace --lcov --output-path lcov.info

      - name: Upload coverage to Codecov
        uses: codecov/codecov-action@v4
        with:
          files: lcov.info
          fail_ci_if_error: false
```

---

## 8. Test Specifications

### 8.1 Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    mod node_id_tests {
        use super::*;

        #[test]
        fn test_create_valid_node_id() {
            let id = NodeId::new(NodeType::Service, "acme", "user-api").unwrap();
            assert_eq!(id.as_str(), "service:acme:user-api");
            assert_eq!(id.node_type(), NodeType::Service);
            assert_eq!(id.namespace(), "acme");
            assert_eq!(id.name(), "user-api");
        }

        #[test]
        fn test_node_id_with_colon_in_segment_fails() {
            let result = NodeId::new(NodeType::Service, "acme:corp", "api");
            assert!(matches!(result, Err(NodeIdError::InvalidCharacter(':'))));
        }

        #[test]
        fn test_parse_valid_node_id() {
            let id = NodeId::parse("database:acme:users-table").unwrap();
            assert_eq!(id.node_type(), NodeType::Database);
        }

        #[test]
        fn test_parse_invalid_format() {
            let result = NodeId::parse("invalid-format");
            assert!(matches!(result, Err(NodeIdError::InvalidFormat(_))));
        }
    }

    mod node_tests {
        use super::*;

        #[test]
        fn test_node_builder() {
            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "acme", "api").unwrap())
                .node_type(NodeType::Service)
                .display_name("Acme API")
                .attribute("language", "typescript")
                .source(DiscoverySource::JavaScriptParser)
                .build()
                .unwrap();

            assert_eq!(node.display_name, "Acme API");
            assert_eq!(
                node.attributes.get("language"),
                Some(&AttributeValue::String("typescript".to_string()))
            );
        }

        #[test]
        fn test_node_serialization_roundtrip() {
            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "svc").unwrap())
                .node_type(NodeType::Service)
                .display_name("Test Service")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();

            assert_eq!(node.id, deserialized.id);
            assert_eq!(node.display_name, deserialized.display_name);
        }
    }

    mod edge_tests {
        use super::*;

        #[test]
        fn test_create_valid_edge() {
            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", "a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "b").unwrap(),
                EdgeType::Calls,
            ).unwrap();

            assert_eq!(edge.edge_type, EdgeType::Calls);
        }

        #[test]
        fn test_invalid_edge_source_type() {
            let result = Edge::new(
                NodeId::new(NodeType::Database, "ns", "db").unwrap(),
                NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
                EdgeType::Calls, // Database cannot CALL
            );

            assert!(matches!(result, Err(EdgeError::InvalidSourceType { .. })));
        }

        #[test]
        fn test_edge_serialization() {
            let edge = Edge::new(
                NodeId::new(NodeType::Service, "ns", "a").unwrap(),
                NodeId::new(NodeType::Database, "ns", "db").unwrap(),
                EdgeType::Reads,
            ).unwrap();

            let json = serde_json::to_string(&edge).unwrap();
            assert!(json.contains("\"type\":\"READS\""));
        }
    }

    mod graph_tests {
        use super::*;

        fn create_test_graph() -> ForgeGraph {
            let mut graph = ForgeGraph::new();

            // Add services
            let svc_a = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "svc-a").unwrap())
                .node_type(NodeType::Service)
                .display_name("Service A")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            let svc_b = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "svc-b").unwrap())
                .node_type(NodeType::Service)
                .display_name("Service B")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            let db = NodeBuilder::new()
                .id(NodeId::new(NodeType::Database, "ns", "users-db").unwrap())
                .node_type(NodeType::Database)
                .display_name("Users DB")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            graph.add_node(svc_a).unwrap();
            graph.add_node(svc_b).unwrap();
            graph.add_node(db).unwrap();

            // Add edges
            graph.add_edge(Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
                EdgeType::Calls,
            ).unwrap()).unwrap();

            graph.add_edge(Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                NodeId::new(NodeType::Database, "ns", "users-db").unwrap(),
                EdgeType::Reads,
            ).unwrap()).unwrap();

            graph
        }

        #[test]
        fn test_add_and_get_node() {
            let mut graph = ForgeGraph::new();

            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
                .node_type(NodeType::Service)
                .display_name("Test")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            graph.add_node(node).unwrap();

            let id = NodeId::new(NodeType::Service, "ns", "test").unwrap();
            let retrieved = graph.get_node(&id).unwrap();
            assert_eq!(retrieved.display_name, "Test");
        }

        #[test]
        fn test_duplicate_node_error() {
            let mut graph = ForgeGraph::new();

            let node1 = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
                .node_type(NodeType::Service)
                .display_name("Test 1")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            let node2 = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
                .node_type(NodeType::Service)
                .display_name("Test 2")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            graph.add_node(node1).unwrap();
            let result = graph.add_node(node2);

            assert!(matches!(result, Err(GraphError::DuplicateNode(_))));
        }

        #[test]
        fn test_upsert_node_merges_attributes() {
            let mut graph = ForgeGraph::new();

            let node1 = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
                .node_type(NodeType::Service)
                .display_name("Test")
                .attribute("key1", "value1")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            graph.add_node(node1).unwrap();

            let node2 = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "test").unwrap())
                .node_type(NodeType::Service)
                .display_name("Test")
                .attribute("key2", "value2")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();

            graph.upsert_node(node2);

            let id = NodeId::new(NodeType::Service, "ns", "test").unwrap();
            let node = graph.get_node(&id).unwrap();

            assert!(node.attributes.contains_key("key1"));
            assert!(node.attributes.contains_key("key2"));
        }

        #[test]
        fn test_traverse_edges() {
            let graph = create_test_graph();

            let svc_a_id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
            let connected = graph.traverse_edges(
                &svc_a_id,
                None,
                TraversalDirection::Outgoing,
            );

            assert_eq!(connected.len(), 2); // svc-b and users-db
        }

        #[test]
        fn test_find_path() {
            let graph = create_test_graph();

            let from = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
            let to = NodeId::new(NodeType::Service, "ns", "svc-b").unwrap();

            let path = graph.find_path(&from, &to);
            assert!(path.is_some());
            assert_eq!(path.unwrap().len(), 2); // svc-a -> svc-b
        }

        #[test]
        fn test_get_subgraph() {
            let graph = create_test_graph();

            let ids = vec![
                NodeId::new(NodeType::Service, "ns", "svc-a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "svc-b").unwrap(),
            ];

            let subgraph = graph.get_subgraph(&ids);

            assert_eq!(subgraph.node_count(), 2);
            assert_eq!(subgraph.edge_count(), 1); // Only CALLS edge
        }

        #[test]
        fn test_persistence_roundtrip() {
            let graph = create_test_graph();
            let temp_dir = tempfile::tempdir().unwrap();
            let path = temp_dir.path().join("test_graph.json");

            graph.save_to_file(&path).unwrap();
            let loaded = ForgeGraph::load_from_file(&path).unwrap();

            assert_eq!(graph.node_count(), loaded.node_count());
            assert_eq!(graph.edge_count(), loaded.edge_count());

            // Verify specific node exists
            let id = NodeId::new(NodeType::Service, "ns", "svc-a").unwrap();
            assert!(loaded.get_node(&id).is_some());
        }

        #[test]
        fn test_nodes_by_type() {
            let graph = create_test_graph();

            let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
            let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();

            assert_eq!(services.len(), 2);
            assert_eq!(databases.len(), 1);
        }
    }
}
```

### 8.2 Performance Tests

```rust
#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore] // Run with: cargo test -- --ignored
    fn test_large_graph_performance() {
        let mut graph = ForgeGraph::new();

        // Add 10,000 nodes
        let start = Instant::now();
        for i in 0..10_000 {
            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", &format!("svc-{}", i)).unwrap())
                .node_type(NodeType::Service)
                .display_name(format!("Service {}", i))
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();
            graph.add_node(node).unwrap();
        }
        let node_add_time = start.elapsed();
        println!("Added 10,000 nodes in {:?}", node_add_time);
        assert!(node_add_time.as_secs() < 5, "Node addition too slow");

        // Add 50,000 edges (5 per node on average)
        let start = Instant::now();
        for i in 0..10_000 {
            for j in 0..5 {
                let target = (i + j + 1) % 10_000;
                if i != target {
                    let _ = graph.add_edge(Edge::new(
                        NodeId::new(NodeType::Service, "ns", &format!("svc-{}", i)).unwrap(),
                        NodeId::new(NodeType::Service, "ns", &format!("svc-{}", target)).unwrap(),
                        EdgeType::Calls,
                    ).unwrap());
                }
            }
        }
        let edge_add_time = start.elapsed();
        println!("Added edges in {:?}", edge_add_time);

        // Test query performance
        let start = Instant::now();
        for _ in 0..1000 {
            let id = NodeId::new(NodeType::Service, "ns", "svc-500").unwrap();
            let _ = graph.get_node(&id);
        }
        let query_time = start.elapsed();
        println!("1000 node lookups in {:?}", query_time);
        assert!(query_time.as_millis() < 100, "Node lookup too slow");

        // Test traversal performance
        let start = Instant::now();
        let id = NodeId::new(NodeType::Service, "ns", "svc-500").unwrap();
        let _ = graph.traverse_edges(&id, None, TraversalDirection::Outgoing);
        let traverse_time = start.elapsed();
        println!("Traverse edges in {:?}", traverse_time);
        assert!(traverse_time.as_millis() < 10, "Traversal too slow");
    }

    #[test]
    #[ignore]
    fn test_serialization_performance() {
        let mut graph = ForgeGraph::new();

        // Create moderately sized graph
        for i in 0..1000 {
            let node = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", &format!("svc-{}", i)).unwrap())
                .node_type(NodeType::Service)
                .display_name(format!("Service {}", i))
                .attribute("description", "A test service with some attributes")
                .source(DiscoverySource::Manual)
                .build()
                .unwrap();
            graph.add_node(node).unwrap();
        }

        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("perf_test.json");

        // Test save performance
        let start = Instant::now();
        graph.save_to_file(&path).unwrap();
        let save_time = start.elapsed();
        println!("Saved 1000-node graph in {:?}", save_time);
        assert!(save_time.as_secs() < 2, "Save too slow");

        // Test load performance
        let start = Instant::now();
        let _ = ForgeGraph::load_from_file(&path).unwrap();
        let load_time = start.elapsed();
        println!("Loaded 1000-node graph in {:?}", load_time);
        assert!(load_time.as_secs() < 2, "Load too slow");
    }
}
```

---

## 9. Implementation Checklist

### 9.1 Task Breakdown

| Task ID | Description | Status | Files |
|---------|-------------|--------|-------|
| M1-T1 | Initialize Cargo workspace | ☐ | `Cargo.toml`, `forge-*/Cargo.toml` |
| M1-T2 | Set up GitHub Actions CI | ☐ | `.github/workflows/ci.yml` |
| M1-T3 | Implement node types | ☐ | `forge-graph/src/node.rs` |
| M1-T4 | Implement edge types | ☐ | `forge-graph/src/edge.rs` |
| M1-T5 | Implement ForgeGraph wrapper | ☐ | `forge-graph/src/graph.rs` |
| M1-T6 | Implement query interface | ☐ | `forge-graph/src/query.rs` |
| M1-T7 | Implement JSON persistence | ☐ | `forge-graph/src/graph.rs` |
| M1-T8 | Write unit tests | ☐ | `forge-graph/src/lib.rs` |

### 9.2 Definition of Done

- [ ] All code compiles with `cargo build --workspace`
- [ ] All tests pass with `cargo test --workspace`
- [ ] No clippy warnings with `cargo clippy -- -D warnings`
- [ ] Code formatted with `cargo fmt`
- [ ] CI pipeline passes
- [ ] Documentation comments on all public items
- [ ] README updated with basic usage examples

---

## 10. Appendix

### 10.1 References

- [petgraph documentation](https://docs.rs/petgraph/)
- [serde documentation](https://serde.rs/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)

### 10.2 Glossary

| Term | Definition |
|------|------------|
| Node | A vertex in the knowledge graph representing an entity (service, database, etc.) |
| Edge | A directed connection between two nodes representing a relationship |
| NodeId | A unique string identifier in the format `{type}:{namespace}:{name}` |
| Subgraph | A portion of the graph containing a subset of nodes and their connecting edges |
| Coupling | A relationship between services, either explicit (API call) or implicit (shared resource) |
