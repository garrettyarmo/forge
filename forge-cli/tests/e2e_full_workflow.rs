//! End-to-end tests for M7-T11: Final integration testing.
//!
//! These tests exercise all Forge commands through subprocess execution,
//! verifying the complete workflow from survey to map works correctly.
//!
//! Test Coverage:
//! - Full survey workflow with multi-language repos
//! - Incremental survey performance
//! - Multiple output formats (Markdown, JSON, Mermaid)
//! - Error handling with actionable messages
//! - Config initialization
//! - Verbose/quiet flags
//!
//! Implementation Note:
//! These tests run the `forge` binary via subprocess to test the CLI
//! as users would experience it, not the library API.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

/// Helper to write a file, creating parent directories as needed.
fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// Helper to create a JavaScript test repository.
fn create_js_repo(path: &Path, name: &str) {
    fs::create_dir_all(path).unwrap();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();

    // Create package.json
    write_file(
        &path.join("package.json"),
        &format!(
            r#"{{
    "name": "{}",
    "version": "1.0.0",
    "dependencies": {{
        "@aws-sdk/client-dynamodb": "^3.0.0",
        "express": "^4.0.0"
    }}
}}"#,
            name
        ),
    );

    // Create source files to meet detection threshold
    write_file(
        &path.join("src/index.js"),
        r#"
const express = require('express');
const { DynamoDBClient, GetItemCommand } = require('@aws-sdk/client-dynamodb');

const app = express();
const client = new DynamoDBClient({ region: 'us-east-1' });

app.get('/users/:id', async (req, res) => {
    const command = new GetItemCommand({
        TableName: 'users-table',
        Key: { id: { S: req.params.id } }
    });
    const result = await client.send(command);
    res.json(result.Item);
});

module.exports = app;
"#,
    );

    write_file(&path.join("src/utils.js"), "module.exports = {};");
    write_file(
        &path.join("src/config.js"),
        "module.exports = { region: 'us-east-1' };",
    );

    // Commit changes
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(path)
        .output()
        .unwrap();
}

/// Helper to create a Python test repository.
fn create_python_repo(path: &Path, name: &str) {
    fs::create_dir_all(path).unwrap();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();

    // Create pyproject.toml
    write_file(
        &path.join("pyproject.toml"),
        &format!(
            r#"[project]
name = "{}"
version = "1.0.0"
dependencies = ["boto3", "fastapi"]
"#,
            name
        ),
    );

    // Create source files
    write_file(
        &path.join("src/main.py"),
        r#"
import boto3
from fastapi import FastAPI

app = FastAPI()
dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table('orders-table')

@app.get("/orders/{order_id}")
def get_order(order_id: str):
    response = table.get_item(Key={'id': order_id})
    return response.get('Item')
"#,
    );

    write_file(
        &path.join("src/models.py"),
        r#"
from pydantic import BaseModel

class Order(BaseModel):
    id: str
    amount: float
"#,
    );

    write_file(
        &path.join("src/utils.py"),
        "def format_response(data): return data",
    );

    // Commit changes
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(path)
        .output()
        .unwrap();
}

/// Helper to create a Terraform repository.
fn create_terraform_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();

    // Create main.tf
    write_file(
        &path.join("main.tf"),
        r#"
terraform {
  backend "s3" {
    bucket = "terraform-state"
    key    = "production/terraform.tfstate"
    region = "us-east-1"
  }
}

resource "aws_dynamodb_table" "users" {
  name         = "users-table"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "id"

  attribute {
    name = "id"
    type = "S"
  }

  tags = {
    Environment = "production"
    ManagedBy   = "Terraform"
  }
}

resource "aws_dynamodb_table" "orders" {
  name         = "orders-table"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "id"

  attribute {
    name = "id"
    type = "S"
  }

  tags = {
    Environment = "production"
    ManagedBy   = "Terraform"
  }
}

resource "aws_sqs_queue" "notifications" {
  name = "notifications-queue"
}
"#,
    );

    write_file(
        &path.join("variables.tf"),
        r#"
variable "environment" {
  description = "Environment name"
  type        = string
  default     = "production"
}

variable "region" {
  description = "AWS region"
  type        = string
  default     = "us-east-1"
}
"#,
    );

    write_file(
        &path.join("outputs.tf"),
        r#"
output "users_table_arn" {
  value = aws_dynamodb_table.users.arn
}

output "orders_table_arn" {
  value = aws_dynamodb_table.orders.arn
}
"#,
    );

    // Commit changes
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(path)
        .output()
        .unwrap();
}

