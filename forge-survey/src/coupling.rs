//! Implicit coupling detection for the knowledge graph.
//!
//! This module detects and models implicit coupling between services that share
//! resources without explicit API contracts. In enterprise polyrepo environments,
//! services often communicate implicitly through shared databases, queues, or
//! storage—creating hidden dependencies that can cause cascading failures.
//!
//! # Overview
//!
//! The coupling analyzer performs the following steps:
//! 1. **Build Access Map**: Scan the graph to track which services access which resources
//! 2. **Infer Ownership**: Determine resource ownership from Terraform, naming, or writes
//! 3. **Detect Couplings**: Find services sharing resources without explicit contracts
//! 4. **Generate Edges**: Create READS_SHARED, WRITES_SHARED, and IMPLICITLY_COUPLED edges
//!
//! # Example
//!
//! ```rust,ignore
//! use forge_survey::coupling::CouplingAnalyzer;
//!
//! let graph = /* ... build graph ... */;
//! let mut analyzer = CouplingAnalyzer::new(&graph);
//! let result = analyzer.analyze();
//! result.apply_to_graph(&mut graph)?;
//! ```

use forge_graph::{Edge, EdgeMetadata, EdgeType, ForgeGraph, GraphError, NodeId, NodeType};
use std::collections::{HashMap, HashSet};

/// Evidence for a resource access relationship.
///
/// Records where and how a service's access to a resource was detected,
/// providing traceability back to source code.
#[derive(Debug, Clone)]
pub struct AccessEvidence {
    /// Source file where access was detected
    pub source_file: String,

    /// Line number in the source file
    pub source_line: u32,

    /// Detection method (e.g., "boto3.get_item", "aws-sdk.query")
    pub detection_method: String,

    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
}

impl AccessEvidence {
    /// Create new access evidence.
    pub fn new(
        source_file: impl Into<String>,
        source_line: u32,
        detection_method: impl Into<String>,
        confidence: f64,
    ) -> Self {
        Self {
            source_file: source_file.into(),
            source_line,
            detection_method: detection_method.into(),
            confidence,
        }
    }
}

/// Tracks which services access which resources in the graph.
///
/// This is the core data structure for coupling analysis. It maintains:
/// - Which services read each resource
/// - Which services write to each resource
/// - Ownership assignments for resources
/// - Evidence for each access relationship
#[derive(Debug, Default)]
pub struct ResourceAccessMap {
    /// Map from resource NodeId to services that read it
    readers: HashMap<NodeId, HashSet<NodeId>>,

    /// Map from resource NodeId to services that write it
    writers: HashMap<NodeId, HashSet<NodeId>>,

    /// Map from resource NodeId to its owner (if known)
    owners: HashMap<NodeId, NodeId>,

    /// Evidence for each access relationship (service, resource) -> evidence list
    evidence: HashMap<(NodeId, NodeId), Vec<AccessEvidence>>,
}

impl ResourceAccessMap {
    /// Create a new empty resource access map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a read access to a resource.
    ///
    /// # Arguments
    /// * `service_id` - The service performing the read
    /// * `resource_id` - The resource being read
    /// * `evidence` - Evidence of this access
    pub fn record_read(
        &mut self,
        service_id: NodeId,
        resource_id: NodeId,
        evidence: AccessEvidence,
    ) {
        self.readers
            .entry(resource_id.clone())
            .or_default()
            .insert(service_id.clone());

        self.evidence
            .entry((service_id, resource_id))
            .or_default()
            .push(evidence);
    }

    /// Record a write access to a resource.
    ///
    /// # Arguments
    /// * `service_id` - The service performing the write
    /// * `resource_id` - The resource being written to
    /// * `evidence` - Evidence of this access
    pub fn record_write(
        &mut self,
        service_id: NodeId,
        resource_id: NodeId,
        evidence: AccessEvidence,
    ) {
        self.writers
            .entry(resource_id.clone())
            .or_default()
            .insert(service_id.clone());

        self.evidence
            .entry((service_id, resource_id))
            .or_default()
            .push(evidence);
    }

    /// Set the owner of a resource.
    ///
    /// Ownership is used to determine which accesses are "shared" vs "owned".
    /// A service accessing a resource it owns is not considered shared access.
    pub fn set_owner(&mut self, resource_id: NodeId, owner_id: NodeId) {
        self.owners.insert(resource_id, owner_id);
    }

    /// Get the owner of a resource, if known.
    pub fn get_owner(&self, resource_id: &NodeId) -> Option<&NodeId> {
        self.owners.get(resource_id)
    }

    /// Get all services that read a resource.
    pub fn get_readers(&self, resource_id: &NodeId) -> Vec<&NodeId> {
        self.readers
            .get(resource_id)
            .map(|s| s.iter().collect())
            .unwrap_or_default()
    }

    /// Get all services that write to a resource.
    pub fn get_writers(&self, resource_id: &NodeId) -> Vec<&NodeId> {
        self.writers
            .get(resource_id)
            .map(|s| s.iter().collect())
            .unwrap_or_default()
    }

    /// Get all unique resources in the map.
    pub fn resources(&self) -> HashSet<&NodeId> {
        self.readers.keys().chain(self.writers.keys()).collect()
    }

