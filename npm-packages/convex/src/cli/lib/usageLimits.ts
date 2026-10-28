import type {
  GetCurrentUsageResponse,
  UsageLimitConfigRequest,
  UsageLimitConfigResponse,
  UsageLimitMetric,
} from "@convex-dev/platform/deploymentApi";
import { Context } from "../../bundler/context.js";
import { deploymentFetch, logAndHandleFetchError } from "./utils/utils.js";

export type UsageLimitDeployment = {
  deploymentUrl: string;
  adminKey: string;
};

export const USAGE_LIMIT_METRICS = [
  "functionCalls",
  "queryMutationComputeGbHours",
  "actionComputeConvexGbHours",
  "actionComputeNodeJsGbHours",
  "actionComputeCpuGbHours",
  "databaseIoGb",
  "searchQueryGb",
  "dataEgressGb",
  "aiGatewayCostDollars",
] as const satisfies readonly UsageLimitMetric[];

// Fails to compile if the backend's `UsageLimitMetric` gains a metric not listed
// above, so the CLI's `--metric` choices and ordering stay complete.
type MissingMetric = Exclude<
  UsageLimitMetric,
  (typeof USAGE_LIMIT_METRICS)[number]
>;
const _metricsExhaustive: [MissingMetric] extends [never] ? true : never = true;
void _metricsExhaustive;

export const USAGE_LIMIT_WINDOWS = ["day", "month"] as const;
export type UsageLimitWindow = (typeof USAGE_LIMIT_WINDOWS)[number];

export const USAGE_LIMIT_TYPES = ["warning", "disable"] as const;
export type UsageLimitType = (typeof USAGE_LIMIT_TYPES)[number];

export const METRIC_LABELS: Record<UsageLimitMetric, string> = {
  functionCalls: "Function calls",
  queryMutationComputeGbHours: "Query/Mutation compute",
  actionComputeConvexGbHours: "Action compute",
  actionComputeNodeJsGbHours: "Action compute (Node.js)",
  actionComputeCpuGbHours: "Action compute (CPU)",
  databaseIoGb: "Database I/O",
  searchQueryGb: "Search queries",
  dataEgressGb: "Data egress",
  aiGatewayCostDollars: "AI Gateway",
};

export function metricLabel(metric: string): string {
  // Look up permissively: a newer backend may report a metric this CLI's label
  // map doesn't know, in which case fall back to the raw id.
  return (METRIC_LABELS as Record<string, string>)[metric] ?? metric;
}

async function usageLimitFetch(
  ctx: Context,
  deployment: UsageLimitDeployment,
  path: string,
  init?: { method?: string; body?: unknown },
): Promise<unknown> {
  const fetch = deploymentFetch(ctx, deployment);
  try {
    const response = await fetch(`/api/v1${path}`, {
      method: init?.method ?? "GET",
      ...(init?.body !== undefined ? { body: JSON.stringify(init.body) } : {}),
    });
    const text = await response.text();
    return text.length > 0 ? JSON.parse(text) : undefined;
  } catch (e) {
    return await logAndHandleFetchError(ctx, e);
  }
}

export async function listUsageLimits(
  ctx: Context,
  deployment: UsageLimitDeployment,
): Promise<UsageLimitConfigResponse[]> {
  const result = (await usageLimitFetch(
    ctx,
    deployment,
    "/list_usage_limits",
  )) as {
    usageLimits: UsageLimitConfigResponse[];
  };
  return result.usageLimits;
}

export async function createUsageLimit(
  ctx: Context,
  deployment: UsageLimitDeployment,
  config: UsageLimitConfigRequest,
): Promise<UsageLimitConfigResponse> {
  const result = (await usageLimitFetch(
    ctx,
    deployment,
    "/create_usage_limit",
    {
      method: "POST",
      body: config,
    },
  )) as { usageLimit: UsageLimitConfigResponse };
  return result.usageLimit;
}

