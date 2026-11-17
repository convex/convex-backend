import { useRouter } from "next/router";
import { useCurrentTeam } from "api/teams";
import {
  PROVISION_PROD_PAGE_NAME,
  PROVISION_DEV_PAGE_NAME,
} from "@common/lib/deploymentContext";
import { useProjectById } from "api/projects";

export function useDeploymentUris(
  projectId: number,
  projectSlug: string,
  teamSlug?: string,
) {
  const router = useRouter();
  const subroute =
    router.route.split("/t/[team]/[project]/[deploymentName]")[1] || "/";
  const team = useCurrentTeam();
  const selectedTeamSlug = team?.slug;

  const { project, isLoading } = useProjectById(projectId);
  const prodDeploymentName = project?.prodDeploymentName ?? null;
  const devDeploymentName = project?.devDeploymentName ?? null;

  const projectURI = `/t/${teamSlug || selectedTeamSlug}/${projectSlug}`;

  const hasDefaultProdDeployment = prodDeploymentName !== null;
  const prodHref = hasDefaultProdDeployment
    ? `${projectURI}/${prodDeploymentName}${subroute}`
    : `${projectURI}/${PROVISION_PROD_PAGE_NAME}`;
  const hasDefaultDevDeployment = devDeploymentName !== null;
  const devHref = hasDefaultDevDeployment
    ? `${projectURI}/${devDeploymentName}${subroute}`
    : `${projectURI}/${PROVISION_DEV_PAGE_NAME}`;

  const isProdDefault = !devDeploymentName;

  return {
    isLoading,
    isProdDefault,
    prodDeploymentName,
    devDeploymentName,
    prodDeploymentReference: project?.prodDeploymentReference ?? null,
    devDeploymentReference: project?.devDeploymentReference ?? null,
    prodHref,
    devHref,
    hasDefaultProdDeployment,
    hasDefaultDevDeployment,
    defaultHref: hasDefaultDevDeployment ? devHref : prodHref,
    generateHref: (deploymentName: string) =>
      `${projectURI}/${deploymentName}${subroute}`,
  };
}
