# Forge V1 Implementation Plan

## Overview

### What is Forge?

Forge is a **reusable platform** for surveying and mapping software ecosystems. It builds a knowledge graph of services, APIs, databases, and their relationships, then serializes that graph into LLM-optimized context for intelligent assistance.

### Goals

- **Survey**: Automatically discover services, APIs, databases, queues, and cloud resources from source code
  - **Purely deterministic**: Uses tree-sitter AST parsing only - no LLM calls
  - Reproducible, fast, works offline
- **Map**: Visualize and serialize the ecosystem for human understanding and LLM consumption
- **Interview**: Augment technical discovery with business context through LLM-assisted interviews
  - **LLMs only here**: Shells out to coding agent CLIs (claude, gemini, etc.)
  - Opt-in via `--business-context` flag
- **Reusable**: Works with any GitHub organization/repos - not tied to any specific codebase

### Non-Goals (V1)

- Real-time monitoring or runtime discovery
- Direct IDE integration (future consideration)
- Multi-cloud support beyond AWS (V1 focuses on AWS patterns)
- Database schema introspection (infers from code patterns only)

### Success Criteria

1. `forge survey` successfully maps a multi-repo ecosystem with JS, Python, and Terraform
2. `forge map` produces context that fits within token budgets and improves LLM task accuracy
3. Any developer can clone Forge, configure it for their repos, and get value within 30 minutes
4. Adding a new language parser requires only implementing a well-defined trait

### Platform Philosophy

Forge is NOT specific to any particular codebase. It must be:

| Principle | Implementation |
|-----------|----------------|
| **Generic** | Works with any GitHub organization/repos |
| **Configurable** | Users define their repos via `forge.yaml` or CLI flags |
| **Extensible** | Parser architecture allows adding new languages |
| **Standalone** | Single Rust binary, no external dependencies beyond Git |

---

## Architecture

### Crate Structure

```
forge/
├── Cargo.toml              # Workspace definition
├── forge-cli/              # CLI entry point and commands
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── survey.rs
│       │   ├── map.rs
│       │   └── init.rs
│       └── config.rs
├── forge-graph/            # Knowledge graph data structures
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── node.rs
│       ├── edge.rs
│       ├── graph.rs
│       └── query.rs
├── forge-survey/           # Code analysis and discovery
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── github.rs
│       ├── parser/
│       │   ├── mod.rs
│       │   ├── traits.rs
│       │   ├── javascript.rs
│       │   ├── python.rs
│       │   └── terraform.rs
│       └── coupling.rs
└── forge-llm/              # LLM CLI adapter layer
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── provider.rs
        ├── adapters/
        │   ├── mod.rs
        │   ├── claude.rs
        │   ├── gemini.rs
        │   └── codex.rs
        └── interview.rs
```

### Data Flow

**Important: Survey Phase is Purely Deterministic**

The survey phase uses **only tree-sitter AST parsing** - no LLM calls. This ensures:
- Reproducible results (same code → same graph)
- Fast execution (no API latency)
- Offline capability (no network needed for local repos)
- Predictable costs (no token usage during survey)

LLMs are **only** used in the explicit business context interview (Milestone 6), triggered by `--business-context` flag.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      SURVEY PHASE (Deterministic, No LLM)                    │
└─────────────────────────────────────────────────────────────────────────────┘

  forge.yaml          GitHub API / Local Paths         Source Files
      │                        │                            │
      ▼                        ▼                            ▼
┌──────────┐            ┌───────────┐              ┌──────────────┐
│  Config  │───────────▶│  Cloner   │─────────────▶│   Parsers    │
│  Loader  │            │           │              │ (JS/Py/TF)   │
└──────────┘            └───────────┘              └──────────────┘
                                                          │
                                                          ▼
                                                   ┌──────────────┐
                                                   │   Coupling   │
                                                   │  Detection   │
                                                   └──────────────┘
                                                          │
                                                          ▼
                                                   ┌──────────────┐
                                                   │    Graph     │
                                                   │  (petgraph)  │
                                                   └──────────────┘
                                                          │
                                                          ▼
                                                   ┌──────────────┐
                                                   │  Persist to  │
                                                   │    JSON      │
                                                   └──────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                               MAP PHASE                                      │
└─────────────────────────────────────────────────────────────────────────────┘

  Graph JSON            Query/Filter               Serializers
      │                      │                          │
      ▼                      ▼                          ▼
┌──────────┐          ┌───────────┐            ┌──────────────┐
│   Load   │─────────▶│  Subgraph │───────────▶│   Markdown   │
│   Graph  │          │ Extractor │            │     JSON     │
└──────────┘          └───────────┘            │    Mermaid   │
                                               └──────────────┘
                                                      │
                                                      ▼
                                               ┌──────────────┐
                                               │    Token     │
                                               │   Budgeting  │
                                               └──────────────┘
                                                      │
                                                      ▼
                                               ┌──────────────┐
                                               │    Output    │
                                               │   to stdout  │
                                               │   or file    │
                                               └──────────────┘
