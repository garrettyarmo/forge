# Milestone 7: Polish Specification

> **Spec Version**: 1.0
> **Status**: Draft
> **Implements**: IMPLEMENTATION_PLAN.md § Milestone 7
> **Depends On**: [M6 Business Context](./m6-business-context.md)

---

## 1. Overview

### 1.1 Purpose

Polish Forge for production release with incremental survey, improved CLI UX, comprehensive documentation, and end-to-end testing. This milestone makes Forge ready for real-world use.

### 1.2 Success Criteria

1. Re-running survey is significantly faster when few files changed (>10x for unchanged repos)
2. Documentation enables a new user to get value within 30 minutes
3. All commands have clear error messages with actionable suggestions
4. No panics on malformed input
5. CI passes with comprehensive test coverage

---

## 2. Incremental Survey

### 2.1 Design

```
                              Full Survey
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                       First Survey Run                           │
│  1. Clone all repos                                              │
│  2. Parse all files                                              │
│  3. Store file hashes + commit SHAs                              │
│  4. Save graph + state to .forge/                                │
└─────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
                          Subsequent Runs
                      (forge survey --incremental)
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Incremental Survey                          │
│  1. Pull latest for each repo                                    │
│  2. Compare commit SHA to cached                                 │
│  3. If different: git diff to find changed files                 │
│  4. Only re-parse changed files                                  │
│  5. Merge new discoveries with existing graph                    │
│  6. Preserve business context annotations                        │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Survey State

```rust
// forge-survey/src/incremental.rs

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

/// Persistent state for incremental surveys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyState {
    /// Version of the state format
    pub version: u32,

    /// When the last full survey was run
    pub last_full_survey: DateTime<Utc>,

    /// State for each repository
    pub repos: HashMap<String, RepoState>,
}

/// State for a single repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoState {
    /// Git commit SHA at last survey
    pub commit_sha: String,

    /// When this repo was last surveyed
    pub last_surveyed: DateTime<Utc>,

    /// Hash of each file at last survey
    pub file_hashes: HashMap<PathBuf, String>,

    /// Discoveries from this repo (for merge)
    pub discovery_count: usize,
}

impl SurveyState {
    pub fn new() -> Self {
        Self {
            version: 1,
            last_full_survey: Utc::now(),
            repos: HashMap::new(),
        }
    }

    /// Load state from file
    pub fn load(path: &Path) -> Result<Self, StateError> {
        let content = std::fs::read_to_string(path)?;
        let state: Self = serde_json::from_str(&content)?;
        Ok(state)
    }

    /// Save state to file
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get state for a repo
    pub fn get_repo(&self, repo_name: &str) -> Option<&RepoState> {
        self.repos.get(repo_name)
    }

    /// Update state for a repo
    pub fn update_repo(&mut self, repo_name: String, state: RepoState) {
        self.repos.insert(repo_name, state);
    }

