import * as Sentry from "@sentry/nextjs";
import { DailyMetric, DailyPerTagMetrics } from "hooks/usageMetrics";
import { Bar, Rectangle, ReferenceArea } from "recharts";
import { ReactNode, useMemo } from "react";
import groupBy from "lodash/groupBy";
import sumBy from "lodash/sumBy";
import { toNumericUTC } from "@common/lib/format";
import { QuantityType } from "./lib/formatQuantity";
import { UsageNoDataError } from "./TeamUsageError";
import { DailyChart } from "./DailyChart";
import { InlineDetailList, InlineDetailItem } from "./InlineDetailList";

// When there is only a data point, we have to set the bar width manually to make it appear (https://github.com/recharts/recharts/issues/3640).
// This value has been measured manually on a desktop screen size, but it should also look good in other contexts where there is only one bar.
const SINGLE_BAR_WIDTH = 91;

const MS_IN_DAY = 24 * 60 * 60 * 1000;

export function UsageStackedBarChart({
  rows,
  categories,
  categoryRenames = {},
  quantityType = "unit",
  isGauge = false,
  showEmptyCategories = false,
  selectedDate,
  setSelectedDate,
  note,
}: {
  rows: DailyPerTagMetrics[];
  categories: {
    [tag: string]: {
      name: string;
      color: string;
      tip?: string;
    };
  };
  // Merge multiple categories together (e.g. to count cached and uncached queries together)
  categoryRenames?: { [sourceDataName: string]: string };
  quantityType?: QuantityType;
  /** If true, total shows the most recent day's value instead of summing all days */
  isGauge?: boolean;
  /**
   * List every category in the legend, including ones with a zero value. Use
   * for fixed category sets where the full breakdown should always be visible.
   */
  showEmptyCategories?: boolean;
  selectedDate: number | null;
  setSelectedDate: (date: number | null) => void;
  note?: ReactNode;
}) {
  const chartData = useMemo(() => {
    const filledData = [];
    const dateSet = new Set(rows.map(({ ds }) => toNumericUTC(ds)));

    // Find the range of dates
    const minDate = Math.min(...Array.from(dateSet));
    const maxDate = Math.max(...Array.from(dateSet));

    // Fill in the missing dates
    for (let date = minDate; date <= maxDate; date += MS_IN_DAY) {
      const row = rows.find(({ ds }) => toNumericUTC(ds) === date);
      filledData.push({
        dateNumeric: date,
        ...(row
          ? Object.fromEntries(
              Object.entries(
                groupBy(row.metrics, (metric) => {
                  const tag = categoryRenames[metric.tag] ?? metric.tag;
                  if (!(tag in categories)) {
                    Sentry.captureMessage(
                      `Unexpected call tag "${tag}"`,
                      "error",
                    );
                  }
                  return tag;
                }),
              ).map(([key, keyMetrics]) => [
                key,
                sumBy(keyMetrics, (metric) => metric.value),
              ]),
            )
          : Object.fromEntries(Object.keys(categories).map((key) => [key, 0]))),
      });
    }

    return filledData;
  }, [rows, categoryRenames, categories]);

  const totalByTag = useMemo(() => {
    if (isGauge) {
      // For gauge metrics (like storage), show the most recent day's value
      const lastDay = chartData[chartData.length - 1];
      if (lastDay) {
        return Object.fromEntries(
          Object.keys(categories).map((tag) => [
            tag,
            ((lastDay as any)[tag] as number) || 0,
          ]),
        );
      }
    }
    return Object.fromEntries(
      Object.entries(
        groupBy(
          rows.flatMap((row) => row.metrics),
          (metric) => categoryRenames[metric.tag] ?? metric.tag,
        ),
      ).map(([tag, entries]) => [tag, sumBy(entries, (entry) => entry.value)]),
    );
  }, [rows, categoryRenames, isGauge, chartData, categories]);

  // Items for the inline list: show per-category breakdown if multiple categories, single total otherwise
  const detailItems = useMemo((): InlineDetailItem[] => {
    const categoryEntries = Object.entries(categories);
    const hasMultipleCategories = categoryEntries.length > 1;

    if (hasMultipleCategories) {
      if (selectedDate !== null) {
        const dataPoint = chartData.find((d) => d.dateNumeric === selectedDate);
        if (dataPoint) {
          return categoryEntries.map(([tag, { name, color, tip }]) => ({
            name,
            value: ((dataPoint as any)[tag] as number) || 0,
            sortValue: totalByTag[tag] || 0,
            color,
            tip,
          }));
        }
      }
      return categoryEntries.map(([tag, { name, color, tip }]) => ({
        name,
        value: totalByTag[tag] || 0,
        color,
        tip,
      }));
    }

    // Single category: show a total
    if (selectedDate !== null) {
      const dataPoint = chartData.find((d) => d.dateNumeric === selectedDate);
      if (dataPoint) {
        const total = Object.keys(categories).reduce(
          (sum, tag) => sum + (((dataPoint as any)[tag] as number) || 0),
          0,
        );
        return [{ name: "Total", value: total, color: "fill-chart-line-1" }];
      }
    }

    const total = Object.values(totalByTag).reduce((sum, val) => sum + val, 0);
    return [{ name: "Total", value: total, color: "fill-chart-line-1" }];
  }, [selectedDate, chartData, categories, totalByTag]);

  const colorMap = useMemo(
    () =>
      new Map(
        Object.entries(categories).map(([tag, { color }]) => [tag, color]),
      ),
    [categories],
  );

  if (!rows.some(({ metrics }) => metrics.some(({ value }) => value > 0))) {
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
        >
          {Object.entries(categories).map(([tag, { name, color }]) => (
            <Bar
              key={tag}
              dataKey={tag}
              className={color}
              name={name}
              barSize={chartData.length === 1 ? SINGLE_BAR_WIDTH : undefined}
              isAnimationActive={false}
              stackId="stack"
              style={{ cursor: "pointer" }}
              onClick={(data: any) => {
                if (data?.dateNumeric) {
                  setSelectedDate(
                    data.dateNumeric === selectedDate ? null : data.dateNumeric,
                  );
                }
              }}
              shape={(props: any) => {
                const { dateNumeric, name: categoryName } = props;
                if (
                  typeof dateNumeric !== "number" ||
                  typeof categoryName !== "string"
                ) {
                  Sentry.captureMessage(
                    "Invalid props in stacked bar",
                    "error",
                  );
                  return <Rectangle {...props} />;
                }

                const isDimmed =
                  selectedDate !== null && selectedDate !== dateNumeric;

                return <Rectangle {...props} opacity={isDimmed ? 0.3 : 1} />;
              }}
            />
          ))}
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

      {note}

      <InlineDetailList
        items={detailItems}
        quantityType={quantityType}
        showZeroValues={showEmptyCategories}
      />
    </div>
  );
}