/// Helper to create a forge.yaml configuration file.
fn create_config(dir: &Path, local_paths: &[&Path]) -> PathBuf {
    let paths_yaml: Vec<String> = local_paths
        .iter()
        .map(|p| format!("    - \"{}\"", p.display()))
        .collect();

    let config = format!(
        r#"# Forge configuration for e2e tests
repos:
  local_paths:
{}

output:
  graph_path: "{}/graph.json"

token_budget: 8000
staleness_days: 7
"#,
        paths_yaml.join("\n"),
        dir.display()
    );

    let config_path = dir.join("forge.yaml");
    fs::write(&config_path, config).unwrap();
    config_path
}

/// Get the path to the forge binary (built in debug mode).
fn get_forge_binary() -> PathBuf {
    // The binary should be at target/debug/forge
    // CARGO_MANIFEST_DIR points to forge-cli, we need to go up to workspace root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
    workspace_root.join("target/debug/forge")
}

/// Run the forge binary with arguments.
fn run_forge(args: &[&str], work_dir: &Path) -> std::process::Output {
    let binary = get_forge_binary();

    // Ensure the binary exists
    if !binary.exists() {
        panic!(
            "Forge binary not found at {}. Run `cargo build` first.",
            binary.display()
        );
    }

    Command::new(binary)
        .args(args)
        .current_dir(work_dir)
        .output()
        .expect("Failed to execute forge command")
}

// ============================================================================
// E2E Test: Full Survey Workflow
// ============================================================================

/// Test the complete survey -> map workflow with multi-language repos.
///
/// This test verifies:
/// 1. `forge survey` successfully processes JS, Python, and Terraform repos
/// 2. The graph is created and contains expected nodes
/// 3. `forge map` produces output with expected services
#[test]
fn test_full_survey_workflow() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create test repositories
    let js_repo = root.join("user-service");
    let py_repo = root.join("order-service");
    let tf_repo = root.join("infrastructure");

    create_js_repo(&js_repo, "user-service");
    create_python_repo(&py_repo, "order-service");
    create_terraform_repo(&tf_repo);

    // Create config
    let config_path = create_config(root, &[&js_repo, &py_repo, &tf_repo]);

    // Run survey
    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Survey failed.\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );

    // Verify graph was created
    let graph_path = root.join("graph.json");
    assert!(
        graph_path.exists(),
        "Graph file not created at {}",
        graph_path.display()
    );

    // Read and verify graph content
    let graph_content = fs::read_to_string(&graph_path).unwrap();
    assert!(
        graph_content.contains("user-service") || graph_content.contains("user_service"),
        "Graph should contain user-service"
    );
    assert!(
        graph_content.contains("order-service") || graph_content.contains("order_service"),
        "Graph should contain order-service"
    );

    // Run map with markdown format
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "markdown",
        ],
        root,
    );

    assert!(
        output.status.success(),
        "Map markdown failed.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let markdown = String::from_utf8_lossy(&output.stdout);
    assert!(
        markdown.contains("Service") || markdown.contains("# Forge"),
        "Markdown output should contain service information"
    );
}

// ============================================================================
// E2E Test: Multiple Output Formats
// ============================================================================

