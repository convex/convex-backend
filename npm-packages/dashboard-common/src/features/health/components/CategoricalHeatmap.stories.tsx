import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { Sheet } from "@ui/Sheet";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import type { FunctionRateHeatmapData } from "@common/lib/appMetrics";
import { CategoricalHeatmap } from "./CategoricalHeatmap";

function makeBucketStartTimes(count: number, bucketMinutes: number): Date[] {
  const now = new Date("2026-01-01T13:00:00Z");
  const start = new Date(now.getTime() - count * bucketMinutes * 60 * 1000);
  return Array.from(
    { length: count },
    (_, i) => new Date(start.getTime() + i * bucketMinutes * 60 * 1000),
  );
}

const TWELVE_BUCKETS = makeBucketStartTimes(12, 5);

function row(key: string, values: (number | null)[]) {
  return {
    key,
    cells: values.map((value, i) => ({ time: TWELVE_BUCKETS[i], value })),
  };
}

const fullSpectrum: FunctionRateHeatmapData = {
  bucketStartTimes: TWELVE_BUCKETS,
  rows: [
    row(
      functionIdentifierValue("messages:list"),
      [98, 96, 95, 97, 92, 90, 88, 85, 82, 80, 78, 75],
    ),
    row(
      functionIdentifierValue("users:profile"),
      [85, 82, 78, 75, 70, 68, 65, 60, 55, 50, 45, 40],
    ),
    row(
      functionIdentifierValue("threads:search"),
      [60, 58, 55, 52, 48, 45, 42, 38, 35, 30, 25, 20],
    ),
    row(
      functionIdentifierValue("posts:popular"),
      [30, 28, 25, 20, 18, 15, 12, 10, 8, 5, 3, 2],
    ),
    row(
      functionIdentifierValue("analytics:heavy"),
      [5, 3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    ),
    row("_rest", [70, 72, 68, 65, 60, 58, 62, 55, 50, 48, 45, 42]),
  ],
};

const withMissingData: FunctionRateHeatmapData = {
  bucketStartTimes: TWELVE_BUCKETS,
  rows: [
    row(functionIdentifierValue("messages:list"), [
      95,
      92,
      null,
      null,
      88,
      85,
      null,
      80,
      78,
      null,
      75,
      72,
    ]),
    row(functionIdentifierValue("users:profile"), [
      null,
      null,
      null,
      60,
      55,
      50,
      45,
      40,
      null,
      null,
      null,
      30,
    ]),
    row("_rest", [80, 78, 75, 72, null, null, null, 65, 62, 60, 58, 55]),
  ],
};

const singleRest: FunctionRateHeatmapData = {
  bucketStartTimes: TWELVE_BUCKETS,
  rows: [row("_rest", [95, 92, 90, 88, 85, 82, 80, 78, 75, 72, 70, 68])],
};

const meta = {
  component: CategoricalHeatmap,
  render: (args) => (
    <Sheet>
      <div className="h-64 w-160">
        <CategoricalHeatmap {...args} />
      </div>
    </Sheet>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof CategoricalHeatmap>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CacheHitRate: Story = {
  args: {
    kind: "cacheHitRate",
    data: fullSpectrum,
  },
};

export const FailureRate: Story = {
  args: {
    kind: "failureRate",
    data: fullSpectrum,
  },
};

export const CacheHitRateWithMissingData: Story = {
  args: {
    kind: "cacheHitRate",
    data: withMissingData,
  },
};

export const FailureRateWithMissingData: Story = {
  args: {
    kind: "failureRate",
    data: withMissingData,
  },
};

export const SingleRestRow: Story = {
  args: {
    kind: "cacheHitRate",
    data: singleRest,
  },
};

export const WithViewMore: Story = {
  args: {
    kind: "cacheHitRate",
    data: fullSpectrum,
    onViewMore: fn(),
  },
};

export const Loading: Story = {
  args: {
    kind: "cacheHitRate",
    data: undefined,
  },
};

export const Empty: Story = {
  args: {
    kind: "cacheHitRate",
    data: null,
  },
};
