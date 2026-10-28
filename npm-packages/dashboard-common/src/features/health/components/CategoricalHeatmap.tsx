import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { format } from "date-fns";
import {
  FunctionRateHeatmapData,
  useTopKFunctionRateHeatmap,
} from "@common/lib/appMetrics";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import {
  displayName,
  functionIdentifierFromValue,
} from "@common/lib/functions/generateFileTree";
import { Tooltip } from "@ui/Tooltip";
import { LoadingTransition } from "@ui/Loading";
import { Spinner } from "@ui/Spinner";
import { Button } from "@ui/Button";

export type CategoricalHeatmapKind = "cacheHitRate" | "failureRate";

// 10-stop viridis ramp. Perceptually uniform and colorblind-safe.
const VIRIDIS_STOPS = [
  "#440154",
  "#481A6C",
  "#472F7D",
  "#404388",
  "#38578C",
  "#2E6B8E",
  "#238E8D",
  "#1E9E89",
  "#4FB870",
  "#B4DE2C",
] as const;

type KindConfig = {
  legendLabel: string;
  tooltipLabel: string;
  emptyMessage: string;
  restLabel: (isRestOnly: boolean) => string;
  restAccessibleLabel: (isRestOnly: boolean) => string;
  accessibleMetricName: string;
  // For cache hit rate, higher is better (0% = dark, 100% = light). For
  // failure rate we invert so 0% = light (good) and 100% = dark (bad).
  valueToRampIndex: (value: number) => number;
  // Legend ends — "0%" on the left always, but which end is "good" flips.
  legendLowLabel: string;
  legendHighLabel: string;
  // Row sort direction: "worst first" means ascending for cache hit rate
  // (lowest hit % is worst) and descending for failure rate (highest fail %
  // is worst). `_rest` is sorted alongside the other rows by its own avg.
  sortDirection: "ascending" | "descending";
};

const KIND_CONFIG: Record<CategoricalHeatmapKind, KindConfig> = {
  cacheHitRate: {
    legendLabel: "Hit rate",
    tooltipLabel: "Hit rate",
    emptyMessage: "Data will appear here as your queries are called.",
    restLabel: (isRestOnly) => (isRestOnly ? "All Queries" : "Other"),
    restAccessibleLabel: (isRestOnly) =>
      isRestOnly ? "all queries" : "all other queries",
    accessibleMetricName: "cache hit rate",
    valueToRampIndex: (v) =>
      Math.min(VIRIDIS_STOPS.length - 1, Math.floor((v / 100) * 10)),
    legendLowLabel: "0%",
    legendHighLabel: "100%",
    sortDirection: "ascending",
  },
  failureRate: {
    legendLabel: "Failure rate",
    tooltipLabel: "Failure rate",
    emptyMessage: "Data will appear here as your functions run.",
    restLabel: (isRestOnly) => (isRestOnly ? "All Functions" : "Other"),
    restAccessibleLabel: (isRestOnly) =>
      isRestOnly ? "all functions" : "all other functions",
    accessibleMetricName: "failure rate",
    valueToRampIndex: (v) =>
      Math.min(VIRIDIS_STOPS.length - 1, Math.floor(((100 - v) / 100) * 10)),
    legendLowLabel: "0%",
    legendHighLabel: "100%",
    sortDirection: "descending",
  },
};

// Rows with no data (all null cells) sort last regardless of direction.
function rowAvg(row: FunctionRateHeatmapData["rows"][number]): number | null {
  let sum = 0;
  let count = 0;
  for (const cell of row.cells) {
    if (cell.value !== null) {
      sum += cell.value;
      count += 1;
    }
  }
  return count === 0 ? null : sum / count;
}

function sortRows(
  rows: FunctionRateHeatmapData["rows"],
  direction: "ascending" | "descending",
): FunctionRateHeatmapData["rows"] {
  return [...rows].sort((a, b) => {
    const avgA = rowAvg(a);
    const avgB = rowAvg(b);
    if (avgA === null && avgB === null) return 0;
    if (avgA === null) return 1;
    if (avgB === null) return -1;
    return direction === "ascending" ? avgA - avgB : avgB - avgA;
  });
}

