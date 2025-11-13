#!/bin/bash
# Docker integration tests
# Tests Docker proxy repository end-to-end

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

# Docker-specific configuration
DOCKER_REPO_PATH="${TEST_STORAGE}/docker-proxy"
FIXTURE_DIR="/fixtures/docker"
IMAGE_NAME="nitro-test/testimg"
IMAGE_TAG="1.0.0"

print_section "Docker Integration Tests"

WORKSPACE=$(create_workspace "docker")
cd "$WORKSPACE"

# Test 1: Build test image
print_test "Build test Docker image"
cp "${FIXTURE_DIR}/Dockerfile.testimg" "$WORKSPACE/Dockerfile"

if run_cmd docker build -t "${IMAGE_NAME}:${IMAGE_TAG}" "$WORKSPACE"; then
    pass
else
    fail "Failed to build Docker image"
fi

# Test 2: Docker login (basic auth with admin credentials)
print_test "Docker login to Nitro Repo"
DOCKER_REGISTRY_HOST="${NITRO_DOCKER_HOST:-${NITRO_URL#http://}}"
if run_cmd docker login "${DOCKER_REGISTRY_HOST}" -u "${TEST_USER}" -p "${TEST_PASSWORD}"; then
    pass
else
    fail "Docker login failed"
fi

# Test 3: Push image to hosted repository
print_test "Push Docker image to Nitro Repo"
REMOTE_IMAGE="${DOCKER_REGISTRY_HOST}/${DOCKER_REPO_PATH}/${IMAGE_NAME}"
run_cmd docker tag "${IMAGE_NAME}:${IMAGE_TAG}" "${REMOTE_IMAGE}:${IMAGE_TAG}"
if run_cmd docker push "${REMOTE_IMAGE}:${IMAGE_TAG}"; then
    pass
else
    fail "Failed to push Docker image"
fi

# Test 4: Pull image back from hosted repository
print_test "Pull Docker image from Nitro Repo"
run_cmd docker rmi "${IMAGE_NAME}:${IMAGE_TAG}" || true
if run_cmd docker pull "${REMOTE_IMAGE}:${IMAGE_TAG}"; then
    pass
else
    fail "Failed to pull Docker image"
fi

# Test 5: Verify image manifest accessible via API
print_test "Fetch image manifest via API"
MANIFEST_PATH="/repositories/${DOCKER_REPO_PATH}/v2/${IMAGE_NAME}/manifests/${IMAGE_TAG}"
STATUS=$(get_http_status "${NITRO_URL}${MANIFEST_PATH}" \
    -H "Accept: application/vnd.docker.distribution.manifest.v2+json")
if [ "$STATUS" = "200" ]; then
    pass
else
    fail "Unexpected status: $STATUS"
fi

# Test 6: Verify blob endpoint returns 404 for unknown digest
print_test "Verify blob endpoint returns 404 for unknown digest"
BLOB_PATH="/repositories/${DOCKER_REPO_PATH}/v2/${IMAGE_NAME}/blobs/sha256:abc123"
STATUS=$(get_http_status "${NITRO_URL}${BLOB_PATH}")
if assert_http_status "404" "$STATUS"; then
    pass
else
    fail "Expected 404 for non-existent blob, got $STATUS"
fi

# Test 7: Verify tags list endpoint
print_test "Verify tags list endpoint"
TAGS_PATH="/repositories/${DOCKER_REPO_PATH}/v2/${IMAGE_NAME}/tags/list"
RESPONSE=$(curl -sf "${NITRO_URL}${TAGS_PATH}" || echo "{}")
record_output "$RESPONSE"
set +e
jq -e '.name and (.tags | index("'"${IMAGE_TAG}"'"))' <<<"$RESPONSE" > /dev/null 2>&1
tags_status=$?
set -e
if [ "$tags_status" -eq 0 ]; then
    clear_last_log
    pass
else
    fail "Tags list endpoint not working correctly"
fi

# Test 8: Authentication required for manifest upload
print_test "Verify authentication required for manifest upload"
UPLOAD_PATH="/repositories/${DOCKER_REPO_PATH}/v2/${IMAGE_NAME}/manifests/latest"
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

# Test 9: Verify catalog endpoint lists repository
print_test "Verify catalog endpoint"
CATALOG_PATH="/repositories/${DOCKER_REPO_PATH}/v2/_catalog"
RESPONSE=$(curl -sf "${NITRO_URL}${CATALOG_PATH}" || echo "{}")
record_output "$RESPONSE"
set +e
jq -e ".repositories | index(\"${IMAGE_NAME}\")" <<<"$RESPONSE" > /dev/null 2>&1
catalog_status=$?
set -e
if [ "$catalog_status" -eq 0 ]; then
    clear_last_log
    pass
else
    fail "Catalog endpoint not working"
fi

# Cleanup
cleanup_workspace "$WORKSPACE"
docker rmi "${REMOTE_IMAGE}:${IMAGE_TAG}" > /dev/null 2>&1 || true

print_summary
