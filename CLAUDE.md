# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`lbtree` is a Rust CLI tool that displays a tree representation of AWS resources.

Currently only Application Load Balancer (ALBv2) configurations are supported, showing the hierarchical structure of load balancers, listeners, rules, actions, target groups, and targets. However more resource trees are planned in the future.

## Development Commands

**Build:**
```bash
cargo build
```

**Run:**
```bash
# With load balancer ARN specified
cargo run -- --load-balancer-arn <ARN>

# Interactive mode (uses skim to select from available load balancers)
cargo run
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

## Architecture

### Core Design Pattern: Present Trait

The application uses a trait-based presentation pattern (`Present` trait in src/main.rs:19) that all AWS resource types implement. Each type provides:
- `content()`: String representation of the resource
- `indent()`: Hierarchical indentation level (0 for LB, 2 for listeners/TGs, 4 for rules/targets, 6 for actions)
- `present()`: Renders the resource to stdout with proper indentation

### Data Flow

1. **Authentication**: Uses AWS SDK default credential chain via `aws_config::load_from_env()`
2. **Load Balancer Selection**: Either CLI arg or interactive skim selection (src/main.rs:144)
3. **Parallel Fetching**: Two concurrent tasks fetch listener/rule hierarchy and target group/target hierarchy
4. **Display**: Results are collected as `Box<dyn Present>` and rendered in order

### AWS Resource Hierarchy

```
LoadBalancer (indent: 0)
├── Listener (indent: 2)
│   └── Rule (indent: 4)
│       └── Action (indent: 6)
└── TargetGroup (indent: 2)
    └── TargetHealthDescription (indent: 4)
```

### Parallelization Strategy

The tool spawns two tokio tasks to fetch data concurrently:
- **listeners_fut**: Fetches listeners → rules → actions for each listener
- **target_groups_fut**: Fetches target groups → target health for each group

Both tasks return `Vec<Box<dyn Present>>` which are then displayed sequentially.

## External Dependencies

None.
