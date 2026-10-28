import { Context } from "../../bundler/context.js";
import { fetchTeamAndProject } from "./api.js";
import { BIG_BRAIN_URL, bigBrainFetch, provisionHost } from "./utils/utils.js";

export const ROOT_COMPONENT_PATH = "-root-component-";
// Query ID for the insights dataset (shared with dashboard/src/api/insights.ts).
export const INSIGHTS_QUERY_ID = "9ab3b74e-a725-480b-88a6-43e6bd70bd82";

export type OccRecentEvent = {
  timestamp: string;
  id: string;
  request_id: string;
  occ_document_id?: string;
  occ_write_source?: string;
  occ_retry_count: number;
};

export type ResourceRecentEvent = {
  timestamp: string;
  id: string;
  request_id: string;
  calls: {
    table_name: string;
    bytes_read: number;
    documents_read: number;
  }[];
  success: boolean;
};

export type OccInsight = {
  kind: "occRetried" | "occFailedPermanently";
  severity: "error" | "warning";
  functionId: string;
  componentPath: string | null;
  occCalls: number;
  occTableName?: string | undefined;
  recentEvents?: OccRecentEvent[] | undefined;
};

export type ResourceInsight = {
  kind:
    | "bytesReadLimit"
    | "bytesReadThreshold"
    | "documentsReadLimit"
    | "documentsReadThreshold";
  severity: "error" | "warning";
  functionId: string;
  componentPath: string | null;
  count: number;
  recentEvents?: ResourceRecentEvent[] | undefined;
};

export type Insight = OccInsight | ResourceInsight;

// Sorted from most to least severe.
const insightKinds: { kind: string; severity: "error" | "warning" }[] = [
  { kind: "documentsReadLimit", severity: "error" },
  { kind: "bytesReadLimit", severity: "error" },
  { kind: "occFailedPermanently", severity: "error" },
  { kind: "documentsReadThreshold", severity: "warning" },
  { kind: "bytesReadThreshold", severity: "warning" },
  { kind: "occRetried", severity: "warning" },
];

const insightKindMap = new Map(
  insightKinds.map((ik, i) => [ik.kind, { severity: ik.severity, order: i }]),
);

export function orderForKind(kind: string): number {
  return insightKindMap.get(kind)?.order ?? insightKinds.length;
}

export function severityForKind(kind: string): "error" | "warning" | undefined {
  return insightKindMap.get(kind)?.severity;
}

const MAX_RECENT_EVENTS = 5;

function parseRow(row: string[], includeRecentEvents: boolean): Insight | null {
  const kind = row[0];
  const functionId = row[1];
  const componentPath = row[2] === ROOT_COMPONENT_PATH ? null : row[2];
  const details = JSON.parse(row[3]);
  const common = { functionId, componentPath };
  const recentEvents = includeRecentEvents
    ? (details.recentEvents as any[]).slice(0, MAX_RECENT_EVENTS)
    : undefined;

  switch (kind) {
    case "occRetried":
      return {
        kind,
        severity: "warning" as const,
        ...common,
        occCalls: details.occCalls,
        occTableName: details.occTableName,
        recentEvents,
      };
    case "occFailedPermanently":
      return {
        kind,
        severity: "error" as const,
        ...common,
        occCalls: details.occCalls,
        occTableName: details.occTableName,
        recentEvents,
      };
    case "bytesReadLimit":
      return {
        kind,
        severity: "error" as const,
        ...common,
        count: details.count,
        recentEvents,
      };
    case "bytesReadThreshold":
      return {
        kind,
        severity: "warning" as const,
        ...common,
        count: details.count,
        recentEvents,
      };
    case "documentsReadLimit":
      return {
        kind,
        severity: "error" as const,
        ...common,
        count: details.count,
        recentEvents,
      };
    case "documentsReadThreshold":
      return {
        kind,
        severity: "warning" as const,
        ...common,
        count: details.count,
        recentEvents,
      };
    default:
      return null;
  }
}

/**
 * Fetch raw insight rows from the Big Brain usage API.
 * Returns the raw string[][] from the API response.
 */
export async function fetchRawInsightsData(
  ctx: Context,
  deploymentName: string,
): Promise<string[][]> {
  const { teamId } = await fetchTeamAndProject(ctx, deploymentName);

  const now = new Date();
  const hoursAgo72 = new Date(now.getTime() - 72 * 60 * 60 * 1000);
  const fromDate = hoursAgo72.toISOString().split("T")[0];
  const toDate = now.toISOString().split("T")[0];

  const queryParams = new URLSearchParams({
    queryId: INSIGHTS_QUERY_ID,
    deploymentName,
    from: fromDate,
    to: toDate,
  });
  const bbFetch = bigBrainFetch(ctx);
  const res = await bbFetch(
    new URL(
      `dashboard/teams/${teamId}/usage/query?${queryParams.toString()}`,
      BIG_BRAIN_URL,
    ),
    {
      method: "GET",
      headers: { Origin: provisionHost },
    },
  );
  return (await res.json()) as string[][];
}

/**
 * Fetch and parse insights from the Big Brain usage API for a deployment.
 * Returns insights sorted by severity (errors first).
 *
 * Pass `includeRecentEvents: true` to include up to 5 recent events per insight.
 */
export async function fetchInsights(
  ctx: Context,
  deploymentName: string,
  options?: { includeRecentEvents?: boolean },
): Promise<Insight[]> {
  const rawData = await fetchRawInsightsData(ctx, deploymentName);
  const includeRecentEvents = options?.includeRecentEvents ?? false;

  const insights: Insight[] = rawData.flatMap((row) => {
    const parsed = parseRow(row, includeRecentEvents);
    return parsed ? [parsed] : [];
  });

  insights.sort((a, b) => orderForKind(a.kind) - orderForKind(b.kind));
  return insights;
}
