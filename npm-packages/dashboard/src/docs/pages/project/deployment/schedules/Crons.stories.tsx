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
import { CronsView } from "@common/features/schedules/components/crons/CronsView";

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

const now = NOW;
const emptyArgs = new TextEncoder().encode("[]").buffer as ArrayBuffer;

function mockLog(
  id: string,
  name: string,
  udfPath: string,
  msAgo: number,
  status:
    | { type: "success"; result: any }
    | { type: "err"; error: any }
    | { type: "canceled"; num_canceled: bigint },
  executionTime: number,
) {
  return {
    _id: id as GenericId<"_cron_job_logs">,
    _creationTime: now - msAgo,
    name,
    ts: BigInt((now - msAgo) * 1_000_000),
    udfPath,
    udfArgs: emptyArgs,
    status,
    logLines: { logLines: [], isTruncated: false },
    executionTime,
  };
}

const lastRunDigest = mockLog(
  "l001",
  "send-daily-digest",
  "digests.js:sendDailyDigest",
  23 * 60 * 60 * 1000,
  { type: "success", result: null },
  0.123,
);
const lastRunCleanup = mockLog(
  "l004",
  "cleanup-expired-sessions",
  "users.js:cleanupSessions",
  50 * 60 * 1000,
  { type: "success", result: null },
  0.042,
);
const lastRunWeekly = mockLog(
  "l003",
  "weekly-report",
  "reports.js:weeklyReport",
  2 * 24 * 60 * 60 * 1000,
  { type: "err", error: "Timeout exceeded" },
  10.0,
);

function mockJob(
  id: string,
  name: string,
  udfPath: string,
  cronSchedule: any,
  nextMs: number,
  lastRun: any,
) {
  return {
    _id: id as GenericId<"_cron_jobs">,
    _creationTime: now - 7 * 24 * 60 * 60 * 1000,
    name,
    cronSpec: {
      udfPath,
      udfArgs: emptyArgs,
      cronSchedule,
    },
    lastRun,
    nextRun: {
      _id: `n${id}` as GenericId<"_cron_next_run">,
      _creationTime: now,
      cronJobId: id as GenericId<"_cron_jobs">,
      state: { type: "pending" as const },
      prevTs: lastRun ? lastRun.ts : null,
      nextTs: BigInt((now + nextMs) * 1_000_000),
    },
  };
}

const mockCronJobs = [
  mockJob(
    "c001",
    "send-daily-digest",
    "digests.js:sendDailyDigest",
    { type: "daily", hourUTC: BigInt(9), minuteUTC: BigInt(0) },
    60 * 60 * 1000,
    lastRunDigest,
  ),
  mockJob(
    "c002",
    "cleanup-expired-sessions",
    "users.js:cleanupSessions",
    { type: "interval", seconds: BigInt(3600) },
    10 * 60 * 1000,
    lastRunCleanup,
  ),
  mockJob(
    "c003",
    "weekly-report",
    "reports.js:weeklyReport",
    {
      type: "weekly",
      dayOfWeek: BigInt(1),
      hourUTC: BigInt(14),
      minuteUTC: BigInt(0),
    },
    5 * 24 * 60 * 60 * 1000,
    lastRunWeekly,
  ),
];

const mockCronJobLogs = [
  lastRunDigest,
  mockLog(
    "l002",
    "send-daily-digest",
    "digests.js:sendDailyDigest",
    47 * 60 * 60 * 1000,
    { type: "success", result: null },
    0.098,
  ),
  lastRunWeekly,
  lastRunCleanup,
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
  .registerQueryFake(udfs.listCronJobs.default, () => mockCronJobs)
  .registerQueryFake(udfs.listCronJobRuns.default, () => mockCronJobLogs)
  .registerQueryFake(
    udfs.modules.listForAllComponents,
    () =>
      [
        [
          null,
          [
            [
              "crons.js",
              {
                cronSpecs: mockCronJobs.map((j) => [j.name, j.cronSpec]),
              },
            ],
          ],
        ],
      ] as [string | null, [string, any][]][],
  );

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

function buildRouter(extra: Record<string, string> = {}) {
  return {
    pathname: "/t/[team]/[project]/[deploymentName]/schedules/crons",
    route: "/t/[team]/[project]/[deploymentName]/schedules/crons",
    asPath: "/t/acme/my-amazing-app/happy-capybara-123/schedules/crons",
    query: {
      team: "acme",
      project: "my-amazing-app",
      deploymentName: "happy-capybara-123",
      ...extra,
    },
  };
}

const meta = {
  component: CronsView,
  parameters: {
    layout: "fullscreen",
    nextjs: { router: buildRouter() },
    a11y: { test: "todo" },
  },
  beforeEach: () => {
    // Polyfill Buffer
    (window as any).Buffer = {
      from: (input: ArrayBuffer | ArrayBufferView | string) => {
        const bytes =
          typeof input === "string"
            ? new TextEncoder().encode(input)
            : input instanceof ArrayBuffer
              ? new Uint8Array(input)
              : new Uint8Array(
                  input.buffer,
                  input.byteOffset,
                  input.byteLength,
                );
        return { toString: () => new TextDecoder("utf-8").decode(bytes) };
      },
    };

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
            deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
            projectsURI: "/t/acme/my-amazing-app",
            teamsURI: "/t/acme",
            isSelfHosted: false,
          }}
        >
          <CronsView />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof CronsView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const History: Story = {
  parameters: {
    nextjs: { router: buildRouter({ id: "send-daily-digest" }) },
  },
};
