import classNames from "classnames";
import { format } from "date-fns";
import { useState, useRef, useEffect } from "react";
import { createPortal } from "react-dom";
import {
  ResponsiveContainer,
  LineChart,
  XAxis,
  YAxis,
  Legend,
  Tooltip,
  Line,
  ReferenceLine,
  CartesianGrid,
} from "recharts";
import { ChartTooltip } from "@common/elements/ChartTooltip";
import { useDeploymentAuditLogs } from "@common/lib/useDeploymentAuditLog";
import { timeLabelForMinute, formatNumberCompact } from "@common/lib/format";
import { ChartData } from "@common/lib/charts/types";
import { DeploymentTimes } from "@common/features/health/components/DeploymentTimes";
import { Button } from "@ui/Button";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { LoadingTransition } from "@ui/Loading";
import { Spinner } from "@ui/Spinner";

function SubscriptionInvalidationLabel({
  dataKey,
  maxChars = 24,
}: {
  dataKey: string;
  maxChars?: number;
}) {
  // Keys are either "mutation:table" (health page) or just "table" (function page).
  const lastColon = dataKey.lastIndexOf(":");
  if (lastColon === -1) {
    // Just a table name
    return <span>{dataKey}</span>;
  }
  const mutation = dataKey.substring(0, lastColon);
  const table = dataKey.substring(lastColon + 1);
  return (
    <span className="flex items-center gap-1">
      <span className="truncate">
        <FunctionNameOption
          maxChars={maxChars}
          label={functionIdentifierValue(mutation)}
        />
      </span>
      <span className="shrink-0 text-content-secondary">→</span>
      <span className="shrink-0">{table}</span>
    </span>
  );
}

function PortalTooltip({
  active,
  payload,
  label,
  coordinate,
  content,
  chartRef,
}: any) {
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [tooltipSize, setTooltipSize] = useState<{
    width: number;
    height: number;
  } | null>(null);

  // Measure tooltip size after render
  useEffect(() => {
    if (tooltipRef.current) {
      const { width, height } = tooltipRef.current.getBoundingClientRect();
      setTooltipSize({ width, height });
    }
  }, [active, payload, label]);

  if (!active || !coordinate) return null;

  // The coordinate is relative to the chart SVG, so we need to convert it to viewport coordinates
  // by adding the chart's position on the page
  const rect = chartRef?.current?.getBoundingClientRect();

  const offset = 10;
  let left = (rect?.left || 0) + coordinate.x + offset;
  let top = (rect?.top || 0) + coordinate.y + offset;

  // Use actual tooltip dimensions if available, otherwise use estimates
  const tooltipWidth = tooltipSize?.width || 250;
  const tooltipHeight = tooltipSize?.height || 150;

  // Check if tooltip would overflow right edge
  if (left + tooltipWidth > window.innerWidth) {
    // Position to the left of cursor, with right edge at cursor position
    left = (rect?.left || 0) + coordinate.x - tooltipWidth;
  }

  // Check if tooltip would overflow bottom edge
  if (top + tooltipHeight > window.innerHeight) {
    top = (rect?.top || 0) + coordinate.y - tooltipHeight;
  }

  // Ensure tooltip doesn't go off left edge
  if (left < offset) {
    left = offset;
  }

  // Ensure tooltip doesn't go off top edge
  if (top < offset) {
    top = offset;
  }

  return createPortal(
    <div
      ref={tooltipRef}
      style={{
        position: "fixed",
        left,
        top,
        pointerEvents: "none",
        zIndex: 9999,
        fontSize: "11px",
      }}
    >
      {content({ active, payload, label })}
    </div>,
    document.body,
  );
}

