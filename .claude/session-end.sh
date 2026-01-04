#!/bin/bash

set -euo pipefail

# Only run in Claude Code web environment
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
    echo "Not in Claude Code web environment, skipping session setup"
    exit 0
fi

echo "Stopping LocalStack..."
localstack stop
echo "LocalStack stopped"
