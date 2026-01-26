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
//! # Enable verbose output (global flag)
//! forge -v survey
//!
//! # Suppress all output except errors (global flag)
//! forge -q survey
//! ```

use crate::config::{CloneMethod, ConfigError, ForgeConfig};
use crate::output;
use crate::progress::SurveyProgress;
use forge_graph::ForgeGraph;
use forge_llm::{LLMConfig, create_and_verify_provider, run_interactive_interview};
use forge_survey::{
    ChangeDetector, CloneMethod as SurveyCloneMethod, CouplingAnalyzer, GitHubClient, GraphBuilder,
    RepoCache, RepoInfo, SurveyState, detect_languages, get_current_commit, parser::ParserRegistry,
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

    /// Incremental survey state error.
    #[error("Survey state error: {0}")]
    StateError(#[from] forge_survey::StateError),

    /// Change detection error.
    #[error("Change detection error: {0}")]
    ChangeError(#[from] forge_survey::ChangeError),
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
    output::verbose("Starting survey...");

    // Load configuration
    let mut config = if let Some(config_path) = &options.config {
        output::verbose(&format!("Loading configuration from: {}", config_path));
        ForgeConfig::load_from_path(Path::new(config_path))?
    } else {
        output::verbose("Loading configuration from: forge.yaml");
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

    output::verbose(&format!(
        "Output graph path: {}",
        config.output.graph_path.display()
    ));
    output::verbose(&format!(
        "Cache path: {}",
        config.output.cache_path.display()
    ));

    // Collect repositories to survey
    let repos = collect_repos(&config, &options).await?;

    if repos.is_empty() {
        return Err(SurveyError::NoRepositories);
    }

    let mut progress = if !output::is_verbose() {
        Some(SurveyProgress::new(repos.len() as u64))
    } else {
        println!("Found {} repositories to survey", repos.len());
        None
    };

    // Calculate state path (same directory as graph)
    let state_path = config
        .output
        .graph_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("survey-state.json");

    // Load existing state for incremental mode
    let survey_state = if options.incremental && state_path.exists() {
        match SurveyState::load(&state_path) {
            Ok(state) => {
                if output::is_verbose() {
                    println!(
                        "Loaded survey state: {} repos surveyed previously",
                        state.repo_count()
                    );
                }
                Some(state)
            }
            Err(e) => {
                println!("Warning: Could not load survey state: {}", e);
                println!("Falling back to full survey.");
                None
            }
        }
    } else {
        None
    };

    // Initialize graph builder
    // For incremental mode, try to load existing graph
    let mut builder = if options.incremental && config.output.graph_path.exists() {
        match ForgeGraph::load_from_file(&config.output.graph_path) {
            Ok(graph) => {
                if output::is_verbose() {
                    println!(
                        "Loaded existing graph: {} nodes, {} edges",
                        graph.node_count(),
                        graph.edge_count()
                    );
                }
                GraphBuilder::from_graph(graph)
            }
            Err(e) => {
                println!("Warning: Could not load existing graph: {}", e);
                println!("Starting fresh survey.");
                GraphBuilder::new()
            }
        }
    } else {
        GraphBuilder::new()
    };

    // Create change detector for incremental mode
    let change_detector = survey_state
        .as_ref()
        .map(|state| ChangeDetector::new(state.clone()));

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
    let mut skipped_count = 0;
    let mut repos_surveyed: Vec<(String, String, usize, Vec<String>, bool)> = Vec::new();

    for (i, repo) in repos.iter().enumerate() {
        // Start repo in progress bar
        if let Some(ref mut p) = progress {
            p.start_repo(&repo.full_name);
        }

        // For incremental mode, check if we need to survey this repo
        if let Some(ref detector) = change_detector {
            // Get the local path first to check changes
            let local_path = if repo.owner == "local" {
                PathBuf::from(&repo.full_name)
            } else {
                cache.repo_path(repo)
            };

            // Only check changes if the repo exists locally
            if local_path.exists() {
                match detector.detect_changes(&repo.full_name, &local_path).await {
                    Ok(changes) if !changes.needs_full_survey && !changes.has_changes() => {
                        skipped_count += 1;
                        if output::is_verbose() {
                            println!(
                                "[{}/{}] Skipping {} (no changes)",
                                i + 1,
                                repos.len(),
                                repo.full_name
                            );
                        }
                        // Still record the state (same SHA, previous discovery count)
                        if let Some(prev_state) = detector.state().get_repo(&repo.full_name) {
                            repos_surveyed.push((
                                repo.full_name.clone(),
                                changes.current_sha,
                                prev_state.discovery_count,
                                prev_state.detected_languages.clone(),
                                true,
                            ));
                        }
                        continue;
                    }
                    Ok(changes) => {
                        if changes.needs_full_survey {
                            if output::is_verbose() {
                                println!(
                                    "[{}/{}] Full survey needed for {}: {}",
                                    i + 1,
                                    repos.len(),
                                    repo.full_name,
                                    changes
                                        .full_survey_reason
                                        .as_deref()
                                        .unwrap_or("unknown reason")
                                );
                            }
                        } else if output::is_verbose() {
                            println!(
                                "[{}/{}] Surveying {} ({} added, {} modified, {} deleted)",
                                i + 1,
                                repos.len(),
                                repo.full_name,
                                changes.added.len(),
                                changes.modified.len(),
                                changes.deleted.len()
                            );
                        }
                    }
                    Err(e) => {
                        if output::is_verbose() {
                            println!(
                                "  Warning: Could not detect changes for {}: {}",
                                repo.full_name, e
                            );
                        }
                        // Fall through to full survey
                    }
                }
            }
        }

        if !options.incremental || change_detector.is_none() {
            println!("[{}/{}] Surveying: {}", i + 1, repos.len(), repo.full_name);
        }

        match survey_repository(repo, &cache, &mut builder, &config, &options, &registry).await {
            Ok(survey_info) => {
                success_count += 1;
                if let Some(ref mut p) = progress {
                    p.finish_repo();
                } else {
                    output::success(&format!("Successfully surveyed {}", repo.full_name));
                }
                repos_surveyed.push(survey_info);
            }
            Err(e) => {
                error_count += 1;
                if let Some(ref mut p) = progress {
                    p.finish_repo();
                } else {
                    output::error(&format!("Error surveying {}: {}", repo.full_name, e));
                }
                // Record failed survey
                let local_path = if repo.owner == "local" {
                    PathBuf::from(&repo.full_name)
                } else {
                    cache.repo_path(repo)
                };
                if let Ok(sha) = get_current_commit(&local_path).await {
                    repos_surveyed.push((repo.full_name.clone(), sha, 0, vec![], false));
                }
                // Continue with other repos - don't crash entire survey
            }
        }
    }

    // Finish the progress bar
    if let Some(ref p) = progress {
        p.finish();
    }

    println!();
    if options.incremental && skipped_count > 0 {
        println!(
            "Survey complete: {} succeeded, {} skipped (no changes), {} failed",
            success_count, skipped_count, error_count
        );
    } else {
        println!(
            "Survey complete: {} succeeded, {} failed",
            success_count, error_count
        );
    }

    // Build graph
    let mut graph = builder.build();
    println!(
        "Built knowledge graph: {} nodes, {} edges",
        graph.node_count(),
        graph.edge_count()
    );

    // Run coupling analysis (M4-T4)
    if output::is_verbose() {
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
                output::warning(&format!(
                    "High-risk coupling: {} ↔ {} ({})",
                    coupling.service_a.name(),
                    coupling.service_b.name(),
                    coupling.reason
                ));
            } else if output::is_verbose() {
                println!(
                    "  {:?}-risk coupling: {} ↔ {} ({})",
                    coupling.risk_level,
                    coupling.service_a.name(),
                    coupling.service_b.name(),
                    coupling.reason
                );
            }
        }
    } else if output::is_verbose() {
        println!("Coupling analysis: no implicit couplings detected");
    }

    // Apply coupling edges to graph
    coupling_result.apply_to_graph(&mut graph)?;
    let edge_count_after = graph.edge_count();
    if edge_count_after > graph.node_count() {
        // Only report if edges were actually added
        if output::is_verbose() {
            println!("Added coupling edges: {} total edges now", edge_count_after);
        }
    }

    // Create output directory if needed
    if let Some(parent) = config.output.graph_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if output::is_verbose() {
                println!("Creating output directory: {}", parent.display());
            }
            std::fs::create_dir_all(parent)?;
        }
    }

    // Save graph (before interview so we don't lose survey progress)
    graph.save_to_file(&config.output.graph_path)?;
    println!(
        "Saved knowledge graph to: {}",
        config.output.graph_path.display()
    );

    // Save incremental survey state
    if options.incremental || survey_state.is_some() {
        let mut new_state = survey_state.unwrap_or_else(SurveyState::new);
        if !options.incremental {
            // First incremental-enabled survey - mark as full survey
            new_state.mark_full_survey_start();
        }

        // Update state with all surveyed repos
        for (repo_name, sha, discovery_count, languages, success) in repos_surveyed {
            new_state.mark_surveyed(&repo_name, &sha, discovery_count, languages, success);
        }

        new_state.save(&state_path)?;
        if output::is_verbose() {
            println!(
                "Saved survey state to: {} ({} repos tracked)",
                state_path.display(),
                new_state.repo_count()
            );
        }
    }

    // Run business context interview if requested (M6-T10)
    if options.business_context {
        let llm_config = LLMConfig {
            provider: config.llm.provider.clone(),
            cli_path: config
                .llm
                .cli_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        };

        match create_and_verify_provider(&llm_config).await {
            Ok(provider) => {
                println!();
                match run_interactive_interview(&mut graph, Some(provider)).await {
                    Ok(result) => {
                        if result.questions_answered > 0 {
                            // Save graph again with interview annotations
                            graph.save_to_file(&config.output.graph_path)?;
                            println!(
                                "Updated knowledge graph with {} annotations.",
                                result.questions_answered
                            );
                        }
                    }
                    Err(e) => {
                        println!("Interview error: {}", e);
                        println!("Survey results were saved before the interview.");
                    }
                }
            }
            Err(e) => {
                println!();
                println!(
                    "Warning: LLM provider '{}' not available: {}",
                    config.llm.provider, e
                );
                println!(
                    "Install the CLI or change llm.provider in forge.yaml to enable interviews."
                );
                println!(
                    "Survey results have been saved. Run with --business-context again after installing the CLI."
                );
            }
        }
    }

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
        if output::is_verbose() {
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
        if output::is_verbose() {
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
        if output::is_verbose() {
            println!("  Found {} repositories in org", org_repos.len());
        }

        for repo in org_repos {
            if !config.is_excluded(&repo.name) {
                repos.push(repo);
            } else if output::is_verbose() {
                println!("  Excluding {} (matches exclude pattern)", repo.name);
            }
        }
    }

    // Collect from explicit GitHub repos
    if !config.repos.github_repos.is_empty() {
        if output::is_verbose() {
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
            } else if output::is_verbose() {
                println!("  Excluding {} (matches exclude pattern)", name);
            }
        }
    }

    // Collect from local paths
    for local_path in &config.repos.local_paths {
        if output::is_verbose() {
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

/// Survey info returned from survey_repository.
/// (repo_name, commit_sha, discovery_count, detected_languages, success)
type SurveyInfo = (String, String, usize, Vec<String>, bool);

/// Survey a single repository.
/// Returns info for incremental state tracking: (repo_name, sha, discovery_count, languages, success)
async fn survey_repository(
    repo: &RepoInfo,
    cache: &RepoCache,
    builder: &mut GraphBuilder,
    config: &ForgeConfig,
    _options: &SurveyOptions,
    registry: &ParserRegistry,
) -> Result<SurveyInfo, SurveyError> {
    // Determine local path
    let local_path = if repo.owner == "local" {
        // Local repository - use the full_name as the path
        PathBuf::from(&repo.full_name)
    } else {
        // GitHub repository - clone/update to cache
        if output::is_verbose() {
            println!("  Cloning/updating repository...");
        }
        let token = config.github_token().ok();
        cache.ensure_repo(repo, token.as_deref()).await?
    };

    if output::is_verbose() {
        println!("  Local path: {}", local_path.display());
    }

    // Get commit SHA for tracking (use get_current_commit for both local and remote)
    let commit_sha = get_current_commit(&local_path)
        .await
        .unwrap_or_else(|_| "unknown".to_string());

    // Set repository context in builder
    builder.set_repo_context(&repo.full_name, Some(&commit_sha));

    // Detect languages in the repository
    let detected = detect_languages(&local_path);
    let detected_languages: Vec<String> = detected.iter().map(|l| l.name.clone()).collect();

    if output::is_verbose() {
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
        if output::is_verbose() {
            if !config.languages.exclude.is_empty() {
                println!(
                    "  No parsers available after exclusions (excluded: {})",
                    config.languages.exclude.join(", ")
                );
            } else {
                println!("  No parsers available for detected languages");
            }
        }
        return Ok((
            repo.full_name.clone(),
            commit_sha,
            0,
            detected_languages,
            true,
        ));
    }

    if output::is_verbose() {
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
                    if output::is_verbose() {
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
                        if output::is_verbose() {
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
        if output::is_verbose() {
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
            deployment_metadata: None,
        };
        service_id = Some(builder.add_service(service));
    }

    let service_id = service_id.expect("service_id should be set at this point");

    // Run each parser and collect discoveries
    let mut total_discoveries = 0usize;
    for parser in &parsers {
        if output::is_verbose() {
            let extensions = parser.supported_extensions();
            println!("  Parsing {} files...", extensions.join("/"));
        }

        match parser.parse_repo(&local_path) {
            Ok(discoveries) => {
                let count = discoveries.len();
                if output::is_verbose() {
                    println!("    Found {} code discoveries", count);
                }
                total_discoveries += count;
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

    Ok((
        repo.full_name.clone(),
        commit_sha,
        total_discoveries,
        detected_languages,
        true,
    ))
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
