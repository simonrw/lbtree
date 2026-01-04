#!/bin/bash
set -e

echo "Installing LocalStack CLI..."
pip install --user localstack

echo "Starting LocalStack..."
localstack start -d

echo "Waiting for LocalStack to be ready..."
localstack wait -t 30

echo "LocalStack is ready!"
