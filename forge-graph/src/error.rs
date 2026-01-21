//! Error types for the forge-graph crate.

use crate::edge::EdgeType;
use crate::node::{NodeBuilderError, NodeType};
use thiserror::Error;

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
    #[error("Node already exists: {0}")]
    DuplicateNode(String),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Edge already exists: {source_node} --{edge_type:?}--> {target_node}")]
    DuplicateEdge {
        source_node: String,
        target_node: String,
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
