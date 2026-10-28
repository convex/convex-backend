import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { DeploymentSummary } from "./DeploymentSummary";
import { BackfillingIndexes, SchemaValidationByTable } from "./DeployProgress";

const mockLastPushEvent = {
  _id: "123" as any,
  _creationTime: Date.now() - 1000 * 60 * 30, // 30 minutes ago
  member_id: BigInt(123),
  action: "push_config" as const,
  metadata: {} as any,
} as any;

const mockConvexCloudUrl = "https://happy-animal-123.convex.cloud";
const mockConvexSiteUrl = "https://happy-animal-123.convex.site";

const mockClient = mockConvexReactClient()
  .registerQueryFake(
    udfs.deploymentEvents.lastPushEvent,
    () => mockLastPushEvent,
  )
  .registerQueryFake(udfs.convexCloudUrl.default, () => mockConvexCloudUrl)
  .registerQueryFake(udfs.convexSiteUrl.default, () => mockConvexSiteUrl)
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(
    udfs.getSchemas.schemaValidationProgressByTable,
    () => null,
  )
  .registerQueryFake(udfs.indexes.backfilling, () => []);

const mockClientNeverDeployed = mockConvexReactClient()
  .registerQueryFake(udfs.deploymentEvents.lastPushEvent, () => null)
  .registerQueryFake(udfs.convexCloudUrl.default, () => mockConvexCloudUrl)
  .registerQueryFake(udfs.convexSiteUrl.default, () => mockConvexSiteUrl)
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(
    udfs.getSchemas.schemaValidationProgressByTable,
    () => null,
  )
  .registerQueryFake(udfs.indexes.backfilling, () => []);

// A deploy still validating a schema and backfilling indexes.
const mockClientDeploying = mockConvexReactClient()
  .registerQueryFake(
    udfs.deploymentEvents.lastPushEvent,
    () => mockLastPushEvent,
  )
  .registerQueryFake(udfs.convexCloudUrl.default, () => mockConvexCloudUrl)
  .registerQueryFake(udfs.convexSiteUrl.default, () => mockConvexSiteUrl)
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(
    udfs.getSchemas.schemaValidationProgressByTable,
    (): SchemaValidationByTable => ({
      numDocsValidated: 432_400,
      totalDocs: 1_200_000,
      tables: [
        {
          tableName: "books",
          staged: false,
          state: "pending",
          numDocsValidated: 428_000,
          totalDocs: 1_190_000,
        },
        {
          tableName: "pages",
          staged: false,
          state: "valid",
          numDocsValidated: 400,
          totalDocs: 400,
        },
        {
          tableName: "reviews",
          staged: true,
          state: "pending",
          numDocsValidated: 4_000,
          totalDocs: 9_600,
        },
      ],
    }),
  )
  .registerQueryFake(
    udfs.indexes.backfilling,
    (): BackfillingIndexes => [
      {
        tableName: "books",
        name: "by_author",
        kind: "database",
        staged: false,
        stats: { numDocsIndexed: 380_000, totalDocs: 1_190_000 },
      },
      {
        tableName: "pages",
        name: "search_body",
        kind: "search",
        staged: true,
        stats: { numDocsIndexed: 12_000, totalDocs: 88_000 },
      },
    ],
  );

function mockClientWithSchemaProgress(progress: SchemaValidationByTable) {
  return mockConvexReactClient()
    .registerQueryFake(
      udfs.deploymentEvents.lastPushEvent,
      () => mockLastPushEvent,
    )
    .registerQueryFake(udfs.convexCloudUrl.default, () => mockConvexCloudUrl)
    .registerQueryFake(udfs.convexSiteUrl.default, () => mockConvexSiteUrl)
    .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
    .registerQueryFake(
      udfs.getSchemas.schemaValidationProgressByTable,
      () => progress,
    )
    .registerQueryFake(udfs.indexes.backfilling, () => []);
}