```

### Node Types

| Type | Description | Key Attributes |
|------|-------------|----------------|
| `Service` | A deployable unit (Lambda, container, server) | name, language, repo, entry_point |
| `API` | An HTTP endpoint or RPC interface | path, method, service_id |
| `Database` | A database or table | type (dynamo, postgres, etc), table_name, arn |
| `Queue` | Message queue (SQS, SNS, EventBridge) | type, name, arn |
| `CloudResource` | Other AWS resources (S3, etc) | type, name, arn |

### Edge Types

| Type | Description | Example |
|------|-------------|---------|
| `CALLS` | Service A invokes Service B via HTTP/RPC | user-service → auth-service |
| `OWNS` | Service defines/manages an API | user-service owns /users/* |
| `READS` | Service reads from a database | user-service reads users-table |
| `WRITES` | Service writes to a database | user-service writes users-table |
| `PUBLISHES` | Service sends messages to queue | order-service publishes to order-events |
| `SUBSCRIBES` | Service consumes from queue | notification-service subscribes to order-events |
| `USES` | Service uses a cloud resource | report-service uses reports-bucket |
| `READS_SHARED` | Reads from resource owned by another service | analytics reads users-table (owned by user-service) |
| `WRITES_SHARED` | Writes to resource owned by another service | import-job writes users-table |
| `IMPLICITLY_COUPLED` | Services share a resource without explicit contract | service-a ↔ service-b (via shared dynamo) |

---

## Milestone 1: Foundation

**Goal**: Establish project structure and core graph operations.

> **Detailed Specification**: [spec/m1-foundation.md](spec/m1-foundation.md)

### Tasks

- [x] **M1-T1**: Initialize Cargo workspace
  - Create `Cargo.toml` at root with workspace members
  - Create stub `Cargo.toml` for each crate
  - Verify `cargo build` succeeds
  - **Files**: `Cargo.toml`, `forge-*/Cargo.toml`

- [x] **M1-T2**: Set up GitHub Actions CI
  - Rust build and test on push/PR
  - Clippy linting
  - Format checking with rustfmt
  - **Files**: `.github/workflows/ci.yml`

- [x] **M1-T3**: Implement node types in forge-graph
  - Define `NodeType` enum (Service, API, Database, Queue, CloudResource)
  - Define `Node` struct with id, type, attributes (HashMap)
  - Implement serialization/deserialization
  - **Files**: `forge-graph/src/node.rs`

- [x] **M1-T4**: Implement edge types in forge-graph
  - Define `EdgeType` enum with all relationship types
  - Define `Edge` struct with source, target, type, metadata
  - **Files**: `forge-graph/src/edge.rs`

- [x] **M1-T5**: Implement graph wrapper around petgraph
  - `ForgeGraph` struct wrapping `petgraph::Graph`
  - Methods: `add_node`, `add_edge`, `get_node`, `get_edges`
  - **Files**: `forge-graph/src/graph.rs`

- [x] **M1-T6**: Implement query interface
  - `get_node(id)` - retrieve node by ID
  - `traverse_edges(node_id, edge_type)` - get connected nodes
  - `find_path(from, to)` - shortest path between nodes
  - `get_subgraph(node_ids)` - extract induced subgraph
  - **Files**: `forge-graph/src/query.rs`

- [x] **M1-T7**: Implement JSON persistence
  - `save_to_file(path)` - serialize graph to JSON
  - `load_from_file(path)` - deserialize graph from JSON
  - **Files**: `forge-graph/src/graph.rs`

- [x] **M1-T8**: Write unit tests for forge-graph
  - Node creation and serialization
  - Edge creation and serialization
  - Graph operations (add, query, traverse)
  - Persistence round-trip
  - **Files**: `forge-graph/src/lib.rs` (tests module)

### Dependencies

```toml
# forge-graph/Cargo.toml
[dependencies]
petgraph = "0.6"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
uuid = { version = "1.0", features = ["v4", "serde"] }
```

### Acceptance Criteria

- [x] `cargo build --workspace` succeeds
- [x] `cargo test --workspace` passes all tests
- [x] Can create a graph with nodes and edges programmatically
- [x] Can save graph to JSON and reload with identical structure
- [x] CI runs on every push

---

## Milestone 2: Survey Core

**Goal**: GitHub integration, JavaScript parser, configurable repo sources.

> **Detailed Specification**: [spec/m2-survey-core.md](spec/m2-survey-core.md)

**Note**: The survey phase is **purely deterministic** using tree-sitter AST parsing. No LLM calls occur during survey - this ensures reproducibility and fast execution. LLM integration is only for the business context interview (M6).

### Tasks

- [x] **M2-T1**: Define forge.yaml configuration schema
  - Top-level structure with repos, output, optional settings
  - Support for org URL, explicit repo list, local paths
  - Languages auto-detected (optional `languages.exclude` for edge cases)
  - **Files**: `forge-cli/src/config.rs`

- [x] **M2-T2**: Implement configuration loading
  - Load from `./forge.yaml` by default
  - Support `--config` flag override
  - Environment variable overrides (`FORGE_*`)
  - **Files**: `forge-cli/src/config.rs`

- [x] **M2-T3**: Implement `forge init` command
  - Generate default `forge.yaml` with comments
  - Prompt for GitHub org or let user edit manually
  - **Files**: `forge-cli/src/commands/init.rs`

- [x] **M2-T4**: Implement GitHub API client
  - List repos in organization
  - Clone/pull repos to local cache (`~/.forge/repos/`)
  - Handle authentication (token from env or config)
  - **Files**: `forge-survey/src/github.rs`

- [x] **M2-T5**: Define parser trait
  - `trait Parser { fn parse(&self, repo_path: &Path) -> Vec<Discovery>; }`
  - `Discovery` enum with Service, Import, APICall, DBAccess variants
  - **Files**: `forge-survey/src/parser/traits.rs`, `forge-survey/src/parser/mod.rs`

- [x] **M2-T6**: Implement JavaScript/TypeScript parser
  - tree-sitter-javascript integration
  - Detect service entry points (package.json scripts, exports)
  - Parse import/require statements
  - Detect AWS SDK patterns (`aws-sdk`, `@aws-sdk/*`)
  - Detect HTTP client patterns (axios, fetch)
  - Detect DynamoDB client patterns
  - **Files**: `forge-survey/src/parser/javascript.rs`

- [x] **M2-T7**: Implement discovery-to-graph mapper
  - Convert parser discoveries to graph nodes/edges
  - Deduplicate nodes (same DB, same service)
  - **Files**: `forge-survey/src/graph_builder.rs`
  - **Implementation Notes**:
    - GraphBuilder maintains internal indexes for deduplication (service_map, resource_map)
    - Supports incremental graph building via from_graph()
    - Properly handles ReadWrite database operations (creates both READS and WRITES edges)
    - Records evidence (file:line) for all edges
    - All 6 tests passing

- [x] **M2-T8**: Implement `forge survey` command
  - Load config, clone repos, run parsers
  - Build graph, save to output path
  - Progress reporting
  - **Files**: `forge-cli/src/commands/survey.rs`
  - **Implementation Notes**:
    - Complete survey command implementation in forge-cli/src/commands/survey.rs
    - Loads configuration from forge.yaml with CLI overrides
    - Discovers repositories from GitHub org, explicit repos, and local paths
    - Clones/updates GitHub repos to local cache
    - Parses JavaScript/TypeScript files using JavaScriptParser
    - Builds knowledge graph using GraphBuilder
    - Saves graph to configured output path
    - Handles errors gracefully (one repo failure doesn't crash entire survey)
    - All tests passing

- [x] **M2-T9**: Write unit tests for JavaScript parser
  - Test import detection
  - Test AWS SDK pattern detection
  - Test HTTP client detection
  - Use inline test fixtures
  - **Files**: `forge-survey/src/parser/javascript.rs` (tests)

- [x] **M2-T10**: Write integration test with synthetic JS repo
  - Created forge-survey/tests/integration_js.rs with 6 comprehensive integration tests
  - Tests cover: synthetic repos, HTTP calls, multiple AWS services, TypeScript/framework detection, empty repos
  - Fixed GraphBuilder to use hyphens instead of colons in fallback resource names (NodeId validation)
  - Fixed queue edge type for unknown operations (use Publishes instead of Uses)
  - All 6 integration tests passing, full test suite (121 tests) passing
  - **Files**: `forge-survey/tests/integration_js.rs`, `forge-survey/src/graph_builder.rs`

### Dependencies

```toml
# forge-survey/Cargo.toml
[dependencies]
tree-sitter = "0.24"
tree-sitter-javascript = "0.23"
streaming-iterator = "0.1"  # Required for tree-sitter 0.24+ QueryMatches iteration
octocrab = "0.44"  # GitHub API
tokio = { version = "1.0", features = ["full"] }
walkdir = "2.5"
forge-graph = { path = "../forge-graph" }

# forge-cli/Cargo.toml
[dependencies]
clap = { version = "4.4", features = ["derive"] }
serde_yaml = "0.9"
forge-graph = { path = "../forge-graph" }
forge-survey = { path = "../forge-survey" }
```

### Configuration Schema (forge.yaml)

```yaml
# forge.yaml - Forge configuration file

# Repository sources (choose one or combine)
repos:
  # Option 1: GitHub organization (discovers all repos)
  github_org: "my-org"

  # Option 2: Explicit list of repos
  github_repos:
    - "my-org/service-a"
    - "my-org/service-b"

  # Option 3: Local paths (for testing or air-gapped environments)
  local_paths:
    - "/path/to/repo1"
    - "/path/to/repo2"

# GitHub authentication (or use GITHUB_TOKEN env var)
github:
  token_env: "GITHUB_TOKEN"  # environment variable name

# Languages are AUTO-DETECTED from file extensions and config files
# (package.json → JS/TS, requirements.txt/pyproject.toml → Python, *.tf → Terraform)
# Only use this section if you need to exclude specific languages:
languages:
  exclude:
    - terraform  # Example: skip Terraform parsing

# Output configuration
output:
  graph_path: ".forge/graph.json"
  cache_path: "~/.forge/repos"

# LLM provider for interview mode (optional)
llm:
  provider: "claude"  # claude | gemini | codex

# Token budget for map output
token_budget: 8000
```

### Acceptance Criteria

- [x] `forge init` creates a valid `forge.yaml`
- [x] `forge survey` clones repos from GitHub org
- [x] `forge survey` works with local paths
- [x] JavaScript imports and AWS SDK calls are detected
- [x] Graph is saved to configured output path
- [x] Parser failures for one repo don't crash entire survey

---

## Milestone 3: Multi-Language

**Goal**: Python and Terraform parsers, extensible parser architecture.

> **Detailed Specification**: [spec/m3-multi-language.md](spec/m3-multi-language.md)

### Tasks

- [x] **M3-T1**: Implement Python parser
  - **Files**: `forge-survey/src/parser/python.rs`, `forge-survey/tests/integration_python.rs`
  - **Implementation Notes**:
    - Complete Python parser with tree-sitter-python integration
    - Detects imports, boto3 patterns, DynamoDB operations, HTTP clients (requests/httpx)
    - Supports pyproject.toml, setup.py, requirements.txt parsing
    - Framework detection (FastAPI, Flask, Django, Chalice, Starlette)
    - Entry point detection
    - 43 unit tests passing
    - 6 integration tests passing

- [x] **M3-T2**: Implement Terraform parser
  - **Files**: `forge-survey/src/parser/terraform.rs`, `forge-survey/Cargo.toml`, `forge-survey/src/parser/mod.rs`
  - **Implementation Notes**:
    - Added hcl-rs dependency (version 0.18) to forge-survey/Cargo.toml
    - TerraformParser with full HCL parsing support
    - Detects aws_dynamodb_table resources (extracts table name from resource name, parses attributes)
    - Detects aws_sqs_queue resources (extracts queue name from resource name)
    - Detects aws_sns_topic resources (extracts topic name from resource name)
    - Detects aws_s3_bucket resources (extracts bucket name from resource name or bucket attribute)
    - Detects aws_lambda_function resources (extracts function name, runtime, handler, creates Service nodes)
    - 6 unit tests all passing (test_parse_dynamodb_table, test_parse_sqs_queue, test_parse_lambda_function, test_parse_s3_bucket, test_parse_sns_topic, test_parse_resource_without_name)
    - Registered and exported in forge-survey/src/parser/mod.rs
    - All 145 workspace tests passing (including new Terraform tests)

- [x] **M3-T3**: Implement parser registry with auto-detection
  - **Files**: `forge-survey/src/parser/mod.rs`
  - **Implementation Notes**:
    - Complete ParserRegistry implementation with thread-safe Arc-based parser sharing
    - Auto-registers all built-in parsers (JavaScript, TypeScript, Python, Terraform)
    - JavaScript and TypeScript share the same parser instance
    - Case-insensitive language lookup
    - Exclusion list support
    - 34 comprehensive unit tests passing

- [x] **M3-T4**: Add automatic language detection to survey
  - **Files**: `forge-survey/src/detection.rs`, `forge-cli/src/commands/survey.rs`, `forge-survey/src/lib.rs`
  - **Implementation Notes**:
    - Implemented detection.rs with file extension scanning and config file detection
    - Detects JavaScript, TypeScript, Python, Terraform automatically
    - Threshold-based detection (≥3 files) with confidence scoring
    - Integrated into survey command - automatically selects appropriate parsers
    - Multi-language repos fully supported
    - 59 unit tests passing for detection module
    - Updated survey command to use ParserRegistry and language detection

- [x] **M3-T5**: Write unit tests for Python parser
  - **Files**: `forge-survey/src/parser/python.rs` (tests)
  - **Implementation Notes**:
    - 43 comprehensive unit tests covering all detection patterns
    - All tests passing

- [x] **M3-T6**: Write unit tests for Terraform parser
  - Test resource extraction
  - Test IAM policy parsing
  - Test ARN extraction
  - **Files**: `forge-survey/src/parser/terraform.rs` (tests)
  - **Implementation Notes**:
    - 6 comprehensive unit tests covering all resource types
    - Tests for DynamoDB table, SQS queue, SNS topic, S3 bucket, Lambda function detection
    - Tests for multi-resource files
    - All tests passing

- [x] **M3-T7**: Write integration test with mixed-language repos
  - Create test fixtures with JS, Python, and Terraform
  - Verify all languages contribute to graph
  - **Files**: `forge-survey/tests/integration_multi.rs`
  - **Implementation Notes**:
    - Created comprehensive integration test in `forge-survey/tests/integration_multi.rs`
    - Two test functions: `test_survey_multi_language_repo` and `test_survey_with_language_exclusion`
    - Tests multi-language survey with JavaScript, Python, and Terraform
    - Verifies language auto-detection from config files and file extensions
    - Tests database deduplication across languages
    - Tests service relationship detection
    - Tests language exclusion functionality
    - Bug fixes discovered and implemented during testing:
      - Fixed Python parser to extract DynamoDB table names from `dynamodb.Table('name')` pattern
      - Fixed JavaScript parser false positive where `axios.get()` was incorrectly detected as DynamoDB operation
      - Removed phantom resource discovery from import-only detection (DynamoDB, SQS, SNS now require actual method calls)

### Dependencies

```toml
# Additional to forge-survey/Cargo.toml
[dependencies]
tree-sitter-python = "0.20"
hcl-rs = "0.18"  # HCL/Terraform parser
```

### Parser Trait Contract

```rust
/// A parser that extracts discoveries from source code.
pub trait Parser: Send + Sync {
    /// Returns the languages/file types this parser handles.
    fn supported_extensions(&self) -> &[&str];

    /// Parse a single file and return discoveries.
    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError>;

    /// Parse an entire repository.
    fn parse_repo(&self, repo_path: &Path) -> Result<Vec<Discovery>, ParserError> {
        // Default implementation walks directory tree
    }
}
```

### Acceptance Criteria

- [x] Python parser detects boto3 DynamoDB/S3/SQS operations
- [x] Terraform parser extracts resource definitions
- [x] Languages are auto-detected from file extensions and config files (no manual config needed)
- [x] Survey correctly applies parsers based on auto-detected languages
- [x] Can exclude a language via `languages.exclude` in forge.yaml
- [x] Adding a new parser only requires implementing the trait

---

## Milestone 4: Implicit Coupling Detection

**Goal**: Detect shared resource coupling between services.

> **Detailed Specification**: [spec/m4-implicit-coupling.md](spec/m4-implicit-coupling.md)

### Tasks

- [x] **M4-T1**: Implement shared resource detection
  - For each Database/Queue node, track which services access it
  - Classify access: primary owner vs. shared reader/writer
  - **Files**: `forge-survey/src/coupling.rs`
  - **Implementation Notes**:
    - Created ResourceAccessMap struct with methods to track reads/writes
    - Created AccessEvidence struct to record access evidence
    - Implemented build_access_map() method in CouplingAnalyzer

- [x] **M4-T2**: Implement ownership inference
  - Heuristics: which service created resource (Terraform), naming conventions
  - Allow manual override via annotations
  - **Files**: `forge-survey/src/coupling.rs`
  - **Implementation Notes**:
    - Implemented infer_ownership() with three strategies:
      - Terraform definition (0.9 confidence)
      - Naming convention (0.7 confidence)
      - Exclusive writer (0.6 confidence)
    - Created OwnershipAssignment and OwnershipReason types

- [x] **M4-T3**: Generate IMPLICITLY_COUPLED edges
  - For services sharing a resource without explicit API
  - Include reason metadata (e.g., "both access users-table")
  - **Files**: `forge-survey/src/coupling.rs`
  - **Implementation Notes**:
    - Implemented detect_implicit_couplings() with risk level classification (Low/Medium/High)
    - Implemented generate_shared_access_edges() for READS_SHARED and WRITES_SHARED
    - Implemented CouplingAnalysisResult with apply_to_graph() method
    - Created ImplicitCoupling, SharedAccess, CouplingRisk types

- [x] **M4-T4**: Add coupling analysis to survey pipeline
  - Run after all parsers complete
  - Add coupling edges to graph
  - **Files**: `forge-cli/src/commands/survey.rs`
  - **Implementation Notes**:
    - CouplingAnalyzer is instantiated after graph building
    - analyzer.analyze() identifies implicit couplings, shared reads, and shared writes
    - Results are applied back to the graph via coupling_result.apply_to_graph()
    - High-risk couplings are reported with special formatting
    - Verbose mode shows detailed coupling information

- [x] **M4-T5**: Write unit tests for coupling detection
  - Test ownership inference
  - Test implicit coupling detection
  - **Files**: `forge-survey/src/coupling.rs` (tests)
  - **Implementation Notes**:
    - 20 comprehensive unit tests covering:
      - ResourceAccessMap (6 tests)
      - CouplingAnalyzer build_access_map (4 tests)
      - Implicit coupling detection (4 tests)
      - Ownership inference (2 tests)
      - Shared access edges (1 test)
      - Apply results to graph (2 tests)
      - Edge cases (1 test)
    - All tests passing (197 total workspace tests)

- [x] **M4-T6**: Write integration test for coupling scenarios
  - Create fixtures with shared DynamoDB patterns
  - Verify IMPLICITLY_COUPLED edges created
  - **Files**: `forge-survey/tests/integration_coupling.rs`
  - **Implementation Notes**:
    - 7 comprehensive integration tests covering:
      - test_coupling_shared_dynamodb_table: Services sharing DynamoDB (write/read → Medium risk)
      - test_coupling_shared_sqs_queue: Services sharing SQS queue (publisher/consumer)
      - test_high_risk_multiple_writers: Multiple writers → High risk
      - test_coupling_edges_applied_to_graph: Edge types verification
      - test_coupling_python_services: Cross-language coupling (Python services)
      - test_no_coupling_when_no_shared_resources: No false positives
      - test_low_risk_both_readers: Both readers → Low risk
    - Added AWS SDK v3 Command pattern detection to JavaScript parser:
      - DynamoDB commands: GetItemCommand, PutItemCommand, UpdateItemCommand, etc.
      - SQS commands: SendMessageCommand, ReceiveMessageCommand
    - Fixed coupling detection to include owner in coupling pairs (was incorrectly excluding owner)
    - Removed phantom DynamoDB discoveries from imports (only detect from actual operations)
    - All 7 tests passing

### Acceptance Criteria

- [x] Services reading the same DynamoDB table get IMPLICITLY_COUPLED edge
- [x] Services sharing SQS queues get IMPLICITLY_COUPLED edge
- [x] Ownership is correctly inferred from Terraform definitions
- [x] Coupling reasons are recorded in edge metadata
- [x] Graph visualizes coupling relationships (via Mermaid serializer in M5-T4)
- [x] Coupling analysis integrated into survey command pipeline

---

## Milestone 5: Serialization

**Goal**: Multiple output formats, token budgeting.

> **Detailed Specification**: [spec/m5-serialization.md](spec/m5-serialization.md)

### Tasks

- [x] **M5-T1**: Implement subgraph extraction
  - Extract by service names (include neighbors)
  - Extract by path pattern (APIs matching route)
  - Relevance scoring based on edge distance
  - **Files**: `forge-graph/src/query.rs`
  - **Implementation Notes**:
    - Created `SubgraphConfig` struct for configuring extraction (seed nodes, max depth, min relevance, edge type filtering)
    - Created `ScoredNode` struct for nodes with relevance scores and depth
    - Created `ExtractedSubgraph` struct for holding extraction results
    - Implemented `extract_subgraph()` method using BFS with depth-limited relevance decay
    - Implemented `edge_relevance_decay()` function with configurable decay rates per edge type:
      - CALLS: 0.8, OWNS: 0.9, READS/WRITES: 0.75, READS_SHARED/WRITES_SHARED: 0.7
      - PUBLISHES/SUBSCRIBES: 0.65, USES: 0.6, IMPLICITLY_COUPLED: 0.5
    - Supports bidirectional traversal (outgoing edges at full decay, incoming edges at 0.7x multiplier)
    - Nodes sorted by relevance score (descending)
    - Edges filtered to only include those between extracted nodes
    - Exported types in forge-graph lib.rs
    - 17 comprehensive unit tests covering all functionality
    - All 69 forge-graph tests passing

- [x] **M5-T2**: Implement Markdown serializer
  - Service-centric view with sections
  - Relationship tables
  - Optimized for LLM context consumption
  - **Files**: `forge-cli/src/serializers/markdown.rs`, `forge-cli/src/serializers/mod.rs`
  - **Implementation Notes**:
    - Created `serializers` module in forge-cli with `MarkdownSerializer` struct
    - Sections: Services, Databases, Queues, Cloud Resources, APIs, Implicit Couplings
    - Full dependency tables with evidence (file:line)
    - Business context display (purpose, owner, history, gotchas)
    - Subgraph serialization with relevance indicators (HIGH/MEDIUM/LOW)
    - Configurable: detail level, evidence inclusion, max evidence items
    - 21 comprehensive unit tests covering all serialization scenarios
    - Integrated with `forge map` command for markdown output

- [x] **M5-T3**: Implement JSON serializer
  - Structured format for tool-based LLM queries
  - Schema-documented output
  - **Files**: `forge-cli/src/serializers/json.rs`
  - **Implementation Notes**:
    - Created `JsonSerializer` struct in forge-cli/src/serializers/json.rs
    - Output follows documented schema with $schema, version, generated_at fields
    - Supports full graph serialization and subgraph extraction with relevance scores
    - Query info included (type, seeds, max_depth)
    - Summary statistics (total_nodes, total_edges, by_type)
    - Business context serialization support
    - Chrono dependency added for RFC3339 timestamps
    - 15 comprehensive unit tests covering all serialization scenarios
    - Integrated with `forge map --format json` command
    - All tests passing

- [x] **M5-T7**: Implement `forge map` command
  - `--format` flag (markdown, json, mermaid)
  - `--service` flag for filtering
  - `--budget` flag for token limit
  - `--output` flag or stdout
  - **Files**: `forge-cli/src/commands/map.rs`
  - **Implementation Notes**:
    - Full implementation of map command in forge-cli
    - Loads graph from configured path or --input override
    - Supports --format flag (markdown, json, and mermaid all working)
    - Service filtering via --service flag (comma-separated names)
    - Subgraph extraction using relevance-scored algorithm from M5-T1
    - Output to file via --output or stdout by default
    - 14 comprehensive unit tests (including JSON and Mermaid format tests)
    - All tests passing

- [x] **M5-T4**: Implement Mermaid serializer
  - Flowchart diagram syntax
  - Color-coding by node type
  - **Files**: `forge-cli/src/serializers/mermaid.rs`
  - **Implementation Notes**:
    - Created `MermaidSerializer` struct in forge-cli/src/serializers/mermaid.rs
    - Produces Mermaid flowchart syntax with configurable direction (LR, RL, TB, BT)
    - Groups nodes by type into subgraphs (Services, Databases, Queues, Resources, APIs)
    - Node shapes: Services (rectangle), Databases (cylinder), Queues (asymmetric), CloudResources (hexagon), APIs (stadium)
    - Edge styles: solid arrows for normal edges, dotted arrows for implicit couplings
    - CSS class styling with color-coding per node type
    - Configurable: direction, include_attributes, max_nodes, include_styles
    - Label building with attribute support (language/framework for services, db_type for databases, queue_type for queues)
    - 18 comprehensive unit tests covering all serialization scenarios
    - Integrated with `forge map --format mermaid` command
    - 4 new tests added to map command for Mermaid format
    - All tests passing

- [x] **M5-T5**: Implement token counting
  - Use tiktoken-rs for accurate OpenAI tokenization
  - Estimate for Claude (similar tokenizer)
  - **Files**: `forge-cli/src/token_budget.rs`
  - **Implementation Notes**:
    - Created `TokenCounter` struct using tiktoken-rs cl100k_base encoding
    - `count(&self, text: &str) -> usize` method for accurate token counting
    - `estimate_node_tokens(&self, node, detail_level)` method with Full/Summary/Minimal estimation
    - `estimate_edge_tokens()` returns ~30 tokens per edge
    - `estimate_subgraph_tokens()` for full subgraph estimation
    - Accuracy is within ±5% of tiktoken (matching spec requirement)
    - 20 comprehensive unit tests covering all functionality
    - All tests passing

- [x] **M5-T6**: Implement token-budgeted output
  - Truncate/summarize to fit budget
  - Prioritize by relevance score
  - **Files**: `forge-cli/src/token_budget.rs`
  - **Implementation Notes**:
    - Created `BudgetedSerializer` struct that respects token limits
    - `serialize_within_budget(&self, subgraph, format)` method
    - Nodes included in order of relevance score (highest first)
    - Detail level adjusted based on relevance: >0.7 Full, 0.4-0.7 Summary, <0.4 Minimal
    - Edges included only if both source and target nodes are included
    - Supports Markdown, JSON, and Mermaid output formats
    - `fits_within_budget()` and `estimate_tokens()` helper methods
    - Tests verify budget constraints are respected
    - All tests passing

- [x] **M5-T8**: Write tests for serializers
  - Round-trip tests where applicable
  - Token count accuracy tests
  - **Files**: `forge-cli/src/serializers/*.rs` (tests), `forge-cli/src/token_budget.rs` (tests)
  - **Implementation Notes**:
    - Markdown serializer: 21 tests
    - JSON serializer: 15 tests
    - Mermaid serializer: 18 tests
    - Token budget: 20 tests including accuracy validation
    - All 74+ serializer-related tests passing

### Dependencies

```toml
# forge-cli/Cargo.toml additions
[dependencies]
tiktoken-rs = "0.6"
```

### Acceptance Criteria

- [x] `forge map --format markdown` produces readable service docs
- [x] `forge map --format json` produces structured data
- [x] `forge map --format mermaid` produces valid diagram syntax
- [x] `forge map --budget 4000` stays under token limit (via BudgetedSerializer)
- [x] Subgraph extraction correctly filters by service
- [x] `forge map --service` filters output to specific services
- [x] Token counting is accurate within ±5% of tiktoken

---

## Milestone 6: Business Context

**Goal**: LLM-assisted interview for business context annotation.

> **Detailed Specification**: [spec/m6-business-context.md](spec/m6-business-context.md)

### Architecture Note: Coding Agent CLI Adapters

Forge integrates with LLMs by **shelling out to coding agent CLIs** (e.g., `claude` from Claude Code, `gemini`, `codex`) rather than making direct API calls. This approach:

- **Leverages existing authentication**: Users already have their CLI configured with API keys
- **No API keys in forge.yaml**: Security is handled by the coding agent CLI
- **Provider-agnostic**: Any CLI that accepts stdin prompts and outputs responses works
- **Subprocess management**: Communication via stdin/stdout piping

### Tasks

- [x] **M6-T1**: Define LLM provider trait
  - `trait LLMProvider { async fn prompt(&self, system: &str, user: &str) -> Result<String>; }`
  - **Subprocess-based execution (NOT direct API calls)**
  - Manage stdin/stdout piping to coding agent CLIs
  - **Files**: `forge-llm/src/provider.rs`
  - **Implementation Notes**:
    - Created `LLMProvider` async trait with `name()`, `is_available()`, `prompt()`, and `prompt_with_history()` methods
    - Created `LLMError` enum with variants: ProcessFailed, NonZeroExit, InvalidOutput, CliNotFound, Timeout, NotConfigured, Io
    - Created `Message` struct with `Role` enum for conversation history support
    - Added `MockProvider` for testing with 5 unit tests passing
    - Dependencies: async-trait, thiserror, tokio (process, io-util, time), serde

- [x] **M6-T2**: Implement Claude CLI adapter
  - Shell out to `claude` CLI (Claude Code's CLI tool)
  - Pipe prompt via stdin, read response from stdout
  - Handle streaming output
  - Leverage user's existing Claude authentication
  - **Files**: `forge-llm/src/adapters/claude.rs`, `forge-llm/src/adapters/base.rs`, `forge-llm/src/adapters/mod.rs`
  - **Implementation Notes**:
    - Created `CliAdapter` base struct in `forge-llm/src/adapters/base.rs` with:
      - Builder pattern for configuration (command, timeout, extra args)
      - `check_available()` method using `which` to verify CLI exists
      - `execute()` method for stdin/stdout subprocess communication with timeout
      - Cross-platform support (Unix `which`, Windows `where`)
    - Created `ClaudeAdapter` in `forge-llm/src/adapters/claude.rs`:
      - Wraps `CliAdapter` with Claude-specific configuration
      - Uses `--print` flag for non-interactive mode
      - Default 180-second timeout
      - `format_claude_prompt()` for system/user prompt formatting
      - `format_history()` for multi-turn conversation support
      - Implements full `LLMProvider` trait including `prompt_with_history()`
    - Created adapters module (`forge-llm/src/adapters/mod.rs`) with re-exports
    - Updated `forge-llm/src/lib.rs` with:
      - `LLMConfig` struct for provider configuration
      - `create_provider()` factory function
      - `create_and_verify_provider()` async factory with availability check
    - 31 unit tests passing covering:
      - Base adapter building and execution
      - Claude adapter prompt formatting
      - Provider factory (create, verify, unknown provider handling)
      - Error handling (CLI not found, process failed)

- [x] **M6-T3**: Implement Gemini CLI adapter
  - Shell out to Gemini CLI tool
  - Same stdin/stdout pattern as Claude adapter
  - **Files**: `forge-llm/src/adapters/gemini.rs`
  - **Implementation Notes**:
    - Created `GeminiAdapter` struct wrapping `CliAdapter` base
    - Uses simpler prompt format than Claude (system + user combined)
    - Default 180-second timeout
    - Formats conversation history as `User: ... / Model: ...`
    - Implements full `LLMProvider` trait including `prompt_with_history()`
    - Added to adapters module with re-export
    - Updated `create_provider()` factory to support "gemini" provider
    - 12 unit tests passing covering adapter functionality
    - 45 total forge-llm tests passing

- [x] **M6-T4**: Implement Codex CLI adapter
  - Shell out to Codex CLI tool
  - Same stdin/stdout pattern as Claude adapter
  - **Files**: `forge-llm/src/adapters/codex.rs`
  - **Implementation Notes**:
    - Created `CodexAdapter` struct wrapping `CliAdapter` base
    - Uses `System: ... User: ...` prompt format for OpenAI-style interaction
    - Default 180-second timeout
    - Formats conversation history as `User: ... / Assistant: ...`
    - Implements full `LLMProvider` trait including `prompt_with_history()`
    - Added to adapters module with re-export
    - Updated `create_provider()` factory to support "codex" provider
    - 12 unit tests passing covering adapter functionality
    - 59 total forge-llm tests passing

- [x] **M6-T5**: Implement provider selection from config
  - Read `llm.provider` from forge.yaml
  - Instantiate appropriate CLI adapter
  - Validate CLI is installed and accessible
  - **Files**: `forge-llm/src/lib.rs`
  - **Implementation Notes**:
    - Implemented `LLMConfig` struct with `provider` and `cli_path` fields
    - Implemented `create_provider()` factory function for synchronous provider creation
    - Implemented `create_and_verify_provider()` async factory that checks CLI availability
    - Supports "claude", "gemini", and "codex" providers
    - 15 unit tests covering factory functionality

- [x] **M6-T6**: Implement gap analysis
  - Identify nodes lacking business context
  - Prioritize by centrality (heavily connected = important)
  - **Files**: `forge-llm/src/interview.rs`
  - **Implementation Notes**:
    - Created `forge-llm/src/interview.rs` module with full gap analysis
    - Implemented `ContextGapScore` struct with node_id, score (0.0-1.0), and reasons
    - Implemented `GapReason` enum: MissingPurpose, MissingOwner, HighCentrality, ImplicitCoupling, SharedResourceWithoutOwner, ComplexWithoutGotchas
    - Implemented `GapAnalysisConfig` struct for configurable thresholds
    - Implemented `analyze_gaps()` and `analyze_gaps_with_config()` functions
    - Analyzes services for: missing purpose/owner, high centrality, implicit couplings, complex services without gotchas
    - Analyzes databases/queues for: shared access without clear ownership
    - Score contributions: MissingPurpose (0.3), MissingOwner (0.2), HighCentrality (up to 0.2), ImplicitCoupling (0.15), SharedResource (0.25), ComplexWithoutGotchas (0.1)
    - Results sorted by score (highest first)
    - 17 comprehensive unit tests covering all gap detection scenarios
    - 74 total forge-llm tests passing

- [x] **M6-T7**: Implement question generation
  - Template-based questions for different node types
  - E.g., "What business function does {service} serve?"
  - **Files**: `forge-llm/src/interview.rs`
  - **Implementation Notes**:
    - Created `InterviewQuestion` struct with node_id, question, annotation_type, priority (1-10), context
    - Created `AnnotationType` enum: Purpose, Owner, History, Gotcha, Note
    - Implemented `generate_questions()` function that generates questions from gap analysis results
    - Implemented `generate_all_questions()` convenience function to analyze entire graph
    - Implemented helper functions for each question type:
      - `generate_purpose_question()` - asks about business purpose, includes dependency context
      - `generate_owner_question()` - asks about ownership/responsibility
      - `generate_centrality_question()` - asks why a central service has many connections
      - `generate_coupling_question()` - asks about implicit coupling coordination
      - `generate_shared_resource_question()` - asks about resource ownership
      - `generate_gotcha_question()` - asks about known issues and operational concerns
    - Questions sorted by priority (highest first)
    - Priority levels: Purpose (9), Centrality (8), Shared Resource (8), Owner (7), Coupling (7), Gotcha (5)
    - 12 comprehensive unit tests covering all question generation scenarios
    - Exported `InterviewQuestion`, `AnnotationType`, `generate_questions`, `generate_all_questions` from lib.rs
    - 85 total forge-llm tests passing

- [ ] **M6-T8**: Implement interview flow
  - Interactive terminal UI
  - Present question, collect answer, update graph
  - Save after each answer (interrupt-safe)
  - **Files**: `forge-llm/src/interview.rs`

- [ ] **M6-T9**: Implement annotation persistence
  - Store business context as node attributes
  - Preserve across re-surveys (merge strategy)
  - **Files**: `forge-graph/src/node.rs`, `forge-survey/src/lib.rs`

- [ ] **M6-T10**: Add `--business-context` flag to survey
  - Trigger interview after technical survey
  - **Files**: `forge-cli/src/commands/survey.rs`

- [ ] **M6-T11**: Write tests with mocked LLM responses
  - Test interview flow
  - Test annotation persistence
  - **Files**: `forge-llm/tests/interview_test.rs`

### Dependencies

```toml
# forge-llm/Cargo.toml
[dependencies]
tokio = { version = "1.0", features = ["process", "io-util"] }  # For subprocess management
async-trait = "0.1"
forge-graph = { path = "../forge-graph" }
# Note: No LLM SDK crates needed - we shell out to coding agent CLIs
```

### Acceptance Criteria

- [ ] `forge survey --business-context` launches interview
- [ ] Interview questions are contextual and useful
- [ ] Annotations persist across multiple survey runs
- [ ] Can switch LLM provider (coding agent CLI) via config
- [ ] Works even when LLM CLI is unavailable (graceful skip with warning)
- [ ] Leverages existing CLI authentication (no API keys in forge.yaml)

---

## Milestone 7: Polish

**Goal**: Production-ready V1 with incremental survey and documentation.

> **Detailed Specification**: [spec/m7-polish.md](spec/m7-polish.md)

### Tasks

- [ ] **M7-T1**: Implement incremental survey
  - Track file hashes or git commit SHAs
  - Only re-parse changed files
  - Merge changes into existing graph
  - **Files**: `forge-survey/src/incremental.rs`

- [ ] **M7-T2**: Implement staleness indicators
  - Track last-surveyed timestamp per node
  - Mark stale nodes in output
  - **Files**: `forge-graph/src/node.rs`

- [ ] **M7-T3**: Improve CLI UX
  - Progress bars with indicatif
  - Colored output
  - Error messages with suggestions
  - **Files**: `forge-cli/src/main.rs`

- [ ] **M7-T4**: Add `--verbose` and `--quiet` flags
  - Control output verbosity
  - **Files**: `forge-cli/src/main.rs`

- [ ] **M7-T5**: Write README.md
  - Installation instructions
  - Quick start guide
  - Usage examples
  - **Files**: `README.md`

- [ ] **M7-T6**: Write CLI reference documentation
  - All commands and flags
  - Examples for each
  - **Files**: `docs/cli-reference.md`

- [ ] **M7-T7**: Write configuration reference
  - Full forge.yaml schema
  - All options with descriptions
  - **Files**: `docs/configuration.md`

- [ ] **M7-T8**: Write parser extension guide
  - Step-by-step for adding new language
  - Trait requirements
  - Testing guidelines
  - **Files**: `docs/extending-parsers.md`

- [ ] **M7-T9**: Write LLM provider extension guide
  - How to add new CLI adapter
  - **Files**: `docs/extending-llm-providers.md`

- [ ] **M7-T10**: Create example forge.yaml configurations
  - Minimal example
  - Full-featured example
  - Multi-org example
  - **Files**: `examples/`

- [ ] **M7-T11**: Final integration testing
  - End-to-end with synthetic test repos
  - All commands exercised
  - **Files**: `tests/e2e/`

### Dependencies

```toml
# forge-cli/Cargo.toml additions
[dependencies]
indicatif = "0.17"
console = "0.15"
```

### Acceptance Criteria

- [ ] Re-running survey is significantly faster when few files changed
- [ ] Documentation is complete and accurate
- [ ] A new user can follow README and get working output
- [ ] All tests pass in CI
- [ ] No panics on malformed input

---

## Testing Strategy

### Unit Tests

Every crate must have unit tests covering:

| Crate | Coverage Areas |
|-------|----------------|
| forge-graph | Node/edge creation, serialization, graph operations, queries |
| forge-survey | Each parser independently, coupling detection |
| forge-llm | Provider trait mock implementation, interview logic |
| forge-cli | Config parsing, serializers |

**Guidelines:**
- Use inline test fixtures (not external files) for parser tests
- Mock external services (GitHub API, LLM CLIs)
- Aim for >80% line coverage on core logic

### Integration Tests

Located in `*/tests/` directories:

| Test Suite | Description |
|------------|-------------|
| `integration_js.rs` | Survey of synthetic JS repo |
| `integration_python.rs` | Survey of synthetic Python repo |
| `integration_terraform.rs` | Survey of synthetic Terraform config |
| `integration_multi.rs` | Survey of multi-language repo |
| `integration_coupling.rs` | Shared resource coupling detection |
| `e2e/full_workflow.rs` | Complete survey → map workflow |

**Synthetic Test Repos:**
- Created as test fixtures in `tests/fixtures/`
- NOT real user repos
- Designed to exercise specific patterns

### CI Pipeline

```yaml
# .github/workflows/ci.yml
name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --workspace
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --check
```

---

## Dependencies Summary

### External Crates

| Crate | Version | Purpose |
|-------|---------|---------|
| petgraph | 0.6 | Graph data structure |
| serde | 1.0 | Serialization framework |
| serde_json | 1.0 | JSON serialization |
| serde_yaml | 0.9 | YAML configuration |
| clap | 4.4 | CLI argument parsing |
| tokio | 1.0 | Async runtime |
| tree-sitter | 0.20 | Parsing framework |
| tree-sitter-javascript | 0.20 | JS/TS parsing |
| tree-sitter-python | 0.20 | Python parsing |
| hcl2 | 0.4 | Terraform HCL parsing |
| octocrab | 0.32 | GitHub API client |
| regex | 1.10 | Pattern matching |
| walkdir | 2.4 | Directory traversal |
| tiktoken-rs | 0.5 | Token counting |
| indicatif | 0.17 | Progress bars |
| console | 0.15 | Terminal colors |
| thiserror | 1.0 | Error handling |
| uuid | 1.0 | Unique identifiers |
| async-trait | 0.1 | Async trait support |

### Development Tools

- Rust 1.75+ (2024 edition)
- Git
- GitHub account (for testing API integration)

---

## Configuration Schema Reference

### Complete forge.yaml

```yaml
# Forge configuration file
# See docs/configuration.md for full reference