    /// Get the number of resources tracked.
    pub fn resource_count(&self) -> usize {
        self.resources().len()
    }

    /// Get evidence for a specific service-resource access.
    pub fn get_evidence(&self, service_id: &NodeId, resource_id: &NodeId) -> &[AccessEvidence] {
        self.evidence
            .get(&(service_id.clone(), resource_id.clone()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Check if a service reads a resource.
    pub fn is_reader(&self, service_id: &NodeId, resource_id: &NodeId) -> bool {
        self.readers
            .get(resource_id)
            .map(|readers| readers.contains(service_id))
            .unwrap_or(false)
    }

    /// Check if a service writes to a resource.
    pub fn is_writer(&self, service_id: &NodeId, resource_id: &NodeId) -> bool {
        self.writers
            .get(resource_id)
            .map(|writers| writers.contains(service_id))
            .unwrap_or(false)
    }

    /// Get all services that access a resource (readers + writers).
    pub fn get_accessors(&self, resource_id: &NodeId) -> HashSet<&NodeId> {
        let readers = self
            .readers
            .get(resource_id)
            .map(|s| s.iter().collect::<HashSet<_>>())
            .unwrap_or_default();
        let writers = self
            .writers
            .get(resource_id)
            .map(|s| s.iter().collect::<HashSet<_>>())
            .unwrap_or_default();
        readers.union(&writers).copied().collect()
    }
}

/// Risk level for implicit coupling between services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouplingRisk {
    /// Both services only read (low risk)
    Low,

    /// One service writes, others read (medium risk - schema changes)
    Medium,

    /// Multiple services write (high risk - race conditions, conflicts)
    High,
}

impl std::fmt::Display for CouplingRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CouplingRisk::Low => write!(f, "low"),
            CouplingRisk::Medium => write!(f, "medium"),
            CouplingRisk::High => write!(f, "high"),
        }
    }
}

/// An implicit coupling relationship between two services.
#[derive(Debug, Clone)]
pub struct ImplicitCoupling {
    /// First service in the coupling
    pub service_a: NodeId,

    /// Second service in the coupling
    pub service_b: NodeId,

    /// Resources shared between the services
    pub shared_resources: Vec<NodeId>,

    /// Human-readable explanation of the coupling
    pub reason: String,

    /// Risk level of this coupling
    pub risk_level: CouplingRisk,
}

/// Type of resource access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Service reads from the resource
    Read,
    /// Service writes to the resource
    Write,
}

/// A shared access relationship (non-owner accessing an owned resource).
#[derive(Debug, Clone)]
pub struct SharedAccess {
    /// Service accessing the resource
    pub service: NodeId,

    /// Resource being accessed
    pub resource: NodeId,

    /// Owner of the resource
    pub owner: NodeId,

    /// Type of access (read or write)
    pub access_type: AccessType,

    /// Evidence for this access
    pub evidence: Vec<AccessEvidence>,
}

/// Reason why ownership was assigned to a resource.
#[derive(Debug, Clone)]
pub enum OwnershipReason {
    /// Resource defined in owner's Terraform
    TerraformDefinition { file: String },

    /// Resource name matches service name pattern
    NamingConvention,

    /// Only this service writes to the resource
    ExclusiveWriter,

    /// Manually specified ownership
    Manual,

    /// Ownership from existing OWNS edge in graph
    ExistingOwnsEdge,
}

impl std::fmt::Debug for OwnershipAssignment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnershipAssignment")
            .field("resource", &self.resource)
            .field("owner", &self.owner)
            .field("reason", &self.reason)
            .field("confidence", &self.confidence)
            .finish()
    }
}

/// An ownership assignment for a resource.
#[derive(Clone)]
pub struct OwnershipAssignment {
    /// Resource being assigned
    pub resource: NodeId,

    /// Owner service
    pub owner: NodeId,

    /// Reason for the assignment
    pub reason: OwnershipReason,

    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
}

/// Result of coupling analysis on a graph.
#[derive(Debug, Default)]
pub struct CouplingAnalysisResult {
    /// IMPLICITLY_COUPLED edges to add
    pub implicit_couplings: Vec<ImplicitCoupling>,

    /// READS_SHARED edges to add
    pub shared_reads: Vec<SharedAccess>,

    /// WRITES_SHARED edges to add
    pub shared_writes: Vec<SharedAccess>,

    /// Ownership assignments inferred
    pub ownership_assignments: Vec<OwnershipAssignment>,
}

