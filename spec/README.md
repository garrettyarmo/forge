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

### Next Up
- Milestone 3: Python and Terraform parsers, language auto-detection
- Milestone 4: Implicit coupling detection
- Milestone 5: Multiple output formats (Markdown, JSON, Mermaid) with token budgeting
- Milestone 6: LLM-assisted business context interview
- Milestone 7: Polish (incremental survey, CLI UX, documentation)

