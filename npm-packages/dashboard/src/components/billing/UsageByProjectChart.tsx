import {
  DailyMetricByProject,
  DailyPerTagMetricsByProject,
} from "hooks/usageMetrics";
import { useMemo } from "react";
import groupBy from "lodash/groupBy";
import sumBy from "lodash/sumBy";
import { TeamResponse } from "generatedApi";
import { toNumericUTC } from "@common/lib/format";
import { Bar, Rectangle, ReferenceArea } from "recharts";
import { useProjectById } from "api/projects";
import { UsageNoDataError } from "./TeamUsageError";
import { QuantityType, formatQuantity } from "./lib/formatQuantity";
import { DailyChart } from "./DailyChart";

// When there is only a data point, we have to set the bar width manually to make it appear (https://github.com/recharts/recharts/issues/3640).
// This value has been measured manually on a desktop screen size, but it should also look good in other contexts where there is only one bar.
const SINGLE_BAR_WIDTH = 91;

const MS_IN_DAY = 24 * 60 * 60 * 1000;

// Colors for projects - we'll cycle through these
const PROJECT_COLORS = [
  "fill-chart-line-1",
  "fill-chart-line-2",
  "fill-chart-line-3",
  "fill-chart-line-4",
  "fill-chart-line-5",
  "fill-chart-line-6",
  "fill-chart-line-7",
  "fill-chart-line-8",
];

// Component to render a single project name (can call hooks)
function ProjectName({ projectId }: { projectId: number | string }) {
  const { project, isLoading } = useProjectById(
    projectId === "_rest" ? undefined : (projectId as number),
  );

  if (projectId === "_rest") {
    return <>All other projects</>;
  }

  if (isLoading) {
    return (
      <span className="inline-block h-4 w-32 animate-pulse rounded-sm bg-content-tertiary" />
    );
  }

  return project?.name ? (
    project.name
  ) : (
    <span className="opacity-70">Deleted Project ({projectId})</span>
  );
}

// Custom tooltip component that renders project names
function ProjectTooltipItem({
  projectId,
  value,
  color,
  quantityType,
}: {
  projectId: number | string;
  value: number;
  color: string;
  quantityType: QuantityType;
}) {
  return (
    <div className="flex items-center gap-2">
      <svg className="size-3 shrink-0" viewBox="0 0 50 50" aria-hidden>
        <circle cx="25" cy="25" r="25" className={color} />
      </svg>
      <span className="tabular-nums">
        <ProjectName projectId={projectId} />:{" "}
        {formatQuantity(value, quantityType)}
      </span>
    </div>
  );
}

// Custom tooltip that can render project names with hooks
function ProjectChartTooltip({
  active,
  payload,
  label,
  quantityType,
  colorMap,
}: {
  active?: boolean;
  payload?: readonly any[];
  label?: string | number;
  quantityType: QuantityType;
  colorMap: Map<string, string>;
}) {
  if (!active || !payload || payload.length === 0) {
    return null;
  }

  // Filter to items with value > 0 and extract project IDs
  const items = payload
    .filter((entry) => {
      const value = entry.value as number;
      return value > 0;
    })
    .reverse(); // Reverse to show highest value first

  if (items.length === 0) {
    return null;
  }

  const formattedDate =
    label !== null && label !== undefined
      ? new Date(label).toLocaleDateString("en-us", {
          year: "numeric",
          month: "long",
          day: "numeric",
          timeZone: "UTC",
        })
      : "";

  return (
    <div className="rounded-sm border bg-background-secondary/70 p-3 backdrop-blur-[2px]">
      <div className="mb-2 font-semibold">{formattedDate}</div>
      <div className="space-y-1">
        {items.map((entry, index) => {
          // Extract project ID from dataKey (format: "project_123")
          const projectIdStr = (entry.dataKey as string).replace(
            "project_",
            "",
          );
          const projectId =
            projectIdStr === "_rest" ? "_rest" : Number(projectIdStr);
          const color = colorMap.get(entry.dataKey as string) || "";

          return (
            <ProjectTooltipItem
              key={index}
              projectId={projectId}
              value={entry.value as number}
              color={color}
              quantityType={quantityType}
            />
          );
        })}
      </div>
    </div>
  );
}

