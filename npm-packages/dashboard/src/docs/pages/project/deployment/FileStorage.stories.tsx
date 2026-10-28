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
import { FileStorageView } from "@common/features/files/components/FileStorageView";

// Fixed "now" so timestamps are stable.
const NOW = new Date("2026-04-28T14:25:00Z").getTime();

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
const mockFiles = [
  {
    _id: "kg25efbgcywfzkr6h78cjw1pzx85r2ha" as GenericId<"_storage">,
    _creationTime: now - 10 * 60 * 1000,
    sha256: "a".repeat(64),
    size: 245_000,
    contentType: "image/png",
    url: "https://happy-capybara-123.convex.cloud/api/storage/1",
  },
  {
    _id: "kg26cx7446n79enzjshegcv4bs85s5zf" as GenericId<"_storage">,
    _creationTime: now - 3 * 60 * 60 * 1000,
    sha256: "b".repeat(64),
    size: 1_800_000,
    contentType: "application/pdf",
    url: "https://happy-capybara-123.convex.cloud/api/storage/2",
  },
  {
    _id: "kg204m40v9m1bmkthp86n486mh85s0rc" as GenericId<"_storage">,
    _creationTime: now - 24 * 60 * 60 * 1000,
    sha256: "c".repeat(64),
    size: 58_000,
    contentType: "text/csv",
    url: "https://happy-capybara-123.convex.cloud/api/storage/3",
  },
  {
    _id: "kg2dsa1sfhkmydncfvvtd2jgbx85rw9z" as GenericId<"_storage">,
    _creationTime: now - 2 * 24 * 60 * 60 * 1000,
    sha256: "d".repeat(64),
    size: 12_000_000,
    contentType: "video/mp4",
    url: "https://happy-capybara-123.convex.cloud/api/storage/4",
  },
  {
    _id: "kg2ch8s6wsdzvkefhzacfjvyvd85r1f1" as GenericId<"_storage">,
    _creationTime: now - 5 * 24 * 60 * 60 * 1000,
    sha256: "e".repeat(64),
    size: 3_400,
    contentType: "application/json",
    url: "https://happy-capybara-123.convex.cloud/api/storage/5",
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
  .registerQueryFake(udfs.fileStorageV2.numFiles, () => mockFiles.length)
  .registerQueryFake(udfs.fileStorageV2.fileMetadata, () => ({
    page: mockFiles,
    isDone: true,
    continueCursor: "",
  }))
  .registerQueryFake(udfs.fileStorageV2.getFile, () => null)
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
  component: FileStorageView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/files",
        route: "/t/[team]/[project]/[deploymentName]/files",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/files",
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
            deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
            projectsURI: "/t/acme/my-amazing-app",
            teamsURI: "/t/acme",
            isSelfHosted: false,
          }}
        >
          <FileStorageView />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof FileStorageView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
