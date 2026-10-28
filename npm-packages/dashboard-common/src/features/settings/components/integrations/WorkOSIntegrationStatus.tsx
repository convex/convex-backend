import { AuthIntegration } from "@common/lib/integrationHelpers";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { useContext, useMemo } from "react";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { HelpTooltip } from "@ui/HelpTooltip";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";

export function WorkOSIntegrationStatus({
  integration,
}: {
  integration: AuthIntegration;
}) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewEnv = useIsOperationAllowed("ViewEnvironmentVariables");
  const environmentVariables = useQuery(
    udfs.listEnvironmentVariables.default,
    canViewEnv ? {} : "skip",
  );

  const workosEnvVars = useMemo(() => {
    if (!environmentVariables) return null;

    const clientId = environmentVariables.find(
      (envVar) => envVar.name === "WORKOS_CLIENT_ID",
    )?.value;

    return {
      clientId: clientId || null,
    };
  }, [environmentVariables]);

  if (!integration.existing) {
    return null;
  }

  // Check if environment variables match
  const clientIdMatches =
    workosEnvVars?.clientId &&
    workosEnvVars.clientId === integration.existing.workosClientId;

  const isConnected = clientIdMatches;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-col items-center">
        <div className="ml-auto flex gap-2">
          {!canViewEnv && (
            <div className="flex gap-1 text-xs text-content-secondary">
              Status Unknown{" "}
              <HelpTooltip>
                <PermissionDeniedTip
                  message="You must be able to view environment variables to check this integration's status"
                  action={"deployment:env:view"}
                />
              </HelpTooltip>
            </div>
          )}
          {canViewEnv &&
            (isConnected ? (
              <div className="text-xs text-content-success">Active</div>
            ) : (
              <div className="text-xs text-content-warning">Misconfigured</div>
            ))}
        </div>
      </div>
    </div>
  );
}
