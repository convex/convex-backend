import { Sheet } from "@ui/Sheet";
import { Tooltip } from "@ui/Tooltip";
import { Spinner } from "@ui/Spinner";
import { formatBytes, formatNumberCompact } from "@common/lib/format";
import { UsageSummaryRow } from "hooks/usageMetrics";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { formatQuantity } from "./lib/formatQuantity";
import { ReactNode, useEffect, useState } from "react";
import { captureException } from "@sentry/nextjs";
import { TeamEntitlementsResponse } from "generatedApi";

import {
  QuestionMarkCircledIcon,
  CrossCircledIcon,
  ChevronRightIcon,
} from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import Link from "next/link";
import { useRouter } from "next/router";
import { Donut } from "@ui/Donut";
import { Loading } from "@ui/Loading";
import type { UsagePricing } from "./lib/usagePrices";
import {
  getMultiPeriodUsageSpend,
  getUsageSpend,
  getIncludedUsage,
  isGaugeMetric,
  formatSpend,
} from "./lib/usageSpend";
import type { PricedUsageState, UsageSpend } from "./lib/usageSpend";

const BUSINESS_METRIC_TO_SECTION: Record<string, string> = {
  functionCalls: "functionCalls",
  compute: "compute",
  databaseStorage: "databaseStorage",
  databaseIO: "databaseIO",
  fileStorage: "filesStorage",
  searchStorage: "searchStorage",
  searchQueries: "searchQueries",
  dataEgress: "dataEgress",
  auditLogBandwidth: "auditLogBandwidth",
  aiGatewayCost: "aiGatewayCost",
  deploymentCount: "deployments",
};

const SELF_SERVE_METRIC_TO_SECTION: Record<string, string> = {
  functionCalls: "functionCalls",
  actionCompute: "actionCompute",
  databaseStorage: "databaseStorage",
  databaseIO: "databaseIO",
  fileStorage: "filesStorage",
  searchStorage: "searchStorage",
  searchQueries: "searchQueries",
  dataEgress: "dataEgress",
  auditLogBandwidth: "auditLogBandwidth",
  aiGatewayCost: "aiGatewayCost",
  deploymentCount: "deployments",
};

type BusinessMetricKey =
  | keyof Omit<UsageSummaryRow, "deploymentClass" | "region">
  | "compute"
  | "actionCompute"
  | "aiGatewayCost"
  | "deploymentCount";

type Section = {
  metric: BusinessMetricKey;
  entitlement?: {
    [K in keyof TeamEntitlementsResponse]: TeamEntitlementsResponse[K] extends number
      ? K
      : never;
  }[keyof TeamEntitlementsResponse];
  format: (value: number) => string;
  detail: string;
  title: string;
  suffix?: string;
  // The plan caps usage at the included limit instead of billing beyond it, so
  // the metric has no spend.
  noOnDemand?: boolean;
  // The plan includes none of this metric; the whole amount bills on demand.
  allOnDemand?: boolean;
};

const businessSections: Section[] = [
  {
    metric: "functionCalls",
    format: formatNumberCompact,
    detail: "The number of function calls across all deployments",
    title: "Function Calls",
  },
  {
    metric: "compute",
    format: (n: number) => formatQuantity(n, "actionCompute"),
    detail:
      "The total execution time of functions multiplied by their allocated RAM",
    title: "Compute",
  },
  {
    metric: "databaseStorage",
    format: formatBytes,
    detail:
      "The total size of all documents and indexes stored in your projects",
    title: "Database Storage",
  },
  {
    metric: "databaseIO",
    format: formatBytes,
    detail: "The amount of data read and written to the database",
    title: "Database I/O",
  },
  {
    metric: "fileStorage",
    format: formatBytes,
    detail: "The total size of all files stored in your projects",
    title: "File Storage",
  },
  {
    metric: "searchStorage",
    format: formatBytes,
    detail: "The total size of all text and vector search indexes stored",
    title: "Search Storage",
  },
  {
    metric: "searchQueries",
    format: (n: number) => formatQuantity(n, "textSearch"),
    detail: "The total query-GB of text and vector search queries",
    title: "Search Queries",
  },
  {
    metric: "dataEgress",
    format: formatBytes,
    detail:
      "The amount of data egressed via file serving, fetch requests, and log streaming",
    title: "Data Egress",
  },
  {
    metric: "auditLogBandwidth",
    format: formatBytes,
    detail:
      "The amount of audit log data egressed to your configured S3 bucket.",
    title: "Audit Log Bandwidth",
  },
  {
    metric: "aiGatewayCost",
    format: (v: number) => formatQuantity(v, "currency"),
    detail: "Spend on Convex-managed AI models through the AI gateway",
    title: "AI Gateway",
    allOnDemand: true,
  },
  {
    metric: "deploymentCount",
    format: formatNumberCompact,
    detail: "The number of deployments across all projects",
    title: "Deployments",
  },
];