export async function updateUsageLimit(
  ctx: Context,
  deployment: UsageLimitDeployment,
  id: string,
  config: UsageLimitConfigRequest,
): Promise<UsageLimitConfigResponse> {
  const result = (await usageLimitFetch(
    ctx,
    deployment,
    `/update_usage_limit/${id}`,
    { method: "POST", body: config },
  )) as { usageLimit: UsageLimitConfigResponse };
  return result.usageLimit;
}

export async function deleteUsageLimit(
  ctx: Context,
  deployment: UsageLimitDeployment,
  id: string,
): Promise<void> {
  await usageLimitFetch(ctx, deployment, `/delete_usage_limit/${id}`, {
    method: "POST",
  });
}

export async function getCurrentUsage(
  ctx: Context,
  deployment: UsageLimitDeployment,
): Promise<GetCurrentUsageResponse> {
  return (await usageLimitFetch(
    ctx,
    deployment,
    "/get_current_usage",
  )) as GetCurrentUsageResponse;
}

function metricRank(metric: string): number {
  const i = (USAGE_LIMIT_METRICS as readonly string[]).indexOf(metric);
  // Unknown metrics (e.g. a newer backend adds one) sort last, stably.
  return i === -1 ? USAGE_LIMIT_METRICS.length : i;
}

// Windows coarsest-first and warning-before-disable,.
const windowRank = (w: string) => (w === "month" ? 0 : 1);
const typeRank = (t: string) => (t === "warning" ? 0 : 1);

export function compareUsageLimits(
  a: { metric: string; window: string; limitType: string },
  b: { metric: string; window: string; limitType: string },
): number {
  return (
    metricRank(a.metric) - metricRank(b.metric) ||
    windowRank(a.window) - windowRank(b.window) ||
    typeRank(a.limitType) - typeRank(b.limitType)
  );
}

export function compareMetricNames(a: string, b: string): number {
  return metricRank(a) - metricRank(b);
}

// A limit annotated with the current usage in its window and whether it's
// currently triggered.
export type UsageLimitStatus = UsageLimitConfigResponse & {
  currentUsage: number | null;
  unit: string | null;
  triggered: boolean;
};

function usageInWindow(
  usage: GetCurrentUsageResponse,
  metric: string,
  window: string,
): number | null {
  const m = usage.metrics[metric];
  if (m === undefined) {
    return null;
  }
  return window === "day" ? m.usage.current_day : m.usage.current_month;
}

// List usage limits annotated with current usage and triggered state, sorted in
// dashboard order. Fetches the limits and current usage together.
export async function listUsageLimitsWithStatus(
  ctx: Context,
  deployment: UsageLimitDeployment,
): Promise<{
  limits: UsageLimitStatus[];
  seedStatus: GetCurrentUsageResponse["seedStatus"];
}> {
  const [limits, usage] = await Promise.all([
    listUsageLimits(ctx, deployment),
    getCurrentUsage(ctx, deployment),
  ]);
  const withStatus = limits
    .map((limit): UsageLimitStatus => {
      const currentUsage = usageInWindow(usage, limit.metric, limit.window);
      const unit = usage.metrics[limit.metric]?.unit ?? null;
      const triggered =
        limit.enabled && currentUsage !== null && currentUsage >= limit.limit;
      return { ...limit, currentUsage, unit, triggered };
    })
    .sort(compareUsageLimits);
  return { limits: withStatus, seedStatus: usage.seedStatus };
}

// A usage limit is uniquely identified by (metric, window, limitType) — the
// same key the backend enforces uniqueness on. The HTTP API addresses limits by
// document id, so callers who know the natural key list and resolve it here.
export type UsageLimitKey = {
  metric: UsageLimitMetric;
  window: string;
  limitType: string;
};

export async function findUsageLimitByKey(
  ctx: Context,
  deployment: UsageLimitDeployment & { deploymentNotice: string },
  key: UsageLimitKey,
): Promise<UsageLimitConfigResponse> {
  const match = (await listUsageLimits(ctx, deployment)).find(
    (limit) =>
      limit.metric === key.metric &&
      limit.window === key.window &&
      limit.limitType === key.limitType,
  );
  if (match === undefined) {
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `error: No ${key.limitType} usage limit on ${key.metric} per ${key.window}${deployment.deploymentNotice}.`,
    });
  }
  return match;
}
