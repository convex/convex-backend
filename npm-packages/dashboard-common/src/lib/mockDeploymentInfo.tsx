import { DeploymentInfo } from "@common/lib/deploymentContext";

export const mockDeploymentInfo: DeploymentInfo = {
  ok: true,
  reportHttpError: () => {},
  captureException: () => {},
  captureMessage: () => {},
  addBreadcrumb: () => {},
  deploymentUrl: process.env.NEXT_PUBLIC_DEPLOYMENT_URL!,
  adminKey: process.env.NEXT_PUBLIC_ADMIN_KEY!,
  useCurrentTeam: () => ({
    id: 0,
    name: "Team",
    slug: "team",
  }),
  useTeamMembers: () => [],
  useCurrentMemberName: () => undefined,
  useMemberPreference: () => ({ value: undefined, set: async () => {} }),
  useTeamEntitlements: () => ({}),
  useCurrentUsageBanner: () => null,
  useCurrentProject: () => ({
    id: 0,
    name: "Project",
    slug: "project",
    teamId: 0,
  }),
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
    useProjectWorkOSEnvironments: () => undefined,
    useGetProjectWorkOSEnvironment: () => undefined,
    useCheckProjectEnvironmentHealth: () => async () => null,
    useProvisionProjectWorkOSEnvironment: () => async () => ({
      workosEnvironmentId: "",
      workosEnvironmentName: "",
      workosClientId: "",
      workosApiKey: "",
      newlyProvisioned: true,
      userEnvironmentName: "",
    }),
    useDeleteProjectWorkOSEnvironment: () => async () => ({
      workosEnvironmentId: "",
      workosEnvironmentName: "",
      workosTeamId: "",
    }),
  },
  useCurrentDeployment: () => ({
    id: 0,
    name: "local",
    deploymentType: "prod",
    projectId: 0,
    kind: "local",
    previewIdentifier: null,
    port: 123,
    isActive: true,
    deviceName: "Local Device",
    creator: 0,
    createTime: 0,
  }),
  useIsProtectedDeployment: () => false,
  useHasProjectAdminPermissions: () => true,
  useHasCustomRole: () => false,
  useIsOperationAllowed: () => true,
  useIsDeploymentPaused: () => false,
  useProjectEnvironmentVariables: () => ({ configs: [] }),
  CloudImport: ({ sourceCloudBackupId }: { sourceCloudBackupId: number }) => (
    <div>{sourceCloudBackupId}</div>
  ),
  ErrorBoundary: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
  TeamMemberLink: ({ name }: { name: string }) => (
    <span className="font-semibold">{name}</span>
  ),
  Link: ({
    href,
    children,
    ...props
  }: {
    href: string;
    children?: React.ReactNode;
    className?: string;
    target?: string;
    rel?: string;
  }) => (
    <a href={href} {...props}>
      {children}
    </a>
  ),
  DisconnectOverlay: () => <div>Disconnected</div>,
  useTeamUsageState: () => "Default",
  useTeamPlanType: () => null,
  teamsURI: "",
  projectsURI: "",
  deploymentsURI: "",
  isSelfHosted: true,
  workosIntegrationEnabled: false,
  healthDeployProgressEnabled: true,
  s3ExportIntegrationEnabled: false,
  connectionStateCheckIntervalMs: 2500,
  showFivetranSyncProgress: false,
  componentSwitcherTreeEnabled: false,
  openFeedbackForm: () => {},
  captureEvent: () => {},
};
