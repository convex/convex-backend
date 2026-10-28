#!/usr/bin/env bash
if [[ "${VERCEL_GIT_COMMIT_REF}" =~ ^(mergify|tmp-mergify)/ ]] ; then
    # Always skip temporary mergify branches.
    echo "🛑 - Build canceled on mergify branch"
    exit 0;
elif [[ "${VERCEL_ENV}" == "production" ]] ; then
    # Proceed with the build
    echo "✅ - Build can proceed on production branch"
    exit 1;
elif [[ "${VERCEL_GIT_COMMIT_REF}" == "release" ]] ; then
    # Proceed with the build
    echo "✅ - Build can proceed on release branch"
    exit 1;
else
    # Only build if dashboard packages or design system changed.
    git diff HEAD^ HEAD --quiet ../dashboard ../dashboard-common ../dashboard-self-hosted ../dashboard-storybook ../@convex-dev/design-system && echo "🛑 - Build canceled"
fi
