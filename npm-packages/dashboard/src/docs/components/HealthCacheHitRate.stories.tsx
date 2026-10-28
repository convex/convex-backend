import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { mocked } from "storybook/test";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import type { ChartData } from "@common/lib/charts/types";
import {
  useTopKFunctionMetrics,
  useTopKFunctionRateHeatmap,
  type FunctionRateHeatmapData,
} from "@common/lib/appMetrics";
import { CacheHitRate } from "@common/features/health/components/CacheHitRate";

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.paginatedDeploymentEvents.default,
  () => ({ page: [], isDone: true, continueCursor: "" }),
);

const functions = [
  {
    key: functionIdentifierValue("messages:list"),
    values: [95, 96, 97, 94, 98, 97],
  },
  {
    key: functionIdentifierValue("channels:list"),
    values: [88, 90, 91, 89, 92, 91],
  },
  {
    key: functionIdentifierValue("users:current"),
    values: [72, 74, 73, 75, 76, 74],
  },
  {
    key: functionIdentifierValue("notifications:list"),
    values: [68, 70, 69, 71, 72, 70],
  },
  {
    key: functionIdentifierValue("teams:members"),
    values: [60, 62, 64, 61, 65, 63],
  },
  {
    key: "_rest",
    values: [82, 80, 78, 76, 79, 81],
  },
] as const;

const chartData: ChartData = {
  xAxisKey: "time",
  data: [
    "12:00 PM",
    "12:10 PM",
    "12:20 PM",
    "12:30 PM",
    "12:40 PM",
    "12:50 PM",
  ].map((time, i) => ({
    time,
    ...Object.fromEntries(functions.map(({ key, values }) => [key, values[i]])),
  })),
  lineKeys: functions.map(({ key }, i) => ({
    key,
    name: key,
    color: `var(--chart-line-${i + 1})`,
  })),
};

const bucketStartTimes = Array.from(
  { length: 12 },
  (_, i) => new Date(Date.UTC(2026, 0, 1, 12, i * 5)),
);

function heatmapRow(
  key: string,
  values: number[],
): FunctionRateHeatmapData["rows"][number] {
  return {
    key,
    cells: values.map((value, i) => ({ time: bucketStartTimes[i], value })),
  };
}

const heatmapData: FunctionRateHeatmapData = {
  bucketStartTimes,
  rows: [
    heatmapRow(
      functions[0].key,
      [98, 96, 95, 97, 92, 90, 88, 95, 96, 97, 95, 94],
    ),
    heatmapRow(
      functions[1].key,
      [85, 82, 78, 75, 80, 78, 75, 72, 78, 80, 82, 85],
    ),
    heatmapRow(
      functions[2].key,
      [60, 58, 55, 52, 60, 65, 62, 58, 60, 62, 65, 68],
    ),
    heatmapRow(
      functions[3].key,
      [55, 52, 50, 48, 52, 54, 51, 49, 50, 53, 55, 57],
    ),
    heatmapRow(
      functions[4].key,
      [42, 40, 38, 35, 36, 39, 41, 44, 42, 40, 38, 37],
    ),
    heatmapRow(
      functions[5].key,
      [70, 72, 68, 65, 60, 58, 62, 55, 50, 48, 45, 42],
    ),
  ],
};

const meta = {
  component: CacheHitRate,
  parameters: { a11y: { test: "todo" } },
  beforeEach: () => {
    mocked(useTopKFunctionMetrics).mockReturnValue(chartData);
    mocked(useTopKFunctionRateHeatmap).mockReturnValue(heatmapData);
  },
  render: () => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <div className="max-w-sm">
          <CacheHitRate />
        </div>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
} satisfies Meta<typeof CacheHitRate>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
