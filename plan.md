# Multi-Resource Support for lbtree

## Overview
Expand lbtree from ALB-only to support multiple AWS resource types, starting with API Gateway REST APIs. Architecture will use subcommands and modular design for easy future expansion.

## User Interface

### CLI Structure (using clap subcommands)
```bash
lbtree                                    # Interactive menu to select resource type
lbtree alb                                 # Interactive ALB selection
lbtree alb --load-balancer-arn <ARN>      # Direct ALB display
lbtree apigateway                          # Interactive API Gateway selection
lbtree apigateway --api-id <ID>           # Direct API Gateway display
```

**Breaking Change:** Running `lbtree` with no args will now show a resource type menu instead of immediately showing ALB selection.

### API Gateway Hierarchy
```
RestApi (indent: 0)
└── Resource (indent: 2) - e.g., /users, /users/{id}
    └── Method (indent: 4) - e.g., GET, POST
        └── Integration (indent: 6) - e.g., Lambda, HTTP endpoint
```

## Architecture

### Module Structure
```
src/
├── main.rs          # CLI entry, resource type selection, routing to modules
├── present.rs       # Present trait definition (extracted from main.rs)
├── alb.rs           # All ALB-specific code (refactored from main.rs)
└── apigateway.rs    # New API Gateway implementation
```

### Present Trait (src/present.rs)
Extract trait definition from main.rs:34-43 into separate module for reuse across all resource types.

### ALB Module (src/alb.rs)
Move all existing ALB code from main.rs:
- 6 Present implementations (LoadBalancer, Listener, Rule, Action, TargetGroup, TargetHealthDescription)
- `select_lbs()` function (lines 144-224)
- LoadBalancerItem struct (lines 18-32)
- Main display logic (lines 231-344)

Create public API:
```rust
pub async fn display_alb(config: &SdkConfig, arn: Option<String>) -> eyre::Result<()>
```

### API Gateway Module (src/apigateway.rs)
New module implementing:
- 4 Present implementations:
  - **RestApi** (indent: 0): `"REST API \"{name}\" ({id})"`
  - **Resource** (indent: 2): `"{path} (id={id})"`
  - **Method** (indent: 4): `"{http_method} auth={authorization_type}"`
  - **Integration** (indent: 6): `"{type} uri={uri}"`
- Interactive REST API selection using skim
- Sequential fetching strategy

Public API:
```rust
pub async fn display_apigateway(config: &SdkConfig, api_id: Option<String>) -> eyre::Result<()>
```

AWS API calls needed:
1. `client.get_rest_apis()` - for interactive selection
2. `client.get_rest_api()` - if API ID provided
3. `client.get_resources()` - fetch all resources for API
4. `client.get_integration()` - for each resource/method combination

### Main Module (src/main.rs)
Restructured to:
1. Define CLI with subcommands:
   ```rust
   enum Commands {
       Alb { load_balancer_arn: Option<String> },
       ApiGateway { api_id: Option<String> },
   }
   ```
2. If no subcommand provided, show resource type selection menu using skim
3. Initialize AWS config once
4. Route to appropriate module based on selection/subcommand

## Implementation Steps

### Step 1: Extract Present Trait with Output Abstraction
- Create `src/present.rs`
- Define `OutputWriter` trait with `StdoutWriter` and `BufferWriter` implementations
- Move and update Present trait definition (lines 34-43 from main.rs)
- Update `present()` method to accept `&dyn OutputWriter` parameter
- Add `pub use present::{Present, OutputWriter, StdoutWriter, BufferWriter};` exports

### Step 2: Create ALB Module
- Create `src/alb.rs`
- Move ALB imports, Present implementations, select_lbs, LoadBalancerItem
- Update all Present implementations to use `OutputWriter`
- Create `display_alb()` function containing lines 231-344 from main.rs
- Accept `&SdkConfig` and `&dyn OutputWriter` parameters
- For production use, pass `&StdoutWriter`

