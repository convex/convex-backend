import { Meta, StoryObj } from "@storybook/nextjs";
import { type ContextType } from "react";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import udfs from "@common/udfs";
import { ConvexProvider } from "convex/react";
import { fn, userEvent, waitFor, within } from "storybook/test";
import { IntegrationsView } from "@common/features/settings/components/integrations/IntegrationsView";

const mockTeam = { id: 2, slug: "acme", name: "Acme Corp" };
const mockProject = {
  id: 7,
  teamId: mockTeam.id,
  name: "My amazing app",
  slug: "my-amazing-app",
};
const mockDeployment = {
  id: 12,
  name: "musical-otter-456",
  deploymentType: "prod" as const,
  kind: "cloud",
  isDefault: true,
  projectId: mockProject.id,
  creator: 1,
  createTime: Date.now(),
  class: "s256",
  deploymentUrl: "https://musical-otter-456.eu-west-1.convex.cloud",
  reference: "production",
  region: "aws-eu-west-1",
} as const;

const mockClient = mockConvexReactClient()
  .registerQueryFake(udfs.listConfiguredSinks.default, () => [])
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
    () => "https://musical-otter-456.convex.site",
  )
  .registerQueryFake(udfs.fileStorageV2.numFiles, () => 0)
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

function renderIntegrations(
  deploymentInfoOverrides: Partial<
    ContextType<typeof DeploymentInfoContext>
  > = {},
) {
  return (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider
          value={
            {
              ...mockDeploymentInfo,
              useCurrentTeam: () => mockTeam,
              useCurrentProject: () => mockProject,
              useCurrentDeployment: () => mockDeployment,
              useIsDeploymentPaused: () => false,
              useLogDeploymentEvent: () => fn(),
              useTeamEntitlements: () => ({
                logStreamingEnabled: true,
                streamingExportEnabled: true,
              }),
              deploymentsURI: "/t/acme/my-amazing-app/musical-otter-456",
              projectsURI: "/t/acme/my-amazing-app",
              teamsURI: "/t/acme",
              isSelfHosted: false,
              ...deploymentInfoOverrides,
            } as ContextType<typeof DeploymentInfoContext>
          }
        >
          <IntegrationsView />
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

const meta = {
  component: IntegrationsView,
  parameters: {
    layout: "fullscreen",
    docsPage: { deploymentType: "prod" },
    nextjs: {
      router: {
        pathname: "/t/[team]/[project]/[deploymentName]/settings/integrations",
        route: "/t/[team]/[project]/[deploymentName]/settings/integrations",
        asPath:
          "/t/acme/my-amazing-app/musical-otter-456/settings/integrations",
        query: {
          team: "acme",
          project: "my-amazing-app",
          deploymentName: "musical-otter-456",
        },
      },
    },
    a11y: { test: "todo" },
  },
  render: () => renderIntegrations(),
} satisfies Meta<typeof IntegrationsView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

async function openConfigure(canvasElement: HTMLElement, label: string) {
  const canvas = within(canvasElement);
  await waitFor(() => canvas.getByText(label));
  // The nearest ancestor holding a configure button is this integration's card.
  let panel = canvas.getByText(label).parentElement!;
  while (!within(panel).queryByTestId("configure-integration")) {
    panel = panel.parentElement!;
  }
  await userEvent.click(within(panel).getByTestId("configure-integration"));
  // Log-stream integrations open a slide-over panel and exception-reporting
  // ones open a modal; both render with role="dialog".
  await waitFor(() => {
    if (!document.querySelector('[role="dialog"]')) {
      throw new Error("config dialog not open yet");
    }
  });
}

export const ConfigureSentry: Story = {
  play: async ({ canvasElement }) => {
    await openConfigure(canvasElement, "Sentry");
  },
};

export const ConfigureDatadog: Story = {
  play: async ({ canvasElement }) => {
    await openConfigure(canvasElement, "Datadog");
  },
};

// Each Analytics export destination is behind its own LaunchDarkly flag.
export const ConfigureS3Export: Story = {
  render: () => renderIntegrations({ s3ExportIntegrationEnabled: true }),
  play: async ({ canvasElement }) => {
    await openConfigure(canvasElement, "AWS S3");
  },
};

export const CustomAuditTopic: Story = {
  parameters: {
    screenshotSelector: '[role="dialog"]',
  },
  render: () =>
    renderIntegrations({
      useTeamEntitlements: () => ({
        logStreamingEnabled: true,
        streamingExportEnabled: true,
        customAuditLogsInLogStreamsConfigEnabled: true,
      }),
    }),
  play: async ({ canvasElement }) => {
    // The Webhook config panel has few fields above the topic selector, so the
    // whole selector (including custom_audit) fits in the screenshot viewport.
    await openConfigure(canvasElement, "Webhook");
    const dialog = within(document.querySelector('[role="dialog"]')!);
    // Fill the URL so toggling a topic doesn't surface a "URL required" error.
    await userEvent.type(
      dialog.getByPlaceholderText("Enter a URL to send logs to"),
      "https://example.com/logs",
    );
    // Each checkbox shares the aria-label "Selected", so target the topic by its
    // label text; clicking it toggles the associated checkbox.
    await userEvent.click(dialog.getByText("custom_audit"));
  },
};
