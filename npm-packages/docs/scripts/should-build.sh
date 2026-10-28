#!/usr/bin/env bash
# Netlify ignore command: exit 0 to skip build, exit 1 to proceed.
# https://docs.netlify.com/build/configure-builds/ignore-builds/
#
# Watched paths are derived from docs build inputs in docusaurus.config.ts:
# - npm-packages/docs/ (the docs site itself)
# - npm-packages/convex/src/{browser,server,react,react-auth0,react-clerk,nextjs,values}/ (TypeDoc entry points)
# - npm-packages/convex/src/{common,type_utils.ts} (cross-imported by TypeDoc entry points)
# - npm-packages/convex/tsconfig.json (TypeDoc config)
# - npm-packages/@convex-dev/platform/*-openapi.json (OpenAPI specs)

WATCHED_PATHS=(
  npm-packages/docs/
  npm-packages/convex/src/browser/
  npm-packages/convex/src/server/
  npm-packages/convex/src/react/
  npm-packages/convex/src/react-auth0/
  npm-packages/convex/src/react-clerk/
  npm-packages/convex/src/nextjs/
  npm-packages/convex/src/values/
  npm-packages/convex/src/common/
  npm-packages/convex/src/type_utils.ts
  npm-packages/convex/tsconfig.json
  npm-packages/@convex-dev/platform/management-openapi.json
  npm-packages/@convex-dev/platform/public-deployment-openapi.json
  npm-packages/@convex-dev/platform/deployment-openapi.json
)

echo ""
echo "========================================"
echo "  Docs ignore script (should-build.sh)"
echo "========================================"
echo ""
echo "Context: ${CONTEXT:-<empty>}"
echo "Commit:  ${COMMIT_REF:-<empty>}"

if [ "$CONTEXT" = "deploy-preview" ]; then
  # Deploy preview (PR): compare the PR's changes against its fork point from
  # main. This answers "does this PR touch docs?" without being affected by
  # stale CACHED_COMMIT_REF values or unrelated changes on main.
  echo "Mode:    deploy-preview (merge-base with origin/main)"
  # Fetch main's full history so merge-base can reach the PR's fork point. Avoid
  # --depth=1: it grafts main's tip shallow, leaving merge-base nothing to find.
  # --unshallow recovers an already-shallow cache; on a complete repo it errors
  # and falls through to a normal fetch.
  git fetch origin main --unshallow 2>/dev/null || git fetch origin main 2>/dev/null
  base=$(git merge-base HEAD origin/main 2>/dev/null)
else
  # Branch deploy (main, docs-prod): compare against the last successfully
  # deployed commit. CACHED_COMMIT_REF is set by Netlify to the SHA of the
  # most recent build that completed; this catches all changes since that
  # deploy, not just the latest single commit.
  #
  # If CACHED_COMMIT_REF is missing or equals COMMIT_REF (no prior cache),
  # we can't diff meaningfully, so force the build.
  echo "Mode:    branch-deploy (CACHED_COMMIT_REF)"
  if [ -z "$CACHED_COMMIT_REF" ] || [ "$CACHED_COMMIT_REF" = "$COMMIT_REF" ]; then
    echo ""
    echo "No usable cached commit (CACHED_COMMIT_REF=${CACHED_COMMIT_REF:-<empty>}), proceeding with build"
    echo "========================================"
    exit 1
  fi
  base="$CACHED_COMMIT_REF"
fi

# Resolve both refs to full SHAs for unambiguous logging.
base_sha=$(git rev-parse "$base" 2>/dev/null)
commit_sha=$(git rev-parse "$COMMIT_REF" 2>/dev/null)

if [ -z "$base_sha" ]; then
  echo ""
  echo "Could not determine base ref, proceeding with build"
  echo "========================================"
  exit 1
fi

echo ""
echo "Base:    $base_sha"
echo "Head:    $commit_sha"
echo ""
echo "Watched paths:"
for p in "${WATCHED_PATHS[@]}"; do
  echo "  $p"
done

echo ""
echo "To reproduce locally:"
echo "  git diff --name-only $base_sha $commit_sha -- ${WATCHED_PATHS[*]}"

echo ""
if git diff --quiet "$base_sha" "$commit_sha" -- "${WATCHED_PATHS[@]}"; then
  echo "Result: no docs-relevant changes found, skipping build"
  echo ""
  echo "========================================"
  exit 0
else
  echo "Result: docs-relevant changes detected, proceeding with build"
  echo ""
  git diff --name-only "$base_sha" "$commit_sha" -- "${WATCHED_PATHS[@]}" | while read -r f; do
    echo "  $f"
  done
  echo ""
  echo "========================================"
  exit 1
fi
