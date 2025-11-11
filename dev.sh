#!/bin/bash

# Parse command line arguments
SKIP_FRONTEND=false
while getopts "b" opt; do
  case $opt in
    b) SKIP_FRONTEND=true ;;
    \?) echo "Invalid option: -$OPTARG" >&2; exit 1 ;;
  esac
done

set -ex

# Build frontend unless -b flag is provided
if [[ "$SKIP_FRONTEND" = false ]]; then
  export VITE_BASE_URL=/
  npm --prefix site run build-only -- --mode development --minify false --sourcemap true
fi

cargo build --features frontend

docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d --force-recreate nitro_repo
