import {
  JSX,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import Link from "next/link";
import { useRouter } from "next/router";
import {
  captureException,
  captureMessage,
  addBreadcrumb,
  ErrorBoundary,
  FallbackRender,
} from "@sentry/nextjs";
import { reportHttpError } from "hooks/fetching";
import {
  DeploymentInfo,
  DeploymentInfoContext,
  ConnectedDeployment,
} from "@common/lib/deploymentContext";
import { LocalDeploymentDisconnectOverlay } from "@common/features/disconnectOverlay/LocalDeploymentDisconnectOverlay";
import { CloudDisconnectOverlay } from "@common/features/disconnectOverlay/CloudDisconnectOverlay";
import { useCurrentTeam, useTeamEntitlements, useTeamMembers } from "api/teams";
import { useCurrentDeployment, useDeployments } from "api/deployments";
import { useHasProjectAdminPermissions } from "api/roles";
import {
  useIsOperationAllowed,
  useHasCustomRole,
} from "hooks/useDeploymentPermissions";
import { useCurrentUsageBanner } from "components/header/UsageBanner";
import { useIsDeploymentPaused } from "hooks/useIsDeploymentPaused";
import { CloudImport } from "elements/BackupIdentifier";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { useLogDeploymentEvent } from "hooks/deploymentApi";
import { useAccessToken } from "hooks/useServerSideData";
import { Fallback } from "pages/500";
import { useTeamUsageState } from "api/usage";
import { useTeamOrbSubscription } from "api/billing";
import { useProjectEnvironmentVariables } from "api/environmentVariables";
import { useCurrentProject, useCurrentProjectWithStatus } from "api/projects";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { useMemberPreferences, useSetPreference } from "api/preferences";
import { PreferenceName } from "generatedApi";
import {
  useDeploymentWorkOSEnvironment,
  useTeamWorkOSIntegration,
  useWorkOSTeamHealth,
  useWorkOSEnvironmentHealth,
  useDisconnectWorkOSTeam,
  useInviteWorkOSTeamMember,
  useWorkOSInvitationEligibleEmails,
  useAvailableWorkOSTeamEmails,
  useProvisionWorkOSTeam,
  useProvisionWorkOSEnvironment,
  useDeleteWorkOSEnvironment,
  useProjectWorkOSEnvironments,
  useGetProjectWorkOSEnvironment,
  useCheckProjectEnvironmentHealth,
  useProvisionProjectWorkOSEnvironment,
  useDeleteProjectWorkOSEnvironment,
} from "api/workos";
import { useProfile } from "api/profile";
import { useSupportFormOpen } from "elements/SupportWidget";
import { useFeedbackFormOpen } from "elements/FeedbackForm";
import { usePostHog as usePostHogLib } from "posthog-js/react";
import { useConvexStatus } from "hooks/useConvexStatus";
import { ConvexStatusWidget } from "lib/ConvexStatusWidget";
import { localDeploymentAuth } from "lib/deploymentAuth";

// A silly, standard hack to dodge warnings about useLayoutEffect on the server.
const useIsomorphicLayoutEffect =
  typeof window !== "undefined" ? useLayoutEffect : useEffect;

function DeploymentErrorBoundary({
  children,
  fallback,
}: {
  children: React.ReactNode;
  fallback?: React.ReactElement | FallbackRender;
}) {
  return (
    <ErrorBoundary fallback={fallback ?? Fallback}>{children}</ErrorBoundary>
  );
}

function useCurrentMemberName() {
  return useProfile()?.name ?? undefined;
}

function CloudDashboardDisconnectOverlay({
  deployment,
  deploymentName,
}: {
  deployment: ConnectedDeployment;
  deploymentName: string;
}) {
  const [, setOpenState] = useSupportFormOpen();
  const { status } = useConvexStatus();

  const openSupportForm = (defaultSubject: string, defaultMessage: string) => {
    setOpenState({
      defaultSubject,
      defaultMessage,
    });
  };

  if (deploymentName.startsWith("local-")) {
    return <LocalDeploymentDisconnectOverlay />;
  }

  return (
    <CloudDisconnectOverlay
      deployment={deployment}
      deploymentName={deploymentName}
      openSupportForm={openSupportForm}
      statusWidget={
        <>
          <ConvexStatusWidget status={status} />
          {status?.indicator === "none" && (
            <p className="mt-2 text-xs text-content-secondary">
              For emerging issues, it may take the Convex team a few minutes to
              update system status.
            </p>
          )}
        </>
      }
    />
  );
}

export function DeploymentInfoProvider({
  children,
  deploymentOverride,
  deploymentUrlOverride,
}: {
  children: React.ReactNode;
  deploymentOverride?: string;
  deploymentUrlOverride?: string;
}): JSX.Element {
  const router = useRouter();
  const { deploymentName } = router.query;
  const [deploymentInfo, setDeploymentInfo] = useState<
    DeploymentInfo | undefined
  >(undefined);

  const [accessToken] = useAccessToken();
  // Use a ref to track the access token without triggering re-authentication
  // when it changes
  const accessTokenRef = useRef(accessToken);
  useEffect(() => {
    accessTokenRef.current = accessToken;
  }, [accessToken]);
  const {
    connectionStateCheckIntervalMs,
    s3ExportIntegration,
    showFivetranSyncProgress,
    healthDeployProgress,
    componentSwitcherTreeEnabled,
  } = useLaunchDarkly();
  const [, openFeedbackForm] = useFeedbackFormOpen();
  const posthog = usePostHogLib();
  const posthogRef = useRef(posthog);
  posthogRef.current = posthog;
  const captureEvent = useCallback(
    (event: string, properties?: Record<string, unknown>) => {
      posthogRef.current?.capture(event, properties);
    },
    [],
  );

  const { project: currentProject, isLoading: projectLoading } =
    useCurrentProjectWithStatus();
  const { deployments: projectDeployments, isLoading: deploymentsLoading } =
    useDeployments(currentProject?.id);
  const matchedDeployment =
    !deploymentOverride && typeof deploymentName === "string"
      ? projectDeployments?.find((d) => d.name === deploymentName)
      : undefined;
  const cloudDeploymentUrl =
    deploymentUrlOverride ??
    (matchedDeployment?.kind === "cloud"
      ? matchedDeployment.deploymentUrl
      : undefined);

  // A cloud deployment's URL is only known from the project's deployments list,
  // and both the project and the list stay undefined when their request fails,
  // not just while it's in flight. So wait only while a lookup is genuinely
  // outstanding, and report a failed one below: waiting on the data itself would
  // leave the dashboard on a spinner for as long as the endpoint kept failing.
  const isDeploymentLookupSettled =
    deploymentOverride !== undefined ||
    ((currentProject?.id !== undefined || !projectLoading) &&
      (projectDeployments !== undefined || !deploymentsLoading));
  // A local deployment is identified by its name alone and Big Brain returns its
  // URL, so it never needs the lookups below.
  const isLocalTarget = (
    deploymentOverride ??
    (typeof deploymentName === "string" ? deploymentName : "")
  ).startsWith("local-");
  // Whether the route's own deployments list came back, so a name missing from
  // it really is missing. Overrides render outside that route, so their target
  // isn't covered by this list.
  const canProveDeploymentMissing =
    deploymentOverride === undefined && projectDeployments !== undefined;
  const authRefreshKey = cloudDeploymentUrl !== undefined ? accessToken : null;

  const selectedTeamSlug = router.query.team as string;
  const projectSlug = router.query.project as string;
  const teamsURI = `/t/${selectedTeamSlug}`;
  const projectsURI = `${teamsURI}/${projectSlug}`;
  const deploymentsURI = `${projectsURI}/${deploymentName}`;
  useIsomorphicLayoutEffect(() => {
    const f = async () => {
      setDeploymentInfo(undefined);
      const token = accessTokenRef.current;
      if (!token) {
        return;
      }
      const target = deploymentOverride || (deploymentName as string);

      let info:
        | { deploymentUrl: string; adminKey: string; ok: true }
        | { ok: false; errorMessage: string; errorCode: string };
      if (!isLocalTarget && !isDeploymentLookupSettled) {
        // A lookup is still in flight; a later run of this effect (once it
        // resolves or fails) will authenticate.
        return;
      }
      if (isLocalTarget) {
        // A local deployment's URL is whatever the CLI registered, and its admin
        // key is minted per deployment rather than derived from the session, so
        // Big Brain has to hand us both.
        info = await localDeploymentAuth(target, `Bearer ${token}`);
      } else if (cloudDeploymentUrl !== undefined) {
        info = {
          ok: true,
          deploymentUrl: cloudDeploymentUrl,
          adminKey: authRefreshKey ?? token,
        };
      } else if (canProveDeploymentMissing) {
        // The list loaded and this cloud deployment isn't in it. Matches the
        // error code Big Brain used to return, which routes to the 404 page.
        info = {
          ok: false,
          errorCode: "DeploymentNotFound",
          errorMessage: `Deployment ${target} could not be found.`,
        };
      } else {
        info = {
          ok: false,
          errorCode: "DeploymentUrlUnavailable",
          errorMessage:
            "Couldn't load this deployment's URL. Check your connection and try again.",
        };
      }
      setDeploymentInfo({
        ...info,
        addBreadcrumb,
        captureMessage,
        captureException,
        reportHttpError,
        useCurrentTeam,
        useCurrentMemberName,
        openFeedbackForm,
        captureEvent,
        useMemberPreference: (name: PreferenceName) => {
          const preferences = useMemberPreferences();
          const setPreference = useSetPreference();
          return {
            value:
              preferences === undefined
                ? undefined
                : ((preferences[name] as boolean | undefined) ?? false),
            set: async (value: boolean) => {
              await setPreference({ name, value });
            },
          };
        },
        useCurrentProject,
        useCurrentUsageBanner,
        useTeamUsageState,
        useTeamPlanType: (teamId) => {
          const { subscription } = useTeamOrbSubscription(teamId ?? undefined);
          return subscription?.plan?.planType ?? null;
        },
        useCurrentDeployment: () => {
          const deployment = useCurrentDeployment();
          if (!deployment) return undefined;
          return deployment;
        },
        useIsProtectedDeployment: () => {
          const deployment = useCurrentDeployment();
          if (!deployment) {
            return false;
          }
          if (deployment.kind === "local") {
            return false;
          }
          return typeof deployment.dashboardEditConfirmation === "boolean"
            ? deployment.dashboardEditConfirmation
            : deployment.deploymentType === "prod";
        },
        useTeamMembers,
        useTeamEntitlements,
        useHasProjectAdminPermissions,
        useHasCustomRole: useHasCustomRole,
        useIsOperationAllowed: useIsOperationAllowed,
        useProjectEnvironmentVariables,
        useIsDeploymentPaused,
        useLogDeploymentEvent,
        workOSOperations: {
          useDeploymentWorkOSEnvironment,
          useTeamWorkOSIntegration,
          useWorkOSTeamHealth,
          useWorkOSEnvironmentHealth,
          useDisconnectWorkOSTeam,
          useInviteWorkOSTeamMember,
          useWorkOSInvitationEligibleEmails,
          useAvailableWorkOSTeamEmails,
          useProvisionWorkOSTeam,
          useProvisionWorkOSEnvironment,
          useDeleteWorkOSEnvironment,
          useProjectWorkOSEnvironments,
          useGetProjectWorkOSEnvironment,
          useCheckProjectEnvironmentHealth,
          useProvisionProjectWorkOSEnvironment,
          useDeleteProjectWorkOSEnvironment,
        },
        Link,
        TeamMemberLink,
        CloudImport,
        ErrorBoundary: DeploymentErrorBoundary,
        DisconnectOverlay: CloudDashboardDisconnectOverlay,
        teamsURI,
        projectsURI,
        deploymentsURI,
        isSelfHosted: false,
        workosIntegrationEnabled: true,
        healthDeployProgressEnabled: healthDeployProgress,
        s3ExportIntegrationEnabled: s3ExportIntegration,
        connectionStateCheckIntervalMs,
        showFivetranSyncProgress,
        componentSwitcherTreeEnabled,
      });
    };
    if (accessTokenRef.current && (deploymentOverride || deploymentName)) {
      void f();
    }
  }, [
    deploymentName,
    deploymentOverride,
    deploymentsURI,
    projectsURI,
    teamsURI,
    connectionStateCheckIntervalMs,
    showFivetranSyncProgress,
    openFeedbackForm,
    isDeploymentLookupSettled,
    isLocalTarget,
    canProveDeploymentMissing,
    cloudDeploymentUrl,
    authRefreshKey,
    healthDeployProgress,
    s3ExportIntegration,
    componentSwitcherTreeEnabled,
  ]);

  return deploymentInfo ? (
    <DeploymentInfoContext.Provider value={deploymentInfo}>
      {children}
    </DeploymentInfoContext.Provider>
  ) : (
    <>{children}</>
  );
}
