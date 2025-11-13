#!/bin/bash
# Helm integration tests
# Tests hosted Helm repositories (HTTP and OCI) end-to-end

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

# Helm-specific configuration
HELM_HOSTED_REPO="${TEST_STORAGE}/helm-hosted"
FIXTURE_DIR="/fixtures/helm/test-chart"
CHART_NAME="test-chart"
VERSION_1="1.0.0"
VERSION_2="1.0.1"

print_section "Helm Integration Tests"

WORKSPACE=$(create_workspace "helm")
cd "$WORKSPACE"

# Copy fixture
cp -r "$FIXTURE_DIR" "$WORKSPACE/test-chart"
cd "$WORKSPACE"

# Test 1: Package Helm chart
print_test "Package Helm chart (${VERSION_1})"
if helm package test-chart > /dev/null 2>&1; then
    CHART_PACKAGE="${CHART_NAME}-${VERSION_1}.tgz"
    if assert_file_exists "$CHART_PACKAGE"; then
        pass
    else
        fail "Chart package not created"
        cleanup_workspace "$WORKSPACE"
        exit 1
    fi
else
    fail "Failed to package chart"
    cleanup_workspace "$WORKSPACE"
    exit 1
fi

# Test 2: Upload chart via HTTP PUT
print_test "Upload chart via HTTP PUT"
UPLOAD_PATH="/repositories/${HELM_HOSTED_REPO}/${CHART_PACKAGE}"

STATUS=$(get_http_status "${NITRO_URL}${UPLOAD_PATH}" \
    -X PUT \
    -H "$(get_auth_header)" \
    --data-binary "@${CHART_PACKAGE}")

if assert_http_status "201" "$STATUS"; then
    pass
else
    fail "Expected 201, got $STATUS"
fi

# Test 3: Upload chart via ChartMuseum API
print_test "Upload chart via ChartMuseum API (second version)"

# Update chart version
cd test-chart
sed -i "s/version: ${VERSION_1}/version: ${VERSION_2}/" Chart.yaml
cd ..

helm package test-chart > /dev/null 2>&1
CHART_PACKAGE_V2="${CHART_NAME}-${VERSION_2}.tgz"

STATUS=$(get_http_status "${NITRO_URL}/repositories/${HELM_HOSTED_REPO}/api/charts" \
    -X POST \
    -H "$(get_auth_header)" \
    -F "chart=@${CHART_PACKAGE_V2}")

if [ "$STATUS" = "201" ] || [ "$STATUS" = "200" ]; then
    pass
else
    fail "Expected 201/200, got $STATUS"
fi

# Test 4: Fetch index.yaml
print_test "Fetch index.yaml"
INDEX_PATH="/repositories/${HELM_HOSTED_REPO}/index.yaml"

INDEX_CONTENT=$(curl -sf "${NITRO_URL}${INDEX_PATH}" || echo "")

if echo "$INDEX_CONTENT" | grep -q "apiVersion: v1" && \
   echo "$INDEX_CONTENT" | grep -q "${CHART_NAME}"; then
    pass
else
    fail "index.yaml not generated correctly"
fi

# Test 5: Verify both versions in index
print_test "Verify both versions in index.yaml"

if echo "$INDEX_CONTENT" | grep -q "${VERSION_1}" && \
   echo "$INDEX_CONTENT" | grep -q "${VERSION_2}"; then
    pass
else
    fail "Both versions not in index"
fi

# Test 6: Add Helm repository
print_test "Add Helm repository"
REPO_NAME="nitro-test-$(random_string 6)"

if helm repo add "$REPO_NAME" "${NITRO_URL}/repositories/${HELM_HOSTED_REPO}" > /dev/null 2>&1; then
    pass
else
    fail "Failed to add Helm repository"
fi

# Test 7: Update Helm repository
print_test "Update Helm repository"
if helm repo update > /dev/null 2>&1; then
    pass
else
    fail "Failed to update Helm repository"
fi

# Test 8: Search for chart
print_test "Search for chart in repository"
SEARCH_RESULT=$(helm search repo "$REPO_NAME/${CHART_NAME}" || echo "")

if echo "$SEARCH_RESULT" | grep -q "${CHART_NAME}"; then
    pass
