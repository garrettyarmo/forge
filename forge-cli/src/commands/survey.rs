//! Implementation of the `forge survey` command.
//!
//! This command surveys repositories and builds a knowledge graph by:
//! 1. Loading configuration from forge.yaml
//! 2. Discovering repositories from GitHub org, explicit repos, or local paths
//! 3. Cloning/updating repositories to local cache
//! 4. Automatically detecting languages and selecting appropriate parsers
//! 5. Parsing code with language-specific parsers (JavaScript/TypeScript, Python, Terraform)
//! 6. Building a knowledge graph from discoveries
//! 7. Saving the graph to the configured output path
//!
//! # Usage
//!
//! ```bash
//! # Survey using forge.yaml configuration
//! forge survey
//!
//! # Override configuration file
//! forge survey --config ./custom-forge.yaml
//!
//! # Override output path
//! forge survey --output ./custom-graph.json
//!
//! # Override repos (bypasses forge.yaml repos config)
//! forge survey --repos "owner/repo1,owner/repo2"
//!
//! # Exclude specific languages
//! forge survey --exclude-lang "terraform,python"
//!
//! # Enable verbose output
//! forge survey --verbose
//! ```

use crate::config::{CloneMethod, ConfigError, ForgeConfig};
use forge_survey::{
    CloneMethod as SurveyCloneMethod, CouplingAnalyzer, GitHubClient, GraphBuilder, RepoCache,
    RepoInfo, detect_languages, parser::ParserRegistry,
};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during the survey process.
#[derive(Debug, Error)]
pub enum SurveyError {
    /// Configuration loading failed.
    #[error("Configuration error: {0}")]
    ConfigError(#[from] ConfigError),

    /// GitHub API error.
    #[error("GitHub error: {0}")]
    GitHubError(#[from] forge_survey::GitHubError),

    /// Parser error.
    #[error("Parser error: {0}")]
    ParserError(#[from] forge_survey::ParserError),

    /// Graph error.
    #[error("Graph error: {0}")]
    GraphError(#[from] forge_graph::GraphError),

    /// IO error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid repository format.
    #[error("Invalid repository format: {0}. Expected 'owner/repo'")]
    InvalidRepoFormat(String),

    /// No repositories configured.
    #[error(
        "No repositories to survey. Configure github_org, github_repos, or local_paths in forge.yaml"
    )]
    NoRepositories,

    /// GitHub token not available.
    #[error("GitHub token not found. Set the {0} environment variable")]
    NoGitHubToken(String),
}

/// Options for the `forge survey` command.
#[derive(Debug, Clone, Default)]
pub struct SurveyOptions {
    /// Path to configuration file.
    pub config: Option<String>,
    /// Override output graph path.
    pub output: Option<String>,
    /// Override repos (comma-separated "owner/repo" format).
    pub repos: Option<String>,
    /// Exclude languages (comma-separated).
    pub exclude_lang: Option<String>,
    /// Launch business context interview after survey (M6 feature).
    pub business_context: bool,
    /// Only re-parse changed files (M7 feature).
    pub incremental: bool,
    /// Show detailed progress.
    pub verbose: bool,
}

/// Run the `forge survey` command.
///
/// # Arguments
///
/// * `options` - The options for the survey command
///
/// # Returns
///
/// Returns `Ok(())` if the survey completed successfully, or an error if
/// any step failed.
pub async fn run_survey(options: SurveyOptions) -> Result<(), SurveyError> {
    if options.verbose {
        println!("Starting survey...");
    }

    // Load configuration
    let mut config = if let Some(config_path) = &options.config {
        if options.verbose {
            println!("Loading configuration from: {}", config_path);
        }
        ForgeConfig::load_from_path(Path::new(config_path))?
    } else {
        if options.verbose {
            println!("Loading configuration from: forge.yaml");
        }
        ForgeConfig::load_default()?
    };

    // Apply CLI overrides
    if let Some(output_path) = &options.output {
        config.output.graph_path = PathBuf::from(output_path);
    }

    if let Some(exclude_langs) = &options.exclude_lang {
        let langs: Vec<String> = exclude_langs
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        config.languages.exclude.extend(langs);
    }

    if options.verbose {
        println!("Output graph path: {}", config.output.graph_path.display());
        println!("Cache path: {}", config.output.cache_path.display());
    }

    // Warn about unimplemented features
    if options.business_context {
        println!("Warning: --business-context is not yet implemented (coming in M6)");
    }
    if options.incremental {
        println!("Warning: --incremental is not yet implemented (coming in M7)");
    }

    // Collect repositories to survey
    let repos = collect_repos(&config, &options).await?;

    if repos.is_empty() {
        return Err(SurveyError::NoRepositories);
    }

    println!("Found {} repositories to survey", repos.len());

    // Initialize graph builder
    let mut builder = GraphBuilder::new();

    // Create parser registry once
    let registry = ParserRegistry::new().map_err(SurveyError::ParserError)?;

    // Setup repository cache for GitHub repos
    let cache = RepoCache::new(
        config.output.cache_path.clone(),
        convert_clone_method(config.github.clone_method),
    );

    // Survey each repository
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, repo) in repos.iter().enumerate() {
        println!("[{}/{}] Surveying: {}", i + 1, repos.len(), repo.full_name);

        match survey_repository(repo, &cache, &mut builder, &config, &options, &registry).await {
            Ok(()) => {
                success_count += 1;
                if options.verbose {
                    println!("  ✓ Successfully surveyed {}", repo.full_name);
                }
            }
            Err(e) => {
                error_count += 1;
                println!("  ✗ Error surveying {}: {}", repo.full_name, e);
                // Continue with other repos - don't crash entire survey
            }
        }
    }

    println!();
    println!(
        "Survey complete: {} succeeded, {} failed",
        success_count, error_count
    );

    // Build graph
    let mut graph = builder.build();
    println!(
        "Built knowledge graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // Run coupling analysis (M4-T4)
    if options.verbose {
        println!("Running coupling analysis...");
    }
    let mut analyzer = CouplingAnalyzer::new(&graph);
    let coupling_result = analyzer.analyze();

    // Report coupling findings
    let coupling_count = coupling_result.implicit_couplings.len();
    let shared_read_count = coupling_result.shared_reads.len();
    let shared_write_count = coupling_result.shared_writes.len();

    if coupling_count > 0 || shared_read_count > 0 || shared_write_count > 0 {
        println!(
            "Coupling analysis: {} implicit couplings, {} shared reads, {} shared writes",
            coupling_count, shared_read_count, shared_write_count
        );

        // Report high-risk couplings
        for coupling in &coupling_result.implicit_couplings {
            if matches!(coupling.risk_level, forge_survey::CouplingRisk::High) {
                println!(
                    "  ⚠ High-risk coupling: {} ↔ {} ({})",
                    coupling.service_a.name(),
                    coupling.service_b.name(),
                    coupling.reason
                );
            } else if options.verbose {
                println!(
                    "  {:?}-risk coupling: {} ↔ {} ({})",
                    coupling.risk_level,
                    coupling.service_a.name(),
                    coupling.service_b.name(),
                    coupling.reason
                );
            }
        }
    } else if options.verbose {
        println!("Coupling analysis: no implicit couplings detected");
    }

    // Apply coupling edges to graph
    coupling_result.apply_to_graph(&mut graph)?;
    let edge_count_after = graph.edge_count();
    if edge_count_after > graph.node_count() {
        // Only report if edges were actually added
        if options.verbose {
            println!("Added coupling edges: {} total edges now", edge_count_after);
        }
    }

    // Create output directory if needed
    if let Some(parent) = config.output.graph_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if options.verbose {
                println!("Creating output directory: {}", parent.display());
            }
            std::fs::create_dir_all(parent)?;
        }
    }

