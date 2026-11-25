#!/usr/bin/env bash
# Gives a Vercel build's clone an `origin` that can fetch from GitHub.
#
# Vercel clones without a remote, and the repository is private, but
# should-build.sh fetches `main` and scripts/reference-source.js fetches the
# release tag from `origin`.
#
# CONVEX_DOCS_GITHUB_TOKEN is a Secret environment variable of the Vercel
# project, for Production and Preview. It holds a fine-grained personal access
# token (https://github.com/settings/personal-access-tokens/new) with resource
# owner `convex`, access to only `convex/convex`, and the Contents permission,
# read-only. It expires after a year: to renew it, create one with the same
# settings, replace the variable's value, and redeploy. It's sent as a header,
# as actions/checkout does, so it never appears in the remote's URL or in git's
# error messages.
#
# Netlify's clone already has an authenticated `origin`, so this does nothing
# outside Vercel. Running it again is harmless.

if [ -z "$VERCEL" ]; then
  exit 0
fi

cd "$(git rev-parse --show-toplevel)" || exit 1

if ! git remote get-url origin >/dev/null 2>&1; then
  git remote add origin \
    "https://github.com/$VERCEL_GIT_REPO_OWNER/$VERCEL_GIT_REPO_SLUG.git"
fi

if [ -z "$CONVEX_DOCS_GITHUB_TOKEN" ]; then
  echo "❌ CONVEX_DOCS_GITHUB_TOKEN is not set in the Vercel project." >&2
  echo "   See npm-packages/docs/scripts/vercel-git-origin.sh." >&2
  exit 1
fi

credentials=$(printf "x-access-token:%s" "$CONVEX_DOCS_GITHUB_TOKEN" | base64 | tr -d "\n")
git config --local http.https://github.com/.extraheader \
  "AUTHORIZATION: basic $credentials"
