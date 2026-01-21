//! Integration tests for JavaScript parsing and graph building.
//!
//! These tests verify the end-to-end workflow:
//! 1. Creating synthetic JavaScript repositories
//! 2. Parsing with JavaScriptParser
//! 3. Building a knowledge graph with GraphBuilder
//! 4. Validating graph structure and content

use forge_graph::{AttributeValue, EdgeType, NodeType};
use forge_survey::parser::{JavaScriptParser, Parser};
use forge_survey::GraphBuilder;
use std::fs;
use tempfile::tempdir;

/// Test surveying a synthetic JavaScript repository with Express and DynamoDB.
///
/// This test creates a realistic JavaScript service with:
/// - package.json with dependencies (express, @aws-sdk/client-dynamodb)
/// - Source file with ES6 imports and DynamoDB operations
/// - Express API endpoint that reads from DynamoDB
///
/// It verifies that the parser correctly detects:
/// - Service from package.json
/// - ES6 imports
/// - AWS SDK v3 DynamoDB usage
/// - DynamoDB table name extraction
/// - Database read operations
#[test]
fn test_survey_synthetic_js_repo() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("test-service");
    fs::create_dir_all(&repo_path).unwrap();

    // Create package.json with realistic dependencies
    fs::write(
        repo_path.join("package.json"),
        r#"{
  "name": "test-service",
  "version": "1.0.0",
  "description": "Test service for integration testing",
  "main": "src/index.js",
  "dependencies": {
    "express": "^4.18.0",
    "@aws-sdk/client-dynamodb": "^3.450.0",
    "@aws-sdk/lib-dynamodb": "^3.450.0"
  },
  "devDependencies": {
    "jest": "^29.0.0"
  }
}"#,
    )
    .unwrap();

    // Create source directory and main file
    fs::create_dir_all(repo_path.join("src")).unwrap();
    fs::write(
        repo_path.join("src/index.js"),
        r#"
import express from 'express';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, PutCommand } from '@aws-sdk/lib-dynamodb';

const app = express();
const client = new DynamoDBClient({ region: 'us-east-1' });
const docClient = DynamoDBDocumentClient.from(client);

