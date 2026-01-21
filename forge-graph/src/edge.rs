//! Edge types and structures for the knowledge graph.

use crate::error::EdgeError;
use crate::node::{NodeId, NodeType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
            EdgeType::ReadsShared | EdgeType::WritesShared => {
                &[NodeType::Database, NodeType::Queue]
            }
            EdgeType::ImplicitlyCoupled => &[NodeType::Service],
        }
    }
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
    pub discovered_at: DateTime<Utc>,

    /// Whether this edge was manually confirmed
    #[serde(default)]
    pub confirmed: bool,
}

impl EdgeMetadata {
    /// Create new EdgeMetadata with current timestamp.
    pub fn new() -> Self {
        Self {
            discovered_at: Utc::now(),
            ..Default::default()
        }
    }

    /// Set the confidence score.
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

    /// Set the reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Add evidence.
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence.push(evidence.into());
        self
    }

    /// Set HTTP method.
    pub fn with_http_method(mut self, method: impl Into<String>) -> Self {
        self.http_method = Some(method.into());
        self
    }

    /// Set endpoint path.
    pub fn with_endpoint_path(mut self, path: impl Into<String>) -> Self {
        self.endpoint_path = Some(path.into());
        self
    }

    /// Mark as confirmed.
    pub fn with_confirmed(mut self, confirmed: bool) -> Self {
        self.confirmed = confirmed;
        self
    }
}

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

impl Edge {
    /// Create a new edge with validation.
    pub fn new(source: NodeId, target: NodeId, edge_type: EdgeType) -> Result<Self, EdgeError> {
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
            metadata: EdgeMetadata::new(),
        })
    }

    /// Create a new edge without validation (for internal use or deserialization).
    pub fn new_unchecked(source: NodeId, target: NodeId, edge_type: EdgeType) -> Self {
        Self {
            source,
            target,
            edge_type,
            metadata: EdgeMetadata::new(),
        }
    }

    /// Set metadata on this edge.
    pub fn with_metadata(mut self, metadata: EdgeMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_create_valid_edge() {
        let edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::Service, "ns", "b").unwrap(),
            EdgeType::Calls,
        )
        .unwrap();

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
    fn test_invalid_edge_target_type() {
        let result = Edge::new(
            NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
            NodeId::new(NodeType::Queue, "ns", "queue").unwrap(),
            EdgeType::Reads, // Cannot READ from a Queue
        );

        assert!(matches!(result, Err(EdgeError::InvalidTargetType { .. })));
    }

    #[test]
    fn test_edge_serialization() {
        let edge = Edge::new(
            NodeId::new(NodeType::Service, "ns", "a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "db").unwrap(),
            EdgeType::Reads,
        )
        .unwrap();

        let json = serde_json::to_string(&edge).unwrap();
        assert!(json.contains("\"type\":\"READS\""));
    }

    #[test]
    fn test_edge_deserialization() {
        let json = r#"{
            "source": "service:ns:a",
            "target": "database:ns:db",
            "type": "WRITES",
            "metadata": {
                "discovered_at": "2024-01-01T00:00:00Z",
                "confirmed": false
            }
        }"#;

        let edge: Edge = serde_json::from_str(json).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Writes);
        assert_eq!(edge.source.as_str(), "service:ns:a");
    }

    #[test]
    fn test_edge_type_directional() {
        assert!(EdgeType::Calls.is_directional());
        assert!(EdgeType::Reads.is_directional());
        assert!(!EdgeType::ImplicitlyCoupled.is_directional());
    }

    #[test]
    fn test_all_valid_edge_types() {
        // Service -> Service (Calls)
        assert!(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "a").unwrap(),
                NodeId::new(NodeType::Service, "ns", "b").unwrap(),
                EdgeType::Calls,
            )
            .is_ok()
        );

        // Service -> API (Calls)
        assert!(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
                NodeId::new(NodeType::Api, "ns", "api").unwrap(),
                EdgeType::Calls,
            )
            .is_ok()
        );

        // Service -> API (Owns)
        assert!(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
                NodeId::new(NodeType::Api, "ns", "api").unwrap(),
                EdgeType::Owns,
            )
            .is_ok()
        );

        // Service -> Database (Reads)
        assert!(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
                NodeId::new(NodeType::Database, "ns", "db").unwrap(),
                EdgeType::Reads,
            )
            .is_ok()
        );

        // Service -> Queue (Publishes)
        assert!(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
                NodeId::new(NodeType::Queue, "ns", "queue").unwrap(),
                EdgeType::Publishes,
            )
            .is_ok()
        );

        // Service -> CloudResource (Uses)
        assert!(
            Edge::new(
                NodeId::new(NodeType::Service, "ns", "svc").unwrap(),
                NodeId::new(NodeType::CloudResource, "ns", "bucket").unwrap(),
                EdgeType::Uses,
            )
            .is_ok()
        );
    }

    #[test]
    fn test_edge_metadata_builder() {
        let metadata = EdgeMetadata::new()
            .with_confidence(0.95)
            .with_reason("Detected via HTTP call")
            .with_evidence("src/api.ts:42")
            .with_http_method("GET")
            .with_endpoint_path("/users")
            .with_confirmed(true);

        assert_eq!(metadata.confidence, Some(0.95));
        assert_eq!(metadata.reason, Some("Detected via HTTP call".to_string()));
        assert_eq!(metadata.evidence, vec!["src/api.ts:42"]);
        assert_eq!(metadata.http_method, Some("GET".to_string()));
        assert_eq!(metadata.endpoint_path, Some("/users".to_string()));
        assert!(metadata.confirmed);
    }
}
