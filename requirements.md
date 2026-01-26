# AWS Cloud Mapping Service - Requirements Document

**Version:** 1.0
**Date:** January 25, 2026
**Status:** Final
**Reference:** cloud-map-design-doc.md v1.0

---

## System Purpose

Automate discovery and consolidation of AWS resources across 16+ client accounts to provide a unified blueprint for LLM agent consumption. The system must capture resource configurations, track changes over time, and identify deployment methods (Terraform, SAM, CloudFormation).

---

## Functional Requirements

### FR-1: Multi-Account Resource Discovery
- Scan 16+ AWS client accounts in parallel
- Discover resources across 13 service categories
- Execute nightly at midnight UTC via EventBridge scheduled rule
- Complete full scan within 5 minutes
- Use cross-account IAM role assumption (roles already exist)
- Scan us-east-1 region only

### FR-2: Service Coverage
Must discover and capture metadata for all resources in these categories:

**Compute:**
- Lambda functions (50+)
- ECS Fargate clusters, services, tasks
- EC2 instances

**Database:**
- DynamoDB tables (20+)
- RDS instances and clusters

**Storage:**
- S3 buckets

**Messaging:**
- SQS queues
- SNS topics
- EventBridge rules

**Security:**
- Secrets Manager secrets
- Cognito user pools
- IAM roles and policies

**Logging:**
- CloudWatch log groups and metrics

**API:**
- API Gateway (REST and HTTP)
- Lambda Function URLs

**AI Services:**
- Bedrock models/knowledge bases
- Lex bots
- Amazon Connect instances
- Translate resources

**Email:**
- SES identities and configuration

**Containers:**
- ECR repositories

**Networking:**
- VPCs
- Subnets
- Security Groups
- NAT Gateways

### FR-3: Resource Metadata Capture
For each resource, capture:
- Resource name
- ARN
- Resource type
- Service-specific configuration (schemas, settings, parameters)
- AWS tags (all tags)
- Deployment metadata:
  - Deployment method (Terraform, SAM, CloudFormation, or unknown)
  - Stack name (if applicable)
  - Terraform workspace (if applicable)
  - Related deployment tags

**Important:** Capture metadata only, not data:
- DynamoDB: schema, keys, indexes (NOT table contents)
- Secrets Manager: secret names (NOT secret values)
- Lambda: env var keys (NOT values)
- IAM: role names and policies (NOT credentials)

### FR-4: Cross-Account Consolidation
- Consolidate resources by name matching across all accounts
- Generate per-service blueprint files showing:
  - Unique resource names
  - Which accounts contain each resource
  - Common configuration/schema
  - Configuration variations (if any)
  - Deployment method breakdown
- Identify universal resources (exist in all accounts with same config)
- Identify account-specific resources (exist in subset of accounts)

### FR-5: Change Tracking
- Compare current scan with previous scan
- For each service, identify:
  - Added resources (new since last scan)
  - Removed resources (deleted since last scan)
  - Modified resources (config changed since last scan)
  - Unchanged resources
- Store detailed diff files per service
- Include change summary in manifest

### FR-6: Output Generation
Store outputs in S3 bucket with structure:
```
s3://cloud-map-{account-id}/
  raw-scans/{account_id}/{timestamp}/{service}.json
  blueprints/{service}.json
  diffs/{timestamp}/{service}.json
  manifests/latest.json
  manifests/history/{timestamp}.json
```

**Blueprint Format:**
- One JSON file per service (e.g., `lambda.json`, `dynamodb.json`)
- Contains consolidated view of all resources of that type
- Groups resources by name with account mappings
- Shows deployment method and config variations

**Manifest Format:**
- JSON file with scan metadata
- Lists all accounts scanned
- Resource counts per service
- Deployment method breakdown per service
- Change summary (added/removed/modified counts)
- Links to blueprint and diff S3 paths
- Scan duration and timestamp

**Diff Format:**
- One JSON file per service
- Lists added, removed, modified resources
- Shows before/after configs for modifications

### FR-7: Error Handling & Validation
- Continue scanning other accounts if one account fails
- Log all errors with structured JSON logging
- Include error details in manifest
- Validate resource counts against previous scan
- Alert via SNS if anomalies detected (>20% change)

### FR-8: Monitoring & Alerting
- Log to CloudWatch with structured JSON entries
- Publish custom CloudWatch metrics:
  - AccountScanDuration (per account)
  - TotalResourcesDiscovered
  - AccountScanErrors
- SNS alerts for:
  - Lambda function errors
  - Lambda duration >4.5 minutes
  - Account scan failures
  - Resource count anomalies

---

## Non-Functional Requirements

### NFR-1: Performance
- Complete full scan of 16+ accounts in <5 minutes
- Parallel account scanning (ThreadPoolExecutor)
- Parallel service scanning within each account

### NFR-2: Cost
- Target: <$0.01 per scan per account
- Total budget: <$50/month for 16+ accounts

