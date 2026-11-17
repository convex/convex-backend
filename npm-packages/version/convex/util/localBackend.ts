import { fetchAllGitHubReleases, findReleaseWithAllAssets } from "./github";

const REPO_PATH = "convex/convex-backend";
const REQUIRED_ASSETS = [
  "convex-local-backend-aarch64-apple-darwin.zip",
  "convex-local-backend-x86_64-apple-darwin.zip",
  "convex-local-backend-aarch64-unknown-linux-gnu.zip",
  "convex-local-backend-x86_64-unknown-linux-gnu.zip",
  "convex-local-backend-x86_64-pc-windows-msvc.zip",
  "dashboard.zip",
];

export async function getLatestLocalBackendVersion(): Promise<string> {
  const releases = await fetchAllGitHubReleases(REPO_PATH);
  const release = findReleaseWithAllAssets(releases, REQUIRED_ASSETS);
  if (!release) {
    throw new Error(
      "Found no stable release with all required local backend assets.",
    );
  }
  return release.tag_name;
}
