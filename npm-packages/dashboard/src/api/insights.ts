import { useCurrentDeployment } from "api/deployments";
import { useCurrentTeam } from "api/teams";

import { rootComponentPath, useUsageQuery } from "api/usage";
import {
  itemIdentifier,
  useModuleFunctions,
} from "@common/lib/functions/FunctionsProvider";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";

export function useInsightsPeriod() {
  const now = new Date();
  const hoursAgo72 = new Date(now.getTime() - 72 * 60 * 60 * 1000);

  return {
    from: hoursAgo72.toISOString(),
    to: now.toISOString(),
  };
}

type HourlyCount = {
  hour: string;
  count: number;
};

type OccRecentEvent = {
  timestamp: string;
  id: string;
  request_id: string;
  occ_document_id?: string;
  occ_write_source?: string;
  occ_retry_count: number;
  // Executions this event covers; absent on events recorded one per execution.
  calls?: number;
};

type BytesReadRecentEvent = {
  timestamp: string;
  id: string;
  request_id: string;
  calls: { table_name: string; bytes_read: number; documents_read: number }[];
  success: boolean;
};

export type Insight = { functionId: string; componentPath: string | null } & (
  | {
      kind: "occRetried" | "occFailedPermanently";
      details: {
        occCalls: number;
        occTableName?: string;
        hourlyCounts: HourlyCount[];
        recentEvents: OccRecentEvent[];
      };
    }
  | {
      kind:
        | "bytesReadLimit"
        | "bytesReadThreshold"
        | "documentsReadLimit"
        | "documentsReadThreshold";
      details: {
        count: number;
        hourlyCounts: HourlyCount[];
        recentEvents: BytesReadRecentEvent[];
      };
    }
);

// Helper to pad and sort hourly data
function padAndSortHourlyData(
  hourlyCounts: HourlyCount[],
  periodStart?: string,
): HourlyCount[] {
  // Get current time to limit future data points
  const currentTime = new Date();

  if (hourlyCounts.length === 0) {
    // If no data but we have period start, create empty data from period start to now
    if (periodStart) {
      const startDate = new Date(periodStart);
      const endDate = new Date(currentTime);

      const result: HourlyCount[] = [];
      const currentDate = new Date(startDate);

      while (currentDate < endDate) {
        // Format the hour in the expected format for the chart (YYYY-MM-DD HH:00:00)
        // Instead of ISO format with T separator
        const year = currentDate.getUTCFullYear();
        const month = String(currentDate.getUTCMonth() + 1).padStart(2, "0");
        const day = String(currentDate.getUTCDate()).padStart(2, "0");
        const hour = String(currentDate.getUTCHours()).padStart(2, "0");

        const formattedHour = `${year}-${month}-${day} ${hour}:00:00`;

        result.push({
          hour: formattedHour,
          count: 0,
        });
        currentDate.setHours(currentDate.getHours() + 1);
      }

      return result;
    }
    return [];
  }

  // Extract all hours and find min/max
  const hours = hourlyCounts.map((item) => item.hour);
  const hourToCountMap = new Map<string, number>();

  // Fill the map with existing data
  hourlyCounts.forEach((item) => {
    hourToCountMap.set(item.hour, item.count);
  });

  // Determine start and end dates
  let startDate: Date;
  let endDate: Date;

  if (periodStart) {
    // Use the provided period start and current time as end
    startDate = new Date(periodStart);
    endDate = new Date(currentTime);
  } else {
    // Otherwise use min/max from the data
    try {
      // Sort hours and determine continuous range
      const sortedHours = [...hours].sort();
      const minHour = sortedHours[0];
      const maxHour = sortedHours[sortedHours.length - 1];

      // Parse the hours to get start/end dates - handle both formats (with or without T separator)
      let minDate: string;
      let minHourNum: string;
      let maxDate: string;
      let maxHourNum: string;

      if (minHour.includes("T")) {
        [minDate, minHourNum] = minHour.split("T");
        [maxDate, maxHourNum] = maxHour.split("T");
      } else {
        // Fallback if T separator isn't present
        minDate = minHour.substring(0, 10);
        minHourNum = minHour.substring(11, 13) || "00";
        maxDate = maxHour.substring(0, 10);
        maxHourNum = maxHour.substring(11, 13) || "23";
      }

      startDate = new Date(`${minDate}T${minHourNum}:00:00Z`);
      endDate = new Date(`${maxDate}T${maxHourNum}:59:59Z`);

      // Ensure we don't add future data points
      if (endDate > currentTime) {
        endDate.setTime(currentTime.getTime());
      }

      // Check if dates are valid
      if (
        Number.isNaN(startDate.getTime()) ||
        Number.isNaN(endDate.getTime())
      ) {
        throw new Error("Invalid date range");
      }
    } catch (error) {
      console.error("Error parsing date range:", error);
      // Return the original data if we can't parse the dates
      return hourlyCounts;
    }
  }

  // Generate all hours in the range
  const result: HourlyCount[] = [];
  const currentDate = new Date(startDate);

  while (currentDate < endDate) {
    // Format the hour in the expected format for the chart (YYYY-MM-DD HH:00:00)
    // Instead of ISO format with T separator
    const year = currentDate.getUTCFullYear();
    const month = String(currentDate.getUTCMonth() + 1).padStart(2, "0");
    const day = String(currentDate.getUTCDate()).padStart(2, "0");
    const hour = String(currentDate.getUTCHours()).padStart(2, "0");

    const formattedHour = `${year}-${month}-${day} ${hour}:00:00`;

    // Use either the original count or 0 if no data exists for this hour
    const isoHour = currentDate.toISOString().slice(0, 13); // For lookup in the map
    const count =
      hourToCountMap.get(isoHour) || hourToCountMap.get(formattedHour) || 0;

    result.push({
      hour: formattedHour,
      count,
    });

    // Move to next hour
    currentDate.setHours(currentDate.getHours() + 1);
  }

  return result;
}

