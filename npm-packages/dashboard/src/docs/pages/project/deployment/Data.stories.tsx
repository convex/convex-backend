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
import { GenericDocument } from "convex/server";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { useDeployments } from "api/deployments";
import { useInfiniteProjects } from "api/projects";
import {
  fn,
  mocked,
  screen,
  userEvent,
  within,
  waitFor,
  expect,
} from "storybook/test";
import { useTableShapes } from "@common/lib/deploymentApi";
import { Shape } from "shapes";
import { DataView } from "@common/features/data/components/DataView";
import { FunctionsContext } from "@common/lib/functions/FunctionsProvider";
import { api } from "system-udfs/convex/_generated/api";

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
      {
        fieldName: "isAdmin",
        optional: true,
        shape: { type: "Boolean" },
      },
    ],
  },
};

const now = NOW;

const mockDocuments: Record<string, GenericDocument[]> = {
  channels: [
    {
      _id: "k17cx2tgs5e3ah8b0gnkq9de2s6yxr3w" as GenericId<string>,
      _creationTime: now - 100000,
      name: "general",
      description: "General discussion",
    },
    {
      _id: "k17d83nrq4a7pm5g1fhev0jb9t6zxc2k" as GenericId<string>,
      _creationTime: now - 90000,
      name: "engineering",
      description: "Engineering team",
    },
    {
      _id: "k17eqw4hy8b2vn6d3jrms0fa1t5gxp7z" as GenericId<string>,
      _creationTime: now - 80000,
      name: "random",
    },
  ],
  messages: [
    {
      _id: "k27fv9xgn3c4wk8e2dpht1ja6m5byr0q" as GenericId<string>,
      _creationTime: now - 50000,
      body: "Hello, world!",
      author: "k37ax5mrd2e8tn4f1ghqv0jb7w6ypc9k",
      channel: "k17cx2tgs5e3ah8b0gnkq9de2s6yxr3w",
    },
    {
      _id: "k27gm6bpz1d9xr3h5eqwt4ka8n7fyc2j" as GenericId<string>,
      _creationTime: now - 40000,
      body: "Welcome to our app",
      author: "k37bh2nqe7f4yk9g3jtmv1da6w8xpc5r",
      channel: "k17cx2tgs5e3ah8b0gnkq9de2s6yxr3w",
    },
    {
      _id: "k27hn3ctw8e5yp6j2frvx1kb4m9gqd7a" as GenericId<string>,
      _creationTime: now - 30000,
      body: "Great work on the new feature!",
      author: "k37ax5mrd2e8tn4f1ghqv0jb7w6ypc9k",
      channel: "k17d83nrq4a7pm5g1fhev0jb9t6zxc2k",
    },
  ],
  users: [
    {
      _id: "k37ax5mrd2e8tn4f1ghqv0jb7w6ypc9k" as GenericId<string>,
      _creationTime: now - 200000,
      name: "Alice Johnson",
      email: "alice@example.com",
      isAdmin: true,
    },
    {
      _id: "k37bh2nqe7f4yk9g3jtmv1da6w8xpc5r" as GenericId<string>,
      _creationTime: now - 190000,
      name: "Bob Smith",
      email: "bob@example.com",
    },
  ],
};

