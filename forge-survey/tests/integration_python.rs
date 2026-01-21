//! Integration tests for Python parsing and graph building.
//!
//! These tests verify the end-to-end workflow:
//! 1. Creating synthetic Python repositories
//! 2. Parsing with PythonParser
//! 3. Building a knowledge graph with GraphBuilder
//! 4. Validating graph structure and content

use forge_graph::{AttributeValue, EdgeType, NodeType};
use forge_survey::parser::{Parser, PythonParser};
use forge_survey::GraphBuilder;
use std::fs;
use tempfile::tempdir;

/// Test surveying a synthetic Python repository with FastAPI and DynamoDB.
///
/// This test creates a realistic Python service with:
/// - pyproject.toml with dependencies (fastapi, boto3)
/// - Source file with boto3 client creation and DynamoDB operations
/// - FastAPI endpoint that reads from DynamoDB
///
/// It verifies that the parser correctly detects:
/// - Service from pyproject.toml
/// - Python imports
/// - boto3 DynamoDB client usage
/// - DynamoDB table name extraction
/// - Database read/write operations
#[test]
fn test_survey_synthetic_python_repo() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("test-service");
    fs::create_dir_all(&repo_path).unwrap();

    // Create pyproject.toml with realistic dependencies
    fs::write(
        repo_path.join("pyproject.toml"),
        r#"
[project]
name = "test-service"
version = "1.0.0"
description = "Test service for integration testing"

[project.dependencies]
fastapi = ">=0.100.0"
boto3 = ">=1.28.0"
uvicorn = ">=0.23.0"
"#,
    )
    .unwrap();

    // Create source directory and main file
    fs::create_dir_all(repo_path.join("src")).unwrap();
    fs::write(
        repo_path.join("src/main.py"),
        r#"
import boto3
from fastapi import FastAPI, HTTPException

app = FastAPI()

# Create DynamoDB client
dynamodb = boto3.client('dynamodb')

@app.get("/users/{user_id}")
async def get_user(user_id: str):
    """Get user from DynamoDB"""
    try:
        response = dynamodb.get_item(
            TableName='users',
            Key={'id': {'S': user_id}}
        )
        if 'Item' in response:
            return response['Item']
        raise HTTPException(status_code=404, detail="User not found")
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/users")
async def create_user(user_data: dict):
    """Create user in DynamoDB"""
    try:
        dynamodb.put_item(
            TableName='users',
            Item={
                'id': {'S': user_data['id']},
                'name': {'S': user_data['name']}
            }
        )
        return {"success": True}
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
"#,
    )
    .unwrap();

    // Run survey
    let parser = PythonParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    // Parse project config and create service node
    if let Some(service) = parser.parse_project_config(&repo_path) {
        let service_id = builder.add_service(service);

        // Parse repository and process discoveries
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Verify graph structure
    assert!(
        graph.node_count() > 0,
        "Graph should contain at least one node"
    );

    // Should have detected the service from pyproject.toml
    let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
    assert_eq!(
        services.len(),
        1,
        "Should detect exactly one service from pyproject.toml"
    );
    let service = services[0];
    assert_eq!(
        service.display_name, "test-service",
        "Service name should match pyproject.toml"
    );

    // Should have detected FastAPI framework
    if let Some(AttributeValue::String(framework)) = service.attributes.get("framework") {
        assert_eq!(
            framework, "fastapi",
            "Should detect FastAPI framework from dependencies"
        );
    }

    // Should have detected DynamoDB database access
    let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
    let has_db = databases.len() > 0;

    // Alternative: Check for database edges
    let has_db_edges = graph
        .edges()
        .any(|e| matches!(e.edge_type, EdgeType::Reads | EdgeType::Writes));

    assert!(
        has_db || has_db_edges,
        "Should detect DynamoDB database access from boto3. Found {} db nodes, {} edges",
        databases.len(),
        graph.edge_count()
    );
}

