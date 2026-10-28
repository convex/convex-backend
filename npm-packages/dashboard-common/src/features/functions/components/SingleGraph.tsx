import React, { JSX, useEffect, useState } from "react";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
} from "recharts";

import { ChartDataSource, ChartData } from "@common/lib/charts/types";
import { HealthCard } from "@common/elements/HealthCard";
import { ChartTooltip } from "@common/elements/ChartTooltip";
import { timeLabelForMinute } from "@common/lib/format";
import { LoadingTransition } from "@ui/Loading";

export function SingleGraph({
  title,
  dataSource,
  data,
  syncId,
}: {
  title: "Cache Hit Rate" | "Function Calls" | "Errors" | "Execution Time";
  syncId?: string;
} & (
  | { dataSource: ChartDataSource; data?: undefined }
  | { dataSource?: undefined; data: ChartData }
)): JSX.Element {
  const [chartData, setChartData] = useState<ChartData | undefined>(data);

  useEffect(() => {
    async function getChartData() {
      if (!dataSource) return;
      const initEndDate = new Date();
      const initStartDate = new Date(initEndDate);
      initStartDate.setHours(initStartDate.getHours() - 1);
      setChartData(await dataSource(initStartDate, initEndDate));
    }
    void getChartData();
  }, [dataSource]);

  const force = Math.random();

  return (
    <HealthCard title={title}>
      <div className="min-h-52 w-full">
        <LoadingTransition
          loadingProps={{
            fullHeight: false,
            className: "h-[13rem]",
          }}
        >
          {chartData && (
            <ResponsiveContainer width="100%" height="100%">
              <LineChart
                syncId={syncId}
                key={force}
                data={chartData.data}
                style={{
                  fontSize: 11,
                  left: -24,
                }}
              >
                <XAxis
                  dataKey="time"
                  axisLine={{ className: "stroke-content-tertiary/30" }}
                  tickLine={false}
                  strokeWidth={1}
                  domain={["auto", "auto"]}
                  minTickGap={25}
                  tick={{ fontSize: 11, fill: "currentColor" }}
                />
                <YAxis
                  axisLine={{ className: "stroke-content-tertiary/30" }}
                  tickLine={false}
                  ticks={
                    title === "Cache Hit Rate"
                      ? [0, 25, 50, 75, 100]
                      : undefined
                  }
                  tickFormatter={(value) =>
                    new Intl.NumberFormat("en-US", {
                      notation: "compact",
                      compactDisplay: "short",
                    }).format(value) + (title === "Cache Hit Rate" ? "%" : "")
                  }
                  tick={{ fontSize: 11, fill: "currentColor" }}
                  width={60}
                />
                <Tooltip
                  content={({ active, payload, label }) => (
                    <ChartTooltip
                      active={active}
                      payload={payload}
                      label={timeLabelForMinute(label)}
                      showLegend
                    />
                  )}
                  animationDuration={100}
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
                      strokeWidth={1}
                      activeDot={{ r: 4, className: "stroke-none" }}
                      key={dataKey}
                      dataKey={dataKey}
                      name={name}
                      min={0}
                      stroke={color}
                      fillOpacity={1}
                      fill={`url(#${dataKey})`}
                      dot={false}
                    />
                  );
                })}
              </LineChart>
            </ResponsiveContainer>
          )}
        </LoadingTransition>
      </div>
    </HealthCard>
  );
}
