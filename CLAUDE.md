# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`lbtree` is a Rust CLI tool that displays tree representations of AWS resources.

**Supported Resource Types:**
- **Application Load Balancer (ALBv2)**: Shows hierarchical structure of load balancers, listeners, rules, actions, target groups, and targets
- **API Gateway REST APIs**: Shows hierarchical structure of REST APIs, resources, methods, and integrations

The tool is designed for easy extension to support additional AWS resource types (ECS, RDS, S3, etc.).

## Development Commands

**Build:**
```bash
cargo build
```

**Run:**
```bash
# Interactive mode - select resource type and then specific resource
cargo run

# ALB subcommand - interactive load balancer selection
cargo run -- alb

# ALB with specific ARN
cargo run -- alb --load-balancer-arn <ARN>

# API Gateway subcommand - interactive API selection
cargo run -- apigateway

# API Gateway with specific ID
cargo run -- apigateway --api-id <API_ID>
```

**Check/Lint:**
```bash
cargo check
cargo clippy
```

**Format:**
```bash
cargo fmt
```

**Test:**
```bash
# Requires LocalStack running at http://localhost:4566
docker run --rm -d -p 4566:4566 localstack/localstack

# Run all tests
cargo test

# Run specific test module
cargo test alb_integration
cargo test apigateway_integration

# Update snapshot tests when changes are intentional
cargo insta review
```

## Architecture

### Module Structure

```
src/
├── main.rs          # CLI entry point, resource type selection, routing
├── lib.rs           # Library interface exposing modules for testing
├── present.rs       # Present trait and OutputWriter abstraction
├── alb.rs           # Application Load Balancer implementation
└── apigateway.rs    # API Gateway REST API implementation

tests/
├── common/
│   └── mod.rs       # Shared test utilities and LocalStack configuration
├── alb_integration.rs         # ALB integration tests
└── apigateway_integration.rs  # API Gateway integration tests
```

### Core Design Pattern: Present Trait

The application uses a trait-based presentation pattern (`Present` trait in `src/present.rs`) that all AWS resource types implement. Each type provides:
- `content()`: String representation of the resource
- `indent()`: Hierarchical indentation level
- `present(writer: &dyn OutputWriter)`: Renders the resource using the provided output writer

### OutputWriter Abstraction

The `OutputWriter` trait enables testable output:
- **StdoutWriter**: Production use, writes to stdout via `println!`
- **BufferWriter**: Test use, captures output to a string buffer for assertions

This pattern allows integration tests to capture and verify rendered output without writing to stdout.

### CLI Structure

Uses clap subcommands for clean resource type selection:

```rust
Commands {
    Alb { load_balancer_arn: Option<String> },
    ApiGateway { api_id: Option<String> },
}
```

When no subcommand is provided, an interactive menu (using skim) allows selecting the resource type.

### Data Flow

1. **Authentication**: Uses AWS SDK default credential chain via `aws_config::load_from_env()`
2. **Resource Type Selection**: Subcommand or interactive menu
3. **Resource Selection**: Interactive skim selection or direct identifier (ARN/ID)
4. **Fetching**: Resource-specific fetching strategy (parallel for ALB, sequential for API Gateway)
5. **Display**: Results collected as `Vec<Box<dyn Present>>` and rendered with proper indentation

### AWS Resource Hierarchies

**Application Load Balancer:**
```
LoadBalancer (indent: 0)
├── Listener (indent: 2)
│   └── Rule (indent: 4)
│       └── Action (indent: 6)
└── TargetGroup (indent: 2)
    └── TargetHealthDescription (indent: 4)
```

**API Gateway REST API:**
```
RestApi (indent: 0)
└── Resource (indent: 2) - e.g., /users, /users/{id}
    └── Method (indent: 4) - e.g., GET, POST
        └── Integration (indent: 6) - e.g., Lambda, HTTP endpoint
```

### ALB Fetching Strategy

The ALB module spawns two tokio tasks to fetch data concurrently:
- **listeners_fut**: Fetches listeners → rules → actions for each listener
- **target_groups_fut**: Fetches target groups → target health for each group

Both tasks return `Vec<Box<dyn Present>>` which are then displayed sequentially.

### API Gateway Fetching Strategy

The API Gateway module uses sequential fetching:
1. Fetch REST API metadata
2. Fetch all resources (single paginated call)
3. For each resource with methods, fetch integrations

This is because API Gateway's API structure doesn't benefit from parallelization (resources and methods are often returned together).

## Testing

### Test Infrastructure

**LocalStack Integration:**
- Tests run against LocalStack (local AWS emulator) instead of real AWS
- Start LocalStack before running tests: `docker run --rm -d -p 4566:4566 localstack/localstack`
- Tests automatically skip if LocalStack is not available at `http://localhost:4566`

**Test Fixtures:**
- **AlbTestFixture**: Creates VPC, subnets, security groups, load balancer, target groups, listeners
- **ApiGatewayTestFixture**: Creates REST API with resources, methods, and integrations

Both fixtures implement Drop for automatic cleanup, ensuring resources are deleted even if tests fail.

**Snapshot Testing:**
- Uses `insta` crate for snapshot assertions
- Captures rendered output and compares to saved snapshots
- Review changes: `cargo insta review`

**Test Hygiene:**
- Random resource names using `uuid::Uuid::new_v4()` prevent conflicts
- Safe for parallel test execution

### Running Tests

```bash
# Start LocalStack (one-time, keep running)
docker run --rm -d -p 4566:4566 localstack/localstack

# Run all tests
cargo test

# Run specific test
cargo test test_alb_display

# Update snapshots after intentional changes
cargo insta review

# Stop LocalStack when done
docker stop $(docker ps -q --filter ancestor=localstack/localstack)
```

### Writing Tests for New Resource Types

When adding a new resource type:

1. Create test fixture struct with setup() and cleanup() methods
2. Implement Drop for automatic cleanup
3. Use `skip_if_localstack_unavailable!()` macro at test start
4. Use `BufferWriter` to capture output
5. Write both assertion-based and snapshot tests

Example:
```rust
#[tokio::test]
async fn test_new_resource_display() {
    skip_if_localstack_unavailable!();

    let fixture = NewResourceTestFixture::new().await.unwrap();
    let output = fixture.run_display().await.unwrap();

    assert!(output.contains("expected content"));
    insta::assert_snapshot!(output);
}
```

## Adding New Resource Types

To add support for a new AWS resource type (e.g., ECS):

1. **Create module**: `src/ecs.rs`
   - Implement `Present` trait for all resource types in the hierarchy
   - Implement interactive selection function using skim
   - Implement `pub async fn display_ecs(config: &SdkConfig, id: Option<String>, writer: &dyn OutputWriter)`

2. **Update main.rs**:
   - Add `mod ecs;`
   - Add variant to `Commands` enum: `Ecs { cluster_arn: Option<String> }`
   - Add variant to `ResourceType` enum
   - Add resource type to selection menu
   - Add routing in match statement

3. **Update Cargo.toml**:
   - Add AWS SDK dependency: `aws-sdk-ecs = "1"`

4. **Write tests**:
   - Create `tests/ecs_integration.rs`
   - Implement test fixture with Drop cleanup
   - Write both assertion and snapshot tests

5. **Update documentation**:
   - Add resource hierarchy diagram to this file
   - Update project overview with supported resource type

Estimated effort: 2-3 hours per resource type.

## External Dependencies

None for production use. Tests require Docker for LocalStack.
