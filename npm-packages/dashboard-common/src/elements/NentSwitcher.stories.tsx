import { Decorator, Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { expect, fn, screen, userEvent, within } from "storybook/test";
import { Id } from "system-udfs/convex/_generated/dataModel";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { NentSwitcher } from "@common/elements/NentSwitcher";

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

// A nested component tree: `resend` installs two workpools, each of which
// installs a worker, so paths span three levels of nesting.
const components = [
  component("resend"),
  component("resend/rateLimiter"),
  component("resend/emailWorkpool"),
  component("resend/emailWorkpool/batchWorker"),
  component("resend/callbackWorkpool"),
  component("resend/callbackWorkpool/batchWorker"),
];

const unmounted = component("legacyCrons", "unmounted");

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

/** Open the dropdown and return its options, in display order. */
async function openDropdown(canvasElement: HTMLElement) {
  const canvas = within(canvasElement);
  await userEvent.click(
    await canvas.findByRole("button", { name: /Select component/i }),
  );
  // The options render in a portal, outside the story canvas.
  await screen.findByPlaceholderText("Search components...");
  return screen.getAllByRole("option").map((o) => o.textContent);
}

const meta = {
  component: NentSwitcher,
  args: { className: "w-64", onChange: fn() },
  parameters: {
    ...routerParams(),
    a11y: { test: "todo" },
  },
  decorators: [withComponents(components)],
} satisfies Meta<typeof NentSwitcher>;

export default meta;
type Story = StoryObj<typeof meta>;

/** No component selected: the button falls back to the app. */
export const AppSelected: Story = {};

/** The option list, with `_App` relabeled to "app" and paths for the rest. */
export const Open: Story = {
  play: async ({ canvasElement }) => {
    await expect(await openDropdown(canvasElement)).toEqual([
      "app",
      ...components.map((c) => c.path),
    ]);
  },
};

/**
 * A selected component gets a yellow button background, so it's clear the page
 * is scoped to something other than the app.
 */
export const ComponentSelected: Story = {
  parameters: routerParams(components[3]),
};

/**
 * Unmounted components sort after the mounted ones and are marked with a `*`.
 */
export const OpenWithUnmountedComponent: Story = {
  decorators: [withComponents([unmounted, ...components])],
  play: async ({ canvasElement }) => {
    await expect(await openDropdown(canvasElement)).toEqual([
      "app",
      ...components.map((c) => c.path),
      `${unmounted.path}*`,
    ]);
  },
};

/**
 * Selecting an unmounted component adds a note pointing at the Components
 * settings page, where it can be deleted.
 */
export const UnmountedComponentSelected: Story = {
  parameters: routerParams(unmounted),
  decorators: [withComponents([...components, unmounted])],
};

/**
 * A deployment with no installed components renders nothing: there would be
 * nothing to switch to.
 */
export const NoComponents: Story = {
  decorators: [withComponents([])],
};