const selfServeSections: Section[] = [
  {
    metric: "functionCalls",
    entitlement: "teamMaxFunctionCalls",
    format: formatNumberCompact,
    detail:
      "The number of times any query, mutation, file access or other function was called",
    title: "Function Calls",
  },
  {
    metric: "actionCompute",
    entitlement: "teamMaxActionCompute",
    format: formatNumberCompact,
    suffix: "GB-hours",
    detail:
      "The execution time of all actions multiplied by their allocated amount of RAM",
    title: "Action Compute",
  },
  {
    metric: "databaseStorage",
    entitlement: "teamMaxDatabaseStorage",
    format: formatBytes,
    detail:
      "The total size of all documents and indexes stored in your projects",
    title: "Database Storage",
  },
  {
    metric: "databaseIO",
    entitlement: "teamMaxDatabaseBandwidth",
    format: formatBytes,
    detail: "The amount of data read and written to the database",
    title: "Database I/O",
  },
  {
    metric: "fileStorage",
    entitlement: "teamMaxFileStorage",
    format: formatBytes,
    detail: "The total size of all files stored in your projects",
    title: "File Storage",
  },
  {
    metric: "dataEgress",
    entitlement: "teamMaxFileBandwidth",
    format: formatBytes,
    detail:
      "The amount of data egressed by file serving, fetch calls in actions, log streams, and streaming export.",
    title: "Data Egress",
  },
  {
    metric: "auditLogBandwidth",
    format: formatBytes,
    detail:
      "The amount of audit log data egressed to your configured S3 bucket.",
    title: "Audit Log Bandwidth",
  },
  {
    metric: "searchStorage",
    entitlement: "teamMaxVectorStorage",
    format: formatBytes,
    detail: "The total size of all text and vector search indexes stored",
    title: "Search Storage",
  },
  {
    metric: "searchQueries",
    entitlement: "teamMaxSearchQueries",
    format: (n: number) => formatQuantity(n, "textSearch"),
    detail: "The total query-GB of text and vector search queries",
    title: "Search Queries",
  },
  {
    metric: "deploymentCount",
    entitlement: "maxDeployments",
    format: formatNumberCompact,
    detail: "The current number of deployments across all projects",
    title: "Deployments",
    noOnDemand: true,
  },
  {
    metric: "aiGatewayCost",
    format: (v: number) => formatQuantity(v, "currency"),
    detail: "Spend on Convex-managed AI models through the AI gateway",
    title: "AI Gateway",
    allOnDemand: true,
  },
];

