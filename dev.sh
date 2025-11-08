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
    cd site && npm run build &>/dev/null && cd ../
    cargo build --features frontend
else
    cargo build --features frontend
fi

docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d --force-recreate nitro_repo