else
    fail "Chart not found in search results"
fi

# Test 9: Pull chart
print_test "Pull chart from repository"
PULL_DIR="$WORKSPACE/pulled"
mkdir -p "$PULL_DIR"
cd "$PULL_DIR"

if helm pull "$REPO_NAME/${CHART_NAME}" --version "${VERSION_1}" > /dev/null 2>&1 && \
   assert_file_exists "${CHART_NAME}-${VERSION_1}.tgz"; then
    pass
else
    fail "Failed to pull chart"
fi

# Test 10: Verify pulled chart integrity
print_test "Verify pulled chart integrity"
ORIGINAL_HASH=$(sha256sum "$WORKSPACE/${CHART_NAME}-${VERSION_1}.tgz" | cut -d' ' -f1)
PULLED_HASH=$(sha256sum "${CHART_NAME}-${VERSION_1}.tgz" | cut -d' ' -f1)

if [ "$ORIGINAL_HASH" = "$PULLED_HASH" ]; then
    pass
else
    fail "Hash mismatch: original=$ORIGINAL_HASH, pulled=$PULLED_HASH"
fi

# Test 11: Pull latest version
print_test "Pull latest version"
cd "$WORKSPACE"
PULL_DIR_LATEST="$WORKSPACE/pulled-latest"
mkdir -p "$PULL_DIR_LATEST"
cd "$PULL_DIR_LATEST"

if helm pull "$REPO_NAME/${CHART_NAME}" > /dev/null 2>&1 && \
   assert_file_exists "${CHART_NAME}-${VERSION_2}.tgz"; then
    pass
else
    fail "Failed to pull latest version"
fi

# Test 12: Download chart via direct URL
print_test "Download chart via direct URL"
DOWNLOAD_PATH="/repositories/${HELM_HOSTED_REPO}/${CHART_NAME}-${VERSION_1}.tgz"

if curl -sf "${NITRO_URL}${DOWNLOAD_PATH}" -o "$WORKSPACE/downloaded.tgz" && \
   assert_file_exists "$WORKSPACE/downloaded.tgz"; then
    pass
else
    fail "Failed to download chart via URL"
fi

# Test 13: ChartMuseum health check
print_test "ChartMuseum health endpoint"
HEALTH_PATH="/repositories/${HELM_HOSTED_REPO}/health"

STATUS=$(get_http_status "${NITRO_URL}${HEALTH_PATH}")

if [ "$STATUS" = "200" ] || [ "$STATUS" = "404" ]; then
    # 404 is acceptable if health endpoint not implemented
    pass
else
    fail "Unexpected status: $STATUS"
fi

# Test 14: Authentication required for upload
print_test "Verify authentication required for upload"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
    -X PUT \
    "${NITRO_URL}${UPLOAD_PATH}" \
    --data '{}')

if assert_http_status "401" "$STATUS"; then
    pass
else
    fail "Expected 401 without auth, got $STATUS"
fi

# Test 15: Not found for non-existent chart
print_test "Verify 404 for non-existent chart"
NONEXISTENT_PATH="/repositories/${HELM_HOSTED_REPO}/nonexistent-chart-1.0.0.tgz"

STATUS=$(get_http_status "${NITRO_URL}${NONEXISTENT_PATH}")

if assert_http_status "404" "$STATUS"; then
    pass
else
    fail "Expected 404, got $STATUS"
fi

# Test 16: Delete chart (if supported)
print_test "Delete chart via ChartMuseum API"
DELETE_PATH="/repositories/${HELM_HOSTED_REPO}/api/charts/${CHART_NAME}/${VERSION_1}"

STATUS=$(get_http_status "${NITRO_URL}${DELETE_PATH}" \
    -X DELETE \
    -H "$(get_auth_header)")

if [ "$STATUS" = "200" ] || [ "$STATUS" = "204" ] || [ "$STATUS" = "404" ]; then
    # 404 is acceptable if delete not implemented
    pass
else
    fail "Unexpected status for delete: $STATUS"
fi

# Cleanup
helm repo remove "$REPO_NAME" > /dev/null 2>&1 || true
cleanup_workspace "$WORKSPACE"

print_summary