# ============================================
# Repository Sources
# ============================================
repos:
  # GitHub organization - discovers all repos automatically
  github_org: "my-company"

  # Or explicit list of repos (overrides github_org)
  github_repos:
    - "my-company/api-gateway"
    - "my-company/user-service"
    - "my-company/order-service"

  # Or local paths (for testing or air-gapped environments)
  local_paths:
    - "./repos/local-service"

  # Exclude patterns (applies to all sources)
  exclude:
    - "*-deprecated"
    - "*.archive"

# ============================================
# GitHub Configuration
# ============================================
github:
  # Environment variable containing PAT (default: GITHUB_TOKEN)
  token_env: "GITHUB_TOKEN"

  # API base URL (for GitHub Enterprise)
  # api_url: "https://github.mycompany.com/api/v3"

# ============================================
# Language Detection (Auto-Detected by Default)
# ============================================
# Languages are AUTOMATICALLY detected from:
#   - File extensions (.js, .ts, .py, .tf, etc.)
#   - Config files (package.json, requirements.txt, pyproject.toml, etc.)
#
# You do NOT need to configure this section for normal usage.
# Only configure if you need to exclude specific languages.
languages:
  # Exclude specific languages from detection (optional)
  exclude:
    - terraform  # Example: skip Terraform if not using IaC

  # Language-specific configuration (optional)
  javascript:
    # Additional patterns for AWS SDK detection
    aws_sdk_patterns:
      - "@aws-sdk/client-dynamodb"
      - "@aws-sdk/lib-dynamodb"

  python:
    # Virtual environment paths to ignore
    ignore_venvs: true

