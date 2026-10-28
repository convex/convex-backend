import {
  ChevronDownIcon,
  DesktopIcon,
  DotsHorizontalIcon,
  DownloadIcon,
  FileIcon,
  PaperPlaneIcon,
  UploadIcon,
} from "@radix-ui/react-icons";
import classNames from "classnames";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import {
  AggregatedFunctionMetrics,
  REST_OF_FUNCTIONS,
  UsageDeploymentType,
} from "hooks/usageMetrics";
import { rootComponentPath } from "api/usage";
import Link from "next/link";
import { ReactNode, useMemo, useState } from "react";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { TeamResponse, ProjectDetails } from "generatedApi";
import { PuzzlePieceIcon } from "@common/elements/icons";
import { BANDWIDTH_CATEGORIES } from "./lib/teamUsageCategories";
import { deploymentTypeColorClasses } from "@common/lib/deploymentTypeColorClasses";
import {
  QuantityType,
  formatQuantity,
  formatQuantityCompact,
} from "./lib/formatQuantity";
import { cn } from "@ui/cn";

const ITEMS_SHOWN_INITIALLY = 6;
const ITEMS_SHOW_MORE_INCREMENT = 20;

// Rows labelled by what they represent rather than by a function path.
type SpecialRowKind =
  | "dashboard"
  | "cloudBackup"
  | "logStreaming"
  | "streamingExport"
  | "fileServing"
  | "restOfFunctions";

function getSpecialRowKind(fn: string): SpecialRowKind | null {
  if (fn.startsWith("_system/")) return "dashboard";
  if (fn === "_system_job/cloud_backup") return "cloudBackup";
  if (fn === "_system_job/log_stream_payload") return "logStreaming";
  if (fn === "_system_job/streaming_export") return "streamingExport";
  if (fn === "_system_job/get_range") return "fileServing";
  if (fn === REST_OF_FUNCTIONS) return "restOfFunctions";
  return null;
}

function renderSpecialRowLabel(kind: SpecialRowKind): ReactNode {
  switch (kind) {
    case "dashboard":
      return (
        <span className="flex items-center gap-1.5">
          <DesktopIcon />
          Dashboard
        </span>
      );
    case "cloudBackup":
      return (
        <span className="flex items-center gap-1.5">
          <DownloadIcon />
          Cloud Backup Generation
        </span>
      );
    case "logStreaming":
      return (
        <span className="flex items-center gap-1.5">
          <PaperPlaneIcon />
          Log Streaming
        </span>
      );
    case "streamingExport":
      return (
        <span className="flex items-center gap-1.5">
          <UploadIcon />
          Streaming Export
        </span>
      );
    case "fileServing":
      return (
        <span className="flex items-center gap-1.5">
          <FileIcon />
          File Serving
        </span>
      );
    case "restOfFunctions":
      return (
        <span className="flex items-center gap-1.5">
          <DotsHorizontalIcon />
          All other functions
        </span>
      );
  }
}

type DeploymentTypeRow = {
  key: string;
  function: string;
  componentPath: string;
  value: number;
  values: number[];
  deploymentType: UsageDeploymentType | null;
  specialKind: SpecialRowKind | null;
  href: string | null;
};

export type FunctionMetricsRow = AggregatedFunctionMetrics;

export type FunctionBreakdownMetric = {
  name: string;
  getTotal: (row: FunctionMetricsRow) => number;
  getValues: (row: FunctionMetricsRow) => number[];
  quantityType: QuantityType;
  categories?: {
    name: string;
    backgroundColor: string;
  }[];
};

// function breakdown metrics

export const FunctionBreakdownMetricCalls: FunctionBreakdownMetric = {
  name: "function calls",
  getTotal: (row) => row.callCount,
  getValues: (row) => [row.callCount],
  quantityType: "unit",
};

export const FunctionBreakdownMetricDatabaseIO: FunctionBreakdownMetric = {
  name: "database I/O",
  getTotal: (row) => row.databaseIngressSize + row.databaseEgressSize,
  getValues: (row) => [row.databaseEgressSize, row.databaseIngressSize],
  quantityType: "storage",
  categories: Object.values(BANDWIDTH_CATEGORIES),
};

