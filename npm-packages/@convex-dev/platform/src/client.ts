import createClient from "openapi-fetch";

import type { paths as ConvexManagementPaths } from "./generatedManagementApi.js";
import type { paths as ConvexDeploymentPaths } from "./generatedDeploymentApi.js";
import type { paths as ConvexEventLogPaths } from "./generatedLogStreamApi.js";
import { version } from "./version.js";

type ConvexEventLog = ConvexEventLogPaths["schemas"];
export type { ConvexEventLog };

export const productionProvisionHost = "https://api.convex.dev";
export const provisionHost =
  (globalThis as any)?.process?.env?.CONVEX_PROVISION_HOST ||
  productionProvisionHost;

export type ConvexAuth = {
  kind: "accessToken";
  accessToken: string;
};

export function createManagementClient(accessToken: string) {
  const baseUrl = `${provisionHost}/v1`;

  const auth = {
    kind: "accessToken",
    accessToken,
  };

  const headers: Record<string, string> = {
    // Yep, even API keys go use Bearer. Everyone else does it.
    Authorization: `Bearer ${auth.accessToken}`,
    "Convex-Client": `convex-platform-${version}`,
  };

  const client = createClient<ConvexManagementPaths>({
    baseUrl,
    headers,
  });
  return client;
}

/**
 * A bare deployment name assumes US East (N. Virginia). For any other region,
 * pass the full URL, such as `https://<name>.eu-west-1.convex.cloud`.
 */
export function createDeploymentClient(nameOrUrl: string, token: string) {
  const deploymentUrl = nameOrUrl.startsWith("http")
    ? nameOrUrl
    : `https://${nameOrUrl}.convex.cloud`;
  const baseUrl = `${deploymentUrl}/api/v1`;

  const headers: Record<string, string> = {
    // Yep, even API keys go use Bearer. Everyone else does it.
    Authorization: `Convex ${token}`,
    "Convex-Client": `convex-platform-${version}`,
  };

  const client = createClient<ConvexDeploymentPaths>({
    baseUrl,
    headers,
  });
  return client;
}
