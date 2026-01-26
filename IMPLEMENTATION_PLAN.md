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

### LLM Agent Success Criteria (M8)

Forge is successful for LLM coding agents when:

1. LLM can query "What does service X do?" and get purpose, constraints, and gotchas
2. LLM can query "How do I deploy service X?" and get deployment commands
3. LLM can query "What are the dependencies of service X?" and get full dependency graph
4. LLM receives actionable DO/DON'T instructions from business context
5. JSON output includes deployment_method, environment, aws_account_id attributes
6. JSON output includes llm_instructions with code_style, testing, and deployment guidance
7. Token-budgeted output fits within LLM context windows

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
│       │   ├── terraform.rs
│       │   └── cloudformation.rs
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

## V1 Implementation Status

**All milestones complete. 652+ tests passing.**

### Milestone Summary

| Milestone | Status | Key Features |
|-----------|--------|--------------|
| **M1: Foundation** | ✅ Complete | Knowledge graph with petgraph, 10 edge types, JSON persistence |
| **M2: Survey Core** | ✅ Complete | JavaScript/TypeScript parser, GitHub integration, forge init/survey commands |
| **M3: Multi-Language** | ✅ Complete | Python parser, Terraform parser, auto-detection, ParserRegistry |
| **M4: Implicit Coupling** | ✅ Complete | Shared resource detection, ownership inference, IMPLICITLY_COUPLED edges |
| **M5: Serialization** | ✅ Complete | Markdown/JSON/Mermaid serializers, token budgeting, forge map command |
| **M6: Business Context** | ✅ Complete | LLM CLI adapters (Claude/Gemini/Codex), gap analysis, interview flow |
| **M7: Polish** | ✅ Complete | Incremental survey, staleness indicators, progress bars, documentation |
| **M8: LLM Optimization** | ✅ Complete | Deployment metadata extraction, SAM/CloudFormation parser, LLM instructions |

### Detailed Specifications

Each milestone has a detailed specification in the `spec/` directory:

- [spec/m1-foundation.md](spec/m1-foundation.md) - Knowledge graph foundation
- [spec/m2-survey-core.md](spec/m2-survey-core.md) - Configuration and JavaScript parser
- [spec/m3-multi-language.md](spec/m3-multi-language.md) - Python and Terraform parsers
- [spec/m4-implicit-coupling.md](spec/m4-implicit-coupling.md) - Coupling detection
- [spec/m5-serialization.md](spec/m5-serialization.md) - Output formats and token budgeting
- [spec/m6-business-context.md](spec/m6-business-context.md) - LLM-assisted interviews
- [spec/m7-polish.md](spec/m7-polish.md) - Production readiness
- [spec/m8-llm-optimization.md](spec/m8-llm-optimization.md) - LLM-optimized context

---

## Future Enhancements (Post-V1)

These items are documented in the specifications as future work:

### Language Support
- Go parser
- Java parser
- Rust parser
- CDK parser (TypeScript/Python AWS CDK)

### Infrastructure
- Runtime AWS state scanning (Cloud Mapper integration)
- Multi-cloud support (Azure ARM templates, GCP Deployment Manager)
- IAM policy analysis for permission inference

### LLM Features
- Pattern-based style inference (async/await detection, decorator patterns)
- Coverage requirement detection from CI configs
- Custom instruction templates in forge.yaml
- LLM-powered instruction refinement
- Framework version-specific guidance
- Security guidance generation

### Other
- Real-time context updates
- Direct IDE integration

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
| tree-sitter | 0.24 | Parsing framework |
| tree-sitter-javascript | 0.23 | JS/TS parsing |
| tree-sitter-python | 0.23 | Python parsing |
| hcl-rs | 0.18 | Terraform HCL parsing |
| octocrab | 0.44 | GitHub API client |
| regex | 1.10 | Pattern matching |
| walkdir | 2.5 | Directory traversal |
| tiktoken-rs | 0.6 | Token counting |
| indicatif | 0.17 | Progress bars |
| console | 0.15 | Terminal colors |
| thiserror | 1.0 | Error handling |
| uuid | 1.0 | Unique identifiers |
| async-trait | 0.1 | Async trait support |
| chrono | 0.4 | Date/time handling |

### Development Tools

- Rust 1.75+ (2024 edition)
- Git
- GitHub account (for testing API integration)

---

## Command Reference

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
  -v, --verbose             Show detailed progress
  -q, --quiet               Suppress non-error output

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
  --service <SERVICES>      Filter to specific services (comma-separated)
  --env <ENVIRONMENT>       Filter to specific environment
  --budget <TOKENS>         Token budget limit
  --output <PATH>           Output file (default: stdout)
```

---

## Configuration Schema

### Complete forge.yaml

```yaml
# Forge configuration file
# See docs/configuration.md for full reference

# Repository Sources
repos:
  # GitHub organization - discovers all repos automatically
  github_org: "my-company"

  # Or explicit list of repos (overrides github_org)
  github_repos:
    - "my-company/api-gateway"
    - "my-company/user-service"

  # Or local paths (for testing or air-gapped environments)
  local_paths:
    - "./repos/local-service"

  # Exclude patterns (applies to all sources)
  exclude:
    - "*-deprecated"

# GitHub Configuration
github:
  token_env: "GITHUB_TOKEN"

# Languages are AUTO-DETECTED from file extensions and config files
# Only configure if you need to exclude specific languages:
languages:
  exclude:
    - terraform  # Example: skip Terraform parsing

# Output Configuration
output:
  graph_path: ".forge/graph.json"
  cache_path: "~/.forge/repos"

# LLM Configuration (for business context)
llm:
  provider: "claude"  # claude | gemini | codex

# Token Budget
token_budget: 8000

# Staleness threshold in days
staleness_days: 7

# Environment definitions (for M8 LLM optimization)
environments:
  - name: production
    aws_account_id: "123456789012"
    repos:
      - "*-prod"
      - "my-company/core-*"
  - name: staging
    aws_account_id: "987654321098"
    repos:
      - "*-staging"
```

---

## Extension Guides

For detailed guides on extending Forge:

- [docs/extending-parsers.md](docs/extending-parsers.md) - Adding new language parsers
- [docs/extending-llm-providers.md](docs/extending-llm-providers.md) - Adding new LLM CLI adapters
- [docs/configuration.md](docs/configuration.md) - Full configuration reference
- [docs/cli-reference.md](docs/cli-reference.md) - Complete CLI documentation

---

## Testing

All tests can be run with:

```bash
cargo test --workspace
```

Current test coverage: **652+ tests** across all crates.

### Test Categories

| Category | Location | Count |
|----------|----------|-------|
| Unit tests | `*/src/*.rs` | ~500 |
| Integration tests (JS) | `forge-survey/tests/integration_js.rs` | 6 |
| Integration tests (Python) | `forge-survey/tests/integration_python.rs` | 6 |
| Integration tests (Multi) | `forge-survey/tests/integration_multi.rs` | 2 |
| Integration tests (Coupling) | `forge-survey/tests/integration_coupling.rs` | 7 |
| Integration tests (LLM) | `forge-cli/tests/integration_llm.rs` | 7 |
| E2E tests | `forge-cli/tests/e2e_full_workflow.rs` | 13 |