export const FunctionBreakdownMetricCompute: FunctionBreakdownMetric = {
  name: "compute",
  getTotal: (row) =>
    ("queryMutationComputeTime" in row ? row.queryMutationComputeTime : 0) +
    ("actionComputeConvexTime" in row ? row.actionComputeConvexTime : 0) +
    ("actionComputeNodeTime" in row ? row.actionComputeNodeTime : 0),
  getValues: (row) => [
    "queryMutationComputeTime" in row ? row.queryMutationComputeTime : 0,
    "actionComputeConvexTime" in row ? row.actionComputeConvexTime : 0,
    "actionComputeNodeTime" in row ? row.actionComputeNodeTime : 0,
  ],
  quantityType: "actionCompute",
  categories: [
    {
      name: "Query/Mutation",
      backgroundColor: "bg-chart-line-1/30 dark:bg-chart-line-1",
    },
    {
      name: "Action (Convex)",
      backgroundColor: "bg-chart-line-2/30 dark:bg-chart-line-2",
    },
    {
      name: "Action (Node)",
      backgroundColor: "bg-chart-line-3/30 dark:bg-chart-line-3",
    },
  ],
};

export const FunctionBreakdownMetricSearch: FunctionBreakdownMetric = {
  name: "search",
  getTotal: (row) =>
    ("textSearchGb" in row ? row.textSearchGb : 0) +
    ("vectorSearchGb" in row ? row.vectorSearchGb : 0),
  getValues: (row) => [
    "textSearchGb" in row ? row.textSearchGb : 0,
    "vectorSearchGb" in row ? row.vectorSearchGb : 0,
  ],
  quantityType: "textSearch",
  categories: [
    {
      name: "Text Search",
      backgroundColor: "bg-chart-line-1/30 dark:bg-chart-line-1",
    },
    {
      name: "Vector Search",
      backgroundColor: "bg-chart-line-2/30 dark:bg-chart-line-2",
    },
  ],
};

export const FunctionBreakdownMetricDataEgress: FunctionBreakdownMetric = {
  name: "data egress",
  getTotal: (row) => ("dataEgress" in row ? row.dataEgress : 0),
  getValues: (row) => ["dataEgress" in row ? row.dataEgress : 0],
  quantityType: "storage",
};

export const FunctionBreakdownMetricAiGateway: FunctionBreakdownMetric = {
  name: "AI",
  getTotal: (row) => ("aiGatewayCost" in row ? row.aiGatewayCost : 0),
  getValues: (row) => ["aiGatewayCost" in row ? row.aiGatewayCost : 0],
  quantityType: "currency",
};

