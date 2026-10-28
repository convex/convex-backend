import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { Loading } from "@ui/Loading";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { Backups } from "components/deploymentSettings/Backups";
import { useCurrentDeployment } from "api/deployments";
import { useCurrentTeam, useTeamEntitlements } from "api/teams";
import { useCurrentProject } from "api/projects";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { deploymentResource } from "lib/permissions";

export { getServerSideProps } from "lib/ssr";

function BackupPage() {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const deployment = useCurrentDeployment();
  const entitlements = useTeamEntitlements(team?.id);

  const resource =
    project && deployment && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  const isAdmin = useHasProjectAdminPermissions(project?.id);
  const canViewCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:backups:view",
    resource,
    true,
  );
  const canView = isAdmin || canViewCustom;

  return (
    <DeploymentSettingsLayout page="backups">
      {canView === false ? (
        <NoPermissionMessage
          message="You do not have permission to view backups."
          missingPermission="deployment:backups:view"
        />
      ) : team && deployment && entitlements ? (
        <div className="h-full animate-fadeInFromLoading">
          <Backups
            team={team}
            deployment={deployment}
            entitlements={entitlements}
          />
        </div>
      ) : (
        <Loading />
      )}
    </DeploymentSettingsLayout>
  );
}

export default withAuthenticatedPage(BackupPage);
