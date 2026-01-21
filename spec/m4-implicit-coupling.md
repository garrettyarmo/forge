# Milestone 4: Implicit Coupling Detection Specification

> **Spec Version**: 1.0
> **Status**: Draft
> **Implements**: IMPLEMENTATION_PLAN.md § Milestone 4
> **Depends On**: [M3 Multi-Language](./m3-multi-language.md)

---

## 1. Overview

### 1.1 Purpose

Detect and model implicit coupling between services that share resources without explicit API contracts. In enterprise polyrepo environments, services often communicate implicitly through shared databases, queues, or storage—creating hidden dependencies that can cause cascading failures.

### 1.2 The Problem

```
┌─────────────────┐          ┌─────────────────┐
│   Service A     │          │   Service B     │
│   (writes)      │          │   (reads)       │
└────────┬────────┘          └────────┬────────┘
         │                            │
         │    No API call between     │
         │    these services!         │
         ▼                            ▼
┌──────────────────────────────────────────────┐
│              DynamoDB Table                   │
│              (shared resource)                │
└──────────────────────────────────────────────┘

If Service A changes the data schema, Service B will break—
but there's no explicit dependency to warn you.
```

### 1.3 Success Criteria

1. Services reading the same DynamoDB table are linked with `IMPLICITLY_COUPLED` edges
2. Services sharing SQS queues are linked with `IMPLICITLY_COUPLED` edges
3. Resource ownership is inferred from Terraform definitions or naming conventions
4. Coupling reasons are recorded in edge metadata
5. Graph can answer "what services share this resource?"

### 1.4 Edge Types for Coupling

| Edge Type | Meaning | Example |
|-----------|---------|---------|
| `READS_SHARED` | Reads from resource owned by another | analytics reads users-table (owned by user-service) |
| `WRITES_SHARED` | Writes to resource owned by another | import-job writes users-table |
| `IMPLICITLY_COUPLED` | Bidirectional coupling via shared resource | service-a ↔ service-b |

---

## 2. Data Structures

### 2.1 Resource Access Tracking

```rust
// forge-survey/src/coupling.rs

use forge_graph::{NodeId, NodeType, EdgeType};
use std::collections::{HashMap, HashSet};

/// Tracks which services access which resources
#[derive(Debug, Default)]
pub struct ResourceAccessMap {
    /// Map from resource NodeId to services that read it
    readers: HashMap<NodeId, HashSet<NodeId>>,

    /// Map from resource NodeId to services that write it
    writers: HashMap<NodeId, HashSet<NodeId>>,

    /// Map from resource NodeId to its owner (if known)
    owners: HashMap<NodeId, NodeId>,

    /// Evidence for each access relationship
    evidence: HashMap<(NodeId, NodeId), Vec<AccessEvidence>>,
}

/// Evidence for a resource access
#[derive(Debug, Clone)]
pub struct AccessEvidence {
    /// Source file where access was detected
    pub source_file: String,

    /// Line number
    pub source_line: u32,

    /// Detection method (e.g., "boto3.get_item", "aws-sdk.query")
    pub detection_method: String,

    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
}

impl ResourceAccessMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a read access to a resource
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

    /// Record a write access to a resource
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

    /// Set the owner of a resource
    pub fn set_owner(&mut self, resource_id: NodeId, owner_id: NodeId) {
        self.owners.insert(resource_id, owner_id);
    }

    /// Get all services that read a resource
    pub fn get_readers(&self, resource_id: &NodeId) -> Vec<&NodeId> {
        self.readers
            .get(resource_id)
            .map(|s| s.iter().collect())
            .unwrap_or_default()
    }

    /// Get all services that write to a resource
    pub fn get_writers(&self, resource_id: &NodeId) -> Vec<&NodeId> {
        self.writers
            .get(resource_id)
            .map(|s| s.iter().collect())
            .unwrap_or_default()
    }

    /// Get the owner of a resource
    pub fn get_owner(&self, resource_id: &NodeId) -> Option<&NodeId> {
        self.owners.get(resource_id)
    }

    /// Get all resources in the map
    pub fn resources(&self) -> HashSet<&NodeId> {
        self.readers.keys()
            .chain(self.writers.keys())
            .collect()
    }

    /// Get evidence for a specific access
    pub fn get_evidence(&self, service_id: &NodeId, resource_id: &NodeId) -> &[AccessEvidence] {
        self.evidence
            .get(&(service_id.clone(), resource_id.clone()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}
```