export function useInsights(): Insight[] | undefined {
  const moduleFunctions = useModuleFunctions();
  const period = useInsightsPeriod();
  const team = useCurrentTeam();
  const deployment = useCurrentDeployment();
  const common = {
    deploymentName: deployment?.name,
    period: {
      from: period.from.split("T")[0],
      to: period.to.split("T")[0],
    },
    teamId: team?.id!,
    projectId: null,
    componentPrefix: null,
  };

  const { data: insightsData } = useUsageQuery({
    queryId: "9ab3b74e-a725-480b-88a6-43e6bd70bd82",
    ...common,
  });

  if (!insightsData) {
    return undefined;
  }

  const insights = insightsData.map((d) => {
    const parsedDetails = JSON.parse(d[3]);

    // Pad and sort hourly counts if they exist, using the period from useInsightsPeriod
    if (
      parsedDetails.hourlyCounts &&
      Array.isArray(parsedDetails.hourlyCounts)
    ) {
      parsedDetails.hourlyCounts = padAndSortHourlyData(
        parsedDetails.hourlyCounts,
        period.from,
      );
    }

    return {
      kind: d[0] as Insight["kind"],
      functionId: d[1],
      componentPath: d[2] === rootComponentPath ? null : d[2],
      details: parsedDetails,
    };
  });

  insights.sort((a, b) => {
    const order: Record<Insight["kind"], number> = {
      documentsReadLimit: 0,
      bytesReadLimit: 1,
      occFailedPermanently: 2,
      documentsReadThreshold: 3,
      bytesReadThreshold: 4,
      occRetried: 5,
    };
    return order[a.kind] - order[b.kind];
  });
  return insights.filter((insight) => {
    const id = functionIdentifierValue(
      insight.functionId,
      insight.componentPath ?? undefined,
    );
    return moduleFunctions.some((mf) => itemIdentifier(mf) === id);
  });
}

/**
 * Generates a unique page identifier for an insight for use in URL query parameters
 * @param insight The insight to generate an identifier for
 * @returns A string identifier that can be used as a query parameter
 */
export function getInsightPageIdentifier(insight: Insight): string {
  // For OCC insights, include the table name in the page identifier
  if (
    (insight.kind === "occRetried" ||
      insight.kind === "occFailedPermanently") &&
    "details" in insight &&
    "occTableName" in insight.details
  ) {
    return `insight:${insight.kind}:${insight.componentPath}:${insight.functionId}:${insight.details.occTableName}`;
  }

  // For other insights, use the standard format
  return `insight:${insight.kind}:${insight.componentPath}:${insight.functionId}`;
}
