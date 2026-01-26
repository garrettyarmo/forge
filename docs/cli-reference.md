# Forge CLI Reference

Complete reference for all Forge command-line interface commands, options, and flags.

## Synopsis

```
forge [OPTIONS] <COMMAND>
```

## Global Options

These options can be used with any command:

| Option | Short | Description |
|--------|-------|-------------|
| `--verbose` | `-v` | Increase verbosity. Can be repeated for more detail (`-v`, `-vv`, `-vvv`) |
| `--quiet` | `-q` | Suppress all output except errors |
| `--help` | `-h` | Print help information |
| `--version` | `-V` | Print version information |

### Verbosity Levels

- No flag: Normal output (progress bars, summary)
- `-v`: Verbose (detailed progress messages)
- `-vv`: Very verbose (debug information)
- `-vvv`: Maximum verbosity (trace-level output)

Note: `--quiet` takes precedence over `--verbose` if both are specified.

---

## Commands

### `forge init`

Initialize a new `forge.yaml` configuration file with helpful comments explaining each section.

#### Usage

```bash
forge init [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--org` | | `<ORG>` | `my-org` | Pre-fill GitHub organization name in the generated config |
| `--output` | `-o` | `<PATH>` | `forge.yaml` | Output path for the configuration file |
| `--force` | `-f` | flag | false | Overwrite existing configuration file |

#### Examples

```bash
# Create forge.yaml in current directory
forge init

# Pre-fill organization name
forge init --org my-company

# Specify custom output path
forge init --output config/forge.yaml

# Overwrite existing file
forge init --force

# Combined options
forge init --org my-company -o config/forge.yaml -f
```

#### Output

On success, creates a `forge.yaml` file and prints next steps:
1. Edit the configuration file to configure repositories
2. Set the GitHub token environment variable
3. Run `forge survey`

#### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | File already exists (without `--force`) |
| 1 | Write error (permissions, disk full, etc.) |

---

### `forge survey`

Survey repositories and build a knowledge graph. This command:
1. Loads configuration from `forge.yaml`
2. Discovers repositories from GitHub org, explicit repos, or local paths
3. Clones or updates repositories to local cache
4. Automatically detects languages and selects appropriate parsers
5. Parses code using tree-sitter AST analysis (no LLM calls)
6. Builds a knowledge graph of services, APIs, databases, and queues
7. Detects implicit coupling through shared resources
8. Saves the graph to the configured output path

#### Usage

```bash
forge survey [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--config` | `-c` | `<PATH>` | `forge.yaml` | Path to configuration file |
| `--output` | `-o` | `<PATH>` | `.forge/graph.json` | Override output graph path |
| `--repos` | | `<REPOS>` | (from config) | Override repos (comma-separated `owner/repo` format) |
| `--exclude-lang` | | `<LANGS>` | (from config) | Exclude languages (comma-separated: `terraform,python`) |
| `--business-context` | | flag | false | Launch business context interview after survey |
| `--incremental` | | flag | false | Only re-parse changed files (uses git to detect changes) |

#### How It Works

##### Language Detection

Languages are automatically detected from:
- File extensions (`.js`, `.ts`, `.py`, `.tf`, etc.)
- Configuration files (`package.json`, `requirements.txt`, `pyproject.toml`)

Detected languages: JavaScript, TypeScript, Python, Terraform, CloudFormation/SAM

##### Deterministic Parsing

The survey phase uses **tree-sitter AST parsing only**—no LLM calls. This ensures:
- **Reproducibility**: Same input code always produces the same graph
- **Speed**: No API latency or rate limits
- **Offline capability**: Works without network for local repos
- **Predictable costs**: Zero token usage during survey

##### Incremental Mode

When using `--incremental`:
- Tracks commit SHAs per repository
- Detects added, modified, and deleted files using git
- Skips unchanged repositories entirely
- Loads existing graph and merges changes
- Saves survey state to `.forge/survey-state.json`