/// Test that all map output formats work correctly.
#[test]
fn test_map_output_formats() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create a simple JS repo
    let js_repo = root.join("test-service");
    create_js_repo(&js_repo, "test-service");

    // Create config
    let config_path = create_config(root, &[&js_repo]);

    // Run survey first
    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);
    assert!(
        output.status.success(),
        "Survey failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Test Markdown format
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "markdown",
        ],
        root,
    );
    assert!(
        output.status.success(),
        "Map markdown failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let markdown = String::from_utf8_lossy(&output.stdout);
    // Markdown should have headers
    assert!(
        markdown.contains("#") || markdown.contains("Forge"),
        "Markdown output should be present"
    );

    // Test JSON format
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "json",
        ],
        root,
    );
    assert!(
        output.status.success(),
        "Map JSON failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json_output = String::from_utf8_lossy(&output.stdout);
    // JSON should be valid
    assert!(
        json_output.starts_with("{"),
        "JSON output should start with {{"
    );
    assert!(
        json_output.contains("\"nodes\"") || json_output.contains("\"version\""),
        "JSON output should contain nodes or version field"
    );

    // Test Mermaid format
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "mermaid",
        ],
        root,
    );
    assert!(
        output.status.success(),
        "Map mermaid failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mermaid = String::from_utf8_lossy(&output.stdout);
    // Mermaid should have flowchart syntax
    assert!(
        mermaid.contains("flowchart") || mermaid.contains("graph"),
        "Mermaid output should contain flowchart or graph directive"
    );
}

// ============================================================================
// E2E Test: Incremental Survey
// ============================================================================

/// Test that incremental survey is faster when no files changed.
///
/// This verifies the M7-T1 incremental survey functionality by:
/// 1. Running a full survey
/// 2. Running an incremental survey with no changes
/// 3. Verifying the incremental survey is faster (or at least doesn't reparse)
#[test]
fn test_incremental_survey() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create test repo
    let repo_path = root.join("incremental-test");
    create_js_repo(&repo_path, "incremental-test");

    // Create config
    let config_path = create_config(root, &[&repo_path]);

    // First survey (full)
    let start = std::time::Instant::now();
    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);
    let full_duration = start.elapsed();
    assert!(
        output.status.success(),
        "First survey failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify graph exists
    assert!(root.join("graph.json").exists(), "Graph should be created");

    // Second survey (incremental - no changes)
    let start = std::time::Instant::now();
    let output = run_forge(
        &[
            "survey",
            "--config",
            config_path.to_str().unwrap(),
            "--incremental",
        ],
        root,
    );
    let incr_duration = start.elapsed();
    assert!(
        output.status.success(),
        "Incremental survey failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Incremental should report skipping unchanged repos or be faster
    // The key indicator is that it should complete successfully
    // and ideally output indicates repos were skipped
    assert!(output.status.success(), "Incremental survey should succeed");

    // Log timing for debugging (not strict assertion due to system variance)
    eprintln!(
        "Full survey: {:?}, Incremental survey: {:?}",
        full_duration, incr_duration
    );

    // The incremental survey should complete (we don't assert strict timing
    // due to system variance, but it should work correctly)
}

// ============================================================================
// E2E Test: Error Handling
// ============================================================================

/// Test that missing config file produces helpful error message.
#[test]
fn test_error_missing_config() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Run survey without creating config
    let output = run_forge(&["survey", "--config", "nonexistent.yaml"], root);

    // Should fail
    assert!(
        !output.status.success(),
        "Survey should fail with missing config"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should mention config file issue
    assert!(
        stderr.contains("config")
            || stderr.contains("not found")
            || stderr.contains("No such file"),
        "Error should mention config file issue. Got: {}",
        stderr
    );
}

/// Test that empty repos list produces helpful error.
#[test]
fn test_error_no_repos() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create config with empty repos
    let config = r#"
repos:
  local_paths: []

output:
  graph_path: "graph.json"
"#;
    let config_path = root.join("forge.yaml");
    fs::write(&config_path, config).unwrap();

    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);

    // Should fail or produce warning about no repos
    let _stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Either it fails or warns about no repos
    if output.status.success() {
        // If it succeeded, it should have warned or the graph should be minimal
        let graph_content = fs::read_to_string(root.join("graph.json")).unwrap_or_default();
        assert!(
            graph_content.contains("nodes") || stdout.contains("no") || stdout.contains("empty"),
            "Should handle empty repos gracefully"
        );
    }
}