export function BusinessPlanSummary({
  summary,
  error,
  aiGatewayCost,
  aiGatewayCostError,
  isBusinessPlan = true,
  entitlements,
  hasSubscription = false,
  subscriptionLoading = false,
  showEntitlements = false,
  isCurrentBillingPeriod = true,
  pricing = { status: "unavailable", reason: "unsupported-period" },
  spendApplicable = true,
  pricedUsage = { status: "loading" },
}: {
  summary?: UsageSummaryRow[];
  error?: any;
  aiGatewayCost?: number;
  aiGatewayCostError?: any;
  isBusinessPlan?: boolean;
  entitlements?: TeamEntitlementsResponse;
  hasSubscription?: boolean;
  subscriptionLoading?: boolean;
  showEntitlements?: boolean;
  isCurrentBillingPeriod?: boolean;
  pricing?: UsagePricing;
  spendApplicable?: boolean;
  pricedUsage?: PricedUsageState;
}) {
  const router = useRouter();
  const { dashboardShowCashUsage } = useLaunchDarkly();
  const showSpend =
    dashboardShowCashUsage &&
    (pricing.status === "ready" || pricing.status === "loading");
  const activeSections = isBusinessPlan ? businessSections : selfServeSections;
  // Only show the audit log egress section for teams that can configure custom audit logs.
  const visibleSections = activeSections.filter(
    (section) =>
      section.metric !== "auditLogBandwidth" ||
      entitlements?.customAuditLogsInLogStreamsConfigEnabled === true,
  );
  const historicalIncludedUsage =
    !isCurrentBillingPeriod && spendApplicable && pricing.status === "ready"
      ? Object.fromEntries(
          visibleSections.map((section) => [
            section.metric,
            section.metric === "deploymentCount"
              ? undefined
              : getIncludedUsage(
                  pricing.periods.map(({ period }) => period),
                  section.metric,
                ),
          ]),
        )
      : undefined;
  const showHistoricalIncluded =
    historicalIncludedUsage !== undefined &&
    Object.values(historicalIncludedUsage).some(
      (usage) => usage !== undefined && usage > 0,
    );
  const showIncluded = hasSubscription || showHistoricalIncluded;
  const showIncludedLimits = showEntitlements && isCurrentBillingPeriod;

  const layoutDecided =
    !subscriptionLoading &&
    (!dashboardShowCashUsage || pricing.status !== "loading");
  const [settledColumns, setSettledColumns] = useState<{
    showIncluded: boolean;
    showSpend: boolean;
  }>();
  if (
    layoutDecided &&
    (settledColumns?.showIncluded !== showIncluded ||
      settledColumns?.showSpend !== showSpend)
  ) {
    setSettledColumns({ showIncluded, showSpend });
  }
  const columns = (!layoutDecided && settledColumns) || {
    showIncluded,
    showSpend,
  };
  const gridColumns = columns.showIncluded
    ? columns.showSpend
      ? "grid-cols-[4fr_3fr_2fr_2fr_auto] sm:grid-cols-[4fr_3fr_3fr_2fr_auto]"
      : "grid-cols-[4fr_3fr_2fr_auto] sm:grid-cols-[4fr_3fr_3fr_auto]"
    : columns.showSpend
      ? "grid-cols-[5fr_4fr_2fr_auto]"
      : "grid-cols-[5fr_4fr_auto]";

  // Deployment counts live on the summary rows but are team-wide (the gauge has
  // no project/component dimension we filter on), so sum across every row.
  const deploymentCount = summary
    ? summary.reduce((sum, row) => sum + row.deploymentCount, 0)
    : undefined;
  const pausedDeploymentCount = summary
    ? summary.reduce((sum, row) => sum + row.pausedDeploymentCount, 0)
    : undefined;
  const idleDeploymentCount = summary
    ? summary.reduce((sum, row) => sum + row.idleDeploymentCount, 0)
    : undefined;

  // Aggregate usage rows by summing each metric in activeSections.
  const aggregateRows = (rows: UsageSummaryRow[]) =>
    rows.reduce(
      (acc, row) => {
        for (const section of activeSections) {
          if (section.metric === "deploymentCount") continue;
          // Comes from its own query, not the summary rows.
          if (section.metric === "aiGatewayCost") continue;
          let value: number;
          if (section.metric === "compute") {
            value =
              row.queryMutationCompute +
              row.actionComputeUser +
              row.actionComputeNode;
          } else if (section.metric === "actionCompute") {
            value = row.actionComputeConvex + row.actionComputeNode;
          } else {
            value = row[section.metric];
          }
          acc[section.metric] = (acc[section.metric] || 0) + value;
        }
        return acc;
      },
      {} as Record<string, number>,
    );

  // Aggregate across deployment classes and regions
  const aggregated = summary ? aggregateRows(summary) : undefined;

  // Apply the team-wide deployment count to the aggregate.
  if (aggregated && deploymentCount !== undefined) {
    aggregated.deploymentCount = deploymentCount;
  }
  if (aggregated && aiGatewayCost !== undefined) {
    aggregated.aiGatewayCost = aiGatewayCost;
  }

  const sectionToRoute = isBusinessPlan
    ? BUSINESS_METRIC_TO_SECTION
    : SELF_SERVE_METRIC_TO_SECTION;

  return (
    <Sheet
      className="animate-fadeInFromLoading overflow-hidden"
      padding={false}
    >
      {/* Rows are subgrids of one shared grid, so a wide value in any row
          widens its column for every row instead of shifting just its own. */}
      <div className={cn("grid gap-x-2 gap-y-1 overflow-x-clip", gridColumns)}>
        <div className="col-span-full grid grid-cols-subgrid items-center rounded-t border-b px-4 py-2 text-sm text-content-secondary">
          <div>Resource</div>
          <div>
            {columns.showIncluded ? (
              <div className="flex items-center gap-1">
                Included{" "}
                <Tooltip
                  tip="The amount of usage used within the included limits of your plan."
                  side="right"
                  className="hidden sm:block"
                >
                  <QuestionMarkCircledIcon />
                </Tooltip>
              </div>
            ) : (
              "Usage"
            )}
          </div>
          {columns.showIncluded && (
            <div className="flex items-center gap-1">
              On-demand{" "}
              <Tooltip
                tip="Usage beyond your plan's included limits."
                side="right"
                className="hidden sm:block"
              >
                <QuestionMarkCircledIcon />
              </Tooltip>
            </div>
          )}
          {columns.showSpend && (
            <div className="flex items-center gap-1">
              Spend
              <Tooltip
                tip="Usage is rounded up to the nearest whole unit before calculating spend. Storage and deployment metrics show estimated monthly prices based on the most recent usage in the selected period."
                side="right"
              >
                <QuestionMarkCircledIcon />
              </Tooltip>
            </div>
          )}
          <span className="invisible flex items-center gap-1 text-xs">
            <span className="hidden whitespace-nowrap sm:inline">
              View breakdown by day
            </span>
            <ChevronRightIcon className="size-4" />
          </span>
        </div>
        {error ? (
          <PlanSummaryError />
        ) : !aggregated || !layoutDecided ? (
          <PlanSummaryLoading />
        ) : (
          visibleSections.map((section, index) => {
            const sectionId = sectionToRoute[section.metric];
            const { section: _s, tab: _t, ...restQuery } = router.query;
            const linkQuery = sectionId
              ? { ...restQuery, section: sectionId }
              : restQuery;
            const linkHref = { pathname: router.pathname, query: linkQuery };

            const metric = aggregated[section.metric] ?? 0;
            const aiCostFailed =
              section.metric === "aiGatewayCost" &&
              aiGatewayCostError !== undefined;
            const aiCostPending =
              section.metric === "aiGatewayCost" &&
              aiGatewayCost === undefined &&
              !aiCostFailed;
            const entitlement =
              section.entitlement && entitlements
                ? entitlements[section.entitlement]
                : undefined;

            const includedAmount = section.allOnDemand
              ? 0
              : showHistoricalIncluded
                ? historicalIncludedUsage?.[section.metric]
                : entitlement !== undefined
                  ? Math.min(metric, entitlement)
                  : undefined;
            const onDemandAmount =
              metric !== undefined && includedAmount !== undefined
                ? Math.max(0, metric - includedAmount)
                : undefined;

            const displayedUsage =
              showIncluded &&
              (showHistoricalIncluded || !section.noOnDemand) &&
              includedAmount !== undefined
                ? includedAmount
                : metric;

            const isGauge = isGaugeMetric(section.metric);
            const pricedSummaryFailed =
              !isGauge &&
              section.metric !== "aiGatewayCost" &&
              pricedUsage.status === "error";
            const pricedSummaryPending =
              !isGauge &&
              section.metric !== "aiGatewayCost" &&
              pricedUsage.status === "loading";

            const spend =
              showSpend && pricing.status === "ready" && spendApplicable
                ? section.metric === "aiGatewayCost"
                  ? aiCostFailed || aiGatewayCost === undefined
                    ? undefined
                    : {
                        amount: aiGatewayCost,
                        includedAmount: 0,
                        missingDimensions: [],
                      }
                  : isGauge
                    ? getUsageSpend(
                        summary,
                        section.metric,
                        pricing.periods.at(-1)?.period,
                      )
                    : pricedUsage.status === "ready"
                      ? getMultiPeriodUsageSpend(
                          pricedUsage.summaries,
                          section.metric,
                        )
                      : undefined
                : undefined;

            return (
              <div
                key={index}
                className="group relative col-span-full grid min-h-10 grid-cols-subgrid items-center rounded-sm px-4 py-2 text-left transition-colors focus-within:bg-background-primary hover:bg-background-primary"
              >
                <div className="flex items-center gap-2">
                  {showIncludedLimits &&
                    (entitlement !== undefined ? (
                      <Tooltip
                        aria-label={`${section.title} included usage`}
                        side="bottom"
                        tip={`Your team has used ${(100 * (displayedUsage / entitlement)).toFixed(2)}% of the included amount of ${section.title}.`}
                        className="relative z-10 flex animate-fadeInFromLoading items-center rounded-sm focus-visible:ring-2 focus-visible:ring-border-selected"
                      >
                        <Donut current={displayedUsage} max={entitlement} />
                      </Tooltip>
                    ) : (
                      // Keep sections without an included limit aligned with
                      // the gauged ones.
                      <div className="hidden size-6 sm:block" />
                    ))}
                  <SectionLabel title={section.title} detail={section.detail} />
                </div>
                <div className="animate-fadeInFromLoading">
                  {showIncluded &&
                  (section.allOnDemand ||
                    (showHistoricalIncluded &&
                      includedAmount === undefined &&
                      section.metric !== "deploymentCount")) ? (
                    <EmptyValue />
                  ) : aiCostFailed ? (
                    <span className="text-content-secondary">Unavailable</span>
                  ) : aiCostPending ? (
                    <Loading fullHeight={false} className="h-4 w-16" />
                  ) : showIncluded &&
                    displayedUsage === 0 &&
                    !(showIncludedLimits && entitlement !== undefined) ? (
                    // "0 / limit" reads fine next to a cap, but a bare zero
                    // looks like missing data.
                    <EmptyValue />
                  ) : (
                    <span>
                      {section.format(displayedUsage)}
                      {section.suffix &&
                        (!showIncludedLimits ? ` ${section.suffix}` : "")}
                    </span>
                  )}
                  {showIncludedLimits && entitlement !== undefined && (
                    <span>
                      {" "}
                      / {section.format(entitlement)}
                      {section.suffix ? ` ${section.suffix}` : ""}
                    </span>
                  )}
                  {section.metric === "deploymentCount" &&
                    isBusinessPlan &&
                    pausedDeploymentCount !== undefined &&
                    idleDeploymentCount !== undefined && (
                      <span className="text-content-secondary">
                        {" "}
                        (
                        {formatNumberCompact(
                          Math.max(
                            displayedUsage -
                              pausedDeploymentCount -
                              idleDeploymentCount,
                            0,
                          ),
                        )}{" "}
                        active · {formatNumberCompact(idleDeploymentCount)} idle
                        · {formatNumberCompact(pausedDeploymentCount)} paused)
                      </span>
                    )}
                </div>
                {showIncluded && (
                  <div className="animate-fadeInFromLoading">
                    {section.metric === "deploymentCount" ? (
                      <EmptyValue />
                    ) : section.allOnDemand ? (
                      aiCostFailed ? (
                        <span className="text-content-secondary">
                          Unavailable
                        </span>
                      ) : aiCostPending ? (
                        <Loading fullHeight={false} className="h-4 w-16" />
                      ) : (
                        // The whole amount bills on demand, so it is not an
                        // increment over an included amount.
                        `${section.format(metric)}${section.suffix ? ` ${section.suffix}` : ""}`
                      )
                    ) : section.noOnDemand ||
                      onDemandAmount === undefined ? null : onDemandAmount ===
                      0 ? (
                      <EmptyValue />
                    ) : (
                      `+${section.format(onDemandAmount)}${section.suffix ? ` ${section.suffix}` : ""}`
                    )}
                  </div>
                )}
                {showSpend && (
                  <div className="animate-fadeInFromLoading tabular-nums">
                    {!spendApplicable || section.noOnDemand ? (
                      <EmptyValue />
                    ) : aiCostFailed || pricedSummaryFailed ? (
                      <span className="text-content-secondary">
                        Unavailable
                      </span>
                    ) : pricing.status === "loading" ||
                      aiCostPending ||
                      pricedSummaryPending ? (
                      <Loading fullHeight={false} className="h-4 w-16" />
                    ) : (
                      <SpendValue
                        spend={spend}
                        sectionTitle={section.title}
                        estimatedMonthly={isGauge}
                        reportUnavailable={
                          pricing.status === "ready" &&
                          section.metric !== "aiGatewayCost"
                        }
                      />
                    )}
                  </div>
                )}
                <Link
                  href={linkHref}
                  shallow
                  aria-label={`View ${section.title} breakdown by day`}
                  className="flex items-center gap-1 text-xs text-content-secondary group-hover:text-content-primary after:absolute after:inset-0 after:rounded-sm focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-border-selected"
                >
                  <span className="hidden whitespace-nowrap opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 sm:inline">
                    View breakdown by day
                  </span>
                  <ChevronRightIcon className="size-4" />
                </Link>
              </div>
            );
          })
        )}
      </div>
    </Sheet>
  );
}

