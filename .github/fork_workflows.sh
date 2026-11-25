#!/usr/bin/env bash
# The fork runs only the workflows listed below (see .github/UPSTREAM_SYNC.md).
# Upstream syncs bring the others back: an upstream edit to a deleted workflow
# surfaces as a modify/delete conflict, and a new upstream workflow arrives
# without one. "Build Convex Backend" runs this check on every pull request.
#
#   .github/fork_workflows.sh          # fail if any other workflow is present
#   .github/fork_workflows.sh --prune  # git rm them (after merging upstream)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

allowed=(
  .github/workflows/build_local_backend.yml
  .github/workflows/prettier.yml
  .github/workflows/publish_fork_backend.yml
)

extra=()
while IFS= read -r path; do
  keep=false
  for allowed_path in "${allowed[@]}"; do
    [[ $path == "$allowed_path" ]] && keep=true
  done
  $keep || extra+=("$path")
done < <(
  {
    git ls-files -- .github/workflows
    find .github/workflows -type f 2>/dev/null
  } | sed 's|^\./||' | sort -u
)

if ((${#extra[@]} == 0)); then
  echo "Only the fork's workflows are present."
  exit 0
fi

if [[ ${1:-} == --prune ]]; then
  for path in "${extra[@]}"; do
    git rm -q -f --ignore-unmatch -- "$path"
    rm -f -- "$path"
    echo "removed $path"
  done
  exit 0
fi

for path in "${extra[@]}"; do
  echo "::error file=$path::$path is not part of the fork's CI." \
    "Run .github/fork_workflows.sh --prune."
done
exit 1