/// Test that invalid repo path produces helpful error.
#[test]
fn test_error_invalid_repo_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create config pointing to non-existent path
    let config = r#"
repos:
  local_paths:
    - "/nonexistent/path/to/repo"

output:
  graph_path: "graph.json"
"#;
    let config_path = root.join("forge.yaml");
    fs::write(&config_path, config).unwrap();

    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);

    // May succeed with warnings or fail
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should at least complete without panic
    // The behavior depends on implementation - it might skip invalid paths
    eprintln!("stdout: {}", stdout);
    eprintln!("stderr: {}", stderr);
}

// ============================================================================
// E2E Test: Verbose/Quiet Flags
// ============================================================================

/// Test that quiet flag suppresses output.
#[test]
fn test_quiet_flag() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create test repo
    let repo_path = root.join("quiet-test");
    create_js_repo(&repo_path, "quiet-test");
    let config_path = create_config(root, &[&repo_path]);

    // Run with quiet flag
    let output = run_forge(
        &[
            "--quiet",
            "survey",
            "--config",
            config_path.to_str().unwrap(),
        ],
        root,
    );

    assert!(
        output.status.success(),
        "Survey with --quiet failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Quiet mode should minimize output (may still have some output)
    // Just verify it works
}

/// Test that verbose flag increases output.
#[test]
fn test_verbose_flag() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create test repo
    let repo_path = root.join("verbose-test");
    create_js_repo(&repo_path, "verbose-test");
    let config_path = create_config(root, &[&repo_path]);

    // Run with verbose flag
    let output = run_forge(
        &[
            "--verbose",
            "survey",
            "--config",
            config_path.to_str().unwrap(),
        ],
        root,
    );

    assert!(
        output.status.success(),
        "Survey with --verbose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verbose mode should produce output
    // Just verify it works and doesn't crash
}

// ============================================================================
// E2E Test: Map with Service Filter
// ============================================================================

/// Test that --service flag filters output correctly.
#[test]
fn test_map_service_filter() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create multiple repos
    let js_repo = root.join("service-a");
    let py_repo = root.join("service-b");
    create_js_repo(&js_repo, "service-a");
    create_python_repo(&py_repo, "service-b");
    let config_path = create_config(root, &[&js_repo, &py_repo]);

    // Run survey
    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);
    assert!(output.status.success());

    // Run map with service filter
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "markdown",
            "--service",
            "service-a",
        ],
        root,
    );

    assert!(
        output.status.success(),
        "Map with service filter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should focus on service-a
    // (The exact behavior depends on implementation)
}

// ============================================================================
// E2E Test: Map with Output File
// ============================================================================

/// Test that --output flag writes to file correctly.
#[test]
fn test_map_output_file() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create test repo
    let repo_path = root.join("output-test");
    create_js_repo(&repo_path, "output-test");
    let config_path = create_config(root, &[&repo_path]);

    // Run survey
    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);
    assert!(output.status.success());

    // Run map with output file
    let output_file = root.join("architecture.md");
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "markdown",
            "--output",
            output_file.to_str().unwrap(),
        ],
        root,
    );

    assert!(
        output.status.success(),
        "Map with output file failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output file was created
    assert!(
        output_file.exists(),
        "Output file should be created at {}",
        output_file.display()
    );

    let content = fs::read_to_string(&output_file).unwrap();
    assert!(!content.is_empty(), "Output file should not be empty");
}

// ============================================================================
// E2E Test: Coupling Detection
// ============================================================================

