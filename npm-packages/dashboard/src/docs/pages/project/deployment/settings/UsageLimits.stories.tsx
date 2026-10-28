import { Meta, StoryObj } from "@storybook/nextjs";
import { SWRConfig } from "swr";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { fn } from "storybook/test";
import { UsageLimitsView } from "@common/features/settings/components/UsageLimitsView";
import { UsageLimit } from "@common/features/settings/components/UsageLimits";
import { EXAMPLE_USAGE_LIMITS } from "@common/features/settings/components/usageLimitsFixtures";

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

// A prod deployment, so the screenshot shows both threshold columns (dev
// deployments hide the warning threshold).
const mockDeployment = {
  id: 11,
  name: "happy-capybara-123",
  deploymentType: "prod" as const,
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: 0,
  class: "s256",
  deploymentUrl: "https://happy-capybara-123.convex.cloud",
  reference: "production",
  region: "aws-us-east-1",
} as const;

function renderPage() {
  // The settings sidebar queries the component list (via useNents) through
  // the nearest ConvexProvider, so that one fake is load-bearing.
  const client = mockConvexReactClient().registerQueryFake(
    udfs.components.list,
    () => [],
  );
  const connectedDeployment = {
    deployment: {
      client,
      httpClient: {} as never,
      deploymentUrl: mockDeployment.deploymentUrl,
      adminKey: "storybook-admin-key",
      deploymentName: mockDeployment.name,
    },
    isDisconnected: false,
  };
  return (
    // Fresh SWR cache per mount: the usage-limits fetch is keyed on the
    // deployment URL, which is identical across stories, and the default
    // module-global cache would serve one story's limits to the next.
    <SWRConfig value={{ provider: () => new Map() }}>
      <ConnectedDeploymentContext.Provider value={connectedDeployment}>
        <ConvexProvider client={client}>
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
            <UsageLimitsView />
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      </ConnectedDeploymentContext.Provider>
    </SWRConfig>
  );
}

const meta = {
  component: UsageLimitsView,
  parameters: {
    layout: "fullscreen",
    // The threshold table (metric + current usage + Warning + Disable columns)
    // needs its full ~1040px content column to render without the inner
    // overflow-x-auto clipping it, which the default 1024px capture viewport is
    // too narrow to give. Widen the viewport so the whole table fits; the
    // height fits every metric row without an inner scroll.
    screenshotViewport: { width: 1512, height: 1220 },
    // Match the decorator's header to the prod deployment the page renders.
    docsPage: {
      deploymentType: "prod",
    },
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/settings/usage-limits",
        route: "/t/[team]/[project]/[deploymentName]/settings/usage-limits",
        asPath:
          "/t/acme/my-amazing-app/happy-capybara-123/settings/usage-limits",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "happy-capybara-123",
        },
      },
    },
    a11y: { test: "todo" },
  },
  // The view loads limits over HTTP from the deployment API (there is no udf
  // to fake), so stub fetch for the deployment's URL and answer
  // /list_usage_limits with the story's limits. Everything else passes
  // through to the real fetch.
  beforeEach: (context) => {
    const limits =
      (context.parameters as { usageLimits?: UsageLimit[] }).usageLimits ?? [];
    const originalFetch = window.fetch;
    window.fetch = async (input, init) => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      if (url.startsWith(mockDeployment.deploymentUrl)) {
        const body = url.endsWith("/list_usage_limits")
          ? { usageLimits: limits }
          : {};
        return new Response(JSON.stringify(body), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return originalFetch(input, init);
    };

    return () => {
      window.fetch = originalFetch;
    };
  },
  render: () => renderPage(),
} satisfies Meta<typeof UsageLimitsView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  parameters: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
  },
};

export const Empty: Story = {
  parameters: {
    usageLimits: [],
  },
};
