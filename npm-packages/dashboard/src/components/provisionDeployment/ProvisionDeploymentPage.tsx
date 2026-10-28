import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import { useDeployments, useProvisionDeployment } from "api/deployments";
import { useProfile } from "api/profile";
import { useRouter } from "next/router";
import { useEffect, useMemo, useRef } from "react";
import { Loading } from "@ui/Loading";
import type { RegionName } from "generatedApi";
import { ProvisionDeploymentForm } from "./ProvisionDeploymentForm";
import { ProvisioningLoading } from "./ProvisioningLoading";

export function ProvisionDeploymentPage({
  deploymentType,
}: {
  deploymentType: "prod" | "dev";
}) {
  const router = useRouter();
  const projectSlug = router.query.project as string;
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const projectId = project?.id ?? null;
  const projectURI = `/t/${team?.slug}/${projectSlug}`;

  const { deployments } = useDeployments(projectId ?? undefined);
  const member = useProfile();
  const defaultRegion = team?.defaultRegion;
  const provisionDeployment = useProvisionDeployment(projectId ?? 0);

  const existingDeployment = useMemo(() => {
    if (!deployments) return undefined;
    if (deploymentType === "prod") {
      return (
        deployments.find(
          (d) =>
            d.kind === "cloud" && d.deploymentType === "prod" && d.isDefault,
        ) ?? null
      );
    }
    if (member === undefined) return undefined;
    return (
      deployments.find(
        (d) =>
          d.kind === "cloud" &&
          d.deploymentType === "dev" &&
          d.isDefault &&
          d.creator === member?.id,
      ) ?? null
    );
  }, [deployments, deploymentType, member]);

  useEffect(() => {
    if (existingDeployment) {
      void router.replace(`${projectURI}/${existingDeployment.name}`);
    }
  }, [existingDeployment, projectURI, router]);

  // Auto-provision once when a default region is set.
  const autoProvisionStarted = useRef(false);
  useEffect(() => {
    if (projectId === null) return;
    if (!defaultRegion) return;
    if (existingDeployment !== null) return;

    if (autoProvisionStarted.current) return;
    autoProvisionStarted.current = true;

    void (async () => {
      const { name } = await provisionDeployment({
        type: deploymentType,
        region: defaultRegion as RegionName,
      });
      void router.replace(`${projectURI}/${name}`);
    })();
  }, [
    projectId,
    defaultRegion,
    deploymentType,
    projectURI,
    provisionDeployment,
    router,
    existingDeployment,
  ]);

  if (
    projectId === null ||
    deployments === undefined ||
    existingDeployment === undefined
  ) {
    return <Loading />;
  }

  if (defaultRegion) {
    return <ProvisioningLoading deploymentType={deploymentType} />;
  }

  return (
    <ProvisionDeploymentForm
      projectId={projectId}
      projectURI={projectURI}
      deploymentType={deploymentType}
    />
  );
}