// Target cell width for the heatmap, in px. The actual cell width depends on
// how many cells fit in the available cells column; we pick `numBuckets` so the
// per-cell width lands as close to this as possible.
const TARGET_CELL_WIDTH_PX = 30;
// Approximate label-column footprint (label width + grid gap + container
// horizontal padding) used to estimate the cells column width before render.
// Slightly conservative — it's better to undershoot and get cells a bit wider
// than 30px than to overshoot and crush the labels.
const LABEL_COLUMN_ALLOWANCE_PX = 180;
// Backend caps `num_buckets` at 60 (one cell per minute over the 60-minute
// window — finer than the 1-minute counter buckets causes aliasing).
const MAX_BUCKETS = 60;
const MIN_BUCKETS = 4;
// Initial top-K and the step each "View more" click adds. Backend caps k at
// 25, which is exactly 5 × 5.
const INITIAL_K = 5;
const K_STEP = 5;
const MAX_K = 25;

const METRIC_TO_HEATMAP_KIND: Record<
  "cacheHitPercentage" | "failurePercentage",
  CategoricalHeatmapKind
> = {
  cacheHitPercentage: "cacheHitRate",
  failurePercentage: "failureRate",
};

export function FunctionRateHeatmapView({
  metricKind,
}: {
  metricKind: "cacheHitPercentage" | "failurePercentage";
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [numBuckets, setNumBuckets] = useState(12);
  const [k, setK] = useState(INITIAL_K);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return undefined;
    const observer = new ResizeObserver(([entry]) => {
      const cellsWidth = Math.max(
        0,
        entry.contentRect.width - LABEL_COLUMN_ALLOWANCE_PX,
      );
      const desired = Math.round(cellsWidth / TARGET_CELL_WIDTH_PX);
      setNumBuckets(Math.max(MIN_BUCKETS, Math.min(MAX_BUCKETS, desired)));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const data = useTopKFunctionRateHeatmap(metricKind, k, numBuckets);
  // Only offer "View more" when the backend returned a full page — if it
  // returned fewer rows than we asked for, we already have every function.
  // Same signal lets us drop the `_rest` aggregate: if `nonRestRowCount < k`
  // then no functions exist outside the page, so `_rest` is redundant.
  // When `_rest` is the only row it's the "all queries" aggregate, so keep it.
  const nonRestRowCount =
    data?.rows.filter((r) => r.key !== "_rest").length ?? 0;
  const canViewMore = k < MAX_K && nonRestRowCount >= k;
  const shouldHideRest = nonRestRowCount > 0 && nonRestRowCount < k;
  const displayData =
    shouldHideRest && data
      ? { ...data, rows: data.rows.filter((r) => r.key !== "_rest") }
      : data;
  return (
    <div ref={containerRef} className="flex size-full">
      <CategoricalHeatmap
        data={displayData}
        kind={METRIC_TO_HEATMAP_KIND[metricKind]}
        onViewMore={
          canViewMore
            ? () => setK((prev) => Math.min(prev + K_STEP, MAX_K))
            : undefined
        }
      />
    </div>
  );
}

export function CategoricalHeatmap({
  data,
  kind,
  onViewMore,
}: {
  data: FunctionRateHeatmapData | undefined | null;
  kind: CategoricalHeatmapKind;
  onViewMore?: () => void;
}) {
  const config = KIND_CONFIG[kind];
  return (
    <div className="flex size-full min-h-52 flex-col">
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
        {data === null ? (
          <div className="flex size-full items-center justify-center px-12 text-center text-sm text-content-secondary">
            {config.emptyMessage}
          </div>
        ) : data === undefined ? null : (
          <HeatmapGrid data={data} config={config} onViewMore={onViewMore} />
        )}
      </LoadingTransition>
    </div>
  );
}

// Char-truncation bounds for function-name labels. Below MIN, names are too
// short to be useful; above MAX, the column wastes space.
const MIN_LABEL_CHARS = 8;
const MAX_LABEL_CHARS = 28;
// Upper bound for the width of a font-mono text-xs character, in px. Used
// both to cap how many chars fit in the label budget and to size the label
// column. Must slightly over-estimate real char width (~7.2px on macOS) so
// the column is a touch wider than the rendered text and the outer
// `truncate` doesn't clip the last character.
const MONO_CHAR_PX = 7.5;
// Minimum px we want to reserve for the cells column so the heatmap stays
// readable even when function names are long.
const MIN_CELLS_PX = 220;

function HeatmapGrid({
  data,
  config,
  onViewMore,
}: {
  data: FunctionRateHeatmapData;
  config: KindConfig;
  onViewMore?: () => void;
}) {
  const isRestOnly = data.rows.length === 1 && data.rows[0].key === "_rest";
  const sortedRows = useMemo(
    () => sortRows(data.rows, config.sortDirection),
    [data.rows, config.sortDirection],
  );
  // "View more" sits in the time-axis row's label slot so showing the button
  // doesn't cost us a data row.
  const showViewMore = Boolean(onViewMore);
  const { bucketStartTimes } = data;
  const first = bucketStartTimes[0];
  const last = bucketStartTimes[bucketStartTimes.length - 1];
  const middle = bucketStartTimes[Math.floor(bucketStartTimes.length / 2)];
  // Bucket end is the next bucket's start; the trailing bucket assumes the
  // same width as the rest of the series.
  const bucketDurationMs =
    bucketStartTimes.length >= 2
      ? bucketStartTimes[1].getTime() - bucketStartTimes[0].getTime()
      : 0;

  const containerRef = useRef<HTMLDivElement>(null);
  const [maxChars, setMaxChars] = useState(MAX_LABEL_CHARS);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return undefined;
    const observer = new ResizeObserver(([entry]) => {
      const width = entry.contentRect.width;
      const labelBudget = Math.max(0, width - MIN_CELLS_PX);
      const chars = Math.floor(labelBudget / MONO_CHAR_PX);
      setMaxChars(Math.max(MIN_LABEL_CHARS, Math.min(MAX_LABEL_CHARS, chars)));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Fade the scroll container's top/bottom edges only when there's content
  // hidden past them, so the mask acts as a scroll affordance rather than
  // always-on decoration. Recompute on scroll and on content/size changes.
  const scrollRef = useRef<HTMLDivElement>(null);
  const [fadeTop, setFadeTop] = useState(false);
  const [fadeBottom, setFadeBottom] = useState(false);
  // Generous epsilon (4px) to absorb subpixel rounding and the scrollbar
  // gutter so the fade flips off cleanly when you're visually at the edge.
  const EPS = 4;
  const recomputeFade = (el: HTMLDivElement) => {
    const { scrollTop, scrollHeight, clientHeight } = el;
    setFadeTop(scrollTop > EPS);
    setFadeBottom(scrollHeight - scrollTop - clientHeight > EPS);
  };
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return undefined;
    const update = () => recomputeFade(el);
    update();
    el.addEventListener("scroll", update, { passive: true });
    const observer = new ResizeObserver(update);
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);
    return () => {
      el.removeEventListener("scroll", update);
      observer.disconnect();
    };
  }, [sortedRows.length]);
  // On mount (and whenever row count changes), scroll to the bottom so the
  // user sees the last (worst-performing) rows first. Without this, the
  // default scrollTop=0 would land us at the top with the bottom fade on.
  // We also sync the fade state in the same layout pass so the first paint
  // after scrolling doesn't flash the wrong fade.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    recomputeFade(el);
  }, [sortedRows.length]);
  // Fade to a translucent-black endpoint (rather than fully transparent) for a
  // softer affordance. 12px tall on a 144px container is ~8%.
  const FADE_PX = 12;
  const FADE_ALPHA = "rgba(0,0,0,0.35)";
  const maskImage = `linear-gradient(to bottom, ${
    fadeTop ? FADE_ALPHA : "black"
  } 0, black ${fadeTop ? `${FADE_PX}px` : "0"}, black ${
    fadeBottom ? `calc(100% - ${FADE_PX}px)` : "100%"
  }, ${fadeBottom ? FADE_ALPHA : "black"} 100%)`;

  // Rows and the timestamp row live in separate grids so only the rows can
  // scroll. Both grids use the same fixed label-column width so their cells
  // columns line up — an `auto` column would size independently per grid.
  const labelColumnPx = maxChars * MONO_CHAR_PX;
  const gridColumns = `${labelColumnPx}px minmax(0, 1fr)`;

  return (
    <div
      ref={containerRef}
      className="flex size-full flex-col gap-1 px-2 pt-1 pb-2"
    >
      <div
        ref={scrollRef}
        className="scrollbar flex scrollbar-gutter-stable flex-col overflow-y-auto pr-1"
        style={{ maskImage, WebkitMaskImage: maskImage, maxHeight: "9rem" }}
      >
        <div
          className="grid items-end gap-x-2"
          style={{ gridTemplateColumns: gridColumns, rowGap: "2px" }}
        >
          {sortedRows.map((row) => (
            <HeatmapRow
              key={row.key}
              row={row}
              isRestOnly={isRestOnly}
              bucketDurationMs={bucketDurationMs}
              maxChars={maxChars}
              config={config}
            />
          ))}
        </div>
      </div>
      {bucketStartTimes.length > 0 && (
        <div
          className="grid items-center gap-x-2"
          style={{ gridTemplateColumns: gridColumns }}
        >
          <div className="flex pb-1">
            {showViewMore ? <ViewMoreButton onClick={onViewMore!} /> : null}
          </div>
          <div className="flex justify-between pt-1 text-[11px] text-content-primary">
            <span>{format(first, "p")}</span>
            <span>{format(middle, "p")}</span>
            <span>{format(last, "p")}</span>
          </div>
        </div>
      )}
      <Legend config={config} />
    </div>
  );
}

