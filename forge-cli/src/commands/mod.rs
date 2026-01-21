//! CLI commands for Forge.
//!
//! This module contains the implementations of all Forge CLI commands:
//!
//! - `forge init` - Initialize a new `forge.yaml` configuration file
//! - `forge survey` - Survey repositories and build the knowledge graph (to be implemented in M2-T8)
//! - `forge map` - Serialize the knowledge graph to various formats (to be implemented in M5)

pub mod init;

pub use init::{InitOptions, run_init};