# ============================================
# Output Configuration
# ============================================
output:
  # Where to save the graph (relative to cwd or absolute)
  graph_path: ".forge/graph.json"

  # Where to cache cloned repos
  cache_path: "~/.forge/repos"

  # Default map format
  default_format: "markdown"

# ============================================
# LLM Configuration (for business context)
# ============================================
llm:
  # Provider: claude | gemini | codex
  provider: "claude"

  # CLI path override (if not in PATH)
  # cli_path: "/usr/local/bin/claude"

# ============================================
# Token Budget
# ============================================
# Default token budget for map output
token_budget: 8000

# ============================================
# Survey Behavior
# ============================================
survey:
  # Enable incremental mode (only parse changed files)
  incremental: true

  # Staleness threshold in days (mark nodes not seen in N days)
  staleness_days: 7
```

---

## Extension Guide: Adding a New Language Parser

### Step 1: Create Parser File

Create `forge-survey/src/parser/{language}.rs`:

```rust
use super::traits::{Parser, Discovery, ParserError};
use std::path::Path;
use tree_sitter::{Parser as TSParser, Language};

// Link the tree-sitter grammar
extern "C" { fn tree_sitter_mylang() -> Language; }

pub struct MyLangParser {
    parser: TSParser,
}

impl MyLangParser {
    pub fn new() -> Result<Self, ParserError> {
        let mut parser = TSParser::new();
        parser.set_language(unsafe { tree_sitter_mylang() })?;
        Ok(Self { parser })
    }
}