function ViewMoreButton({ onClick }: { onClick: () => void }) {
  return (
    <Button
      variant="unstyled"
      onClick={onClick}
      aria-label="Show more functions"
      className="text-[11px] text-content-secondary underline hover:text-content-primary"
    >
      View more
    </Button>
  );
}

function HeatmapRow({
  row,
  isRestOnly,
  bucketDurationMs,
  maxChars,
  config,
}: {
  row: FunctionRateHeatmapData["rows"][number];
  isRestOnly: boolean;
  bucketDurationMs: number;
  maxChars: number;
  config: KindConfig;
}) {
  const accessibleLabel =
    row.key === "_rest"
      ? config.restAccessibleLabel(isRestOnly)
      : displayName(functionIdentifierFromValue(row.key).identifier);
  return (
    <>
      <div className="truncate text-left font-mono text-xs text-content-secondary">
        {row.key === "_rest" ? (
          <span>{config.restLabel(isRestOnly)}</span>
        ) : (
          <FunctionNameOption label={row.key} maxChars={maxChars} />
        )}
      </div>
      <div
        role="row"
        aria-label={`${config.tooltipLabel} for ${accessibleLabel}`}
        className="flex items-center gap-0.5"
      >
        {row.cells.map((cell, i) => (
          <Cell
            key={i}
            time={cell.time}
            value={cell.value}
            accessibleLabel={accessibleLabel}
            bucketDurationMs={bucketDurationMs}
            config={config}
          />
        ))}
      </div>
    </>
  );
}