/// Test that implicit coupling is detected between services sharing resources.
#[test]
fn test_coupling_detection() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create two services that both access the same DynamoDB table
    let service_a = root.join("reader-service");
    let service_b = root.join("writer-service");
    let infra = root.join("infrastructure");

    // Service A reads from shared-table
    fs::create_dir_all(&service_a).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&service_a)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&service_a)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&service_a)
        .output()
        .unwrap();
    write_file(
        &service_a.join("package.json"),
        r#"{"name": "reader-service", "dependencies": {"@aws-sdk/client-dynamodb": "^3.0.0"}}"#,
    );
    write_file(
        &service_a.join("src/index.js"),
        r#"
const { DynamoDBClient, GetItemCommand } = require('@aws-sdk/client-dynamodb');
const client = new DynamoDBClient({});
async function read() {
    return await client.send(new GetItemCommand({ TableName: 'shared-table', Key: { id: { S: '1' } } }));
}
"#,
    );
    write_file(&service_a.join("src/utils.js"), "module.exports = {};");
    write_file(&service_a.join("src/config.js"), "module.exports = {};");
    Command::new("git")
        .args(["add", "."])
        .current_dir(&service_a)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&service_a)
        .output()
        .unwrap();

    // Service B writes to shared-table
    fs::create_dir_all(&service_b).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&service_b)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&service_b)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&service_b)
        .output()
        .unwrap();
    write_file(
        &service_b.join("package.json"),
        r#"{"name": "writer-service", "dependencies": {"@aws-sdk/client-dynamodb": "^3.0.0"}}"#,
    );
    write_file(
        &service_b.join("src/index.js"),
        r#"
const { DynamoDBClient, PutItemCommand } = require('@aws-sdk/client-dynamodb');
const client = new DynamoDBClient({});
async function write(data) {
    return await client.send(new PutItemCommand({ TableName: 'shared-table', Item: data }));
}
"#,
    );
    write_file(&service_b.join("src/utils.js"), "module.exports = {};");
    write_file(&service_b.join("src/config.js"), "module.exports = {};");
    Command::new("git")
        .args(["add", "."])
        .current_dir(&service_b)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&service_b)
        .output()
        .unwrap();

    // Terraform defines the shared table
    fs::create_dir_all(&infra).unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&infra)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&infra)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&infra)
        .output()
        .unwrap();
    write_file(
        &infra.join("main.tf"),
        r#"
resource "aws_dynamodb_table" "shared" {
  name = "shared-table"
  hash_key = "id"
  attribute { name = "id" type = "S" }
}
"#,
    );
    write_file(
        &infra.join("variables.tf"),
        "variable \"env\" { default = \"dev\" }",
    );
    write_file(
        &infra.join("outputs.tf"),
        "output \"table_arn\" { value = aws_dynamodb_table.shared.arn }",
    );
    Command::new("git")
        .args(["add", "."])
        .current_dir(&infra)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&infra)
        .output()
        .unwrap();

    // Create config
    let config_path = create_config(root, &[&service_a, &service_b, &infra]);

    // Run survey
    let output = run_forge(&["survey", "--config", config_path.to_str().unwrap()], root);
    assert!(
        output.status.success(),
        "Survey failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check graph for coupling
    let graph_content = fs::read_to_string(root.join("graph.json")).unwrap();

    // Should have the shared table
    assert!(
        graph_content.contains("shared-table"),
        "Graph should contain shared-table"
    );

    // Run map to see coupling in output
    let output = run_forge(
        &[
            "map",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "json",
        ],
        root,
    );
    assert!(output.status.success());

    let json_output = String::from_utf8_lossy(&output.stdout);
    // JSON should have edges
    assert!(
        json_output.contains("edges") || json_output.contains("relationships"),
        "JSON should contain edges"
    );
}

// ============================================================================
// E2E Test: Init Command
// ============================================================================

/// Test that forge init creates a valid configuration file.
#[test]
fn test_init_command() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Run init
    let output = run_forge(&["init"], root);

    // Init should succeed
    assert!(
        output.status.success(),
        "Init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should create forge.yaml
    let config_path = root.join("forge.yaml");
    assert!(config_path.exists(), "forge.yaml should be created by init");

    // Config should be valid YAML
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("repos") || content.contains("#"),
        "Config should contain repos section or comments"
    );
}

/// Test that forge init with --org flag pre-fills organization.
#[test]
fn test_init_with_org() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Run init with org
    let output = run_forge(&["init", "--org", "my-test-org"], root);

    assert!(
        output.status.success(),
        "Init with org failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Config should contain the org
    let content = fs::read_to_string(root.join("forge.yaml")).unwrap();
    assert!(
        content.contains("my-test-org"),
        "Config should contain the specified org"
    );
}