impl Parser for MyLangParser {
    fn supported_extensions(&self) -> &[&str] {
        &["mylang", "ml"]
    }

    fn parse_file(&self, path: &Path, content: &str) -> Result<Vec<Discovery>, ParserError> {
        let tree = self.parser.parse(content, None)
            .ok_or(ParserError::ParseFailed)?;

        let mut discoveries = Vec::new();

        // Walk AST and extract discoveries
        // ... implementation ...

        Ok(discoveries)
    }
}
```

### Step 2: Register Parser

In `forge-survey/src/parser/mod.rs`:

```rust
mod mylang;
pub use mylang::MyLangParser;

pub fn create_parser(language: &str) -> Option<Box<dyn Parser>> {
    match language {
        "javascript" | "typescript" => Some(Box::new(JavaScriptParser::new().ok()?)),
        "python" => Some(Box::new(PythonParser::new().ok()?)),
        "terraform" => Some(Box::new(TerraformParser::new().ok()?)),
        "mylang" => Some(Box::new(MyLangParser::new().ok()?)),  // Add this
        _ => None,
    }
}
```

### Step 3: Add Dependency

In `forge-survey/Cargo.toml`:

```toml
[dependencies]
tree-sitter-mylang = "0.1"  # Or path to local grammar
```

### Step 4: Write Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_detection() {
        let parser = MyLangParser::new().unwrap();
        let content = r#"import foo from "bar""#;
        let discoveries = parser.parse_file(Path::new("test.ml"), content).unwrap();
        assert!(discoveries.iter().any(|d| matches!(d, Discovery::Import { .. })));
    }
}
```