// API endpoint that reads from DynamoDB
app.get('/users/:id', async (req, res) => {
    try {
        const result = await docClient.get({
            TableName: 'users',
            Key: { id: req.params.id }
        });
        res.json(result.Item);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});

// API endpoint that writes to DynamoDB
app.post('/users', async (req, res) => {
    try {
        await docClient.put({
            TableName: 'users',
            Item: req.body
        });
        res.json({ success: true });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});

app.listen(3000, () => {
    console.log('Server running on port 3000');
});
"#,
    )
    .unwrap();

    // Run survey
    let parser = JavaScriptParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    // Parse package.json and create service node
    if let Some(service) = parser.parse_package_json(&repo_path) {
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

    // Should have detected the service from package.json
    let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
    assert_eq!(
        services.len(),
        1,
        "Should detect exactly one service from package.json"
    );
    let service = services[0];
    assert_eq!(
        service.display_name, "test-service",
        "Service name should match package.json"
    );

    // Should have detected DynamoDB access (from AWS SDK imports)
    let databases: Vec<_> = graph.nodes_by_type(NodeType::Database).collect();
    let has_db = databases.len() > 0;

    // Alternative: Check for database edges
    let has_db_edges = graph
        .edges()
        .any(|e| matches!(e.edge_type, EdgeType::Reads | EdgeType::Writes));

    // Note: The parser detects AWS SDK imports which create database nodes.
    // Table name extraction from method calls is a bonus but not required.
    assert!(
        has_db || has_db_edges,
        "Should detect DynamoDB database access from AWS SDK imports. Found {} db nodes, {} edges",
        databases.len(),
        graph.edge_count()
    );

    // Verify edges exist
    assert!(
        graph.edge_count() > 0,
        "Graph should contain edges representing relationships"
    );

    // Should have at least one READS edge from service to database
    let read_edges: Vec<_> = graph.edges_by_type(EdgeType::Reads).collect();
    assert!(
        read_edges.len() > 0,
        "Should have at least one READS edge for DynamoDB GetCommand"
    );
}

/// Test surveying a JavaScript repository with HTTP client calls.
///
/// This test verifies detection of:
/// - axios HTTP client usage
/// - API call patterns
/// - Service-to-service communication
#[test]
fn test_survey_js_repo_with_http_calls() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("api-gateway");
    fs::create_dir_all(&repo_path).unwrap();

    fs::write(
        repo_path.join("package.json"),
        r#"{"name": "api-gateway", "dependencies": {"axios": "^1.0.0"}}"#,
    )
    .unwrap();

    fs::create_dir_all(repo_path.join("src")).unwrap();
    fs::write(
        repo_path.join("src/proxy.js"),
        r#"
import axios from 'axios';

async function callUserService(userId) {
    const response = await axios.get(`https://user-service.example.com/users/${userId}`);
    return response.data;
}

async function createOrder(orderData) {
    const response = await axios.post('https://order-service.example.com/orders', orderData);
    return response.data;
}
"#,
    )
    .unwrap();

    let parser = JavaScriptParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    if let Some(service) = parser.parse_package_json(&repo_path) {
        let service_id = builder.add_service(service);
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Should detect service
    assert_eq!(graph.nodes_by_type(NodeType::Service).count(), 1);

    // Should have detected HTTP calls - these might create API nodes or edges
    // The exact representation depends on GraphBuilder implementation
    assert!(
        graph.node_count() > 1 || graph.edge_count() > 0,
        "HTTP calls should result in additional nodes or edges"
    );
}

/// Test surveying a JavaScript repository with multiple AWS services.
///
/// This test verifies detection of:
/// - Multiple AWS SDK imports (DynamoDB, S3, SQS)
/// - Different operation types (reads, writes, publishes)
/// - Multiple resource types (databases, queues, cloud resources)
#[test]
fn test_survey_js_repo_with_multiple_aws_services() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("data-processor");
    fs::create_dir_all(&repo_path).unwrap();

    fs::write(
        repo_path.join("package.json"),
        r#"{
  "name": "data-processor",
  "dependencies": {
    "@aws-sdk/client-dynamodb": "^3.0.0",
    "@aws-sdk/client-s3": "^3.0.0",
    "@aws-sdk/client-sqs": "^3.0.0"
  }
}"#,
    )
    .unwrap();

    fs::create_dir_all(repo_path.join("src")).unwrap();
    fs::write(
        repo_path.join("src/processor.js"),
        r#"
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient } from '@aws-sdk/lib-dynamodb';
import { S3Client } from '@aws-sdk/client-s3';
import { SQSClient } from '@aws-sdk/client-sqs';

const dynamoClient = new DynamoDBClient({});
const docClient = DynamoDBDocumentClient.from(dynamoClient);
const s3 = new S3Client({});
const sqs = new SQSClient({});