##### Business Context Interview

When using `--business-context`:
- Identifies gaps in business documentation (missing purpose, owner, etc.)
- Generates targeted questions based on graph structure
- Uses your configured LLM CLI (Claude, Gemini, Codex)
- Annotations persist across re-surveys

#### Examples

```bash
# Survey using default forge.yaml
forge survey

# Use custom configuration file
forge survey --config ./my-config.yaml

# Override output path
forge survey --output ./output/graph.json

# Survey specific repos (bypasses config)
forge survey --repos "owner/repo1,owner/repo2"

# Exclude specific languages
forge survey --exclude-lang "terraform,python"

# Fast incremental survey (only changed files)
forge survey --incremental

# Survey with business context interview
forge survey --business-context

# Combined: incremental with verbose output
forge -v survey --incremental

# Combined: custom config with excluded languages
forge survey --config prod.yaml --exclude-lang terraform
```

#### Output

Prints progress information:
- Number of repositories discovered
- Language detection results per repo
- Parser execution and discovery counts
- Coupling analysis results (implicit couplings found)
- Final graph statistics (node count, edge count)
- Output file path

If `--business-context` is specified and the LLM CLI is available, launches an interactive interview session.

#### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success (some repos may have failed) |
| 1 | Configuration error (file not found, invalid format) |
| 1 | No repositories to survey |
| 1 | GitHub token missing (for GitHub repos) |

Note: Individual repository failures do not cause the command to fail; errors are logged and the survey continues with remaining repos.

---

### `forge map`

Serialize the knowledge graph to various output formats. Supports filtering by service, environment, and token budget.

#### Usage

```bash
forge map [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--config` | `-c` | `<PATH>` | `forge.yaml` | Path to configuration file |
| `--input` | `-i` | `<PATH>` | `.forge/graph.json` | Input graph path |
| `--format` | `-f` | `<FORMAT>` | `markdown` | Output format: `markdown`, `json`, `mermaid` |
| `--service` | `-s` | `<SERVICES>` | (none) | Filter to specific services (comma-separated) |
| `--env` | `-e` | `<ENV>` | (none) | Filter to specific environment |
| `--budget` | `-b` | `<TOKENS>` | (from config) | Token budget limit |
| `--output` | `-o` | `<PATH>` | stdout | Output file path |

#### Output Formats

##### Markdown (`--format markdown` or `--format md`)

Human-readable documentation optimized for LLM context:
- Service inventory with attributes (language, framework, entry point)
- Database and queue listings
- Relationship tables showing dependencies
- Implicit coupling warnings with risk levels
- Business context annotations (purpose, owner, gotchas)
- Staleness indicators for outdated nodes

##### JSON (`--format json`)

Structured format for programmatic access:
- Schema version and generation timestamp
- Nodes with types, attributes, and business context
- Edges with types and metadata
- LLM instructions (code style, testing, deployment commands)
- Summary statistics (node counts by type, edge counts)
- Relevance scores for subgraph queries

##### Mermaid (`--format mermaid` or `--format mmd`)

Flowchart diagrams in Mermaid syntax:
- Nodes grouped by type (Services, Databases, Queues, CloudResources)
- Shape coding: rectangles (services), cylinders (databases), hexagons (resources)
- Edge types with labels (CALLS, READS, WRITES, etc.)
- Dotted lines for implicit coupling
- Subgraph organization

#### Service Filtering

When `--service` is specified, extracts a relevance-scored subgraph:
- Starts from specified seed services
- Includes related nodes (databases, queues, called services)
- Traverses up to 2 hops by default
- Includes implicit couplings
- Nodes scored by relevance (based on edge distance)

Service names can be:
- Display names: `"User API"`, `"Order Service"`
- Node ID names: `user-api`, `order-service`
- Case-insensitive matching

#### Environment Filtering