### 2.2 Coupling Analysis Result

```rust
/// Result of coupling analysis
#[derive(Debug)]
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

/// An implicit coupling between two services
#[derive(Debug, Clone)]
pub struct ImplicitCoupling {
    /// First service
    pub service_a: NodeId,

    /// Second service
    pub service_b: NodeId,

    /// Resources they share
    pub shared_resources: Vec<NodeId>,

    /// Reason for coupling
    pub reason: String,

    /// Risk level (high if one writes and one reads)
    pub risk_level: CouplingRisk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouplingRisk {
    /// Both services only read (low risk)
    Low,

    /// One service writes, others read (medium risk - schema changes)
    Medium,

    /// Multiple services write (high risk - race conditions, conflicts)
    High,
}

/// A shared access relationship
#[derive(Debug, Clone)]
pub struct SharedAccess {
    pub service: NodeId,
    pub resource: NodeId,
    pub owner: NodeId,
    pub access_type: AccessType,
    pub evidence: Vec<AccessEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read,
    Write,
}

/// An ownership assignment
#[derive(Debug, Clone)]
pub struct OwnershipAssignment {
    pub resource: NodeId,
    pub owner: NodeId,
    pub reason: OwnershipReason,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub enum OwnershipReason {
    /// Resource defined in owner's Terraform
    TerraformDefinition { file: String },

    /// Resource name matches service name pattern
    NamingConvention,

    /// Only this service writes to the resource
    ExclusiveWriter,

    /// Manually specified
    Manual,
}
```

---

## 3. Coupling Detection Algorithm

### 3.1 Main Algorithm

```rust
// forge-survey/src/coupling.rs (continued)

use forge_graph::ForgeGraph;

/// Analyzes a graph for implicit coupling
pub struct CouplingAnalyzer<'a> {
    graph: &'a ForgeGraph,
    access_map: ResourceAccessMap,
}

impl<'a> CouplingAnalyzer<'a> {
    pub fn new(graph: &'a ForgeGraph) -> Self {
        Self {
            graph,
            access_map: ResourceAccessMap::new(),
        }
    }

    /// Run full coupling analysis
    pub fn analyze(&mut self) -> CouplingAnalysisResult {
        // Step 1: Build resource access map from existing edges
        self.build_access_map();

        // Step 2: Infer resource ownership
        let ownership_assignments = self.infer_ownership();
        for assignment in &ownership_assignments {
            self.access_map.set_owner(
                assignment.resource.clone(),
                assignment.owner.clone(),
            );
        }

        // Step 3: Detect implicit couplings
        let implicit_couplings = self.detect_implicit_couplings();

        // Step 4: Generate shared access edges
        let (shared_reads, shared_writes) = self.generate_shared_access_edges();

        CouplingAnalysisResult {
            implicit_couplings,
            shared_reads,
            shared_writes,
            ownership_assignments,
        }
    }

    /// Build resource access map from graph edges
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

            let evidence = AccessEvidence {
                source_file: edge.metadata.evidence.first()
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
                source_line: 0,
                detection_method: format!("{:?}", edge.edge_type),
                confidence: edge.metadata.confidence.unwrap_or(1.0),
            };

            match edge.edge_type {
                EdgeType::Reads | EdgeType::ReadsShared | EdgeType::Subscribes => {
                    self.access_map.record_read(
                        edge.source.clone(),
                        edge.target.clone(),
                        evidence,
                    );
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
                    self.access_map.record_read(
                        edge.source.clone(),
                        edge.target.clone(),
                        evidence,
                    );
                }
                EdgeType::Owns => {
                    // This service owns the resource
                    self.access_map.set_owner(
                        edge.target.clone(),
                        edge.source.clone(),
                    );
                }
                _ => {}
            }
        }
    }

    /// Infer ownership of resources
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
            if resource_name.contains(service_name) ||
               resource_name.starts_with(&format!("{}-", service_name)) ||
               resource_name.starts_with(&format!("{}_", service_name))
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
                                if service.display_name == repo_name_str ||
                                   service.id.name() == repo_name_str
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

    /// Detect implicit couplings between services
    fn detect_implicit_couplings(&self) -> Vec<ImplicitCoupling> {
        let mut couplings = Vec::new();
        let mut processed_pairs: HashSet<(NodeId, NodeId)> = HashSet::new();

        for resource_id in self.access_map.resources() {
            let readers = self.access_map.get_readers(resource_id);
            let writers = self.access_map.get_writers(resource_id);

            // All services accessing this resource
            let all_services: HashSet<_> = readers.iter()
                .chain(writers.iter())
                .cloned()
                .collect();

            // Skip if only one service
            if all_services.len() <= 1 {
                continue;
            }

            // Get owner to exclude from "shared" consideration
            let owner = self.access_map.get_owner(resource_id);

            // Create coupling between each pair of non-owner services
            let services: Vec<_> = all_services.iter()
                .filter(|s| owner.map(|o| o != *s).unwrap_or(true))
                .cloned()
                .collect();

            for i in 0..services.len() {
                for j in (i + 1)..services.len() {
                    let service_a = services[i];
                    let service_b = services[j];

                    // Skip if already processed
                    let pair = if service_a.as_str() < service_b.as_str() {
                        (service_a.clone(), service_b.clone())
                    } else {
                        (service_b.clone(), service_a.clone())
                    };

                    if processed_pairs.contains(&pair) {
                        // Add this resource to existing coupling
                        if let Some(existing) = couplings.iter_mut()
                            .find(|c: &&mut ImplicitCoupling| {
                                (c.service_a == pair.0 && c.service_b == pair.1) ||
                                (c.service_a == pair.1 && c.service_b == pair.0)
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
                    let resource = self.graph.get_node(resource_id)
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

    /// Generate READS_SHARED and WRITES_SHARED edges
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
                        evidence: self.access_map
                            .get_evidence(reader, resource_id)
                            .to_vec(),
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
                        evidence: self.access_map
                            .get_evidence(writer, resource_id)
                            .to_vec(),
                    });
                }
            }
        }

        (shared_reads, shared_writes)
    }
}
```