impl CouplingAnalysisResult {
    /// Create an empty result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the analysis results to a graph.
    ///
    /// This adds the inferred edges to the graph:
    /// - OWNS edges for ownership assignments
    /// - READS_SHARED edges for non-owner readers
    /// - WRITES_SHARED edges for non-owner writers
    /// - IMPLICITLY_COUPLED edges between coupled services
    pub fn apply_to_graph(&self, graph: &mut ForgeGraph) -> Result<(), GraphError> {
        // Add OWNS edges for inferred ownership
        for assignment in &self.ownership_assignments {
            // Only add if not already present
            let existing_owns = graph
                .edges_from(&assignment.owner)
                .iter()
                .any(|e| e.edge_type == EdgeType::Owns && e.target == assignment.resource);

            if !existing_owns {
                let edge = Edge::new(
                    assignment.owner.clone(),
                    assignment.resource.clone(),
                    EdgeType::Owns,
                )?;

                let edge = edge.with_metadata(EdgeMetadata {
                    confidence: Some(assignment.confidence),
                    reason: Some(format!("{:?}", assignment.reason)),
                    discovered_at: chrono::Utc::now(),
                    ..Default::default()
                });

                graph.upsert_edge(edge)?;
            }
        }

        // Add READS_SHARED edges
        for access in &self.shared_reads {
            let edge = Edge::new(
                access.service.clone(),
                access.resource.clone(),
                EdgeType::ReadsShared,
            )?;

            let metadata = EdgeMetadata {
                reason: Some(format!("Reads resource owned by {}", access.owner.name())),
                evidence: access
                    .evidence
                    .iter()
                    .map(|e| format!("{}:{}", e.source_file, e.source_line))
                    .collect(),
                discovered_at: chrono::Utc::now(),
                ..Default::default()
            };

            graph.upsert_edge(edge.with_metadata(metadata))?;
        }

        // Add WRITES_SHARED edges
        for access in &self.shared_writes {
            let edge = Edge::new(
                access.service.clone(),
                access.resource.clone(),
                EdgeType::WritesShared,
            )?;

            let metadata = EdgeMetadata {
                reason: Some(format!(
                    "Writes to resource owned by {}",
                    access.owner.name()
                )),
                evidence: access
                    .evidence
                    .iter()
                    .map(|e| format!("{}:{}", e.source_file, e.source_line))
                    .collect(),
                discovered_at: chrono::Utc::now(),
                ..Default::default()
            };

            graph.upsert_edge(edge.with_metadata(metadata))?;
        }

        // Add IMPLICITLY_COUPLED edges
        for coupling in &self.implicit_couplings {
            let edge = Edge::new(
                coupling.service_a.clone(),
                coupling.service_b.clone(),
                EdgeType::ImplicitlyCoupled,
            )?;

            let resources_str = coupling
                .shared_resources
                .iter()
                .filter_map(|r| graph.get_node(r).map(|n| n.display_name.clone()))
                .collect::<Vec<_>>()
                .join(", ");

            let metadata = EdgeMetadata {
                confidence: Some(match coupling.risk_level {
                    CouplingRisk::High => 0.95,
                    CouplingRisk::Medium => 0.8,
                    CouplingRisk::Low => 0.6,
                }),
                reason: Some(coupling.reason.clone()),
                evidence: vec![format!("Shared resources: {}", resources_str)],
                discovered_at: chrono::Utc::now(),
                ..Default::default()
            };

            // IMPLICITLY_COUPLED is bidirectional, but we store it once
            graph.upsert_edge(edge.with_metadata(metadata))?;
        }

        Ok(())
    }
}

/// Analyzes a graph for implicit coupling between services.
///
/// The analyzer scans the graph's edges to build a map of which services
/// access which resources, then detects implicit couplings where multiple
/// services share a resource without an explicit API contract.
pub struct CouplingAnalyzer<'a> {
    graph: &'a ForgeGraph,
    access_map: ResourceAccessMap,
}

impl<'a> CouplingAnalyzer<'a> {
    /// Create a new coupling analyzer for the given graph.
    pub fn new(graph: &'a ForgeGraph) -> Self {
        Self {
            graph,
            access_map: ResourceAccessMap::new(),
        }
    }

    /// Run the full coupling analysis pipeline.
    ///
    /// This performs the following steps:
    /// 1. Build resource access map from existing edges
    /// 2. Infer resource ownership
    /// 3. Detect implicit couplings
    /// 4. Generate shared access edges
    pub fn analyze(&mut self) -> CouplingAnalysisResult {
        // Step 1: Build resource access map from existing edges (M4-T1)
        self.build_access_map();

        // Step 2: Infer resource ownership (M4-T2)
        let ownership_assignments = self.infer_ownership();
        for assignment in &ownership_assignments {
            self.access_map
                .set_owner(assignment.resource.clone(), assignment.owner.clone());
        }

        // Step 3: Detect implicit couplings (M4-T3)
        let implicit_couplings = self.detect_implicit_couplings();

        // Step 4: Generate shared access edges (M4-T3)
        let (shared_reads, shared_writes) = self.generate_shared_access_edges();

        CouplingAnalysisResult {
            implicit_couplings,
            shared_reads,
            shared_writes,
            ownership_assignments,
        }
    }

    /// Get a reference to the built access map.
    ///
    /// Useful for testing or inspection.
    pub fn access_map(&self) -> &ResourceAccessMap {
        &self.access_map
    }