export function UsageBarChart({
  rows,
  entity,
  quantityType = "unit",
}: {
  rows: DailyMetric[];
  entity: string;
  quantityType?: QuantityType;
}) {
  // Sort rows and convert date to numeric values
  const chartData = useMemo(() => {
    const filledData = [];
    const dateSet = new Set(rows.map(({ ds }) => toNumericUTC(ds)));

    // Find the range of dates
    const minDate = Math.min(...Array.from(dateSet));
    const maxDate = Math.max(...Array.from(dateSet));

    // Fill in the missing dates
    for (let date = minDate; date <= maxDate; date += MS_IN_DAY) {
      const row = rows.find(({ ds }) => toNumericUTC(ds) === date);
      filledData.push({
        dateNumeric: date,
        value: row ? row.value : 0,
      });
    }

    return filledData;
  }, [rows]);

  if (!rows.some(({ value }) => value > 0)) {
    return <UsageNoDataError />;
  }

  return (
    <div className="h-48 animate-fadeInFromLoading">
      <DailyChart
        data={chartData}
        quantityType={quantityType}
        yAxisWidth={quantityType === "actionCompute" ? 80 : 60}
      >
        <Bar
          dataKey="value"
          isAnimationActive={false}
          className="fill-chart-line-1"
          name={quantityType === "unit" ? entity : "bytes"}
          barSize={chartData.length === 1 ? SINGLE_BAR_WIDTH : undefined}
          minPointSize={4}
        />
      </DailyChart>
    </div>
  );
}
