# Forge Configuration Reference

Complete reference for the `forge.yaml` configuration file schema.

## Overview

Forge uses a YAML configuration file to specify:
- Where to find repositories (GitHub org, explicit repos, or local paths)
- GitHub authentication and cloning settings
- Language detection and exclusion rules
- Output paths for graphs and cached repositories
- LLM provider for business context interviews
- Token budget and staleness settings
- Environment definitions for deployment context

## Configuration File Location

By default, Forge looks for `forge.yaml` in the current directory. Override with:

```bash
forge --config /path/to/forge.yaml survey
```

Generate a default configuration with:

```bash
forge init
```

---

## Full Schema

```yaml
# Repository sources (at least one required)
repos:
  github_org: <string>           # GitHub organization name
  github_repos: [<string>]       # List of "owner/repo" strings
  local_paths: [<path>]          # List of local filesystem paths
  exclude: [<glob>]              # Patterns to exclude repos

# GitHub settings
github:
  token_env: <string>            # Environment variable for token (default: GITHUB_TOKEN)
  api_url: <url>                 # GitHub Enterprise API URL (optional)
  clone_method: <https|ssh>      # Clone method (default: https)
  clone_concurrency: <int>       # Concurrent clones (default: 4)

# Language settings
languages:
  exclude: [<string>]            # Languages to exclude from parsing

# Output paths
output:
  graph_path: <path>             # Knowledge graph output (default: .forge/graph.json)
  cache_path: <path>             # Repository cache (default: ~/.forge/repos)

# LLM configuration
llm:
  provider: <claude|gemini|codex>  # LLM CLI provider (default: claude)
  cli_path: <path>               # Custom CLI path (optional)

# Token budget for map output
token_budget: <int>              # Default: 8000

# Staleness threshold
staleness_days: <int>            # Default: 7

# Environment definitions (optional)
environments:
  - name: <string>               # Environment name (required)
    aws_account_id: <string>     # AWS account ID (optional)
    repos: [<glob>]              # Repo patterns for this environment
    local_only: <bool>           # Mark as local-only (optional)
```

---

## Section Reference

### `repos` (required)

Defines where Forge should discover repositories. At least one source must be configured.

#### `github_org`

| Property | Value |
|----------|-------|
| Type | `string` |
| Required | No (if `github_repos` or `local_paths` set) |
| Default | `null` |

GitHub organization name. Forge will discover all non-archived repositories in this organization.

```yaml
repos:
  github_org: "my-company"
```

**Notes:**
- Requires `GITHUB_TOKEN` environment variable to be set
- Discovers both public and private repos (based on token permissions)
- Archived repositories are automatically excluded

#### `github_repos`

| Property | Value |
|----------|-------|
| Type | `array[string]` |
| Required | No |
| Default | `[]` |
| Format | `"owner/repo"` |

Explicit list of GitHub repositories. Use this to survey specific repos instead of an entire organization.

```yaml
repos:
  github_repos:
    - "my-company/api-gateway"
    - "my-company/user-service"
    - "other-org/shared-lib"
```

**Validation:**
- Each entry must contain exactly one `/` separator
- Format must be `owner/repo`

#### `local_paths`

| Property | Value |
|----------|-------|
| Type | `array[path]` |
| Required | No |
| Default | `[]` |

Local filesystem paths to repositories. Supports tilde expansion (`~`).

```yaml
repos:
  local_paths:
    - "~/projects/internal-tools"
    - "/absolute/path/to/repo"
    - "./relative/path"
```

**Notes:**
- Paths are expanded at load time (`~` → home directory)
- Relative paths are resolved from the config file's directory
- Useful for air-gapped environments or monorepos

#### `exclude`

| Property | Value |
|----------|-------|
| Type | `array[glob]` |
| Required | No |
| Default | `[]` |

Glob patterns to exclude repositories by name. Applies to all sources.

```yaml
repos:
  exclude:
    - "*-deprecated"
    - "fork-*"
    - "*.archive"
    - "test-*"
```

**Supported patterns:**
- `*` - matches any characters except `/`
- `?` - matches any single character
- `[abc]` - matches any character in brackets
- `[!abc]` - matches any character not in brackets

---

### `github`

GitHub-specific settings for authentication and cloning.

#### `token_env`

| Property | Value |
|----------|-------|
| Type | `string` |
| Required | No |
| Default | `"GITHUB_TOKEN"` |

Name of the environment variable containing your GitHub personal access token.

```yaml
github:
  token_env: "MY_GITHUB_TOKEN"
```

**Required token scopes:**
- `repo` - for private repositories
- `public_repo` - for public repositories only

#### `api_url`

| Property | Value |
|----------|-------|
| Type | `url` |
| Required | No |
| Default | `null` (uses github.com API) |

GitHub Enterprise Server API URL. Only set this if using GitHub Enterprise.

```yaml
github:
  api_url: "https://github.mycompany.com/api/v3"
```

#### `clone_method`

| Property | Value |
|----------|-------|
| Type | `enum` |
| Required | No |
| Default | `"https"` |
| Values | `"https"`, `"ssh"` |

Git clone method to use.

