import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { Sheet } from "@ui/Sheet";
import { DeployKeysForDeployment } from "components/deploymentSettings/DeployKeysForDeployment";
import { useCurrentDeployment, useDeploymentRegions } from "api/deployments";
import { useRouter } from "next/router";
import { usePathname } from "next/navigation";
import { DeleteDeployment } from "components/deploymentSettings/DeleteDeployment";
import { TransferDeployment } from "components/deploymentSettings/TransferDeployment";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { DeploymentAdvancedSettings } from "components/deploymentSettings/DeploymentAdvancedSettings";
import { PauseDeployment } from "@common/features/settings/components/PauseDeployment";
import { DeploymentSummary } from "@common/features/health/components/DeploymentSummary";
import { useScrollToHash } from "@common/lib/useScrollToHash";
import { usePostHog } from "hooks/usePostHog";
import { useCurrentTeam, useTeamMembers } from "api/teams";
import { useCurrentProject } from "api/projects";
import { useListCloudBackupsIfAvailable } from "api/backups";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { useContext, useMemo, useRef } from "react";
import { DEPLOYMENT_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export { getServerSideProps } from "lib/ssr";

export function DeploymentSettingsPage() {
  const router = useRouter();
  const envVars = router.query.var;
  const pathname = usePathname();

  // If "var" is present as a query parameter, we route to settings/environment-variables since, previously,
  // all deployment settings were on the same page and this was handled without routing. We don't want
  // to break links to this so we just manually handle this here.
  if (envVars) {
    void router.push({
      pathname: `${pathname}/environment-variables`,
      query: { var: envVars },
    });
  }

  return (
    <DeploymentSettingsLayout page="general">
      <DeploymentURLAndDeployKey />
    </DeploymentSettingsLayout>
  );
}

export default withAuthenticatedPage(DeploymentSettingsPage);

function DeploymentURLAndDeployKey() {
  const deployment = useCurrentDeployment();
  const { capture } = usePostHog();
  const pauseDeploymentRef = useRef<HTMLDivElement | null>(null);
  useScrollToHash(
    `#${DEPLOYMENT_SETTINGS_SECTIONS.pauseDeployment.id}`,
    pauseDeploymentRef,
  );

  const team = useCurrentTeam();
  const project = useCurrentProject();
  const teamMembers = useTeamMembers(team?.id);
  const { regions } = useDeploymentRegions(team?.id);

  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewBackups = useIsOperationAllowed("ViewBackups");
  const backups = useListCloudBackupsIfAvailable(
    canViewBackups ? deployment : undefined,
  );

  // backups is null when not available (d1024, non-cloud), undefined when loading
  const lastBackupTime = useMemo(() => {
    if (backups === null) return null;
    if (backups === undefined) return undefined;
    const deploymentsBackups = backups.filter((b) => b.state === "complete");
    return deploymentsBackups.length > 0
      ? deploymentsBackups[0].requestedTime
      : null;
  }, [backups]);

  return (
    <div className="flex flex-col gap-4">
      {deployment && team?.slug && project?.slug && (
        <DeploymentSummary
          deployment={deployment}
          teamSlug={team.slug}
          projectSlug={project.slug}
          lastBackupTime={lastBackupTime}
          teamMembers={teamMembers}
          regions={regions}
        />
      )}
      <Sheet>
        <DeployKeysForDeployment />
      </Sheet>
      <DeploymentAdvancedSettings />
      <div
        id={DEPLOYMENT_SETTINGS_SECTIONS.pauseDeployment.id}
        ref={pauseDeploymentRef}
      >
        <PauseDeployment
          onPausedDeployment={() => {
            capture("paused_deployment");
          }}
        />
      </div>
      <DeleteDeployment />
      <TransferDeployment />
    </div>
  );
}
