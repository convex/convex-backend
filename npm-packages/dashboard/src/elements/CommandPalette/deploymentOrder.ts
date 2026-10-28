import type { PlatformDeploymentResponse } from "generatedApi";

// The switcher's group into which a deployment falls, in display order:
// production, then your own dev deployments, then teammates' devs, previews,
// and custom deployments.
function switcherDeploymentRank(
  deployment: PlatformDeploymentResponse,
  memberId: number | undefined,
): number {
  switch (deployment.deploymentType) {
    case "prod":
      return 0;
    case "dev":
      return deployment.creator === memberId ? 1 : 2;
    case "preview":
      return 3;
    case "custom":
      return 4;
    default:
      return 5;
  }
}

export function compareSwitcherDeployments(memberId: number | undefined) {
  return (a: PlatformDeploymentResponse, b: PlatformDeploymentResponse) => {
    const rankA = switcherDeploymentRank(a, memberId);
    const rankB = switcherDeploymentRank(b, memberId);
    if (rankA !== rankB) {
      return rankA - rankB;
    }
    // Production (cloud only): the default prod first, then newest first.
    if (
      a.kind === "cloud" &&
      b.kind === "cloud" &&
      a.deploymentType === "prod"
    ) {
      if (a.isDefault !== b.isDefault) {
        return a.isDefault ? -1 : 1;
      }
      return b.createTime - a.createTime;
    }
    // Your own dev deployments: local ones ahead of cloud, then oldest first.
    if (rankA === 1) {
      if ((a.kind === "local") !== (b.kind === "local")) {
        return a.kind === "local" ? -1 : 1;
      }
      return a.createTime - b.createTime;
    }
    if (a.deploymentType === "preview" && b.deploymentType === "preview") {
      return (a.previewIdentifier?.toLowerCase() ?? "").localeCompare(
        b.previewIdentifier?.toLowerCase() ?? "",
      );
    }
    // Teammates' devs and custom deployments, newest first.
    return b.createTime - a.createTime;
  };
}
