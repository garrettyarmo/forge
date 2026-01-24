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

### Incremental Survey (forge-survey/src/incremental.rs)
- **SurveyState**: Persistent state tracking repos, commit SHAs, discovery counts
- **RepoState**: Per-repo state (commit SHA, last surveyed, languages, success flag)
- **ChangeDetector**: Git-based change detection using `git diff`
- **ChangeResult**: Reports added/modified/deleted files
- State saved to `.forge/survey-state.json`
- Skips unchanged repos (same commit SHA)
- Falls back to full survey on force push or shallow clone issues
- Status: Complete (M7-T1)

## Observability Suite

_To be implemented in future milestones_

## LLM Integration (forge-llm)

Forge integrates with LLMs by **shelling out to coding agent CLIs** rather than making direct API calls. This approach leverages user's existing CLI authentication and avoids storing API keys in forge.yaml.

### LLM Provider Trait
- `LLMProvider` async trait with `name()`, `is_available()`, `prompt()`, and `prompt_with_history()` methods
- `LLMError` enum for comprehensive error handling (ProcessFailed, NonZeroExit, InvalidOutput, CliNotFound, Timeout, NotConfigured)
- `Message` struct with `Role` enum for conversation history support
- Status: Complete (M6-T1)

### CLI Adapters
- **ClaudeAdapter**: Claude Code CLI adapter (`claude`)
  - Uses `--print` flag for non-interactive mode
  - `[System: ...]\n\n{user}` prompt format
  - Status: Complete (M6-T2)
- **GeminiAdapter**: Google Gemini CLI adapter (`gemini`)
  - Simpler `{system}\n\n{user}` prompt format
  - `User: ... / Model: ...` history format
  - Status: Complete (M6-T3)
- **CodexAdapter**: OpenAI Codex CLI adapter (`codex`)
  - `System: ... User: ...` prompt format
  - `User: ... / Assistant: ...` history format
  - Status: Complete (M6-T4)

### Provider Factory
- `create_provider(config)`: Instantiate provider from configuration
- `create_and_verify_provider(config)`: Create and verify CLI availability
- Supports: claude, gemini, codex
- Status: Complete (M6-T5)

### Business Context Interview
- **Gap Analysis** (M6-T6): Identifies nodes lacking business context
  - Analyzes services for: missing purpose/owner, high centrality, implicit couplings, complexity without gotchas
  - Analyzes databases/queues for: shared access without clear ownership
  - Scoring system with configurable weights and thresholds
  - Status: Complete
- **Question Generation** (M6-T7): Creates contextual questions based on gaps
  - Purpose, owner, centrality, coupling, shared resource, and gotcha questions
  - Priority-sorted (1-10 scale)
  - Context-aware questions include dependency information
  - Status: Complete
- **Interview Flow** (M6-T8): Interactive terminal-based interview
  - InterviewSession manages state and question progression
  - Commands: [s]uggest (LLM), [k]skip, [q]uit, or type answer directly
  - LLM suggestions when provider available
  - Status: Complete
- **Annotation Persistence** (M6-T9): Preserves annotations across re-surveys
  - BusinessContext::merge() in forge-graph for controlled merging
  - Existing values preserved, gotchas deduplicated, notes merged
  - merge_business_context() for graph-level annotation transfer
  - Status: Complete
- **--business-context Flag** (M6-T10): Triggers interview after survey
  - Graceful degradation when LLM CLI unavailable
  - Graph saved before and after interview (interrupt-safe)
  - Status: Complete

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

- ✅ **Milestone 6 (Business Context)**: Complete
  - ✅ M6-T1: LLM provider trait
  - ✅ M6-T2: Claude CLI adapter
  - ✅ M6-T3: Gemini CLI adapter
  - ✅ M6-T4: Codex CLI adapter
  - ✅ M6-T5: Provider factory
  - ✅ M6-T6: Gap analysis (context gap scoring with configurable thresholds)
  - ✅ M6-T7: Question generation (priority-sorted, context-aware questions)
  - ✅ M6-T8: Interview flow (InterviewSession with LLM suggestions)
  - ✅ M6-T9: Annotation persistence (BusinessContext::merge, cross-graph merging)
  - ✅ M6-T10: --business-context flag (graceful degradation, interrupt-safe)
  - ✅ M6-T11: Interview tests (15 tests for session and persistence)

### In Progress
- ⏳ **Milestone 7 (Polish)**: Incremental survey, CLI UX, documentation
  - ✅ M7-T1: Incremental survey (SurveyState, ChangeDetector, git-based change detection)
  - ⏳ M7-T2: Staleness indicators
  - ⏳ M7-T3: Progress bars (indicatif)
  - ⏳ M7-T4: --verbose/--quiet flags
  - ⏳ M7-T5: README.md
  - ⏳ M7-T6: CLI reference documentation
  - ⏳ M7-T7: Configuration reference
  - ⏳ M7-T8: Parser extension guide
  - ⏳ M7-T9: LLM provider extension guide
  - ⏳ M7-T10: Example configurations
  - ⏳ M7-T11: Final integration testing

