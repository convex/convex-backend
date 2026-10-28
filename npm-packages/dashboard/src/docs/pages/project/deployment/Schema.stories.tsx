import { Meta, StoryObj } from "@storybook/nextjs";
import {
  PermissionsProvider,
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { GenericId } from "convex/values";
import { fn, mocked, userEvent, within, waitFor, expect } from "storybook/test";
import { useTableShapes } from "@common/lib/deploymentApi";
import { Shape } from "shapes";
import { SchemaView } from "@common/features/schema/components/SchemaView";
import { FunctionsContext } from "@common/lib/functions/FunctionsProvider";

// Fixed "now" so timestamps are stable.
const NOW = new Date("2026-03-10T14:25:00Z").getTime();

const mockTeam = {
  id: 2,
  slug: "acme",
  name: "Acme Corp",
};

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

// A saved schema (the developer's `convex/schema.ts`) so the diagram shows the
// intended relationships between tables and the "View schema file" panel has
// something to display.
const mockActiveSchema = {
  schemaValidation: true,
  tables: [
    {
      tableName: "channels",
      documentType: {
        type: "object" as const,
        value: {
          name: { fieldType: { type: "string" as const }, optional: false },
          description: {
            fieldType: { type: "string" as const },
            optional: true,
          },
        },
      },
      indexes: [{ indexDescriptor: "by_name", fields: ["name"] }],
      searchIndexes: [],
      vectorIndexes: [],
    },
    {
      tableName: "messages",
      documentType: {
        type: "object" as const,
        value: {
          body: { fieldType: { type: "string" as const }, optional: false },
          author: {
            fieldType: { type: "id" as const, tableName: "users" },
            optional: false,
          },
          channel: {
            fieldType: { type: "id" as const, tableName: "channels" },
            optional: false,
          },
        },
      },
      indexes: [{ indexDescriptor: "by_channel", fields: ["channel"] }],
      searchIndexes: [
        {
          indexDescriptor: "search_body",
          searchField: "body",
          filterFields: ["channel"],
        },
      ],
      vectorIndexes: [],
    },
    {
      tableName: "users",
      documentType: {
        type: "object" as const,
        value: {
          name: { fieldType: { type: "string" as const }, optional: false },
          email: { fieldType: { type: "string" as const }, optional: false },
          isAdmin: {
            fieldType: { type: "boolean" as const },
            optional: true,
          },
        },
      },
      indexes: [{ indexDescriptor: "by_email", fields: ["email"] }],
      searchIndexes: [],
      vectorIndexes: [],
    },
  ],
};

const mockShapesData: Record<string, Shape> = {
  channels: {
    type: "Object",
    fields: [
      {
        fieldName: "_id",
        optional: false,
        shape: { type: "Id", tableName: "channels" },
      },
      {
        fieldName: "_creationTime",
        optional: false,
        shape: { type: "Float64", float64Range: {} },
      },
      { fieldName: "name", optional: false, shape: { type: "String" } },
      { fieldName: "description", optional: true, shape: { type: "String" } },
    ],
  },
  messages: {
    type: "Object",
    fields: [
      {
        fieldName: "_id",
        optional: false,
        shape: { type: "Id", tableName: "messages" },
      },
      {
        fieldName: "_creationTime",
        optional: false,
        shape: { type: "Float64", float64Range: {} },
      },
      { fieldName: "body", optional: false, shape: { type: "String" } },
      {
        fieldName: "author",
        optional: false,
        shape: { type: "Id", tableName: "users" },
      },
      {
        fieldName: "channel",
        optional: false,
        shape: { type: "Id", tableName: "channels" },
      },
    ],
  },
  users: {
    type: "Object",
    fields: [
      {
        fieldName: "_id",
        optional: false,
        shape: { type: "Id", tableName: "users" },
      },
      {
        fieldName: "_creationTime",
        optional: false,
        shape: { type: "Float64", float64Range: {} },
      },
      { fieldName: "name", optional: false, shape: { type: "String" } },
      { fieldName: "email", optional: false, shape: { type: "String" } },
      { fieldName: "isAdmin", optional: true, shape: { type: "Boolean" } },
    ],
  },
};

const mockConvexClient = mockConvexReactClient()
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(udfs.getSchemas.default, () => ({
    active: JSON.stringify(mockActiveSchema),
    inProgress: undefined,
  }))
  .registerQueryFake(udfs.getSchemas.schemaValidationProgress, () => null)
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
  .registerQueryFake(udfs.fileStorageV2.numFiles, () => 0)
  .registerQueryFake(udfs.tableSize.sizeOfAllTables, () => 0);

const mockConnectedDeployment = {
  deployment: {
    client: mockConvexClient,
    httpClient: {} as never,
    deploymentUrl: mockDeployment.deploymentUrl,
    adminKey: "storybook-admin-key",
    deploymentName: mockDeployment.name,
  },
  isDisconnected: false,
};

const meta = {
  component: SchemaView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/schema",
        route: "/t/[team]/[project]/[deploymentName]/schema",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/schema",
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
  decorators: [
    (storyFn) => {
      const tables = new Map(
        Object.entries(mockShapesData)
          .sort()
          .filter(([name]) => !name.startsWith("_")),
      );
      mocked(useTableShapes).mockReturnValue({ tables, hadError: false });
      return storyFn();
    },
  ],
  render: () => (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockConvexClient}>
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
          <PermissionsProvider>
            <FunctionsContext.Provider value={new Map()}>
              <SchemaView />
            </FunctionsContext.Provider>
          </PermissionsProvider>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof SchemaView>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * The Schema page with the relationship diagram of all tables.
 */
export const Default: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for the diagram to lay out and render table nodes.
    await waitFor(
      async () => {
        await expect(canvas.queryAllByText("messages").length).toBeGreaterThan(
          0,
        );
      },
      { timeout: 10000 },
    );

    // Give the ELK layout and fit-to-view a moment to settle so the whole
    // diagram is centered in the screenshot.
    await new Promise((resolve) => {
      setTimeout(resolve, 750);
    });
  },
};

/**
 * The "View schema file" panel, showing the generated/saved schema code.
 */
export const SchemaFile: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for the "View schema file" button to be enabled (data has loaded).
    await waitFor(async () => {
      await expect(
        canvas.queryByRole("button", { name: /view schema/i }),
      ).toBeTruthy();
    });

    await userEvent.click(canvas.getByRole("button", { name: /view schema/i }));
  },
};
