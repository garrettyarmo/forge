# Forge Configuration Examples

This directory contains example `forge.yaml` configurations for common use cases.

## Quick Start

1. Copy the example that best matches your needs:
   ```bash
   cp examples/minimal.yaml forge.yaml
   ```

2. Edit the configuration to match your repositories

3. Run Forge:
   ```bash
   export GITHUB_TOKEN=ghp_xxxx
   forge survey
   forge map
   ```

## Examples

| File | Use Case | GitHub API Required |
|------|----------|---------------------|
| `minimal.yaml` | Simplest setup - survey a GitHub org | Yes |
| `local-only.yaml` | Survey local repos without GitHub | No |
| `full-featured.yaml` | All configuration options demonstrated | Yes |
| `multi-org.yaml` | Multiple GitHub orgs and teams | Yes |
| `ci-cd.yaml` | CI/CD pipeline integration | Optional |

## Configuration Reference

See [docs/configuration.md](../docs/configuration.md) for the complete configuration reference.

## Environment Variables

All examples can be customized via environment variables:

| Variable | Description | Example |
|----------|-------------|---------|
| `GITHUB_TOKEN` | GitHub Personal Access Token | `ghp_xxxxxxxxxxxx` |
| `FORGE_REPOS_GITHUB_ORG` | Override GitHub org | `my-other-org` |
| `FORGE_OUTPUT_GRAPH_PATH` | Override graph output path | `/tmp/graph.json` |
| `FORGE_TOKEN_BUDGET` | Override token budget | `16000` |
| `FORGE_STALENESS_DAYS` | Override staleness threshold | `14` |
| `FORGE_LLM_PROVIDER` | Override LLM provider | `gemini` |

## Commands

```bash
# Initialize from scratch
forge init --org my-org

# Survey with specific config
forge survey --config examples/full-featured.yaml

# Survey with business context interview
forge survey --config examples/full-featured.yaml --business-context

# Incremental survey (only changed files)
forge survey --config examples/full-featured.yaml --incremental

# Output as markdown
forge map --format markdown

# Output as JSON for LLM consumption
forge map --format json --budget 8000

# Generate architecture diagram
forge map --format mermaid > architecture.mmd

# Filter to specific services
forge map --service user-service,order-service

# Filter by environment
forge map --env production
```