    /// Build the resource access map from graph edges.
    ///
    /// This scans all edges in the graph and records:
    /// - Service -> Resource reads (Reads, ReadsShared, Subscribes, Uses edges)
    /// - Service -> Resource writes (Writes, WritesShared, Publishes edges)
    /// - Ownership from OWNS edges
    fn build_access_map(&mut self) {
        for edge in self.graph.edges() {
            let source_node = match self.graph.get_node(&edge.source) {
                Some(n) => n,
                None => continue,
            };

            let target_node = match self.graph.get_node(&edge.target) {
                Some(n) => n,
                None => continue,
            };

            // Only track service -> resource relationships
            if source_node.node_type != NodeType::Service {
                continue;
            }

            let is_resource = matches!(
                target_node.node_type,
                NodeType::Database | NodeType::Queue | NodeType::CloudResource
            );

            if !is_resource {
                continue;
            }

            // Build evidence from edge metadata
            let evidence = AccessEvidence {
                source_file: edge
                    .metadata
                    .evidence
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
                source_line: 0,
                detection_method: format!("{:?}", edge.edge_type),
                confidence: edge.metadata.confidence.unwrap_or(1.0),
            };

            match edge.edge_type {
                EdgeType::Reads | EdgeType::ReadsShared | EdgeType::Subscribes => {
                    self.access_map
                        .record_read(edge.source.clone(), edge.target.clone(), evidence);
                }
                EdgeType::Writes | EdgeType::WritesShared | EdgeType::Publishes => {
                    self.access_map.record_write(
                        edge.source.clone(),
                        edge.target.clone(),
                        evidence,
                    );
                }
                EdgeType::Uses => {
                    // Generic use - treat as read
                    self.access_map
                        .record_read(edge.source.clone(), edge.target.clone(), evidence);
                }
                EdgeType::Owns => {
                    // This service owns the resource
                    self.access_map
                        .set_owner(edge.target.clone(), edge.source.clone());
                }
                _ => {}
            }
        }
    }

    /// Infer ownership of resources.
    ///
    /// Uses three strategies in order of confidence:
    /// 1. Terraform definition (0.9 confidence)
    /// 2. Naming convention (0.7 confidence)
    /// 3. Exclusive writer (0.6 confidence)
    fn infer_ownership(&self) -> Vec<OwnershipAssignment> {
        let mut assignments = Vec::new();

        for resource_id in self.access_map.resources() {
            // Skip if already has owner from OWNS edge
            if self.access_map.get_owner(resource_id).is_some() {
                continue;
            }

            if let Some(assignment) = self.infer_resource_owner(resource_id) {
                assignments.push(assignment);
            }
        }

        assignments
    }

    /// Infer the owner of a specific resource.
    fn infer_resource_owner(&self, resource_id: &NodeId) -> Option<OwnershipAssignment> {
        let resource = self.graph.get_node(resource_id)?;

        // Strategy 1: Check if resource was defined in Terraform
        if let Some(source_file) = &resource.metadata.source_file {
            if source_file.ends_with(".tf") {
                // Resource defined in Terraform - owner is the service in the same repo
                if let Some(owner) = self.find_service_in_same_repo(source_file) {
                    return Some(OwnershipAssignment {
                        resource: resource_id.clone(),
                        owner,
                        reason: OwnershipReason::TerraformDefinition {
                            file: source_file.clone(),
                        },
                        confidence: 0.9,
                    });
                }
            }
        }

        // Strategy 2: Naming convention
        // If resource name contains a service name, that service owns it
        let resource_name = &resource.display_name;
        for service in self.graph.nodes_by_type(NodeType::Service) {
            let service_name = &service.display_name;
            if resource_name.contains(service_name)
                || resource_name.starts_with(&format!("{}-", service_name))
                || resource_name.starts_with(&format!("{}_", service_name))
            {
                return Some(OwnershipAssignment {
                    resource: resource_id.clone(),
                    owner: service.id.clone(),
                    reason: OwnershipReason::NamingConvention,
                    confidence: 0.7,
                });
            }
        }

        // Strategy 3: Exclusive writer
        let writers = self.access_map.get_writers(resource_id);
        if writers.len() == 1 {
            return Some(OwnershipAssignment {
                resource: resource_id.clone(),
                owner: writers[0].clone(),
                reason: OwnershipReason::ExclusiveWriter,
                confidence: 0.6,
            });
        }

        None
    }