    /// Mark a repo as surveyed with current commit
    pub fn mark_surveyed(&mut self, repo_name: &str, commit_sha: &str, file_hashes: HashMap<PathBuf, String>) {
        self.repos.insert(repo_name.to_string(), RepoState {
            commit_sha: commit_sha.to_string(),
            last_surveyed: Utc::now(),
            file_hashes,
            discovery_count: 0,
        });
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
```

### 2.3 Change Detection

```rust
// forge-survey/src/incremental.rs (continued)

use tokio::process::Command;

/// Detect which files changed since last survey
pub struct ChangeDetector {
    state: SurveyState,
}

/// Result of change detection
#[derive(Debug)]
pub struct ChangeResult {
    /// Files that were added
    pub added: Vec<PathBuf>,

    /// Files that were modified
    pub modified: Vec<PathBuf>,

    /// Files that were deleted
    pub deleted: Vec<PathBuf>,

    /// Whether a full re-survey is needed
    pub needs_full_survey: bool,
}

impl ChangeDetector {
    pub fn new(state: SurveyState) -> Self {
        Self { state }
    }

    /// Detect changes in a repository
    pub async fn detect_changes(
        &self,
        repo_name: &str,
        repo_path: &Path,
    ) -> Result<ChangeResult, ChangeError> {
        let repo_state = match self.state.get_repo(repo_name) {
            Some(s) => s,
            None => {
                // No previous state - needs full survey
                return Ok(ChangeResult {
                    added: vec![],
                    modified: vec![],
                    deleted: vec![],
                    needs_full_survey: true,
                });
            }
        };

        // Get current commit SHA
        let current_sha = get_current_commit(repo_path).await?;

        if current_sha == repo_state.commit_sha {
            // No changes
            return Ok(ChangeResult {
                added: vec![],
                modified: vec![],
                deleted: vec![],
                needs_full_survey: false,
            });
        }

        // Get changed files via git diff
        let diff_output = Command::new("git")
            .args([
                "diff",
                "--name-status",
                &repo_state.commit_sha,
                &current_sha,
            ])
            .current_dir(repo_path)
            .output()
            .await?;

        if !diff_output.status.success() {
            // Git diff failed - might be a force push or rebase
            // Fall back to full survey
            return Ok(ChangeResult {
                added: vec![],
                modified: vec![],
                deleted: vec![],
                needs_full_survey: true,
            });
        }

        let diff_str = String::from_utf8_lossy(&diff_output.stdout);
        let mut result = ChangeResult {
            added: vec![],
            modified: vec![],
            deleted: vec![],
            needs_full_survey: false,
        };

        for line in diff_str.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let status = parts[0];
            let file_path = PathBuf::from(parts[1]);

            // Only track files we care about
            if !is_parseable_file(&file_path) {
                continue;
            }

            match status {
                "A" => result.added.push(file_path),
                "M" => result.modified.push(file_path),
                "D" => result.deleted.push(file_path),
                _ => {
                    // Renames, copies - treat as add+delete
                    if parts.len() >= 3 {
                        result.deleted.push(PathBuf::from(parts[1]));
                        result.added.push(PathBuf::from(parts[2]));
                    }
                }
            }
        }

        Ok(result)
    }
}

async fn get_current_commit(repo_path: &Path) -> Result<String, ChangeError> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(ChangeError::GitError("Failed to get HEAD commit".into()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_parseable_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(ext, "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "py" | "tf")
}

#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Git error: {0}")]
    GitError(String),
}
```

### 2.4 Incremental Survey Pipeline

```rust
// forge-survey/src/incremental.rs (continued)

use forge_graph::ForgeGraph;

/// Run an incremental survey
pub async fn run_incremental_survey(
    config: &ForgeConfig,
    state_path: &Path,
    graph_path: &Path,
    registry: &ParserRegistry,
) -> Result<ForgeGraph, SurveyError> {
    // Load existing state and graph
    let mut state = if state_path.exists() {
        SurveyState::load(state_path)?
    } else {
        SurveyState::new()
    };

    let mut graph = if graph_path.exists() {
        ForgeGraph::load_from_file(graph_path)?
    } else {
        ForgeGraph::new()
    };

    let detector = ChangeDetector::new(state.clone());
    let cache = RepoCache::new(config.output.cache_path.clone(), CloneMethod::Https);

    let repos = collect_repos(config).await?;
    let mut changes_found = false;

    for repo in repos {
        // Ensure repo is up to date
        let local_path = cache.ensure_repo(&repo, None).await?;

        // Detect changes
        let changes = detector.detect_changes(&repo.full_name, &local_path).await?;

        if changes.needs_full_survey {
            println!("Full survey needed for {}", repo.full_name);
            // Run full survey for this repo
            survey_repo(&local_path, &mut GraphBuilder::from_graph(graph.clone()), registry, &[])?;
            changes_found = true;
        } else if !changes.added.is_empty() || !changes.modified.is_empty() || !changes.deleted.is_empty() {
            println!(
                "Changes in {}: {} added, {} modified, {} deleted",
                repo.full_name,
                changes.added.len(),
                changes.modified.len(),
                changes.deleted.len()
            );

            // Incremental update
            incremental_update(&mut graph, &local_path, &changes, registry)?;
            changes_found = true;
        } else {
            println!("No changes in {}", repo.full_name);
        }

        // Update state
        let current_sha = get_current_commit(&local_path).await?;
        state.mark_surveyed(&repo.full_name, &current_sha, HashMap::new());
    }

    // Save state
    state.save(state_path)?;

    if changes_found {
        // Re-run coupling analysis
        let mut analyzer = CouplingAnalyzer::new(&graph);
        let coupling_result = analyzer.analyze();
        coupling_result.apply_to_graph(&mut graph)?;
    }

    Ok(graph)
}

fn incremental_update(
    graph: &mut ForgeGraph,
    repo_path: &Path,
    changes: &ChangeResult,
    registry: &ParserRegistry,
) -> Result<(), SurveyError> {
    // For deleted files, we might need to remove nodes
    // (but we keep business context - nodes aren't fully deleted)

    // For added/modified files, re-parse them
    let files_to_parse: Vec<_> = changes.added.iter()
        .chain(changes.modified.iter())
        .map(|p| repo_path.join(p))
        .collect();

    // Get applicable parsers based on file extensions
    let detected = detect_languages(repo_path);
    let parsers = registry.get_for_languages(
        &detected.languages.iter().cloned().collect::<Vec<_>>(),
        &[],
    );

    let mut builder = GraphBuilder::from_graph(graph.clone());

    for file_path in files_to_parse {
        for (_, parser) in &parsers {
            let extensions = parser.supported_extensions();
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

            if extensions.contains(&ext) {
                if let Ok(content) = std::fs::read_to_string(&file_path) {
                    if let Ok(discoveries) = parser.parse_file(&file_path, &content) {
                        // Get or create service for this repo
                        let service_id = detect_or_get_service_id(graph, repo_path)?;
                        builder.process_discoveries(discoveries, &service_id);
                    }
                }
            }
        }
    }

    *graph = builder.build();
    Ok(())
}
```

---

## 3. Staleness Indicators

### 3.1 Staleness Tracking

```rust
// forge-graph/src/node.rs (extended)

impl NodeMetadata {
    /// Check if this node is stale
    pub fn is_stale(&self, staleness_days: u32) -> bool {
        let threshold = chrono::Duration::days(staleness_days as i64);
        let age = chrono::Utc::now() - self.updated_at;
        age > threshold
    }

    /// Get staleness as a human-readable string
    pub fn staleness_description(&self) -> String {
        let age = chrono::Utc::now() - self.updated_at;

        if age.num_days() == 0 {
            "Updated today".to_string()
        } else if age.num_days() == 1 {
            "Updated yesterday".to_string()
        } else if age.num_days() < 7 {
            format!("Updated {} days ago", age.num_days())
        } else if age.num_weeks() < 4 {
            format!("Updated {} weeks ago", age.num_weeks())
        } else {
            format!("Updated {} months ago", age.num_days() / 30)
        }
    }
}
```

### 3.2 Staleness in Output

```rust
// forge-cli/src/commands/map.rs (extended)

/// Show stale nodes
pub fn run_map_stale(graph: &ForgeGraph, staleness_days: u32) {
    println!("Stale nodes (not updated in {} days):\n", staleness_days);

    let mut stale_nodes: Vec<_> = graph.nodes()
        .filter(|n| n.metadata.is_stale(staleness_days))
        .collect();

    stale_nodes.sort_by(|a, b| a.metadata.updated_at.cmp(&b.metadata.updated_at));

    if stale_nodes.is_empty() {
        println!("No stale nodes found.");
        return;
    }

    println!("| Node | Type | Last Updated |");
    println!("|------|------|--------------|");

    for node in stale_nodes {
        println!(
            "| {} | {:?} | {} |",
            node.display_name,
            node.node_type,
            node.metadata.staleness_description()
        );
    }
}
```

---

## 4. CLI UX Improvements

### 4.1 Progress Bars

```rust
// forge-cli/src/progress.rs

use indicatif::{ProgressBar, ProgressStyle, MultiProgress};

/// Progress tracking for survey operations
pub struct SurveyProgress {
    multi: MultiProgress,
    main_bar: ProgressBar,
    current_repo_bar: Option<ProgressBar>,
}

impl SurveyProgress {
    pub fn new(total_repos: u64) -> Self {
        let multi = MultiProgress::new();

        let main_bar = multi.add(ProgressBar::new(total_repos));
        main_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} repos ({msg})")
                .unwrap()
                .progress_chars("█▓▒░")
        );

        Self {
            multi,
            main_bar,
            current_repo_bar: None,
        }
    }

