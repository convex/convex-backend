import { useEffect } from "react";
import { useLocalStorage } from "react-use";
import { z } from "zod";
import {
  DeploymentType,
  PlatformDeploymentResponse,
} from "@convex-dev/platform/managementApi";
import { ProjectDetails } from "generatedApi";

export function useLastViewedTeam() {
  return useLocalStorage<string>(`/lastViewedTeam`);
}

export function useRememberLastViewedTeam(slug: string | undefined) {
  const [, setLastViewedTeam] = useLastViewedTeam();
  useEffect(() => {
    if (slug !== undefined) {
      setLastViewedTeam(slug);
    }
  }, [slug, setLastViewedTeam]);
}

export function useLastViewedProject() {
  return useLocalStorage<string>(`/lastViewedProject`);
}

export function useRememberLastViewedProject(slug: string | undefined) {
  const [, setLastViewedProject] = useLastViewedProject();
  useEffect(() => {
    if (slug !== undefined) {
      setLastViewedProject(slug);
    }
  }, [slug, setLastViewedProject]);
}

export function useLastViewedDeployment() {
  return useLocalStorage<string>(`/lastViewedDeployment`);
}

export function useLastViewedDeploymentForProject(projectSlug: string) {
  return useLocalStorage<string>(
    `/lastViewedDeploymentForProject/${projectSlug}`,
  );
}

export function useRememberLastViewedDeploymentForProject(
  projectSlug: string,
  name: string | undefined,
) {
  const [, setLastViewedDeploymentForProject] =
    useLastViewedDeploymentForProject(projectSlug);
  useRememberLastViewedDeployment(name);
  useEffect(() => {
    if (name !== undefined) {
      setLastViewedDeploymentForProject(name);
    }
  }, [name, setLastViewedDeploymentForProject]);
}

function useRememberLastViewedDeployment(name: string | undefined) {
  const [, setLastViewedDeployment] = useLastViewedDeployment();
  useEffect(() => {
    if (name !== undefined) {
      setLastViewedDeployment(name);
    }
  }, [name, setLastViewedDeployment]);
}

export type LastViewedDeployment = {
  name: string;
  reference: string;
  deploymentType: DeploymentType;
  // Set once Big Brain reports the deployment missing, so the project card
  // stops fetching it and shows it as deleted.
  deleted?: boolean;
};

const lastViewedDeploymentSchema: z.ZodType<LastViewedDeployment> = z.object({
  name: z.string(),
  reference: z.string(),
  deploymentType: z.enum(["dev", "prod", "preview", "custom"]),
  deleted: z.boolean().optional(),
});

function lastViewedNonDefaultDeploymentKey(projectId: number) {
  return `/lastViewedNonDefaultDeploymentForProject/${projectId}`;
}

/**
 * The cloud deployment most recently opened in a project, other than the
 * project's default prod and dev deployments.
 */
export function useLastViewedNonDefaultDeployment(projectId: number) {
  const [stored, setStored] = useLocalStorage<LastViewedDeployment>(
    lastViewedNonDefaultDeploymentKey(projectId),
  );
  // Local storage may hold another shape, e.g. the bare name an earlier build
  // stored. Returns `stored` itself rather than the parsed copy so its identity
  // stays stable across renders.
  const lastViewed = lastViewedDeploymentSchema.safeParse(stored).success
    ? stored
    : undefined;
  return [lastViewed, setStored] as const;
}

export function useRememberLastViewedNonDefaultDeployment(
  project: ProjectDetails,
  deployment: PlatformDeploymentResponse | undefined,
) {
  const [stored, setStored] = useLastViewedNonDefaultDeployment(project.id);
  const name = deployment?.name;
  // Local deployments aren't stored: the card looks the deployment up by
  // name, and that endpoint only serves cloud deployments.
  const reference = deployment?.kind === "cloud" ? deployment.reference : null;
  const deploymentType = deployment?.deploymentType;
  // The same defaults ProjectCard already links to, so the stored deployment
  // is always one it can suggest alongside them.
  const isDefault =
    name === project.prodDeploymentName || name === project.devDeploymentName;
  useEffect(() => {
    if (
      name === undefined ||
      reference === null ||
      deploymentType === undefined ||
      isDefault
    ) {
      return;
    }
    // Writing updates `stored`, so skip writes that wouldn't change it.
    if (
      stored?.name === name &&
      stored.reference === reference &&
      stored.deploymentType === deploymentType &&
      !stored.deleted
    ) {
      return;
    }
    setStored({ name, reference, deploymentType });
  }, [name, reference, deploymentType, isDefault, stored, setStored]);
}
