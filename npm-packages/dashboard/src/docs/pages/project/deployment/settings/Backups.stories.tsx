import { Meta, StoryObj } from "@storybook/nextjs";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { fn, mocked, userEvent, within } from "storybook/test";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Id } from "system-udfs/convex/_generated/dataModel";
import {
  BackupResponse,
  useGetCloudBackup,
  useGetPeriodicBackupConfig,
  useListCloudBackups,
  useListCloudBackupsIfAvailable,
} from "api/backups";
import { useDeploymentByName } from "api/deployments";
import { useInfiniteProjects } from "api/projects";
import { useCurrentTeam, useTeamEntitlements } from "api/teams";
import { Backups } from "components/deploymentSettings/Backups";
import {
  type BackupStorageSummary,
  useBackupStorageSummary,
} from "hooks/usageMetrics";

// Fixed "now" so the relative timestamps ("Expires in 6 days") are stable. The
// absolute times this page also renders (`toLocaleString`, `Intl.DateTimeFormat`
// and the timezone abbreviation next to the schedule) depend on the browser's
// locale and timezone, which the screenshot capture pins to en-US/UTC.
const NOW = new Date("2026-04-01T09:41:00Z").getTime();
const DAY = 24 * 60 * 60 * 1000;

const mockTeam = { id: 2, slug: "acme", name: "Acme Corp" };
const mockProject = {
  id: 7,
  teamId: mockTeam.id,
  teamSlug: mockTeam.slug,
  name: "My amazing app",
  slug: "my-amazing-app",
  createTime: NOW - 200 * DAY,
};
// Matches STORYBOOK_PROD_DEPLOYMENT in the docs page decorator, which renders
// the surrounding dashboard shell for `docsPage.deploymentType: "prod"`.
const mockDeployment: PlatformDeploymentResponse = {
  id: 12,
  name: "musical-otter-456",
  deploymentType: "prod",
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: NOW - 120 * DAY,
  class: "s256",
  deploymentUrl: "https://musical-otter-456.eu-west-1.convex.cloud",
  reference: "production",
  region: "aws-eu-west-1",
};

// One backup a day for the past week, each expiring 7 days after it was taken.
const mockBackups: BackupResponse[] = Array.from({ length: 7 }, (_, i) => {
  const requestedTime = NOW - (i + 1) * DAY + 3 * 60 * 60 * 1000;
  return {
    id: 4000 + (7 - i),
    snapshotId: `ex${i}7dnq3wr8cbk2vmtx5jz9a4hf6gsy` as Id<"_exports">,
    sourceDeploymentId: mockDeployment.id,
    sourceDeploymentName: mockDeployment.name,
    state: "complete",
    requestedTime,
    completedTime: requestedTime + 42_000,
    expirationTime: requestedTime + 7 * DAY,
    includeStorage: false,
  };
});

const mockPeriodicBackupConfig = {
  sourceDeploymentId: mockDeployment.id,
  cronspec: "0 17 * * *",
  expirationDeltaSecs: 7 * 24 * 60 * 60,
  // The 17:00 UTC run following `NOW`, so the "next backup" line agrees with
  // the schedule the selector renders from the cronspec.
  nextRun: new Date("2026-04-01T17:00:00Z").getTime(),
  includeStorage: false,
};

const mockUsageSummary: BackupStorageSummary = {
  databaseStorage: 5 * 1024 * 1024 * 1024,
  fileStorage: 10 * 1024 * 1024 * 1024,
};

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.latestExport.default, () => null)
  .registerQueryFake(udfs.latestExport.latestCloudExport, () => null)
  .registerQueryFake(udfs.snapshotImport.list, () => []);

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

function BackupsPage() {
  const team = useCurrentTeam()!;
  const entitlements = useTeamEntitlements(team.id)!;

  return (
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
            deploymentsURI: "/t/acme/my-amazing-app/musical-otter-456",
            projectsURI: "/t/acme/my-amazing-app",
            teamsURI: "/t/acme",
            isSelfHosted: false,
          }}
        >
          <DeploymentSettingsLayout page="backups">
            <Backups
              team={team}
              deployment={mockDeployment}
              entitlements={entitlements}
            />
          </DeploymentSettingsLayout>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

const meta = {
  component: BackupsPage,
  parameters: {
    layout: "fullscreen",
    docsPage: { deploymentType: "prod" },
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/settings/backups",
        route: "/t/[team]/[project]/[deploymentName]/settings/backups",
        asPath: "/t/acme/my-amazing-app/musical-otter-456/settings/backups",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "musical-otter-456",
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
  decorators: [
    (storyFn) => {
      // These run in a decorator rather than `beforeEach` because the docs page
      // decorator mocks the backup hooks to an empty list on every render.
      mocked(useListCloudBackups).mockReturnValue(mockBackups);
      mocked(useListCloudBackupsIfAvailable).mockReturnValue(mockBackups);
      mocked(useGetPeriodicBackupConfig).mockReturnValue(
        mockPeriodicBackupConfig,
      );
      mocked(useGetCloudBackup).mockReturnValue(undefined);
      mocked(useBackupStorageSummary).mockReturnValue({
        data: mockUsageSummary,
        error: undefined,
      });
      // Each list item resolves its backup identifier from the source
      // deployment name.
      mocked(useDeploymentByName).mockReturnValue(mockDeployment);
      mocked(useInfiniteProjects).mockReturnValue({
        projects: [mockProject],
        isLoading: false,
        isLoadingMore: false,
        hasMore: false,
        loadMore: fn(),
        debouncedQuery: "",
        pageSize: 25,
      });
      return storyFn();
    },
  ],
  render: () => <BackupsPage />,
} satisfies Meta<typeof BackupsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const RequestImmediateBackup: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"]',
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: "Backup Now" }),
    );

    const dialog = within(await within(document.body).findByRole("dialog"));
    await dialog.findByText("Request an immediate backup");
    await dialog.findByText(/\(Est\. 5\sGB\)/);
    await dialog.findByText(/\(Est\. \+10\sGB\)/);
  },
};

export const RequestImmediateBackupWithTooMuchFileStorage: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"], [role="tooltip"]',
  },
  decorators: [
    (storyFn) => {
      mocked(useBackupStorageSummary).mockReturnValue({
        data: { ...mockUsageSummary, fileStorage: 10 * 1024 ** 4 },
        error: undefined,
      });
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      await canvas.findByRole("button", { name: "Backup Now" }),
    );

    const dialog = within(await within(document.body).findByRole("dialog"));
    await dialog.findByText(/\(Est\. \+10\sTB\)/);
    await userEvent.hover(dialog.getByText("Include file storage"));
    await within(document.body).findAllByText(
      "Backups can include up to 1 TB of file storage. This deployment has more than 1 TB.",
    );
  },
};
