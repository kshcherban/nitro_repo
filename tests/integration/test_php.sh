#!/bin/bash
# PHP (Composer V2) integration tests - hosted repository

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

PHP_HOSTED_REPO="${TEST_STORAGE}/php-hosted"
FIXTURE_DIR="/fixtures/php/sample-lib"
PACKAGE_NAME="nitro-test/sample-lib"
VENDOR="nitro-test"
PACKAGE="sample-lib"
VERSION_1="1.0.0"
VERSION_2="1.0.1"

print_section "PHP Integration Tests (Composer V2 hosted)"

WORKSPACE=$(create_workspace "php")
cd "$WORKSPACE"

copy_fixture() {
    rm -rf "$WORKSPACE/sample-lib"
    cp -r "$FIXTURE_DIR" "$WORKSPACE/sample-lib"
}

build_zip() {
    local version="$1"
    pushd "$WORKSPACE/sample-lib" >/dev/null
    jq ".version = \"${version}\"" composer.json > composer.json.tmp && mv composer.json.tmp composer.json
    cat > src/Greeter.php <<EOF
<?php

namespace NitroTest\\SampleLib;

class Greeter
{
    public static function greet(string \$name): string
    {
        return "Hello, {\$name}!";
    }

    public static function getVersion(): string
    {
        return '${version}';
    }
}
EOF
    zip -qr "$WORKSPACE/sample-lib-${version}.zip" .
    popd >/dev/null
}

# Test 1: create and upload v1
print_test "Create and upload ${VERSION_1} dist"
copy_fixture
build_zip "$VERSION_1"
UPLOAD_PATH_V1="/repositories/${PHP_HOSTED_REPO}/dist/${VENDOR}/${PACKAGE}/${VERSION_1}.zip"
STATUS=$(get_http_status "${NITRO_URL}${UPLOAD_PATH_V1}" \
    -X PUT \
    -H "$(get_auth_header)" \
    --data-binary "@${WORKSPACE}/sample-lib-${VERSION_1}.zip")
if assert_http_status "201" "$STATUS"; then
    pass
else
    fail "Expected 201, got $STATUS"
fi

# Test 2: composer install v1
print_test "Composer install ${VERSION_1}"
CONSUMER_DIR="$WORKSPACE/consumer-v1"
mkdir -p "$CONSUMER_DIR"
cat > "$CONSUMER_DIR/composer.json" <<EOF
{
  "name": "test/consumer",
  "repositories": [
    { "type": "composer", "url": "${NITRO_URL}/repositories/${PHP_HOSTED_REPO}" }
  ],
  "require": { "${PACKAGE_NAME}": "${VERSION_1}" },
  "config": { "secure-http": false }
}
EOF
pushd "$CONSUMER_DIR" >/dev/null
if run_cmd composer install --no-interaction --no-progress; then
    pass
else
    fail "Composer install failed"
fi

# Test 3: verify library output
print_test "Verify library output for ${VERSION_1}"
cat > test.php <<'EOF'
<?php
require 'vendor/autoload.php';
use NitroTest\SampleLib\Greeter;
echo Greeter::greet('World') . PHP_EOL;
echo Greeter::getVersion() . PHP_EOL;
EOF
OUTPUT=$(php test.php)
record_output "$OUTPUT"
if echo "$OUTPUT" | grep -q "Hello, World!" && echo "$OUTPUT" | grep -q "${VERSION_1}"; then
    clear_last_log
    pass
else
    fail "Unexpected output"
fi
popd >/dev/null

# Test 4: upload v2
print_test "Create and upload ${VERSION_2} dist"
copy_fixture
build_zip "$VERSION_2"
UPLOAD_PATH_V2="/repositories/${PHP_HOSTED_REPO}/dist/${VENDOR}/${PACKAGE}/${VERSION_2}.zip"
STATUS=$(get_http_status "${NITRO_URL}${UPLOAD_PATH_V2}" \
    -X PUT \
    -H "$(get_auth_header)" \
    --data-binary "@${WORKSPACE}/sample-lib-${VERSION_2}.zip")
if assert_http_status "201" "$STATUS"; then
    pass
else
    fail "Expected 201, got $STATUS"
fi

# Test 5: install v2 explicitly
print_test "Composer install ${VERSION_2}"
CONSUMER_DIR_V2="$WORKSPACE/consumer-v2"
mkdir -p "$CONSUMER_DIR_V2"
cat > "$CONSUMER_DIR_V2/composer.json" <<EOF
{
  "name": "test/consumer-v2",
  "repositories": [
    { "type": "composer", "url": "${NITRO_URL}/repositories/${PHP_HOSTED_REPO}" }
  ],
  "require": { "${PACKAGE_NAME}": "${VERSION_2}" },
  "config": { "secure-http": false }
}
EOF
pushd "$CONSUMER_DIR_V2" >/dev/null
if run_cmd composer install --no-interaction --no-progress; then
    pass
else
    fail "Composer install failed for v2"
fi
OUTPUT2=$(php -r "require 'vendor/autoload.php'; echo NitroTest\\SampleLib\\Greeter::getVersion();")
record_output "$OUTPUT2"
if [ "$OUTPUT2" = "$VERSION_2" ]; then
    clear_last_log
    pass
else
    fail "Expected version ${VERSION_2}, got ${OUTPUT2}"
fi
popd >/dev/null

# Test 6: upload requires auth
print_test "Upload without auth should be rejected"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
    -X PUT \
    "${NITRO_URL}${UPLOAD_PATH_V1}" \
    --data-binary "@${WORKSPACE}/sample-lib-${VERSION_1}.zip")
if assert_http_status "401" "$STATUS"; then
    pass
else
    fail "Expected 401 without auth, got $STATUS"
fi

# Test 7: 404 on missing package metadata
print_test "404 for missing package metadata"
MISSING_META="/repositories/${PHP_HOSTED_REPO}/p2/${VENDOR}/does-not-exist.json"
STATUS=$(get_http_status "${NITRO_URL}${MISSING_META}")
if assert_http_status "404" "$STATUS"; then
    pass
else
    fail "Expected 404, got $STATUS"
fi

cleanup_workspace "$WORKSPACE"
