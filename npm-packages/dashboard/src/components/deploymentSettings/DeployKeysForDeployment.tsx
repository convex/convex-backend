import { useCurrentDeployment } from "api/deployments";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import {
  useCreateDeployKey,
  useDeleteDeployKey,
  useDeployKeys,
} from "api/accessTokens";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { useProfile } from "api/profile";
import { deploymentTokenResource } from "lib/permissions";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { Link } from "@ui/Link";

import { DeploymentAccessTokenList } from "./DeploymentAccessTokenList";
import { DeployKeyAction } from "./GenerateDeployKeyButton";
import { DEPLOYMENT_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export function DeployKeysForDeployment() {
  const project = useCurrentProject();
  const team = useCurrentTeam();
  const profile = useProfile();
  const hasAdminPermissions = useHasProjectAdminPermissions(project?.id);
  const deployment = useCurrentDeployment();
  const deploymentType = deployment?.deploymentType ?? "prod";
  const isProd = deploymentType === "prod";

  // Deployment-token resources are scoped under the project+deployment
  // segments (canonical path `project:*:deployment:*:token:*`).
  //
  // Two resource shapes:
  // - `*-Any` uses creator=null, which means a `creator=me`-restricted
  //   role *won't* match. We use this for whole-list operations (view,
  //   delete) since the deploy-keys list shows every member's tokens —
  //   a role that only grants "view your own" shouldn't let you see
  //   the full list.
  // - `*-Own` uses the current member's id, so a `creator=me` role
  //   *does* match. We use this for create, where the actor becomes
  //   the token's creator.
  const tokenResourceAny =
    project && deployment && deployment.kind === "cloud"
      ? deploymentTokenResource(
          project,
          {
            id: deployment.id,
            deploymentType: deployment.deploymentType,
            creator: deployment.creator ?? null,
          },
          null,
        )
      : undefined;
  const tokenResourceOwn =
    project && deployment && deployment.kind === "cloud"
      ? deploymentTokenResource(
          project,
          {
            id: deployment.id,
            deploymentType: deployment.deploymentType,
            creator: deployment.creator ?? null,
          },
          profile?.id ?? null,
        )
      : undefined;

  // Built-in admin/developer members keep the historical prod-only
  // gate (developers can view/create/delete deploy keys on non-prod;
  // admins anywhere — prod deploy keys are admin-only because they
  // grant full prod access). Custom-role members start with no
  // permissions, so a `deployment:token:*` grant is required on every
  // deployment type, not just prod.
  const canViewCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:token:view",
    tokenResourceAny,
    !isProd,
  );
  const canCreateCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:token:create",
    tokenResourceOwn,
    !isProd,
  );
  const canDeleteCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:token:delete",
    tokenResourceAny,
    !isProd,
  );

  const canView = hasAdminPermissions || canViewCustom !== false;
  const canCreate = hasAdminPermissions || canCreateCustom === true;
  const canDelete = hasAdminPermissions || canDeleteCustom === true;
  const disabledReason = !canCreate
    ? "CannotManageDeployment"
    : deployment?.kind === "local"
      ? "LocalDeployment"
      : null;

  const createDeployKey = useCreateDeployKey(deployment?.name || "");
  const deleteDeployKey = useDeleteDeployKey(deployment?.name || "");

  // Skip the list query when the member can't view tokens or the
  // deployment isn't cloud-backed (local deployments don't have a list
  // endpoint). Don't gate on `disabledReason` — a member who can view
  // but not create (e.g. a built-in developer on prod) still needs the
  // list to render their read-only view, otherwise the UI spins
  // forever waiting on a query that never fires.
  const deployKeys = useDeployKeys(
    canView && deployment?.kind === "cloud" ? deployment?.name : undefined,
  );

  if (canView === false) {
    return (
      <div id={DEPLOYMENT_SETTINGS_SECTIONS.deployKeys.id} className="w-full">
        <div className="mb-2 flex w-full items-center justify-between">
          <h4>Deploy Keys</h4>
        </div>
        <NoPermissionMessage
          message="You do not have permission to view deploy keys in this deployment."
          missingPermission="deployment:token:view"
        />
      </div>
    );
  }

  const deployKeyDescription = (
    <p className="mb-2 max-w-prose text-content-primary">
      Generate a deploy key to configure Convex integrations, such as
      automatically deploying to a{" "}
      <Link
        passHref
        href="https://docs.convex.dev/production/hosting"
        target="_blank"
      >
        hosting provider
      </Link>{" "}
      (like Netlify or Vercel) or syncing data with{" "}
      <Link
        passHref
        href="https://docs.convex.dev/database/import-export/streaming"
        target="_blank"
      >
        Fivetran or Airbyte
      </Link>
      .
    </p>
  );
  return (
    <div id={DEPLOYMENT_SETTINGS_SECTIONS.deployKeys.id} className="w-full">
      {team && deployment && (
        <DeploymentAccessTokenList
          header="Deploy Keys"
          buttonProps={{
            deploymentType,
            getAdminKey: async (
              name: string,
              allowedActions: DeployKeyAction[] | undefined,
              expiresAt: number | undefined,
            ) => {
              try {
                const result = await createDeployKey({
                  name,
                  allowedActions,
                  ...(expiresAt !== undefined && { expiresAt }),
                });
                if (!result)
                  return {
                    ok: false as const,
                    error: "Failed to create deploy key.",
                  };
                return { ok: true as const, adminKey: result.deployKey };
              } catch (e) {
                return {
                  ok: false as const,
                  error:
                    (e as { message?: string })?.message ??
                    "Failed to create deploy key.",
                };
              }
            },
            disabledReason,
          }}
          description={deployKeyDescription}
          deploymentType={deploymentType}
          onDelete={deleteDeployKey}
          canDelete={canDelete}
          deployKeys={deployKeys}
          disabledReason={disabledReason}
        />
      )}
    </div>
  );
}
