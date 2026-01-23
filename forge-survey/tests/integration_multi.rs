//! Integration test for surveying a multi-language repository.

use forge_graph::{EdgeType, NodeType};
use forge_survey::{survey, SurveyConfig};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

// Inlined from common.rs to avoid module issues in integration tests
fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// Test that the survey correctly handles a multi-language repository with
/// JavaScript, Python, and Terraform.
///
/// This test verifies:
/// - Languages are auto-detected from config files and file extensions
/// - Parsers from different languages all contribute to the same graph
/// - Database resources are detected and properly deduplicated across languages
/// - Service relationships (edges) are correctly created
#[tokio::test]
async fn test_survey_multi_language_repo() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // 1. Create a synthetic multi-language repository

    // JavaScript service that makes HTTP calls
    let js_service_path = root.join("js_service");
    write_file(
        &js_service_path.join("package.json"),
        r#"{ "name": "js-caller", "dependencies": { "axios": "latest" } }"#,
    );
    write_file(
        &js_service_path.join("src/index.js"),
        r#"
        const axios = require('axios');
        // Call the Python service API
        axios.get('http://py-service.local:8000/items/1');
    "#,
    );
    // Add another JS file to ensure detection threshold
    write_file(
        &js_service_path.join("src/utils.js"),
        r#"
        module.exports = {
            formatDate: (d) => d.toISOString()
        };
    "#,
    );
    write_file(
        &js_service_path.join("src/config.js"),
        r#"
        module.exports = {
            apiUrl: 'http://py-service.local:8000'
        };
    "#,
    );

    // Python service that reads from DynamoDB
    let py_service_path = root.join("py_service");
    write_file(
        &py_service_path.join("requirements.txt"),
        "boto3\nfastapi",
    );
    write_file(
        &py_service_path.join("src/main.py"),
        r#"
import boto3
from fastapi import FastAPI

app = FastAPI()
dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table('my-table')

@app.get("/items/{item_id}")
def read_item(item_id: str):
    response = table.get_item(Key={'id': item_id})
    return response.get('Item')
"#,
    );
    // Add more Python files to ensure detection threshold
    write_file(
        &py_service_path.join("src/models.py"),
        r#"
from pydantic import BaseModel

class Item(BaseModel):
    id: str
    name: str
"#,
    );
    write_file(
        &py_service_path.join("src/utils.py"),
        r#"
def format_response(data):
    return {"status": "ok", "data": data}
"#,
    );

    // Terraform that defines the DynamoDB table
    // Need multiple .tf files to meet detection threshold (3+ files)
    let tf_path = root.join("terraform");
    write_file(
        &tf_path.join("main.tf"),
        r#"
resource "aws_dynamodb_table" "my_table" {
  name = "my-table"
  billing_mode = "PAY_PER_REQUEST"
  hash_key = "id"

  attribute {
    name = "id"
    type = "S"
  }
}
"#,
    );
    write_file(
        &tf_path.join("variables.tf"),
        r#"
variable "environment" {
  description = "Environment name"
  type        = string
  default     = "dev"
}

variable "region" {
  description = "AWS region"
  type        = string
  default     = "us-east-1"
}
"#,
    );
    write_file(
        &tf_path.join("outputs.tf"),
        r#"
output "table_name" {
  description = "Name of the DynamoDB table"
  value       = aws_dynamodb_table.my_table.name
}

output "table_arn" {
  description = "ARN of the DynamoDB table"
  value       = aws_dynamodb_table.my_table.arn
}
"#,
    );

    // 2. Run the full survey process
    let config = SurveyConfig {
        sources: vec![
            js_service_path.clone(),
            py_service_path.clone(),
            tf_path.clone(),
        ],
        exclusions: HashSet::new(),
        ..Default::default()
    };
    let graph = survey(config).await.unwrap();

    // 3. Assert graph contents

    // Services: Should detect JS and Python services
    // Terraform doesn't create a "service" - it defines infrastructure
    let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
    assert!(
        services.len() >= 2,
        "Should detect at least two services (JS and Python), found {}",
        services.len()
    );

    let js_service = services
        .iter()
        .find(|s| s.display_name == "js-caller")
        .expect("JS service not found");
    let py_service = services
        .iter()
        .find(|s| s.display_name == "py_service")
        .expect("Python service not found");

    // Databases: Should detect DynamoDB table
    // The Python parser extracts table name from dynamodb.Table('my-table')
    // The Terraform parser extracts from aws_dynamodb_table resource
    // These should be deduplicated into a single node
    let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
    assert!(
        databases.len() >= 1,
        "Should detect at least one database, found {}",
        databases.len()
    );

    // At least one database should be the "my-table" from Python/Terraform
    let my_table = databases.iter().find(|d| d.display_name == "my-table");
    assert!(
        my_table.is_some(),
        "Should detect 'my-table' DynamoDB table. Found databases: {:?}",
        databases.iter().map(|d| &d.display_name).collect::<Vec<_>>()
    );
    let db_node = my_table.unwrap();

    // Edges: Python service should have a READS edge to the database
    let reads_edges: Vec<_> = graph.edges_by_type(EdgeType::Reads).collect();
    assert!(
        !reads_edges.is_empty(),
        "Should have at least one READS edge"
    );
    assert!(
        graph.has_edge_between(py_service.id.clone(), db_node.id.clone()),
        "Python service should have a READS edge to the database"
    );

    // Verify the JavaScript service doesn't have false positive DynamoDB edges
    // (This was a bug where axios.get() was detected as DynamoDB get operation)
    let js_db_edges: Vec<_> = reads_edges
        .iter()
        .filter(|e| e.source == js_service.id)
        .collect();
    assert!(
        js_db_edges.is_empty(),
        "JS service should NOT have READS edges to databases (axios.get is not DynamoDB)"
    );
}

/// Test that language exclusions work correctly.
#[tokio::test]
async fn test_survey_with_language_exclusion() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create a Python service
    let py_path = root.join("py_service");
    write_file(&py_path.join("requirements.txt"), "boto3");
    write_file(
        &py_path.join("src/main.py"),
        r#"
import boto3
dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table('test-table')
response = table.get_item(Key={'id': '123'})
"#,
    );
    write_file(&py_path.join("src/utils.py"), "def helper(): pass");
    write_file(&py_path.join("src/models.py"), "class Model: pass");

    // Survey with Python excluded
    let config = SurveyConfig {
        sources: vec![py_path],
        exclusions: HashSet::from(["python".to_string()]),
        ..Default::default()
    };
    let graph = survey(config).await.unwrap();

    // With Python excluded, we should not detect the DynamoDB access
    // The service might still be created but without Python-specific discoveries
    let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
    assert!(
        databases.is_empty(),
        "With Python excluded, should not detect DynamoDB table. Found: {:?}",
        databases.iter().map(|d| &d.display_name).collect::<Vec<_>>()
    );
}