// Staged validators on the active schema, with no push in flight. The settled
// `pages` attempt stays listed while `reviews` is still validating.
const mockClientStagedValidation = mockClientWithSchemaProgress({
  numDocsValidated: 404_400,
  totalDocs: 1_200_400,
  tables: [
    {
      tableName: "pages",
      staged: true,
      state: "valid",
      numDocsValidated: 400,
      totalDocs: 400,
    },
    {
      tableName: "reviews",
      staged: true,
      state: "pending",
      numDocsValidated: 404_000,
      totalDocs: 1_200_000,
    },
  ],
});

// A staged validator that found a non-matching document while another is
// still validating.
const mockClientStagedValidationFailed = mockClientWithSchemaProgress({
  numDocsValidated: 61_000,
  totalDocs: 129_600,
  tables: [
    {
      tableName: "authors",
      staged: true,
      state: "failed",
      error: "Document with ID abc123 does not match the staged schema",
      numDocsValidated: 57_000,
      totalDocs: 120_000,
    },
    {
      tableName: "reviews",
      staged: true,
      state: "pending",
      numDocsValidated: 4_000,
      totalDocs: 9_600,
    },
  ],
});

// A push validating `books` against its new enforced validator while the
// active schema's staged validator for `books` is still validating.
const mockClientStagedAndEnforcedSameTable = mockClientWithSchemaProgress({
  numDocsValidated: 1_028_000,
  totalDocs: 2_380_000,
  tables: [
    {
      tableName: "books",
      staged: false,
      state: "pending",
      numDocsValidated: 428_000,
      totalDocs: 1_190_000,
    },
    {
      tableName: "books",
      staged: true,
      state: "pending",
      numDocsValidated: 600_000,
      totalDocs: 1_190_000,
    },
  ],
});

const prodDeployment: PlatformDeploymentResponse = {
  id: 1,
  name: "happy-animal-123",
  deploymentType: "prod",
  projectId: 1,
  kind: "cloud",
  class: "s16",
  region: "aws-us-east-1",
  createTime: Date.now() - 1000 * 60 * 60 * 24 * 7, // 1 week ago
  isDefault: true,
  creator: null,
  previewIdentifier: null,
  reference: "production",
  deploymentUrl: "https://happy-animal-123.convex.cloud",
};

const devCloudDeployment: PlatformDeploymentResponse = {
  id: 2,
  name: "joyful-capybara-123",
  deploymentType: "dev",
  projectId: 1,
  kind: "cloud",
  class: "s16",
  region: "aws-eu-west-1",
  createTime: Date.now() - 1000 * 60 * 60 * 24 * 2, // 2 days ago
  isDefault: false,
  creator: 123,
  previewIdentifier: null,
  reference: "dev/nicolas",
  deploymentUrl: "https://joyful-capybara-123.convex.cloud",
};

const devLocalDeployment: PlatformDeploymentResponse = {
  name: "local-dev-789",
  deploymentType: "dev",
  projectId: 1,
  kind: "local",
  createTime: Date.now() - 1000 * 60 * 60, // 1 hour ago
  creator: 123,
  deviceName: "MacBook Pro",
  port: 3210,
  isActive: true,
  previewIdentifier: null,
};

const previewDeployment: PlatformDeploymentResponse = {
  id: 4,
  name: "musical-dog-123",
  deploymentType: "preview",
  projectId: 1,
  kind: "cloud",
  class: "s16",
  region: "aws-us-east-1",
  createTime: Date.now() - 1000 * 60 * 60 * 12, // 12 hours ago
  isDefault: false,
  creator: 456,
  previewIdentifier: "pr-42",
  reference: "preview/my-feature",
  deploymentUrl: "https://musical-dog-123.convex.cloud",
};

const previewDeploymentWithExpiry: PlatformDeploymentResponse = {
  id: 6,
  name: "curious-lemur-456",
  deploymentType: "preview",
  projectId: 1,
  kind: "cloud",
  class: "s16",
  region: "aws-us-east-1",
  createTime: Date.now() - 1000 * 60 * 60 * 24, // 1 day ago
  isDefault: false,
  creator: 456,
  previewIdentifier: "pr-99",
  reference: "preview/add-feature",
  deploymentUrl: "https://curious-lemur-456.convex.cloud",
  expiresAt: Date.now() + 1000 * 60 * 60 * 24 * 3, // 3 days from now
};