    /// Find a service in the same repository as a Terraform file.
    fn find_service_in_same_repo(&self, tf_file: &str) -> Option<NodeId> {
        // Extract repo name from file path
        // e.g., "/path/to/repos/user-service/terraform/main.tf" -> "user-service"
        let path = std::path::Path::new(tf_file);
        let components: Vec<_> = path.components().collect();

        for (i, component) in components.iter().enumerate() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_str()?;
                if name_str == "terraform" || name_str == "infra" {
                    // Look at parent directory
                    if i > 0 {
                        if let std::path::Component::Normal(repo_name) = components[i - 1] {
                            let repo_name_str = repo_name.to_str()?;
                            // Find matching service
                            for service in self.graph.nodes_by_type(NodeType::Service) {
                                if service.display_name == repo_name_str
                                    || service.id.name() == repo_name_str
                                {
                                    return Some(service.id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Detect implicit couplings between services.
    ///
    /// Services are implicitly coupled when they both access the same resource
    /// without an explicit API contract between them.
    ///
    /// Note: The owner IS included in coupling detection because they are still
    /// implicitly coupled with other services accessing the same resource.
    /// Owner exclusion only applies to READS_SHARED/WRITES_SHARED edge generation.
    fn detect_implicit_couplings(&self) -> Vec<ImplicitCoupling> {
        let mut couplings = Vec::new();
        let mut processed_pairs: HashSet<(NodeId, NodeId)> = HashSet::new();

        for resource_id in self.access_map.resources() {
            let readers = self.access_map.get_readers(resource_id);
            let writers = self.access_map.get_writers(resource_id);

            // All services accessing this resource (including owner - they are still coupled!)
            let all_services: HashSet<_> = readers.iter().chain(writers.iter()).cloned().collect();

            // Skip if only one service
            if all_services.len() <= 1 {
                continue;
            }

            // Create coupling between each pair of services (owner included)
            let services: Vec<_> = all_services.iter().cloned().collect();

            for i in 0..services.len() {
                for j in (i + 1)..services.len() {
                    let service_a = services[i];
                    let service_b = services[j];

                    // Normalize pair ordering for deduplication
                    let pair = if service_a.as_str() < service_b.as_str() {
                        (service_a.clone(), service_b.clone())
                    } else {
                        (service_b.clone(), service_a.clone())
                    };

                    if processed_pairs.contains(&pair) {
                        // Add this resource to existing coupling
                        if let Some(existing) =
                            couplings.iter_mut().find(|c: &&mut ImplicitCoupling| {
                                (c.service_a == pair.0 && c.service_b == pair.1)
                                    || (c.service_a == pair.1 && c.service_b == pair.0)
                            })
                        {
                            existing.shared_resources.push(resource_id.clone());
                        }
                        continue;
                    }

                    processed_pairs.insert(pair);

                    // Determine risk level
                    let a_writes = writers.contains(&service_a);
                    let b_writes = writers.contains(&service_b);

                    let risk_level = if a_writes && b_writes {
                        CouplingRisk::High
                    } else if a_writes || b_writes {
                        CouplingRisk::Medium
                    } else {
                        CouplingRisk::Low
                    };

                    // Generate reason
                    let resource = self
                        .graph
                        .get_node(resource_id)
                        .map(|n| n.display_name.clone())
                        .unwrap_or_else(|| "unknown".to_string());

                    let reason = match risk_level {
                        CouplingRisk::High => {
                            format!(
                                "Both services write to shared resource '{}' - potential race conditions",
                                resource
                            )
                        }
                        CouplingRisk::Medium => {
                            format!(
                                "Services share resource '{}' (one writes, one reads) - schema changes affect both",
                                resource
                            )
                        }
                        CouplingRisk::Low => {
                            format!(
                                "Services share read access to '{}' - changes to data may affect both",
                                resource
                            )
                        }
                    };

                    couplings.push(ImplicitCoupling {
                        service_a: service_a.clone(),
                        service_b: service_b.clone(),
                        shared_resources: vec![resource_id.clone()],
                        reason,
                        risk_level,
                    });
                }
            }
        }

        couplings
    }

    /// Generate READS_SHARED and WRITES_SHARED edges for non-owner accesses.
    fn generate_shared_access_edges(&self) -> (Vec<SharedAccess>, Vec<SharedAccess>) {
        let mut shared_reads = Vec::new();
        let mut shared_writes = Vec::new();

        for resource_id in self.access_map.resources() {
            let owner = match self.access_map.get_owner(resource_id) {
                Some(o) => o,
                None => continue, // Can't determine shared without owner
            };

            // Readers other than owner
            for reader in self.access_map.get_readers(resource_id) {
                if reader != owner {
                    shared_reads.push(SharedAccess {
                        service: reader.clone(),
                        resource: resource_id.clone(),
                        owner: owner.clone(),
                        access_type: AccessType::Read,
                        evidence: self.access_map.get_evidence(reader, resource_id).to_vec(),
                    });
                }
            }

            // Writers other than owner
            for writer in self.access_map.get_writers(resource_id) {
                if writer != owner {
                    shared_writes.push(SharedAccess {
                        service: writer.clone(),
                        resource: resource_id.clone(),
                        owner: owner.clone(),
                        access_type: AccessType::Write,
                        evidence: self.access_map.get_evidence(writer, resource_id).to_vec(),
                    });
                }
            }
        }

        (shared_reads, shared_writes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_graph::{DiscoverySource, NodeBuilder};

    /// Helper to create a test service node.
    fn create_service(name: &str, namespace: &str) -> forge_graph::Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, namespace, name).unwrap())
            .node_type(NodeType::Service)
            .display_name(name)
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }

    /// Helper to create a test database node.
    fn create_database(name: &str, namespace: &str) -> forge_graph::Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, namespace, name).unwrap())
            .node_type(NodeType::Database)
            .display_name(name)
            .attribute("db_type", "dynamodb")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap()
    }

    /// Helper to create a test queue node.
    fn create_queue(name: &str, namespace: &str) -> forge_graph::Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Queue, namespace, name).unwrap())
            .node_type(NodeType::Queue)
            .display_name(name)
            .attribute("queue_type", "sqs")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap()
    }

    mod resource_access_map_tests {
        use super::*;

        #[test]
        fn test_new_access_map_is_empty() {
            let map = ResourceAccessMap::new();
            assert_eq!(map.resource_count(), 0);
        }

