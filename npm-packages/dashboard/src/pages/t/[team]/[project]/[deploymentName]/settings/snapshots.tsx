import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

import { SnapshotExport } from "components/deploymentSettings/SnapshotExport";
import { SnapshotImport } from "components/deploymentSettings/SnapshotImport";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { useCurrentDeployment } from "api/deployments";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { deploymentResource } from "lib/permissions";

export { getServerSideProps } from "lib/ssr";

function SnapshotExportPage() {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const deployment = useCurrentDeployment();

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
    <DeploymentSettingsLayout page="snapshots">
      {canView === false ? (
        <NoPermissionMessage
          message="You do not have permission to view snapshots."
          missingPermission="deployment:backups:view"
        />
      ) : (
        <>
          <SnapshotExport />
          <SnapshotImport />
        </>
      )}
    </DeploymentSettingsLayout>
  );
}

export default withAuthenticatedPage(SnapshotExportPage);