---

## Extension Guide: Adding a New LLM Provider (Coding Agent CLI)

Forge adapters shell out to **coding agent CLIs** (not direct API calls). This leverages the user's existing authentication and avoids storing API keys in forge.yaml.

### Step 1: Create Adapter File

Create `forge-llm/src/adapters/{provider}.rs`:

```rust
use super::super::provider::{LLMProvider, LLMError};
use async_trait::async_trait;
use tokio::process::Command;
use tokio::io::AsyncWriteExt;

pub struct MyProviderAdapter {
    cli_path: String,
}

impl MyProviderAdapter {
    pub fn new(cli_path: Option<String>) -> Self {
        Self {
            // Default to CLI command name (must be in user's PATH)
            cli_path: cli_path.unwrap_or_else(|| "myprovider".to_string()),
        }
    }
}

#[async_trait]
impl LLMProvider for MyProviderAdapter {
    /// Shell out to coding agent CLI with stdin/stdout piping
    async fn prompt(&self, system: &str, user: &str) -> Result<String, LLMError> {
        // Spawn the CLI process
        let mut child = Command::new(&self.cli_path)
            .arg("--system")
            .arg(system)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| LLMError::ProcessFailed(format!(
                "Failed to spawn '{}': {}. Is the CLI installed?",
                self.cli_path, e
            )))?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(user.as_bytes()).await
                .map_err(|e| LLMError::ProcessFailed(e.to_string()))?;
        }

        // Wait for response
        let output = child.wait_with_output().await
            .map_err(|e| LLMError::ProcessFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LLMError::NonZeroExit(output.status.code(), stderr.to_string()));
        }

        String::from_utf8(output.stdout)
            .map_err(|e| LLMError::InvalidOutput(e.to_string()))
    }
}
```

