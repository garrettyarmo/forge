# Forge Specifications

Design documentation for Forge, a reusable platform for surveying and mapping software ecosystems.

## Core Architecture

### Knowledge Graph (forge-graph)
- **Node Types**: Service, API, Database, Queue, CloudResource
- **Edge Types**: Calls, Owns, Reads, Writes, Publishes, Subscribes, Uses, ReadsShared, WritesShared, ImplicitlyCoupled
- **Graph Operations**: Add/remove nodes and edges, queries for traversal, path finding, subgraph extraction
- **Persistence**: JSON serialization for saving and loading graphs
- **Status**: Fully implemented and tested (Milestone 1 complete)

### Survey Engine (forge-survey)
- **Parser Architecture**: Trait-based system for language-specific parsers
  - JavaScriptParser: Detects services, imports, AWS SDK usage, DynamoDB operations, HTTP calls
  - Status: JavaScript/TypeScript parser complete (M2-T6)
  - PythonParser: Detects services, imports, AWS SDK usage (boto3), DynamoDB operations, HTTP calls
  - Status: Python parser complete (M3-T1)
  - TerraformParser: Detects AWS resources (DynamoDB, SQS, SNS, S3, Lambda)
  - Status: Terraform parser complete (M3-T2)
- **GitHub Integration**: Clone and cache repositories from GitHub organizations
  - RepoCache: Manages local repository copies with automatic pulling
  - GitHubClient: Lists repos using octocrab
  - Status: Fully implemented (M2-T4)
- **GraphBuilder**: Converts parser discoveries into knowledge graph
  - Maps ServiceDiscovery → Service nodes
  - Maps DatabaseAccessDiscovery → Database nodes + READS/WRITES edges
  - Maps QueueOperationDiscovery → Queue nodes + PUBLISHES/SUBSCRIBES edges
  - Maps CloudResourceDiscovery → CloudResource nodes + USES edges
  - Deduplication: Tracks services and resources across repos to avoid duplicates
  - Incremental updates: Can load existing graph and merge new discoveries
  - Status: Fully implemented and tested with 6 passing tests (M2-T7)

### Configuration System (forge-cli)
- **forge.yaml**: YAML-based configuration for repository sources, GitHub settings, output paths
- **forge init**: Generates default configuration with comments
- **Environment overrides**: Supports FORGE_* environment variables
- **Status**: Configuration and init command complete (M2-T1, M2-T2, M2-T3)

### Serialization System (forge-cli)
- **MarkdownSerializer**: Human-readable output optimized for LLM context consumption
  - Service-centric sections with dependency tables
  - Business context display (purpose, owner, history, gotchas)
  - Implicit coupling risk summary
  - Subgraph serialization with relevance indicators
  - Status: Complete (M5-T2)
- **JsonSerializer**: Structured JSON output for programmatic access
  - Schema-documented output with $schema, version, generated_at fields
  - Query info for subgraph extractions (type, seeds, max_depth)
  - Summary statistics (total_nodes, total_edges, by_type)
  - Business context serialization
  - Status: Complete (M5-T3)
- **MermaidSerializer**: Visual diagram syntax for documentation
  - Flowchart diagram with configurable direction (LR, RL, TB, BT)
  - Node grouping into subgraphs (Services, Databases, Queues, Resources, APIs)
  - Shape coding: Services (rectangle), Databases (cylinder), Queues (asymmetric), CloudResources (hexagon)
  - Edge styling: solid arrows for normal edges, dotted for implicit couplings
  - CSS class styling with color-coding per node type
  - Status: Complete (M5-T4)
- **forge map command**: Serialize knowledge graphs to various formats
  - `--format` flag (markdown, json, mermaid)
  - `--service` flag for filtering to specific services
  - `--output` flag for file output (default: stdout)
  - Relevance-scored subgraph extraction
  - Status: All formats complete (M5-T7)

## Survey Phase Implementation

The survey phase is **purely deterministic** using tree-sitter AST parsing only - no LLM calls. This ensures:
- **Reproducibility**: Same input code always produces the same graph
- **Speed**: No API latency or rate limits
- **Offline capability**: Works without network for local repos
- **Predictable costs**: Zero token usage during survey

## Observability Suite

_To be implemented in future milestones_

## LLM Integration

_Milestone 6: Business context interview using CLI adapters for Claude, Gemini, Codex_

## Configuration & Security

- GitHub token authentication via environment variables (no secrets in forge.yaml)
- Local repository caching in ~/.forge/repos
- Graph output to .forge/graph.json (configurable)

## Implementation Status

### Completed Milestones
- ✅ **Milestone 1 (Foundation)**: Knowledge graph data structures, JSON persistence
- ✅ **Milestone 2 (Survey Core)**: Complete
  - ✅ M2-T1: forge.yaml schema
  - ✅ M2-T2: Configuration loading
  - ✅ M2-T3: forge init command
  - ✅ M2-T4: GitHub API client
  - ✅ M2-T5: Parser trait
  - ✅ M2-T6: JavaScript/TypeScript parser
  - ✅ M2-T7: Discovery-to-graph mapper (GraphBuilder)
  - ✅ M2-T8: forge survey command
  - ✅ M2-T9: JavaScript parser unit tests
  - ✅ M2-T10: Integration test with synthetic JS repo (6 tests passing)
- ✅ **Milestone 3 (Multi-Language Support)**: Complete
  - ✅ M3-T1: Python parser
  - ✅ M3-T2: Terraform parser
  - ✅ M3-T3: Parser registry with auto-detection
  - ✅ M3-T4: Automatic language detection in survey
  - ✅ M3-T5: Python parser tests
  - ✅ M3-T6: Terraform parser tests
  - ✅ M3-T7: Multi-language integration tests
- ✅ **Milestone 4 (Implicit Coupling)**: Complete
  - ✅ M4-T1: Shared resource detection
  - ✅ M4-T2: Ownership inference
  - ✅ M4-T3: IMPLICITLY_COUPLED edge generation
  - ✅ M4-T4: Coupling analysis in survey pipeline
  - ✅ M4-T5: Coupling detection unit tests
  - ✅ M4-T6: Coupling integration tests
- ✅ **Milestone 5 (Serialization)**: Complete
  - ✅ M5-T1: Subgraph extraction with relevance scoring
  - ✅ M5-T2: Markdown serializer
  - ✅ M5-T3: JSON serializer
  - ✅ M5-T4: Mermaid serializer
  - ✅ M5-T5: Token counting (tiktoken-rs cl100k_base, ±5% accuracy)
  - ✅ M5-T6: Token-budgeted output (BudgetedSerializer with relevance-based detail levels)
  - ✅ M5-T7: forge map command (all formats)
  - ✅ M5-T8: Serializer tests (74+ tests covering all serializers and token budgeting)

### Next Up
- Milestone 6: LLM-assisted business context interview
- Milestone 7: Polish (incremental survey, CLI UX, documentation)

