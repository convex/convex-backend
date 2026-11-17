import { ReactNode, useLayoutEffect, useState } from "react";
import type { Meta, StoryObj } from "@storybook/nextjs";
import { getRouter } from "@storybook/nextjs/router.mock";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ConvexProvider } from "convex/react";
import { Module } from "system-udfs/convex/_system/frontend/common";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import {
  FunctionsProvider,
  useFunctionSearchTerm,
} from "@common/lib/functions/FunctionsProvider";
import { VirtualizedDirectorySidebar } from "./VirtualizedDirectorySidebar";

type FunctionSpec = {
  name: string;
  udfType?: "Query" | "Mutation" | "Action" | "HttpAction";
  internal?: boolean;
};

function rootModules(files: [string, FunctionSpec[]][]) {
  const modules: [string, Module][] = files.map(([file, functions]) => [
    file,
    {
      functions: functions.map(({ name, udfType = "Query", internal }, i) => ({
        name,
        lineno: i + 1,
        udfType,
        visibility: { kind: internal ? "internal" : "public" },
      })),
      sourcePackageId: "sourcePackage",
    },
  ]);
  return [[null, modules]] as [null, [string, Module][]][];
}

function withDeployment(modules: ReturnType<typeof rootModules>) {
  const client = mockConvexReactClient()
    .registerQueryFake(udfs.components.list, () => [])
    .registerQueryFake(udfs.modules.listForAllComponents, () => modules);
  return () => (
    <ConvexProvider client={client}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <FunctionsProvider>
          <WithoutSearchTerm>
            <VirtualizedDirectorySidebar onChangeFunction={() => {}} />
          </WithoutSearchTerm>
        </FunctionsProvider>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  );
}

// The search term is global state, which would otherwise carry over from the
// previous story.
function WithoutSearchTerm({ children }: { children: ReactNode }) {
  const [searchTerm, setSearchTerm] = useFunctionSearchTerm();
  const [cleared, setCleared] = useState(false);
  useLayoutEffect(() => {
    setSearchTerm("");
    setCleared(true);
  }, [setSearchTerm]);
  return cleared || searchTerm === "" ? children : null;
}

const meta = {
  component: VirtualizedDirectorySidebar,
  args: { onChangeFunction: () => {} },
  decorators: [
    (Story) => (
      <div className="h-screen">
        <Story />
      </div>
    ),
  ],
  parameters: { layout: "fullscreen", a11y: { test: "todo" } },
} satisfies Meta<typeof VirtualizedDirectorySidebar>;

export default meta;
type Story = StoryObj<typeof meta>;

// Covers each kind of row: a file with several functions, internal functions
// with short and long names, a file with a single function, a nested folder,
// and HTTP routes.
export const SmallDeployment: Story = {
  render: withDeployment(
    rootModules([
      [
        "messages.js",
        [
          { name: "list" },
          { name: "send", udfType: "Mutation" },
          { name: "purge", udfType: "Mutation", internal: true },
          { name: "purgeExpiredDrafts", udfType: "Mutation", internal: true },
          {
            name: "reindexAllChannelMessagesAfterSearchMigration",
            udfType: "Action",
            internal: true,
          },
        ],
      ],
      ["users.js", [{ name: "get" }]],
      [
        "admin/reports.js",
        [
          { name: "daily", udfType: "Action" },
          { name: "weekly" },
          { name: "rebuildMonthlyRollup", udfType: "Action", internal: true },
        ],
      ],
      [
        "http.js",
        [
          { name: "GET /api/messages", udfType: "HttpAction" },
          { name: "POST /api/messages", udfType: "HttpAction" },
        ],
      ],
    ]),
  ),
  // The styles in `FunctionTree` target the tree's internal markup, which a
  // new version of `@pierre/trees` could change.
  play: async ({ canvasElement }) => {
    const purge = await waitFor(() => row(canvasElement, "messages/purge"));
    const badge = purge.querySelector<HTMLElement>(
      "[data-item-section='decoration'] > span",
    );
    await expect(badge).toHaveTextContent("Internal");
    const badgeStyle = getComputedStyle(badge!);
    await expect(badgeStyle.fontSize).toBe("10px");
    await expect(badgeStyle.borderRadius).toBe("4px");
    await expect(badgeStyle.backgroundColor).toBe(
      tokenColor(canvasElement, "--color-background-tertiary"),
    );
    await expect(getComputedStyle(badge!, "::before").content).toBe("none");

    await expect(
      purge.querySelector("[data-item-section='icon'] use"),
    ).toHaveAttribute("href", "#convex-function");
    const chevron = row(canvasElement, "messages/").querySelector(
      "[data-item-section='icon'] svg",
    );
    await expect(chevron).toHaveAttribute("width", "15");

    await expect(getComputedStyle(purge).userSelect).toBe("none");
  },
};