    // Save graph
    graph.save_to_file(&config.output.graph_path)?;
    println!(
        "Saved knowledge graph to: {}",
        config.output.graph_path.display()
    );

    Ok(())
}

/// Collect repositories to survey based on configuration and CLI overrides.
async fn collect_repos(
    config: &ForgeConfig,
    options: &SurveyOptions,
) -> Result<Vec<RepoInfo>, SurveyError> {
    let mut repos = Vec::new();

    // If --repos flag is provided, it overrides all config sources
    if let Some(repos_arg) = &options.repos {
        if options.verbose {
            println!("Using repos from --repos flag");
        }
        for repo_str in repos_arg.split(',') {
            let repo_str = repo_str.trim();
            if repo_str.is_empty() {
                continue;
            }
            let repo = parse_repo_string(repo_str)?;
            repos.push(repo);
        }
        return Ok(repos);
    }

    // Collect from GitHub org
    if let Some(org) = &config.repos.github_org {
        if options.verbose {
            println!("Discovering repositories from GitHub org: {}", org);
        }

        // Need GitHub token for API access
        let token = config
            .github_token()
            .map_err(|_| SurveyError::NoGitHubToken(config.github.token_env.clone()))?;

        let client = GitHubClient::new(
            &token,
            config.github.api_url.as_deref(),
            convert_clone_method(config.github.clone_method),
        )?;

        let org_repos = client.list_org_repos(org).await?;
        if options.verbose {
            println!("  Found {} repositories in org", org_repos.len());
        }

        for repo in org_repos {
            if !config.is_excluded(&repo.name) {
                repos.push(repo);
            } else if options.verbose {
                println!("  Excluding {} (matches exclude pattern)", repo.name);
            }
        }
    }

    // Collect from explicit GitHub repos
    if !config.repos.github_repos.is_empty() {
        if options.verbose {
            println!(
                "Fetching {} explicit GitHub repositories",
                config.repos.github_repos.len()
            );
        }

        // Need GitHub token for API access
        let token = config
            .github_token()
            .map_err(|_| SurveyError::NoGitHubToken(config.github.token_env.clone()))?;

        let client = GitHubClient::new(
            &token,
            config.github.api_url.as_deref(),
            convert_clone_method(config.github.clone_method),
        )?;

        for repo_str in &config.repos.github_repos {
            let (owner, name) = parse_owner_repo(repo_str)?;
            if !config.is_excluded(name) {
                match client.get_repo(owner, name).await {
                    Ok(repo) => repos.push(repo),
                    Err(e) => println!("  Warning: Failed to fetch {}: {}", repo_str, e),
                }
            } else if options.verbose {
                println!("  Excluding {} (matches exclude pattern)", name);
            }
        }
    }

    // Collect from local paths
    for local_path in &config.repos.local_paths {
        if options.verbose {
            println!("Adding local repository: {}", local_path.display());
        }

        // Create a RepoInfo for local paths
        let name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        repos.push(RepoInfo {
            full_name: local_path.to_string_lossy().to_string(),
            name: name.to_string(),
            owner: "local".to_string(),
            clone_url: String::new(), // Not needed for local paths
            default_branch: "main".to_string(),
            language: None,
            archived: false,
            fork: false,
            topics: vec![],
        });
    }

    Ok(repos)
}

