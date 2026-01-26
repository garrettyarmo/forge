# Forge

**Ecosystem intelligence platform for AI-assisted development**

Forge builds a knowledge graph of your software ecosystem—services, APIs, databases, and their relationships—so AI coding agents can understand your architecture before modifying it.

## Features

- **Survey** - Automatically discover services and dependencies from source code
- **Map** - Visualize and serialize your ecosystem for humans and LLMs
- **Interview** - Capture business context through LLM-assisted interviews
- **Incremental** - Fast re-surveys that only process changed files

## Quick Start

```bash
# Build from source
cargo build --release

# Initialize configuration
./target/release/forge init --org my-github-org

# Set your GitHub token
export GITHUB_TOKEN=ghp_xxxx

# Run survey
./target/release/forge survey

# View your ecosystem
./target/release/forge map --format markdown

# Generate a diagram
./target/release/forge map --format mermaid > architecture.mmd
```

## Installation

### From Source

```bash
git clone https://github.com/your-org/forge
cd forge
cargo build --release

# Binary will be at ./target/release/forge
```

### Prerequisites

- Rust 1.85+ (2024 edition)
- Git
- GitHub personal access token (for GitHub repos)

## How It Works

### 1. Survey Phase (deterministic, no LLM)

The survey phase uses tree-sitter AST parsing only—no LLM calls. This ensures:
- **Reproducibility**: Same input code always produces the same graph
- **Speed**: No API latency or rate limits
- **Offline capability**: Works without network for local repos
- **Predictable costs**: Zero token usage during survey

During survey, Forge:
- Clones repos from GitHub or local paths
- Parses JavaScript/TypeScript, Python, and Terraform using tree-sitter
- Builds a knowledge graph of services, databases, queues
- Detects implicit coupling through shared resources (e.g., multiple services accessing the same DynamoDB table)

### 2. Map Phase

Serialize the knowledge graph for consumption:
- **Markdown**: Human-readable documentation optimized for LLM context
- **JSON**: Structured format with schema, nodes, edges, and LLM instructions
- **Mermaid**: Visual diagrams with flowchart syntax

Features:
- Token budgeting for LLM context windows
- Subgraph extraction based on service queries
- Environment filtering (production, staging, etc.)

### 3. Interview Phase (optional, uses LLM)

Augment technical discovery with business context:
- Identifies gaps in business context
- Generates targeted questions
- Persists annotations across re-surveys
- Uses your existing LLM CLI (Claude Code, Gemini, etc.)

## Commands

### `forge init`

Initialize a new `forge.yaml` configuration file.

```bash
forge init [OPTIONS]

Options:
  --org <ORG>       Pre-fill GitHub organization
  -o, --output      Output path (default: forge.yaml)
  -f, --force       Overwrite existing file
```

### `forge survey`

Survey repositories and build the knowledge graph.

```bash
forge survey [OPTIONS]

Options:
  -c, --config <PATH>         Config file (default: forge.yaml)
  -o, --output <PATH>         Override output graph path
  --repos <REPOS>             Override repos (comma-separated)
  --exclude-lang <LANGS>      Exclude languages (comma-separated)
  --business-context          Launch business context interview
  --incremental               Only re-parse changed files
```

### `forge map`

Serialize the knowledge graph to various formats.

```bash
forge map [OPTIONS]

Options:
  -c, --config <PATH>         Config file (default: forge.yaml)
  -i, --input <PATH>          Input graph path
  -f, --format <FORMAT>       Output format: markdown|json|mermaid
  -s, --service <SERVICES>    Filter to specific services
  -e, --env <ENV>             Filter to specific environment
  -b, --budget <TOKENS>       Token budget limit
  -o, --output <PATH>         Output file (default: stdout)
```

### Global Flags

These flags apply to all commands:

```bash
-v, --verbose    Increase verbosity (-v, -vv, -vvv)
-q, --quiet      Suppress all output except errors
```

## Configuration

Forge uses a `forge.yaml` configuration file. Run `forge init` to generate one with helpful comments.

### Example Configuration