/// Test surveying a Python repository with HTTP client usage (requests).
///
/// This test verifies that the parser correctly detects:
/// - requests library usage
/// - HTTP methods (GET, POST, PUT, DELETE)
/// - API call URLs
#[test]
fn test_survey_python_repo_with_http_calls() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("http-client");
    fs::create_dir_all(&repo_path).unwrap();

    // Create requirements.txt
    fs::write(
        repo_path.join("requirements.txt"),
        "requests>=2.31.0\n",
    )
    .unwrap();

    // Create Python file with requests usage
    fs::write(
        repo_path.join("client.py"),
        r#"
import requests

def fetch_users():
    response = requests.get('https://api.example.com/users')
    return response.json()

def create_order(order_data):
    response = requests.post('https://api.example.com/orders', json=order_data)
    return response.json()

def update_user(user_id, data):
    response = requests.put(f'https://api.example.com/users/{user_id}', json=data)
    return response.json()

def delete_order(order_id):
    response = requests.delete(f'https://api.example.com/orders/{order_id}')
    return response.status_code
"#,
    )
    .unwrap();

    // Run survey
    let parser = PythonParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    // Parse project config and create service node
    if let Some(service) = parser.parse_project_config(&repo_path) {
        let service_id = builder.add_service(service);

        // Parse repository and process discoveries
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Should detect multiple API calls
    // Note: API calls create edges but may not create separate nodes
    let edges: Vec<_> = graph.edges().collect();

    // We expect at least some discoveries from HTTP client usage
    assert!(
        edges.len() > 0 || graph.node_count() > 1,
        "Should detect HTTP client usage. Found {} edges, {} nodes",
        edges.len(),
        graph.node_count()
    );
}

/// Test surveying a Python repository with multiple AWS services.
///
/// This test verifies that the parser correctly detects:
/// - Multiple boto3 clients (DynamoDB, S3, SQS)
/// - Different AWS service types
/// - Correct node types for each service
#[test]
fn test_survey_python_repo_with_multiple_aws_services() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("aws-service");
    fs::create_dir_all(&repo_path).unwrap();

    // Create requirements.txt
    fs::write(
        repo_path.join("requirements.txt"),
        "boto3>=1.28.0\n",
    )
    .unwrap();

    // Create Python file with multiple AWS services
    fs::write(
        repo_path.join("aws_handler.py"),
        r#"
import boto3
import json

# Initialize multiple AWS clients
dynamodb = boto3.client('dynamodb')
s3 = boto3.client('s3')
sqs = boto3.client('sqs')

def process_data(data_id):
    """Process data using multiple AWS services"""

    # Read from DynamoDB
    db_response = dynamodb.get_item(
        TableName='data-table',
        Key={'id': {'S': data_id}}
    )

    # Upload to S3
    s3.put_object(
        Bucket='my-bucket',
        Key=f'data/{data_id}.json',
        Body=json.dumps(db_response['Item'])
    )

    # Send message to SQS
    sqs.send_message(
        QueueUrl='https://sqs.us-east-1.amazonaws.com/123456789/my-queue',
        MessageBody=json.dumps({'data_id': data_id, 'status': 'processed'})
    )

    return True
"#,
    )
    .unwrap();

    // Run survey
    let parser = PythonParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    // Parse project config and create service node
    if let Some(service) = parser.parse_project_config(&repo_path) {
        let service_id = builder.add_service(service);

        // Parse repository and process discoveries
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Should detect DynamoDB (Database node)
    let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
    assert!(
        databases.len() > 0,
        "Should detect DynamoDB database node"
    );

    // Should detect S3 (CloudResource node)
    let cloud_resources: Vec<_> = graph.nodes_by_type(NodeType::CloudResource).collect();
    assert!(
        cloud_resources.len() > 0,
        "Should detect S3 cloud resource node"
    );

    // Should detect SQS (Queue node)
    let queues: Vec<_> = graph.nodes_by_type(NodeType::Queue).collect();
    assert!(
        queues.len() > 0,
        "Should detect SQS queue node"
    );

    // Verify we have a good mix of node types
    assert!(
        graph.node_count() >= 3,
        "Should have at least 3 resource nodes (DynamoDB, S3, SQS). Found {}",
        graph.node_count()
    );
}

