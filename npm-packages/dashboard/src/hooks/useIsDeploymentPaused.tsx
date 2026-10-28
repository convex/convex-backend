import udfs from "@common/udfs";
import { useQuery } from "convex/react";
import { useContext } from "react";
import { useCurrentTeam } from "api/teams";
import { useTeamUsageState } from "api/usage";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export function useIsDeploymentPaused() {
  const currentTeam = useCurrentTeam();
  const teamState = useTeamUsageState(currentTeam?.id ?? null);
  const { useIsOperationAllowed } = useContext(DeploymentInfoContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const deploymentState = useQuery(
    udfs.deploymentState.deploymentState,
    canViewData ? {} : "skip",
  );

  if (
    currentTeam === undefined ||
    teamState === undefined ||
    (canViewData && deploymentState === undefined)
  ) {
    return undefined;
  }

  return (
    teamState === "Paused" ||
    teamState === "Disabled" ||
    deploymentState?.state === "paused"
  );
}