const mockConvexClient = mockConvexReactClient()
  .registerQueryFake(udfs.components.list, () => [])
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(udfs.getSchemas.default, () => ({
    active: undefined,
    inProgress: undefined,
  }))
  .registerQueryFake(udfs.getSchemas.schemaValidationProgress, () => null)
  .registerQueryFake(
    udfs.tableSize.default,
    ({ tableName }: { tableName: string }) =>
      mockDocuments[tableName]?.length ?? 0,
  )
  .registerQueryFake(
    udfs.paginatedTableDocuments.default,
    ({ table }: { table: string }) => ({
      page: mockDocuments[table] ?? [],
      isDone: true,
      continueCursor: "",
    }),
  )
  .registerQueryFake(api._system.frontend.indexes.default, () => [
    {
      name: "by_name",
      fields: ["name", "_creation_time"],
      staged: false,
      backfill: { state: "done" as const },
    },
    {
      name: "search_description",
      fields: { searchField: "description", filterFields: ["name"] },
      staged: false,
      backfill: { state: "done" as const },
    },
  ])
  .registerQueryFake(udfs.getTableMapping.default, () => ({
    1: "channels",
    2: "messages",
    3: "users",
  }))
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
  component: DataView,
  parameters: {
    layout: "fullscreen",
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/data",
        route: "/t/[team]/[project]/[deploymentName]/data",
        asPath: "/t/acme/my-amazing-app/happy-capybara-123/data",
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
  render: renderDataPage,
} satisfies Meta<typeof DataView>;

function renderDataPage() {
  return (
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
          <FunctionsContext.Provider value={new Map()}>
            <DataView />
          </FunctionsContext.Provider>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

/** Shows the Data page's index filter bar, sorting by the `by_name` index. */
export const IndexFilterBar: Story = {
  parameters: {
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/data",
        route: "/t/[team]/[project]/[deploymentName]/data",
        asPath:
          "/t/acme/my-amazing-app/happy-capybara-123/data?filters=eyJjbGF1c2VzIjpbXSwiaW5kZXgiOnsibmFtZSI6ImJ5X25hbWUiLCJjbGF1c2VzIjpbeyJ0eXBlIjoiaW5kZXhFcSIsImVuYWJsZWQiOnRydWUsInZhbHVlIjoiZ2VuZXJhbCJ9LHsidHlwZSI6ImluZGV4RXEiLCJlbmFibGVkIjpmYWxzZSwidmFsdWUiOjE3NzU1MTUxNjk5NDd9XX19",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
          filters:
            "eyJjbGF1c2VzIjpbXSwiaW5kZXgiOnsibmFtZSI6ImJ5X25hbWUiLCJjbGF1c2VzIjpbeyJ0eXBlIjoiaW5kZXhFcSIsImVuYWJsZWQiOnRydWUsInZhbHVlIjoiZ2VuZXJhbCJ9LHsidHlwZSI6ImluZGV4RXEiLCJlbmFibGVkIjpmYWxzZSwidmFsdWUiOjE3NzU1MTUxNjk5NDd9XX19",
        },
      },
    },
    screenshotSelector: '[data-testid="indexFilterBar"]',
  },
};

/**
 * Shows the Data page with "Add documents" panel open.
 */
export const AddDocument: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Anchored to the exact name: the filter bar's "Add a filter" button also
    // starts with "add".
    await waitFor(async () => {
      await expect(
        canvas.getByRole("button", { name: /^add$/i }),
      ).toBeDefined();
    });

    await userEvent.click(canvas.getByRole("button", { name: /^add$/i }));

    // Wait for the add documents panel to appear
    await waitFor(async () => {
      const inputs = canvas.queryAllByRole("textbox");
      await expect(inputs.length).toBeGreaterThan(0);
    });
  },
};

/**
 * Shows inline cell editor when double-clicking a cell.
 */
export const EditInline: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for the table to be visible with data cells
    let cellButton: HTMLElement | null = null;
    await waitFor(async () => {
      const buttons = canvas.queryAllByTestId("cell-editor-button");
      cellButton =
        buttons.find((btn) =>
          btn.textContent?.includes("General discussion"),
        ) ?? null;
      await expect(cellButton).toBeTruthy();
    });

    // Double-click the cell button to open the inline editor
    if (cellButton) {
      await userEvent.dblClick(cellButton);
    }

    // Cell editor popper renders in a portal at document.body
    const body = within(document.body);
    await waitFor(async () => {
      await expect(body.queryByTestId("cell-editor-popper")).toBeTruthy();
    });
  },
  parameters: {
    screenshotSelector: '[data-testid="cell-editor-popper"]',
  },
};

/**
 * Shows context menu when right-clicking a table cell.
 */
