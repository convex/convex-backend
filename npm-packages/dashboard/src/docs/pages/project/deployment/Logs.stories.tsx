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
import { fn, mocked, userEvent, within } from "storybook/test";
import { LogsView } from "@common/features/logs/components/LogsView";
import { FunctionsContext } from "@common/lib/functions/FunctionsProvider";
import { streamFunctionLogs } from "@common/lib/appMetrics";
import { TeamMemberLink } from "elements/TeamMemberLink";
import {
  FunctionExecution,
  UsageStats,
} from "system-udfs/convex/_system/frontend/common";

// Fixed "now" so timestamps are stable.
const NOW = new Date("2026-08-01T14:25:00Z").getTime();

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
  deploymentType: "dev" as const,
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: NOW,
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "dev/nicolas",
  region: "aws-us-east-1",
} as const;

const mockTeamMembers = [
  {
    id: 1,
    name: "Nicolas Ettlin",
    email: "nicolas@acme.dev",
    role: "admin" as const,
    customRoles: [],
  },
  {
    id: 2,
    name: "Ari Trakh",
    email: "ari@acme.dev",
    role: "admin" as const,
    customRoles: [],
  },
];

const usageStats: UsageStats = {
  memoryUsedMb: null,
  databaseReadBytes: 1_024,
  databaseWriteBytes: 512,
  databaseIoReadBytes: 1_024,
  databaseIoWriteBytes: 512,
  databaseReadDocuments: 3,
  storageReadBytes: 0,
  storageWriteBytes: 0,
  vectorIndexReadBytes: 0,
  vectorIndexWriteBytes: 0,
  textIndexQueryBytes: 0,
  textIndexWriteQueryBytes: 0,
  vectorIndexReadQueryBytes: 0,
  vectorIndexWriteQueryBytes: 0,
  networkEgressBytes: 128,
};

// `FunctionExecution.timestamp` and `executionTime` are in seconds.
const seconds = (msSinceNow: number) => (NOW + msSinceNow) / 1000;

/** The request the "Request logs" story drills down into. */
const FAILED_REQUEST_ID = "a974c10eb9c971e3";

const mockExecutions: FunctionExecution[] = [
  {
    kind: "Completion",
    identifier: "messages.js:send",
    udfType: "Mutation",
    arguments: ["{}"],
    logLines: [
      {
        messages: ["'Sending message from Ari'"],
        level: "LOG",
        timestamp: NOW - 95_000,
        isTruncated: false,
      },
    ],
    timestamp: seconds(-95_000),
    success: null,
    error: null,
    cachedResult: false,
    executionTime: 0.034,
    requestId: "6b1f0c8a4d2e7b93",
    executionId: "exec-1",
    executionTimestamp: seconds(-95_000),
    usageStats,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    parentExecutionId: null,
  },
  {
    kind: "Completion",
    identifier: "messages.js:send",
    udfType: "Mutation",
    arguments: ["{}"],
    logLines: [
      {
        messages: ["'Sending message from Pepper'"],
        level: "LOG",
        timestamp: NOW - 88_000,
        isTruncated: false,
      },
    ],
    timestamp: seconds(-88_000),
    success: null,
    error: null,
    cachedResult: false,
    executionTime: 0.017,
    requestId: "30ac5e91f7b64d20",
    executionId: "exec-2",
    executionTimestamp: seconds(-88_000),
    usageStats,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    parentExecutionId: null,
  },
  {
    kind: "Completion",
    identifier: "messages.js:list",
    udfType: "Query",
    arguments: ["{}"],
    logLines: [],
    timestamp: seconds(-70_000),
    success: null,
    error: null,
    cachedResult: true,
    executionTime: 0.002,
    requestId: "c50d7a3e19b84f6a",
    executionId: "exec-3",
    executionTimestamp: seconds(-70_000),
    usageStats,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    parentExecutionId: null,
  },
  {
    kind: "Completion",
    identifier: "messages.js:send",
    udfType: "Mutation",
    arguments: ["{}"],
    logLines: [
      {
        messages: ["'Sending message from Michal'"],
        level: "LOG",
        timestamp: NOW - 30_000,
        isTruncated: false,
      },
      {
        messages: ["'User \"Michal\" is not allowed to send messages'"],
        level: "ERROR",
        timestamp: NOW - 30_000,
        isTruncated: false,
      },
    ],
    timestamp: seconds(-30_000),
    success: null,
    error:
      "Uncaught Error: You are not allowed to send messages\n    at handler (../convex/messages.ts:18:4)",
    cachedResult: false,
    executionTime: 0.019,
    requestId: FAILED_REQUEST_ID,
    executionId: "bd81867b-770b-4260-943c-e6905a363c8f",
    executionTimestamp: seconds(-30_000),
    usageStats,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    parentExecutionId: null,
  },
];

const mockAuditLogEvents = [
  {
    _id: "a001" as GenericId<"_deployment_audit_log">,
    _creationTime: NOW - 60_000,
    action: "push_config" as const,
    member_id: BigInt(2),
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
];

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(udfs.deploymentState.deploymentState, () => ({
    _id: "" as GenericId<"_backend_state">,
    _creationTime: 0,
    state: "running" as const,
  }))
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
    page: mockAuditLogEvents,
    isDone: true,
    continueCursor: "",
  }))
  .registerQueryFake(udfs.fileStorageV2.numFiles, () => 0)
  .registerQueryFake(udfs.tableSize.sizeOfAllTables, () => 0);

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
  component: LogsView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/logs",
        route: "/t/[team]/[project]/[deploymentName]/logs",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/logs",
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

    // The logs page long-polls `stream_function_logs`. Serve the fixtures once,
    // then never resolve so the stream stays open without producing new logs.
    // Resolving on a later tick lets us drop requests whose effect has already
    // been torn down (React strict mode runs effects twice), which would
    // otherwise deliver the fixtures a second time.
    mocked(streamFunctionLogs).mockImplementation(
      async (_deploymentUrl, _authHeader, cursor, _requestFilter, signal) => {
        await new Promise((resolve) => {
          setTimeout(resolve, 0);
        });
        if (cursor !== 0 || signal.aborted) {
          return new Promise(() => {});
        }
        return { entries: mockExecutions, newCursor: NOW };
      },
    );

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
            useTeamMembers: () => mockTeamMembers,
            useTeamEntitlements: () => ({ auditLogRetentionDays: 90 }),
            TeamMemberLink,
            deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
            projectsURI: "/t/acme/my-amazing-app",
            teamsURI: "/t/acme",
            isSelfHosted: false,
          }}
        >
          <FunctionsContext.Provider value={new Map()}>
            <LogsView />
          </FunctionsContext.Provider>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof LogsView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

/**
 * Shows the logs of a single request in the drilldown panel.
 */
export const RequestLogs: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const row = await canvas.findByText(/Uncaught Error: You are not allowed/);
    await userEvent.click(row);
  },
};
