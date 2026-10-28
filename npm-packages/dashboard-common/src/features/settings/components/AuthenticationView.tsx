import { useContext } from "react";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";
import { AuthConfig } from "@common/features/settings/components/AuthConfig";

export function AuthenticationView() {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const canViewEnv = useIsOperationAllowed("ViewEnvironmentVariables");

  return (
    <DeploymentSettingsLayout page="authentication">
      {!canViewEnv ? (
        <NoPermissionMessage
          message="You do not have permission to view authentication config in this deployment."
          missingPermission="deployment:env:view"
        />
      ) : !canViewData ? (
        <NoPermissionMessage
          message="You do not have permission to view authentication config in this deployment."
          missingPermission="deployment:data:view"
        />
      ) : (
        <AuthConfig />
      )}
    </DeploymentSettingsLayout>
  );
}
