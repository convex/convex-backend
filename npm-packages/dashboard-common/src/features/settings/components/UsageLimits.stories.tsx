import type { Meta, StoryObj } from "@storybook/nextjs";
import { useState, type ComponentProps } from "react";
import {
  UsageLimits,
  UsageLimit,
  UsageLimitConfig,
  CurrentUsage,
  computeUnbilledMetrics,
} from "./UsageLimits";
import { EXAMPLE_USAGE_LIMITS } from "./usageLimitsFixtures";

// Current usage per metric/window. `searchQueryGb` has usage but no configured
// limit, to show the Current usage column renders without a limit.
const EXAMPLE_CURRENT_USAGE: CurrentUsage = {
  functionCalls: { month: 4_500_000, day: 180_000 },
  databaseIoGb: { month: 120 },
  actionComputeNodeJsGbHours: { day: 30 },
  dataEgressGb: { day: 3 },
  searchQueryGb: { month: 8 },
};

// Every applicable metric has usage, and every limit is at a different fill
// level, so the whole spread of donut gauges shows at once: barely started
// (2%), halfway (45%), nearly full (92%), and an Inactive limit that's over
// its amount (120%) without triggering.
const RICH_USAGE_LIMITS: UsageLimit[] = [
  {
    id: "rich-1",
    metric: "functionCalls",
    limit: 10_000_000,
    window: "month",
    limitType: "warning",
    enabled: true,
  },
  {
    id: "rich-2",
    metric: "functionCalls",
    limit: 50_000_000,
    window: "month",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "rich-3",
    metric: "actionComputeConvexGbHours",
    limit: 100,
    window: "month",
    limitType: "warning",
    enabled: true,
  },
  {
    id: "rich-4",
    metric: "actionComputeNodeJsGbHours",
    limit: 100,
    window: "month",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "rich-5",
    metric: "databaseIoGb",
    limit: 100,
    window: "month",
    limitType: "warning",
    enabled: false,
  },
  {
    id: "rich-6",
    metric: "dataEgressGb",
    limit: 1000,
    window: "month",
    limitType: "disable",
    enabled: true,
  },
];

const RICH_CURRENT_USAGE: CurrentUsage = {
  functionCalls: { month: 4_500_000 },
  actionComputeConvexGbHours: { month: 92 },
  actionComputeNodeJsGbHours: { month: 2 },
  databaseIoGb: { month: 120 },
  searchQueryGb: { month: 8 },
  dataEgressGb: { month: 450 },
};

// A Business/Enterprise team on a non-dedicated deployment: Convex runtime and
// Query/Mutation compute aren't billed. Includes a configured limit for an
// unbilled metric in the Monthly window so it pins to the top with a callout.
const UNBILLED_METRICS = computeUnbilledMetrics({
  isBusinessPlan: true,
  isDedicated: false,
});
const UNBILLED_EXAMPLE: UsageLimit[] = [
  {
    id: "unbilled-1",
    metric: "actionComputeConvexGbHours",
    limit: 100,
    window: "month",
    limitType: "warning",
    enabled: true,
  },
  {
    id: "billed-1",
    metric: "functionCalls",
    limit: 50_000_000,
    window: "month",
    limitType: "disable",
    enabled: true,
  },
];

// Triggered limits spread across windows. The Daily window has a triggered
// disable threshold (the most severe trigger), so the page opens on Daily and
// that segment shows an error badge; the triggered warnings show warning badges.
const TRIGGERED_USAGE_LIMITS: UsageLimit[] = [
  {
    id: "triggered-1",
    metric: "functionCalls",
    limit: 10_000_000,
    window: "month",
    limitType: "warning",
    enabled: true,
  },
  {
    id: "triggered-2",
    metric: "dataEgressGb",
    limit: 10,
    window: "day",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "triggered-3",
    metric: "actionComputeNodeJsGbHours",
    limit: 80,
    window: "day",
    limitType: "warning",
    enabled: true,
  },
  {
    id: "triggered-4",
    metric: "databaseIoGb",
    limit: 500,
    window: "month",
    limitType: "warning",
    enabled: false,
  },
];

const TRIGGERED_CURRENT_USAGE: CurrentUsage = {
  functionCalls: { month: 11_200_000 },
  dataEgressGb: { day: 13 },
  actionComputeNodeJsGbHours: { day: 92 },
  databaseIoGb: { month: 210 },
};

// Dev deployments can't have warning limits (they send no email, and the
// backend rejects them), so this fixture is disable thresholds only, spread
// across windows with one inactive.
const DEV_USAGE_LIMITS: UsageLimit[] = [
  {
    id: "dev-1",
    metric: "functionCalls",
    limit: 50_000_000,
    window: "month",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "dev-2",
    metric: "databaseIoGb",
    limit: 500,
    window: "month",
    limitType: "disable",
    enabled: false,
  },
  {
    id: "dev-3",
    metric: "actionComputeNodeJsGbHours",
    limit: 80,
    window: "day",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "dev-4",
    metric: "dataEgressGb",
    limit: 10,
    window: "day",
    limitType: "disable",
    enabled: true,
  },
];