/// Survey a single repository.
async fn survey_repository(
    repo: &RepoInfo,
    cache: &RepoCache,
    builder: &mut GraphBuilder,
    config: &ForgeConfig,
    options: &SurveyOptions,
    registry: &ParserRegistry,
) -> Result<(), SurveyError> {
    // Determine local path
    let local_path = if repo.owner == "local" {
        // Local repository - use the full_name as the path
        PathBuf::from(&repo.full_name)
    } else {
        // GitHub repository - clone/update to cache
        if options.verbose {
            println!("  Cloning/updating repository...");
        }
        let token = config.github_token().ok();
        cache.ensure_repo(repo, token.as_deref()).await?
    };

    if options.verbose {
        println!("  Local path: {}", local_path.display());
    }

    // Get commit SHA for tracking
    let commit_sha = if repo.owner != "local" {
        cache.get_commit_sha(&local_path).await
    } else {
        None
    };

    // Set repository context in builder
    builder.set_repo_context(&repo.full_name, commit_sha.as_deref());

    // Detect languages in the repository
    let detected = detect_languages(&local_path);

    if options.verbose {
        if detected.is_empty() {
            println!("  No supported languages detected");
        } else {
            let lang_names: Vec<String> = detected
                .iter()
                .map(|l| format!("{} ({:.0}%)", l.name, l.confidence * 100.0))
                .collect();
            println!("  Detected languages: {}", lang_names.join(", "));
        }
    }

    // Get parsers for detected languages, respecting exclusions
    let parsers = registry.get_for_languages(&detected, &config.languages.exclude);

    if parsers.is_empty() {
        if options.verbose {
            if !config.languages.exclude.is_empty() {
                println!(
                    "  No parsers available after exclusions (excluded: {})",
                    config.languages.exclude.join(", ")
                );
            } else {
                println!("  No parsers available for detected languages");
            }
        }
        return Ok(());
    }

    if options.verbose {
        println!("  Using {} parser(s)", parsers.len());
    }

    // Detect service metadata from applicable config files
    // Try JavaScript/TypeScript (package.json)
    let package_json_path = local_path.join("package.json");
    let mut service_id = None;
    if package_json_path.exists() {
        if let Some(js_parser) = registry.get("javascript") {
            // Use downcast to call parse_package_json on JavaScriptParser
            if let Some(js_parser) = js_parser
                .as_ref()
                .as_any()
                .downcast_ref::<forge_survey::parser::javascript::JavaScriptParser>(
            ) {
                if let Some(service) = js_parser.parse_package_json(&local_path) {
                    if options.verbose {
                        println!("  Found service: {} (from package.json)", service.name);
                    }
                    service_id = Some(builder.add_service(service));
                }
            }
        }
    }

    // Try Python (pyproject.toml, setup.py, requirements.txt)
    if service_id.is_none() {
        let python_configs = ["pyproject.toml", "setup.py", "requirements.txt"];
        let has_python_config = python_configs.iter().any(|f| local_path.join(f).exists());

        if has_python_config {
            if let Some(py_parser) = registry.get("python") {
                if let Some(py_parser) = py_parser
                    .as_ref()
                    .as_any()
                    .downcast_ref::<forge_survey::parser::python::PythonParser>(
                ) {
                    if let Some(service) = py_parser.parse_project_config(&local_path) {
                        if options.verbose {
                            println!("  Found service: {} (from Python config)", service.name);
                        }
                        service_id = Some(builder.add_service(service));
                    }
                }
            }
        }
    }

    // If no service was detected from config files, use repo name
    if service_id.is_none() {
        if options.verbose {
            println!("  No service metadata found - using repository name");
        }
        // Create a minimal service discovery from the repo name
        let service = forge_survey::ServiceDiscovery {
            name: repo.name.clone(),
            language: detected
                .iter()
                .next()
                .map(|l| l.name.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            framework: None,
            entry_point: "unknown".to_string(),
            source_file: repo.full_name.clone(),
            source_line: 0,
        };
        service_id = Some(builder.add_service(service));
    }

    let service_id = service_id.expect("service_id should be set at this point");

    // Run each parser and collect discoveries
    for parser in &parsers {
        if options.verbose {
            let extensions = parser.supported_extensions();
            println!("  Parsing {} files...", extensions.join("/"));
        }

        match parser.parse_repo(&local_path) {
            Ok(discoveries) => {
                if options.verbose {
                    println!("    Found {} code discoveries", discoveries.len());
                }
                builder.process_discoveries(discoveries, &service_id);
            }
            Err(e) => {
                // Log warning and continue with other parsers
                println!(
                    "    Warning: Parser failed for {}: {}",
                    parser.supported_extensions().join("/"),
                    e
                );
            }
        }
    }

    Ok(())
}

/// Parse a "owner/repo" string into owner and repo parts.
fn parse_owner_repo(repo_str: &str) -> Result<(&str, &str), SurveyError> {
    let parts: Vec<&str> = repo_str.split('/').collect();
    if parts.len() != 2 {
        return Err(SurveyError::InvalidRepoFormat(repo_str.to_string()));
    }
    Ok((parts[0], parts[1]))
}

/// Parse a "owner/repo" string into a RepoInfo for the --repos flag.
fn parse_repo_string(repo_str: &str) -> Result<RepoInfo, SurveyError> {
    let (owner, name) = parse_owner_repo(repo_str)?;

    Ok(RepoInfo {
        full_name: repo_str.to_string(),
        name: name.to_string(),
        owner: owner.to_string(),
        clone_url: format!("https://github.com/{}", repo_str),
        default_branch: "main".to_string(),
        language: None,
        archived: false,
        fork: false,
        topics: vec![],
    })
}

/// Convert CLI CloneMethod to forge-survey CloneMethod.
fn convert_clone_method(method: CloneMethod) -> SurveyCloneMethod {
    match method {
        CloneMethod::Https => SurveyCloneMethod::Https,
        CloneMethod::Ssh => SurveyCloneMethod::Ssh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_owner_repo() {
        let (owner, repo) = parse_owner_repo("my-org/my-repo").unwrap();
        assert_eq!(owner, "my-org");
        assert_eq!(repo, "my-repo");

        let result = parse_owner_repo("invalid");
        assert!(result.is_err());

        let result = parse_owner_repo("too/many/slashes");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_repo_string() {
        let repo = parse_repo_string("owner/repo").unwrap();
        assert_eq!(repo.full_name, "owner/repo");
        assert_eq!(repo.owner, "owner");
        assert_eq!(repo.name, "repo");
        assert_eq!(repo.clone_url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_convert_clone_method() {
        assert_eq!(
            convert_clone_method(CloneMethod::Https),
            SurveyCloneMethod::Https
        );
        assert_eq!(
            convert_clone_method(CloneMethod::Ssh),
            SurveyCloneMethod::Ssh
        );
    }
}
