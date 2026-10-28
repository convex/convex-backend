import { useContext } from "react";
import { DeploymentPageTitle } from "@common/elements/DeploymentPageTitle";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { SchedulingLayout } from "@common/layouts/SchedulingLayout";
import { useCurrentOpenFunction } from "@common/lib/functions/FunctionsProvider";
import { ScheduledFunctionsContent } from "@common/features/schedules/components/ScheduledFunctionsContent";

export function ScheduledFunctionsView() {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const currentOpenFunction = useCurrentOpenFunction();

  if (!canViewData) {
    return (
      <>
        <DeploymentPageTitle title="Scheduled Functions" />
        <NoPermissionMessage
          message="You do not have permission to view scheduled functions in this deployment."
          missingPermission="deployment:data:view"
        />
      </>
    );
  }

  return (
    <SchedulingLayout>
      <DeploymentPageTitle title="Scheduled Functions" />
      <ScheduledFunctionsContent
        currentOpenFunction={currentOpenFunction ?? undefined}
        // Important! This key is used to reset the state of the component when the currentOpenFunction changes
        key={currentOpenFunction ? JSON.stringify(currentOpenFunction) : "all"}
      />
    </SchedulingLayout>
  );
}
