import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import udfs from "@common/udfs";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { Sheet } from "@ui/Sheet";
import { ChartData } from "@common/lib/charts/types";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { HealthCard } from "@common/elements/HealthCard";
import { ChartForFunctionRate } from "./ChartForFunctionRate";

const mockClient = mockConvexReactClient().registerQueryFake(
  udfs.paginatedDeploymentEvents.default,
  () => ({
    page: [],
    isDone: true,
    continueCursor: "",
  }),
);

const points: Array<{ time: string; queryA: number; queryB: number }> = [
  { time: "12:00 PM", queryA: 12.5, queryB: 80.25 },
  { time: "12:01 PM", queryA: 20.75, queryB: 75 },
  { time: "12:02 PM", queryA: 10, queryB: 60.5 },
];

const functionKeyA = functionIdentifierValue("module.js:queryA");
const functionKeyB = functionIdentifierValue("module.js:queryB");

const chartData: ChartData = {
  xAxisKey: "time",
  data: points.map((row) => ({
    time: row.time,
    [functionKeyA]: row.queryA,
    [functionKeyB]: row.queryB,
  })),
  lineKeys: [
    { key: functionKeyA, name: "queryA", color: "var(--chart-line-1)" },
    { key: functionKeyB, name: "queryB", color: "var(--chart-line-2)" },
  ],
};

const meta = {
  component: ChartForFunctionRate,
  args: {
    chartData,
  },
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <Sheet>
          <div className="h-56">
            <ChartForFunctionRate {...args} />
          </div>
        </Sheet>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof ChartForFunctionRate>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CacheHitRate: Story = {
  args: {
    kind: "cacheHitRate",
  },
};

export const FailureRate: Story = {
  args: {
    kind: "failureRate",
  },
};

export const SchedulerStatus: Story = {
  args: {
    kind: "schedulerStatus",
    chartData: {
      ...chartData,
      data: points.map((row) => ({
        time: row.time,
        scheduler: Math.round((row.queryA / 10) * 10),
      })),
      lineKeys: [
        { key: "scheduler", name: "scheduler", color: "var(--chart-line-1)" },
      ],
    },
  },
};

const subscriptionInvalidationsRender = (
  args: { chartData: ChartData | null | undefined; kind: string },
  title: string,
  tip: string,
) => (
  <ConvexProvider client={mockClient}>
    <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
      <div className="w-96">
        <HealthCard title={title} tip={tip}>
          <ChartForFunctionRate
            chartData={args.chartData}
            kind={args.kind as any}
          />
        </HealthCard>
      </div>
    </DeploymentInfoContext.Provider>
  </ConvexProvider>
);

export const SubscriptionInvalidationsHealthPage: Story = {
  render: (args) =>
    subscriptionInvalidationsRender(
      args,
      "Subscription Invalidations",
      "The mutation and table pairs that cause the most subscription invalidations, bucketed by minute.",
    ),
  args: {
    kind: "subscriptionInvalidations",
    chartData: (() => {
      const keyA = "messages/mutations:sendMessage:messages";
      const keyB =
        "components/chat/messages:broadcastNotification:user_presence";
      const keyC = "auth:updateUserProfile:users";
      const subInvPoints = [
        { time: "12:00 PM", a: 45, b: 22, c: 8 },
        { time: "12:01 PM", a: 52, b: 18, c: 12 },
        { time: "12:02 PM", a: 38, b: 30, c: 5 },
        { time: "12:03 PM", a: 60, b: 25, c: 15 },
        { time: "12:04 PM", a: 41, b: 20, c: 9 },
      ];
      return {
        xAxisKey: "time",
        data: subInvPoints.map((row) => ({
          time: row.time,
          [keyA]: row.a,
          [keyB]: row.b,
          [keyC]: row.c,
        })),
        lineKeys: [
          { key: keyA, name: keyA, color: "var(--chart-line-2)" },
          { key: keyB, name: keyB, color: "var(--chart-line-3)" },
          { key: keyC, name: keyC, color: "var(--chart-line-4)" },
        ],
      };
    })(),
  },
};

export const SubscriptionInvalidationsFunctionPage: Story = {
  render: (args) =>
    subscriptionInvalidationsRender(
      args,
      "Subscription Invalidations",
      "The tables whose subscriptions are most frequently invalidated by this mutation, bucketed by minute.",
    ),
  args: {
    kind: "subscriptionInvalidations",
    chartData: (() => {
      const keyA = "messages";
      const keyB = "user_presence";
      const keyC = "conversation_participants";
      const subInvPoints = [
        { time: "12:00 PM", a: 45, b: 22, c: 8 },
        { time: "12:01 PM", a: 52, b: 18, c: 12 },
        { time: "12:02 PM", a: 38, b: 30, c: 5 },
        { time: "12:03 PM", a: 60, b: 25, c: 15 },
        { time: "12:04 PM", a: 41, b: 20, c: 9 },
      ];
      return {
        xAxisKey: "time",
        data: subInvPoints.map((row) => ({
          time: row.time,
          [keyA]: row.a,
          [keyB]: row.b,
          [keyC]: row.c,
        })),
        lineKeys: [
          { key: keyA, name: keyA, color: "var(--chart-line-2)" },
          { key: keyB, name: keyB, color: "var(--chart-line-3)" },
          { key: keyC, name: keyC, color: "var(--chart-line-4)" },
        ],
      };
    })(),
  },
};

export const SubscriptionInvalidationsWithRest: Story = {
  render: (args) =>
    subscriptionInvalidationsRender(
      args,
      "Subscription Invalidations",
      "The mutation and table pairs that cause the most subscription invalidations, bucketed by minute.",
    ),
  args: {
    kind: "subscriptionInvalidations",
    chartData: (() => {
      const keyA =
        "features/realtime/sync:pushRealtimeUpdate:active_subscriptions";
      const keyRest = "_rest";
      const subInvPoints = [
        { time: "12:00 PM", a: 120, rest: 35 },
        { time: "12:01 PM", a: 95, rest: 42 },
        { time: "12:02 PM", a: 110, rest: 28 },
        { time: "12:03 PM", a: 130, rest: 38 },
        { time: "12:04 PM", a: 105, rest: 31 },
      ];
      return {
        xAxisKey: "time",
        data: subInvPoints.map((row) => ({
          time: row.time,
          [keyA]: row.a,
          [keyRest]: row.rest,
        })),
        lineKeys: [
          { key: keyA, name: keyA, color: "var(--chart-line-2)" },
          { key: keyRest, name: keyRest, color: "var(--chart-line-1)" },
        ],
      };
    })(),
  },
};

export const Empty: Story = {
  args: {
    chartData: null,
    kind: "cacheHitRate",
  },
};