```yaml
# Repository sources
repos:
  # GitHub organization (discovers all repos)
  github_org: "my-company"

  # Or explicit repos
  github_repos:
    - "my-company/api-gateway"
    - "my-company/user-service"

  # Or local paths
  local_paths:
    - "~/projects/internal-tools"

  # Exclude patterns
  exclude:
    - "*-deprecated"

# GitHub settings
github:
  token_env: "GITHUB_TOKEN"
  clone_method: "https"
  clone_concurrency: 4

# Languages are auto-detected, but you can exclude some
languages:
  exclude:
    - terraform

# Output paths
output:
  graph_path: ".forge/graph.json"
  cache_path: "~/.forge/repos"

# LLM for business context interviews
llm:
  provider: "claude"

# Token budget for map output
token_budget: 8000

# Days before nodes are considered stale
staleness_days: 7

# Environment definitions (optional)
environments:
  - name: production
    aws_account_id: "123456789012"
    repos:
      - "my-company/api-*"
      - "my-company/user-service"

  - name: staging
    aws_account_id: "987654321098"
    repos:
      - "my-company/*-staging"
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `GITHUB_TOKEN` | GitHub personal access token (required for GitHub repos) |
| `FORGE_TOKEN_BUDGET` | Override default token budget |
| `FORGE_STALENESS_DAYS` | Override staleness threshold |
| `FORGE_OUTPUT_GRAPH_PATH` | Override graph output path |
| `FORGE_LLM_PROVIDER` | Override LLM provider |

## Examples

### Survey a GitHub Organization

```bash
# Initialize with your org
forge init --org my-company

# Set token and survey
export GITHUB_TOKEN=ghp_xxxx
forge survey
```

### Survey Local Repositories

```yaml
# forge.yaml
repos:
  local_paths:
    - "./services/user-api"
    - "./services/order-api"
    - "./infra/terraform"
```

```bash
forge survey
```

### Generate Architecture Documentation

```bash
# Markdown for documentation
forge map --format markdown > ARCHITECTURE.md

# JSON for programmatic access
forge map --format json > graph.json

# Mermaid diagram
forge map --format mermaid > diagram.mmd
```

### Focus on Specific Services

```bash
# Extract subgraph for a specific service
forge map --service "user-api" --format markdown

# Multiple services
forge map --service "user-api,order-api" --format json
```

### Filter by Environment

```bash
# Production services only
forge map --env production --format markdown

# Staging with token budget
forge map --env staging --budget 4000 --format json
```

### Incremental Survey

After the initial survey, use incremental mode for fast updates:

```bash
# First survey (full)
forge survey

# Subsequent surveys (fast - only changed files)
forge survey --incremental
```

### Business Context Interview

Capture business knowledge about your services:

```bash
# Survey with interview
forge survey --business-context

# Interview uses your configured LLM CLI (claude, gemini, codex)
# Annotations persist across re-surveys
```

## Language Support

Forge automatically detects and parses:

| Language | Extensions | Detection |
|----------|------------|-----------|
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | `package.json` |
| TypeScript | `.ts`, `.tsx` | `package.json`, `tsconfig.json` |
| Python | `.py` | `requirements.txt`, `pyproject.toml`, `setup.py` |
| Terraform | `.tf` | `*.tf` files |
| CloudFormation/SAM | `.yaml`, `.yml` | `AWSTemplateFormatVersion` |

Detected patterns:
- AWS SDK usage (DynamoDB, S3, SQS, SNS, Lambda)
- HTTP client calls (axios, fetch, requests)
- Framework detection (Express, FastAPI, Flask, Django)
- Infrastructure as Code resources

## Output Formats

### Markdown

Human-readable documentation with:
- Service inventory with attributes
- Database and queue listings
- Relationship tables
- Implicit coupling warnings
- Business context annotations

### JSON

Structured format including:
- Schema version and generation timestamp
- Nodes with types, attributes, and business context
- Edges with types and metadata
- LLM instructions (code style, testing, deployment)
- Summary statistics

### Mermaid

Flowchart diagrams with:
- Nodes grouped by type (Services, Databases, Queues)
- Shape coding (rectangles, cylinders, hexagons)
- Edge types (solid for explicit, dotted for implicit)
- Subgraph organization

## Architecture

Forge is organized as a Rust workspace:

```
forge/
├── forge-cli/      # CLI entry point and commands
├── forge-graph/    # Knowledge graph data structures
├── forge-survey/   # Code analysis and discovery
└── forge-llm/      # LLM CLI adapter layer
```

## Development

### Build

```bash
cargo build --workspace
```

### Test

```bash
cargo test --workspace
```

### Lint

```bash
cargo clippy --workspace -- -D warnings
```

### Format

```bash
cargo fmt
```

## License

MIT OR Apache-2.0
