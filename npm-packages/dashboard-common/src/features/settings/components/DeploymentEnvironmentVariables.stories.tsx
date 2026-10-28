import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { GenericId } from "convex/values";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { DeploymentEnvironmentVariables } from "@common/features/settings/components/DeploymentEnvironmentVariables";

// Fixed "now" so the mocked `_creationTime`s are stable.
const NOW = new Date("2026-03-10T14:25:00Z").getTime();

const mockProject = {
  id: 7,
  teamId: 2,
  name: "My amazing app",
  slug: "my-amazing-app",
};

const cloudDeployment = {
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
} as const satisfies PlatformDeploymentResponse;

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.listEnvironmentVariables.default,
  () => [
    {
      _id: "k8envaaaaaaaaaaaaaaaaaaaaaaaaaaa" as GenericId<"_environment_variables">,
      _creationTime: NOW,
      name: "STRIPE_SECRET_KEY",
      value: "sk_test_abcdefghijklmnop",
    },
  ],
);

const mockConnectedDeployment = {
  deployment: {
    client: mockClient,
    httpClient: {} as never,
    deploymentUrl: cloudDeployment.deploymentUrl,
    adminKey: "storybook-admin-key",
    deploymentName: cloudDeployment.name,
  },
  isDisconnected: false,
};

function renderStory({
  deployment,
  isSelfHosted,
}: {
  deployment: PlatformDeploymentResponse;
  isSelfHosted: boolean;
}) {
  return (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider
          value={{
            ...mockDeploymentInfo,
            useCurrentProject: () => mockProject,
            useCurrentDeployment: () => deployment,
            useProjectEnvironmentVariables: () => ({ configs: [] }),
            projectsURI: "/t/acme/my-amazing-app",
            isSelfHosted,
          }}
        >
          <div className="max-w-2xl">
            <DeploymentEnvironmentVariables />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  );
}

const meta = {
  component: DeploymentEnvironmentVariables,
  parameters: { a11y: { test: "todo" } },
  render: (_args, { parameters }) => {
    const { deployment, isSelfHosted } = parameters as {
      deployment?: PlatformDeploymentResponse;
      isSelfHosted?: boolean;
    };
    return renderStory({
      deployment: deployment ?? cloudDeployment,
      isSelfHosted: isSelfHosted ?? false,
    });
  },
} satisfies Meta<typeof DeploymentEnvironmentVariables>;

export default meta;
type Story = StoryObj<typeof meta>;

export const DevDeployment: Story = {};

export const ProductionDeployment: Story = {
  parameters: {
    deployment: {
      ...cloudDeployment,
      name: "wary-mongoose-456",
      deploymentType: "prod",
      reference: "prod",
    },
  },
};

export const PreviewDeployment: Story = {
  parameters: {
    deployment: {
      ...cloudDeployment,
      name: "swift-otter-789",
      deploymentType: "preview",
      reference: "preview/add-search-page",
      previewIdentifier: "add-search-page",
    },
  },
};

export const CustomDeployment: Story = {
  parameters: {
    deployment: {
      ...cloudDeployment,
      name: "brave-lemur-321",
      deploymentType: "custom",
      reference: "custom/staging",
    },
  },
};

// Self-hosted deployments have no project-level environment variables.
export const SelfHosted: Story = {
  parameters: { isSelfHosted: true },
};