function Cell({
  time,
  value,
  accessibleLabel,
  bucketDurationMs,
  config,
}: {
  time: Date;
  value: number | null;
  accessibleLabel: string;
  bucketDurationMs: number;
  config: KindConfig;
}) {
  const background =
    value === null
      ? "var(--background-tertiary)"
      : VIRIDIS_STOPS[config.valueToRampIndex(value)];
  const endTime = new Date(time.getTime() + bucketDurationMs);
  const range =
    bucketDurationMs > 0
      ? `${format(time, "p")} – ${format(endTime, "p")}`
      : format(time, "p");
  const formattedValue =
    value === null ? null : value.toFixed(value % 1 === 0 ? 0 : 1);
  const tip = (
    <span>
      <span className="font-medium">{range}</span>
      <br />
      {value === null
        ? "No data"
        : `${config.tooltipLabel}: ${formattedValue}%`}
    </span>
  );
  const ariaLabel =
    value === null
      ? `${accessibleLabel}, ${range}: no data`
      : `${accessibleLabel}, ${range}: ${config.accessibleMetricName} ${formattedValue}%`;

  return (
    <Tooltip
      tip={tip}
      side="bottom"
      asChild
      className="flex-1"
      contentClassName="pointer-events-none"
      disableHoverableContent
    >
      <div
        role="img"
        style={{ background, height: 22, borderRadius: 1 }}
        aria-label={ariaLabel}
      />
    </Tooltip>
  );
}

function Legend({ config }: { config: KindConfig }) {
  return (
    <div
      role="img"
      aria-label={`Color scale for ${config.accessibleMetricName}, ranging from ${config.legendLowLabel} to ${config.legendHighLabel}`}
      className="flex items-center gap-2 text-[11px] text-content-secondary"
    >
      <span className="shrink-0">{config.legendLabel}</span>
      <span className="shrink-0" aria-hidden="true">
        {config.legendLowLabel}
      </span>
      <div
        aria-hidden="true"
        className="flex h-3 w-full max-w-[220px] overflow-hidden rounded-sm"
      >
        {(() => {
          // The legend bar shows the color ramp aligned with the low/high
          // labels. For failure rate the mapping is inverted, so reverse
          // the ramp visually too.
          const stops =
            config.valueToRampIndex(100) === VIRIDIS_STOPS.length - 1
              ? VIRIDIS_STOPS
              : [...VIRIDIS_STOPS].reverse();
          return stops.map((color, i) => (
            <div key={i} className="flex-1" style={{ background: color }} />
          ));
        })()}
      </div>
      <span className="shrink-0" aria-hidden="true">
        {config.legendHighLabel}
      </span>
    </div>
  );
}