export const EditDocument: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for cells to render
    let cellButton: HTMLElement | null = null;
    await waitFor(async () => {
      const buttons = canvas.queryAllByTestId("cell-editor-button");
      cellButton =
        buttons.find((btn) =>
          btn.textContent?.includes("General discussion"),
        ) ?? null;
      await expect(cellButton).toBeTruthy();
    });

    // Dispatch a native contextmenu event (the context menu listens via addEventListener)
    if (cellButton) {
      const rect = (cellButton as HTMLElement).getBoundingClientRect();
      (cellButton as HTMLElement).dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          clientX: rect.left + rect.width / 2,
          clientY: rect.top + rect.height / 2,
        }),
      );
    }

    // Context menu renders in a FloatingPortal, query from document.body
    const body = within(document.body);
    await waitFor(async () => {
      await expect(body.queryByTestId("table-context-menu")).toBeTruthy();
    });

    // Navigate to "Edit Document" using native KeyboardEvents on the menu.
    // The ContextMenu uses Floating UI's useListNavigation which tracks
    // activeIndex via keyboard events on the menu's role="menu" element.
    // Menu items: View desc, Copy desc, Edit desc, Filter by desc,
    //             View Doc, Copy Doc, Edit Doc, Delete Doc
    // 7 ArrowDown presses from null activeIndex to reach "Edit Document".
    const menuEl = body.getByRole("menu");
    menuEl.focus();
    for (let i = 0; i < 7; i++) {
      menuEl.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowDown",
          bubbles: true,
        }),
      );
      // Small delay to let React process each navigation step
      await new Promise((r) => setTimeout(r, 50));
    }
  },
  parameters: {
    screenshotSelector: '[data-testid="table-context-menu"]',
  },
};

/**
 * Shows context menu with expanded submenu.
 */
export const ContextMenu: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for cells to render
    let cellButton: HTMLElement | null = null;
    await waitFor(async () => {
      const buttons = canvas.queryAllByTestId("cell-editor-button");
      cellButton =
        buttons.find((btn) =>
          btn.textContent?.includes("General discussion"),
        ) ?? null;
      await expect(cellButton).toBeTruthy();
    });

    // Dispatch a native contextmenu event
    if (cellButton) {
      const rect = (cellButton as HTMLElement).getBoundingClientRect();
      (cellButton as HTMLElement).dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          clientX: rect.left + rect.width / 2,
          clientY: rect.top + rect.height / 2,
        }),
      );
    }

    // Context menu renders in a FloatingPortal, query from document.body
    const body = within(document.body);
    await waitFor(async () => {
      await expect(body.queryByTestId("table-context-menu")).toBeTruthy();
    });

    // Hover over "Filter by" submenu item to expand it
    const contextMenu = body.getByTestId("table-context-menu");
    const filterByItem = within(contextMenu).queryByRole("menuitem", {
      name: /filter by/i,
    });
    if (filterByItem) {
      await userEvent.hover(filterByItem);
    }
  },
  parameters: {
    screenshotSelector: '[role="menu"]',
  },
};

/**
 * Shows Data page with 2 rows selected via checkboxes.
 */
export const BulkEdit: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for the table to be visible
    await waitFor(async () => {
      const checkboxes = canvas.queryAllByRole("checkbox");
      await expect(checkboxes.length).toBeGreaterThan(0);
    });

    // Click the first row checkbox
    const checkboxes = canvas.getAllByRole("checkbox");
    if (checkboxes.length > 0) {
      await userEvent.click(checkboxes[0]);
    }

    // Click the second row checkbox
    if (checkboxes.length > 1) {
      await userEvent.click(checkboxes[1]);
    }

    // Wait to ensure rows are selected
    await waitFor(async () => {
      const selectedCheckboxes = canvas.queryAllByRole("checkbox", {
        checked: true,
      });
      await expect(selectedCheckboxes.length).toBeGreaterThanOrEqual(2);
    });
  },
};