const d1024Deployment: PlatformDeploymentResponse = {
  id: 7,
  name: "speedy-falcon-789",
  deploymentType: "prod",
  projectId: 1,
  kind: "cloud",
  class: "d1024",
  region: "aws-us-east-1",
  createTime: Date.now() - 1000 * 60 * 60 * 24 * 7, // 1 week ago
  isDefault: true,
  creator: null,
  previewIdentifier: null,
  reference: "production",
  deploymentUrl: "https://speedy-falcon-789.convex.cloud",
};

const customDeployment: PlatformDeploymentResponse = {
  id: 5,
  name: "wandering-fish-513",
  deploymentType: "custom",
  projectId: 1,
  kind: "cloud",
  class: "s16",
  region: "aws-us-east-1",
  createTime: Date.now() - 1000 * 60 * 60 * 24 * 30, // 30 days ago
  isDefault: false,
  creator: 789,
  previewIdentifier: null,
  reference: "staging",
  deploymentUrl: "https://wandering-fish-513.convex.cloud",
};

const mockRegions = [
  { name: "aws-us-east-1", displayName: "US East (N. Virginia)" },
  { name: "aws-eu-west-1", displayName: "EU (Ireland)" },
];

const meta = {
  component: DeploymentSummary,
  args: {
    deployment: prodDeployment,
    teamSlug: "my-team",
    projectSlug: "my-project",
    lastBackupTime: Date.now() - 1000 * 60 * 60 * 24, // 1 day ago
    teamMembers: [{ id: 123, name: "Ari", email: "ari@example.com" }],
    regions: mockRegions,
  },
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof DeploymentSummary>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Production: Story = {};

export const ProductionWithRecentBackup: Story = {
  args: {
    lastBackupTime: Date.now() - 1000 * 60 * 15, // 15 minutes ago
  },
};

export const ProductionNoBackups: Story = {
  args: {
    lastBackupTime: null,
  },
};

export const ProductionNeverDeployed: Story = {
  render: (args) => (
    <ConvexProvider client={mockClientNeverDeployed}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};

export const ProductionDeploying: Story = {
  render: (args) => (
    <ConvexProvider client={mockClientDeploying}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} showDeployProgress />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};

export const ProductionStagedValidation: Story = {
  render: (args) => (
    <ConvexProvider client={mockClientStagedValidation}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} showDeployProgress />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};

export const ProductionStagedValidationFailed: Story = {
  render: (args) => (
    <ConvexProvider client={mockClientStagedValidationFailed}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} showDeployProgress />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};

export const ProductionStagedAndEnforcedSameTable: Story = {
  render: (args) => (
    <ConvexProvider client={mockClientStagedAndEnforcedSameTable}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} showDeployProgress />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};

export const DevelopmentCloud: Story = {
  args: {
    deployment: devCloudDeployment,
  },
};

export const DevelopmentLocal: Story = {
  args: {
    deployment: devLocalDeployment,
  },
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};

export const Preview: Story = {
  args: {
    deployment: previewDeployment,
  },
};

export const Custom: Story = {
  args: {
    deployment: customDeployment,
  },
};

export const D1024Production: Story = {
  args: {
    deployment: d1024Deployment,
    lastBackupTime: null,
  },
};

export const PreviewWithExpiry: Story = {
  args: {
    deployment: previewDeploymentWithExpiry,
  },
};

export const Loading: Story = {
  render: (args) => (
    <ConvexProvider
      client={mockConvexReactClient()
        .registerQueryFake(
          udfs.deploymentEvents.lastPushEvent,
          () => undefined as any,
        )
        .registerQueryFake(udfs.convexCloudUrl.default, () => undefined as any)
        .registerQueryFake(udfs.convexSiteUrl.default, () => undefined as any)
        .registerQueryFake(udfs.getVersion.default, () => undefined as any)
        .registerQueryFake(
          udfs.getSchemas.schemaValidationProgressByTable,
          () => undefined as any,
        )
        .registerQueryFake(udfs.indexes.backfilling, () => undefined as any)}
    >
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-4xl">
          <DeploymentSummary {...args} lastBackupTime={undefined} />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
};