### Step 2: Register Provider

In `forge-llm/src/lib.rs`:

```rust
pub fn create_provider(name: &str, config: &LLMConfig) -> Option<Box<dyn LLMProvider>> {
    match name {
        "claude" => Some(Box::new(ClaudeAdapter::new(config.cli_path.clone()))),
        "gemini" => Some(Box::new(GeminiAdapter::new(config.cli_path.clone()))),
        "myprovider" => Some(Box::new(MyProviderAdapter::new(config.cli_path.clone()))),
        _ => None,
    }
}
```

---

## Open Questions / Decisions During Implementation

These items should be resolved during implementation:

1. **Tree-sitter vs. regex for simple patterns**: For AWS SDK detection, is full AST parsing necessary or would regex suffice?

2. **Graph merge strategy**: When re-surveying, how to handle nodes that disappeared (deleted service)? Mark stale vs. remove?

3. **Token counting accuracy**: tiktoken-rs is OpenAI's tokenizer. Claude's is similar but not identical. Accept approximation or find Claude-specific solution?

4. **GitHub rate limiting**: How to handle rate limits gracefully? Exponential backoff? Local caching?

5. **Large monorepo handling**: Some repos may be very large. Stream parsing or memory limits?

6. **Concurrent parsing**: How many files/repos to parse in parallel? Configurable?