    pub fn start_repo(&mut self, repo_name: &str) {
        self.main_bar.set_message(repo_name.to_string());

        // Create sub-progress for current repo
        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.green} {msg}")
                .unwrap()
        );
        bar.set_message(format!("Processing {}...", repo_name));

        self.current_repo_bar = Some(bar);
    }

    pub fn finish_repo(&mut self) {
        if let Some(bar) = self.current_repo_bar.take() {
            bar.finish_and_clear();
        }
        self.main_bar.inc(1);
    }

    pub fn set_repo_status(&self, status: &str) {
        if let Some(ref bar) = self.current_repo_bar {
            bar.set_message(status.to_string());
        }
    }

    pub fn finish(&self) {
        self.main_bar.finish_with_message("Survey complete!");
    }
}
```

### 4.2 Colored Output

```rust
// forge-cli/src/output.rs

use console::{style, Emoji};

pub static SUCCESS: Emoji<'_, '_> = Emoji("✅ ", "OK ");
pub static WARNING: Emoji<'_, '_> = Emoji("⚠️ ", "!! ");
pub static ERROR: Emoji<'_, '_> = Emoji("❌ ", "ERR ");
pub static INFO: Emoji<'_, '_> = Emoji("ℹ️ ", "i ");

/// Print a success message
pub fn success(msg: &str) {
    println!("{} {}", SUCCESS, style(msg).green());
}

