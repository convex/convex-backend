import { GoogleAnalytics } from "elements/GoogleAnalytics";

import { useCurrentDeployment, useDeployments } from "api/deployments";
import { useTeamEntitlements } from "api/teams";
import { useCurrentProject } from "api/projects";
import { useAccessToken } from "hooks/useServerSideData";
import { useRouter } from "next/router";
import { JSX, useEffect } from "react";
import { usePostHog } from "hooks/usePostHog";
import {
  useGlobalLDContext,
  useLDContextWithDeployment,
} from "hooks/useLaunchDarklyContext";
import { WaitForDeploymentApi } from "@common/lib/deploymentContext";
import { DeploymentDashboardLayout as CommonDeploymentDashboardLayout } from "@common/layouts/DeploymentDashboardLayout";

type LayoutProps = {
  children: JSX.Element;
};

export function CurrentDeploymentDashboardLayout({ children }: LayoutProps) {
  const [accessToken] = useAccessToken();

  return accessToken ? (
    <CurrentDeploymentDashboardLayoutWhenLoggedIn>
      {children}
    </CurrentDeploymentDashboardLayoutWhenLoggedIn>
  ) : (
    // Render the page without the layout so the page can get it's server side props.
    children
  );
}

function CurrentDeploymentDashboardLayoutWhenLoggedIn({
  children,
}: LayoutProps & {}) {
  const router = useRouter();
  const { query } = router;
  const projectsURI = `/t/${query.team}/${query.project}`;

  const project = useCurrentProject();
  const { deployments } = useDeployments(project?.id);
  const currentDeployment = useCurrentDeployment();
  const isLoading = deployments === undefined;

  const entitlements = useTeamEntitlements(project?.teamId);
  const auditLogsEnabled =
    entitlements && entitlements.auditLogRetentionDays !== 0;

  const { capture } = usePostHog();

  useEffect(() => {
    if (
      !isLoading &&
      query.deploymentName !== undefined &&
      currentDeployment === undefined
    ) {
      // This deployment does not exist (probably deactivated), so navigate away
      void router.push(projectsURI);
    }
  });

  return (
    <WaitForDeploymentApi>
      <GoogleAnalytics />
      <LaunchDarklyWithDeployment>
        <CommonDeploymentDashboardLayout
          auditLogsEnabled={auditLogsEnabled}
          onRanCustomQuery={() => capture("ran_custom_query")}
          onCopiedQueryResult={() => capture("copied_query_result")}
        >
          {children}
        </CommonDeploymentDashboardLayout>
      </LaunchDarklyWithDeployment>
    </WaitForDeploymentApi>
  );
}

function LaunchDarklyWithDeployment({
  children,
}: {
  children: React.ReactElement;
}) {
  const [, setContext] = useGlobalLDContext();
  const localContext = useLDContextWithDeployment();
  useEffect(() => {
    if (localContext) {
      setContext(localContext);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(localContext), setContext]);

  return children;
}