### Step 3: Update Main with Subcommands
- Add clap Subcommand enum
- Implement resource type selection menu
- Route to `alb::display_alb()` with `StdoutWriter`
- Test: `cargo run alb` should work identically to old behavior

### Step 4: Add Dependencies
- Add to Cargo.toml dependencies: `aws-sdk-apigateway = "1.98.0"`
- Add to Cargo.toml dev-dependencies: `insta = "1.40"`, `uuid = "1.0"`

### Step 5: Create API Gateway Module
- Create `src/apigateway.rs`
- Implement 4 Present traits for API Gateway types
- Implement REST API selection with skim
- Implement `display_apigateway()` function
- Accept `&SdkConfig` and `&dyn OutputWriter` parameters
- Sequential fetch: API → Resources → Methods → Integrations

### Step 6: Wire API Gateway to Main
- Add ApiGateway subcommand routing
- Add "API Gateway REST API" to resource type menu
- Test: `cargo run apigateway` should display API Gateway trees

### Step 7: Create Test Infrastructure
- Create `tests/common/mod.rs` with:
  - `localstack_config()` helper
  - LocalStack availability check
  - Shared test utilities
- Add test fixtures with Drop-based cleanup

### Step 8: Write ALB Integration Tests
- Create `tests/alb_integration.rs`
- Implement `AlbTestFixture` struct with:
  - Resource creation (VPC, subnets, security groups, load balancer, target group, listener)
  - `run_display()` method that captures output using `BufferWriter`
  - Drop implementation for cleanup
- Write snapshot tests for ALB display
- Use random names with `uuid` for test hygiene

### Step 9: Write API Gateway Integration Tests
- Create `tests/apigateway_integration.rs`
- Implement `ApiGatewayTestFixture` struct with:
  - REST API creation with resources, methods, and integrations
  - `run_display()` method
  - Drop implementation for cleanup
- Write snapshot tests for API Gateway display
- Use random names with `uuid`

### Step 10: Update Documentation
- Update CLAUDE.md with new architecture, commands, and testing instructions
- Document LocalStack setup for contributors

## Critical Files

**New Files:**
- `src/present.rs` - Present trait + OutputWriter abstraction (~60 lines)
- `src/alb.rs` - Refactored ALB code (~250 lines)
- `src/apigateway.rs` - New API Gateway implementation (~200 lines)
- `tests/common/mod.rs` - Shared test utilities and LocalStack setup (~50 lines)
- `tests/alb_integration.rs` - ALB integration tests with LocalStack (~150 lines)
- `tests/apigateway_integration.rs` - API Gateway integration tests (~120 lines)

**Modified Files:**
- `src/main.rs` - CLI subcommands, resource type selection, routing (~150 lines)
- `Cargo.toml` - Add aws-sdk-apigateway, insta, uuid dependencies

## Future Extensibility

Adding new resource types (ECS, RDS, S3, etc.) requires:
1. Create `src/<resource>.rs`
2. Implement Present trait for resource types
3. Implement `display_<resource>()` function
4. Add subcommand variant to Commands enum
5. Add routing in main.rs
6. Add AWS SDK dependency to Cargo.toml
7. Add option to resource type selection menu

Estimated: 2-3 hours per resource type

## Testing Strategy

### Test Infrastructure

**LocalStack Integration:**
- Tests run against LocalStack (local AWS emulator) instead of real AWS
- Set up resources programmatically for each test
- Clean up resources in Drop implementation (ensures cleanup even on test failure)

**Output Abstraction:**
Instead of `println!` directly, abstract output to enable testing:

```rust
// src/present.rs
pub trait OutputWriter: Send + Sync {
    fn write_line(&self, content: &str);
}

pub struct StdoutWriter;
impl OutputWriter for StdoutWriter {
    fn write_line(&self, content: &str) {
        println!("{}", content);
    }
}

pub struct BufferWriter {
    buffer: Arc<Mutex<String>>,
}
impl OutputWriter for BufferWriter {
    fn write_line(&self, content: &str) {
        let mut buf = self.buffer.lock().unwrap();
        buf.push_str(content);
        buf.push('\n');
    }
}

// Update Present trait
trait Present: std::fmt::Debug + Send + Sync + 'static {
    fn content(&self) -> String;
    fn indent(&self) -> usize;

    fn present(&self, writer: &dyn OutputWriter) {
        let prefix = " ".repeat(self.indent()) + "-> ";
        writer.write_line(&format!("{}{}", prefix, self.content()));
    }
}
```

**Snapshot Testing with Insta:**
- Use `insta` crate for snapshot assertions
- Capture rendered output in tests and compare to saved snapshots
- Easy to review changes when intentional

**Test Structure:**
```rust
// tests/alb_integration.rs
#[tokio::test]
async fn test_alb_display() {
    let fixture = AlbTestFixture::new().await; // Sets up LocalStack resources
    let output = fixture.run_display().await;
    insta::assert_snapshot!(output);
} // Drop cleans up resources automatically
```

### Test Dependencies

Add to `Cargo.toml`:
```toml
[dev-dependencies]
insta = "1.40"
uuid = "1.0"  # For random resource names
```

### Resource Cleanup Pattern

Use Drop trait to ensure cleanup:
```rust
struct AlbTestFixture {
    client: aws_sdk_elasticloadbalancingv2::Client,
    lb_arn: String,
    // other resource IDs
}

impl Drop for AlbTestFixture {
    fn drop(&mut self) {
        // Synchronous cleanup in Drop
        // Use tokio::runtime::Handle::current().block_on() if needed
        // Delete load balancer, target groups, etc.
    }
}
```

### Random Test Data

Use `uuid` crate for unique resource names:
```rust
let lb_name = format!("test-lb-{}", uuid::Uuid::new_v4());
```

Prevents conflicts when running tests in parallel or against shared LocalStack.

### Test Coverage

**ALB Module Tests:**
1. Display load balancer with listeners and rules
2. Display target groups with targets
3. Handle missing optional fields gracefully

**API Gateway Module Tests:**
1. Display REST API with resources and methods
2. Display integrations (Lambda, HTTP)
3. Handle nested resource paths

**Integration Tests Location:**
- `tests/alb_integration.rs`
- `tests/apigateway_integration.rs`
- Shared LocalStack setup utilities in `tests/common/mod.rs`

### LocalStack Configuration

Tests assume LocalStack is already running and will:
1. Check if LocalStack is available at `http://localhost:4566` (skip tests if not)
2. Use endpoint override: `http://localhost:4566`
3. Use dummy AWS credentials (LocalStack doesn't validate them)

```rust
// tests/common/mod.rs
async fn localstack_config() -> SdkConfig {
    aws_config::from_env()
        .endpoint_url("http://localhost:4566")
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .load()
        .await
}

async fn is_localstack_available() -> bool {
    // Try a simple AWS call to check if LocalStack is responding
    let config = localstack_config().await;
    let client = aws_sdk_elasticloadbalancingv2::Client::new(&config);
    client.describe_load_balancers().send().await.is_ok()
}
```

### Running Tests

```bash
# Start LocalStack first (one-time setup, keep running)
docker run --rm -d -p 4566:4566 localstack/localstack

# Run tests (assumes LocalStack is already running)
cargo test

# Update snapshots when changes are intentional
cargo insta review

# Stop LocalStack when done
docker stop $(docker ps -q --filter ancestor=localstack/localstack)
```

## Design Principles

- **Present trait remains universal** - All resources use same display mechanism
- **Module isolation** - Each resource type is self-contained
- **No abstraction over-engineering** - Skim selection logic duplicated per resource (different display formats)
- **Parallel fetching where beneficial** - ALB uses it, API Gateway may not need it
- **Type safety** - Each module uses its own AWS SDK client type
- **Testable output** - OutputWriter abstraction enables snapshot testing
