import { BusinessPlanSummary } from "components/billing/PlanSummary";
import { Sheet } from "@ui/Sheet";
import { Spinner } from "@ui/Spinner";
import { Button } from "@ui/Button";
import { SegmentedControl } from "@ui/SegmentedControl";
import { TeamResponse } from "generatedApi";
import { useEffect, useMemo, useState } from "react";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useDeployments } from "api/deployments";
import { useHasCustomRolePermission } from "api/roles";
import { useTeamEntitlements } from "api/teams";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { TEAM_RESOURCE } from "lib/permissions";
import { useProjectById, useProjectBySlug } from "api/projects";
import { useTeamOrbSubscription, useTeamPrices } from "api/billing";
import { getUsagePricing } from "./lib/usagePrices";
import groupBy from "lodash/groupBy";
import sumBy from "lodash/sumBy";
import { Period } from "elements/UsagePeriodSelector";
import { useRouter } from "next/router";
import { Link } from "@ui/Link";
import { ExternalLinkIcon } from "@radix-ui/react-icons";
import { DateRange, useCurrentBillingPeriod } from "api/usage";
import { cn } from "@ui/cn";
import { usePagination } from "hooks/usePagination";
import { PaginationControls } from "elements/PaginationControls";
import { useProfile } from "api/profile";
import { formatQuantity } from "./lib/formatQuantity";
import {
  DATABASE_STORAGE_CATEGORIES,
  CATEGORY_RENAMES,
  TAG_CATEGORIES,
  DATA_EGRESS_CATEGORIES,
  DATA_EGRESS_CATEGORY_RENAMES,
  COMPUTE_CATEGORIES_SELF_SERVE,
  SEARCH_STORAGE_CATEGORIES,
  SEARCH_QUERIES_CATEGORIES,
  DATABASE_IO_CATEGORIES,
  COMPUTE_CATEGORIES,
  DEPLOYMENT_CLASS_CATEGORIES,
  DEPLOYMENT_STATUS_CATEGORIES,
} from "./lib/teamUsageCategories";
import {
  FunctionBreakdownMetric,
  FunctionMetricsRow,
  FunctionBreakdownMetricCalls,
  FunctionBreakdownMetricDatabaseIO,
  FunctionBreakdownMetricCompute,
  FunctionBreakdownMetricSearch,
  FunctionBreakdownMetricDataEgress,
  FunctionBreakdownMetricAiGateway,
  TeamUsageByFunctionChart,
} from "./TeamUsageByFunctionChart";
import { UsageBarChart, UsageStackedBarChart } from "./UsageBarChart";
import { UsageByProjectChart } from "./UsageByProjectChart";
import { UsageByTableChart } from "./UsageByTableChart";
import {
  UsageChartUnavailable,
  UsageNoDataError,
  UsageDataError,
} from "./TeamUsageError";
import { TeamUsageToolbar } from "./TeamUsageToolbar";
import {
  GroupBy,
  BusinessGroupBy,
  BusinessDatabaseGroupBy,
  DeploymentGroupBy,
  GroupBySelector,
  GROUP_BY_OPTIONS,
  DATABASE_GROUP_BY_OPTIONS,
  BUSINESS_GROUP_BY_OPTIONS,
  BUSINESS_DATABASE_GROUP_BY_OPTIONS,
  DEPLOYMENT_GROUP_BY_OPTIONS,
  AI_GATEWAY_GROUP_BY_OPTIONS,
  AiGatewayGroupBy,
} from "./GroupBySelector";
import { ProjectLink } from "./ProjectLink";
import {
  useUsageTeamSummary,
  useUsageTeamMetricsByFunction,
  useDatabaseStoragePerDayByProjectAndClass,
  useDatabaseStoragePerDayByTable,
  useDocumentCountPerDayByTable,
  useDatabaseIOPerDayByProjectAndClass,
  useFunctionCallsPerDayByProjectAndClass,
  useComputePerDayByProject,
  useFileStoragePerDayByProject,
  useSearchStoragePerDayByProject,
  useDataEgressPerDayByProject,
  useAuditLogBandwidthPerDayByProject,
  useAiGatewayCostPerDayByModel,
  useAiGatewayCostPerDayByProject,
  useSearchQueriesPerDayByProject,
  useDeploymentsByClassAndRegion,
  useComputePerDayByProjectSelfServe,
  useUsageTeamDocumentsPerDayByProject,
  useUsageTeamDeploymentCountPerDayByProject,
  useUsageTeamDeploymentCountByType,
  useUsageTeamDeploymentCountByStatus,
  DailyMetric,
  DailyMetricByProject,
  DailyPerTagMetrics,
  DailyPerTagMetricsByProject,
  DailyPerTagMetricsByProjectAndClass,
} from "hooks/usageMetrics";

// Status breakdown data only exists starting on this date. Earlier days are
// rendered as empty bars, never with real (nonexistent) values.
const DEPLOYMENT_STATUS_DATA_START = "2026-07-23";

// Matches the `LIMIT` on the function breakdown Databricks query, which returns
// only the highest-usage projects to keep its result under the 25MB inline cap.
const MAX_PROJECTS_IN_BREAKDOWN = 250;

const FUNCTION_BREAKDOWN_TABS_ = [
  FunctionBreakdownMetricCalls,
  FunctionBreakdownMetricDatabaseIO,
  FunctionBreakdownMetricCompute,
  FunctionBreakdownMetricSearch,
  FunctionBreakdownMetricDataEgress,
  FunctionBreakdownMetricAiGateway,
];

export type UsageSectionId =
  | "functionCalls"
  | "actionCompute"
  | "databaseStorage"
  | "filesStorage"
  | "deployments"
  // Business plan sections
  | "compute"
  | "databaseIO"
  | "searchStorage"
  | "searchQueries"
  | "dataEgress"
  | "auditLogBandwidth"
  | "aiGatewayCost";

export function TeamUsage({ team }: { team: TeamResponse }) {
  const canViewUsage = useHasCustomRolePermission(
    team.id,
    "team:usage:view",
    TEAM_RESOURCE,
    true,
  );

  if (canViewUsage === false) {
    return (
      <>
        <h2>Usage</h2>
        <NoPermissionMessage
          message="You do not have permission to view team usage."
          missingPermission="team:usage:view"
        />
      </>
    );
  }

  return <TeamUsageContents team={team} />;
}