// The selection is the open function, whatever keys the tree gets.
export const SingleSelection: Story = {
  ...SmallDeployment,
  // Storybook's router doesn't navigate, so the open function stays this one.
  parameters: { nextjs: { router: { query: { function: "messages:list" } } } },
  play: async ({ canvasElement }) => {
    await userEvent.click(
      await waitFor(() => row(canvasElement, "messages/list")),
    );
    await waitFor(() =>
      expect(selectedRows(canvasElement)).toEqual(["messages/list"]),
    );
    await userEvent.keyboard("{Shift>}{ArrowDown}{ArrowDown}{/Shift}");
    await userEvent.keyboard("{Meta>}a{/Meta}{Control>}a{/Control}");
    await userEvent.keyboard("{Shift>}");
    await userEvent.click(row(canvasElement, "http/"));
    await userEvent.keyboard("{/Shift}");
    await expect(selectedRows(canvasElement)).toEqual(["messages/list"]);

    // A plain click on a directory toggles it and keeps the selection. The
    // click on the heading keeps it from counting as a double click.
    await userEvent.click(within(canvasElement).getByText("Functions"));
    const wasExpanded = row(canvasElement, "http/").getAttribute(
      "aria-expanded",
    );
    await userEvent.click(row(canvasElement, "http/"));
    await waitFor(() =>
      expect(row(canvasElement, "http/")).toHaveAttribute(
        "aria-expanded",
        wasExpanded === "true" ? "false" : "true",
      ),
    );
    await waitFor(() =>
      expect(selectedRows(canvasElement)).toEqual(["messages/list"]),
    );

    // Arrow keys move the focus ring, not the selection.
    await userEvent.click(row(canvasElement, "messages/list"));
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(() =>
      expect(row(canvasElement, "messages/send")).toHaveAttribute(
        "data-item-focused",
      ),
    );
    await expect(selectedRows(canvasElement)).toEqual(["messages/list"]);
  },
};

// While the search input has focus, a cursor starts on the first match, the
// arrow keys move it, and Enter opens its function.
export const SearchKeyboard: Story = {
  ...SmallDeployment,
  play: async ({ canvasElement }) => {
    const input = await within(canvasElement).findByPlaceholderText(
      "Search functions...",
    );
    await waitFor(() => row(canvasElement, "messages/purge"));
    await userEvent.type(input, "purge");
    await waitFor(() =>
      expect(cursorRows(canvasElement)).toEqual(["messages/purge"]),
    );
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(
      () =>
        expect(cursorRows(canvasElement)).toEqual([
          "messages/purgeExpiredDrafts",
        ]),
      { timeout: 3000 },
    );
    await expect(
      getComputedStyle(
        row(canvasElement, "messages/purgeExpiredDrafts"),
        "::before",
      ).outlineStyle,
    ).toBe("solid");
    await userEvent.keyboard("{Enter}");
    await expect(getRouter().push).toHaveBeenLastCalledWith({
      query: expect.objectContaining({
        function: "messages:purgeExpiredDrafts",
      }),
    });
    await expect(input).toHaveFocus();

    await userEvent.tab();
    await expect(cursorRows(canvasElement)).toEqual([]);
  },
};

// Names too long for any panel width: they truncate, and the
// "Internal" badge next to them shortens to "Int.".
export const LongFunctionNames: Story = {
  render: withDeployment(
    rootModules([
      [
        "migrations.js",
        [
          {
            name: "backfillDenormalizedChannelMembershipCountsForWorkspacesCreatedBeforeTheTeamsMigrationAndRecomputeUnreadBadgesForEveryMemberOfThoseChannelsThenNotifyTheirOwnersByEmailOnceTheBackfillHasFinishedAndTheAuditLogEntriesForEachWorkspaceHaveBeenWrittenAndVerified",
            udfType: "Mutation",
            internal: true,
          },
          {
            name: "listWorkspacesWhoseChannelMembershipCountsStillNeedToBeBackfilledAfterTheTeamsMigrationGroupedByRegionAndSortedByCreationTimeIncludingArchivedWorkspacesThatWereRestoredAfterTheMigrationStartedAndWorkspacesWhoseOwnersHaveSinceLeftTheOrganization",
          },
          { name: "status" },
        ],
      ],
    ]),
  ),
  play: async ({ canvasElement }) => {
    const longest = await waitFor(() =>
      row(
        canvasElement,
        "migrations/backfillDenormalizedChannelMembershipCountsForWorkspacesCreatedBeforeTheTeamsMigrationAndRecomputeUnreadBadgesForEveryMemberOfThoseChannelsThenNotifyTheirOwnersByEmailOnceTheBackfillHasFinishedAndTheAuditLogEntriesForEachWorkspaceHaveBeenWrittenAndVerified",
      ),
    );
    const badge = longest.querySelector(
      "[data-item-section='decoration'] > span",
    );
    await waitFor(() =>
      expect(getComputedStyle(badge!, "::before").content).toBe('"Int."'),
    );
    await expect(getComputedStyle(badge!).fontSize).toBe("0px");
  },
};

