import { useBBQueries } from "api/api";
import { USAGE_REFRESH_INTERVAL_MS } from "api/usage";
import { parseUsageSummary, USAGE_SUMMARY_QUERY_ID } from "hooks/usageMetrics";
import type { UsagePricing } from "components/billing/lib/usagePrices";
import type { PricedUsageState } from "components/billing/lib/usageSpend";

export function useUsageByPricingPeriod({
  teamId,
  pricing,
  enabled,
}: {
  teamId: number;
  pricing: UsagePricing;
  enabled: boolean;
}): PricedUsageState {
  const active = enabled && pricing.status === "ready";
  const periods = pricing.status === "ready" ? pricing.periods : [];
  const { data, error } = useBBQueries({
    path: "/teams/{team_id}/usage/query",

    // Prices can change independently of usage; only the selection identifies the batch.
    requests: periods.map(({ range }) => ({
      pathParams: { team_id: active ? teamId.toString() : "" },
      queryParams: { queryId: USAGE_SUMMARY_QUERY_ID, ...range },
    })),
    swrOptions: { refreshInterval: USAGE_REFRESH_INTERVAL_MS },
  });
  // Empty batches resolve successfully even when pricing has no periods yet.
  if (!active) return { status: "loading" };
  if (error !== undefined) return { status: "error", error };
  if (!data) return { status: "loading" };
  return {
    status: "ready",
    summaries: periods.map(({ period }, index) => ({
      period,
      summary: parseUsageSummary(
        data[index].map((row) => row.map((cell) => cell ?? "")),
      ),
    })),
  };
}