/**
 * Shows Data page with overflow menu (three-dot menu) open.
 */
export const CustomQuery: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for the overflow menu button to be visible
    await waitFor(async () => {
      await expect(canvas.queryByLabelText("Open table settings")).toBeTruthy();
    });

    // Click the overflow menu button (aria-label="Open table settings")
    const overflowButton = canvas.getByLabelText("Open table settings");
    await userEvent.click(overflowButton);

    // Menu items render in a Headless UI Portal, query from document.body
    const body = within(document.body);
    await waitFor(async () => {
      await expect(body.queryAllByRole("menuitem").length).toBeGreaterThan(0);
    });
  },
  parameters: {
    screenshotSelector: '[role="menu"]',
  },
};

/**
 * Shows Data page with overflow menu open and "Custom query" item highlighted.
 */
export const CustomQueryRunner: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);

    // Wait for the overflow menu button to be visible
    await waitFor(async () => {
      await expect(canvas.queryByLabelText("Open table settings")).toBeTruthy();
    });

    // Click the overflow menu button
    const overflowButton = canvas.getByLabelText("Open table settings");
    await userEvent.click(overflowButton);

    // Menu items render in a Headless UI Portal, query from document.body
    const body = within(document.body);
    await waitFor(async () => {
      await expect(body.queryAllByRole("menuitem").length).toBeGreaterThan(0);
    });

    // Click on "Custom query" menu item
    const customQueryItem = body.queryByRole("menuitem", {
      name: /custom query/i,
    });
    if (customQueryItem) {
      await userEvent.click(customQueryItem);
    }
  },
};

const mockComponents = [
  {
    id: "k17componentsaaaaaaaaaaaaaaaa1111" as GenericId<"_components">,
    name: "rateLimiter",
    path: "rateLimiter",
    args: {},
    state: "active" as const,
    httpPrefix: null,
  },
  {
    id: "k17componentsaaaaaaaaaaaaaaaa2222" as GenericId<"_components">,
    name: "migrations",
    path: "migrations",
    args: {},
    state: "active" as const,
    httpPrefix: null,
  },
];

const mockConvexClientWithComponents = mockConvexReactClient()
  .registerQueryFake(udfs.components.list, () => mockComponents)
  .registerQueryFake(udfs.getVersion.default, () => "1.18.0")
  .registerQueryFake(udfs.getSchemas.default, () => ({
    active: undefined,
    inProgress: undefined,
  }))
  .registerQueryFake(udfs.getSchemas.schemaValidationProgress, () => null)
  .registerQueryFake(
    udfs.tableSize.default,
    ({ tableName }: { tableName: string }) =>
      mockDocuments[tableName]?.length ?? 0,
  )
  .registerQueryFake(
    udfs.paginatedTableDocuments.default,
    ({ table }: { table: string }) => ({
      page: mockDocuments[table] ?? [],
      isDone: true,
      continueCursor: "",
    }),
  )
  .registerQueryFake(api._system.frontend.indexes.default, () => [
    {
      name: "by_name",
      fields: ["name", "_creation_time"],
      staged: false,
      backfill: { state: "done" as const },
    },
    {
      name: "search_description",
      fields: { searchField: "description", filterFields: ["name"] },
      staged: false,
      backfill: { state: "done" as const },
    },
  ])
  .registerQueryFake(udfs.getTableMapping.default, () => ({
    1: "channels",
    2: "messages",
    3: "users",
  }))
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

const mockConnectedDeploymentWithComponents = {
  deployment: {
    client: mockConvexClientWithComponents,
    httpClient: {} as never,
    deploymentUrl: mockDeployment.deploymentUrl,
    adminKey: "storybook-admin-key",
    deploymentName: mockDeployment.name,
  },
  isDisconnected: false,
};

/**
 * Shows the component dropdown expanded in the Data page header.
 */
