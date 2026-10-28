import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { Tooltip } from "@ui/Tooltip";
import { Spinner } from "@ui/Spinner";
import { formatBytes, formatNumberCompact } from "@common/lib/format";
import { UsageSummaryRow } from "hooks/usageMetrics";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { formatQuantity } from "./lib/formatQuantity";
import { ReactNode } from "react";
import { TeamEntitlementsResponse } from "generatedApi";

import {
  QuestionMarkCircledIcon,
  CrossCircledIcon,
  ChevronRightIcon,
  ExclamationTriangleIcon,
} from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import { useRouter } from "next/router";
import { Donut } from "@ui/Donut";
import { Loading } from "@ui/Loading";
import { formatDistanceToNowStrict } from "date-fns";
import type { UsagePricing } from "./lib/usagePrices";
import { getUsageSpend, formatSpend } from "./lib/usageSpend";
import type { UsageSpend } from "./lib/usageSpend";

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
  showEntitlements = false,
  pricing = { status: "unavailable", reason: "unsupported-period" },
  spendApplicable = true,
}: {
  summary?: UsageSummaryRow[];
  error?: any;
  aiGatewayCost?: number;
  aiGatewayCostError?: any;
  isBusinessPlan?: boolean;
  entitlements?: TeamEntitlementsResponse;
  hasSubscription?: boolean;
  showEntitlements?: boolean;
  pricing?: UsagePricing;
  spendApplicable?: boolean;
}) {
  const router = useRouter();
  const { dashboardShowCashUsage } = useLaunchDarkly();
  const showSpend = dashboardShowCashUsage && pricing.status !== "unavailable";
  const gridColumns = hasSubscription
    ? showSpend
      ? "grid-cols-[4fr_3fr_2fr_2fr_auto] sm:grid-cols-[4fr_3fr_3fr_2fr_auto]"
      : "grid-cols-[4fr_3fr_2fr_auto] sm:grid-cols-[4fr_3fr_3fr_auto]"
    : showSpend
      ? "grid-cols-[5fr_4fr_2fr_auto]"
      : "grid-cols-[5fr_4fr_auto]";
  const activeSections = isBusinessPlan ? businessSections : selfServeSections;

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

  // Only show the audit log egress section for teams that can configure custom audit logs.
  const visibleSections = activeSections.filter(
    (section) =>
      section.metric !== "auditLogBandwidth" ||
      entitlements?.customAuditLogsInLogStreamsConfigEnabled === true,
  );

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
      <div className="flex flex-col gap-1 overflow-x-clip">
        <div
          className={cn(
            "grid items-center gap-2 rounded-t border-b px-4 py-2 text-sm text-content-secondary",
            gridColumns,
          )}
        >
          <div>Resource</div>
          <div>
            {hasSubscription ? (
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
          {hasSubscription && (
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
          {showSpend && (
            <span className="flex items-center gap-1">
              Spend
              {pricing.status === "ready" && (
                <Tooltip
                  tip={`Spend is only available for time ranges starting after the previous change to prices (${formatDistanceToNowStrict(pricing.period.start, { addSuffix: true })}).`}
                >
                  <QuestionMarkCircledIcon />
                </Tooltip>
              )}
            </span>
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
        ) : !aggregated ? (
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
              : entitlement !== undefined
                ? Math.min(metric, entitlement)
                : undefined;
            const onDemandAmount =
              metric !== undefined && includedAmount !== undefined
                ? metric - includedAmount
                : undefined;

            const displayedUsage =
              hasSubscription &&
              !section.noOnDemand &&
              includedAmount !== undefined
                ? includedAmount
                : metric;

            const spend =
              showSpend && pricing.status === "ready" && spendApplicable
                ? section.metric === "aiGatewayCost"
                  ? aiCostFailed || aiGatewayCost === undefined
                    ? undefined
                    : { amount: aiGatewayCost, missingDimensions: [] }
                  : getUsageSpend(summary, section.metric, pricing.period)
                : undefined;

            return (
              <Button
                key={index}
                variant="unstyled"
                onClick={() => {
                  void router.push(linkHref, undefined, { shallow: true });
                }}
                className={cn(
                  "group grid min-h-10 items-center gap-2 rounded-sm px-4 py-2 text-left transition-colors hover:bg-background-primary focus-visible:bg-background-primary focus-visible:outline-2 focus-visible:outline-border-selected",
                  gridColumns,
                )}
              >
                <div className="flex items-center gap-2">
                  {showEntitlements &&
                    (entitlement !== undefined ? (
                      <Tooltip
                        side="bottom"
                        tip={`Your team has used ${(100 * (displayedUsage / entitlement)).toFixed(2)}% of the included amount of ${section.title}.`}
                        className="flex animate-fadeInFromLoading items-center"
                      >
                        <Donut current={displayedUsage} max={entitlement} />
                      </Tooltip>
                    ) : (
                      // Keep sections without an included limit aligned with
                      // the gauged ones.
                      <div className="hidden size-6 sm:block" />
                    ))}
                  <SectionLabel detail={section.detail}>
                    {section.title}
                  </SectionLabel>
                </div>
                <div className="animate-fadeInFromLoading">
                  {hasSubscription && section.allOnDemand ? (
                    <span className="text-content-secondary">–</span>
                  ) : aiCostFailed ? (
                    <span className="text-content-secondary">Unavailable</span>
                  ) : aiCostPending ? (
                    <Loading fullHeight={false} className="h-4 w-16" />
                  ) : (
                    <span>
                      {section.format(displayedUsage)}
                      {section.suffix &&
                        (!showEntitlements ? ` ${section.suffix}` : "")}
                    </span>
                  )}
                  {showEntitlements && entitlement !== undefined && (
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
                {hasSubscription && (
                  <div className="animate-fadeInFromLoading">
                    {section.allOnDemand ? (
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
                    ) : (
                      !section.noOnDemand &&
                      onDemandAmount !== undefined &&
                      onDemandAmount > 0 &&
                      `+${section.format(onDemandAmount)}${section.suffix ? ` ${section.suffix}` : ""}`
                    )}
                  </div>
                )}
                {showSpend && (
                  <div className="animate-fadeInFromLoading tabular-nums">
                    {!spendApplicable ||
                    section.metric === "deploymentCount" ? (
                      <span className="text-content-secondary">–</span>
                    ) : pricing.status === "error" || aiCostFailed ? (
                      <span className="text-content-secondary">
                        Unavailable
                      </span>
                    ) : pricing.status === "loading" || aiCostPending ? (
                      <Loading fullHeight={false} className="h-4 w-16" />
                    ) : (
                      <SpendValue
                        spend={spend}
                        sectionTitle={section.title}
                        showUnavailableWarning={
                          pricing.status === "ready" &&
                          section.metric !== "aiGatewayCost"
                        }
                      />
                    )}
                  </div>
                )}
                <span className="flex items-center gap-1 text-xs text-content-secondary">
                  <span className="hidden whitespace-nowrap opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100 sm:inline">
                    View breakdown by day
                  </span>
                  <ChevronRightIcon className="size-4" />
                </span>
              </Button>
            );
          })
        )}
      </div>
    </Sheet>
  );
}

function SpendValue({
  spend,
  sectionTitle,
  showUnavailableWarning,
}: {
  spend: UsageSpend | undefined;
  sectionTitle: string;
  showUnavailableWarning: boolean;
}) {
  return (
    <span className="inline-flex items-center gap-1 whitespace-nowrap">
      {spend === undefined || spend.missingDimensions.length > 0 ? (
        <span className="text-content-secondary">–</span>
      ) : (
        formatSpend(spend.amount)
      )}
      {spend === undefined
        ? showUnavailableWarning && <SpendWarning sectionTitle={sectionTitle} />
        : spend.missingDimensions.length > 0 && (
            <SpendWarning
              sectionTitle={sectionTitle}
              missingDimensions={spend.missingDimensions}
            />
          )}
    </span>
  );
}

function SpendWarning({
  sectionTitle,
  missingDimensions,
}: {
  sectionTitle: string;
  missingDimensions?: string[];
}) {
  return (
    <Tooltip
      aria-label={
        missingDimensions
          ? "Incomplete billing information"
          : "Missing billing information"
      }
      className="inline-flex shrink-0 items-center text-content-warning"
      tip={
        missingDimensions ? (
          <div className="space-y-2 p-1 text-left">
            <p>
              Spend unavailable: billing information is missing for some
              metrics, deployment classes, or regions.
            </p>
            <div>
              <p className="mb-1 font-semibold">Missing billing information</p>
              <ul className="list-none space-y-1 text-content-secondary">
                {missingDimensions.map((dimension) => (
                  <li key={dimension}>{dimension}</li>
                ))}
              </ul>
            </div>
          </div>
        ) : (
          <p className="p-1 text-left">
            Spend unavailable: {sectionTitle} spend cannot be calculated from
            the available billing information.
          </p>
        )
      }
    >
      <ExclamationTriangleIcon className="size-4" />
    </Tooltip>
  );
}

function PlanSummaryError() {
  return (
    <div className="flex h-56 flex-col items-center justify-center p-4 text-center">
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
    <div className="flex h-100 items-center justify-center p-4">
      <div className="flex items-center justify-center">
        <Spinner className="size-12" />
      </div>
    </div>
  );
}

function SectionLabel({
  detail,
  children,
}: {
  detail?: ReactNode;
  children: ReactNode;
}) {
  return (
    <p className="flex animate-fadeInFromLoading items-center text-sm">
      {children}
      {detail !== null && detail !== undefined ? (
        <Tooltip tip={detail} side="right" className="hidden sm:block">
          <QuestionMarkCircledIcon className="ml-1" />
        </Tooltip>
      ) : null}
    </p>
  );
}
