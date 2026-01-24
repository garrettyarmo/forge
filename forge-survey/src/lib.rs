//! Code analysis and discovery for Forge.
//!
//! This crate provides functionality for surveying codebases
//! and building knowledge graphs.
//!
//! # Overview
//!
//! The forge-survey crate is responsible for:
//!
//! - **GitHub Integration**: Discovering and cloning repositories from GitHub organizations
//! - **Code Parsing**: Analyzing source code using tree-sitter AST parsing
//! - **Discovery**: Detecting services, APIs, databases, and their relationships
//! - **Coupling Detection**: Finding implicit coupling between services via shared resources
//!
//! # Architecture
//!
//! The survey phase is **purely deterministic** - it uses only tree-sitter AST parsing
//! with no LLM calls. This ensures:
//!
//! - Reproducibility: Same input code always produces the same graph
//! - Speed: No API latency or rate limits
//! - Offline capability: Works without network for local repos
//! - Predictable costs: Zero token usage during survey
//!
//! # Modules
//!
//! - [`github`]: GitHub API client and repository caching
//! - [`parser`]: Language-specific code parsers and discovery types
//! - [`graph_builder`]: Converts parser discoveries into a knowledge graph
//! - [`coupling`]: Implicit coupling detection and resource access tracking
//! - [`incremental`]: Incremental survey support for efficient re-surveys

pub mod coupling;
pub mod detection;
pub mod github;
pub mod graph_builder;
pub mod incremental;
pub mod parser;

use forge_graph::{ForgeGraph, GraphError};
use std::collections::HashSet;
use std::path::PathBuf;
use thiserror::Error;

pub use coupling::{
    AccessEvidence, AccessType, CouplingAnalysisResult, CouplingAnalyzer, CouplingRisk,
    ImplicitCoupling, OwnershipAssignment, OwnershipReason, ResourceAccessMap, SharedAccess,
};
pub use detection::{DetectedLanguage, DetectedLanguages, DetectionMethod, detect_languages};
pub use github::{CloneMethod, GitHubClient, GitHubError, RepoCache, RepoInfo};
pub use graph_builder::GraphBuilder;
pub use incremental::{
    ChangeDetector, ChangeError, ChangeResult, RepoState, StateError, SurveyState,
    get_current_commit, is_parseable_file,
};
// Re-export commonly used parser types for convenience
pub use parser::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError, ParserRegistry, QueueOperationDiscovery,
    QueueOperationType, ServiceDiscovery,
};

#[derive(Debug, Error)]
pub enum SurveyError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("GitHub error: {0}")]
    GitHubError(#[from] GitHubError),

    #[error("Parser error: {0}")]
    ParserError(#[from] ParserError),

    #[error("Graph error: {0}")]
    GraphError(#[from] GraphError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid repository format: {0}. Expected 'owner/repo' or local path")]
    InvalidRepoFormat(String),

    #[error("No repositories to survey.")]
    NoRepositories,
}

#[derive(Debug, Clone, Default)]
pub struct SurveyConfig {
    pub sources: Vec<PathBuf>,
    pub exclusions: HashSet<String>,
    pub cache_path: Option<PathBuf>,
    pub github_token: Option<String>,
}

pub async fn survey(config: SurveyConfig) -> Result<ForgeGraph, SurveyError> {
    let mut builder = GraphBuilder::new();
    let registry = ParserRegistry::new()?;

    for source in &config.sources {
        tracing::info!("Surveying: {}", source.display());

        let detected_langs = detect_languages(source);
        let exclusions: Vec<String> = config.exclusions.iter().cloned().collect();
        let parsers = registry.get_for_languages(&detected_langs, &exclusions);

        if parsers.is_empty() {
            tracing::warn!("No applicable parsers found for {}", source.display());
            continue;
        }

        let service_name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown_service")
            .to_string();

        let mut service_id = None;

        if let Some(parser) = registry.get("javascript") {
            if let Some(js_parser) = parser
                .as_ref()
                .as_any()
                .downcast_ref::<parser::javascript::JavaScriptParser>()
            {
                if let Some(service) = js_parser.parse_package_json(source) {
                    service_id = Some(builder.add_service(service));
                }
            }
        }

        if service_id.is_none() {
            if let Some(parser) = registry.get("python") {
                if let Some(py_parser) = parser
                    .as_ref()
                    .as_any()
                    .downcast_ref::<parser::python::PythonParser>()
                {
                    if let Some(service) = py_parser.parse_project_config(source) {
                        service_id = Some(builder.add_service(service));
                    }
                }
            }
        }

        let service_id = service_id.unwrap_or_else(|| {
            builder.add_service(ServiceDiscovery {
                name: service_name,
                language: detected_langs
                    .iter()
                    .next()
                    .map(|l| l.name.clone())
                    .unwrap_or_default(),
                ..Default::default()
            })
        });

        for parser in parsers {
            let discoveries = parser.parse_repo(source)?;
            builder.process_discoveries(discoveries, &service_id);
        }
    }

    Ok(builder.build())
}