// Wrap in a stateful container so edits/additions/removals persist in the
// story. This mimics what the real API-backed manager does, but in memory.
// Every prop other than the CRUD callbacks is forwarded straight through.
function StatefulUsageLimits({
  usageLimits: initial,
  ...rest
}: ComponentProps<typeof UsageLimits>) {
  const [usageLimits, setUsageLimits] = useState<UsageLimit[]>(initial);
  let nextId = usageLimits.length;

  const onCreate = (config: UsageLimitConfig) => {
    nextId += 1;
    setUsageLimits((prev) => [...prev, { id: `example-${nextId}`, ...config }]);
  };
  const onUpdate = (id: string, config: UsageLimitConfig) => {
    setUsageLimits((prev) =>
      prev.map((limit) => (limit.id === id ? { id, ...config } : limit)),
    );
  };
  const onDelete = (id: string) => {
    setUsageLimits((prev) => prev.filter((limit) => limit.id !== id));
  };

  return (
    <UsageLimits
      {...rest}
      usageLimits={usageLimits}
      onCreate={onCreate}
      onUpdate={onUpdate}
      onDelete={onDelete}
    />
  );
}

// A typical standard-plan, non-dedicated deployment: CPU and Query/Mutation
// compute aren't billed, so those cards are hidden unless configured (rather
// than every metric showing at once). Stories can override this.
const STANDARD_UNBILLED = computeUnbilledMetrics({
  isBusinessPlan: false,
  isDedicated: false,
});

const meta = {
  component: UsageLimits,
  args: {
    onCreate: () => {},
    onUpdate: () => {},
    onDelete: () => {},
    unbilledMetrics: STANDARD_UNBILLED,
    deploymentType: "prod",
    billingUri: "/t/team/settings/billing",
  },
  render: (args) => <StatefulUsageLimits {...args} />,
} satisfies Meta<typeof UsageLimits>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
  },
};

// Usage on every metric with limits at every fill level: donut gauges from
// barely started to nearly full, plus an Inactive limit sitting over its
// amount and a metric with usage but no limit configured.
export const WithCurrentUsage: Story = {
  args: {
    usageLimits: RICH_USAGE_LIMITS,
    currentUsage: RICH_CURRENT_USAGE,
  },
};

// AI Gateway usage is reported as fractional USD while thresholds are whole
// dollars, so this verifies the two values remain directly comparable.
export const AiGatewayUsage: Story = {
  args: {
    usageLimits: [
      {
        id: "ai-gateway-1",
        metric: "aiGatewayCostDollars",
        limit: 1,
        window: "month",
        limitType: "warning",
        enabled: true,
      },
    ],
    currentUsage: {
      aiGatewayCostDollars: { month: 0.25 },
    },
  },
};

// No limits configured: every applicable metric still shows, with empty rows.
export const Empty: Story = {
  args: {
    usageLimits: [],
  },
};

// No limits configured, but the deployment has usage: the Current usage
// column fills in while every threshold cell offers Configure limit.
export const NoLimitsWithUsage: Story = {
  args: {
    usageLimits: [],
    currentUsage: EXAMPLE_CURRENT_USAGE,
  },
};

export const Loading: Story = {
  args: {
    usageLimits: [],
    isLoading: true,
  },
};

// A member without write access: no Add/Edit buttons.
export const ReadOnly: Story = {
  args: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
    canWrite: false,
  },
};

// Some limits are currently triggered. Triggered thresholds take precedence:
// the segmented control gains warning/error badges counting them, and the page
// opens on the window with the most severe trigger (here, Daily's disable).
export const Triggered: Story = {
  args: {
    usageLimits: TRIGGERED_USAGE_LIMITS,
    currentUsage: TRIGGERED_CURRENT_USAGE,
  },
};

// The historical-usage backfill is still in progress, so the usage figures may
// understate actual usage — surfaced with a callout above the limits.
export const SeedInProgress: Story = {
  args: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
    seedStatus: "pending",
  },
};

// The historical-usage backfill failed: the callout warns the figures may
// understate actual usage but notes limits are still enforced going forward.
// ("partial" renders the same callout as "pending", so these two stories cover
// every non-complete status.)
export const SeedFailed: Story = {
  args: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
    seedStatus: "failed",
  },
};

// A freshly created deployment has no historical usage to load yet, so the seed
// status callout is suppressed even though the backfill isn't "complete".
export const SeedInProgressNewDeployment: Story = {
  args: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
    seedStatus: "pending",
    deploymentCreateTime: Date.now(),
  },
};

// Some metrics aren't billed on this plan/deployment. A configured unbilled
// metric pins to the top of its window with a callout.
export const WithUnbilledMetrics: Story = {
  args: {
    usageLimits: UNBILLED_EXAMPLE,
    unbilledMetrics: UNBILLED_METRICS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
  },
};

// A dev deployment: no email notifications are sent, so the warning threshold
// column is disabled with an explanation, and the disable threshold tooltip
// notes no email is sent.
export const DevDeployment: Story = {
  args: {
    usageLimits: DEV_USAGE_LIMITS,
    currentUsage: EXAMPLE_CURRENT_USAGE,
    deploymentType: "dev",
  },
};

// Deployment type unknown (e.g. self-hosted): treated like prod/preview/custom,
// so both thresholds are available and tooltips mention emailing all team
// members.
export const UnknownDeploymentType: Story = {
  args: {
    usageLimits: EXAMPLE_USAGE_LIMITS,
    deploymentType: undefined,
  },
};