// Fills cells with no value to show (zero, not applicable, or missing
// billing information) so the table doesn't read as missing data.
function EmptyValue() {
  return <span className="text-content-secondary">—</span>;
}

function SpendValue({
  spend,
  sectionTitle,
  reportUnavailable,
  estimatedMonthly,
}: {
  spend: UsageSpend | undefined;
  sectionTitle: string;
  reportUnavailable: boolean;
  estimatedMonthly: boolean;
}) {
  const missingDimensions = spend?.missingDimensions.join(", ");
  const missingBillingInformation =
    spend === undefined
      ? reportUnavailable
      : spend.missingDimensions.length > 0;

  useEffect(() => {
    if (missingBillingInformation) {
      captureException(
        new Error(
          `Spend unavailable: missing billing information for ${sectionTitle}`,
        ),
        { extra: { sectionTitle, missingDimensions } },
      );
    }
  }, [missingBillingInformation, sectionTitle, missingDimensions]);

  if (spend === undefined || spend.missingDimensions.length > 0) {
    return <EmptyValue />;
  }

  return (
    <span className="whitespace-nowrap">
      {formatSpend(Math.max(0, spend.amount - spend.includedAmount))}
      {estimatedMonthly && (
        <span className="ml-1 text-content-secondary">/ month</span>
      )}
    </span>
  );
}

function PlanSummaryError() {
  return (
    <div className="col-span-full flex h-56 flex-col items-center justify-center p-4 text-center">
      <CrossCircledIcon className="size-6 text-content-error" />
      <h5 className="mt-2">Error fetching Usage summary data</h5>
      <p className="mt-1 text-sm text-content-secondary">
        An error occurred while fetching usage summary data. Please try again
        later.
      </p>
    </div>
  );
}

function PlanSummaryLoading() {
  return (
    <div className="col-span-full flex h-100 items-center justify-center p-4">
      <div className="flex items-center justify-center">
        <Spinner className="size-12" />
      </div>
    </div>
  );
}

function SectionLabel({
  detail,
  title,
}: {
  detail?: ReactNode;
  title: string;
}) {
  return (
    <p className="flex animate-fadeInFromLoading items-center text-sm">
      {title}
      {detail !== null && detail !== undefined ? (
        <Tooltip
          aria-label={`${title} details`}
          tip={detail}
          side="right"
          className="relative z-10 hidden rounded-sm focus-visible:ring-2 focus-visible:ring-border-selected sm:block"
        >
          <QuestionMarkCircledIcon className="ml-1" />
        </Tooltip>
      ) : null}
    </p>
  );
}
