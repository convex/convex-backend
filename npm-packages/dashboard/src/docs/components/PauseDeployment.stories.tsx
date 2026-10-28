import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { PauseDeployment } from "@common/features/settings/components/PauseDeployment";

const deploymentName = "local";
const deploymentUrl = "http://127.0.0.1:3000";

function makeMockClient(state: "running" | "paused") {
  return mockConvexReactClient().registerQueryFake(
    udfs.deploymentState.deploymentState,
    () => ({ state }),
  );
}

const mockRunningClient = makeMockClient("running");
const mockPausedClient = makeMockClient("paused");

function makeConnectedDeployment(client: ReturnType<typeof makeMockClient>) {
  return {
    deployment: {
      client,
      httpClient: {} as never,
      deploymentUrl,
      adminKey: "storybook-admin-key",
      deploymentName,
    },
    isDisconnected: false,
  };
}

const meta = {
  component: PauseDeployment,
  render: (args, { parameters }) => {
    const { mockClient } = parameters as {
      mockClient: ReturnType<typeof makeMockClient>;
    };
    return (
      <ConnectedDeploymentContext.Provider
        value={makeConnectedDeployment(mockClient)}
      >
        <ConvexProvider client={mockClient}>
          <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
            <div className="max-w-2xl">
              <PauseDeployment {...args} />
            </div>
          </DeploymentInfoContext.Provider>
        </ConvexProvider>
      </ConnectedDeploymentContext.Provider>
    );
  },
} satisfies Meta<typeof PauseDeployment>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Running: Story = {
  parameters: {
    mockClient: mockRunningClient,
  },
};

export const Paused: Story = {
  parameters: {
    mockClient: mockPausedClient,
  },
};