7. **Error aggregation**: When multiple parsers fail, how to report errors? Fail fast vs. collect all?

---

## Appendix: Command Reference

### forge init

Initialize a new forge.yaml configuration file.

```
forge init [OPTIONS]

Options:
  --org <ORG>       Pre-fill GitHub organization
  --output <PATH>   Output path (default: ./forge.yaml)
  --force           Overwrite existing file
```

### forge survey

Survey repositories and build the knowledge graph.

```
forge survey [OPTIONS]

Options:
  --config <PATH>           Config file (default: ./forge.yaml)
  --output <PATH>           Override output graph path
  --repos <REPOS>           Override repos (comma-separated)
  --exclude-lang <LANGS>    Exclude languages (comma-separated, e.g., "terraform")
  --business-context        Launch business context interview after survey (uses LLM CLI)
  --incremental             Only re-parse changed files
  --verbose                 Show detailed progress

Note: Languages are auto-detected from file extensions and config files.
The survey phase is deterministic (tree-sitter only) - no LLM calls unless --business-context is specified.
```

### forge map

Serialize the knowledge graph to various formats.

```
forge map [OPTIONS]

Options:
  --config <PATH>           Config file (default: ./forge.yaml)
  --input <PATH>            Override input graph path
  --format <FORMAT>         Output format: markdown|json|mermaid
  --service <SERVICES>      Filter to specific services
  --budget <TOKENS>         Token budget limit
  --output <PATH>           Output file (default: stdout)
```