function TeamUsageContents({ team }: { team: TeamResponse }) {
  const router = useRouter();
  const { query } = router;
  const project = useProjectBySlug(team.id, query.projectSlug as string);
  const projectId = project?.id ?? null;

  const componentPrefix = (query.componentPrefix ?? null) as string | null;

  const section = (query.section as UsageSectionId) || null;

  const SECTION_TITLES: Record<UsageSectionId, string> = {
    functionCalls: "Function Calls",
    actionCompute: "Action Compute",
    databaseStorage: "Database Storage",
    filesStorage: "File Storage",
    deployments: "Deployments",
    compute: "Compute",
    databaseIO: "Database I/O",
    searchStorage: "Search Storage",
    searchQueries: "Search Queries",
    dataEgress: "Data Egress",
    auditLogBandwidth: "Audit Log Bandwidth",
    aiGatewayCost: "AI Gateway",
  };

  const summaryHref = (() => {
    const { section: _s, tab: _t, ...restQuery } = query;
    return { pathname: router.pathname, query: restQuery };
  })();

  const [selectedBillingPeriod, setSelectedBillingPeriod] =
    useState<Period | null>(null);
  const currentBillingPeriod = useCurrentBillingPeriod(team.id);
  const shownBillingPeriod =
    selectedBillingPeriod === null && currentBillingPeriod !== undefined
      ? ({
          type: "currentBillingPeriod",
          from: currentBillingPeriod.start,
          to: currentBillingPeriod.end,
        } as const)
      : selectedBillingPeriod;

  const dateRange =
    shownBillingPeriod !== null &&
    shownBillingPeriod.type !== "currentBillingPeriod"
      ? { from: shownBillingPeriod.from, to: shownBillingPeriod.to }
      : null;

  const { subscription } = useTeamOrbSubscription(team?.id);
  const prices = useTeamPrices(team.id);
  const pricing = getUsagePricing(prices, shownBillingPeriod);

  const isBusinessPlanType = subscription?.plan.planType === "CONVEX_BUSINESS";

  const billingPeriodRange = shownBillingPeriod
    ? { from: shownBillingPeriod.from, to: shownBillingPeriod.to }
    : null;

  const { data: aiGatewayCostByDay, error: aiGatewayCostError } =
    useAiGatewayCostPerDayByProject(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );
  // TODO: consolidate into one query. The (v2) Summary query has no AI
  // column, so the nav card totals the per-day rows this section's own query
  // already returns. Adding the column to the Summary query in
  // databricks-workbooks (like Function Breakdown gained one in #35) would let
  // this card read AI cost like every other metric and delete this reduce.
  const aiGatewayCost = aiGatewayCostByDay?.reduce(
    (sum, row) => sum + row.value,
    0,
  );

  const { data: summary, error: summaryError } = useUsageTeamSummary(
    team?.id,
    billingPeriodRange,
    projectId,
    componentPrefix,
  );

  const entitlements = useTeamEntitlements(team?.id);

  const hasOrbSubscription = useHasSubscription(team?.id);

  const hasSubscription =
    (!shownBillingPeriod ||
      shownBillingPeriod.type === "currentBillingPeriod") &&
    (hasOrbSubscription || hasOrbSubscription === undefined) &&
    projectId === null &&
    !isBusinessPlanType;

  const showEntitlements =
    (!shownBillingPeriod ||
      shownBillingPeriod.type === "currentBillingPeriod") &&
    projectId === null &&
    !isBusinessPlanType;

  return (
    <div className="flex min-w-160 flex-col gap-2 [--team-usage-toolbar-height:--spacing(32)] md:[--team-usage-toolbar-height:--spacing(28)] lg:[--team-usage-toolbar-height:--spacing(20)]">
      <div className="flex justify-between">
        <h2 className="flex items-center gap-2">
          {section ? (
            <>
              <Link href={summaryHref}>Usage</Link>
              <span className="animate-fadeInFromLoading">/</span>
              <span className="animate-fadeInFromLoading">
                {SECTION_TITLES[section]}
              </span>
            </>
          ) : (
            "Usage"
          )}
        </h2>
        {subscription !== undefined && (
          <Button
            href={`/t/${team?.slug}/settings/billing`}
            variant="neutral"
            icon={<ExternalLinkIcon />}
            className="animate-fadeInFromLoading"
            size="xs"
          >
            {subscription
              ? team.managedBy === "vercel"
                ? "View Subscription"
                : "View Subscription & Invoices"
              : "Upgrade Subscription"}
          </Button>
        )}
      </div>

      {currentBillingPeriod !== undefined && shownBillingPeriod !== null && (
        <>
          <TeamUsageToolbar
            {...{
              shownBillingPeriod,
              setSelectedBillingPeriod,
              currentBillingPeriod,
              teamId: team.id,
              projectId,
              selectedProject: project ?? null,
            }}
          />

          <div className="overflow-x-clip">
            <div
              className={cn(
                "flex gap-6 transition-transform duration-500 motion-reduce:transition-none",
                section ? "-translate-x-[calc(100%+1.5rem)]" : "translate-x-0",
              )}
            >
              {/* Overview pane */}
              <div
                className={cn(
                  "flex w-full shrink-0 flex-col gap-6",
                  section &&
                    "pointer-events-none h-0 overflow-hidden select-none",
                )}
                // @ts-expect-error https://github.com/facebook/react/issues/17157
                inert={section ? "inert" : undefined}
              >
                <BusinessPlanSummary
                  summary={summary}
                  error={summaryError}
                  pricing={pricing}
                  spendApplicable={
                    !query.projectSlug && projectId === null && !componentPrefix
                  }
                  aiGatewayCost={aiGatewayCost}
                  aiGatewayCostError={aiGatewayCostError}
                  isBusinessPlan={isBusinessPlanType}
                  entitlements={entitlements}
                  hasSubscription={hasSubscription}
                  showEntitlements={showEntitlements}
                />
                <FunctionBreakdownSection
                  team={team}
                  dateRange={dateRange}
                  projectId={projectId}
                  componentPrefix={componentPrefix}
                  shownBillingPeriod={shownBillingPeriod}
                />
              </div>

              {/* Detail pane */}
              <div
                className={cn(
                  "flex w-full shrink-0 flex-col gap-6",
                  !section &&
                    "pointer-events-none h-0 overflow-hidden select-none",
                )}
                // @ts-expect-error https://github.com/facebook/react/issues/17157
                inert={!section ? "inert" : undefined}
              >
                {section === "functionCalls" && (
                  <FunctionCallsUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {/* Self-serve action compute uses the same section ID */}
                {section === "actionCompute" && (
                  <ComputeUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                    isBusinessPlan={false}
                  />
                )}

                {section === "databaseStorage" && (
                  <DatabaseStorageUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "filesStorage" && (
                  <FileStorageUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "deployments" && (
                  <DeploymentCountUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                    shownBillingPeriod={shownBillingPeriod}
                  />
                )}

                {section === "compute" && (
                  <ComputeUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                    isBusinessPlan={true}
                  />
                )}

                {section === "databaseIO" && (
                  <DatabaseIOUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "searchStorage" && (
                  <SearchStorageUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "searchQueries" && (
                  <SearchQueriesUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "dataEgress" && (
                  <DataEgressUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "auditLogBandwidth" && (
                  <AuditLogBandwidthUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}

                {section === "aiGatewayCost" && (
                  <AiGatewayCostUsage
                    team={team}
                    dateRange={dateRange}
                    projectId={projectId}
                    componentPrefix={componentPrefix}
                  />
                )}
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

type UsageInProject = {
  key: string;
  projectId: number;
  rows: FunctionMetricsRow[];
  total: number;
};

function useUsageByProject(
  callsByDeployment: FunctionMetricsRow[] | undefined,
  metric: FunctionBreakdownMetric,
): UsageInProject[] | undefined {
  return useMemo(() => {
    if (callsByDeployment === undefined) {
      return undefined;
    }

    const byProject = groupBy(callsByDeployment, (row) => row.projectId);
    return Object.entries(byProject)
      .map(
        ([projectId, rows]): UsageInProject => ({
          key: projectId,
          projectId: rows[0].projectId,
          rows,
          total: sumBy(rows, metric.getTotal),
        }),
      )
      .filter((project) => project.total > 0) // Ignore projects with no data for this metric
      .sort((a, b) => b.total - a.total);
  }, [callsByDeployment, metric]);
}

function ChartLoading() {
  return (
    <div className="flex h-56 w-full items-center justify-center">
      <div className="flex items-center justify-center">
        <Spinner className="size-12" />
      </div>
    </div>
  );
}

function FunctionUsageBreakdown({
  usageByProject,
  team,
  metricsByDeployment,
  metric,
}: {
  usageByProject: UsageInProject[];
  metricsByDeployment: FunctionMetricsRow[];
  metric: FunctionBreakdownMetric;
  team: TeamResponse;
}) {
  const maxValue = useMemo(
    () =>
      metricsByDeployment.reduce(
        (max, row) => Math.max(max, metric.getTotal(row)),
        0,
      ),
    [metricsByDeployment, metric],
  );

  if (usageByProject.length === 0) {
    return <UsageNoDataError />;
  }

  if (maxValue === 0) {
    return <UsageNoDataError />;
  }

  return (
    <div className="scrollbar animate-fadeInFromLoading overflow-y-auto">
      {usageByProject.map(({ key, projectId, rows, total }) => (
        <FunctionUsageBreakdownByProject
          key={key}
          projectId={projectId}
          metric={metric}
          rows={rows}
          projectTotal={total}
          maxValue={maxValue}
          team={team}
        />
      ))}
    </div>
  );
}

function FunctionUsageBreakdownByProject({
  projectId,
  metric,
  rows,
  maxValue,
  team,
  projectTotal,
}: {
  projectId: number;
  metric: FunctionBreakdownMetric;
  rows: FunctionMetricsRow[];
  team: TeamResponse;
  maxValue: number;
  projectTotal: number;
}) {
  const { project, isLoading: isLoadingProject } = useProjectById(projectId);
  const { deployments, isLoading: isLoadingDeployments } =
    useDeployments(projectId);
  const member = useProfile();

  return (
    <div className="mb-4">
      <p className="flex align-baseline">
        <ProjectLink
          project={project ?? null}
          team={team}
          memberId={member?.id}
          isLoading={isLoadingProject}
        />
        <span className="flex-1 px-4 py-2 text-right tabular-nums">
          {formatQuantity(projectTotal, metric.quantityType)}
        </span>
      </p>

      {isLoadingDeployments && <ChartLoading />}
      {!isLoadingDeployments && (
        <TeamUsageByFunctionChart
          project={project ?? null}
          deployments={deployments ?? []}
          metric={metric}
          rows={rows}
          team={team}
          maxValue={maxValue}
        />
      )}
    </div>
  );
}

function DeploymentCountUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
  shownBillingPeriod,
}: DetailSectionProps & { shownBillingPeriod: Period }) {
  const [storedViewMode, setViewMode] =
    useGlobalLocalStorage<DeploymentGroupBy>(
      "usageViewMode_businessDeploymentCount",
      "byType",
    );
  // The by-deployment-class and by-status data are only available team-wide, so
  // they aren't valid views when filtered to a single project — fall back to
  // by-type.
  const teamWideDisabled = projectId !== null;
  const viewMode =
    teamWideDisabled &&
    (storedViewMode === "byDeploymentClass" || storedViewMode === "byStatus")
      ? "byType"
      : storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);

  const { data: deploymentsByClassAndRegion, error: deploymentsByClassError } =
    useDeploymentsByClassAndRegion(team.id, dateRange);

  const {
    data: rawDeploymentCountByStatus,
    error: deploymentCountByStatusError,
  } = useUsageTeamDeploymentCountByStatus(team.id, dateRange);
  // Status data doesn't exist before DEPLOYMENT_STATUS_DATA_START, so drop any
  // earlier rows the query returns.
  const deploymentCountByStatus = useMemo(
    () =>
      rawDeploymentCountByStatus?.filter(
        (row) => row.ds >= DEPLOYMENT_STATUS_DATA_START,
      ),
    [rawDeploymentCountByStatus],
  );
  const statusRangeBeforeCutoff =
    shownBillingPeriod.from < DEPLOYMENT_STATUS_DATA_START;
  const statusCutoffNote = (
    <p className="text-center text-sm text-content-secondary">
      The deployment status breakdown is only available from July 23, 2026
      onwards.
    </p>
  );
  // Anchor the x-axis at the start of the viewed period with an empty day so
  // the status chart spans the same range as the other deployment views; the
  // chart gap-fills the pre-cutoff days as empty bars, avoiding the axis (and
  // bar widths) shifting when toggling to the status breakdown.
  const deploymentCountByStatusChartRows = useMemo(() => {
    if (
      deploymentCountByStatus === undefined ||
      deploymentCountByStatus.length === 0 ||
      !statusRangeBeforeCutoff
    ) {
      return deploymentCountByStatus;
    }
    return [
      {
        ds: shownBillingPeriod.from,
        metrics: [
          { tag: "active", value: 0 },
          { tag: "paused", value: 0 },
        ],
      },
      ...deploymentCountByStatus,
    ];
  }, [
    deploymentCountByStatus,
    statusRangeBeforeCutoff,
    shownBillingPeriod.from,
  ]);

  const { data: deploymentCountByType, error: deploymentCountByTypeError } =
    useUsageTeamDeploymentCountByType(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const { data: allDeploymentCountDailyByProject, error: byProjectError } =
    useUsageTeamDeploymentCountPerDayByProject(
      team.id,
      dateRange,
      componentPrefix,
    );

  // The by-project hook always returns every project, so scope it to the
  // selected project when a project filter is active.
  const deploymentCountDailyByProject =
    projectId === null
      ? allDeploymentCountDailyByProject
      : allDeploymentCountDailyByProject?.filter(
          (row) => row.projectId === projectId,
        );
  const deploymentCountDailyByProjectError = byProjectError;

  const deploymentTypeCategories = {
    prod: {
      name: "Production",
      color: "fill-chart-line-1",
    },
    dev: {
      name: "Development",
      color: "fill-chart-line-2",
    },
    preview: {
      name: "Preview",
      color: "fill-chart-line-3",
    },
    deleted: {
      name: "Deleted Deployment",
      color: "fill-chart-line-4",
    },
  };

  // Aggregate deployment data by class for the byDeploymentClass view
  const deploymentsByClass = useMemo(() => {
    if (deploymentsByClassAndRegion === undefined) return undefined;
    const grouped = groupBy(deploymentsByClassAndRegion, (row) => row.ds);
    return Object.entries(grouped).map(([ds, dayRows]) => {
      const classTotals = new Map<string, number>();
      for (const row of dayRows) {
        classTotals.set(
          row.deploymentClass,
          (classTotals.get(row.deploymentClass) || 0) + row.count,
        );
      }
      return {
        ds,
        metrics: Array.from(classTotals.entries()).map(([tag, value]) => ({
          tag,
          value,
        })),
      };
    });
  }, [deploymentsByClassAndRegion]);

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Deployments</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={DEPLOYMENT_GROUP_BY_OPTIONS}
            disabledOptions={
              teamWideDisabled
                ? {
                    byDeploymentClass:
                      "Deployment class breakdown isn't available when filtered to a single project.",
                    byStatus:
                      "Status breakdown isn't available when filtered to a single project.",
                  }
                : undefined
            }
          />
        </>
      }
    >
      <div className="px-4">
        {viewMode === "byType" ? (
          deploymentCountByTypeError ? (
            <UsageDataError entity="Deployments" />
          ) : deploymentCountByType === undefined ? (
            <ChartLoading />
          ) : (
            <UsageStackedBarChart
              rows={deploymentCountByType}
              categories={deploymentTypeCategories}
              selectedDate={selectedDate}
              setSelectedDate={setSelectedDate}
              isGauge
            />
          )
        ) : viewMode === "byProject" ? (
          deploymentCountDailyByProjectError ? (
            <UsageDataError entity="Deployments" />
          ) : deploymentCountDailyByProject === undefined ? (
            <ChartLoading />
          ) : (
            <UsageByProjectChart
              rows={deploymentCountDailyByProject}
              team={team}
              selectedDate={selectedDate}
              setSelectedDate={setSelectedDate}
              isGauge
            />
          )
        ) : viewMode === "byStatus" ? (
          deploymentCountByStatusError ? (
            <UsageDataError entity="Deployments" />
          ) : deploymentCountByStatus === undefined ? (
            <ChartLoading />
          ) : (
            <div className="flex flex-col gap-2">
              {deploymentCountByStatus.length === 0 &&
              statusRangeBeforeCutoff ? (
                // The whole selected range predates the status data — there are
                // no bars to render, so reserve the chart's space; the message
                // explains why it's blank. A post-cutoff range with no rows
                // falls through to the chart's own no-data state instead.
                <>
                  <div className="h-56" />
                  {statusCutoffNote}
                </>
              ) : (
                <UsageStackedBarChart
                  rows={deploymentCountByStatusChartRows!}
                  categories={DEPLOYMENT_STATUS_CATEGORIES}
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                  isGauge
                  showEmptyCategories
                  note={statusRangeBeforeCutoff ? statusCutoffNote : undefined}
                />
              )}
            </div>
          )
        ) : deploymentsByClassError ? (
          <UsageDataError entity="Deployments" />
        ) : deploymentsByClass === undefined ? (
          <ChartLoading />
        ) : (
          <UsageStackedBarChart
            rows={deploymentsByClass}
            categories={DEPLOYMENT_CLASS_CATEGORIES}
            selectedDate={selectedDate}
            setSelectedDate={setSelectedDate}
            isGauge
          />
        )}
      </div>
    </TeamUsageSection>
  );
}

// --- Business plan sections ---

function FunctionBreakdownSection({
  team,
  dateRange,
  projectId,
  componentPrefix,
  shownBillingPeriod,
}: {
  team: TeamResponse;
  dateRange: DateRange | null;
  projectId: number | null;
  componentPrefix: string | null;
  shownBillingPeriod: Period;
}) {
  const { data: metricsByFunction, error: metricsByFunctionError } =
    useUsageTeamMetricsByFunction(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const [functionBreakdownTab, setFunctionBreakdownTab] = useState(
    FUNCTION_BREAKDOWN_TABS_[0].name,
  );
  const metric =
    FUNCTION_BREAKDOWN_TABS_.find((t) => t.name === functionBreakdownTab) ??
    FUNCTION_BREAKDOWN_TABS_[0];
  const usageByProject = useUsageByProject(metricsByFunction, metric);

  // Counted over every returned row, not `usageByProject`, which drops projects
  // with no usage of the currently selected metric.
  const isProjectListTruncated =
    metricsByFunction !== undefined &&
    new Set(metricsByFunction.map((row) => row.projectId)).size >=
      MAX_PROJECTS_IN_BREAKDOWN;

  const {
    visibleItems: visibleProjects,
    totalPages,
    currentPage,
    setCurrentPage,
  } = usePagination({
    items: usageByProject ?? [],
    itemsPerPage: 20,
  });

  useEffect(() => {
    setCurrentPage(1);
  }, [
    team,
    projectId,
    componentPrefix,
    dateRange?.from,
    dateRange?.to,
    shownBillingPeriod.type,
    shownBillingPeriod.from,
    shownBillingPeriod.to,
    functionBreakdownTab,
    setCurrentPage,
  ]);

  const functionBreakdownOptions = FUNCTION_BREAKDOWN_TABS_.map((tab) => ({
    label: tab.name.replace(/\b\w/g, (c) => c.toUpperCase()),
    value: tab.name,
  }));

  return (
    <TeamUsageSection
      stickyHeader
      header={
        <div className="flex w-full flex-col gap-2">
          <div className="flex items-center justify-between gap-4">
            <h3>Breakdown by function</h3>

            <SegmentedControl
              options={functionBreakdownOptions}
              value={functionBreakdownTab}
              onChange={setFunctionBreakdownTab}
            />
          </div>

          {(totalPages > 1 || isProjectListTruncated) && (
            <div className="flex items-center justify-between gap-4">
              {isProjectListTruncated ? (
                <p className="text-xs text-content-secondary">
                  Showing the {MAX_PROJECTS_IN_BREAKDOWN} highest-usage
                  projects. Select a project above to see its full breakdown.
                </p>
              ) : (
                <div />
              )}
              {totalPages > 1 && (
                <PaginationControls
                  currentPage={currentPage}
                  totalPages={totalPages}
                  onPageChange={setCurrentPage}
                />
              )}
            </div>
          )}
        </div>
      }
    >
      <div className="px-4">
        {metricsByFunctionError ? (
          <UsageDataError entity="Functions breakdown" />
        ) : !metricsByFunction ? (
          <ChartLoading />
        ) : (
          <FunctionUsageBreakdown
            team={team}
            usageByProject={visibleProjects}
            metricsByDeployment={metricsByFunction}
            metric={metric}
          />
        )}
      </div>
    </TeamUsageSection>
  );
}

type DetailSectionProps = {
  team: TeamResponse;
  dateRange: DateRange | null;
  projectId: number | null;
  componentPrefix: string | null;
  isBusinessPlan?: boolean;
};

// Helper to aggregate DailyPerTagMetricsByProjectAndClass to DailyPerTagMetrics
function aggregateTagByClassToByType(
  rows: DailyPerTagMetricsByProjectAndClass[] | undefined,
  projectId: number | null,
): DailyPerTagMetrics[] | undefined | null {
  if (rows === undefined) return undefined;
  const filtered =
    projectId === null
      ? rows
      : rows.filter((row) => row.projectId === projectId);
  const grouped = groupBy(filtered, (row) => row.ds);
  return Object.entries(grouped).map(([ds, dayRows]) => {
    const metricsMap = new Map<string, number>();
    for (const row of dayRows) {
      for (const metric of row.metrics) {
        metricsMap.set(
          metric.tag,
          (metricsMap.get(metric.tag) || 0) + metric.value,
        );
      }
    }
    return {
      ds,
      metrics: Array.from(metricsMap.entries()).map(([tag, value]) => ({
        tag,
        value,
      })),
    };
  });
}

// Aggregate by-project-and-class data to by-project view (summing across deployment classes)
function aggregateTagByClassToByProject(
  rows: DailyPerTagMetricsByProjectAndClass[] | undefined,
): DailyPerTagMetricsByProject[] | undefined {
  if (rows === undefined) return undefined;
  const grouped = groupBy(rows, (row) => `${row.ds}-${row.projectId}`);
  return Object.entries(grouped).map(([, dayProjectRows]) => {
    const metricsMap = new Map<string, number>();
    for (const row of dayProjectRows) {
      for (const metric of row.metrics) {
        metricsMap.set(
          metric.tag,
          (metricsMap.get(metric.tag) || 0) + metric.value,
        );
      }
    }
    return {
      ds: dayProjectRows[0].ds,
      projectId: dayProjectRows[0].projectId,
      metrics: Array.from(metricsMap.entries()).map(([tag, value]) => ({
        tag,
        value,
      })),
    };
  });
}

// Aggregate by-project-and-class data to by-deployment-class view (summing across projects and tags)
function aggregateTagByProjectToByClass(
  rows: DailyPerTagMetricsByProjectAndClass[] | undefined,
): DailyPerTagMetrics[] | undefined {
  if (rows === undefined) return undefined;
  const grouped = groupBy(rows, (row) => row.ds);
  return Object.entries(grouped).map(([ds, dayRows]) => {
    const classTotals = new Map<string, number>();
    for (const row of dayRows) {
      const total = sumBy(row.metrics, (m) => m.value);
      classTotals.set(
        row.deploymentClass,
        (classTotals.get(row.deploymentClass) || 0) + total,
      );
    }
    return {
      ds,
      metrics: Array.from(classTotals.entries()).map(([tag, value]) => ({
        tag,
        value,
      })),
    };
  });
}

export function FunctionCallsUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] = useGlobalLocalStorage<BusinessGroupBy>(
    "usageViewMode_businessFunctionCalls",
    "byType",
  );
  const viewMode = storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data: callsByTagByProjectAndClass, error } =
    useFunctionCallsPerDayByProjectAndClass(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const callsByTag =
    viewMode === "byType"
      ? aggregateTagByClassToByType(callsByTagByProjectAndClass, null)
      : null;

  const callsByProject =
    viewMode === "byProject"
      ? aggregateTagByClassToByProject(callsByTagByProjectAndClass)
      : undefined;

  const callsByClass =
    viewMode === "byDeploymentClass"
      ? aggregateTagByProjectToByClass(callsByTagByProjectAndClass)
      : null;

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Function calls</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={BUSINESS_GROUP_BY_OPTIONS}
          />
        </>
      }
    >
      <div className="px-4">
        {error ? (
          <UsageDataError entity="Function calls" />
        ) : (
          <>
            {viewMode === "byType" ? (
              callsByTag === undefined ? (
                <ChartLoading />
              ) : callsByTag === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={callsByTag}
                  categories={TAG_CATEGORIES}
                  categoryRenames={CATEGORY_RENAMES}
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : viewMode === "byProject" ? (
              callsByProject === undefined ? (
                <ChartLoading />
              ) : (
                <UsageByProjectChart
                  rows={callsByProject}
                  team={team}
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : callsByClass === undefined ? (
              <ChartLoading />
            ) : callsByClass === null ? (
              <UsageChartUnavailable />
            ) : (
              <UsageStackedBarChart
                rows={callsByClass}
                categories={DEPLOYMENT_CLASS_CATEGORIES}
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function ComputeUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
  isBusinessPlan = true,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] = useGlobalLocalStorage<GroupBy>(
    "usageViewMode_businessCompute",
    "byType",
  );
  const viewMode = storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const businessResult = useComputePerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );
  const selfServeResult = useComputePerDayByProjectSelfServe(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );
  const { data: computeData, error } = isBusinessPlan
    ? businessResult
    : selfServeResult;

  const categories = isBusinessPlan
    ? COMPUTE_CATEGORIES
    : COMPUTE_CATEGORIES_SELF_SERVE;

  const daily =
    viewMode === "byType"
      ? aggregateByProjectToByType(computeData, null)
      : null;

  const title = isBusinessPlan ? "Compute" : "Action Compute";

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">{title}</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={GROUP_BY_OPTIONS}
          />
        </>
      }
    >
      <div className="px-4">
        {error ? (
          <UsageDataError entity={title} />
        ) : (
          <>
            {viewMode === "byType" ? (
              daily === undefined ? (
                <ChartLoading />
              ) : daily === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={daily}
                  categories={categories}
                  quantityType="actionCompute"
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : computeData === undefined ? (
              <ChartLoading />
            ) : (
              <UsageByProjectChart
                rows={computeData}
                team={team}
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
                quantityType="actionCompute"
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function DatabaseStorageUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] =
    useGlobalLocalStorage<BusinessDatabaseGroupBy>(
      "usageViewMode_businessDatabaseStorage",
      "byTable",
    );
  const viewMode = storedViewMode;

  const [activeTab, setActiveTab] = useState<"size" | "count">("size");
  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data: dataByProjectAndClass, error: storageError } =
    useDatabaseStoragePerDayByProjectAndClass(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const { data: databaseStorageByTable, error: databaseStorageByTableError } =
    useDatabaseStoragePerDayByTable(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const { data: documentsCountByProject, error: documentsCountByProjectError } =
    useUsageTeamDocumentsPerDayByProject(team.id, dateRange, componentPrefix);

  const { data: documentsCountByTable, error: documentsCountByTableError } =
    useDocumentCountPerDayByTable(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const daily =
    viewMode === "byType"
      ? aggregateTagByClassToByType(dataByProjectAndClass, null)
      : null;

  const dailyByProject =
    viewMode === "byProject"
      ? aggregateTagByClassToByProject(dataByProjectAndClass)
      : undefined;

  const dailyByClass =
    viewMode === "byDeploymentClass"
      ? aggregateTagByProjectToByClass(dataByProjectAndClass)
      : null;

  const documentsCount =
    viewMode === "byType"
      ? aggregateSimpleByProjectToByType(documentsCountByProject, projectId)
      : null;

  const hasError =
    activeTab === "size"
      ? storageError || databaseStorageByTableError
      : documentsCountByProjectError || documentsCountByTableError;

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Database Storage</h3>
          <div className="flex flex-wrap items-center gap-3">
            <SegmentedControl
              options={[
                { label: "Document Size", value: "size" },
                { label: "Document Count", value: "count" },
              ]}
              value={activeTab}
              onChange={(v) => {
                setActiveTab(v);
                setSelectedDate(null);
                if (v === "count" && viewMode === "byDeploymentClass") {
                  setViewMode("byType");
                }
              }}
            />
            <GroupBySelector
              value={viewMode}
              onChange={setViewMode}
              options={
                activeTab === "count"
                  ? DATABASE_GROUP_BY_OPTIONS
                  : BUSINESS_DATABASE_GROUP_BY_OPTIONS
              }
            />
          </div>
        </>
      }
    >
      <div className="px-4">
        {hasError ? (
          <UsageDataError entity="Database storage" />
        ) : activeTab === "size" ? (
          <>
            {viewMode === "byType" ? (
              daily === undefined ? (
                <ChartLoading />
              ) : daily === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={daily}
                  categories={DATABASE_STORAGE_CATEGORIES}
                  quantityType="storage"
                  isGauge
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : viewMode === "byTable" ? (
              databaseStorageByTable === undefined ? (
                <ChartLoading />
              ) : databaseStorageByTable === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageByTableChart
                  rows={databaseStorageByTable}
                  quantityType="storage"
                  isGauge
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : viewMode === "byProject" ? (
              dailyByProject === undefined ? (
                <ChartLoading />
              ) : (
                <UsageByProjectChart
                  rows={dailyByProject}
                  team={team}
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                  quantityType="storage"
                  isGauge
                />
              )
            ) : dailyByClass === undefined ? (
              <ChartLoading />
            ) : dailyByClass === null ? (
              <UsageChartUnavailable />
            ) : (
              <UsageStackedBarChart
                rows={dailyByClass}
                categories={DEPLOYMENT_CLASS_CATEGORIES}
                quantityType="storage"
                isGauge
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
              />
            )}
          </>
        ) : (
          <>
            {viewMode === "byType" ? (
              documentsCount === undefined ? (
                <ChartLoading />
              ) : documentsCount === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageBarChart rows={documentsCount} entity="documents" />
              )
            ) : viewMode === "byTable" ? (
              documentsCountByTable === undefined ? (
                <ChartLoading />
              ) : documentsCountByTable === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageByTableChart
                  rows={documentsCountByTable}
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : documentsCountByProject === undefined ? (
              <ChartLoading />
            ) : (
              <UsageByProjectChart
                rows={documentsCountByProject}
                team={team}
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function DatabaseIOUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] = useGlobalLocalStorage<BusinessGroupBy>(
    "usageViewMode_businessDatabaseIO",
    "byType",
  );
  const viewMode = storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data: dataByProjectAndClass, error } =
    useDatabaseIOPerDayByProjectAndClass(
      team.id,
      dateRange,
      projectId,
      componentPrefix,
    );

  const daily =
    viewMode === "byType"
      ? aggregateTagByClassToByType(dataByProjectAndClass, null)
      : null;

  const dailyByProject =
    viewMode === "byProject"
      ? aggregateTagByClassToByProject(dataByProjectAndClass)
      : undefined;

  const dailyByClass =
    viewMode === "byDeploymentClass"
      ? aggregateTagByProjectToByClass(dataByProjectAndClass)
      : null;

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Database I/O</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={BUSINESS_GROUP_BY_OPTIONS}
          />
        </>
      }
    >
      <div className="px-4">
        {error ? (
          <UsageDataError entity="Database I/O" />
        ) : (
          <>
            {viewMode === "byType" ? (
              daily === undefined ? (
                <ChartLoading />
              ) : daily === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={daily}
                  categories={DATABASE_IO_CATEGORIES}
                  quantityType="storage"
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : viewMode === "byProject" ? (
              dailyByProject === undefined ? (
                <ChartLoading />
              ) : (
                <UsageByProjectChart
                  rows={dailyByProject}
                  team={team}
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                  quantityType="storage"
                />
              )
            ) : dailyByClass === undefined ? (
              <ChartLoading />
            ) : dailyByClass === null ? (
              <UsageChartUnavailable />
            ) : (
              <UsageStackedBarChart
                rows={dailyByClass}
                categories={DEPLOYMENT_CLASS_CATEGORIES}
                quantityType="storage"
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function SearchStorageUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] = useGlobalLocalStorage<GroupBy>(
    "usageViewMode_businessSearchStorage",
    "byType",
  );
  const viewMode = storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data, error } = useSearchStoragePerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );

  const daily =
    viewMode === "byType" ? aggregateByProjectToByType(data, null) : null;

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Search Storage</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={GROUP_BY_OPTIONS}
          />
        </>
      }
    >
      <div className="px-4">
        {error ? (
          <UsageDataError entity="Search storage" />
        ) : (
          <>
            {viewMode === "byType" ? (
              daily === undefined ? (
                <ChartLoading />
              ) : daily === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={daily}
                  categories={SEARCH_STORAGE_CATEGORIES}
                  quantityType="storage"
                  isGauge
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : data === undefined ? (
              <ChartLoading />
            ) : (
              <UsageByProjectChart
                rows={data}
                team={team}
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
                quantityType="storage"
                isGauge
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function FileStorageUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data, error } = useFileStoragePerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );

  return (
    <TeamUsageSection header={<h3 className="py-2">File Storage</h3>}>
      <div className="px-4">
        {error ? (
          <UsageDataError entity="File storage" />
        ) : data === undefined ? (
          <ChartLoading />
        ) : (
          <UsageByProjectChart
            rows={data}
            team={team}
            selectedDate={selectedDate}
            setSelectedDate={setSelectedDate}
            quantityType="storage"
            isGauge
          />
        )}
      </div>
    </TeamUsageSection>
  );
}

function AuditLogBandwidthUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data, error } = useAuditLogBandwidthPerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );

  return (
    <TeamUsageSection header={<h3 className="py-2">Audit Log Bandwidth</h3>}>
      <div className="px-4">
        {error ? (
          <UsageDataError entity="Audit log bandwidth" />
        ) : data === undefined ? (
          <ChartLoading />
        ) : (
          <UsageByProjectChart
            rows={data}
            team={team}
            selectedDate={selectedDate}
            setSelectedDate={setSelectedDate}
            quantityType="storage"
          />
        )}
      </div>
    </TeamUsageSection>
  );
}

// There's no fixed list of models — whatever shows up in the data gets a
// chart color, reusing the palette when there are more models than colors.
function modelCategories(rows: DailyPerTagMetrics[]) {
  const models = [
    ...new Set(rows.flatMap(({ metrics }) => metrics.map(({ tag }) => tag))),
  ].sort();
  return Object.fromEntries(
    models.map((model, i) => [
      model,
      { name: model, color: `fill-chart-line-${(i % 8) + 1}` },
    ]),
  );
}

function AiGatewayCostUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [viewMode, setViewMode] = useGlobalLocalStorage<AiGatewayGroupBy>(
    "usageViewMode_aiGateway",
    "byProject",
  );
  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data, error } = useAiGatewayCostPerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );
  const { data: byModel, error: byModelError } = useAiGatewayCostPerDayByModel(
    team.id,
    dateRange,
    viewMode === "byModel" ? projectId : null,
    componentPrefix,
  );

  return (
    <div data-testid="ai-gateway-usage">
      <TeamUsageSection
        header={
          <>
            <h3 className="py-2">AI Gateway</h3>
            <GroupBySelector
              value={viewMode}
              onChange={setViewMode}
              options={AI_GATEWAY_GROUP_BY_OPTIONS}
            />
          </>
        }
      >
        <div className="px-4">
          {viewMode === "byModel" ? (
            byModelError ? (
              <UsageDataError entity="AI gateway spend" />
            ) : byModel === undefined ? (
              <ChartLoading />
            ) : (
              <UsageStackedBarChart
                rows={byModel}
                categories={modelCategories(byModel)}
                quantityType="currency"
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
              />
            )
          ) : error ? (
            <UsageDataError entity="AI gateway spend" />
          ) : data === undefined ? (
            <ChartLoading />
          ) : (
            <UsageByProjectChart
              rows={data}
              team={team}
              selectedDate={selectedDate}
              setSelectedDate={setSelectedDate}
              quantityType="currency"
            />
          )}
        </div>
      </TeamUsageSection>
    </div>
  );
}

function DataEgressUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] = useGlobalLocalStorage<GroupBy>(
    "usageViewMode_businessDataEgress",
    "byType",
  );
  const viewMode = storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data, error } = useDataEgressPerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );

  const categories = DATA_EGRESS_CATEGORIES;
  const categoryRenames = DATA_EGRESS_CATEGORY_RENAMES;

  const daily =
    viewMode === "byType" ? aggregateByProjectToByType(data, null) : null;

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Data Egress</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={GROUP_BY_OPTIONS}
          />
        </>
      }
    >
      <div className="px-4">
        {error ? (
          <UsageDataError entity="Data egress" />
        ) : (
          <>
            {viewMode === "byType" ? (
              daily === undefined ? (
                <ChartLoading />
              ) : daily === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={daily}
                  categories={categories}
                  categoryRenames={categoryRenames}
                  quantityType="storage"
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : data === undefined ? (
              <ChartLoading />
            ) : (
              <UsageByProjectChart
                rows={data}
                team={team}
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
                quantityType="storage"
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function SearchQueriesUsage({
  team,
  dateRange,
  projectId,
  componentPrefix,
}: DetailSectionProps) {
  const [storedViewMode, setViewMode] = useGlobalLocalStorage<GroupBy>(
    "usageViewMode_businessSearchQueries",
    "byType",
  );
  const viewMode = storedViewMode;

  const [selectedDate, setSelectedDate] = useState<number | null>(null);
  const { data, error } = useSearchQueriesPerDayByProject(
    team.id,
    dateRange,
    projectId,
    componentPrefix,
  );

  const daily =
    viewMode === "byType" ? aggregateByProjectToByType(data, null) : null;

  const dailyByProject = viewMode === "byProject" ? data : undefined;

  return (
    <TeamUsageSection
      header={
        <>
          <h3 className="py-2">Search Queries</h3>
          <GroupBySelector
            value={viewMode}
            onChange={setViewMode}
            options={GROUP_BY_OPTIONS}
          />
        </>
      }
    >
      <div className="px-4">
        {error ? (
          <UsageDataError entity="Search queries" />
        ) : (
          <>
            {viewMode === "byType" ? (
              daily === undefined ? (
                <ChartLoading />
              ) : daily === null ? (
                <UsageChartUnavailable />
              ) : (
                <UsageStackedBarChart
                  rows={daily}
                  categories={SEARCH_QUERIES_CATEGORIES}
                  quantityType="textSearch"
                  selectedDate={selectedDate}
                  setSelectedDate={setSelectedDate}
                />
              )
            ) : dailyByProject === undefined ? (
              <ChartLoading />
            ) : (
              <UsageByProjectChart
                rows={dailyByProject}
                quantityType="textSearch"
                team={team}
                selectedDate={selectedDate}
                setSelectedDate={setSelectedDate}
              />
            )}
          </>
        )}
      </div>
    </TeamUsageSection>
  );
}

function useHasSubscription(teamId?: number): boolean | undefined {
  const { subscription: orbSub } = useTeamOrbSubscription(teamId);
  return orbSub === undefined ? undefined : orbSub !== null;
}

function TeamUsageSection({
  header,
  children,
  stickyHeader,
}: React.PropsWithChildren<{
  header: React.ReactNode;
  stickyHeader?: boolean;
}>) {
  return (
    <section>
      <header
        className={
          stickyHeader
            ? "sticky top-(--team-usage-toolbar-height) z-10 bg-background-primary pt-2"
            : undefined
        }
      >
        <div className="flex w-full flex-wrap items-center justify-between gap-4 rounded-t-lg border bg-background-secondary p-4 py-2">
          {header}
        </div>
      </header>
      <Sheet padding={false} className="rounded-t-none border-t-0 py-4">
        {children}
      </Sheet>
    </section>
  );
}

// Aggregate by-project data to by-type view by summing across all projects
function aggregateByProjectToByType(
  rows: DailyPerTagMetricsByProject[] | undefined | null,
  projectId: number | null,
): DailyPerTagMetrics[] | undefined | null {
  if (rows === undefined) return undefined;
  if (rows === null) return null;

  // If a project filter is active, filter to that project
  const filteredRows =
    projectId === null
      ? rows
      : rows.filter((row) => row.projectId === projectId);

  const grouped = groupBy(filteredRows, (row) => row.ds);
  return Object.entries(grouped).map(([ds, dayRows]) => {
    // For each day, aggregate metrics across all projects
    const metricsMap = new Map<string, number>();
    for (const row of dayRows) {
      for (const metric of row.metrics) {
        metricsMap.set(
          metric.tag,
          (metricsMap.get(metric.tag) || 0) + metric.value,
        );
      }
    }
    return {
      ds,
      metrics: Array.from(metricsMap.entries()).map(([tag, value]) => ({
        tag,
        value,
      })),
    };
  });
}

// Aggregate simple by-project data to by-type view
function aggregateSimpleByProjectToByType(
  rows: DailyMetricByProject[] | undefined | null,
  projectId: number | null,
): DailyMetric[] | undefined | null {
  if (rows === undefined) return undefined;
  if (rows === null) return null;
  // If a project filter is active, filter to that project
  const filteredRows =
    projectId === null
      ? rows
      : rows.filter((row) => row.projectId === projectId);

  const grouped = groupBy(filteredRows, (row) => row.ds);
  return Object.entries(grouped).map(([ds, dayRows]) => ({
    ds,
    value: sumBy(dayRows, (row) => row.value),
  }));
}