        #[test]
        fn test_record_read_access() {
            let mut map = ResourceAccessMap::new();

            let service_id = NodeId::new(NodeType::Service, "ns", "service-a").unwrap();
            let resource_id = NodeId::new(NodeType::Database, "ns", "users-table").unwrap();
            let evidence = AccessEvidence::new("src/db.ts", 42, "aws-sdk.query", 1.0);

            map.record_read(service_id.clone(), resource_id.clone(), evidence);

            assert_eq!(map.resource_count(), 1);
            assert!(map.is_reader(&service_id, &resource_id));
            assert!(!map.is_writer(&service_id, &resource_id));
        }

        #[test]
        fn test_record_write_access() {
            let mut map = ResourceAccessMap::new();

            let service_id = NodeId::new(NodeType::Service, "ns", "service-a").unwrap();
            let resource_id = NodeId::new(NodeType::Database, "ns", "users-table").unwrap();
            let evidence = AccessEvidence::new("src/db.ts", 50, "aws-sdk.put", 1.0);

            map.record_write(service_id.clone(), resource_id.clone(), evidence);

            assert!(map.is_writer(&service_id, &resource_id));
            assert!(!map.is_reader(&service_id, &resource_id));
        }

        #[test]
        fn test_multiple_readers_same_resource() {
            let mut map = ResourceAccessMap::new();

            let service_a = NodeId::new(NodeType::Service, "ns", "service-a").unwrap();
            let service_b = NodeId::new(NodeType::Service, "ns", "service-b").unwrap();
            let resource_id = NodeId::new(NodeType::Database, "ns", "users-table").unwrap();

            map.record_read(
                service_a.clone(),
                resource_id.clone(),
                AccessEvidence::new("a.ts", 1, "read", 1.0),
            );
            map.record_read(
                service_b.clone(),
                resource_id.clone(),
                AccessEvidence::new("b.ts", 1, "read", 1.0),
            );

            let readers = map.get_readers(&resource_id);
            assert_eq!(readers.len(), 2);
            assert!(readers.contains(&&service_a));
            assert!(readers.contains(&&service_b));
        }

        #[test]
        fn test_set_and_get_owner() {
            let mut map = ResourceAccessMap::new();

            let service_id = NodeId::new(NodeType::Service, "ns", "owner-service").unwrap();
            let resource_id = NodeId::new(NodeType::Database, "ns", "my-table").unwrap();

            assert!(map.get_owner(&resource_id).is_none());

            map.set_owner(resource_id.clone(), service_id.clone());

            assert_eq!(map.get_owner(&resource_id), Some(&service_id));
        }

        #[test]
        fn test_get_accessors_combines_readers_and_writers() {
            let mut map = ResourceAccessMap::new();

            let service_a = NodeId::new(NodeType::Service, "ns", "reader").unwrap();
            let service_b = NodeId::new(NodeType::Service, "ns", "writer").unwrap();
            let service_c = NodeId::new(NodeType::Service, "ns", "readwriter").unwrap();
            let resource_id = NodeId::new(NodeType::Database, "ns", "table").unwrap();

            map.record_read(
                service_a.clone(),
                resource_id.clone(),
                AccessEvidence::new("a.ts", 1, "read", 1.0),
            );
            map.record_write(
                service_b.clone(),
                resource_id.clone(),
                AccessEvidence::new("b.ts", 1, "write", 1.0),
            );
            map.record_read(
                service_c.clone(),
                resource_id.clone(),
                AccessEvidence::new("c.ts", 1, "read", 1.0),
            );
            map.record_write(
                service_c.clone(),
                resource_id.clone(),
                AccessEvidence::new("c.ts", 2, "write", 1.0),
            );

            let accessors = map.get_accessors(&resource_id);
            assert_eq!(accessors.len(), 3);
        }

        #[test]
        fn test_evidence_is_recorded() {
            let mut map = ResourceAccessMap::new();

            let service_id = NodeId::new(NodeType::Service, "ns", "service").unwrap();
            let resource_id = NodeId::new(NodeType::Database, "ns", "table").unwrap();

            map.record_read(
                service_id.clone(),
                resource_id.clone(),
                AccessEvidence::new("file1.ts", 10, "query", 0.9),
            );
            map.record_read(
                service_id.clone(),
                resource_id.clone(),
                AccessEvidence::new("file2.ts", 20, "scan", 0.8),
            );

            let evidence = map.get_evidence(&service_id, &resource_id);
            assert_eq!(evidence.len(), 2);
            assert_eq!(evidence[0].source_file, "file1.ts");
            assert_eq!(evidence[1].source_file, "file2.ts");
        }
    }

    mod coupling_analyzer_tests {
        use super::*;

