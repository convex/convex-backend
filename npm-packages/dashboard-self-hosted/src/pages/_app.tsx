// eslint-disable-next-line import/no-relative-packages
import "../../../@convex-dev/design-system/src/styles/shared.css";
// eslint-disable-next-line import/no-relative-packages
import "../../../dashboard-common/src/styles/globals.css";
import { AppProps } from "next/app";
import Head from "next/head";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import { useSessionStorage } from "react-use";
import { ExitIcon, GearIcon } from "@radix-ui/react-icons";
import { ConvexLogo } from "@common/elements/ConvexLogo";
import { ToastContainer } from "@common/elements/ToastContainer";
import { ThemeConsumer } from "@common/elements/ThemeConsumer";
import { Favicon } from "@common/elements/Favicon";
import { ToggleTheme } from "@common/elements/ToggleTheme";
import { SelfHostedDisconnectOverlay } from "@common/features/disconnectOverlay/SelfHostedDisconnectOverlay";
import { Menu, MenuItem } from "@ui/Menu";
import { ThemeProvider } from "next-themes";
import React, {
  JSX,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  createContext,
} from "react";
import { ErrorBoundary } from "components/ErrorBoundary";
import { DeploymentDashboardLayout } from "@common/layouts/DeploymentDashboardLayout";
import {
  DeploymentApiProvider,
  WaitForDeploymentApi,
  DeploymentInfo,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { Tooltip } from "@ui/Tooltip";
import { DeploymentCredentialsForm } from "components/DeploymentCredentialsForm";
import { DeploymentList } from "components/DeploymentList";
import { checkDeploymentInfo } from "lib/checkDeploymentInfo";
import {
  CurrentDeployment,
  fetchCurrentDeployment,
} from "lib/fetchCurrentDeployment";
import { ConvexCloudReminderToast } from "components/ConvexCloudReminderToast";
import { z } from "zod";
import { UIProvider } from "@ui/UIContext";
import Link from "next/link";

// Monaco only runs in the browser, and pulling it into the server bundle makes
// it fail to evaluate there.
if (
  typeof window !== "undefined" &&
  process.env.NEXT_PUBLIC_LOAD_MONACO_INTERNALLY === "true"
) {
  import("../lib/monacoInternalLoader").then((a) => a).catch(console.error);
}

// Context for self-hosted dashboard sidebar settings
const SelfHostedSettingsContext = createContext<{
  visiblePages?: string[];
}>({
  visiblePages: undefined,
});

/**
 * Wrapper component that consumes SelfHostedSettingsContext and passes
 * the settings to DeploymentDashboardLayout
 */
function DeploymentDashboardLayoutWrapper({
  children,
}: {
  children: JSX.Element;
}) {
  const { visiblePages } = useContext(SelfHostedSettingsContext);

  return (
    <DeploymentDashboardLayout visiblePages={visiblePages}>
      {children}
    </DeploymentDashboardLayout>
  );
}

function App({
  Component,
  pageProps: {
    deploymentUrl,
    adminKey,
    defaultListDeploymentsApiUrl,
    ...pageProps
  },
}: AppProps & {
  pageProps: {
    deploymentUrl: string | null;
    adminKey: string | null;
    defaultListDeploymentsApiUrl: string | null;
  };
}) {
  return (
    <>
      <Head>
        <title>Convex Dashboard</title>
        <meta name="description" content="Manage your Convex apps" />
        <Favicon />
      </Head>
      <UIProvider Link={Link}>
        <ThemeProvider attribute="class" disableTransitionOnChange>
          <ThemeConsumer />
          <ToastContainer />
          <div className="flex h-screen flex-col">
            <DeploymentInfoProvider
              deploymentUrl={deploymentUrl}
              adminKey={adminKey}
              defaultListDeploymentsApiUrl={defaultListDeploymentsApiUrl}
            >
              <DeploymentApiProvider deploymentOverride="local">
                <WaitForDeploymentApi>
                  <DeploymentDashboardLayoutWrapper>
                    <>
                      <Component {...pageProps} />
                      <ConvexCloudReminderToast />
                    </>
                  </DeploymentDashboardLayoutWrapper>
                </WaitForDeploymentApi>
              </DeploymentApiProvider>
            </DeploymentInfoProvider>
          </div>
        </ThemeProvider>
      </UIProvider>
    </>
  );
}

// Whether to try fetching the deployment credentials from the
// /api/current_deployment endpoint served by the CLI in anonymous mode.
// Inlined at build time by Next.js; set in the `build:export` script.
const USE_CURRENT_DEPLOYMENT_API =
  process.env.NEXT_PUBLIC_USE_CURRENT_DEPLOYMENT_API === "true";

const SESSION_STORAGE_DEPLOYMENT_NAME_KEY = "deploymentName";

function normalizeUrl(url: string) {
  try {
    const parsedUrl = new URL(url);
    // remove trailing slash
    return parsedUrl.href.replace(/\/$/, "");
  } catch {
    return null;
  }
}

App.getInitialProps = async ({ ctx }: { ctx: { req?: any } }) => {
  // On server-side, get from process.env
  if (ctx.req) {
    // Note -- we can't use `ctx.req.url` when serving the dashboard statically,
    // so instead we'll read from query params on the client side.

    let deploymentUrl: string | null = null;
    if (process.env.NEXT_PUBLIC_DEPLOYMENT_URL) {
      deploymentUrl = normalizeUrl(process.env.NEXT_PUBLIC_DEPLOYMENT_URL);
    }
    let adminKey: string | null = null;
    if (process.env.NEXT_PUBLIC_ADMIN_KEY) {
      adminKey = process.env.NEXT_PUBLIC_ADMIN_KEY;
    }

    const listDeploymentsApiPort =
      process.env.NEXT_PUBLIC_DEFAULT_LIST_DEPLOYMENTS_API_PORT;
    let listDeploymentsApiUrl: string | null = null;
    if (listDeploymentsApiPort) {
      const port = parseInt(listDeploymentsApiPort);
      if (!Number.isNaN(port)) {
        listDeploymentsApiUrl = normalizeUrl(`http://127.0.0.1:${port}`);
      }
    }

    return {
      pageProps: {
        deploymentUrl,
        adminKey,
        defaultListDeploymentsApiUrl: listDeploymentsApiUrl,
      },
    };
  }

  // On client-side navigation, get from window.__NEXT_DATA__
  const clientSideDeploymentUrl =
    window.__NEXT_DATA__?.props?.pageProps?.deploymentUrl ?? null;
  const clientSideAdminKey =
    window.__NEXT_DATA__?.props?.pageProps?.adminKey ?? null;
  const clientSideListDeploymentsApiUrl =
    window.__NEXT_DATA__?.props?.pageProps?.listDeploymentsApiUrl ?? null;
  const clientSideSelectedDeploymentName =
    window.__NEXT_DATA__?.props?.pageProps?.selectedDeploymentName ?? null;
  return {
    pageProps: {
      deploymentUrl: clientSideDeploymentUrl ?? null,
      adminKey: clientSideAdminKey ?? null,
      defaultListDeploymentsApiUrl: clientSideListDeploymentsApiUrl ?? null,
      selectedDeploymentName: clientSideSelectedDeploymentName ?? null,
    },
  };
};

export default App;

const deploymentInfo: Omit<DeploymentInfo, "deploymentUrl" | "adminKey"> = {
  ok: true,
  addBreadcrumb: console.error,
  captureMessage: console.error,
  captureException: console.error,
  reportHttpError: (
    method: string,
    url: string,
    error: { code: string; message: string },
  ) => {
    console.error(
      `failed to request ${method} ${url}: ${error.code} - ${error.message} `,
    );
  },
  useCurrentTeam: () => ({
    id: 0,
    name: "Team",
    slug: "team",
  }),
  useTeamMembers: () => [],
  useCurrentMemberName: () => undefined,
  useMemberPreference: () => ({ value: false, set: async () => {} }),
  useTeamEntitlements: () => ({
    auditLogRetentionDays: -1,
    logStreamingEnabled: true,
    streamingExportEnabled: true,
  }),
  useCurrentUsageBanner: () => null,
  useCurrentProject: () => ({
    id: 0,
    name: "Project",
    slug: "project",
    teamId: 0,
  }),
  useCurrentDeployment: () => {
    const [storedDeploymentName] = useSessionStorage(
      SESSION_STORAGE_DEPLOYMENT_NAME_KEY,
      "",
    );
    return {
      id: 0,
      name: storedDeploymentName,
      deploymentType: "dev",
      projectId: 0,
      kind: "local",
      previewIdentifier: null,
      creator: 0,
      createTime: 0,
      port: 0,
      deviceName: "local",
      isActive: true,
    };
  },
  useIsProtectedDeployment: () => false,
  useHasProjectAdminPermissions: () => true,
  useHasCustomRole: () => false,
  useIsOperationAllowed: () => true,
  useIsDeploymentPaused: () => {
    const deploymentState = useQuery(udfs.deploymentState.deploymentState);
    return deploymentState?.state === "paused";
  },
  useProjectEnvironmentVariables: () => ({ configs: [] }),
  // no-op. don't send analytics in the self-hosted dashboard.
  useLogDeploymentEvent: () => () => {},
  workOSOperations: {
    useDeploymentWorkOSEnvironment: () => ({
      data: undefined,
      error: undefined,
    }),
    useTeamWorkOSIntegration: () => undefined,
    useWorkOSTeamHealth: () => undefined,
    useWorkOSEnvironmentHealth: () => ({ data: undefined, error: undefined }),
    useDisconnectWorkOSTeam: (_teamId?: string) => async () => undefined,
    useInviteWorkOSTeamMember: () => async () => undefined,
    useWorkOSInvitationEligibleEmails: () => undefined,
    useAvailableWorkOSTeamEmails: () => undefined,
    useProvisionWorkOSTeam: (_teamId?: string) => async () => undefined,
    useProvisionWorkOSEnvironment: (_deploymentName?: string) => async () =>
      undefined,
    useDeleteWorkOSEnvironment: (_deploymentName?: string) => async () =>
      undefined,
    useProjectWorkOSEnvironments: (_projectId?: number) => undefined,
    useGetProjectWorkOSEnvironment: (_projectId?: number, _clientId?: string) =>
      undefined,
    useCheckProjectEnvironmentHealth:
      (_projectId?: number, _clientId?: string) => async () =>
        null,
    useProvisionProjectWorkOSEnvironment:
      (_projectId?: number) => async (_body: { environmentName: string }) => ({
        workosEnvironmentId: "",
        workosEnvironmentName: "",
        workosClientId: "",
        workosApiKey: "",
        newlyProvisioned: false,
        userEnvironmentName: "",
      }),
    useDeleteProjectWorkOSEnvironment:
      (_projectId?: number) => async (_clientId: string) => ({
        workosEnvironmentId: "",
        workosEnvironmentName: "",
        workosTeamId: "",
      }),
  },
  CloudImport: ({ sourceCloudBackupId }: { sourceCloudBackupId: number }) => (
    <div>{sourceCloudBackupId}</div>
  ),
  TeamMemberLink: () => (
    <Tooltip tip="Identity management is not available in self-hosted deployments.">
      <div className="underline decoration-dotted underline-offset-4">
        An admin
      </div>
    </Tooltip>
  ),
  Link,
  ErrorBoundary: ({ children }: { children: React.ReactNode }) => (
    <ErrorBoundary>{children}</ErrorBoundary>
  ),
  DisconnectOverlay: () => <SelfHostedDisconnectOverlay />,
  useTeamUsageState: () => "Default",
  useTeamPlanType: () => null,
  teamsURI: "",
  projectsURI: "",
  deploymentsURI: "",
  isSelfHosted: true,
  workosIntegrationEnabled: false,
  healthDeployProgressEnabled: false,
  s3ExportIntegrationEnabled: false,
  connectionStateCheckIntervalMs: 2500,
  showFivetranSyncProgress: false,
  componentSwitcherTreeEnabled: false,
};

function DeploymentInfoProvider({
  children,
  deploymentUrl,
  adminKey,
  defaultListDeploymentsApiUrl,
}: {
  children: React.ReactNode;
  deploymentUrl: string | null;
  adminKey: string | null;
  defaultListDeploymentsApiUrl: string | null;
}) {
  const [listDeploymentsApiUrl, setListDeploymentsApiUrl] = useState<
    string | null
  >(null);
  const [selectedDeploymentName, setSelectedDeploymentName] = useState<
    string | null
  >(null);
  const [isValidDeploymentInfo, setIsValidDeploymentInfo] = useState<
    boolean | null
  >(null);
  const [storedAdminKey, setStoredAdminKey] = useSessionStorage("adminKey", "");
  const [storedDeploymentUrl, setStoredDeploymentUrl] = useSessionStorage(
    "deploymentUrl",
    "",
  );
  const [_storedDeploymentName, setStoredDeploymentName] = useSessionStorage(
    SESSION_STORAGE_DEPLOYMENT_NAME_KEY,
    "",
  );
  const [visiblePages, setVisiblePages] = useState<string[] | undefined>(
    undefined,
  );
  const [allowedOps, setAllowedOps] = useState<string[]>([]);

  // Memoize this so it can safely be passed into the context
  const settingsContextValue = useMemo(
    () => ({ visiblePages }),
    [visiblePages],
  );

  const attemptLogin = useCallback(
    async ({
      submittedAdminKey,
      submittedDeploymentUrl,
      submittedDeploymentName,
      submittedVisiblePages,
    }: {
      submittedAdminKey: string;
      submittedDeploymentUrl: string;
      submittedDeploymentName: string;
      submittedVisiblePages?: string[];
    }) => {
      const result = await checkDeploymentInfo(
        submittedAdminKey,
        submittedDeploymentUrl,
      );
      if (result === null) {
        setIsValidDeploymentInfo(false);
        return;
      }
      setIsValidDeploymentInfo(true);
      setAllowedOps(result.allowedOps);
      setStoredAdminKey(submittedAdminKey);
      setStoredDeploymentUrl(submittedDeploymentUrl);
      setStoredDeploymentName(submittedDeploymentName);
      setVisiblePages(submittedVisiblePages);
    },
    [setStoredAdminKey, setStoredDeploymentUrl, setStoredDeploymentName],
  );

  useEmbeddedDashboardCredentials(attemptLogin);

  // `undefined` while the request to /api/current_deployment is in flight,
  // `null` once it has failed (e.g. 404 → fall back to the other mechanisms).
  const [currentDeployment, setCurrentDeployment] = useState<
    CurrentDeployment | null | undefined
  >(USE_CURRENT_DEPLOYMENT_API ? undefined : null);
  useEffect(() => {
    if (!USE_CURRENT_DEPLOYMENT_API) {
      return;
    }
    void fetchCurrentDeployment().then((deployment) => {
      setCurrentDeployment(deployment);
      if (deployment) {
        void attemptLogin({
          submittedAdminKey: deployment.adminKey,
          submittedDeploymentUrl:
            normalizeUrl(deployment.url) ?? deployment.url,
          submittedDeploymentName: deployment.name,
        });
      }
    });
  }, [attemptLogin]);

  const finalValue: DeploymentInfo = useMemo(
    () =>
      ({
        ...deploymentInfo,
        ok: true,
        adminKey: storedAdminKey,
        deploymentUrl: storedDeploymentUrl,
        useIsOperationAllowed: (operation: string) => {
          // Empty allowedOps means all operations are allowed (full admin key).
          if (allowedOps.length === 0) return true;
          return allowedOps.includes(operation);
        },
      }) as DeploymentInfo,
    [storedAdminKey, storedDeploymentUrl, allowedOps],
  );
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  // Pre-1.41.0 CLI versions had a “list deployments” API server allowing the
  // dashboard to find the list of anonymous backends and show it on screen (<DeploymentList />).
  // URL parameters, e.g. ?a=6791&d=anonymous-projectName, were used to automatically open
  // the dashboard for a given deployment.
  // This workflow isn’t used in CLI versions ≥1.41.0, but we keep it for backwards compatibility.
  useEffect(() => {
    if (typeof window !== "undefined") {
      const LIST_DEPLOYMENTS_API_PORT_QUERY_PARAM = "a";
      const SELECTED_DEPLOYMENT_NAME_QUERY_PARAM = "d";

      const url = new URL(window.location.href);
      const listDeploymentsApiPort = url.searchParams.get(
        LIST_DEPLOYMENTS_API_PORT_QUERY_PARAM,
      );
      const selectedDeploymentNameFromUrl = url.searchParams.get(
        SELECTED_DEPLOYMENT_NAME_QUERY_PARAM,
      );
      url.searchParams.delete(LIST_DEPLOYMENTS_API_PORT_QUERY_PARAM);
      url.searchParams.delete(SELECTED_DEPLOYMENT_NAME_QUERY_PARAM);
      window.history.replaceState({}, "", url.toString());
      if (listDeploymentsApiPort) {
        if (!Number.isNaN(parseInt(listDeploymentsApiPort))) {
          setListDeploymentsApiUrl(
            normalizeUrl(`http://127.0.0.1:${listDeploymentsApiPort}`),
          );
        }
      } else {
        setListDeploymentsApiUrl(defaultListDeploymentsApiUrl);
      }
      if (selectedDeploymentNameFromUrl) {
        setSelectedDeploymentName(selectedDeploymentNameFromUrl);
      }
    }
  }, [defaultListDeploymentsApiUrl]);

  if (!mounted) return null;

  if (!isValidDeploymentInfo) {
    // Wait for /api/current_deployment (and the validation of the credentials
    // it returns) before falling back, so the deployment list doesn’t flash
    // while the requests are in flight. If the credentials turn out to be
    // invalid (isValidDeploymentInfo === false), show the fallback UI.
    if (currentDeployment !== null && isValidDeploymentInfo === null) {
      return null;
    }
    return (
      <div className="flex h-screen w-screen flex-col items-center justify-center gap-8">
        <ConvexLogo />
        {listDeploymentsApiUrl !== null ? (
          <DeploymentList
            listDeploymentsApiUrl={listDeploymentsApiUrl}
            onError={() => {
              setListDeploymentsApiUrl(null);
            }}
            onSelect={attemptLogin}
            selectedDeploymentName={selectedDeploymentName}
          />
        ) : (
          <DeploymentCredentialsForm
            onSubmit={attemptLogin}
            initialAdminKey={adminKey}
            initialDeploymentUrl={deploymentUrl}
          />
        )}
        {isValidDeploymentInfo === false && (
          <div className="text-sm text-content-secondary">
            The deployment URL or admin key is invalid. Please check that you
            have entered the correct values.
          </div>
        )}
      </div>
    );
  }
  return (
    <>
      <Header
        onLogout={() => {
          setStoredAdminKey("");
          setStoredDeploymentUrl("");
          setStoredDeploymentName("");
        }}
      />
      <DeploymentInfoContext.Provider value={finalValue}>
        <SelfHostedSettingsContext.Provider value={settingsContextValue}>
          <ErrorBoundary>{children}</ErrorBoundary>
        </SelfHostedSettingsContext.Provider>
      </DeploymentInfoContext.Provider>
    </>
  );
}

function Header({ onLogout }: { onLogout: () => void }) {
  if (process.env.NEXT_PUBLIC_HIDE_HEADER) {
    return null;
  }

  return (
    <header className="-ml-1 scrollbar-none flex min-h-[56px] items-center justify-between gap-1 overflow-x-auto border-b bg-background-secondary pr-4 sm:gap-6">
      <ConvexLogo height={64} width={192} />
      <Menu
        buttonProps={{
          icon: (
            <GearIcon className="size-7 rounded-sm p-1 text-content-primary hover:bg-background-tertiary" />
          ),
          variant: "unstyled",
          "aria-label": "Dashboard Settings",
        }}
        placement="bottom-end"
      >
        <ToggleTheme />
        <MenuItem action={onLogout}>
          <div className="flex w-full items-center justify-between">
            Log Out
            <ExitIcon className="text-content-secondary" />
          </div>
        </MenuItem>
      </Menu>
    </header>
  );
}

/**
 * Allow the parent window to send credentials to the dashboard.
 * This is used when the dashboard is embedded in another application via an iframe.
 */
function useEmbeddedDashboardCredentials(
  attemptLogin: ({
    submittedAdminKey,
    submittedDeploymentUrl,
    submittedDeploymentName,
    submittedVisiblePages,
  }: {
    submittedAdminKey: string;
    submittedDeploymentUrl: string;
    submittedDeploymentName: string;
    submittedVisiblePages?: string[];
  }) => void,
) {
  // Send a message to the parent iframe to request the credentials.
  // This prevents race conditions where the parent iframe sends the message
  // before the dashboard loads.
  useEffect(() => {
    window.parent.postMessage(
      {
        type: "dashboard-credentials-request",
      },
      "*",
    );
  }, []);

  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      const credentialsSchema = z.object({
        type: z.literal("dashboard-credentials"),
        adminKey: z.string(),
        deploymentUrl: z.string().url(),
        deploymentName: z.string(),
        visiblePages: z.array(z.string()).optional(),
      });

      try {
        credentialsSchema.parse(event.data);
      } catch {
        return;
      }

      if (event.data.type === "dashboard-credentials") {
        attemptLogin({
          submittedAdminKey: event.data.adminKey,
          submittedDeploymentUrl: event.data.deploymentUrl,
          submittedDeploymentName: event.data.deploymentName,
          submittedVisiblePages: event.data.visiblePages,
        });
      }
    };

    window.addEventListener("message", handleMessage);
    return () => {
      window.removeEventListener("message", handleMessage);
    };
  }, [attemptLogin]);
}
