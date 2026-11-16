import { useBBQueries } from "api/api";
import { USAGE_REFRESH_INTERVAL_MS, usageQueryParams } from "api/usage";
import {
  parseDailyUsageSummary,
  parseUsageSummary,
  summarizeDailyUsage,
  USAGE_SUMMARY_BY_DAY_QUERY_ID,
  USAGE_SUMMARY_QUERY_ID,
} from "hooks/usageMetrics";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import type { UsagePricing } from "components/billing/lib/usagePrices";
import {
  hasIncludedUsage,
  type PricedUsageState,
} from "components/billing/lib/usageSpend";

export function useUsageByPricingPeriod({
  teamId,
  pricing,
  enabled,
  projectId,
  componentPrefix,
}: {
  teamId: number;
  pricing: UsagePricing;
  enabled: boolean;
  projectId: number | null;
  componentPrefix: string | null;
}): PricedUsageState {
  const { dailyUsageSummaries } = useLaunchDarkly();
  const active = enabled && pricing.status === "ready";
  const periods = pricing.status === "ready" ? pricing.periods : [];
  // An empty prefix filters nothing, as in `usageQueryParams`.
  const filtered = projectId !== null || !!componentPrefix;
  // Filtered spend is a share of team spend only when there is an included
  // allowance to split; otherwise the filtered usage alone prices it.
  const withTeamSummaries =
    filtered && hasIncludedUsage(periods.map(({ period }) => period));
  const request = (filter: {
    projectId: number | null;
    componentPrefix: string | null;
  }) =>
    periods.map(({ range }) => ({
      pathParams: { team_id: active ? teamId.toString() : "" },
      queryParams: usageQueryParams({
        queryId: dailyUsageSummaries
          ? USAGE_SUMMARY_BY_DAY_QUERY_ID
          : USAGE_SUMMARY_QUERY_ID,
        teamId,
        period: range,
        ...filter,
      }),
    }));
  const { data, error } = useBBQueries({
    path: "/teams/{team_id}/usage/query",

    // Prices can change independently of usage; only the selection identifies the batch.
    requests: [
      ...request({ projectId, componentPrefix }),
      ...(withTeamSummaries
        ? request({ projectId: null, componentPrefix: null })
        : []),
    ],
    swrOptions: { refreshInterval: USAGE_REFRESH_INTERVAL_MS },
  });
  // Empty batches resolve successfully even when pricing has no periods yet.
  if (!active) return { status: "loading" };
  if (error !== undefined) return { status: "error", error };
  if (!data) return { status: "loading" };
  const parse = (index: number) => {
    const rows = data[index].map((row) => row.map((cell) => cell ?? ""));
    return dailyUsageSummaries
      ? summarizeDailyUsage(parseDailyUsageSummary(rows))
      : parseUsageSummary(rows);
  };
  return {
    status: "ready",
    summaries: periods.map(({ period }, index) => ({
      period,
      summary: parse(index),
      ...(withTeamSummaries
        ? { teamSummary: parse(periods.length + index) }
        : {}),
    })),
  };
}
