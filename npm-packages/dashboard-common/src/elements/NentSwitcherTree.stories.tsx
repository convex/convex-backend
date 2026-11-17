import { Decorator, Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { expect, fn, screen, userEvent, within } from "storybook/test";
import { Id } from "system-udfs/convex/_generated/dataModel";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { NentSwitcherTree } from "@common/elements/NentSwitcherTree";

type Component = {
  id: Id<"_components">;
  name: string | null;
  path: string;
  args: Record<string, never>;
  state: "active" | "unmounted";
  httpPrefix: string | null;
};

function component(
  path: string,
  state: "active" | "unmounted" = "active",
): Component {
  return {
    id: `component_${path.replace(/\//g, "_")}` as Id<"_components">,
    name: path.split("/").pop()!,
    path,
    args: {},
    state,
    httpPrefix: null,
  };
}

// `resend` installs two workpools, each of which installs a worker, so paths
// span three levels of nesting. `aggregate` has no children, and the unmounted
// `legacyCrons` still has an unmounted child of its own.
const components = [
  component("resend"),
  component("resend/rateLimiter"),
  component("resend/emailWorkpool"),
  component("resend/emailWorkpool/batchWorker"),
  component("resend/callbackWorkpool"),
  component("resend/callbackWorkpool/batchWorker"),
  component("aggregate"),
  component("legacyCrons", "unmounted"),
  component("legacyCrons/workpool", "unmounted"),
];

const deploymentInfo = {
  ...mockDeploymentInfo,
  deploymentsURI: "/t/acme/my-amazing-app/happy-capybara-123",
  projectsURI: "/t/acme/my-amazing-app",
  teamsURI: "/t/acme",
};

function withComponents(list: Component[]): Decorator {
  const client = mockConvexReactClient().registerQueryFake(
    udfs.components.list,
    () => list,
  );
  return (Story) => (
    <ConvexProvider client={client}>
      <DeploymentInfoContext.Provider value={deploymentInfo}>
        <Story />
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  );
}

/**
 * The selected component is read from the `component` query param, so each
 * story picks its selection by setting that param on the mocked router.
 */
function routerParams(selected?: Component) {
  return {
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/data",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
          ...(selected ? { component: selected.id } : {}),
        },
      },
    },
  };
}

/** The button opening the tree, named after the selected component. */
function switcherButton(canvasElement: HTMLElement) {
  return within(canvasElement).findByRole("button", { name: /^Component: / });
}

async function openTree(canvasElement: HTMLElement) {
  await userEvent.click(await switcherButton(canvasElement));
  // The tree renders in a portal, outside the story canvas.
  return screen.findByRole("tree");
}

/** The visible tree items, indented by level. */
function treeItems(tree: HTMLElement) {
  return within(tree)
    .getAllByRole("treeitem")
    .map(
      (item) =>
        "  ".repeat(Number(item.getAttribute("aria-level")) - 1) +
        item.textContent,
    );
}

function treeItem(name: string) {
  return screen.getByRole("treeitem", { name: new RegExp(`^${name}`) });
}

function toggleOf(name: string) {
  return treeItem(name).querySelector<HTMLElement>("[data-tree-toggle]")!;
}

function labelOf(name: string) {
  return treeItem(name).querySelector<HTMLElement>("[data-tree-label]")!;
}

function searchInput() {
  return screen.getByRole("combobox", { name: "Search components" });
}

/** The row the arrow keys have moved to, while focus stays in the search. */
function activeItem() {
  const id = searchInput().getAttribute("aria-activedescendant");
  return id === null ? null : document.getElementById(id);
}

