import { useContext } from "react";
import { LoadingTransition } from "@ui/Loading";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { useNents } from "@common/lib/useNents";
import { Components } from "@common/features/settings/components/Components";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";

export function ComponentsView() {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const { nents } = useNents();
  return (
    <DeploymentSettingsLayout page="components">
      {!canViewData ? (
        <NoPermissionMessage
          message="You do not have permission to view components in this deployment."
          missingPermission="deployment:data:view"
        />
      ) : (
        <LoadingTransition>
          {nents && <Components nents={nents} />}
        </LoadingTransition>
      )}
    </DeploymentSettingsLayout>
  );
}
