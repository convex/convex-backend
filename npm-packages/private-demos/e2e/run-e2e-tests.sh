#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Use the same ports as backendHarness.js and read the dev admin key from the
# canonical location, matching the pattern used by js-integration-tests and
# smoke tests.
CONVEX_URL="${CONVEX_URL:-http://127.0.0.1:3210}"
ADMIN_KEY="${ADMIN_KEY:-$(cat "$REPO_ROOT/crates/keybroker/dev/admin_key.txt")}"

project_dir_for() {
  case "$1" in
    tanstack-start-clerk)      echo "tanstack-start-clerk" ;;
    tanstack-start-workos)     echo "tanstack-start-workos" ;;
    tanstack-start)            echo "tanstack-start" ;;
    tanstack-start-quickstart) echo "quickstarts/tanstack-start" ;;
    *)                         echo "" ;;
  esac
}

run_project_tests() {
  local project="$1"
  local project_dir
  project_dir="$(project_dir_for "$project")"

  if [ -z "$project_dir" ]; then
    echo "Unknown project: $project"
    echo "Valid options: tanstack-start-clerk, tanstack-start-workos, tanstack-start, tanstack-start-quickstart"
    exit 1
  fi

  local full_project_dir="$SCRIPT_DIR/../$project_dir"

  echo "=== Testing $project ($project_dir) ==="

  # Deploy Convex functions to local backend
  echo "Deploying Convex functions..."
  pushd "$full_project_dir" > /dev/null
  npx convex deploy --url "$CONVEX_URL" --admin-key "$ADMIN_KEY" -y
  popd > /dev/null

  # Run Playwright tests for this project
  echo "Running Playwright tests..."
  PROJECT="$project" npx playwright test --config "$SCRIPT_DIR/playwright.config.ts"
}

# If a specific project is provided, run just that one
if [ $# -gt 0 ]; then
  run_project_tests "$1"
else
  # Run all projects
  for project in tanstack-start-clerk tanstack-start tanstack-start-quickstart; do
    run_project_tests "$project"
  done
fi
