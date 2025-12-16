#!/bin/bash
# RubyGems integration tests
# Tests hosted and proxy RubyGems repositories end-to-end using Bundler + gem

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

wait_for_server 60

print_section "RubyGems Integration Tests"

WORKSPACE=$(create_workspace "ruby")

cleanup() {
    cleanup_workspace "$WORKSPACE"
}
trap cleanup EXIT

RUBY_HOSTED_REPO="${TEST_STORAGE}/ruby-hosted"
RUBY_PROXY_REPO="${TEST_STORAGE}/ruby-proxy"

GEM_NAME="nitro-test-gem"
GEM_VERSION="0.1.0"
GEM_FILE="${GEM_NAME}-${GEM_VERSION}.gem"

HOSTED_REPO_ID=""
PROXY_REPO_ID=""
STORAGE_NAME=""

print_test "Locate seeded Ruby repositories"
if repos=$(api_get "/api/repository/list"); then
    hosted_entry=$(jq -r --arg name "ruby-hosted" '[.[] | select(.name == $name)][0]' <<<"$repos")
    proxy_entry=$(jq -r --arg name "ruby-proxy" '[.[] | select(.name == $name)][0]' <<<"$repos")
    if [[ "$hosted_entry" == "null" || "$proxy_entry" == "null" ]]; then
        record_output "$repos"
        fail "Seeded ruby-hosted/ruby-proxy repositories not found. Ensure tests/docker/seed-data.sql contains them."
        exit 1
    fi

    HOSTED_REPO_ID=$(jq -r '.id' <<<"$hosted_entry")
    PROXY_REPO_ID=$(jq -r '.id' <<<"$proxy_entry")
    STORAGE_NAME=$(jq -r '.storage_name' <<<"$hosted_entry")

    if [[ -z "$HOSTED_REPO_ID" || "$HOSTED_REPO_ID" == "null" || -z "$PROXY_REPO_ID" || "$PROXY_REPO_ID" == "null" ]]; then
        record_output "$repos"
        fail "Seeded ruby repositories missing id fields"
        exit 1
    fi
    pass
else
    fail "Unable to query repository list"
    exit 1
fi

print_test "Build Ruby gem fixture"
mkdir -p "$WORKSPACE/gem"
cd "$WORKSPACE/gem"
mkdir -p lib

cat > "${GEM_NAME}.gemspec" <<EOF
Gem::Specification.new do |spec|
  spec.name = "${GEM_NAME}"
  spec.version = "${GEM_VERSION}"
  spec.summary = "Nitro Repo test gem"
  spec.authors = ["Nitro Repo"]
  spec.files = Dir["lib/**/*.rb"]
  spec.require_paths = ["lib"]
end
EOF

cat > "lib/${GEM_NAME}.rb" <<EOF
require_relative "${GEM_NAME}/version"

module NitroTestGem
  def self.hello(name)
    "Hello, #{name}!"
  end
end
EOF

mkdir -p "lib/${GEM_NAME}"
cat > "lib/${GEM_NAME}/version.rb" <<EOF
module NitroTestGem
  VERSION = "${GEM_VERSION}"
end
EOF

if run_cmd gem build "${GEM_NAME}.gemspec"; then
    if [ -f "$GEM_FILE" ]; then
        pass
    else
        fail "gem build completed but ${GEM_FILE} is missing"
        exit 1
    fi
else
    fail "Failed to build gem"
    exit 1
fi

print_test "Publish gem to ruby-hosted using gem push"
export GEM_HOST_API_KEY="${TEST_TOKEN}"
HOSTED_BASE="${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/"
if run_cmd gem push --host "${HOSTED_BASE}" "${GEM_FILE}"; then
    pass
else
    fail "gem push failed"
    exit 1
fi

print_test "Hosted: /names contains gem"
names_body=$(curl -sf "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/names" || echo "")
record_output "$names_body"
if echo "$names_body" | grep -qFx "${GEM_NAME}"; then
    clear_last_log
    pass
else
    fail "Expected ${GEM_NAME} in /names"
fi

print_test "Hosted: /versions contains gem version"
versions_body=$(curl -sf "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/versions" || echo "")
record_output "$versions_body"
if echo "$versions_body" | grep -q "${GEM_NAME} ${GEM_VERSION}"; then
    clear_last_log
    pass
else
    fail "Expected ${GEM_NAME} ${GEM_VERSION} in /versions"
fi

print_test "Hosted: /info/<gem> includes version and checksum"
info_body=$(curl -sf "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/info/${GEM_NAME}" || echo "")
record_output "$info_body"
if echo "$info_body" | grep -q "${GEM_VERSION}" && echo "$info_body" | grep -q "checksum:"; then
    clear_last_log
    pass
else
    fail "Expected ${GEM_VERSION} and checksum in /info/${GEM_NAME}"
fi

print_test "Hosted: gem download endpoint works"
HOSTED_DOWNLOAD="${WORKSPACE}/download-hosted.gem"
if run_cmd curl -sf -o "${HOSTED_DOWNLOAD}" "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/gems/${GEM_FILE}"; then
    if assert_file_exists "${HOSTED_DOWNLOAD}"; then
        pass
    else
        fail "Downloaded gem missing"
    fi
else
    fail "Failed to download gem from hosted repo"
fi