When `--env` is specified:
- Filters to nodes with matching `environment` attribute
- Case-insensitive matching
- Only includes edges between matching nodes
- Returns error if no nodes match

Environment attributes are set during survey when you configure environments in `forge.yaml`.

#### Examples

```bash
# Generate markdown documentation
forge map

# Output to file
forge map --output ARCHITECTURE.md

# JSON for programmatic access
forge map --format json --output graph.json

# Mermaid diagram
forge map --format mermaid --output diagram.mmd

# Filter to specific service
forge map --service "User API"

# Multiple services
forge map --service "User API,Order API" --format json

# Filter by environment
forge map --env production --format markdown

# Combine environment and token budget
forge map --env staging --budget 4000 --format json

# Use custom input path
forge map --input ./graphs/latest.json --format mermaid

# Verbose output showing what's being processed
forge -v map --service "User API"
```

#### Token Budgeting

The `--budget` option controls output size for LLM context windows:
- Nodes included in order of relevance score
- Detail level adjusted based on relevance:
  - High relevance (>0.7): Full details
  - Medium relevance (0.4-0.7): Summary
  - Low relevance (<0.4): Minimal
- Edges included only if both nodes are included

#### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Graph file not found |
| 1 | Invalid format specified |
| 1 | Service not found (when using `--service`) |
| 1 | No nodes found in environment (when using `--env`) |
| 1 | Write error |

---

## Environment Variables

Environment variables can override configuration file values:

| Variable | Description | Example |
|----------|-------------|---------|
| `GITHUB_TOKEN` | GitHub personal access token (required for GitHub repos) | `ghp_xxxx...` |
| `FORGE_REPOS_GITHUB_ORG` | Override GitHub organization | `my-company` |
| `FORGE_OUTPUT_GRAPH_PATH` | Override graph output path | `.forge/graph.json` |
| `FORGE_OUTPUT_CACHE_PATH` | Override repository cache path | `~/.forge/repos` |
| `FORGE_TOKEN_BUDGET` | Override default token budget | `16000` |
| `FORGE_STALENESS_DAYS` | Override staleness threshold | `14` |
| `FORGE_LLM_PROVIDER` | Override LLM provider | `gemini` |

### GitHub Token

The GitHub token is required for:
- Listing repositories in an organization
- Cloning private repositories
- Accessing private repository content

Token scopes needed:
- `repo` for private repositories
- `public_repo` for public repositories only

To set the token:
```bash
export GITHUB_TOKEN=ghp_your_token_here
```

Or configure a different environment variable name in `forge.yaml`:
```yaml
github:
  token_env: "MY_GITHUB_TOKEN"
```

---

## Common Workflows

### Initial Setup

```bash
# 1. Initialize configuration
forge init --org my-company

# 2. Set GitHub token
export GITHUB_TOKEN=ghp_xxxx

# 3. Run initial survey
forge survey

# 4. View results
forge map
```

### Daily Development

```bash
# Fast incremental survey
forge survey --incremental

# Check specific service
forge map --service "My Service" --format markdown
```

### Documentation Generation

```bash
# Generate architecture documentation
forge map --format markdown --output docs/ARCHITECTURE.md

# Generate visual diagram
forge map --format mermaid --output docs/architecture.mmd

# Generate machine-readable format
forge map --format json --output .forge/graph-export.json
```

### Environment-Specific Views

```bash
# Production architecture
forge map --env production --output docs/production.md

# Staging with limited context
forge map --env staging --budget 4000 --format json
```

### Debugging

```bash
# Verbose survey to see what's happening
forge -vv survey

# Check specific repos
forge -v survey --repos "owner/repo1,owner/repo2"

# See full incremental mode details
forge -vv survey --incremental
```

---

## See Also

- [Configuration Reference](configuration.md) - Full `forge.yaml` schema
- [Extending Parsers](extending-parsers.md) - Adding new language support
- [Extending LLM Providers](extending-llm-providers.md) - Adding new LLM CLI adapters