### 3.2 Applying Results to Graph

```rust
// forge-survey/src/coupling.rs (continued)

impl CouplingAnalysisResult {
    /// Apply the analysis results to a graph
    pub fn apply_to_graph(&self, graph: &mut ForgeGraph) -> Result<(), GraphError> {
        // Add OWNS edges for inferred ownership
        for assignment in &self.ownership_assignments {
            let edge = Edge::new(
                assignment.owner.clone(),
                assignment.resource.clone(),
                EdgeType::Owns,
            )?;

            let mut edge = edge.with_metadata(EdgeMetadata {
                confidence: Some(assignment.confidence),
                reason: Some(format!("{:?}", assignment.reason)),
                discovered_at: chrono::Utc::now(),
                ..Default::default()
            });

            graph.upsert_edge(edge)?;
        }

        // Add READS_SHARED edges
        for access in &self.shared_reads {
            let mut edge = Edge::new(
                access.service.clone(),
                access.resource.clone(),
                EdgeType::ReadsShared,
            )?;

            edge.metadata = EdgeMetadata {
                reason: Some(format!("Reads resource owned by {:?}", access.owner)),
                evidence: access.evidence.iter()
                    .map(|e| format!("{}:{}", e.source_file, e.source_line))
                    .collect(),
                discovered_at: chrono::Utc::now(),
                ..Default::default()
            };

            graph.upsert_edge(edge)?;
        }

        // Add WRITES_SHARED edges
        for access in &self.shared_writes {
            let mut edge = Edge::new(
                access.service.clone(),
                access.resource.clone(),
                EdgeType::WritesShared,
            )?;

            edge.metadata = EdgeMetadata {
                reason: Some(format!("Writes to resource owned by {:?}", access.owner)),
                evidence: access.evidence.iter()
                    .map(|e| format!("{}:{}", e.source_file, e.source_line))
                    .collect(),
                discovered_at: chrono::Utc::now(),
                ..Default::default()
            };

            graph.upsert_edge(edge)?;
        }

        // Add IMPLICITLY_COUPLED edges
        for coupling in &self.implicit_couplings {
            let mut edge = Edge::new(
                coupling.service_a.clone(),
                coupling.service_b.clone(),
                EdgeType::ImplicitlyCoupled,
            )?;

            let resources_str = coupling.shared_resources
                .iter()
                .filter_map(|r| graph.get_node(r).map(|n| n.display_name.clone()))
                .collect::<Vec<_>>()
                .join(", ");

            edge.metadata = EdgeMetadata {
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
            graph.upsert_edge(edge)?;
        }

        Ok(())
    }
}
```

