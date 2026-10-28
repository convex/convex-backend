import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import {
  ConnectedDeploymentContext,
  DeploymentInfoContext,
} from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import type { ChartData } from "@common/lib/charts/types";
import { SchedulerStatus } from "@common/elements/SchedulerStatus";

const deploymentName = "happy-capybara-123";
const deploymentUrl = `https://${deploymentName}.convex.cloud`;
const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.paginatedDeploymentEvents.default,
  () => ({ page: [], isDone: true, continueCursor: "" }),
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

const healthyLag: ChartData = {
  xAxisKey: "time",
  data: [
    { time: "12:00 PM", lag: 0 },
    { time: "12:10 PM", lag: 0 },
    { time: "12:20 PM", lag: 0 },
    { time: "12:30 PM", lag: 0 },
    { time: "12:40 PM", lag: 0 },
    { time: "12:50 PM", lag: 0 },
  ],
  lineKeys: [{ key: "lag", name: "Lag", color: "var(--chart-line-1)" }],
};

const meta = {
  component: SchedulerStatus,
  render: (args) => (
    <ConnectedDeploymentContext.Provider value={mockConnectedDeployment}>
      <ConvexProvider client={mockClient}>
        <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
          <div className="max-w-sm">
            <SchedulerStatus {...args} />
          </div>
        </DeploymentInfoContext.Provider>
      </ConvexProvider>
    </ConnectedDeploymentContext.Provider>
  ),
} satisfies Meta<typeof SchedulerStatus>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { lag: healthyLag },
};
