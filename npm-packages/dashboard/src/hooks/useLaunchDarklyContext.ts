import { useRouter } from "next/router";
import { LDMultiKindContext } from "launchdarkly-js-sdk-common";
import { useContext, useMemo } from "react";
import { createGlobalState } from "react-use";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { useProfile } from "api/profile";
import { useCurrentTeam } from "api/teams";
import { useTeamOrbSubscription } from "api/billing";
import { useCurrentProject } from "api/projects";
import { useCurrentDeployment } from "api/deployments";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export const useGlobalLDContext = createGlobalState<
  LDMultiKindContext | undefined
>();

export const useLDContext = () => {
  const router = useRouter();
  const team = useCurrentTeam();
  const profile = useProfile();

  const project = useCurrentProject();

  // The subscription loads asynchronously; until it's available, the team's
  // plan type is left undefined and re-identified once it loads.
  const { subscription } = useTeamOrbSubscription(team?.id);

  return useMemo(() => {
    if (
      !router.isReady ||
      !profile ||
      (router.query.team && !team) ||
      (router.query.project && !project)
    ) {
      return undefined;
    }
    const ctx: LDMultiKindContext = {
      kind: "multi",
      user: {
        key: profile.id.toString(),
        email: profile.email,
        id: profile.id,
        _meta: {
          privateAttributes: ["email"],
        },
      },
    };
    if (team) {
      ctx.team = {
        key: team.id.toString(),
        name: team.name,
        slug: team.slug,
        planType: subscription?.plan.planType,
      };
    }
    if (project) {
      ctx.project = {
        key: project.id.toString(),
        name: project.name,
        slug: project.slug,
        createTime: project.createTime,
      };
    }

    return ctx;
  }, [
    router.isReady,
    router.query.team,
    router.query.project,
    profile,
    team,
    project,
    subscription,
  ]);
};

export const useLDContextWithDeployment = () => {
  const ctx = useLDContext();
  const { useIsOperationAllowed } = useContext(DeploymentInfoContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const serverVersion = useQuery(
    udfs.getVersion.default,
    canViewData ? undefined : "skip",
  );
  const deployment = useCurrentDeployment();

  if (!ctx || (canViewData && serverVersion === undefined) || !deployment) {
    return undefined;
  }

  ctx.deployment = {
    // The deployment name is unique.
    key: deployment.name,
    type: deployment.deploymentType,
    createTime: deployment.createTime,
    serverVersion: serverVersion ?? null,
    // Same as serverVersion, but renaming to npmPackageVersion.
    // Keeping both here for now to avoid breaking changes to existing
    // flag configs.
    npmPackageVersion: serverVersion,
  };
  return ctx;
};