export function ChartForFunctionRate({
  chartData,
  kind,
}: {
  chartData: ChartData | undefined | null;
  kind:
    | "cacheHitRate"
    | "failureRate"
    | "schedulerStatus"
    | "functionConcurrency"
    | "functionCalls"
    | "subscriptionInvalidations";
}) {
  const [shown, setShown] = useState<string | null>(null);
  const [startDate] = useState(new Date(Date.now() - 3600 * 1000));
  const chartRef = useRef<HTMLDivElement>(null);
  const auditLogs = useDeploymentAuditLogs(startDate.getTime(), {
    actions: ["push_config", "push_config_with_components"],
  });
  const deploysByMinute = auditLogs?.map(
    (log) => {
      const creationDate = new Date(log._creationTime);

      return {
        hour: format(creationDate, "h:mm a"),
        timestamp: log._creationTime,
      };
    },
    {} as Record<string, { hour: string; timestamp: number }[]>,
  );
  return (
    <div className="size-full min-h-52 **:outline-none!">
      <LoadingTransition
        loadingProps={{
          fullHeight: false,
          className: "h-full w-full",
          shimmer: false,
        }}
        loadingState={
          <div className="flex size-full items-center justify-center">
            <Spinner className="m-auto size-12" />
          </div>
        }
      >
        {chartData === null ? (
          <div className="flex h-45 w-full items-center justify-center px-12 text-center text-sm text-content-secondary">
            {`Data will appear here as your ${kind === "cacheHitRate" ? "queries" : "functions"} are called.`}
          </div>
        ) : chartData === undefined ? null : (
          <div ref={chartRef} style={{ width: "99%", height: "99%" }}>
            <ResponsiveContainer width="100%" height="100%">
              <LineChart
                data={chartData.data}
                style={{
                  fontSize: 11,
                }}
              >
                {/* Show a reference line for each time bucket that had a deployment */}
                {deploysByMinute?.map(({ hour, timestamp }) => (
                  <ReferenceLine
                    key={timestamp}
                    x={hour}
                    stroke="var(--brand-yellow)"
                    strokeDasharray="3 3"
                  />
                ))}
                <XAxis
                  axisLine={{ className: "stroke-content-tertiary/30" }}
                  tickLine={false}
                  dataKey="time"
                  strokeWidth={1}
                  domain={["auto", "auto"]}
                  minTickGap={25}
                  tick={{ fontSize: 11, fill: "currentColor" }}
                />
                <YAxis
                  axisLine={{ className: "stroke-content-tertiary/30" }}
                  tickLine={false}
                  width="auto"
                  tickFormatter={(value) =>
                    kind === "functionCalls" ||
                    kind === "subscriptionInvalidations"
                      ? formatNumberCompact(value as number)
                      : kind === "schedulerStatus" ||
                          kind === "functionConcurrency"
                        ? value
                        : `${value.toFixed((value as number) % 1 === 0 ? 0 : 2)}%`
                  }
                  domain={
                    kind !== "schedulerStatus" &&
                    kind !== "functionConcurrency" &&
                    kind !== "functionCalls" &&
                    kind !== "subscriptionInvalidations"
                      ? [0, 100]
                      : undefined
                  }
                  interval={
                    kind === "schedulerStatus" ||
                    kind === "functionConcurrency" ||
                    kind === "functionCalls" ||
                    kind === "subscriptionInvalidations"
                      ? 0
                      : undefined
                  }
                  allowDecimals={
                    kind !== "functionCalls" &&
                    kind !== "subscriptionInvalidations"
                  }
                  tick={{ fontSize: 11, fill: "currentColor" }}
                />
                <Legend
                  align="left"
                  verticalAlign="bottom"
                  iconType="plainline"
                  iconSize={12}
                  layout="horizontal"
                  content={() => (
                    <div className="flex flex-wrap gap-x-3 gap-y-1 pt-1">
                      {chartData.lineKeys.map((line) => (
                        <Button
                          variant="unstyled"
                          key={line.key}
                          className={classNames(
                            "flex items-center gap-1 transition-opacity text-content-primary",
                            shown === line.key || shown === null
                              ? "opacity-100"
                              : "opacity-50",
                          )}
                          onClick={() =>
                            shown === line.key
                              ? setShown(null)
                              : setShown(line.key)
                          }
                        >
                          <svg className="w-3" viewBox="0 0 12 12" aria-hidden>
                            <line
                              x1="0"
                              y1="6"
                              x2="12"
                              y2="6"
                              stroke={line.color}
                              strokeWidth="2"
                            />
                          </svg>
                          {line.key === "_rest" ? (
                            kind === "subscriptionInvalidations" ? (
                              "Other"
                            ) : (
                              `All${[].length > 1 ? " other" : ""} ${kind === "cacheHitRate" ? "queries" : "functions"}`
                            )
                          ) : kind === "schedulerStatus" ? (
                            "Lag Time (minutes)"
                          ) : kind === "functionConcurrency" ? (
                            line.key
                          ) : kind === "subscriptionInvalidations" ? (
                            <SubscriptionInvalidationLabel
                              dataKey={line.key}
                              maxChars={24}
                            />
                          ) : (
                            <FunctionNameOption
                              maxChars={24}
                              label={line.key}
                            />
                          )}
                        </Button>
                      ))}
                    </div>
                  )}
                />

                <Tooltip
                  animationDuration={100}
                  content={(props) => (
                    <PortalTooltip
                      {...props}
                      chartRef={chartRef}
                      content={({ active, payload, label }: any) => {
                        const deploymentTimes = deploysByMinute
                          ?.filter((deploy) => deploy.hour === label)
                          .map((deploy) =>
                            format(new Date(deploy.timestamp), "pp"),
                          );
                        return (
                          <ChartTooltip
                            active={active}
                            payload={payload
                              ?.filter(
                                ({ dataKey }: any) =>
                                  shown === dataKey || shown === null,
                              )
                              .sort((a: any, b: any) =>
                                a.dataKey === "_rest"
                                  ? 1
                                  : b.dataKey === "_rest"
                                    ? -1
                                    : 0,
                              )
                              .map((dataPoint: any) => ({
                                ...dataPoint,
                                formattedValue: (
                                  <span className="flex flex-1 items-center justify-between gap-2">
                                    <div className="flex-1 truncate">
                                      {dataPoint.dataKey === "_rest" ? (
                                        kind === "subscriptionInvalidations" ? (
                                          "Other"
                                        ) : (
                                          `All${payload.length > 1 ? " other" : ""} ${kind === "cacheHitRate" ? "queries" : "functions"}`
                                        )
                                      ) : kind === "schedulerStatus" ? (
                                        "Lag Time"
                                      ) : kind === "functionConcurrency" ? (
                                        (dataPoint.dataKey as string)
                                      ) : kind ===
                                        "subscriptionInvalidations" ? (
                                        <SubscriptionInvalidationLabel
                                          dataKey={dataPoint.dataKey as string}
                                          maxChars={24}
                                        />
                                      ) : (
                                        <FunctionNameOption
                                          maxChars={24}
                                          label={dataPoint.dataKey as string}
                                        />
                                      )}
                                    </div>
                                    <div className="shrink-0 text-right">
                                      {kind === "schedulerStatus"
                                        ? `${(dataPoint.value as number).toLocaleString()} minutes`
                                        : kind === "functionConcurrency" ||
                                            kind === "functionCalls"
                                          ? `${formatNumberCompact(dataPoint.value as number)} ${(dataPoint.value as number) === 1 ? "call" : "calls"}`
                                          : kind === "subscriptionInvalidations"
                                            ? `${formatNumberCompact(dataPoint.value as number)} ${(dataPoint.value as number) === 1 ? "invalidation" : "invalidations"}`
                                            : `${(
                                                dataPoint.value as number
                                              ).toFixed(
                                                (dataPoint.value as number) %
                                                  1 ===
                                                  0
                                                  ? 0
                                                  : 2,
                                              )}%`}
                                    </div>
                                  </span>
                                ),
                              }))}
                            extraContent={
                              <DeploymentTimes
                                deploymentTimes={deploymentTimes}
                              />
                            }
                            label={timeLabelForMinute(label)}
                            showLegend
                          />
                        );
                      }}
                    />
                  )}
                />
                <CartesianGrid
                  className="stroke-content-tertiary/30"
                  horizontal
                  strokeWidth={1}
                  syncWithTicks
                />

                {chartData.lineKeys.map((line) => {
                  const dataKey = line.key;
                  const { name } = line;
                  const { color } = line;
                  return (
                    <Line
                      isAnimationActive={false}
                      strokeWidth={1.5}
                      activeDot={{
                        r: 4,
                        className: "stroke-none",
                        display:
                          shown === dataKey || shown === null
                            ? "block"
                            : "none",
                      }}
                      key={dataKey}
                      dataKey={dataKey}
                      name={name}
                      min={0}
                      display={
                        shown === dataKey || shown === null ? "block" : "none"
                      }
                      className="transition-opacity"
                      stroke={color}
                      fillOpacity={1}
                      fill={`url(#${dataKey})`}
                      dot={false}
                    />
                  );
                })}
              </LineChart>
            </ResponsiveContainer>
          </div>
        )}
      </LoadingTransition>
    </div>
  );
}