export const ComponentDropdown: Story = {
  parameters: {
    screenshotSelector:
      '[data-testid="combobox-button-Select component"], [role="listbox"], [data-testid="home-link"]',
  },
  render: () => (
    <ConnectedDeploymentContext.Provider
      value={mockConnectedDeploymentWithComponents}
    >
      <ConvexProvider client={mockConvexClientWithComponents}>
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
              <DataView />
            </FunctionsContext.Provider>
          </PermissionsProvider>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await waitFor(async () => {
      await expect(
        canvas.queryByTestId("combobox-button-Select component"),
      ).toBeTruthy();
    });
    await userEvent.click(
      canvas.getByTestId("combobox-button-Select component"),
    );
  },
};

/**
 * Open a header trigger's anchored command palette menu.
 *
 * `DeploymentInfoProvider` moves its children from a fragment into a context
 * provider once deployment auth resolves, which makes React rebuild the page
 * subtree. A trigger looked up before that rebuild is detached by the time it
 * is clicked, and clicking a detached node dispatches events that reach no
 * handler — so re-look-up the trigger on every attempt and retry until the menu
 * it opens is on screen. The guard keeps a retry from closing a menu that did
 * open and is only slow to fill in.
 */
async function openAnchoredMenu(
  getTrigger: () => HTMLElement,
  getMenuContent: () => HTMLElement,
) {
  await waitFor(
    async () => {
      const trigger = getTrigger();
      if (!trigger.ownerDocument.querySelector(".command-palette--anchored")) {
        await userEvent.click(trigger);
      }
      getMenuContent();
    },
    { timeout: 10_000 },
  );
}

