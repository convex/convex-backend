import { Meta, StoryObj } from "@storybook/nextjs";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { GenericId } from "convex/values";
import { fn } from "storybook/test";
import { HistoryView } from "@common/features/history/components/HistoryView";
import { TeamMemberLink } from "elements/TeamMemberLink";

// Fixed "now" so timestamps are stable.
const NOW = new Date("2026-03-10T14:25:00Z").getTime();

const mockTeam = { id: 2, slug: "acme", name: "Acme Corp" };
const mockProject = {
  id: 7,
  teamId: mockTeam.id,
  name: "My amazing app",
  slug: "my-amazing-app",
};
const mockDeployment = {
  id: 11,
  name: "happy-capybara-123",
  deploymentType: "prod" as const,
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: NOW,
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "production",
  region: "aws-us-east-1",
} as const;

const now = NOW;

const mockEvents = [
  {
    _id: "a001" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 30 * 60 * 1000,
    action: "create_environment_variable" as const,
    member_id: BigInt(1),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: { variable_name: "OPENAI_API_KEY" },
  },
  {
    _id: "a002" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 2 * 60 * 60 * 1000,
    action: "update_environment_variable" as const,
    member_id: BigInt(2),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: { variable_name: "STRIPE_SECRET_KEY" },
  },
  {
    _id: "a003" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 5 * 60 * 60 * 1000,
    action: "push_config" as const,
    member_id: BigInt(1),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: {
      auth: { added: [], removed: [] },
      server_version: { previous_version: "1.36.0", next_version: "1.36.0" },
      modules: { added: [], removed: [] },
      crons: { added: [], updated: [], deleted: [] },
      schema: { previous_schema_id: null, next_schema_id: null },
    },
  },
  {
    _id: "a004" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 24 * 60 * 60 * 1000,
    action: "delete_environment_variable" as const,
    member_id: BigInt(2),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: { variable_name: "LEGACY_TOKEN" },
  },
  {
    _id: "a005" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 6 * 60 * 60 * 1000,
    action: "create_usage_limit" as const,
    member_id: BigInt(1),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: {
      id: "ul_1",
      config: {
        name: null,
        metric: "functionCalls",
        window: "month",
        limitType: "disable",
        limit: BigInt(1_000_000),
        enabled: true,
      },
    },
  },
  {
    _id: "a006" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 8 * 60 * 60 * 1000,
    action: "update_usage_limit" as const,
    member_id: BigInt(2),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: {
      id: "ul_1",
      previous: {
        name: null,
        metric: "functionCalls",
        window: "month",
        limitType: "disable",
        limit: BigInt(1_000_000),
        enabled: true,
      },
      current: {
        name: null,
        metric: "functionCalls",
        window: "month",
        limitType: "disable",
        limit: BigInt(2_000_000),
        enabled: false,
      },
    },
  },
  {
    _id: "a007" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 10 * 60 * 60 * 1000,
    action: "usage_limit_exceeded" as const,
    member_id: null,
    token_id: null,
    app_client_id: null,
    metadata: {
      id: "ul_2",
      config: {
        name: null,
        metric: "databaseIoGb",
        window: "day",
        limitType: "warning",
        limit: BigInt(50),
        enabled: true,
      },
    },
  },
  {
    _id: "a008" as GenericId<"_deployment_audit_log">,
    _creationTime: now - 12 * 60 * 60 * 1000,
    action: "delete_usage_limit" as const,
    member_id: BigInt(1),
    token_id: BigInt(1),
    app_client_id: null,
    metadata: {
      id: "ul_2",
      config: {
        name: null,
        metric: "databaseIoGb",
        window: "day",
        limitType: "warning",
        limit: BigInt(50),
        enabled: false,
      },
    },
  },
];

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.deploymentState.deploymentState, () => ({
    _id: "" as any,
    _creationTime: 0,
    state: "running" as const,
  }))
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(udfs.deploymentEvents.lastPushEvent, () => null)
  .registerQueryFake(
    udfs.convexCloudUrl.default,
    () => mockDeployment.deploymentUrl,
  )
  .registerQueryFake(
    udfs.convexSiteUrl.default,
    () => "https://happy-capybara-123.convex.site",
  )
  .registerQueryFake(udfs.paginatedDeploymentEvents.default, () => ({
    page: mockEvents,
    isDone: true,
    continueCursor: "",
  }));

const mockConnectedDeployment = {
  deployment: {
    client: mockClient,
    httpClient: {} as never,
    deploymentUrl: mockDeployment.deploymentUrl,
    adminKey: "storybook-admin-key",
    deploymentName: mockDeployment.name,
  },
  isDisconnected: false,
};

const meta = {
  component: HistoryView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/history",
        route: "/t/[team]/[project]/[deploymentName]/history",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/history",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
        },
      },
    },
    a11y: { test: "todo" },
  },
  beforeEach: () => {
    const originalDateNow = Date.now;
    Date.now = () => NOW;

    return () => {
      Date.now = originalDateNow;
    };
  },
  render: () => (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider
          value={{
            ...mockDeploymentInfo,
            useCurrentTeam: () => mockTeam,
            useCurrentProject: () => mockProject,
            useCurrentDeployment: () => mockDeployment,
            useIsDeploymentPaused: () => false,
            useLogDeploymentEvent: () => fn(),
            useTeamEntitlements: () => ({ auditLogRetentionDays: 90 }),
            TeamMemberLink,
            useTeamMembers: () => [
              {
                id: 1,
                name: "Nicolas Ettlin",
                email: "nicolas@acme.dev",
                role: "admin",
                customRoles: [],
              },
              {
                id: 2,
                name: "Ari Trakh",
                email: "ari@acme.dev",
                role: "admin",
                customRoles: [],
              },
            ],
            deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
            projectsURI: "/t/acme/my-amazing-app",
            teamsURI: "/t/acme",
            isSelfHosted: false,
          }}
        >
          <HistoryView />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof HistoryView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
