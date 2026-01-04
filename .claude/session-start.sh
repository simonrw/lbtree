#!/bin/bash
set -euo pipefail

# Only run in Claude Code web environment
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
    echo "Not in Claude Code web environment, skipping session setup"
    exit 0
fi

echo "Installing LocalStack CLI..."
pip install --user localstack

echo "Starting LocalStack..."
localstack start -d

echo "Waiting for LocalStack to be ready..."
localstack wait -t 30

echo "LocalStack is ready!"