print_test "Hosted: bundle install from ruby-hosted succeeds"
HOSTED_BUNDLE_DIR="${WORKSPACE}/bundle-hosted"
mkdir -p "$HOSTED_BUNDLE_DIR"
cd "$HOSTED_BUNDLE_DIR"
cat > Gemfile <<EOF
source "${HOSTED_BASE}"

gem "${GEM_NAME}", "${GEM_VERSION}"
EOF

if run_cmd bundle install --path vendor/bundle; then
    if output=$(bundle exec ruby -e "require '${GEM_NAME}'; puts NitroTestGem.hello('World'); puts NitroTestGem::VERSION"); then
        record_output "$output"
        if echo "$output" | grep -q "Hello, World!" && echo "$output" | grep -q "${GEM_VERSION}"; then
            clear_last_log
            pass
        else
            fail "Installed gem did not behave as expected"
        fi
    else
        fail "bundle exec failed"
    fi
else
    fail "bundle install failed"
fi

print_test "Proxy: bundle install from ruby-proxy succeeds"
PROXY_BASE="${NITRO_URL}/repositories/${RUBY_PROXY_REPO}/"
PROXY_BUNDLE_DIR="${WORKSPACE}/bundle-proxy"
mkdir -p "$PROXY_BUNDLE_DIR"
cd "$PROXY_BUNDLE_DIR"
cat > Gemfile <<EOF
source "${PROXY_BASE}"

gem "${GEM_NAME}", "${GEM_VERSION}"
EOF

if run_cmd bundle install --path vendor/bundle; then
    if output=$(bundle exec ruby -e "require '${GEM_NAME}'; puts NitroTestGem.hello('Proxy'); puts NitroTestGem::VERSION"); then
        record_output "$output"
        if echo "$output" | grep -q "Hello, Proxy!" && echo "$output" | grep -q "${GEM_VERSION}"; then
            clear_last_log
            pass
        else
            fail "Proxy-installed gem did not behave as expected"
        fi
    else
        fail "bundle exec failed for proxy install"
    fi
else
    fail "bundle install failed for proxy repo"
fi

print_test "Proxy: cached gem served after upstream URL is changed"
proxy_payload_bad=$(jq -n --arg url "http://invalid.invalid" '{type: "Proxy", config: {upstream_url: $url}}')
if api_put "/api/repository/${PROXY_REPO_ID}/config/ruby" \
    -H "Content-Type: application/json" \
    -d "$proxy_payload_bad" >/dev/null; then
    # The gem file should still be available via cache hit (non-Range request).
    PROXY_DOWNLOAD="${WORKSPACE}/download-proxy.gem"
    if run_cmd curl -sf -o "${PROXY_DOWNLOAD}" "${NITRO_URL}/repositories/${RUBY_PROXY_REPO}/gems/${GEM_FILE}"; then
        if assert_file_exists "${PROXY_DOWNLOAD}"; then
            pass
        else
            fail "Downloaded proxy gem missing after upstream change"
        fi
    else
        fail "Failed to download cached gem from proxy after upstream change"
    fi
else
    fail "Failed to update ruby-proxy config (bad upstream URL)"
fi

print_test "Restore ruby-proxy upstream URL"
proxy_payload_restore=$(jq -n --arg url "http://nitro-repo:8888/repositories/${RUBY_HOSTED_REPO}" '{type: "Proxy", config: {upstream_url: $url}}')
if api_put "/api/repository/${PROXY_REPO_ID}/config/ruby" \
    -H "Content-Type: application/json" \
    -d "$proxy_payload_restore" >/dev/null; then
    pass
else
    fail "Failed to restore ruby-proxy upstream URL"
fi

print_test "Yank gem via API"
if api_delete "/repositories/${RUBY_HOSTED_REPO}/api/v1/gems/yank" \
    -d "gem_name=${GEM_NAME}" -d "version=${GEM_VERSION}" >/dev/null; then
    pass
else
    fail "Failed to yank gem"
fi

print_test "Hosted: /names no longer contains gem after yank"
names_body_after=$(curl -sf "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/names" || echo "")
record_output "$names_body_after"
if echo "$names_body_after" | grep -qF "${GEM_NAME}"; then
    fail "Gem still present in /names after yank"
else
    clear_last_log
    pass
fi

print_test "Hosted: /versions no longer contains gem after yank"
versions_body_after=$(curl -sf "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/versions" || echo "")
record_output "$versions_body_after"
if echo "$versions_body_after" | grep -qF "${GEM_NAME}"; then
    fail "Gem still present in /versions after yank"
else
    clear_last_log
    pass
fi

print_test "Hosted: /info/<gem> is removed after yank"
info_status=$(get_http_status "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/info/${GEM_NAME}")
record_output "$info_status"
if assert_http_status "404" "$info_status"; then
    clear_last_log
    pass
else
    fail "Expected 404 for /info/${GEM_NAME} after yank (got ${info_status})"
fi

print_test "Hosted: gem download returns 404 after yank"
gem_status=$(get_http_status "${NITRO_URL}/repositories/${RUBY_HOSTED_REPO}/gems/${GEM_FILE}")
record_output "$gem_status"
if assert_http_status "404" "$gem_status"; then
    clear_last_log
    pass
else
    fail "Expected 404 for gem download after yank (got ${gem_status})"
fi

print_summary