### NFR-3: Security
- Read-only IAM permissions only
- Cross-account role assumption with temporary credentials
- S3 SSE-KMS encryption at rest
- No storage of sensitive data (secrets, credentials, PII)
- CloudTrail audit logging enabled

### NFR-4: Reliability
- Idempotent operations (safe to re-run)
- Retry logic for transient AWS API failures
- Graceful degradation (continue if one service/account fails)

### NFR-5: Maintainability
- Infrastructure as Code (AWS CDK or Terraform)
- Consistent scanner pattern across all services
- Structured logging (JSON format)
- Unit tests for all scanners
- Integration tests

---

## Technical Constraints

### TC-1: Technology Stack
- Runtime: Python 3.12
- SDK: Boto3 (pure AWS SDK, no CloudQuery)
- IaC: AWS CDK (Python)
- Deployment: AWS Lambda (compute)
- Storage: S3 (outputs), SSM Parameter Store (config)
- Scheduling: EventBridge scheduled rules
- Monitoring: CloudWatch Logs and Metrics
- Alerting: SNS

### TC-2: AWS Resources
- Lambda: 1024MB memory, 300 second timeout
- S3: Single bucket in central account
- IAM: Cross-account roles already exist in all client accounts
- Region: us-east-1 only

### TC-3: Configuration
- Account list: Stored in SSM Parameter Store `/cloud-mapper/accounts`
- Role name: Stored in SSM Parameter Store `/cloud-mapper/role-name`
- Regions: Stored in SSM Parameter Store `/cloud-mapper/regions`

---

## Out of Scope

### Explicitly NOT Included:
- Resource creation, modification, or deletion (read-only system)
- Multi-region scanning (us-east-1 only for MVP)
- Real-time/event-driven incremental updates (nightly scans only)
- Monorepo integration (no git commits)
- Human-facing UI/dashboard
- Query API or MCP server (just S3 outputs)
- IaC parsing (Terraform/SAM file analysis)
- Cost analysis or optimization recommendations
- Security scanning or vulnerability detection
- Compliance checking

---

## Deployment Method Detection Rules

The system identifies deployment methods by examining AWS resource tags:

### Terraform
- Tags contain: `terraform:*` (any tag starting with "terraform:")
- Extract workspace from tags if available

### AWS SAM
- Tags contain: `aws:sam:*` or `aws:cloudformation:stack-name` with SAM-related stack
- Extract stack name

### CloudFormation
- Tags contain: `aws:cloudformation:stack-name`
- Extract stack name

### Unknown
- No deployment-related tags found
- Potentially manually created or deployed via other methods

---

## Success Criteria

The system is considered successful when:
1. All 16+ accounts scanned successfully every night
2. All 13 service categories captured in blueprints
3. Blueprints show consolidated view with account mappings
4. Deployment methods identified for resources
5. Diffs accurately track changes between scans
6. Scan completes in <5 minutes
7. Zero data loss (all resources discovered)
8. Manifest provides complete inventory summary
9. Errors logged and alerted appropriately
10. Cost remains <$50/month

---

## Dependencies

### Required Access:
- Central AWS account with admin access
- List of all 16+ client AWS account IDs
- Cross-account IAM role ARNs (already exist)
- S3 bucket creation permissions in central account

### Required Permissions (per client account role):
- Compute: `lambda:List*`, `lambda:Get*`, `ecs:Describe*`, `ec2:Describe*`
- Database: `dynamodb:Describe*`, `dynamodb:List*`, `rds:Describe*`
- Storage: `s3:ListAllMyBuckets`, `s3:GetBucket*`
- Messaging: `sqs:List*`, `sqs:GetQueueAttributes`, `sns:List*`, `sns:GetTopicAttributes`, `events:List*`, `events:Describe*`
- Security: `secretsmanager:List*`, `secretsmanager:Describe*`, `cognito-idp:List*`, `cognito-idp:Describe*`, `iam:List*`, `iam:Get*`
- Logging: `logs:Describe*`, `cloudwatch:Describe*`, `cloudwatch:List*`
- API: `apigateway:GET`, `execute-api:GET`
- AI: `bedrock:List*`, `lex:Describe*`, `lex:List*`, `connect:List*`, `connect:Describe*`, `translate:List*`
- Email: `ses:List*`, `ses:GetAccount*`, `sesv2:List*`, `sesv2:Get*`
- Containers: `ecr:Describe*`, `ecr:List*`
- Networking: `ec2:DescribeVpcs`, `ec2:DescribeSubnets`, `ec2:DescribeSecurityGroups`, `ec2:DescribeNatGateways`

---

## Glossary

- **Blueprint**: Consolidated view of a specific service type across all accounts
- **Universal Resource**: Resource that exists in all accounts with identical configuration
- **Account-Specific Resource**: Resource that exists in only a subset of accounts
- **Deployment Method**: How the resource was created (Terraform, SAM, CloudFormation, or unknown)
- **Diff**: Comparison between two scans showing added/removed/modified resources
- **Manifest**: Summary file containing scan metadata, resource counts, and S3 paths
- **Raw Scan**: Per-account, per-service resource data before consolidation
