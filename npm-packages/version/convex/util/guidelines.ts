import {
  downloadAssetFromRelease,
  fetchAllGitHubReleases,
  findReleaseWithAsset,
} from "./github";

const GUIDELINES_FILE_NAME = "convex_rules.txt";

export async function getLatestGuidelines() {
  const repoPath = "convex/convex-evals";

  // Fetch all releases from GitHub
  const releases = await fetchAllGitHubReleases(repoPath);

  // Find the first stable release with the guidelines file
  const targetRelease = findReleaseWithAsset(releases, GUIDELINES_FILE_NAME);

  if (!targetRelease) {
    throw new Error(`Found no stable releases with a ${GUIDELINES_FILE_NAME}.`);
  }

  // Download the guidelines file
  const content = await downloadAssetFromRelease(
    repoPath,
    targetRelease.tag_name,
    GUIDELINES_FILE_NAME,
  );

  return { content, version: targetRelease.tag_name };
}
