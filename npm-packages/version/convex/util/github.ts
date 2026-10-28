import { env } from "../_generated/server";

export type GitHubRelease = {
  tag_name: string;
  prerelease: boolean;
  draft: boolean;
  assets: { name: string }[];
};

type LinkHeader = {
  prev?: string;
  next?: string;
  first?: string;
  last?: string;
};

/**
 * Get headers for GitHub API requests with authentication if available
 */
export function getGitHubHeaders(): HeadersInit {
  const githubToken = env.GITHUB_TOKEN;
  if (!githubToken) {
    console.warn(
      "GITHUB_TOKEN environment variable not set. GitHub API requests will have lower rate limits.",
    );
  }

  return githubToken
    ? {
        Authorization: `Bearer ${githubToken}`,
        Accept: "application/vnd.github+json",
      }
    : {
        Accept: "application/vnd.github+json",
      };
}

/**
 * Parse the HTTP Link header for pagination
 * https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api?apiVersion=2022-11-28#using-link-headers
 */
export function parseLinkHeader(header: string): LinkHeader {
  const links: { [key: string]: string } = {};
  const parts = header.split(",");
  for (const part of parts) {
    const section = part.split(";");
    if (section.length !== 2) {
      continue;
    }
    const url = section[0].trim().slice(1, -1);
    const rel = section[1].trim().slice(5, -1);
    links[rel] = url;
  }
  return links;
}

/**
 * Fetch all GitHub releases for a repository with pagination
 */
export async function fetchAllGitHubReleases(
  repoPath: string,
  perPage: number = 30,
): Promise<GitHubRelease[]> {
  const allReleases: GitHubRelease[] = [];
  let nextUrl = `https://api.github.com/repos/${repoPath}/releases?per_page=${perPage}`;

  while (nextUrl) {
    const response = await fetch(nextUrl, { headers: getGitHubHeaders() });

    if (!response.ok) {
      const text = await response.text();
      throw new Error(`GitHub API returned ${response.status}: ${text}`);
    }

    const releases = (await response.json()) as GitHubRelease[];
    if (releases.length === 0) {
      break;
    }

    allReleases.push(...releases);

    // Get the next page URL from the Link header
    const linkHeader = response.headers.get("Link");
    if (!linkHeader) {
      break;
    }

    const links = parseLinkHeader(linkHeader);
    nextUrl = links["next"] || "";
  }

  return allReleases;
}

/**
 * Find the first stable release that contains a specific asset
 */
export function findReleaseWithAsset(
  releases: GitHubRelease[],
  assetName: string,
  options: { verbose?: boolean } = {},
): GitHubRelease | null {
  for (const release of releases) {
    // Only consider stable releases
    if (!release.prerelease && !release.draft) {
      // Check if this release has the asset
      if (release.assets.find((asset) => asset.name === assetName)) {
        if (options.verbose) {
          console.log(
            `Latest stable version with ${assetName} is ${release.tag_name}`,
          );
        }
        return release;
      } else {
        if (options.verbose) {
          console.log(
            `Version ${release.tag_name} does not contain a ${assetName}, checking previous version`,
          );
          console.log(
            `ASSETS: ${release.assets.map((asset) => asset.name).join(", ")}`,
          );
        }
      }
    }
  }

  return null;
}

/**
 * Find the first stable release that contains all specified assets
 */
export function findReleaseWithAllAssets(
  releases: GitHubRelease[],
  assetNames: string[],
): GitHubRelease | null {
  for (const release of releases) {
    if (release.prerelease || release.draft) continue;
    const releaseAssetNames = new Set(release.assets.map((a) => a.name));
    const matching = assetNames.filter((name) => releaseAssetNames.has(name));
    if (matching.length === assetNames.length) {
      return release;
    }
  }
  return null;
}

/**
 * Download an asset from a GitHub release
 */
export async function downloadAssetFromRelease(
  repoPath: string,
  version: string,
  assetName: string,
): Promise<string> {
  const downloadUrl = `https://github.com/${repoPath}/releases/download/${version}/${assetName}`;

  const response = await fetch(downloadUrl, { headers: getGitHubHeaders() });

  if (!response.ok) {
    throw new Error(`Failed to download ${assetName} from ${downloadUrl}`);
  }

  return await response.text();
}