// Files and folders that share a display name: `messages.js` next to a
// `messages/` folder, `init.js` (whose only function is `init`) next to an
// `init/` folder, and two modules that both display as `seed`. Each function
// must get a row of its own.
export const NameCollisions: Story = {
  render: withDeployment(
    rootModules([
      [
        "messages.js",
        [{ name: "list" }, { name: "send", udfType: "Mutation" }],
      ],
      ["messages/drafts.js", [{ name: "save" }, { name: "discard" }]],
      ["init.js", [{ name: "init", udfType: "Mutation", internal: true }]],
      ["init/steps.js", [{ name: "first" }, { name: "second" }]],
      ["seed.js", [{ name: "users" }, { name: "teams" }]],
      ["seed.dev.js", [{ name: "fixtures" }, { name: "reset" }]],
    ]),
  ),
  play: async ({ canvasElement }) => {
    await waitFor(() => treeShadowRoot(canvasElement));
    await expandAllDirectories(canvasElement);
    await waitFor(() =>
      expect(functionRowLabels(canvasElement).sort()).toEqual(
        [
          "list",
          "send",
          "save",
          "discard",
          "init",
          "first",
          "second",
          "users",
          "teams",
          "fixtures",
          "reset",
        ].sort(),
      ),
    );
  },
};

// The tree renders its rows in a shadow root, out of reach of `within`.
function treeShadowRoot(canvasElement: HTMLElement) {
  const host = [...canvasElement.querySelectorAll("*")].find(
    (element) => element.shadowRoot,
  );
  if (!host?.shadowRoot) {
    throw new Error("The tree hasn't rendered yet");
  }
  return host.shadowRoot;
}

function row(canvasElement: HTMLElement, path: string) {
  const element = treeShadowRoot(canvasElement).querySelector<HTMLElement>(
    `button[data-item-path="${CSS.escape(path)}"]`,
  );
  if (!element) {
    throw new Error(`No row for ${path}`);
  }
  return element;
}

function selectedRows(canvasElement: HTMLElement) {
  return [
    ...treeShadowRoot(canvasElement).querySelectorAll<HTMLElement>(
      "button[data-item-selected]",
    ),
  ].map((element) => element.dataset.itemPath);
}

function cursorRows(canvasElement: HTMLElement) {
  return [
    ...treeShadowRoot(canvasElement).querySelectorAll<HTMLElement>(
      "button[data-search-cursor]",
    ),
  ].map((element) => element.dataset.itemPath);
}

// A design token's color, as the browser computes it.
function tokenColor(canvasElement: HTMLElement, token: string) {
  const probe = document.createElement("div");
  probe.style.backgroundColor = `var(${token})`;
  canvasElement.append(probe);
  const color = getComputedStyle(probe).backgroundColor;
  probe.remove();
  return color;
}

async function expandAllDirectories(canvasElement: HTMLElement) {
  for (;;) {
    const collapsed = treeShadowRoot(canvasElement).querySelector<HTMLElement>(
      "button[data-item-type='folder'][aria-expanded='false']",
    );
    if (!collapsed) {
      return;
    }
    await userEvent.click(collapsed);
  }
}

function functionRowLabels(canvasElement: HTMLElement) {
  return [
    ...treeShadowRoot(canvasElement).querySelectorAll(
      "button[data-item-type='file']",
    ),
  ].map((row) =>
    // Without the invisible suffix that keeps repeated names apart.
    row.getAttribute("aria-label")?.replaceAll("\u2060", ""),
  );
}

// 1,000 root-level files of 5 functions each, all expanded: 6,000 rows, of
// which the tree mounts only those in view.
export const LargeFlatDeployment: Story = {
  render: withDeployment(
    rootModules(
      Array.from({ length: 1000 }, (_, i) => [
        `file${i}.js`,
        Array.from({ length: 5 }, (__, fn) => ({ name: `fn${fn}` })),
      ]),
    ),
  ),
};
