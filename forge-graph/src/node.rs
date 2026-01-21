//! Node types and structures for the knowledge graph.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

impl NodeType {
    /// Get the string representation used in NodeId.
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Service => "service",
            NodeType::Api => "api",
            NodeType::Database => "database",
            NodeType::Queue => "queue",
            NodeType::CloudResource => "cloud_resource",
        }
    }
}

impl std::str::FromStr for NodeType {
    type Err = NodeIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "service" => Ok(NodeType::Service),
            "api" => Ok(NodeType::Api),
            "database" => Ok(NodeType::Database),
            "queue" => Ok(NodeType::Queue),
            "cloud_resource" => Ok(NodeType::CloudResource),
            _ => Err(NodeIdError::InvalidType(s.to_string())),
        }
    }
}

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
        Ok(Self(format!(
            "{}:{}:{}",
            node_type.as_str(),
            namespace,
            name
        )))
    }

    /// Parse an existing NodeId string.
    pub fn parse(s: &str) -> Result<Self, NodeIdError> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(NodeIdError::InvalidFormat(s.to_string()));
        }
        // Validate the type portion
        parts[0].parse::<NodeType>()?;
        Ok(Self(s.to_string()))
    }

    /// Extract the node type from the ID.
    pub fn node_type(&self) -> NodeType {
        let type_str = self.0.split(':').next().unwrap();
        type_str.parse().expect("NodeId invariant violated")
    }

    /// Get the namespace portion.
    pub fn namespace(&self) -> &str {
        self.0.split(':').nth(1).unwrap()
    }

    /// Get the name portion.
    pub fn name(&self) -> &str {
        self.0.splitn(3, ':').nth(2).unwrap()
    }

    /// Get the full ID string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
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

impl From<&str> for AttributeValue {
    fn from(s: &str) -> Self {
        AttributeValue::String(s.to_string())
    }
}

impl From<String> for AttributeValue {
    fn from(s: String) -> Self {
        AttributeValue::String(s)
    }
}

impl From<i64> for AttributeValue {
    fn from(n: i64) -> Self {
        AttributeValue::Integer(n)
    }
}

impl From<i32> for AttributeValue {
    fn from(n: i32) -> Self {
        AttributeValue::Integer(n as i64)
    }
}

impl From<f64> for AttributeValue {
    fn from(n: f64) -> Self {
        AttributeValue::Float(n)
    }
}

impl From<bool> for AttributeValue {
    fn from(b: bool) -> Self {
        AttributeValue::Boolean(b)
    }
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

/// Where a node was discovered from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    /// Discovered from JavaScript/TypeScript code
    JavaScriptParser,
    /// Discovered from Python code
    PythonParser,
    /// Discovered from Terraform HCL
    TerraformParser,
    /// Manually added by user
    #[default]
    Manual,
    /// Inferred from coupling analysis
    CouplingAnalysis,
    /// Added during business context interview
    Interview,
}

/// Metadata tracking node discovery and updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    /// When this node was first discovered
    pub created_at: DateTime<Utc>,

    /// When this node was last updated by survey
    pub updated_at: DateTime<Utc>,

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

impl Default for NodeMetadata {
    fn default() -> Self {
        Self {
            created_at: Utc::now(),
            updated_at: Utc::now(),
            source: DiscoverySource::Manual,
            commit_sha: None,
            source_file: None,
            source_line: None,
        }
    }
}

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

/// Builder for constructing Node instances.
#[derive(Debug, Default)]
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
    /// Create a new NodeBuilder with default values.
    pub fn new() -> Self {
        Self {
            source: DiscoverySource::Manual,
            ..Default::default()
        }
    }

    /// Set the node ID.
    pub fn id(mut self, id: NodeId) -> Self {
        self.node_type = Some(id.node_type());
        self.id = Some(id);
        self
    }

    /// Set the node type.
    pub fn node_type(mut self, t: NodeType) -> Self {
        self.node_type = Some(t);
        self
    }

    /// Set the display name.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Add an attribute.
    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<AttributeValue>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Set the business context.
    pub fn business_context(mut self, ctx: BusinessContext) -> Self {
        self.business_context = Some(ctx);
        self
    }

    /// Set the discovery source.
    pub fn source(mut self, source: DiscoverySource) -> Self {
        self.source = source;
        self
    }

    /// Set the commit SHA.
    pub fn commit_sha(mut self, sha: impl Into<String>) -> Self {
        self.commit_sha = Some(sha.into());
        self
    }

    /// Set the source file path.
    pub fn source_file(mut self, path: impl Into<String>) -> Self {
        self.source_file = Some(path.into());
        self
    }

    /// Set the source line number.
    pub fn source_line(mut self, line: u32) -> Self {
        self.source_line = Some(line);
        self
    }

    /// Build the Node.
    pub fn build(self) -> Result<Node, NodeBuilderError> {
        let id = self.id.ok_or(NodeBuilderError::MissingId)?;
        let node_type = self.node_type.ok_or(NodeBuilderError::MissingType)?;
        let display_name = self
            .display_name
            .ok_or(NodeBuilderError::MissingDisplayName)?;

        let now = Utc::now();
        Ok(Node {
            id,
            node_type,
            display_name,
            attributes: self.attributes,
            business_context: self.business_context,
            metadata: NodeMetadata {
                created_at: now,
                updated_at: now,
                source: self.source,
                commit_sha: self.commit_sha,
                source_file: self.source_file,
                source_line: self.source_line,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod node_id_tests {
        use super::*;
        use pretty_assertions::assert_eq;

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

        #[test]
        fn test_parse_invalid_type() {
            let result = NodeId::parse("unknown:ns:name");
            assert!(matches!(result, Err(NodeIdError::InvalidType(_))));
        }

        #[test]
        fn test_empty_segment() {
            let result = NodeId::new(NodeType::Service, "", "name");
            assert!(matches!(result, Err(NodeIdError::EmptySegment)));
        }

        #[test]
        fn test_all_node_types() {
            let types = [
                (NodeType::Service, "service"),
                (NodeType::Api, "api"),
                (NodeType::Database, "database"),
                (NodeType::Queue, "queue"),
                (NodeType::CloudResource, "cloud_resource"),
            ];

            for (node_type, expected_str) in types {
                let id = NodeId::new(node_type, "ns", "name").unwrap();
                assert!(id.as_str().starts_with(expected_str));
                assert_eq!(id.node_type(), node_type);
            }
        }
    }

    mod node_tests {
        use super::*;
        use pretty_assertions::assert_eq;

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
        fn test_node_builder_missing_id() {
            let result = NodeBuilder::new()
                .node_type(NodeType::Service)
                .display_name("Test")
                .build();
            assert!(matches!(result, Err(NodeBuilderError::MissingId)));
        }

        #[test]
        fn test_node_builder_missing_display_name() {
            let result = NodeBuilder::new()
                .id(NodeId::new(NodeType::Service, "ns", "name").unwrap())
                .node_type(NodeType::Service)
                .build();
            assert!(matches!(result, Err(NodeBuilderError::MissingDisplayName)));
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

        #[test]
        fn test_attribute_value_from_impls() {
            let s: AttributeValue = "hello".into();
            assert_eq!(s, AttributeValue::String("hello".to_string()));

            let i: AttributeValue = 42i64.into();
            assert_eq!(i, AttributeValue::Integer(42));

            let b: AttributeValue = true.into();
            assert_eq!(b, AttributeValue::Boolean(true));

            let f: AttributeValue = 3.14f64.into();
            assert_eq!(f, AttributeValue::Float(3.14));
        }
    }
}