const meta = {
  component: NentSwitcherTree,
  args: { className: "w-64", onChange: fn() },
  parameters: {
    ...routerParams(),
    // Headless UI brackets the portaled panel with `aria-hidden` sentinel
    // buttons that hand focus back across the portal boundary, which axe reads
    // as hidden controls. The rule still checks every other element.
    a11y: {
      config: {
        rules: [
          {
            id: "aria-hidden-focus",
            selector: '[aria-hidden="true"]:not([data-headlessui-focus-guard])',
          },
        ],
      },
    },
  },
  decorators: [
    withComponents(components),
    // Leaves room under the button for the open tree in screenshots.
    (Story) => (
      <div className="h-96">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof NentSwitcherTree>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * Clicking a component's name opens it; the toggle before it, which has its own
 * hover target, expands it. The indentation does neither.
 */
export const ExpandAndOpen: Story = {
  play: async ({ canvasElement, args }) => {
    const tree = await openTree(canvasElement);
    await expect(treeItems(tree)).toEqual([
      "app",
      "aggregate",
      "resend",
      "legacyCrons* (unmounted)",
    ]);

    await userEvent.click(toggleOf("resend"));
    await expect(args.onChange).not.toHaveBeenCalled();
    await expect(treeItems(tree)).toEqual([
      "app",
      "aggregate",
      "resend",
      "  callbackWorkpool",
      "  emailWorkpool",
      "  rateLimiter",
      "legacyCrons* (unmounted)",
    ]);

    await userEvent.click(treeItem("emailWorkpool"));
    await expect(args.onChange).not.toHaveBeenCalled();
    await userEvent.click(labelOf("emailWorkpool"));
    await expect(args.onChange).toHaveBeenCalledWith(
      "component_resend_emailWorkpool",
    );
  },
};

/** The ancestors of a nested selection start out expanded. */
export const NestedComponentSelected: Story = {
  parameters: routerParams(components[3]),
  play: async ({ canvasElement }) => {
    await expect(await switcherButton(canvasElement)).toHaveAccessibleName(
      "Component: resend/emailWorkpool/batchWorker",
    );
    const tree = await openTree(canvasElement);
    await expect(treeItems(tree)).toEqual([
      "app",
      "aggregate",
      "resend",
      "  callbackWorkpool",
      "  emailWorkpool",
      "    batchWorker",
      "  rateLimiter",
      "legacyCrons* (unmounted)",
    ]);
    await expect(treeItem("batchWorker")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  },
};

/**
 * A search keeps the matching components and the ancestors that place them,
 * expanded. The ancestors that don't match are dimmed. Since the search
 * decides what's expanded, the toggles and rails don't respond until it's
 * cleared, which restores the tree as it was.
 */
export const Search: Story = {
  play: async ({ canvasElement }) => {
    const tree = await openTree(canvasElement);
    await userEvent.click(toggleOf("resend"));
    const beforeSearch = treeItems(tree);
    await userEvent.type(searchInput(), "batch");
    await expect(treeItems(tree)).toEqual([
      "resend",
      "  callbackWorkpool",
      "    batchWorker",
      "  emailWorkpool",
      "    batchWorker",
    ]);
    await expect(
      tree.querySelectorAll("[data-tree-toggle], [data-tree-rail]"),
    ).toHaveLength(0);
    await userEvent.clear(searchInput());
    await expect(treeItems(tree)).toEqual(beforeSearch);
  },
};

/** A search that matches nothing says so. */
export const NoSearchResults: Story = {
  play: async ({ canvasElement }) => {
    const tree = await openTree(canvasElement);
    await userEvent.type(searchInput(), "zzz");
    await expect(within(tree).queryAllByRole("treeitem")).toHaveLength(0);
    await expect(screen.getByRole("status")).toHaveTextContent(
      "No components matching “zzz”.",
    );
    // With nothing to open, Enter does nothing.
    await userEvent.keyboard("{Enter}");
    await expect(screen.getByRole("tree")).toBeInTheDocument();
  },
};

/**
 * A deployment with no installed components renders nothing: there would be
 * nothing to switch to.
 */
export const NoComponents: Story = {
  decorators: [withComponents([])],
  play: async ({ canvasElement }) => {
    await expect(
      within(canvasElement).queryByRole("button", { name: /^Component: / }),
    ).toBeNull();
  },
};

/**
 * With only top-level components, there's nothing to expand: no toggles,
 * rails, or expand-all button. Without the toggle column, the rows start with
 * the names, like the options of `Combobox`.
 */
export const TopLevelOnly: Story = {
  decorators: [
    withComponents([
      component("resend"),
      component("aggregate"),
      component("rateLimiter"),
      component("workpool"),
      component("legacyCrons", "unmounted"),
    ]),
  ],
  play: async ({ canvasElement }) => {
    const tree = await openTree(canvasElement);
    await expect(treeItems(tree)).toEqual([
      "app",
      "aggregate",
      "rateLimiter",
      "resend",
      "workpool",
      "legacyCrons* (unmounted)",
    ]);
    await expect(
      tree.querySelectorAll("[data-tree-toggle], [data-tree-rail]"),
    ).toHaveLength(0);
    await expect(
      screen.queryByRole("button", { name: "Expand all" }),
    ).toBeNull();
  },
};

/**
 * Focus stays in the search input, where the arrow keys move through the tree
 * and expand it, and Enter opens the active row. A query of only spaces isn't
 * a search, so the keys still drive the tree.
 */
export const KeyboardNavigation: Story = {
  play: async ({ canvasElement, args }) => {
    await openTree(canvasElement);
    await expect(searchInput()).toHaveFocus();
    // The selected component starts out active.
    await expect(activeItem()).toBe(treeItem("app"));
    await userEvent.keyboard(" {ArrowDown}{ArrowDown}{ArrowRight}");
    await expect(treeItem("resend")).toHaveAttribute("aria-expanded", "true");
    await userEvent.keyboard("{ArrowRight}");
    await expect(activeItem()).toBe(treeItem("callbackWorkpool"));
    await userEvent.keyboard("{ArrowLeft}");
    await expect(activeItem()).toBe(treeItem("resend"));
    await userEvent.keyboard("{ArrowLeft}");
    await expect(treeItem("resend")).toHaveAttribute("aria-expanded", "false");
    await userEvent.keyboard("{End}");
    await expect(activeItem()).toBe(treeItem("legacyCrons"));
    await userEvent.keyboard("{Home}{ArrowDown}{ArrowDown}{Enter}");
    await expect(args.onChange).toHaveBeenCalledWith("component_resend");
  },
};

/**
 * While searching, the first match becomes active and the arrow keys move
 * through the results.
 */
export const KeyboardNavigationWhileSearching: Story = {
  play: async ({ canvasElement, args }) => {
    await openTree(canvasElement);
    await userEvent.keyboard("batch");
    await expect(activeItem()).toHaveTextContent(/^batchWorker$/);
    await expect(activeItem()).toHaveAttribute("aria-level", "3");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    await expect(args.onChange).toHaveBeenCalledWith(
      "component_resend_emailWorkpool_batchWorker",
    );
  },
};

/**
 * Selecting an unmounted component adds a note pointing at the Components
 * settings page, where it can be deleted.
 */
export const UnmountedComponentSelected: Story = {
  play: async ({ canvasElement }) => {
    await expect(await switcherButton(canvasElement)).toHaveAccessibleName(
      "Component: legacyCrons/workpool (unmounted)",
    );
  },
  parameters: {
    ...routerParams(components[8]),
    // The note's `text-content-secondary` is 4.43:1 against the page
    // background in the light theme, a hair under AA wherever the token sits.
    // The note matches the list switcher's, so the token is the place to fix
    // it.
    a11y: { config: { rules: [{ id: "color-contrast", enabled: false }] } },
  },
};

// Seven levels of nesting, with names long enough to need truncating once
// indented.
const deeplyNested = [
  "agents",
  "agents/threads",
  "agents/threads/messages",
  "agents/threads/messages/vectorSearch",
  "agents/threads/messages/vectorSearch/embeddingsCache",
  "agents/threads/messages/vectorSearch/embeddingsCache/workpool",
  "agents/threads/messages/vectorSearch/embeddingsCache/workpool/retryingBatchWorkerWithAVeryLongName",
].map((path) => component(path));

/**
 * The indentation keeps growing with depth, and names that no longer fit are
 * truncated. The deepest component is selected, so its ancestors start out
 * expanded.
 */
export const DeeplyNested: Story = {
  decorators: [withComponents([...deeplyNested, component("aggregate")])],
  parameters: routerParams(deeplyNested[deeplyNested.length - 1]),
  play: async ({ canvasElement }) => {
    const tree = await openTree(canvasElement);
    await expect(treeItems(tree)).toEqual([
      "app",
      "agents",
      "  threads",
      "    messages",
      "      vectorSearch",
      "        embeddingsCache",
      "          workpool",
      "            retryingBatchWorkerWithAVeryLongName",
      "aggregate",
    ]);
  },
};

/**
 * Every kind of row at once: siblings before and after an expanded component,
 * a collapsed one, leaves, an unmounted component, and seven levels of
 * nesting. The rail holding the active row is darker: hover `batchWorker`,
 * then `aggregate`. Hovering a rail darkens it further.
 */
export const Overview: Story = {
  decorators: [
    withComponents([...components, ...deeplyNested]),
    (Story) => (
      <div className="h-136">
        <Story />
      </div>
    ),
  ],
  parameters: routerParams(deeplyNested[4]),
  play: async ({ canvasElement }) => {
    await openTree(canvasElement);
    await userEvent.click(toggleOf("resend"));
    await userEvent.click(toggleOf("emailWorkpool"));
  },
};

/**
 * Hovering a row that's cut off at the edge of the list leaves the list where
 * it is; only the keyboard scrolls to the active row.
 */
export const HoverDoesNotScroll: Story = {
  ...Overview,
  play: async (context) => {
    await Overview.play!(context);
    const tree = screen.getByRole("tree");
    const before = tree.scrollTop;
    const bottom = tree.getBoundingClientRect().bottom;
    const cutOff = within(tree)
      .getAllByRole("treeitem")
      .find((item) => {
        const rect = item.getBoundingClientRect();
        return rect.top < bottom && rect.bottom > bottom;
      })!;
    await userEvent.hover(
      cutOff.querySelector<HTMLElement>("[data-tree-label]")!,
    );
    await expect(searchInput().getAttribute("aria-activedescendant")).toBe(
      cutOff.id,
    );
    await expect(tree.scrollTop).toBe(before);
  },
};

/**
 * On narrow screens, the tree opens as a sheet along the bottom of the screen,
 * like the list switcher's, with rows tall enough to tap. The search doesn't
 * take focus, which would open the on-screen keyboard over the sheet.
 */
export const Mobile: Story = {
  ...Overview,
  globals: { viewport: { value: "mobile2" } },
  play: async (context) => {
    await Overview.play!(context);
    await expect(searchInput()).not.toHaveFocus();
    await expect(treeItem("resend").getBoundingClientRect().height).toBe(40);
    const tree = screen.getByRole("tree");
    // The sheet's bottom edge is the screen's.
    await expect(
      tree
        .closest<HTMLElement>("[id^=headlessui-popover-panel]")!
        .getBoundingClientRect().bottom,
    ).toBe(window.innerHeight);
  },
};

/**
 * Tapping the backdrop around the sheet closes it, and the backdrop goes with
 * it.
 */
export const MobileBackdropCloses: Story = {
  globals: { viewport: { value: "mobile2" } },
  play: async ({ canvasElement }) => {
    const button = await switcherButton(canvasElement);
    // `userEvent.click` dispatches to its target even when something covers
    // it, so check what's actually under the pointer.
    const isUncovered = () => {
      const { left, top, width, height } = button.getBoundingClientRect();
      const hit = document.elementFromPoint(left + width / 2, top + height / 2);
      return hit !== null && button.contains(hit);
    };
    await expect(isUncovered()).toBe(true);

    await openTree(canvasElement);
    await expect(isUncovered()).toBe(false);
    // Above the sheet, so on the backdrop.
    await userEvent.click(document.elementFromPoint(10, 10)!);
    await expect(screen.queryByRole("tree")).toBeNull();
    await expect(isUncovered()).toBe(true);
  },
};

/**
 * Checks the search bar's size, that its input fills it (so a click anywhere,
 * the icon included, focuses the search), and that it lines up with the
 * top-level rows: the icon over the toggles, the text with the names.
 */
async function expectSearchBar({
  height,
  fontSize,
}: {
  height: number;
  fontSize: string;
}) {
  const input = searchInput();
  const bar = input.parentElement!;
  const inputRect = input.getBoundingClientRect();
  await expect(inputRect.height).toBe(height);
  await expect(getComputedStyle(input).fontSize).toBe(fontSize);
  await expect(inputRect.width).toBe(bar.getBoundingClientRect().width);

  const icon = bar.querySelector("svg")!.getBoundingClientRect();
  const iconCenter = icon.left + icon.width / 2;
  const toggle = toggleOf("resend").getBoundingClientRect();
  await expect(iconCenter).toBeCloseTo(toggle.left + toggle.width / 2, 0);
  const name = labelOf("resend").firstElementChild!.getBoundingClientRect();
  await expect(
    inputRect.left + parseFloat(getComputedStyle(input).paddingLeft),
  ).toBeCloseTo(name.left, 0);

  await expect(
    document.elementFromPoint(iconCenter, icon.top + icon.height / 2),
  ).toBe(input);
}

/**
 * The search bar is 38px tall with 14px text, lined up with the
 * rows below it.
 */
export const SearchBar: Story = {
  play: async ({ canvasElement }) => {
    await openTree(canvasElement);
    await expectSearchBar({ height: 38, fontSize: "14px" });
  },
};

/**
 * On narrow screens, the search bar is 44px tall with 16px text, the smallest
 * that iOS doesn't zoom into when the input is focused.
 */
export const SearchBarMobile: Story = {
  globals: { viewport: { value: "mobile2" } },
  play: async ({ canvasElement }) => {
    await openTree(canvasElement);
    await expectSearchBar({ height: 44, fontSize: "16px" });
  },
};

/** Clicking a rail collapses the component it comes down from. */
export const RailCollapses: Story = {
  play: async ({ canvasElement, args }) => {
    await openTree(canvasElement);
    await userEvent.click(toggleOf("resend"));
    await expect(treeItem("resend")).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(
      treeItem("resend").querySelector<HTMLElement>("[data-tree-rail]")!,
    );
    await expect(treeItem("resend")).toHaveAttribute("aria-expanded", "false");
    await expect(args.onChange).not.toHaveBeenCalled();
  },
};

/**
 * Names too long for the tree or the button are cut off with an ellipsis,
 * whether they're top-level or nested.
 */
export const LongNames: Story = {
  decorators: [
    withComponents([
      component("transactionalEmailsWithRetriesAndRateLimiting"),
      component(
        "transactionalEmailsWithRetriesAndRateLimiting/backgroundJobWorkpoolForOutgoingMessages",
      ),
      component("aggregate"),
    ]),
  ],
  parameters: routerParams(
    component(
      "transactionalEmailsWithRetriesAndRateLimiting/backgroundJobWorkpoolForOutgoingMessages",
    ),
  ),
  play: async ({ canvasElement }) => {
    await openTree(canvasElement);
    const label = labelOf("backgroundJobWorkpoolForOutgoingMessages");
    const text = label.firstElementChild!;
    await expect(text.scrollWidth).toBeGreaterThan(text.clientWidth);
  },
};

function expandAllButton() {
  return screen.getByRole("button", { name: /^(Expand|Collapse) all$/ });
}

/**
 * The button at the end of the search bar expands every component, nested ones
 * included, then collapses them all. While searching, every match is already
 * expanded, so it's disabled.
 */
export const ExpandAll: Story = {
  play: async ({ canvasElement, args }) => {
    const tree = await openTree(canvasElement);
    await userEvent.click(expandAllButton());
    await expect(treeItems(tree)).toEqual([
      "app",
      "aggregate",
      "resend",
      "  callbackWorkpool",
      "    batchWorker",
      "  emailWorkpool",
      "    batchWorker",
      "  rateLimiter",
      "legacyCrons* (unmounted)",
      "  workpool* (unmounted)",
    ]);
    await expect(searchInput()).toHaveFocus();
    await expect(args.onChange).not.toHaveBeenCalled();

    await userEvent.click(expandAllButton());
    await expect(expandAllButton()).toHaveAccessibleName("Expand all");
    await expect(treeItems(tree)).toEqual([
      "app",
      "aggregate",
      "resend",
      "legacyCrons* (unmounted)",
    ]);

    await userEvent.type(searchInput(), "batch");
    await expect(expandAllButton()).toBeDisabled();
  },
};

/**
 * With some components expanded, the button still expands the rest; it
 * collapses only once everything is expanded.
 */
export const ExpandAllWhenPartlyExpanded: Story = {
  play: async ({ canvasElement }) => {
    await openTree(canvasElement);
    await userEvent.click(toggleOf("resend"));
    await expect(expandAllButton()).toHaveAccessibleName("Expand all");
    await userEvent.click(expandAllButton());
    await expect(expandAllButton()).toHaveAccessibleName("Collapse all");
    await expect(treeItem("legacyCrons")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  },
};
