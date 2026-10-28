import { captureException } from "@sentry/nextjs";
import { useCurrentTeam } from "api/teams";
import { useCurrentDeployment } from "api/deployments";
import { useCurrentProject } from "api/projects";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
  useMyCustomRoles,
} from "api/roles";
import {
  DEPLOYMENT_OP_TO_ACTION,
  deploymentResource,
  isReadOnlyAction,
} from "lib/permissions";
import type { RoleStatementAction } from "@convex-dev/platform/managementApi";
import type { DeploymentOp } from "system-udfs/convex/_system/server";

// Wrapper that gates UI on the current member's custom-role grants
// for the deployment implied by the current page context. The
// underlying `useHasCustomRolePermission` already short-circuits to
// `true` for `deployment:*` actions on local deployments.
export function useCustomRolePermission(
  action: RoleStatementAction,
  nonCustomRoleResult: boolean,
): boolean | undefined {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const deployment = useCurrentDeployment();
  const isProjectAdmin = useHasProjectAdminPermissions(project?.id);
  const resource =
    project && deployment && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  const customRoleResult = useHasCustomRolePermission(
    team?.id,
    action,
    resource,
    nonCustomRoleResult,
  );
  // Project admin bypasses custom-role clamping for deployment ops.
  if (isProjectAdmin) return nonCustomRoleResult;
  return customRoleResult;
}

export function useIsActionAllowedForBuiltinRole(
  action: RoleStatementAction,
): boolean {
  const deployment = useCurrentDeployment();
  const isProjectAdmin = useHasProjectAdminPermissions(deployment?.projectId);
  const isProdDeployment = deployment?.deploymentType === "prod";
  const isReadAction = isReadOnlyAction(action);
  return isProjectAdmin || !isProdDeployment || isReadAction;
}

export function useIsOperationAllowed(
  operation: DeploymentOp,
): boolean | undefined {
  const action = DEPLOYMENT_OP_TO_ACTION[operation];

  if (action === undefined) {
    captureException(`Invalid DeploymentOp ${operation}`);
  }

  const allowedForNonCustomRole = useIsActionAllowedForBuiltinRole(action);

  return useCustomRolePermission(action, allowedForNonCustomRole) ?? undefined;
}

export function useHasCustomRole(): boolean {
  const team = useCurrentTeam();
  const myRoles = useMyCustomRoles(team?.id);
  return myRoles?.role === "custom";
}
