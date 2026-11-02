import { useBBQuery } from "api/api";

export type DateRange = { from: string; to: string };

export type UsageState =
  | "Default"
  | "Approaching"
  | "Exceeded"
  | "Disabled"
  | "Paused";

export function useTeamUsageState(
  teamId: number | null,
): UsageState | undefined {
  const { data } = useBBQuery({
    path: "/teams/{team_id}/usage/team_usage_state",
    pathParams: {
      team_id: teamId?.toString() || "",
    },
    swrOptions: { refreshInterval: 0 },
  });
  return data?.usageState;
}

export function useCurrentBillingPeriod(teamId: number) {
  const { data } = useBBQuery({
    path: "/teams/{team_id}/usage/current_billing_period",
    pathParams: {
      team_id: teamId.toString(),
    },
  });
  return data;
}

export const USAGE_REFRESH_INTERVAL_MS =
  getURLConfigInt("usage_refresh_secs", 60 * 10) * 1000;

export type DatabricksQueryId = string;

export const rootComponentPath = "-root-component-";

type UsageQueryArgs = {
  queryId: DatabricksQueryId;
  teamId: number;
  projectId: number | null;
  functionId?: string;
  tableName?: string;
  period: DateRange | null;
  componentPrefix: string | null;
  deploymentName?: string;
  skip?: boolean;
};

function usageQueryParams({
  queryId,
  projectId,
  deploymentName,
  period,
  componentPrefix,
  functionId,
  tableName,
}: UsageQueryArgs) {
  return {
    queryId,
    projectId: projectId ?? undefined,
    ...(deploymentName ? { deploymentName } : {}),
    ...(functionId ? { udfId: functionId } : {}),
    ...(tableName ? { tableName } : {}),
    ...(period ? { from: period.from, to: period.to } : {}),
    ...(componentPrefix
      ? {
          componentPath:
            componentPrefix === "app" ? rootComponentPath : componentPrefix,
        }
      : {}),
  };
}

function normalizeUsageData(data: (string | null)[][]): string[][] {
  return data.map((row) => row.map((cell) => cell ?? ""));
}

export function useUsageQuery(args: UsageQueryArgs) {
  const result = useBBQuery({
    path: "/teams/{team_id}/usage/query",
    pathParams: { team_id: args.teamId.toString() },
    queryParams: usageQueryParams(args),
    swrOptions: {
      keepPreviousData: false,
      refreshInterval: USAGE_REFRESH_INTERVAL_MS,
      isPaused: () => args.skip ?? false,
    },
  });
  return {
    ...result,
    data: result.data && normalizeUsageData(result.data),
  };
}

function getURLConfigInt(name: string, default_value: number) {
  if (typeof window === "undefined") {
    return default_value;
  }
  const value = new URLSearchParams(window.location.search).get(name);
  return value !== null ? parseInt(value) : default_value;
}
