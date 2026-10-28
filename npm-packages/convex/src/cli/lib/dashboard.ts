import { Context } from "../../bundler/context.js";
import { DeploymentType } from "./api.js";
import { dashboardUrl as localDashboardUrl } from "./localDeployment/dashboard.js";

export const DASHBOARD_HOST = process.env.CONVEX_PROVISION_HOST
  ? "http://localhost:6789"
  : "https://dashboard.convex.dev";

export async function getDashboardUrl(
  ctx: Context,
  {
    deploymentName,
    deploymentType,
  }: {
    deploymentName: string;
    deploymentType: DeploymentType;
  },
): Promise<string | null> {
  switch (deploymentType) {
    case "anonymous": {
      return localDashboardUrl(ctx, deploymentName);
    }
    case "local":
    case "dev":
    case "prod":
    case "preview":
    case "custom":
      return deploymentDashboardUrlPage(deploymentName, "");
    default: {
      deploymentType satisfies never;
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Unknown deployment type: ${deploymentType as any}`,
      });
    }
  }
}

// FIXME: this doesn't work correctly in local-dev/self-hosted, where the
// dashboard isn't at DASHBOARD_HOST and deployments aren't addressed by `/d/`.
export function deploymentDashboardUrlPage(
  configuredDeployment: string | null,
  page: string,
): string {
  const deploymentFrag = configuredDeployment
    ? `/d/${configuredDeployment}`
    : "";
  return `${DASHBOARD_HOST}${deploymentFrag}${page}`;
}

export function deploymentDashboardUrl(
  team: string,
  project: string,
  deploymentName: string,
) {
  return `${projectDashboardUrl(team, project)}/${deploymentName}`;
}

export function projectDashboardUrl(team: string, project: string) {
  return `${teamDashboardUrl(team)}/${project}`;
}

export function teamDashboardUrl(team: string) {
  return `${DASHBOARD_HOST}/t/${team}`;
}