```yaml
github:
  clone_method: "ssh"
```

**`https`** (default):
- Uses `https://github.com/owner/repo.git`
- Requires token for private repos
- Works through HTTP proxies

**`ssh`**:
- Uses `git@github.com:owner/repo.git`
- Requires SSH key setup
- Better for environments with SSH agents

#### `clone_concurrency`

| Property | Value |
|----------|-------|
| Type | `integer` |
| Required | No |
| Default | `4` |
| Range | `1` - `32` |

Number of repositories to clone concurrently.

```yaml
github:
  clone_concurrency: 8
```

**Notes:**
- Higher values speed up initial surveys
- May hit GitHub rate limits with very high values
- Consider network bandwidth and disk I/O

---

### `languages`

Language detection and parsing configuration. Languages are auto-detected from file extensions and configuration files.

#### `exclude`

| Property | Value |
|----------|-------|
| Type | `array[string]` |
| Required | No |
| Default | `[]` |

Languages to exclude from parsing. Case-insensitive matching.

```yaml
languages:
  exclude:
    - "terraform"
    - "python"
```

**Supported languages:**
- `javascript` - `.js`, `.jsx`, `.mjs`, `.cjs` files
- `typescript` - `.ts`, `.tsx` files
- `python` - `.py` files
- `terraform` - `.tf` files
- `cloudformation` - CloudFormation/SAM templates

**Use cases for exclusion:**
- Skip Terraform if infrastructure is managed separately
- Skip Python tests in a primarily JavaScript project
- Focus on specific parts of a polyglot codebase

---

### `output`

Output paths for the knowledge graph and cached repositories.

#### `graph_path`

| Property | Value |
|----------|-------|
| Type | `path` |
| Required | No |
| Default | `".forge/graph.json"` |

Path where the knowledge graph is saved. Supports tilde expansion.

```yaml
output:
  graph_path: ".forge/graph.json"
```

**Notes:**
- Parent directories are created automatically
- Typically committed to version control
- Used as input for `forge map`

#### `cache_path`

| Property | Value |
|----------|-------|
| Type | `path` |
| Required | No |
| Default | `"~/.forge/repos"` |

Path where cloned repositories are cached. Supports tilde expansion.

```yaml
output:
  cache_path: "~/.forge/repos"
```

**Notes:**
- Shared across all Forge projects
- Speeds up subsequent surveys
- Can be deleted to force fresh clones
- Typically not committed to version control

---

### `llm`

LLM provider configuration for business context interviews.

#### `provider`

| Property | Value |
|----------|-------|
| Type | `enum` |
| Required | No |
| Default | `"claude"` |
| Values | `"claude"`, `"gemini"`, `"codex"` |

Which LLM CLI to use for business context interviews.

```yaml
llm:
  provider: "claude"
```

**Supported providers:**

| Provider | CLI Command | Description |
|----------|-------------|-------------|
| `claude` | `claude` | Claude Code CLI |
| `gemini` | `gemini` | Google Gemini CLI |
| `codex` | `codex` | OpenAI Codex CLI |

**Notes:**
- Forge shells out to these CLIs (no direct API calls)
- Uses your existing CLI authentication
- No API keys stored in forge.yaml

#### `cli_path`

| Property | Value |
|----------|-------|
| Type | `path` |
| Required | No |
| Default | `null` (uses PATH) |

Custom path to the LLM CLI executable.

```yaml
llm:
  cli_path: "/usr/local/bin/claude"
```

**Use cases:**
- CLI installed in non-standard location
- Multiple versions of CLI installed
- Testing with wrapper scripts

---

### `token_budget`

| Property | Value |
|----------|-------|
| Type | `integer` |
| Required | No |
| Default | `8000` |

Default token budget for `forge map` output. Controls how much content is included in the serialized output.

```yaml
token_budget: 16000
```

**Notes:**
- Override with `forge map --budget <N>`
- Token counting uses tiktoken (cl100k_base encoding)
- Affects all output formats (markdown, json, mermaid)
- Higher budgets include more detail and nodes

**Recommended values:**
- 4000 - Minimal context, critical services only
- 8000 - Standard context (default)
- 16000 - Extended context, more detail
- 32000+ - Full context for large models

---

### `staleness_days`

| Property | Value |
|----------|-------|
| Type | `integer` |
| Required | No |
| Default | `7` |

Number of days after which a node is considered stale.

```yaml
staleness_days: 14
```

**Notes:**
- Stale nodes are marked in serialized output
- Based on `updated_at` timestamp of nodes
- Updated when nodes are re-discovered during survey
- Visual indicators in markdown and mermaid output

---

### `environments`

Environment definitions for mapping repositories to deployment contexts. This enables environment-specific filtering and helps LLM coding agents understand deployment targets.

Each environment is an object with:

#### `name` (required)

| Property | Value |
|----------|-------|
| Type | `string` |
| Required | Yes |

Environment identifier. Used for filtering with `forge map --env <name>`.

#### `aws_account_id`

| Property | Value |
|----------|-------|
| Type | `string` |
| Required | No |

AWS account ID for this environment. Injected into node attributes during survey.

