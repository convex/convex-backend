import { UsageLimit } from "./UsageLimits";

// Example limits for stories, spread across windows and both threshold types
// so the segmented control's enabled/configured counts are non-trivial
// (Monthly 2/3, Daily 2/2) and at least one metric card shows both rows
// filled.
export const EXAMPLE_USAGE_LIMITS: UsageLimit[] = [
  {
    id: "example-1",
    metric: "functionCalls",
    limit: 10_000_000,
    window: "month",
    limitType: "warning",
    enabled: true,
  },
  {
    id: "example-2",
    metric: "functionCalls",
    limit: 50_000_000,
    window: "month",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "example-3",
    metric: "databaseIoGb",
    limit: 500,
    window: "month",
    limitType: "warning",
    enabled: false,
  },
  {
    id: "example-4",
    metric: "actionComputeNodeJsGbHours",
    limit: 80,
    window: "day",
    limitType: "disable",
    enabled: true,
  },
  {
    id: "example-5",
    metric: "dataEgressGb",
    limit: 10,
    window: "day",
    limitType: "warning",
    enabled: true,
  },
];
