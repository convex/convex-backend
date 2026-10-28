import { DeploymentPageTitle } from "@common/elements/DeploymentPageTitle";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";
import { LoadingTransition } from "@ui/Loading";
import { PageContent } from "@common/elements/PageContent";
import { useNents } from "@common/lib/useNents";
import { Logs } from "@common/features/logs/components/Logs";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { useContext } from "react";

export function LogsView() {
  const { nents, selectedNent } = useNents();
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const deployment = useCurrentDeployment();
  const deploymentId = deployment && "id" in deployment ? deployment.id : null;
  const canViewLogs = useIsOperationAllowed("ViewLogs");

  if (!canViewLogs) {
    return (
      <>
        <DeploymentPageTitle title="Logs" />
        <NoPermissionMessage
          message="You do not have permission to view logs in this deployment."
          missingPermission="deployment:logs:view"
        />
      </>
    );
  }

  return (
    <PageContent key={deploymentId}>
      <DeploymentPageTitle title="Logs" />
      <LoadingTransition>
        {nents && <Logs nents={nents} selectedNent={selectedNent} />}
      </LoadingTransition>
    </PageContent>
  );
}