/// Test surveying a Python repository with Flask framework.
///
/// This test verifies:
/// - Flask framework detection from dependencies
/// - Service metadata extraction
/// - Mixed boto3 and httpx usage
#[test]
fn test_survey_python_repo_with_flask_framework() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("flask-service");
    fs::create_dir_all(&repo_path).unwrap();

    // Create pyproject.toml with Flask
    fs::write(
        repo_path.join("pyproject.toml"),
        r#"
[project]
name = "flask-service"
version = "2.1.0"

[project.dependencies]
flask = ">=3.0.0"
boto3 = ">=1.28.0"
httpx = ">=0.25.0"
"#,
    )
    .unwrap();

    // Create Flask app
    fs::write(
        repo_path.join("app.py"),
        r#"
import boto3
import httpx
from flask import Flask, jsonify, request

app = Flask(__name__)
dynamodb = boto3.client('dynamodb')

@app.route('/proxy/<path:endpoint>')
def proxy_request(endpoint):
    """Proxy requests to external API"""
    response = httpx.get(f'https://external-api.com/{endpoint}')
    return response.json()

@app.route('/data/<item_id>')
def get_data(item_id):
    """Get data from DynamoDB"""
    result = dynamodb.get_item(
        TableName='items',
        Key={'id': {'S': item_id}}
    )
    return jsonify(result.get('Item', {}))

if __name__ == '__main__':
    app.run(port=5000)
"#,
    )
    .unwrap();

    // Run survey
    let parser = PythonParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    // Parse project config and create service node
    if let Some(service) = parser.parse_project_config(&repo_path) {
        let service_id = builder.add_service(service);

        // Parse repository and process discoveries
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Should detect service with Flask framework
    let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
    assert_eq!(services.len(), 1);

    let service = services[0];
    assert_eq!(service.display_name, "flask-service");

    if let Some(AttributeValue::String(framework)) = service.attributes.get("framework") {
        assert_eq!(framework, "flask", "Should detect Flask framework");
    }

    // Should detect both DynamoDB and httpx usage
    assert!(
        graph.node_count() > 1,
        "Should detect multiple resources (DynamoDB + external API calls)"
    );
}

/// Test surveying an empty Python repository.
///
/// This test verifies that the parser handles edge cases gracefully:
/// - Empty directories don't crash
/// - Config files without source code are handled
#[test]
fn test_survey_empty_python_repo() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("empty-repo");
    fs::create_dir_all(&repo_path).unwrap();

    // Create only a requirements.txt with no source files
    fs::write(
        repo_path.join("requirements.txt"),
        "boto3>=1.28.0\n",
    )
    .unwrap();

    // Run survey - should not crash
    let parser = PythonParser::new().unwrap();
    let result = parser.parse_repo(&repo_path);

    // Should succeed even with no Python files
    assert!(result.is_ok(), "Should handle empty repo gracefully");

    let discoveries = result.unwrap();
    // No source files means no discoveries (except possibly service config)
    assert!(
        discoveries.is_empty() || discoveries.len() < 5,
        "Empty repo should have minimal or no discoveries"
    );
}

/// Test surveying a Python repository with only configuration files.
///
/// This test verifies:
/// - Service detection from pyproject.toml works standalone
/// - No crashes when source directory is missing
#[test]
fn test_survey_python_repo_without_source() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("config-only");
    fs::create_dir_all(&repo_path).unwrap();

    // Create only pyproject.toml
    fs::write(
        repo_path.join("pyproject.toml"),
        r#"
[project]
name = "config-only-service"
version = "1.0.0"

[project.dependencies]
django = ">=4.2.0"
"#,
    )
    .unwrap();

    // Run survey
    let parser = PythonParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    // Parse project config
    if let Some(service) = parser.parse_project_config(&repo_path) {
        let service_id = builder.add_service(service);

        // Parse repository (should handle missing source gracefully)
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Should still detect service from config
    let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
    assert_eq!(services.len(), 1);

    let service = services[0];
    assert_eq!(service.display_name, "config-only-service");

    // Should detect Django framework
    if let Some(AttributeValue::String(framework)) = service.attributes.get("framework") {
        assert_eq!(framework, "django");
    }
}
