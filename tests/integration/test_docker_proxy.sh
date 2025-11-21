#!/bin/bash
# Docker proxy integration tests
# Validates pull-through caching, package listings, and cache deletion/re-download

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

wait_for_server 60

print_section "Docker Proxy Integration Tests"

WORKSPACE=$(create_workspace "docker-proxy")
REPO_NAME="${DOCKER_PROXY_REPOSITORY:-docker-proxy}"
REPO_ID=""
STORAGE_NAME=""
DOCKER_REGISTRY_HOST="${NITRO_DOCKER_HOST:-${NITRO_URL#http://}}"
UPSTREAM_IMAGE="library/alpine"
CACHE_PATH=""
PROXY_IMAGE=""

cleanup() {
    if [[ -n "$PROXY_IMAGE" ]]; then
        docker rmi "${PROXY_IMAGE}" >/dev/null 2>&1 || true
    fi
    cleanup_workspace "$WORKSPACE"
}
trap cleanup EXIT

print_test "Locate seeded Docker proxy repository"
if repos=$(api_get "/api/repository/list"); then
    REPO_ENTRY=$(jq -r --arg name "$REPO_NAME" '[.[] | select(.name == $name)][0]' <<<"$repos")
    if [[ "$REPO_ENTRY" == "null" || -z "$REPO_ENTRY" ]]; then
        fail "Seeded repository '$REPO_NAME' not found. Ensure tests/docker/seed-data.sql contains it."
        exit 1
    else
        REPO_ID=$(jq -r '.id' <<<"$REPO_ENTRY")
        STORAGE_NAME=$(jq -r '.storage_name' <<<"$REPO_ENTRY")
        REPO_PATH="${STORAGE_NAME}/${REPO_NAME}"
        PROXY_IMAGE="${DOCKER_REGISTRY_HOST}/${REPO_PATH}/${UPSTREAM_IMAGE}:latest"
        pass
    fi
else
    fail "Unable to query repository list"
    exit 1
fi

print_test "Docker login to Nitro Repo"
if run_cmd bash -lc "printf '%s' \"${TEST_PASSWORD}\" | docker login \"${DOCKER_REGISTRY_HOST}\" --username \"${TEST_USER}\" --password-stdin"; then
    pass
else
    fail "Docker login failed"
fi

print_test "Pull upstream image via proxy"
if run_cmd docker pull "${PROXY_IMAGE}"; then
    pass
else
    fail "Failed to pull image through proxy"
fi

print_test "Packages API lists cached manifest"
for attempt in {1..5}; do
    if packages_json=$(api_get "/api/repository/${REPO_ID}/packages?page=1&per_page=5"); then
        CACHE_PATH=$(jq -r '.items[0].cache_path // empty' <<<"$packages_json")
        if [[ -n "$CACHE_PATH" ]]; then
            pass
            break
        fi
    fi
    sleep 2
done
if [[ -z "$CACHE_PATH" ]]; then
    fail "Packages API did not surface cached manifest after pull"
fi

print_test "Delete cached manifest via API"
delete_payload=$(jq -n --arg path "$CACHE_PATH" '{paths: [$path]}')
if delete_response=$(api_delete "/api/repository/${REPO_ID}/packages" \
    -H "Content-Type: application/json" \
    -d "$delete_payload"); then
    deleted_count=$(jq -r '.deleted' <<<"$delete_response")
    if [[ "$deleted_count" -ge 1 ]]; then
        pass
    else
        fail "Deletion API responded but reported zero deletions"
    fi
else
    fail "Failed to delete cached manifest"
fi

print_test "Re-pull re-downloads manifest after deletion"
docker rmi "${PROXY_IMAGE}" >/dev/null 2>&1 || true
if run_cmd docker pull "${PROXY_IMAGE}"; then
    pass
else
    fail "Failed to re-download image via proxy"
fi

print_summary
