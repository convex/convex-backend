import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { CanonicalDomainForm } from "components/deploymentSettings/CustomDomains";

const deploymentName = "festive-capybara-729";
const deploymentUrl = `https://${deploymentName}.convex.cloud`;

const mockClient = mockConvexReactClient()
  .registerQueryFake(
    udfs.convexCloudUrl.default,
    () => "https://api.rapgenie.net",
  )
  .registerQueryFake(
    udfs.convexSiteUrl.default,
    () => `https://${deploymentName}.convex.site`,
  );

const mockConnectedDeployment = {
  deployment: {
    client: mockClient,
    httpClient: {} as never,
    deploymentUrl,
    adminKey: "storybook-admin-key",
    deploymentName,
  },
  isDisconnected: false,
};

const vanityDomains = [
  {
    domain: "api.rapgenie.net",
    requestDestination: "convexCloud" as const,
    deploymentName,
    creationTime: 0,
    verificationTime: 1,
  },
];

const meta = {
  component: CanonicalDomainForm,
  args: {
    vanityDomains,
  },
  render: (args) => (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
          <div className="max-w-2xl">
            <CanonicalDomainForm {...args} />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof CanonicalDomainForm>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
