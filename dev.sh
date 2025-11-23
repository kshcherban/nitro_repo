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

# Build backend based on OS
case "$(uname -s)" in
  Linux)
    export NITRO_BINARY_PATH="./target/debug/nitro_repo"
    cargo build --features frontend
    ;;
  Darwin)
    # Install zig and cargo-zigbuild if not already installed
    # brew install zig
    # cargo install cargo-zigbuild
    # rustup target add aarch64-unknown-linux-gnu
    export NITRO_BINARY_PATH="./target/aarch64-unknown-linux-gnu/debug/nitro_repo"
    ulimit -n 65536
    cargo zigbuild --target aarch64-unknown-linux-gnu --features frontend
    ;;
esac

docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d --force-recreate nitro