// Detail item that includes project ID for lazy loading
interface ProjectDetailItem {
  projectId: number | string;
  value: number;
  sortValue?: number;
  color: string;
}

// Inline detail row for a project
function ProjectDetailRow({
  item,
  total,
  quantityType,
}: {
  item: ProjectDetailItem;
  total: number;
  quantityType: QuantityType;
}) {
  const percentage = total > 0 ? (item.value / total) * 100 : 0;

  return (
    <div className="flex items-center justify-between gap-4 rounded-sm px-2 py-1 text-sm hover:bg-slate-900/5 dark:hover:bg-white/5">
      <span className="flex min-w-0 items-center gap-2">
        <span
          className="size-2.5 shrink-0 rounded-full"
          style={{
            backgroundColor: `var(--color-${item.color.replace("fill-", "")})`,
          }}
        />
        <span className="truncate">
          <ProjectName projectId={item.projectId} />
        </span>
      </span>
      <span className="flex shrink-0 items-center gap-3 tabular-nums">
        <span>{formatQuantity(item.value, quantityType)}</span>
        <span className="w-12 text-right opacity-70">
          {percentage.toFixed(1)}%
        </span>
      </span>
    </div>
  );
}

export function UsageByProjectChart({
  rows,
  quantityType = "unit",
  isGauge = false,
  selectedDate,
  setSelectedDate,
}: {
  rows: DailyPerTagMetricsByProject[] | DailyMetricByProject[];
  quantityType?: QuantityType;
  /** If true, total shows the most recent day's value instead of summing all days */
  isGauge?: boolean;
  team?: TeamResponse;
  selectedDate: number | null;
  setSelectedDate: (date: number | null) => void;
}) {
  const { chartData, projectIds, totalByProject } = useMemo(() => {
    // Helper to get the total value from a row (handles both data types)
    const getRowTotal = (
      row: DailyPerTagMetricsByProject | DailyMetricByProject,
    ) => {
      if ("metrics" in row) {
        return sumBy(row.metrics, (m) => m.value);
      }
      return row.value;
    };

    // Get all unique project IDs and sort by total usage
    const byProject = groupBy(rows, (row) =>
      String(
        (row as DailyPerTagMetricsByProject | DailyMetricByProject).projectId,
      ),
    );
    const projectTotals = Object.entries(byProject).map(
      ([projectId, projectRows]) => {
        const parsedId = projectId === "_rest" ? "_rest" : Number(projectId);
        return {
          projectId: parsedId,
          total: sumBy(
            projectRows as Array<
              DailyPerTagMetricsByProject | DailyMetricByProject
            >,
            getRowTotal,
          ),
        };
      },
    );

    // Create quantity-sorted list for stacking (largest at bottom)
    // Also used for legend display (sorted by quantity, not alphabetically)
    const stackProjectIds = [...projectTotals]
      .sort((a, b) => {
        if (a.projectId === "_rest") return 1;
        if (b.projectId === "_rest") return -1;
        return b.total - a.total;
      })
      .map((p) => p.projectId);

    const filledData = [];
    const dateSet = new Set(rows.map(({ ds }) => toNumericUTC(ds)));

    // Find the range of dates
    const minDate = Math.min(...Array.from(dateSet));
    const maxDate = Math.max(...Array.from(dateSet));

    // Fill in the missing dates
    for (let date = minDate; date <= maxDate; date += MS_IN_DAY) {
      const dayRows = rows.filter(({ ds }) => toNumericUTC(ds) === date);
      const dataPoint: any = {
        dateNumeric: date,
      };

      // For each project, sum up all values for that day
      for (const projectId of stackProjectIds) {
        const projectRows = dayRows.filter((r) => r.projectId === projectId);
        const total = sumBy(projectRows, getRowTotal);
        dataPoint[`project_${projectId}`] = total;
      }

      filledData.push(dataPoint);
    }

    const totals = Object.fromEntries(
      projectTotals.map((p) => [p.projectId, p.total]),
    );

    // For gauge metrics, compute totals from the most recent day
    let displayTotals = totals;
    if (isGauge && filledData.length > 0) {
      const lastDay = filledData[filledData.length - 1];
      displayTotals = Object.fromEntries(
        stackProjectIds.map((projectId) => [
          projectId,
          (lastDay[`project_${projectId}`] as number) || 0,
        ]),
      );
    }

    return {
      chartData: filledData,
      projectIds: stackProjectIds,
      totalByProject: displayTotals,
    };
  }, [rows, isGauge]);

  const colorMap = useMemo(() => {
    const map = new Map<string, string>();
    projectIds.forEach((projectId, index) => {
      const color = PROJECT_COLORS[index % PROJECT_COLORS.length];
      map.set(`project_${projectId}`, color);
    });
    return map;
  }, [projectIds]);

  // Items for the inline list: show day values when a date is selected, totals otherwise
  const detailItems = useMemo((): ProjectDetailItem[] => {
    if (selectedDate !== null) {
      const dataPoint = chartData.find((d) => d.dateNumeric === selectedDate);
      if (dataPoint) {
        return projectIds.map((projectId, index) => {
          const color = PROJECT_COLORS[index % PROJECT_COLORS.length];
          return {
            projectId,
            value: (dataPoint[`project_${projectId}`] as number) || 0,
            sortValue: totalByProject[projectId] || 0,
            color,
          };
        });
      }
    }

    return projectIds.map((projectId, index) => {
      const color = PROJECT_COLORS[index % PROJECT_COLORS.length];
      return {
        projectId,
        value: totalByProject[projectId] || 0,
        color,
      };
    });
  }, [selectedDate, chartData, projectIds, totalByProject]);

  const detailTotal = useMemo(
    () => detailItems.reduce((sum, item) => sum + item.value, 0),
    [detailItems],
  );

  if (
    !rows.some((row) => {
      if ("metrics" in row) {
        return row.metrics.some(({ value }) => value > 0);
      }
      return row.value > 0;
    })
  ) {
    return <UsageNoDataError />;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="h-56">
        <DailyChart
          data={chartData}
          quantityType={quantityType}
          showCategoryInTooltip
          colorMap={colorMap}
          yAxisWidth={quantityType === "actionCompute" ? 80 : 60}
          hideTooltip={selectedDate !== null}
          customTooltip={(props) => (
            <ProjectChartTooltip
              {...props}
              quantityType={quantityType}
              colorMap={colorMap}
            />
          )}
        >
          {projectIds.map((projectId, index) => {
            const color = PROJECT_COLORS[index % PROJECT_COLORS.length];

            return (
              <Bar
                key={projectId}
                dataKey={`project_${projectId}`}
                className={color}
                name={` `} // Space for consistent tooltip formatting
                barSize={chartData.length === 1 ? SINGLE_BAR_WIDTH : undefined}
                isAnimationActive={false}
                stackId="stack"
                style={{ cursor: "pointer" }}
                tabIndex={0}
                onClick={(data) => {
                  if (typeof data.payload?.dateNumeric === "number") {
                    setSelectedDate(
                      data.payload.dateNumeric === selectedDate
                        ? null
                        : data.payload.dateNumeric,
                    );
                  }
                }}
                onKeyDown={(data, _idx, event) => {
                  if (event?.key === "Enter") {
                    if (typeof data.payload?.dateNumeric === "number") {
                      setSelectedDate(
                        data.payload.dateNumeric === selectedDate
                          ? null
                          : data.payload.dateNumeric,
                      );
                    }
                  }
                }}
                shape={(props: any) => {
                  const { dateNumeric } = props;
                  const isDimmed =
                    selectedDate !== null && selectedDate !== dateNumeric;

                  return <Rectangle {...props} opacity={isDimmed ? 0.3 : 1} />;
                }}
              />
            );
          })}
          {selectedDate !== null && (
            <ReferenceArea
              x1={selectedDate - MS_IN_DAY / 2}
              x2={selectedDate + MS_IN_DAY / 2}
              fill="currentColor"
              fillOpacity={0.06}
              ifOverflow="hidden"
            />
          )}
        </DailyChart>
      </div>

      <div className="flex flex-col gap-1">
        {detailItems
          .filter((item) => (item.sortValue ?? item.value) > 0)
          .sort((a, b) => (b.sortValue ?? b.value) - (a.sortValue ?? a.value))
          .map((item, index) => (
            <ProjectDetailRow
              key={index}
              item={item}
              total={detailTotal}
              quantityType={quantityType}
            />
          ))}
      </div>
    </div>
  );
}
