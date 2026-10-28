import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { Sheet } from "@ui/Sheet";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { ModuleFunction } from "@common/lib/functions/types";
import { FunctionsProvider } from "@common/lib/functions/FunctionsProvider";
import { FunctionSummary } from "./FunctionSummary";

const SITE_URL = "https://happy-otter-123.convex.site";

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.getVersion.default, () => "1.44.0")
  .registerQueryFake(udfs.convexSiteUrl.default, () => SITE_URL)
  .registerQueryFake(udfs.components.list, () => [
    component({ id: "root", name: null, path: "", httpPrefix: null }),
    component({
      id: "rateLimiter",
      name: "rateLimiter",
      path: "rateLimiter",
      httpPrefix: "/rate_limiter/",
    }),
    component({
      id: "emailSender",
      name: "emailSender",
      path: "emailSender",
      httpPrefix: null,
    }),
  ])
  // The "Run Function" button wires up the function runner, which reads the
  // deployment's modules and tables.
  .registerQueryFake(udfs.modules.listForAllComponents, () => [])
  .registerQueryFake(udfs.getTableMapping.default, () => ({}));

function component({
  id,
  name,
  path,
  httpPrefix,
}: {
  id: string;
  name: string | null;
  path: string;
  httpPrefix: string | null;
}) {
  return {
    id: id as Id<"_components">,
    name,
    path,
    args: {},
    state: "active" as const,
    httpPrefix,
  };
}

function moduleFunction(
  overrides: Partial<ModuleFunction> & Pick<ModuleFunction, "name" | "udfType">,
): ModuleFunction {
  return {
    displayName: overrides.name,
    type: "function",
    identifier: overrides.name,
    visibility: { kind: "public" },
    componentId: null,
    componentPath: null,
    file: { name: "messages.ts", identifier: "messages.js" },
    ...overrides,
  };
}

const meta = {
  component: FunctionSummary,
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <FunctionsProvider>
          <FunctionSummary {...args} />
        </FunctionsProvider>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
  // The functions page renders this header on `bg-background-secondary`, which
  // is what `Sheet` provides.
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof FunctionSummary>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Query: Story = {
  args: {
    currentOpenFunction: moduleFunction({
      name: "messages:list",
      udfType: "Query",
    }),
  },
};

export const InternalMutation: Story = {
  args: {
    currentOpenFunction: moduleFunction({
      name: "messages:purge",
      udfType: "Mutation",
      visibility: { kind: "internal" },
    }),
  },
};

// HTTP actions are named `"<METHOD> <path>"`, and get a button copying the URL
// they're served at instead of a "Run Function" button.
export const HttpAction: Story = {
  args: {
    currentOpenFunction: moduleFunction({
      name: "POST /webhooks/stripe",
      udfType: "HttpAction",
      file: { name: "http.ts", identifier: "http.js" },
    }),
  },
};

export const HttpActionInComponent: Story = {
  args: {
    currentOpenFunction: moduleFunction({
      name: "GET /reset",
      udfType: "HttpAction",
      componentId: "rateLimiter" as Id<"_components">,
      componentPath: "rateLimiter",
      file: { name: "http.ts", identifier: "http.js" },
    }),
  },
};

// `emailSender` is installed without an `httpPrefix`, so none of its routes are
// served: the badge replaces the copy-URL button.
export const HttpActionInUnmountedComponent: Story = {
  args: {
    currentOpenFunction: moduleFunction({
      name: "POST /send",
      udfType: "HttpAction",
      componentId: "emailSender" as Id<"_components">,
      componentPath: "emailSender",
      file: { name: "http.ts", identifier: "http.js" },
    }),
  },
};
