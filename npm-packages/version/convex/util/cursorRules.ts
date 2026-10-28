import {
  downloadAssetFromRelease,
  fetchAllGitHubReleases,
  findReleaseWithAsset,
} from "./github";

const CURSOR_RULES_FILE_NAME = "convex_rules.mdc";

export async function getLatestCursorRules() {
  const repoPath = "convex/convex-evals";

  // Fetch all releases from GitHub
  const releases = await fetchAllGitHubReleases(repoPath);

  // Find the first stable release with the cursor rules file
  const targetRelease = findReleaseWithAsset(releases, CURSOR_RULES_FILE_NAME);

  if (!targetRelease) {
    throw new Error(
      `Found no stable releases with a ${CURSOR_RULES_FILE_NAME}.`,
    );
  }

  // Download the cursor rules file
  const content = await downloadAssetFromRelease(
    repoPath,
    targetRelease.tag_name,
    CURSOR_RULES_FILE_NAME,
  );

  return { content, version: targetRelease.tag_name };
}
