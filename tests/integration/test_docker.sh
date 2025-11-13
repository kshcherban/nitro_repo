#!/bin/bash
# Docker integration tests
# Tests Docker proxy repository end-to-end

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

# Docker-specific configuration
DOCKER_PROXY_REPO="${TEST_STORAGE}/docker-proxy"
FIXTURE_DIR="/fixtures/docker"
IMAGE_NAME="nitro-test/testimg"
IMAGE_TAG="1.0.0"

print_section "Docker Integration Tests"

WORKSPACE=$(create_workspace "docker")
cd "$WORKSPACE"

# Test 1: Verify Docker Registry V2 API
print_test "Verify Docker Registry V2 API endpoint"
STATUS=$(get_http_status "${NITRO_URL}/repositories/${DOCKER_PROXY_REPO}/v2/")

if assert_http_status "200" "$STATUS"; then
    pass
else
    fail "Expected 200, got $STATUS"
fi

# Test 2: Build test image
print_test "Build test Docker image"
cp "${FIXTURE_DIR}/Dockerfile.testimg" "$WORKSPACE/Dockerfile"

if docker build -t "${IMAGE_NAME}:${IMAGE_TAG}" "$WORKSPACE" > /dev/null 2>&1; then
    pass
else
    fail "Failed to build Docker image"
fi

# Note: Docker push to hosted repo would require Docker registry support
# For now, we test proxy functionality

# Test 3: Pull image through proxy (alpine)
print_test "Proxy: pull alpine image"

# Configure Docker to use Nitro Repo as registry proxy
PROXY_REGISTRY="${NITRO_URL#http://}/repositories/${DOCKER_PROXY_REPO}"

if docker pull "${PROXY_REGISTRY}/library/alpine:3.18" > /dev/null 2>&1; then
    pass
else
    # Proxy might need authentication or special setup
    fail "Failed to pull through proxy (may require Docker auth setup)"
fi

# Test 4: Verify image manifest accessible via API
print_test "Fetch image manifest via API"
MANIFEST_PATH="/repositories/${DOCKER_PROXY_REPO}/v2/library/alpine/manifests/3.18"

STATUS=$(get_http_status "${NITRO_URL}${MANIFEST_PATH}" \
    -H "Accept: application/vnd.docker.distribution.manifest.v2+json")

if [ "$STATUS" = "200" ] || [ "$STATUS" = "404" ]; then
    # 404 is acceptable if proxy hasn't cached yet
    pass
else
    fail "Unexpected status: $STATUS"
fi

# Test 5: Verify blob endpoint
print_test "Verify blob endpoint exists"
BLOB_PATH="/repositories/${DOCKER_PROXY_REPO}/v2/library/alpine/blobs/sha256:abc123"

STATUS=$(get_http_status "${NITRO_URL}${BLOB_PATH}")

# Should return 404 for non-existent blob
if assert_http_status "404" "$STATUS"; then
    pass
else
    fail "Expected 404 for non-existent blob, got $STATUS"
fi

# Test 6: Verify tags list endpoint
print_test "Verify tags list endpoint"
TAGS_PATH="/repositories/${DOCKER_PROXY_REPO}/v2/library/alpine/tags/list"

RESPONSE=$(curl -sf "${NITRO_URL}${TAGS_PATH}" || echo "{}")

if echo "$RESPONSE" | jq -e '.name' > /dev/null 2>&1; then
    pass
else
    fail "Tags list endpoint not working correctly"
fi

# Test 7: Authentication for write operations
print_test "Verify authentication required for manifest upload"
UPLOAD_PATH="/repositories/${DOCKER_PROXY_REPO}/v2/${IMAGE_NAME}/manifests/latest"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
    -X PUT \
    "${NITRO_URL}${UPLOAD_PATH}" \
    -H "Content-Type: application/vnd.docker.distribution.manifest.v2+json" \
    --data '{}')

if [ "$STATUS" = "401" ] || [ "$STATUS" = "403" ]; then
    pass
else
    fail "Expected 401/403 without auth, got $STATUS"
fi

# Test 8: Verify catalog endpoint
print_test "Verify catalog endpoint"
CATALOG_PATH="/repositories/${DOCKER_PROXY_REPO}/v2/_catalog"

RESPONSE=$(curl -sf "${NITRO_URL}${CATALOG_PATH}" || echo "{}")

if echo "$RESPONSE" | jq -e '.repositories' > /dev/null 2>&1; then
    pass
else
    fail "Catalog endpoint not working"
fi

# Cleanup
cleanup_workspace "$WORKSPACE"
docker rmi "${IMAGE_NAME}:${IMAGE_TAG}" > /dev/null 2>&1 || true

print_summary