/// Print a warning message
pub fn warning(msg: &str) {
    eprintln!("{} {}", WARNING, style(msg).yellow());
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("{} {}", ERROR, style(msg).red().bold());
}

/// Print an info message
pub fn info(msg: &str) {
    println!("{} {}", INFO, style(msg).cyan());
}

/// Print a heading
pub fn heading(msg: &str) {
    println!("\n{}\n{}", style(msg).bold(), "=".repeat(msg.len()));
}
```

### 4.3 Error Messages with Suggestions

```rust
// forge-cli/src/errors.rs

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("Configuration file not found at {path}")]
    ConfigNotFound { path: String },

    #[error("GitHub token not found in environment")]
    GitHubTokenMissing,

    #[error("Failed to clone repository {repo}: {reason}")]
    CloneFailed { repo: String, reason: String },

    #[error("Parser failed for {file}: {reason}")]
    ParseError { file: String, reason: String },

    #[error("Graph file not found: {path}")]
    GraphNotFound { path: String },
}

impl ForgeError {
    /// Get a suggestion for how to fix this error
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            ForgeError::ConfigNotFound { .. } => Some(
                "Run 'forge init' to create a configuration file, or use --config to specify a path"
            ),
            ForgeError::GitHubTokenMissing => Some(
                "Set the GITHUB_TOKEN environment variable with a personal access token:\n  export GITHUB_TOKEN=ghp_xxxx"
            ),
            ForgeError::CloneFailed { .. } => Some(
                "Check your network connection and GitHub token permissions.\nFor private repos, ensure your token has 'repo' scope."
            ),
            ForgeError::ParseError { .. } => Some(
                "This file may have syntax errors. The survey will continue with other files."
            ),
            ForgeError::GraphNotFound { .. } => Some(
                "Run 'forge survey' first to build the knowledge graph."
            ),
        }
    }

    /// Format error with suggestion for CLI output
    pub fn format_for_cli(&self) -> String {
        let mut output = format!("Error: {}", self);

        if let Some(suggestion) = self.suggestion() {
            output.push_str(&format!("\n\nSuggestion: {}", suggestion));
        }

        output
    }
}
```

### 4.4 Verbose/Quiet Modes

```rust
// forge-cli/src/main.rs