export const MultipleDevDeploymentsSelector: Story = {
  parameters: {
    ...meta.parameters,
    // Clicking the deployment badge opens the command palette's deployment
    // switcher menu (a cmdk dialog anchored beneath the badge).
    screenshotSelector: "#select-deployment, [cmdk-root]",
  },
  decorators: [
    (storyFn) => {
      mocked(useDeployments).mockReturnValue({
        deployments: [
          {
            id: 11,
            name: "happy-capybara-123",
            deploymentType: "dev" as const,
            kind: "cloud" as const,
            isDefault: true,
            projectId: mockProject.id,
            creator: 1,
            createTime: NOW,
            class: "s256",
            deploymentUrl: "https://happy-capybara-123.convex.cloud",
            reference: "dev/nicolas",
            region: "aws-us-east-1",
          },
          {
            id: 12,
            name: "musical-otter-456",
            deploymentType: "prod" as const,
            kind: "cloud" as const,
            isDefault: true,
            projectId: mockProject.id,
            creator: 1,
            createTime: NOW,
            class: "s256",
            deploymentUrl: "https://musical-otter-456.convex.cloud",
            reference: "production",
            region: "aws-us-east-1",
          },
          // Ari's feature branch deployments
          {
            id: 21,
            name: "quick-panda-202",
            deploymentType: "dev" as const,
            kind: "cloud" as const,
            isDefault: false,
            projectId: mockProject.id,
            creator: 2,
            createTime: NOW - 72000000,
            class: "s256",
            deploymentUrl: "https://quick-panda-202.convex.cloud",
            reference: "dev/ari/auth-flow",
            region: "aws-us-east-1",
          },
        ] satisfies PlatformDeploymentResponse[],
        isLoading: false,
      });
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    // The palette renders in a portal outside the canvas; wait until its
    // deployment list is populated so the screenshot captures the open menu.
    const body = within(canvasElement.ownerDocument.body);
    await openAnchoredMenu(
      () => within(canvasElement).getByTestId("select-deployment"),
      () => body.getByText("dev/ari/auth-flow"),
    );
  },
};

/**
 * The deployment switcher, on a project with just a production and a personal
 * development deployment.
 */
export const DeploymentSwitcher: Story = {
  parameters: {
    ...meta.parameters,
    screenshotSelector: "#select-deployment, .command-palette--anchored",
    // The menu's list is capped at min(330px, 40vh): at the default 700px-tall
    // viewport the 40vh half of that clips the last deployment.
    screenshotViewport: { width: 1024, height: 1000 },
  },
  play: async ({ canvasElement }) => {
    await openAnchoredMenu(
      () => within(canvasElement).getByTestId("select-deployment"),
      () => screen.getByText("Deployments"),
    );
  },
};

/**
 * The deployment switcher on a project that also has a preview deployment, as
 * created by a PR build.
 */
export const PreviewDeploymentSwitcher: Story = {
  parameters: {
    ...meta.parameters,
    screenshotSelector: "#select-deployment, .command-palette--anchored",
    screenshotViewport: { width: 1024, height: 1000 },
  },
  decorators: [
    (storyFn) => {
      mocked(useDeployments).mockReturnValue({
        deployments: [
          {
            id: 11,
            name: "happy-capybara-123",
            deploymentType: "dev",
            kind: "cloud",
            isDefault: true,
            projectId: mockProject.id,
            creator: 1,
            createTime: NOW,
            class: "s256",
            deploymentUrl: "https://happy-capybara-123.convex.cloud",
            reference: "dev/nicolas",
            region: "aws-us-east-1",
          },
          {
            id: 12,
            name: "musical-otter-456",
            deploymentType: "prod",
            kind: "cloud",
            isDefault: true,
            projectId: mockProject.id,
            creator: 1,
            createTime: NOW,
            class: "s256",
            deploymentUrl: "https://musical-otter-456.convex.cloud",
            reference: "production",
            region: "aws-us-east-1",
          },
          {
            id: 13,
            name: "fearless-gerbil-789",
            deploymentType: "preview",
            kind: "cloud",
            isDefault: false,
            projectId: mockProject.id,
            creator: 1,
            createTime: NOW - 6 * 60 * 1000,
            class: "s256",
            deploymentUrl: "https://fearless-gerbil-789.convex.cloud",
            reference: "preview/my-cool-feature",
            previewIdentifier: "my-cool-feature",
            region: "aws-us-east-1",
          },
        ] satisfies PlatformDeploymentResponse[],
        isLoading: false,
      });
      return storyFn();
    },
  ],
  play: async ({ canvasElement }) => {
    await openAnchoredMenu(
      () => within(canvasElement).getByTestId("select-deployment"),
      () => screen.getByText("Deployments"),
    );
  },
};

const otherProjects = [
  mockProject,
  {
    id: 8,
    teamId: mockTeam.id,
    name: "Marketing site",
    slug: "marketing-site",
  },
  {
    id: 9,
    teamId: mockTeam.id,
    name: "Internal tools",
    slug: "internal-tools",
  },
] as ReturnType<typeof useInfiniteProjects>["projects"];

/**
 * The project switcher the header's project name opens.
 */
export const ProjectSwitcher: Story = {
  parameters: {
    ...meta.parameters,
    screenshotSelector:
      '[aria-label="Switch project"], .command-palette--anchored',
    screenshotViewport: { width: 1024, height: 1000 },
  },
  decorators: [
    (storyFn) => {
      // `useInfiniteProjects` is server-backed: its rows bypass the palette's
      // client-side filter, so the mock filters by the search argument itself.
      mocked(useInfiniteProjects).mockImplementation(
        (_teamId, searchQuery = "") => {
          const q = searchQuery.trim().toLowerCase();
          return {
            projects: otherProjects.filter(
              (p) => !q || `${p.name} ${p.slug}`.toLowerCase().includes(q),
            ),
            isLoading: false,
            isLoadingMore: false,
            hasMore: false,
            loadMore: () => {},
            debouncedQuery: searchQuery,
            pageSize: 20,
          };
        },
      );
      return storyFn();
    },
  ],
  play: async () => {
    // The header is rendered by the docs decorator and the palette portals to
    // document.body, so query the whole screen rather than the story canvas.
    await openAnchoredMenu(
      () => screen.getByLabelText("Switch project"),
      () => screen.getByText("Create Project…"),
    );
  },
};