---

## 4. Integration with Survey Pipeline

```rust
// forge-survey/src/lib.rs (updated for M4)

/// Full survey pipeline with coupling analysis
pub async fn run_full_survey(
    config: &ForgeConfig,
    registry: &ParserRegistry,
) -> Result<ForgeGraph, SurveyError> {
    let mut builder = GraphBuilder::new();

    // Phase 1: Survey all repositories
    for repo in collect_repos(config).await? {
        survey_repo(&repo.local_path, &mut builder, registry, &config.languages.exclude)?;
    }

    let mut graph = builder.build();

    // Phase 2: Run coupling analysis
    let mut analyzer = CouplingAnalyzer::new(&graph);
    let coupling_result = analyzer.analyze();

    // Phase 3: Apply coupling results
    coupling_result.apply_to_graph(&mut graph)?;

    // Log coupling summary
    tracing::info!(
        "Coupling analysis complete: {} implicit couplings, {} shared reads, {} shared writes",
        coupling_result.implicit_couplings.len(),
        coupling_result.shared_reads.len(),
        coupling_result.shared_writes.len(),
    );

    // Log high-risk couplings
    for coupling in coupling_result.implicit_couplings.iter()
        .filter(|c| c.risk_level == CouplingRisk::High)
    {
        tracing::warn!(
            "High-risk coupling detected: {} <-> {} - {}",
            coupling.service_a.name(),
            coupling.service_b.name(),
            coupling.reason
        );
    }

    Ok(graph)
}
```

---

## 5. Test Specifications

### 5.1 Coupling Detection Tests