use clap::Parser;

#[derive(Parser)]
#[command(name = "forge", version, about = "Ecosystem intelligence platform")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,
}

fn main() {
    let cli = Cli::parse();

    // Configure logging based on verbosity
    let log_level = if cli.quiet {
        tracing::Level::ERROR
    } else {
        match cli.verbose {
            0 => tracing::Level::INFO,
            1 => tracing::Level::DEBUG,
            _ => tracing::Level::TRACE,
        }
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .init();

    // Run command
    if let Err(e) = run_command(cli.command) {
        output::error(&e.format_for_cli());
        std::process::exit(1);
    }
}
```

---

## 5. Documentation

### 5.1 README.md Structure

```markdown
# Forge

> Ecosystem intelligence platform for AI-assisted development

Forge builds a knowledge graph of your software ecosystem—services, APIs, databases, and their relationships—so AI agents can understand your architecture before modifying it.

## Features

- **Survey** - Automatically discover services and dependencies from source code
- **Map** - Visualize and serialize your ecosystem for humans and LLMs
- **Interview** - Capture business context through LLM-assisted interviews
- **Incremental** - Fast re-surveys that only process changed files

## Quick Start

```bash
# Install
cargo install forge

# Initialize configuration
forge init --org my-github-org

# Set your GitHub token
export GITHUB_TOKEN=ghp_xxxx

# Run survey
forge survey

# View your ecosystem
forge map --format markdown

# Generate a diagram
forge map --format mermaid > architecture.mmd
```

## Installation

### From Source

```bash
git clone https://github.com/your-org/forge
cd forge
cargo install --path forge-cli
```

### Pre-built Binaries

Download from [Releases](https://github.com/your-org/forge/releases)

## Documentation

- [CLI Reference](docs/cli-reference.md)
- [Configuration](docs/configuration.md)
- [Extending Parsers](docs/extending-parsers.md)
- [Extending LLM Providers](docs/extending-llm-providers.md)

## How It Works

1. **Survey Phase** (deterministic, no LLM)
   - Clones repos from GitHub or local paths
   - Parses JavaScript, Python, and Terraform using tree-sitter
   - Builds a knowledge graph of services, databases, queues, etc.
   - Detects implicit coupling through shared resources

2. **Map Phase**
   - Serializes the graph to Markdown, JSON, or Mermaid
   - Applies token budgeting for LLM context windows
   - Extracts relevant subgraphs based on queries

3. **Interview Phase** (optional, uses LLM)
   - Identifies gaps in business context
   - Generates targeted questions
   - Persists annotations across re-surveys
```

### 5.2 docs/cli-reference.md

```markdown
# CLI Reference

## Global Options

| Option | Description |
|--------|-------------|
| `-v, --verbose` | Increase verbosity (use multiple times: -vvv) |
| `-q, --quiet` | Suppress all output except errors |
| `--help` | Show help information |
| `--version` | Show version |

## Commands

### forge init

Initialize a new configuration file.

```bash
forge init [OPTIONS]

Options:
  --org <ORG>       Pre-fill GitHub organization
  --output <PATH>   Output path (default: ./forge.yaml)
  --force           Overwrite existing file
```

### forge survey

Survey repositories and build the knowledge graph.

```bash
forge survey [OPTIONS]

Options:
  --config <PATH>           Configuration file (default: ./forge.yaml)
  --output <PATH>           Override output graph path
  --repos <REPOS>           Override repos (comma-separated)
  --exclude-lang <LANGS>    Exclude languages (comma-separated)
  --business-context        Launch business context interview
  --incremental             Only re-parse changed files
  --verbose                 Show detailed progress
```

### forge map

Serialize the knowledge graph to various formats.

```bash
forge map [OPTIONS]

Options:
  --config <PATH>           Configuration file
  --input <PATH>            Input graph path
  --format <FORMAT>         Output format: markdown|json|mermaid
  --service <SERVICES>      Filter to specific services
  --budget <TOKENS>         Token budget limit
  --output <PATH>           Output file (default: stdout)
  --stale                   Show stale nodes
```

## Examples

```bash
# Survey a GitHub organization
forge survey

# Survey specific repos
forge survey --repos owner/repo1,owner/repo2

# Survey with business context interview
forge survey --business-context

# Incremental survey (fast)
forge survey --incremental

# Output ecosystem as Markdown
forge map --format markdown > ARCHITECTURE.md

# Output specific service as JSON
forge map --service user-api --format json

# Generate Mermaid diagram
forge map --format mermaid > diagram.mmd

# Budget-constrained output for LLM context
forge map --budget 4000 --service user-api
```
```

### 5.3 docs/configuration.md

Document the complete forge.yaml schema (already detailed in M2 spec).

### 5.4 docs/extending-parsers.md

Document the parser extension process (already detailed in M3 spec).

---

## 6. Example Configurations

```yaml
# examples/minimal.yaml
repos:
  github_org: "my-org"

# examples/local-only.yaml
repos:
  local_paths:
    - "./services/user-api"
    - "./services/order-api"
    - "./infra/terraform"

# examples/full-featured.yaml
repos:
  github_repos:
    - "my-org/user-service"
    - "my-org/order-service"
    - "my-org/payment-service"
  exclude:
    - "*-deprecated"

github:
  token_env: "GITHUB_TOKEN"
  clone_concurrency: 8

output:
  graph_path: ".forge/graph.json"
  cache_path: "~/.forge/repos"

llm:
  provider: "claude"

token_budget: 8000

survey:
  incremental: true
  staleness_days: 7
```

---

## 7. End-to-End Tests

```rust
// tests/e2e/full_workflow.rs

use tempfile::tempdir;
use std::process::Command;

#[test]
fn test_full_survey_workflow() {
    let dir = tempdir().unwrap();

    // Create test repos
    create_test_repo(dir.path().join("repo-a"), "javascript");
    create_test_repo(dir.path().join("repo-b"), "python");

    // Create config
    let config = format!(r#"
repos:
  local_paths:
    - "{}/repo-a"
    - "{}/repo-b"
output:
  graph_path: "{}/graph.json"
"#, dir.path().display(), dir.path().display(), dir.path().display());

    std::fs::write(dir.path().join("forge.yaml"), config).unwrap();

    // Run survey
    let output = Command::new("cargo")
        .args(["run", "--", "survey", "--config", dir.path().join("forge.yaml").to_str().unwrap()])
        .output()
        .expect("Failed to run forge");

    assert!(output.status.success(), "Survey failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify graph was created
    assert!(dir.path().join("graph.json").exists());

    // Run map
    let output = Command::new("cargo")
        .args([
            "run", "--", "map",
            "--config", dir.path().join("forge.yaml").to_str().unwrap(),
            "--format", "markdown"
        ])
        .output()
        .expect("Failed to run forge map");

    assert!(output.status.success());
    let markdown = String::from_utf8_lossy(&output.stdout);
    assert!(markdown.contains("repo-a") || markdown.contains("repo-b"));
}

#[test]
fn test_incremental_survey() {
    let dir = tempdir().unwrap();

    // Create initial repo
    let repo_path = dir.path().join("repo");
    create_test_repo(&repo_path, "javascript");

    // Create config
    let config = format!(r#"
repos:
  local_paths:
    - "{}"
output:
  graph_path: "{}/graph.json"
"#, repo_path.display(), dir.path().display());

    std::fs::write(dir.path().join("forge.yaml"), config).unwrap();

    // First survey
    let start = std::time::Instant::now();
    run_survey(&dir.path().join("forge.yaml"), false);
    let full_duration = start.elapsed();

    // Incremental survey (no changes)
    let start = std::time::Instant::now();
    run_survey(&dir.path().join("forge.yaml"), true);
    let incr_duration = start.elapsed();

    // Incremental should be significantly faster
    assert!(incr_duration < full_duration / 2, "Incremental not faster than full");
}

fn create_test_repo(path: &Path, language: &str) {
    std::fs::create_dir_all(&path).unwrap();

    // Initialize git repo
    Command::new("git").args(["init"]).current_dir(&path).output().unwrap();

    match language {
        "javascript" => {
            std::fs::write(path.join("package.json"), r#"{"name": "test-service"}"#).unwrap();
            std::fs::write(path.join("index.js"), "const express = require('express');").unwrap();
        }
        "python" => {
            std::fs::write(path.join("requirements.txt"), "boto3==1.28.0").unwrap();
            std::fs::write(path.join("main.py"), "import boto3").unwrap();
        }
        _ => {}
    }

    // Commit
    Command::new("git").args(["add", "."]).current_dir(&path).output().unwrap();
    Command::new("git").args(["commit", "-m", "init"]).current_dir(&path).output().unwrap();
}

fn run_survey(config_path: &Path, incremental: bool) {
    let mut args = vec!["run", "--", "survey", "--config", config_path.to_str().unwrap()];
    if incremental {
        args.push("--incremental");
    }

    Command::new("cargo").args(&args).output().unwrap();
}
```

---

## 8. Implementation Checklist

| Task ID | Description | Files |
|---------|-------------|-------|
| M7-T1 | Implement incremental survey | `forge-survey/src/incremental.rs` |
| M7-T2 | Implement staleness indicators | `forge-graph/src/node.rs` |
| M7-T3 | Add progress bars | `forge-cli/src/progress.rs` |
| M7-T4 | Add colored output | `forge-cli/src/output.rs` |
| M7-T5 | Improve error messages | `forge-cli/src/errors.rs` |
| M7-T6 | Add `--verbose`/`--quiet` flags | `forge-cli/src/main.rs` |
| M7-T7 | Write README.md | `README.md` |
| M7-T8 | Write CLI reference | `docs/cli-reference.md` |
| M7-T9 | Write configuration reference | `docs/configuration.md` |
| M7-T10 | Write parser extension guide | `docs/extending-parsers.md` |
| M7-T11 | Write LLM provider extension guide | `docs/extending-llm-providers.md` |
| M7-T12 | Create example configs | `examples/` |
| M7-T13 | Write e2e tests | `tests/e2e/` |

---

## 9. Acceptance Criteria

- [ ] `forge survey --incremental` is >10x faster when no files changed
- [ ] `forge map --stale` shows nodes not updated in N days
- [ ] Progress bars display during survey
- [ ] Errors include actionable suggestions
- [ ] `-v` flag increases output verbosity
- [ ] `-q` flag suppresses non-error output
- [ ] README enables new user to get value in <30 minutes
- [ ] All example configs work correctly
- [ ] E2E tests pass
- [ ] No panics on malformed config/input
- [ ] CI passes with >80% coverage
