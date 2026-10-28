import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import type { ChartData } from "@common/lib/charts/types";
import { FailureRateCard } from "@common/features/health/components/FailureRate";

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.paginatedDeploymentEvents.default,
  () => ({
    page: [],
    isDone: true,
    continueCursor: "",
  }),
);

const auth = functionIdentifierValue("auth:login");
const users = functionIdentifierValue("users:create");
const payments = functionIdentifierValue("payments:charge");

const chartData: ChartData = {
  xAxisKey: "time",
  data: [
    { time: "12:00 PM", [auth]: 0.9, [users]: 5.8, [payments]: 10.6 },
    { time: "12:10 PM", [auth]: 5.2, [users]: 10.4, [payments]: 20.3 },
    { time: "12:20 PM", [auth]: 0.7, [users]: 5.9, [payments]: 10.1 },
    { time: "12:30 PM", [auth]: 5.6, [users]: 15.2, [payments]: 20.1 },
    { time: "12:40 PM", [auth]: 5.0, [users]: 10.0, [payments]: 10.8 },
    { time: "12:50 PM", [auth]: 5.4, [users]: 10.7, [payments]: 10.5 },
  ],
  lineKeys: [
    { key: auth, name: auth, color: "var(--chart-line-1)" },
    { key: users, name: users, color: "var(--chart-line-2)" },
    { key: payments, name: payments, color: "var(--chart-line-3)" },
  ],
};

const meta = {
  component: FailureRateCard,
  args: { chartData },
  parameters: {
    a11y: { test: "todo" },
  },
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-sm">
          <FailureRateCard {...args} />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
} satisfies Meta<typeof FailureRateCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
