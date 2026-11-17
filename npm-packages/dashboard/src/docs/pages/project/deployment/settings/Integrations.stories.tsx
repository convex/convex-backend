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
import { SWRConfig, unstable_serialize } from "swr";
import { fn, screen, userEvent, waitFor, within } from "storybook/test";
import { IntegrationsView } from "@common/features/settings/components/integrations/IntegrationsView";
import { AnalyticsIntegration } from "@common/lib/integrationHelpers";

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

type S3Export = NonNullable<
  Extract<AnalyticsIntegration, { kind: "s3Export" }>["existing"]
>;

// Captured screenshots must not change between runs, so S3 export timestamps
// are relative to this instant and the S3 stories freeze `Date.now()` to it.
const NOW = new Date("2026-11-04T17:00:00Z").getTime();

const mockClient = (sinks: S3Export[] = []) =>
  mockConvexReactClient()
    .registerQueryFake(udfs.listConfiguredSinks.default, () => sinks)
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

const adminKey = "storybook-admin-key";

// The S3 export's table picker lists these.
const swrFallback = {
  [unstable_serialize([
    mockDeployment.deploymentUrl,
    "/api/json_schemas?byComponent=true",
    `Convex ${adminKey}`,
  ])]: {
    "": ["customers", "invoices", "orders", "products"],
    aggregate: ["items", "namespaces"],
    workpool: ["config", "pendingWork", "work", "workers"],
  },
};

function renderIntegrations(
  deploymentInfoOverrides: Partial<
    ContextType<typeof DeploymentInfoContext>
  > = {},
  sinks: S3Export[] = [],
) {
  const client = mockClient(sinks);
  return (
    <ConnectedDeploymentContext.Provider
      value={{
        deployment: {
          client,
          httpClient: {} as never,
          deploymentUrl: mockDeployment.deploymentUrl,
          adminKey,
          deploymentName: mockDeployment.name,
        },
        isDisconnected: false,
      }}
    >
      <ConvexProvider client={client}>
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
          <SWRConfig
            value={{
              provider: () => new Map(),
              revalidateOnMount: false,
              isPaused: () => true,
              fallback: swrFallback,
            }}
          >
            <IntegrationsView />
          </SWRConfig>
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

function s3Export(overrides: Partial<S3Export> = {}): S3Export {
  return {
    _id: "s3export" as never,
    _creationTime: NOW - 24 * 60 * 60 * 1000,
    status: { type: "active" },
    exportProgress: undefined,
    config: {
      type: "s3Export",
      bucket: `convex-export-${mockDeployment.name}`,
      region: "eu-west-1",
      // Role exports store an empty access key ID for older binaries.
      accessKeyId: "",
      roleArn:
        "arn:aws:iam::123456789012:role/convex/ConvexIntegrations-Qk9xT3mZa8vLw2Hc5RfG1uYpN7dB4eJs",
      roleExternalId: "Qk9xT3mZa8vLw2Hc5RfG1uYpN7dB4eJs",
      selection: {},
    },
    ...overrides,
  };
}

/** An export saved on the Bucket step, before its role is connected. */
function pendingS3Export(): S3Export {
  const existing = s3Export({ status: { type: "pending" } });
  existing.config.roleArn = "";
  return existing;
}

// Each Analytics export destination is behind its own LaunchDarkly flag.
const renderS3Export = (sinks: S3Export[]) => () =>
  renderIntegrations({ s3ExportIntegrationEnabled: true }, sinks);

const freezeNow = () => {
  const realNow = Date.now;
  Date.now = () => NOW;
  return () => {
    Date.now = realNow;
  };
};

export const ConfigureS3Export: Story = {
  name: "Configure S3 Export",
  render: renderS3Export([]),
  play: async ({ canvasElement }) => {
    await openConfigure(canvasElement, "AWS S3");
  },
};

// A saved export is configured from its card's overflow menu, and a pending
// one opens on the Connect AWS step.
async function configurePendingS3Export(canvasElement: HTMLElement) {
  const canvas = within(canvasElement);
  // The overflow menu's button reuses this label on every integration card.
  await userEvent.click(await canvas.findByLabelText("Open table settings"));
  await userEvent.click(
    await screen.findByRole("menuitem", { name: "Configure" }),
  );
  await waitFor(() => {
    if (!document.querySelector('[role="dialog"]')) {
      throw new Error("config dialog not open yet");
    }
  });
  return within(document.querySelector<HTMLElement>('[role="dialog"]')!);
}

async function openPendingS3Export(canvasElement: HTMLElement) {
  const dialog = await configurePendingS3Export(canvasElement);
  await userEvent.type(
    await dialog.findByLabelText("AWS account ID"),
    "123456789012",
  );
  await dialog.findByText(/ConvexIntegrations-/);
  return dialog;
}

export const ConfigureS3ExportConnectAws: Story = {
  name: "Configure S3 Export Connect AWS",
  beforeEach: freezeNow,
  render: renderS3Export([pendingS3Export()]),
  play: async ({ canvasElement }) => {
    await openPendingS3Export(canvasElement);
  },
};

export const ConfigureS3ExportTerraform: Story = {
  name: "Configure S3 Export Terraform",
  beforeEach: freezeNow,
  render: renderS3Export([pendingS3Export()]),
  play: async ({ canvasElement }) => {
    const dialog = await configurePendingS3Export(canvasElement);
    await userEvent.click(await dialog.findByText("Terraform"));
    await dialog.findByText("Copy Terraform");
  },
};

export const ConfigureS3ExportSyncSettings: Story = {
  name: "Configure S3 Export Sync Settings",
  beforeEach: freezeNow,
  render: renderS3Export([pendingS3Export()]),
  play: async ({ canvasElement }) => {
    const dialog = await openPendingS3Export(canvasElement);
    await userEvent.click(
      dialog.getByRole("button", { name: "Next: sync settings" }),
    );
    await dialog.findByText("Include component tables");
  },
};

export const S3ExportActive: Story = {
  name: "S3 Export Active",
  beforeEach: freezeNow,
  render: renderS3Export([
    s3Export({
      exportProgress: {
        type: "upToDate",
        // Nanoseconds since the epoch.
        ts: BigInt(NOW - 2 * 60 * 1000) * BigInt(1_000_000),
      },
    }),
  ]),
  play: async ({ canvasElement }) => {
    await within(canvasElement).findByText("Active");
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