        fn create_test_graph_with_shared_db() -> ForgeGraph {
            let mut graph = ForgeGraph::new();

            // Create services
            graph.add_node(create_service("service-a", "ns")).unwrap();
            graph.add_node(create_service("service-b", "ns")).unwrap();
            graph.add_node(create_service("service-c", "ns")).unwrap();

            // Create shared database
            graph
                .add_node(create_database("users-table", "ns"))
                .unwrap();

            // Service A writes to DB
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "service-a").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            // Service B reads from DB
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "service-b").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            // Service C also reads from DB
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "service-c").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            graph
        }

        #[test]
        fn test_build_access_map_from_edges() {
            let graph = create_test_graph_with_shared_db();
            let mut analyzer = CouplingAnalyzer::new(&graph);

            analyzer.build_access_map();

            let map = analyzer.access_map();
            let db_id = NodeId::new(NodeType::Database, "ns", "users-table").unwrap();

            // Should have tracked the database
            assert_eq!(map.resource_count(), 1);

            // Should have one writer (service-a)
            let writers = map.get_writers(&db_id);
            assert_eq!(writers.len(), 1);
            assert_eq!(writers[0].name(), "service-a");

            // Should have two readers (service-b, service-c)
            let readers = map.get_readers(&db_id);
            assert_eq!(readers.len(), 2);
        }

        #[test]
        fn test_build_access_map_records_ownership() {
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("owner-svc", "ns")).unwrap();
            graph
                .add_node(create_database("owned-table", "ns"))
                .unwrap();

            // Add OWNS edge
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "owner-svc").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "owned-table").unwrap(),
                        EdgeType::Owns,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            analyzer.build_access_map();

            let db_id = NodeId::new(NodeType::Database, "ns", "owned-table").unwrap();
            let owner = analyzer.access_map().get_owner(&db_id);
            assert!(owner.is_some());
            assert_eq!(owner.unwrap().name(), "owner-svc");
        }

        #[test]
        fn test_build_access_map_handles_queue_operations() {
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("publisher", "ns")).unwrap();
            graph.add_node(create_service("subscriber", "ns")).unwrap();
            graph.add_node(create_queue("events-queue", "ns")).unwrap();

            // Publisher publishes to queue
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "publisher").unwrap(),
                        NodeId::new(NodeType::Queue, "ns", "events-queue").unwrap(),
                        EdgeType::Publishes,
                    )
                    .unwrap(),
                )
                .unwrap();

            // Subscriber subscribes to queue
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "subscriber").unwrap(),
                        NodeId::new(NodeType::Queue, "ns", "events-queue").unwrap(),
                        EdgeType::Subscribes,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            analyzer.build_access_map();

            let queue_id = NodeId::new(NodeType::Queue, "ns", "events-queue").unwrap();

            // Publishes should be recorded as write
            let writers = analyzer.access_map().get_writers(&queue_id);
            assert_eq!(writers.len(), 1);
            assert_eq!(writers[0].name(), "publisher");

            // Subscribes should be recorded as read
            let readers = analyzer.access_map().get_readers(&queue_id);
            assert_eq!(readers.len(), 1);
            assert_eq!(readers[0].name(), "subscriber");
        }

        #[test]
        fn test_detect_implicit_coupling() {
            let graph = create_test_graph_with_shared_db();
            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Should detect couplings between services sharing the database
            assert!(!result.implicit_couplings.is_empty());

            // At least one coupling should exist
            let coupling_count = result.implicit_couplings.len();
            assert!(coupling_count >= 1, "Expected at least one coupling");
        }

        #[test]
        fn test_coupling_risk_level_medium() {
            let graph = create_test_graph_with_shared_db();
            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Since service-a writes and service-b/c read, risk should be Medium
            for coupling in &result.implicit_couplings {
                if coupling.service_a.name() == "service-a"
                    || coupling.service_b.name() == "service-a"
                {
                    assert_eq!(
                        coupling.risk_level,
                        CouplingRisk::Medium,
                        "Expected medium risk for write/read coupling"
                    );
                }
            }
        }

        #[test]
        fn test_coupling_risk_level_high() {
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("writer-a", "ns")).unwrap();
            graph.add_node(create_service("writer-b", "ns")).unwrap();
            graph.add_node(create_database("shared-db", "ns")).unwrap();

            // Both services write
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "writer-a").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "writer-b").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            assert_eq!(result.implicit_couplings.len(), 1);
            assert_eq!(result.implicit_couplings[0].risk_level, CouplingRisk::High);
        }

        #[test]
        fn test_coupling_risk_level_low() {
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("reader-a", "ns")).unwrap();
            graph.add_node(create_service("reader-b", "ns")).unwrap();
            graph.add_node(create_database("shared-db", "ns")).unwrap();

            // Both services only read
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "reader-a").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "reader-b").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            assert_eq!(result.implicit_couplings.len(), 1);
            assert_eq!(result.implicit_couplings[0].risk_level, CouplingRisk::Low);
        }

        #[test]
        fn test_infer_ownership_from_naming_convention() {
            let mut graph = ForgeGraph::new();

            // Service named "user-service"
            graph
                .add_node(create_service("user-service", "ns"))
                .unwrap();

            // Database named "user-service-data" (should be owned by user-service)
            graph
                .add_node(create_database("user-service-data", "ns"))
                .unwrap();

            // Add write edge
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "user-service").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "user-service-data").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Should infer ownership
            let ownership = result
                .ownership_assignments
                .iter()
                .find(|a| a.resource.name() == "user-service-data");

            assert!(ownership.is_some());
            assert_eq!(ownership.unwrap().owner.name(), "user-service");
            assert!(matches!(
                ownership.unwrap().reason,
                OwnershipReason::NamingConvention
            ));
        }

        #[test]
        fn test_infer_ownership_exclusive_writer() {
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("service-a", "ns")).unwrap();
            graph.add_node(create_service("service-b", "ns")).unwrap();
            graph.add_node(create_database("shared-db", "ns")).unwrap();

            // Only service-a writes
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "service-a").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            // service-b only reads
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "service-b").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Ownership should be assigned to exclusive writer
            let ownership = result
                .ownership_assignments
                .iter()
                .find(|a| a.resource.name() == "shared-db");

            assert!(ownership.is_some());
            assert_eq!(ownership.unwrap().owner.name(), "service-a");
            assert!(matches!(
                ownership.unwrap().reason,
                OwnershipReason::ExclusiveWriter
            ));
        }

        #[test]
        fn test_generate_shared_access_edges() {
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("owner-svc", "ns")).unwrap();
            graph.add_node(create_service("reader-svc", "ns")).unwrap();
            graph.add_node(create_database("my-table", "ns")).unwrap();

            // Owner owns and writes
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "owner-svc").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "my-table").unwrap(),
                        EdgeType::Owns,
                    )
                    .unwrap(),
                )
                .unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "owner-svc").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "my-table").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            // Reader reads the owned table
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "reader-svc").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "my-table").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Should generate a shared read edge
            assert_eq!(result.shared_reads.len(), 1);
            assert_eq!(result.shared_reads[0].service.name(), "reader-svc");
            assert_eq!(result.shared_reads[0].owner.name(), "owner-svc");
            assert_eq!(result.shared_reads[0].access_type, AccessType::Read);
        }

        #[test]
        fn test_apply_results_to_graph_adds_shared_read() {
            // When one service is the exclusive writer (owner), the other becomes a "shared" reader
            let mut graph = ForgeGraph::new();

            graph.add_node(create_service("writer", "ns")).unwrap();
            graph.add_node(create_service("reader", "ns")).unwrap();
            graph.add_node(create_database("shared-db", "ns")).unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "writer").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Writes,
                    )
                    .unwrap(),
                )
                .unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "reader").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            let initial_edge_count = graph.edge_count();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Verify ownership was inferred (exclusive writer pattern)
            assert_eq!(result.ownership_assignments.len(), 1);
            assert_eq!(result.ownership_assignments[0].owner.name(), "writer");

            // Verify shared read was detected
            assert_eq!(result.shared_reads.len(), 1);
            assert_eq!(result.shared_reads[0].service.name(), "reader");

            result.apply_to_graph(&mut graph).unwrap();

            // Should have added new edges (OWNS + READS_SHARED)
            assert!(graph.edge_count() > initial_edge_count);

            // Should have READS_SHARED edge from reader to shared-db
            let reader_id = NodeId::new(NodeType::Service, "ns", "reader").unwrap();
            let edges = graph.edges_from(&reader_id);
            let shared_read_edges: Vec<_> = edges
                .iter()
                .filter(|e| e.edge_type == EdgeType::ReadsShared)
                .collect();
            assert!(!shared_read_edges.is_empty());
        }

        #[test]
        fn test_apply_results_to_graph_adds_implicit_coupling() {
            // When multiple non-owner services access the same resource, they're implicitly coupled
            let mut graph = ForgeGraph::new();

            // Create three services and a database with explicit owner
            graph.add_node(create_service("owner-svc", "ns")).unwrap();
            graph.add_node(create_service("accessor-a", "ns")).unwrap();
            graph.add_node(create_service("accessor-b", "ns")).unwrap();
            graph.add_node(create_database("shared-db", "ns")).unwrap();

            // Owner owns the database explicitly
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "owner-svc").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Owns,
                    )
                    .unwrap(),
                )
                .unwrap();

            // Two other services read from it
            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "accessor-a").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "accessor-b").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Should detect implicit coupling between the two accessors
            assert_eq!(result.implicit_couplings.len(), 1);

            result.apply_to_graph(&mut graph).unwrap();

            // Should have IMPLICITLY_COUPLED edge
            let accessor_a_id = NodeId::new(NodeType::Service, "ns", "accessor-a").unwrap();
            let accessor_b_id = NodeId::new(NodeType::Service, "ns", "accessor-b").unwrap();

            // Check edges from accessor-a or accessor-b (depends on alphabetical ordering)
            let edges_a = graph.edges_from(&accessor_a_id);
            let edges_b = graph.edges_from(&accessor_b_id);

            let coupling_edges: Vec<_> = edges_a
                .iter()
                .chain(edges_b.iter())
                .filter(|e| e.edge_type == EdgeType::ImplicitlyCoupled)
                .collect();

            assert!(
                !coupling_edges.is_empty(),
                "Expected IMPLICITLY_COUPLED edge between accessor-a and accessor-b"
            );
        }

        #[test]
        fn test_no_coupling_for_single_accessor() {
            let mut graph = ForgeGraph::new();

            graph
                .add_node(create_service("only-service", "ns"))
                .unwrap();
            graph.add_node(create_database("my-table", "ns")).unwrap();

            graph
                .add_edge(
                    Edge::new(
                        NodeId::new(NodeType::Service, "ns", "only-service").unwrap(),
                        NodeId::new(NodeType::Database, "ns", "my-table").unwrap(),
                        EdgeType::Reads,
                    )
                    .unwrap(),
                )
                .unwrap();

            let mut analyzer = CouplingAnalyzer::new(&graph);
            let result = analyzer.analyze();

            // Should not detect any coupling (only one service accesses the resource)
            assert!(result.implicit_couplings.is_empty());
        }
    }
}
