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
    # Only build if a commit on this branch since it forked from main changed
    # the dashboard packages or the design system. Vercel's clone is shallow,
    # so fetch exactly the branch's commits since main plus the one they fork
    # from, which is then the only commit without parents. Build when that
    # fails or is ambiguous, e.g. when main was merged into the branch.
    if ! git fetch --quiet --no-tags --shallow-exclude=main origin "${VERCEL_GIT_COMMIT_REF}" \
        || ! git fetch --quiet --no-tags --deepen=1 origin "${VERCEL_GIT_COMMIT_REF}" \
        || [[ $(git rev-list --max-parents=0 HEAD | wc -l) -ne 1 ]] ; then
        echo "✅ - Build can proceed: could not find where this branch forks from main"
        exit 1;
    fi
    base=$(git rev-list --max-parents=0 HEAD)
    echo "Comparing HEAD with ${base}, where this branch forks from main"
    git diff "${base}" HEAD --quiet -- ../dashboard ../dashboard-common ../dashboard-self-hosted ../dashboard-storybook ../@convex-dev/design-system && echo "🛑 - Build canceled"
fi
