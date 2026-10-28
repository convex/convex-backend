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
import { fn, mocked } from "storybook/test";
import { useSchedulerLag } from "@common/lib/appMetrics";
import { ScheduledFunctionsView } from "@common/features/schedules/components/ScheduledFunctionsView";

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

const scheduledArgs = new TextEncoder().encode(
  '[{"messageId":"k57c1p8xr4m2vd9btgqjn0saf36ywhze"}]',
).buffer as ArrayBuffer;

function mockScheduledJob(id: string, udfPath: string, msFromNow: number) {
  return {
    _id: id as GenericId<"_scheduled_jobs">,
    _creationTime: NOW - 60_000,
    // Scheduled timestamps are stored in nanoseconds.
    nextTs: BigInt((NOW + msFromNow) * 1_000_000),
    udfPath,
    state: { type: "pending" as const },
    udfArgs: null,
    argsId: `a${id.slice(1)}` as GenericId<"_scheduled_job_args">,
  };
}

const mockScheduledJobs = [
  mockScheduledJob(
    "j820gh4kr9wnf3d7q1esmz5x2tvb6cpa",
    "messages.js:updateExpiringMessage",
    12_000,
  ),
  mockScheduledJob(
    "j828nvq2yd6bs4h0e3rjmkx7f1twpc95",
    "messages.js:updateExpiringMessage",
    73_000,
  ),
  mockScheduledJob(
    "j822v0mep7ct5kb9x4gnrjd1s6hzwy38",
    "messages.js:updateExpiringMessage",
    145_000,
  ),
  mockScheduledJob(
    "j8245jhbn3wq8mf2t6xdgvk0r5cepa71",
    "messages.js:updateExpiringMessage",
    208_000,
  ),
  mockScheduledJob(
    "j82b6cwx1ka7pn5r3jtdmfy9e0svgz42",
    "digests.js:sendDailyDigest",
    900_000,
  ),
  mockScheduledJob(
    "j82d9rtfk6pw2xm8v4hnqcb3g7azyj15",
    "users.js:cleanupSessions",
    3_600_000,
  ),
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
  .registerQueryFake(udfs.modules.listForAllComponents, () => [])
  .registerQueryFake(udfs.paginatedScheduledJobs.default, () => ({
    page: mockScheduledJobs,
    isDone: true,
    continueCursor: "",
  }))
  .registerQueryFake(udfs.scheduler.getArgs, ({ argsId }) => ({
    _id: argsId,
    _creationTime: NOW - 60_000,
    args: scheduledArgs,
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
  component: ScheduledFunctionsView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/schedules/functions",
        route: "/t/[team]/[project]/[deploymentName]/schedules/functions",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/schedules/functions",
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
    // The scheduler-lag chart is fetched over HTTP from the deployment; without
    // a value the toolbar's SchedulerStatus renders nothing, which is what a
    // healthy deployment looks like.
    mocked(useSchedulerLag).mockReturnValue(undefined);

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
          <ScheduledFunctionsView />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof ScheduledFunctionsView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