#### `repos`

| Property | Value |
|----------|-------|
| Type | `array[glob]` |
| Required | No |
| Default | `[]` |

Glob patterns matching repository names that belong to this environment.

#### `local_only`

| Property | Value |
|----------|-------|
| Type | `boolean` |
| Required | No |
| Default | `false` |

Mark this environment as local-only (not deployed to AWS).

**Example:**

```yaml
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

  - name: development
    repos:
      - "my-company/*-dev"
    local_only: true
```

**Resolution rules:**
- First matching environment wins
- Patterns are glob-matched against full repo name (`owner/repo`)
- Repos not matching any pattern have no environment set

---

## Environment Variable Overrides

Configuration values can be overridden using environment variables:

| Variable | Overrides | Example |
|----------|-----------|---------|
| `GITHUB_TOKEN` | (GitHub authentication) | `ghp_xxxx...` |
| `FORGE_REPOS_GITHUB_ORG` | `repos.github_org` | `my-company` |
| `FORGE_OUTPUT_GRAPH_PATH` | `output.graph_path` | `.forge/graph.json` |
| `FORGE_OUTPUT_CACHE_PATH` | `output.cache_path` | `~/.forge/repos` |
| `FORGE_TOKEN_BUDGET` | `token_budget` | `16000` |
| `FORGE_STALENESS_DAYS` | `staleness_days` | `14` |
| `FORGE_LLM_PROVIDER` | `llm.provider` | `gemini` |

**Precedence:** Environment variables > Configuration file > Defaults

---

## Complete Example

```yaml
# forge.yaml - Complete example with all options

# ============================================
# Repository Sources
# ============================================
repos:
  # GitHub organization (discovers all repos)
  github_org: "my-company"

  # Or explicit list of repos
  github_repos:
    - "my-company/api-gateway"
    - "my-company/user-service"
    - "my-company/order-service"
    - "partner-org/shared-lib"

  # Local paths (for monorepos or air-gapped environments)
  local_paths:
    - "~/projects/internal-tools"

  # Exclude patterns
  exclude:
    - "*-deprecated"
    - "fork-*"
    - "*.archive"
    - "test-*"

# ============================================
# GitHub Configuration
# ============================================
github:
  # Environment variable for token (default: GITHUB_TOKEN)
  token_env: "GITHUB_TOKEN"

  # GitHub Enterprise API URL (uncomment for GHE)
  # api_url: "https://github.mycompany.com/api/v3"

  # Clone method: https or ssh
  clone_method: "https"

  # Concurrent clone operations
  clone_concurrency: 4

# ============================================
# Language Detection
# ============================================
# Languages are auto-detected. Only configure to exclude.
languages:
  exclude:
    - "terraform"  # Skip if infra is managed separately

# ============================================
# Output Paths
# ============================================
output:
  # Knowledge graph output
  graph_path: ".forge/graph.json"

  # Repository cache (shared across projects)
  cache_path: "~/.forge/repos"

# ============================================
# LLM Configuration
# ============================================
llm:
  # Provider: claude, gemini, or codex
  provider: "claude"

  # Custom CLI path (uncomment if needed)
  # cli_path: "/usr/local/bin/claude"

# ============================================
# Token Budget
# ============================================
# Default token budget for map output
token_budget: 8000

# ============================================
# Staleness Threshold
# ============================================
# Days before nodes are marked stale
staleness_days: 7

# ============================================
# Environments
# ============================================
environments:
  - name: production
    aws_account_id: "123456789012"
    repos:
      - "my-company/api-*"
      - "my-company/user-service"
      - "my-company/order-service"

  - name: staging
    aws_account_id: "987654321098"
    repos:
      - "my-company/*-staging"
      - "my-company/*-stage"

  - name: development
    repos:
      - "my-company/*-dev"
      - "my-company/*-local"
    local_only: true
```

---

## Minimal Examples

### GitHub Organization Only

```yaml
repos:
  github_org: "my-company"
```

### Local Paths Only

```yaml
repos:
  local_paths:
    - "./services/api"
    - "./services/web"
    - "./infra"
```

### Explicit Repos Only

```yaml
repos:
  github_repos:
    - "my-company/api-gateway"
    - "my-company/user-service"
```

### Mixed Sources

```yaml
repos:
  github_org: "my-company"
  local_paths:
    - "~/projects/wip-service"
  exclude:
    - "*-deprecated"
```

---

## Validation Rules

Forge validates the configuration file on load:

| Rule | Error Message |
|------|---------------|
| At least one repo source | "No repository sources configured. Set github_org, github_repos, or local_paths" |
| Valid repo format | "Invalid repo format 'X'. Expected 'owner/repo'" |
| Single slash in repos | "Invalid repo format 'X'. Expected 'owner/repo' with exactly one '/'" |
| Valid LLM provider | "Invalid LLM provider 'X'. Expected one of: claude, gemini, codex" |

---

## See Also

- [CLI Reference](cli-reference.md) - Command-line options
- [Extending Parsers](extending-parsers.md) - Adding new language support
- [Extending LLM Providers](extending-llm-providers.md) - Adding new LLM CLI adapters
