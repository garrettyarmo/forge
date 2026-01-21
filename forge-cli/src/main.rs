//! Forge CLI - Survey and map software ecosystems.
//!
//! Forge is a reusable platform for surveying and mapping software ecosystems.
//! It builds a knowledge graph of services, APIs, databases, and their relationships,
//! then serializes that graph into LLM-optimized context for intelligent assistance.
//!
//! # Commands
//!
//! - `forge init` - Initialize a new configuration file
//! - `forge survey` - Survey repositories and build the knowledge graph
//! - `forge map` - Serialize the knowledge graph to various formats
//!
//! # Usage
//!
//! ```bash
//! # Initialize a new project
//! forge init --org my-company
//!
//! # Survey repositories
//! forge survey
//!
//! # Generate map output
//! forge map --format markdown
//! ```

use clap::{Parser, Subcommand};

mod commands;
mod config;

/// Forge - Survey and map software ecosystems
#[derive(Parser)]
#[command(name = "forge")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new forge.yaml configuration file
    Init {
        /// GitHub organization to pre-fill in the configuration
        #[arg(long)]
        org: Option<String>,

        /// Output path for the configuration file
        #[arg(long, short)]
        output: Option<String>,

        /// Overwrite existing configuration file
        #[arg(long, short)]
        force: bool,
    },

    /// Survey repositories and build the knowledge graph
    Survey {
        /// Path to the configuration file
        #[arg(long, short)]
        config: Option<String>,

        /// Override output graph path
        #[arg(long, short)]
        output: Option<String>,

        /// Override repos (comma-separated: owner/repo,owner/repo2)
        #[arg(long)]
        repos: Option<String>,

        /// Exclude languages (comma-separated: terraform,python)
        #[arg(long)]
        exclude_lang: Option<String>,

        /// Launch business context interview after survey (uses LLM CLI)
        #[arg(long)]
        business_context: bool,

        /// Only re-parse changed files
        #[arg(long)]
        incremental: bool,

        /// Show detailed progress
        #[arg(long, short)]
        verbose: bool,
    },

    /// Serialize the knowledge graph to various formats
    Map {
        /// Path to the configuration file
        #[arg(long, short)]
        config: Option<String>,

        /// Override input graph path
        #[arg(long, short)]
        input: Option<String>,

        /// Output format: markdown, json, mermaid
        #[arg(long, short, default_value = "markdown")]
        format: String,

        /// Filter to specific services (comma-separated)
        #[arg(long, short)]
        service: Option<String>,

        /// Token budget limit
        #[arg(long, short)]
        budget: Option<u32>,

        /// Output file (default: stdout)
        #[arg(long, short)]
        output: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { org, output, force } => {
            let options = commands::InitOptions { org, output, force };
            commands::run_init(options).map_err(|e| e.to_string())
        }
        Commands::Survey {
            config,
            output,
            repos,
            exclude_lang,
            business_context,
            incremental,
            verbose,
        } => {
            let options = commands::SurveyOptions {
                config,
                output,
                repos,
                exclude_lang,
                business_context,
                incremental,
                verbose,
            };
            // Survey is async, so we need a tokio runtime
            match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime
                    .block_on(commands::run_survey(options))
                    .map_err(|e| e.to_string()),
                Err(e) => Err(format!("Failed to create tokio runtime: {}", e)),
            }
        }
        Commands::Map {
            config,
            input,
            format,
            service,
            budget,
            output,
        } => {
            // TODO: Implement map command (M5)
            let _ = (config, input, format, service, budget, output);
            println!("Map command not yet implemented. Coming in Milestone 5.");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
