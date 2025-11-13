#!/bin/bash
# PHP (Composer) integration tests
# Tests hosted PHP repositories end-to-end

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

# PHP-specific configuration
PHP_HOSTED_REPO="${TEST_STORAGE}/php-hosted"
FIXTURE_DIR="/fixtures/php/sample-lib"
PACKAGE_NAME="nitro-test/sample-lib"
VERSION_1="1.0.0"
VERSION_2="1.0.1"

print_section "PHP Integration Tests"

WORKSPACE=$(create_workspace "php")
cd "$WORKSPACE"

# Copy fixture
cp -r "$FIXTURE_DIR" "$WORKSPACE/sample-lib"
cd "$WORKSPACE/sample-lib"

# Test 1: Create package archive
print_test "Create PHP package archive"
if tar -czf "$WORKSPACE/sample-lib-${VERSION_1}.tar.gz" -C "$WORKSPACE" sample-lib; then
    pass
else
    fail "Failed to create package archive"
    cleanup_workspace "$WORKSPACE"
    exit 1
fi

# Test 2: Upload package to hosted repository
print_test "Upload package to php-hosted"
UPLOAD_PATH="/repositories/${PHP_HOSTED_REPO}/packages/${PACKAGE_NAME//\//-}-${VERSION_1}.tar.gz"

STATUS=$(get_http_status "${NITRO_URL}${UPLOAD_PATH}" \
    -X PUT \
    -H "$(get_auth_header)" \
    --data-binary "@${WORKSPACE}/sample-lib-${VERSION_1}.tar.gz")

if assert_http_status "201" "$STATUS"; then
    pass
else
    fail "Expected 201, got $STATUS"
fi

# Test 3: Create packages.json metadata
print_test "Create/update packages.json"
PACKAGES_JSON_PATH="/repositories/${PHP_HOSTED_REPO}/packages.json"

cat > "$WORKSPACE/packages.json" <<EOF
{
  "packages": {
    "${PACKAGE_NAME}": {
      "${VERSION_1}": {
        "name": "${PACKAGE_NAME}",
        "version": "${VERSION_1}",
        "dist": {
          "url": "${NITRO_URL}${UPLOAD_PATH}",
          "type": "tar"
        },
        "require": {
          "php": ">=7.4"
        },
        "type": "library"
      }
    }
  }
}
EOF

STATUS=$(get_http_status "${NITRO_URL}${PACKAGES_JSON_PATH}" \
    -X PUT \
    -H "$(get_auth_header)" \
    -H "Content-Type: application/json" \
    --data-binary "@${WORKSPACE}/packages.json")

if [ "$STATUS" = "201" ] || [ "$STATUS" = "200" ]; then
    pass
else
    fail "Expected 201/200, got $STATUS"
fi

# Test 4: Configure Composer to use Nitro Repo
print_test "Configure Composer repository"
CONSUMER_DIR="$WORKSPACE/consumer"
mkdir -p "$CONSUMER_DIR"
cd "$CONSUMER_DIR"

cat > "$CONSUMER_DIR/composer.json" <<EOF
{
  "name": "test/consumer",
  "repositories": [
    {
      "type": "composer",
      "url": "${NITRO_URL}/repositories/${PHP_HOSTED_REPO}"
    }
  ],
  "require": {
    "${PACKAGE_NAME}": "${VERSION_1}"
  }
}
EOF

if run_cmd composer install --no-interaction; then
    pass
else
    fail "Composer install failed"
fi

# Test 5: Verify installed package works
print_test "Verify installed package functionality"
cat > "$CONSUMER_DIR/test.php" <<EOF
<?php
require 'vendor/autoload.php';

use NitroTest\\SampleLib\\Greeter;

echo Greeter::greet('World') . PHP_EOL;
echo Greeter::getVersion() . PHP_EOL;
EOF

OUTPUT=$(php test.php)
record_output "$OUTPUT"

if echo "$OUTPUT" | grep -q "Hello, World!" && \
   echo "$OUTPUT" | grep -q "${VERSION_1}"; then
    clear_last_log
    pass
else
    fail "Package not functioning correctly"
fi

# Test 6: Upload second version
print_test "Upload second version (${VERSION_2})"
cd "$WORKSPACE/sample-lib"

# Update version in composer.json
jq ".version = \"${VERSION_2}\"" composer.json > composer.json.tmp && mv composer.json.tmp composer.json

tar -czf "$WORKSPACE/sample-lib-${VERSION_2}.tar.gz" -C "$WORKSPACE" sample-lib

UPLOAD_PATH_V2="/repositories/${PHP_HOSTED_REPO}/packages/${PACKAGE_NAME//\//-}-${VERSION_2}.tar.gz"

STATUS=$(get_http_status "${NITRO_URL}${UPLOAD_PATH_V2}" \
    -X PUT \
    -H "$(get_auth_header)" \
    --data-binary "@${WORKSPACE}/sample-lib-${VERSION_2}.tar.gz")

if assert_http_status "201" "$STATUS"; then
    pass
else
    fail "Expected 201, got $STATUS"
fi

# Test 7: Update packages.json with second version
print_test "Update packages.json with both versions"
cat > "$WORKSPACE/packages-v2.json" <<EOF
{
  "packages": {
    "${PACKAGE_NAME}": {
      "${VERSION_1}": {
        "name": "${PACKAGE_NAME}",
        "version": "${VERSION_1}",
        "dist": {
          "url": "${NITRO_URL}${UPLOAD_PATH}",
          "type": "tar"
        },
        "require": {
          "php": ">=7.4"
        },
        "type": "library"
      },
      "${VERSION_2}": {
        "name": "${PACKAGE_NAME}",
        "version": "${VERSION_2}",
        "dist": {
          "url": "${NITRO_URL}${UPLOAD_PATH_V2}",
          "type": "tar"
        },
        "require": {
          "php": ">=7.4"
        },
        "type": "library"
      }
    }
  }
}
EOF

STATUS=$(get_http_status "${NITRO_URL}${PACKAGES_JSON_PATH}" \
    -X PUT \
    -H "$(get_auth_header)" \
    -H "Content-Type: application/json" \
    --data-binary "@${WORKSPACE}/packages-v2.json")

if [ "$STATUS" = "201" ] || [ "$STATUS" = "200" ]; then
    pass
else
    fail "Expected 201/200, got $STATUS"
fi

# Test 8: Install specific version
print_test "Install specific version (${VERSION_2})"
CONSUMER_DIR_V2="$WORKSPACE/consumer-v2"
mkdir -p "$CONSUMER_DIR_V2"
cd "$CONSUMER_DIR_V2"

cat > "$CONSUMER_DIR_V2/composer.json" <<EOF
{
  "name": "test/consumer-v2",
  "repositories": [
    {
      "type": "composer",
      "url": "${NITRO_URL}/repositories/${PHP_HOSTED_REPO}"
    }
  ],
  "require": {
    "${PACKAGE_NAME}": "${VERSION_2}"
  }
}
EOF

if run_cmd composer install --no-interaction; then
    pass
else
    fail "Failed to install specific version"
fi

# Test 9: Authentication required for upload
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

# Test 10: Not found for non-existent package
print_test "Verify 404 for non-existent package"
NONEXISTENT_PATH="/repositories/${PHP_HOSTED_REPO}/packages/nonexistent-1.0.0.tar.gz"

STATUS=$(get_http_status "${NITRO_URL}${NONEXISTENT_PATH}")

if assert_http_status "404" "$STATUS"; then
    pass
else
    fail "Expected 404, got $STATUS"
fi

# Cleanup
cleanup_workspace "$WORKSPACE"

print_summary