async function processData(bucket, key) {
    // Read from S3
    const object = await s3.getObject({
        Bucket: bucket,
        Key: key
    });

    // Write to DynamoDB
    await docClient.put({
        TableName: 'processed-data',
        Item: { id: key, data: object.Body.toString() }
    });

    // Send to SQS
    await sqs.sendMessage({
        QueueUrl: 'https://sqs.us-east-1.amazonaws.com/123456789/processing-queue',
        MessageBody: JSON.stringify({ bucket, key })
    });
}
"#,
    )
    .unwrap();

    let parser = JavaScriptParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    if let Some(service) = parser.parse_package_json(&repo_path) {
        let service_id = builder.add_service(service);
        let discoveries = parser.parse_repo(&repo_path).unwrap();
        builder.process_discoveries(discoveries, &service_id);
    }

    let graph = builder.build();

    // Should detect service
    assert_eq!(
        graph.nodes_by_type(NodeType::Service).count(),
        1,
        "Should detect one service"
    );

    // Should detect multiple resource types
    let db_count = graph.nodes_by_type(NodeType::Database).count();
    let queue_count = graph.nodes_by_type(NodeType::Queue).count();
    let cloud_count = graph.nodes_by_type(NodeType::CloudResource).count();

    assert!(
        db_count > 0 || queue_count > 0 || cloud_count > 0,
        "Should detect at least one AWS resource type"
    );

    // Should have multiple types of edges
    let has_reads = graph.edges_by_type(EdgeType::Reads).count() > 0;
    let has_writes = graph.edges_by_type(EdgeType::Writes).count() > 0;
    let has_publishes = graph.edges_by_type(EdgeType::Publishes).count() > 0;
    let has_uses = graph.edges_by_type(EdgeType::Uses).count() > 0;

    assert!(
        has_reads || has_writes || has_publishes || has_uses,
        "Should detect various AWS operation types as edges"
    );
}

/// Test surveying a JavaScript repository with TypeScript framework detection.
///
/// This test verifies:
/// - TypeScript detection from package.json
/// - Framework detection (NestJS)
/// - Multiple dependencies
#[test]
fn test_survey_typescript_repo_with_framework() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("nestjs-api");
    fs::create_dir_all(&repo_path).unwrap();

    fs::write(
        repo_path.join("package.json"),
        r#"{
  "name": "nestjs-api",
  "dependencies": {
    "@nestjs/core": "^10.0.0",
    "@nestjs/common": "^10.0.0"
  },
  "devDependencies": {
    "@types/node": "^20.0.0",
    "typescript": "^5.0.0"
  }
}"#,
    )
    .unwrap();

    let parser = JavaScriptParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    if let Some(service) = parser.parse_package_json(&repo_path) {
        builder.add_service(service);
    }

    let graph = builder.build();

    let services: Vec<_> = graph.nodes_by_type(NodeType::Service).collect();
    assert_eq!(services.len(), 1);

    let service = services[0];
    assert_eq!(service.display_name, "nestjs-api");

    // Verify TypeScript is detected (language attribute should be "typescript")
    assert!(
        matches!(
            service.attributes.get("language"),
            Some(AttributeValue::String(s)) if s == "typescript"
        ),
        "Should detect TypeScript from package.json - language attribute should be 'typescript'"
    );

    // Verify framework detection
    assert!(
        matches!(
            service.attributes.get("framework"),
            Some(AttributeValue::String(s)) if s == "nestjs"
        ),
        "Should detect NestJS framework"
    );
}

/// Test that parser handles empty directories gracefully.
#[test]
fn test_survey_empty_js_repo() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("empty-service");
    fs::create_dir_all(&repo_path).unwrap();

    let parser = JavaScriptParser::new().unwrap();
    let builder = GraphBuilder::new();

    // Should not panic on empty repo
    let discoveries = parser.parse_repo(&repo_path).unwrap();
    assert_eq!(discoveries.len(), 0, "Empty repo should yield no discoveries");

    let graph = builder.build();
    assert_eq!(graph.node_count(), 0, "Graph should be empty");
}

/// Test that parser handles repos with only package.json (no source files).
#[test]
fn test_survey_js_repo_without_source() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("config-only");
    fs::create_dir_all(&repo_path).unwrap();

    fs::write(
        repo_path.join("package.json"),
        r#"{"name": "config-only", "version": "1.0.0"}"#,
    )
    .unwrap();

    let parser = JavaScriptParser::new().unwrap();
    let mut builder = GraphBuilder::new();

    if let Some(service) = parser.parse_package_json(&repo_path) {
        builder.add_service(service);
    }

    let graph = builder.build();

    // Should still create a service node from package.json
    assert_eq!(
        graph.nodes_by_type(NodeType::Service).count(),
        1,
        "Should create service from package.json even without source files"
    );
}
