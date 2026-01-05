#!/bin/bash
set -euo pipefail

# Only run in Claude Code web environment
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
    echo "Not in Claude Code web environment, skipping session setup"
    exit 0
fi

echo "Starting LocalStack..."
docker compose up --wait --wait-timeout 300
echo "LocalStack started"
