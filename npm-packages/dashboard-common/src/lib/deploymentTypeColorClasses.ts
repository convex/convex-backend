import {
  DeploymentType,
  PlatformDeploymentResponse,
} from "@convex-dev/platform/managementApi";

export function deploymentIdentityLabel(
  deployment: PlatformDeploymentResponse,
): string {
  return deployment.kind === "cloud" ? deployment.reference : deployment.name;
}

// The capitalized, human-readable name for a deployment type (e.g.
// "Production"). Lowercase it for mid-sentence use.
export function deploymentTypeLabel(deploymentType: DeploymentType): string {
  switch (deploymentType) {
    case "prod":
      return "Production";
    case "preview":
      return "Preview";
    case "dev":
      return "Development";
    case "custom":
      return "Custom";
    default: {
      deploymentType satisfies never;
      return "";
    }
  }
}

export function deploymentTypeColorClasses(
  deploymentType: DeploymentType,
): string {
  switch (deploymentType) {
    case "prod":
      return "[--bg-opacity:100%] border-purple-600 dark:border-purple-100 bg-purple-100/(--bg-opacity) text-purple-800 dark:bg-purple-700/(--bg-opacity) dark:text-purple-100";
    case "preview":
      return "[--bg-opacity:100%] border-orange-600 dark:border-orange-400 bg-orange-100/(--bg-opacity) text-orange-800 dark:bg-orange-900/(--bg-opacity) dark:text-orange-400";
    case "dev":
      return "[--bg-opacity:100%] border-green-600 dark:border-green-400 bg-green-100/(--bg-opacity) text-green-800 dark:bg-green-900/(--bg-opacity) dark:text-green-400";
    case "custom":
      return "[--bg-opacity:100%] border-neutral-4 dark:border-neutral-6 bg-neutral-1/(--bg-opacity) text-neutral-11 dark:bg-neutral-12/(--bg-opacity) dark:text-neutral-2";
    default: {
      deploymentType satisfies never;
      return "";
    }
  }
}