```rust
#[cfg(test)]
mod coupling_tests {
    use super::*;

    fn create_test_graph() -> ForgeGraph {
        let mut graph = ForgeGraph::new();

        // Create services
        let svc_a = create_service("service-a", "ns");
        let svc_b = create_service("service-b", "ns");
        let svc_c = create_service("service-c", "ns");

        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();
        graph.add_node(svc_c).unwrap();

        // Create shared database
        let db = NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, "ns", "users-table").unwrap())
            .node_type(NodeType::Database)
            .display_name("users-table")
            .attribute("db_type", "dynamodb")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap();

        graph.add_node(db).unwrap();

        // Service A writes to DB
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
            EdgeType::Writes,
        ).unwrap()).unwrap();

        // Service B reads from DB
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-b").unwrap(),
            NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
            EdgeType::Reads,
        ).unwrap()).unwrap();

        // Service C also reads from DB
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-c").unwrap(),
            NodeId::new(NodeType::Database, "ns", "users-table").unwrap(),
            EdgeType::Reads,
        ).unwrap()).unwrap();

        graph
    }

    #[test]
    fn test_detect_implicit_coupling() {
        let graph = create_test_graph();
        let mut analyzer = CouplingAnalyzer::new(&graph);
        let result = analyzer.analyze();

        // Should detect couplings between A-B, A-C, B-C
        assert!(!result.implicit_couplings.is_empty());

        // A-B coupling should be medium risk (one writes, one reads)
        let ab_coupling = result.implicit_couplings.iter()
            .find(|c| {
                (c.service_a.name() == "service-a" && c.service_b.name() == "service-b") ||
                (c.service_a.name() == "service-b" && c.service_b.name() == "service-a")
            });

        assert!(ab_coupling.is_some());
        assert_eq!(ab_coupling.unwrap().risk_level, CouplingRisk::Medium);
    }

    #[test]
    fn test_infer_ownership_from_naming() {
        let mut graph = ForgeGraph::new();

        // Service named "user-service"
        let svc = create_service("user-service", "ns");
        graph.add_node(svc).unwrap();

        // Database named "user-service-data" (should be owned by user-service)
        let db = NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, "ns", "user-service-data").unwrap())
            .node_type(NodeType::Database)
            .display_name("user-service-data")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap();

        graph.add_node(db).unwrap();

        // Add write edge
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "user-service").unwrap(),
            NodeId::new(NodeType::Database, "ns", "user-service-data").unwrap(),
            EdgeType::Writes,
        ).unwrap()).unwrap();

        let mut analyzer = CouplingAnalyzer::new(&graph);
        let result = analyzer.analyze();

        // Should infer ownership
        let ownership = result.ownership_assignments.iter()
            .find(|a| a.resource.name() == "user-service-data");

        assert!(ownership.is_some());
        assert_eq!(ownership.unwrap().owner.name(), "user-service");
    }

    #[test]
    fn test_exclusive_writer_ownership() {
        let mut graph = ForgeGraph::new();

        let svc_a = create_service("service-a", "ns");
        let svc_b = create_service("service-b", "ns");

        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        let db = NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, "ns", "shared-db").unwrap())
            .node_type(NodeType::Database)
            .display_name("shared-db")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap();

        graph.add_node(db).unwrap();

        // Only service-a writes
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
            EdgeType::Writes,
        ).unwrap()).unwrap();

        // service-b only reads
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-b").unwrap(),
            NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
            EdgeType::Reads,
        ).unwrap()).unwrap();

        let mut analyzer = CouplingAnalyzer::new(&graph);
        let result = analyzer.analyze();

        // Ownership should be assigned to exclusive writer
        let ownership = result.ownership_assignments.iter()
            .find(|a| a.resource.name() == "shared-db");

        assert!(ownership.is_some());
        assert_eq!(ownership.unwrap().owner.name(), "service-a");
        assert!(matches!(ownership.unwrap().reason, OwnershipReason::ExclusiveWriter));
    }

    #[test]
    fn test_high_risk_multiple_writers() {
        let mut graph = ForgeGraph::new();

        let svc_a = create_service("service-a", "ns");
        let svc_b = create_service("service-b", "ns");

        graph.add_node(svc_a).unwrap();
        graph.add_node(svc_b).unwrap();

        let db = NodeBuilder::new()
            .id(NodeId::new(NodeType::Database, "ns", "shared-db").unwrap())
            .node_type(NodeType::Database)
            .display_name("shared-db")
            .source(DiscoverySource::TerraformParser)
            .build()
            .unwrap();

        graph.add_node(db).unwrap();

        // Both services write
        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-a").unwrap(),
            NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
            EdgeType::Writes,
        ).unwrap()).unwrap();

        graph.add_edge(Edge::new(
            NodeId::new(NodeType::Service, "ns", "service-b").unwrap(),
            NodeId::new(NodeType::Database, "ns", "shared-db").unwrap(),
            EdgeType::Writes,
        ).unwrap()).unwrap();

        let mut analyzer = CouplingAnalyzer::new(&graph);
        let result = analyzer.analyze();

        // Should be high risk
        let coupling = result.implicit_couplings.first();
        assert!(coupling.is_some());
        assert_eq!(coupling.unwrap().risk_level, CouplingRisk::High);
    }

    fn create_service(name: &str, namespace: &str) -> Node {
        NodeBuilder::new()
            .id(NodeId::new(NodeType::Service, namespace, name).unwrap())
            .node_type(NodeType::Service)
            .display_name(name)
            .source(DiscoverySource::Manual)
            .build()
            .unwrap()
    }
}
```

---

## 6. Implementation Checklist

| Task ID | Description | Files |
|---------|-------------|-------|
| M4-T1 | Implement shared resource detection | `forge-survey/src/coupling.rs` |
| M4-T2 | Implement ownership inference | `forge-survey/src/coupling.rs` |
| M4-T3 | Generate IMPLICITLY_COUPLED edges | `forge-survey/src/coupling.rs` |
| M4-T4 | Add coupling analysis to pipeline | `forge-survey/src/lib.rs` |
| M4-T5 | Write coupling detection tests | `forge-survey/src/coupling.rs` |
| M4-T6 | Write integration tests | `forge-survey/tests/integration_coupling.rs` |

---

## 7. Acceptance Criteria

- [ ] Services reading the same DynamoDB table get `IMPLICITLY_COUPLED` edge
- [ ] Services sharing SQS queues get `IMPLICITLY_COUPLED` edge
- [ ] Ownership is inferred from Terraform definitions
- [ ] Ownership is inferred from naming conventions (service-name-data)
- [ ] Ownership is inferred from exclusive writer pattern
- [ ] `READS_SHARED` edges are created for non-owner readers
- [ ] `WRITES_SHARED` edges are created for non-owner writers
- [ ] High-risk couplings (multiple writers) are logged as warnings
- [ ] Coupling reasons are recorded in edge metadata
- [ ] `forge map --query "services sharing users-table"` works correctly