export function TeamUsageByFunctionChart({
  project,
  metric,
  deployments,
  rows,
  team,
  maxValue,
}: {
  project: ProjectDetails | null;
  metric: FunctionBreakdownMetric;
  deployments: PlatformDeploymentResponse[];
  rows: FunctionMetricsRow[];
  team: TeamResponse;
  maxValue: number;
}) {
  const [numShown, setNumShown] = useState(ITEMS_SHOWN_INITIALLY);

  const orderedAndGroupedRows = useOrderedAndGroupedRows(
    rows,
    metric,
    project,
    deployments,
    team,
  );

  const nonZeroRows = useMemo(
    () => orderedAndGroupedRows.filter((row) => row.value > 0),
    [orderedAndGroupedRows],
  );

  return (
    <div role="table">
      <div role="rowgroup" hidden aria-hidden={false}>
        <span role="columnheader">Function</span>
        <span role="columnheader">Deployment type</span>
        <span role="columnheader">Value</span>
      </div>

      <div className="relative" role="rowgroup">
        {nonZeroRows.slice(0, numShown).map((row) => (
          <ChartRow
            key={row.key}
            row={row}
            maxValue={maxValue}
            quantityType={metric.quantityType}
            categories={metric.categories}
          />
        ))}

        {numShown < nonZeroRows.length && (
          <div className="h-4">
            <div className="bottom-four pointer-events-none absolute h-24 w-full bg-linear-to-b from-transparent to-background-secondary" />
            <div className="absolute bottom-0 left-[50%]">
              <Button
                className="-translate-x-1/2"
                variant="neutral"
                onClick={() =>
                  setNumShown((n) => n + ITEMS_SHOW_MORE_INCREMENT)
                }
                icon={<ChevronDownIcon />}
                inline
              >
                Show more
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function ChartRow({
  row,
  maxValue,
  quantityType,
  categories,
}: {
  row: DeploymentTypeRow;
  maxValue: number;
  quantityType: QuantityType;
  categories:
    | {
        name: string;
        backgroundColor: string;
      }[]
    | undefined;
}) {
  const path = row.function;
  const { componentPath, specialKind } = row;
  const { module, functionName } = useMemo(() => {
    const separator = ".js:";
    const separatorPosition = path.indexOf(separator);

    if (specialKind !== null) {
      return { module: "", functionName: "default" };
    }

    if (separatorPosition === -1) {
      // HTTP Actions use the request path as the function name
      return { module: "HTTP", functionName: path };
    }

    return {
      module: path.substring(0, separatorPosition),
      functionName: path.substring(separatorPosition + separator.length),
    };
  }, [path, specialKind]);

  const { values } = row;
  const nonZeroValues = values
    .map((value, i) => [value, i])
    .filter(([value]) => value > 0);
  const linkContents = (
    <div className="group relative flex h-10 py-1">
      <div role="cell" className="relative flex grow">
        <div className="absolute top-0 left-0 flex size-full items-center">
          {nonZeroValues.map(([value, index], i) => (
            <div
              className={classNames(
                "flex h-6 min-w-[4px] items-center overflow-hidden",
                categories
                  ? categories[index].backgroundColor
                  : "bg-blue-200 dark:bg-cyan-900",
                i === 0 ? "rounded-l" : "",
                i === nonZeroValues.length - 1 ? "rounded-r" : "",
              )}
              key={index}
              style={{ width: `${(value / maxValue) * 100}%` }}
            />
          ))}
        </div>

        <div className="absolute top-0 left-0 flex size-full items-center text-sm">
          <div className="truncate px-4">
            {specialKind !== null ? (
              renderSpecialRowLabel(specialKind)
            ) : (
              <div className="flex items-center gap-1.5">
                {componentPath && componentPath !== rootComponentPath && (
                  <PuzzlePieceIcon className="min-w-[13px]" />
                )}
                <div>
                  {componentPath && componentPath !== rootComponentPath && (
                    <span className="font-mono text-content-secondary">
                      {componentPath}/
                    </span>
                  )}
                  <span className="font-mono font-semibold">{module}</span>
                  {functionName !== "default" && (
                    <span className="font-mono">
                      {module === "HTTP" ? " " : "."}
                      {functionName}
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      <div role="cell" className="flex w-32 items-center pl-6">
        {row.deploymentType && (
          <DeploymentTypeIndicator deploymentType={row.deploymentType} />
        )}
      </div>

      <div
        role="cell"
        className="flex w-24 items-center justify-end px-4 whitespace-nowrap tabular-nums"
      >
        {formatQuantityCompact(row.value, quantityType)}
      </div>

      <div
        role="presentation"
        aria-hidden
        className={classNames(
          "absolute left-0 top-0 h-full w-full group-hover:bg-slate-900/5 dark:group-hover:bg-white/5 pointer-events-none rounded-sm",
        )}
      />
    </div>
  );

  const valueTip =
    categories !== undefined
      ? values
          .map((value, index) => ({ value, index }))
          .filter(({ value }) => value > 0)
          .map(({ value, index }) => (
            <div key={index}>
              <span
                className={classNames(
                  "rounded-full size-2 inline-block",
                  "border border-border-transparent dark:border-border-transparent",
                  categories![index].backgroundColor,
                )}
              />{" "}
              {categories![index].name}: {formatQuantity(value, quantityType)}
            </div>
          ))
      : quantityType === "actionCompute"
        ? formatQuantity(values[0], quantityType)
        : null;

  const deploymentTypeTip =
    row.deploymentType === "local" ? (
      <div>AI Gateway spend from local development linked to this project.</div>
    ) : row.deploymentType === "dev" ? (
      <div>This row aggregates all dev deployments of your team.</div>
    ) : row.deploymentType === "preview" ? (
      <div>This row aggregates all preview deployments of your team.</div>
    ) : null;

  const specialFunctionTip =
    specialKind === "dashboard" ? (
      <div>
        Usage incurred by using the Convex dashboard, such as viewing the data
        or logs page for your deployment.
      </div>
    ) : specialKind === "restOfFunctions" ? (
      <div>
        Usage from this project’s lower-usage functions, which aren’t broken out
        individually. Spans all deployment types.
      </div>
    ) : null;
  const tip =
    valueTip !== null ||
    deploymentTypeTip !== null ||
    specialFunctionTip !== null ? (
      <div className="flex flex-col items-center gap-2">
        {valueTip !== null ? (
          <div className="flex flex-col items-end">{valueTip}</div>
        ) : null}
        {deploymentTypeTip}
        {specialFunctionTip}
      </div>
    ) : undefined;

  const rowContents = row.href ? (
    <Tooltip tip={tip} side="top" asChild>
      <Link passHref href={row.href}>
        {linkContents}
      </Link>
    </Tooltip>
  ) : (
    <Tooltip tip={tip} side="top" className="w-full">
      {linkContents}
    </Tooltip>
  );

  return <div role="row">{rowContents}</div>;
}

function DeploymentTypeIndicator({
  deploymentType,
}: {
  deploymentType: UsageDeploymentType;
}) {
  const colorClasses = deploymentTypeColorClasses(
    deploymentType === "local" ? "custom" : deploymentType,
  );
  return (
    <>
      <span
        className={cn(
          "mr-2 size-4 rounded-xl border",
          colorClasses,
          "border-border-transparent dark:border-border-transparent",
        )}
      />
      <span className="capitalize">{deploymentType}</span>
    </>
  );
}

/**
 * Groups the rows so that multiple development deployments are grouped together
 * and they are sorted by call count.
 */
function useOrderedAndGroupedRows(
  rows: FunctionMetricsRow[],
  metric: FunctionBreakdownMetric,
  project: ProjectDetails | null,
  deployments: PlatformDeploymentResponse[],
  team: TeamResponse,
): DeploymentTypeRow[] {
  return useMemo(() => {
    // A project can have several production deployments; link to the default
    // one. `isDefault` only exists on cloud deployments.
    const prodDeploymentName = deployments.find(
      (d) => d.kind === "cloud" && d.deploymentType === "prod" && d.isDefault,
    )?.name;
    const byFunctionAndDeploymentType = rows.reduce(
      (accumulator, row) => {
        const { componentPath } = row;
        let key;
        const specialKind = getSpecialRowKind(row.function);
        const name = specialKind === "dashboard" ? "" : row.function;
        // Without a project, rows from every deployment type are shown together.
        const deploymentType = project ? row.deploymentType : null;
        if (project) {
          key = `${componentPath} ${name} ${deploymentType}`;
        } else {
          key = `${componentPath} ${name}`;
        }

        const total = metric.getTotal(row);
        const values = metric.getValues(row);
        if (key in accumulator) {
          const accumulated = accumulator[key];
          accumulated.value += total;
          values.forEach((value, index) => {
            accumulated.values[index] += value;
          });
        } else {
          accumulator[key] = {
            key,
            function: name,
            componentPath,
            value: total,
            values,
            deploymentType,
            specialKind,

            // We don’t link to development environments because they might belong to
            // someone else in the team. This might be improved later.
            href:
              project &&
              deploymentType === "prod" &&
              specialKind === null &&
              prodDeploymentName
                ? `/t/${team.slug}/${project.slug}/${prodDeploymentName}/functions?function=${encodeURIComponent(
                    name.replace(".js", ""),
                  )}`
                : null,
          };
        }

        return accumulator;
      },
      {} as Record<string, DeploymentTypeRow>,
    );

    // The roll-up sits last however large it is, so it reads as the tail of the
    // list rather than as the project's biggest function.
    return Object.values(byFunctionAndDeploymentType).sort((a, b) => {
      const aIsRest = a.specialKind === "restOfFunctions";
      const bIsRest = b.specialKind === "restOfFunctions";
      if (aIsRest !== bIsRest) {
        return aIsRest ? 1 : -1;
      }
      return b.value - a.value;
    });
  }, [rows, metric, deployments, project, team]);
}
