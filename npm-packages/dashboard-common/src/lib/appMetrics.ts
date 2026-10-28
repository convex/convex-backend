import { format } from "date-fns";
import useSWR from "swr";
import {
  FunctionExecution,
  UdfType,
} from "system-udfs/convex/_system/frontend/common";
import { deploymentFetch } from "@common/lib/fetching";
import {
  useDeploymentIsDisconnected,
  useDeploymentUrl,
  useDeploymentAuthHeader,
} from "@common/lib/deploymentApi";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { ChartData } from "@common/lib/charts/types";

export type UdfMetric =
  | "invocations"
  | "errors"
  | "cacheHits"
  | "cacheMisses"
  | "subscriptionInvalidations";
export type TableMetric = "rowsRead" | "rowsWritten";

type TimeseriesResponse = [SerializedDate, number | null][];

export type Bucket = {
  // Start time for the bucket
  time: Date;
  metric: number | null;
};
export type Timeseries = Bucket[];

function parseDate(date: SerializedDate): Date {
  let unixTsMs = date.secs_since_epoch * 1000;
  unixTsMs += date.nanos_since_epoch / 1_000_000;
  return new Date(unixTsMs);
}

const responseToTimeseries = (resp: TimeseriesResponse) =>
  resp.map(([time, metricOut]) => ({
    time: parseDate(time),
    metric: metricOut,
  }));

interface SerializedDate {
  secs_since_epoch: number;
  nanos_since_epoch: number;
}

function serializeDate(date: Date): SerializedDate {
  const unixTsSeconds = date.getTime() / 1000;
  const secsSinceEpoch = Math.floor(unixTsSeconds);
  const nanosSinceEpoch = Math.floor((unixTsSeconds - secsSinceEpoch) * 1e9);
  return {
    secs_since_epoch: secsSinceEpoch,
    nanos_since_epoch: nanosSinceEpoch,
  };
}

function appMetricsUrl(deploymentUrl: string): string {
  return `${deploymentUrl}/api/app_metrics`;
}

export type RequestFilter = {
  sessionId: string;
  clientRequestCounter: number;
};

export async function streamFunctionLogs(
  deploymentUrl: string,
  authHeader: string,
  cursorMs: number,
  requestFilter: RequestFilter | "all",
  signal: AbortSignal,
): Promise<{ entries: FunctionExecution[]; newCursor: number }> {
  const searchParams = new URLSearchParams({
    cursor: cursorMs.toString(),
  });
  if (requestFilter !== "all") {
    searchParams.set(
      "clientRequestCounter",
      requestFilter.clientRequestCounter.toString(),
    );
    searchParams.set("sessionId", requestFilter.sessionId);
  }
  const url = `${appMetricsUrl(
    deploymentUrl,
  )}/stream_function_logs?${searchParams.toString()}`;
  const response = await fetch(url, {
    headers: { Authorization: authHeader, "Convex-Client": "dashboard-0.0.0" },
    signal,
  });
  if (!response.ok) {
    const text = await response.text();
    if (response.status === 403) {
      let message = text;
      try {
        const parsed = JSON.parse(text);
        if (typeof parsed.message === "string") {
          message = parsed.message;
        }
      } catch {
        // not JSON, use raw text
      }
      throw new PermissionDeniedError(message);
    }
    throw new Error(text);
  }
  return response.json();
}

export class PermissionDeniedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PermissionDeniedError";
  }
}

export function useSchedulerLag() {
  const url = "/api/app_metrics/scheduled_job_lag";
  const isDisconnected = useDeploymentIsDisconnected();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  const fetcher = async () => {
    const start = new Date(Date.now() - 60 * 60 * 1000); // 1 hour ago
    const end = new Date();
    const windowArgs = {
      start: serializeDate(start),
      end: serializeDate(end),
      num_buckets: 60,
    };
    const window = JSON.stringify(windowArgs);
    const params = { window };
    const queryString = new URLSearchParams(params).toString();
    return deploymentFetch([
      deploymentUrl,
      `${url}?${queryString}`,
      authHeader,
    ]);
  };

  const { data: d } = useSWR(isDisconnected ? null : url, fetcher, {
    refreshInterval: 60 * 1000,
  });
  if (!d) {
    return undefined;
  }
  const buckets = responseToTimeseries(d as TimeseriesResponse);
  const data = buckets.map((value) =>
    value.metric
      ? {
          time: format(value.time, "h:mm a"),
          lag: Math.round(value.metric / 60),
        }
      : {
          time: format(value.time, "h:mm a"),
          lag: 0,
        },
  );
  return {
    data,
    xAxisKey: "time",
    lineKeys: [{ key: "lag", name: "Lag", color: "var(--brand-yellow)" }],
  };
}

type LatencyMetricsResponse = [number, TimeseriesResponse][];
type TopKMetricsResponse = [string, TimeseriesResponse][];

const multiResponseToTimeSeries = (
  resp: LatencyMetricsResponse | TopKMetricsResponse,
) => {
  const out = new Map();
  resp.forEach(([key, timeseries]) => {
    out.set(key, responseToTimeseries(timeseries));
  });
  return out;
};

export function useTopKCacheKey(
  kind: "cacheHitPercentage" | "failurePercentage",
) {
  const deploymentUrl = useDeploymentUrl();
  const route =
    kind === "cacheHitPercentage"
      ? "cache_hit_percentage_top_k"
      : "failure_percentage_top_k";

  return `${deploymentUrl}/api/app_metrics/${route}`;
}

export function useTopKFunctionMetrics(
  kind: "cacheHitPercentage" | "failurePercentage",
  k: number = 3,
  numBuckets: number = 60,
) {
  const url = `/api/app_metrics/${kind === "cacheHitPercentage" ? "cache_hit_percentage_top_k" : "failure_percentage_top_k"}`;
  const cacheKey = `${useTopKCacheKey(kind)}?k=${k}&numBuckets=${numBuckets}`;
  const isDisconnected = useDeploymentIsDisconnected();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  const fetcher = async () => {
    const start = new Date(Date.now() - 60 * 60 * 1000); // 1 hour ago
    const end = new Date();
    const windowArgs = {
      start: serializeDate(start),
      end: serializeDate(end),
      num_buckets: numBuckets,
    };
    const window = JSON.stringify(windowArgs);
    const params = { window, k: k.toString() };
    const queryString = new URLSearchParams(params).toString();
    return deploymentFetch([
      deploymentUrl,
      `${url}?${queryString}`,
      authHeader,
    ]);
  };

  const { data: d } = useSWR(
    // The key here is not used by the fetcher, but
    // it's used by the SWR cache to differentiate between different calls.
    isDisconnected ? null : cacheKey,
    fetcher,
    {
      refreshInterval: 8 * 1000,
    },
  );
  if (!d) {
    return undefined;
  }
  const mapFunctionToBuckets = multiResponseToTimeSeries(
    d as TopKMetricsResponse,
  );
  const data = [];
  const lineKeys = [];
  const functions: string[] = [...mapFunctionToBuckets.keys()];
  const xAxisKey = "time";
  if (!mapFunctionToBuckets || !functions.length) {
    return null;
  }
  let hadDataAt = -1;
  for (const [i, bucket] of mapFunctionToBuckets.get(functions[0])!.entries()) {
    const dataPoint: any = {};
    dataPoint[xAxisKey] = format(bucket.time, "h:mm a");
    for (const f of functions) {
      const { metric } = mapFunctionToBuckets.get(f)![i];
      if (hadDataAt === -1) {
        hadDataAt = metric !== null ? i : hadDataAt;
      }
      dataPoint[identifierForMetricName(f)] =
        typeof metric === "number"
          ? metric
          : hadDataAt > -1
            ? kind === "cacheHitPercentage"
              ? 100
              : 0
            : null;
    }
    data.push(dataPoint);
  }

  const colorForFunction = new Map<string, string>();
  for (const f of functions) {
    if (f === "_rest") {
      colorForFunction.set(f, restColor);
      continue;
    }

    const colorIndex =
      [...f].reduce((acc, char) => acc + char.charCodeAt(0), 0) %
      lineColors.length;
    let color = lineColors[colorIndex];
    let attempts = 0;
    while (
      [...colorForFunction.values()].includes(color) &&
      attempts < lineColors.length
    ) {
      attempts++;
      color = lineColors[(colorIndex + attempts) % lineColors.length];
    }
    colorForFunction.set(f, color);
  }

  for (const [_, f] of functions.entries()) {
    const key = identifierForMetricName(f);
    const lineKey = {
      key,
      name: key,
      color: colorForFunction.get(f)!,
    };
    lineKeys.push(lineKey);
  }
  return {
    // If there's missing data, only show up to where we had data.
    // If there's only one data point, show 2 data points so the graph doesn't look strange.
    data: hadDataAt > -1 ? data.slice(hadDataAt === 59 ? 58 : hadDataAt) : data,
    xAxisKey,
    lineKeys,
  };
}

export type FunctionRateHeatmapRow = {
  /// Function identifier (or "_rest" for the catch-all aggregate row).
  key: string;
  cells: Array<{
    time: Date;
    value: number | null;
  }>;
};

export type FunctionRateHeatmapData = {
  rows: FunctionRateHeatmapRow[];
  bucketStartTimes: Date[];
};

/**
 * Fetches per-function rate (cache hit % or failure %) over the last hour for
 * the heatmap. Returns raw per-bucket values (no fill hack — missing buckets
 * stay null so the heatmap can render them as empty cells). `numBuckets`
 * controls the resolution: with the fixed 60-minute window, 12 buckets →
 * 5-minute cells, 30 → 2-minute, 60 → 1-minute (the backend cap).
 */
export function useTopKFunctionRateHeatmap(
  kind: "cacheHitPercentage" | "failurePercentage",
  k: number,
  numBuckets: number = 12,
): FunctionRateHeatmapData | null | undefined {
  const route =
    kind === "cacheHitPercentage"
      ? "cache_hit_percentage_top_k"
      : "failure_percentage_top_k";
  const url = `/api/app_metrics/${route}`;
  const isDisconnected = useDeploymentIsDisconnected();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  const cacheKey = `${deploymentUrl}${url}?k=${k}&numBuckets=${numBuckets}`;

  const fetcher = async () => {
    const start = new Date(Date.now() - 60 * 60 * 1000);
    const end = new Date();
    const windowArgs = {
      start: serializeDate(start),
      end: serializeDate(end),
      num_buckets: numBuckets,
    };
    const params: Record<string, string> = {
      window: JSON.stringify(windowArgs),
      k: k.toString(),
    };
    const queryString = new URLSearchParams(params).toString();
    return deploymentFetch([
      deploymentUrl,
      `${url}?${queryString}`,
      authHeader,
    ]);
  };

  // `keepPreviousData` means the hook returns the last successful response
  // while a new fetch (triggered by a changed `k` or `numBuckets` — e.g.
  // after a container resize) is in flight, so the heatmap keeps rendering
  // instead of flashing a spinner.
  const { data: d } = useSWR(isDisconnected ? null : cacheKey, fetcher, {
    refreshInterval: 8 * 1000,
    keepPreviousData: true,
  });
  if (!d) {
    return undefined;
  }
  const mapFunctionToBuckets = multiResponseToTimeSeries(
    d as TopKMetricsResponse,
  );
  const functions: string[] = [...mapFunctionToBuckets.keys()];
  if (!functions.length) {
    return null;
  }

  const bucketStartTimes: Date[] = mapFunctionToBuckets
    .get(functions[0])!
    .map((b: Bucket) => b.time);

  // Forward-fill threshold is shared across functions: once *any* function
  // has a data point at bucket i, all functions fill later null buckets
  // (including the current one) with the "idle" value — 100 for cache hit,
  // 0 for failure. This keeps rows aligned so a quiet function reads as
  // "idle" rather than "no data" once the deployment is actively running.
  // Buckets before the first data point from any function stay null.
  const idleFill = kind === "cacheHitPercentage" ? 100 : 0;
  const bucketCount = bucketStartTimes.length;
  let firstDataBucketIdx = -1;
  for (let i = 0; i < bucketCount; i++) {
    const anyHasData = functions.some(
      (f) => typeof mapFunctionToBuckets.get(f)![i].metric === "number",
    );
    if (anyHasData) {
      firstDataBucketIdx = i;
      break;
    }
  }
  const rows: FunctionRateHeatmapRow[] = functions.map((f) => {
    const buckets = mapFunctionToBuckets.get(f)!;
    const cells = buckets.map(({ time, metric }: Bucket, i: number) => {
      if (typeof metric === "number") return { time, value: metric };
      const inFilledRegion =
        firstDataBucketIdx !== -1 && i >= firstDataBucketIdx;
      return { time, value: inFilledRegion ? idleFill : null };
    });
    return { key: identifierForMetricName(f), cells };
  });

  return { rows, bucketStartTimes };
}

export function useFunctionCallCountTopK(k: number = 5) {
  const url = "/api/app_metrics/function_call_count_top_k";
  const isDisconnected = useDeploymentIsDisconnected();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  const cacheKey = `${deploymentUrl}${url}?k=${k}`;

  const fetcher = async () => {
    const start = new Date(Date.now() - 60 * 60 * 1000);
    const end = new Date();
    const windowArgs = {
      start: serializeDate(start),
      end: serializeDate(end),
      num_buckets: 60,
    };
    const window = JSON.stringify(windowArgs);
    const params: Record<string, string> = { window, k: k.toString() };
    const queryString = new URLSearchParams(params).toString();
    return deploymentFetch([
      deploymentUrl,
      `${url}?${queryString}`,
      authHeader,
    ]);
  };

  const { data: d } = useSWR(isDisconnected ? null : cacheKey, fetcher, {
    refreshInterval: 8 * 1000,
  });

  if (!d) {
    return undefined;
  }

  const mapFunctionToBuckets = multiResponseToTimeSeries(
    d as TopKMetricsResponse,
  );
  const data = [];
  const lineKeys = [];
  const functions: string[] = [...mapFunctionToBuckets.keys()];
  const xAxisKey = "time";

  if (!mapFunctionToBuckets || !functions.length) {
    return null;
  }

  let hadDataAt = -1;
  for (const [i, bucket] of mapFunctionToBuckets.get(functions[0])!.entries()) {
    const dataPoint: any = {};
    dataPoint[xAxisKey] = format(bucket.time, "h:mm a");
    for (const f of functions) {
      const { metric } = mapFunctionToBuckets.get(f)![i];
      if (hadDataAt === -1) {
        hadDataAt = metric !== null ? i : hadDataAt;
      }
      dataPoint[identifierForMetricName(f)] =
        metric ?? (hadDataAt > -1 ? 0 : null);
    }
    data.push(dataPoint);
  }

  const colorForFunction = new Map<string, string>();
  for (const f of functions) {
    if (f === "_rest") {
      colorForFunction.set(f, restColor);
      continue;
    }

    const colorIndex =
      [...f].reduce((acc, char) => acc + char.charCodeAt(0), 0) %
      lineColors.length;
    let color = lineColors[colorIndex];
    let attempts = 0;
    while (
      [...colorForFunction.values()].includes(color) &&
      attempts < lineColors.length
    ) {
      attempts++;
      color = lineColors[(colorIndex + attempts) % lineColors.length];
    }
    colorForFunction.set(f, color);
  }

  for (const f of functions) {
    const key = identifierForMetricName(f);
    const lineKey = {
      key,
      name: key,
      color: colorForFunction.get(f)!,
    };
    lineKeys.push(lineKey);
  }

  return {
    data: hadDataAt > -1 ? data.slice(hadDataAt === 59 ? 58 : hadDataAt) : data,
    xAxisKey,
    lineKeys,
  };
}

export function useSubscriptionInvalidationsTopK(
  k: number = 5,
  opts?: {
    udfIdentifier: string;
    componentPath?: string;
    udfType: UdfType;
  },
) {
  const url = "/api/app_metrics/subscription_invalidations_top_k";
  const isDisconnected = useDeploymentIsDisconnected();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  const cacheKey = `${deploymentUrl}${url}?k=${k}${opts ? `&path=${opts.udfIdentifier}` : ""}`;

  const fetcher = async () => {
    const start = new Date(Date.now() - 60 * 60 * 1000);
    const end = new Date();
    const windowArgs = {
      start: serializeDate(start),
      end: serializeDate(end),
      num_buckets: 60,
    };
    const params: Record<string, string> = {
      window: JSON.stringify(windowArgs),
      k: k.toString(),
    };
    if (opts) {
      params.udfPath = opts.udfIdentifier;
      params.udfType = opts.udfType;
      if (opts.componentPath) {
        params.componentPath = opts.componentPath;
      }
    }
    const queryString = new URLSearchParams(params).toString();
    return deploymentFetch([
      deploymentUrl,
      `${url}?${queryString}`,
      authHeader,
    ]);
  };

  const { data: d } = useSWR(isDisconnected ? null : cacheKey, fetcher, {
    refreshInterval: 8 * 1000,
  });

  return useTopKChartData(d as TopKMetricsResponse | undefined);
}

function useTopKChartData(d: TopKMetricsResponse | undefined) {
  if (!d) {
    return undefined;
  }

  const mapFunctionToBuckets = multiResponseToTimeSeries(d);
  const data = [];
  const lineKeys = [];
  const functions: string[] = [...mapFunctionToBuckets.keys()].sort((a, b) =>
    a === "_rest" ? 1 : b === "_rest" ? -1 : 0,
  );
  const xAxisKey = "time";

  if (!mapFunctionToBuckets || !functions.length) {
    return null;
  }

  let hadDataAt = -1;
  for (const [i, bucket] of mapFunctionToBuckets.get(functions[0])!.entries()) {
    const dataPoint: any = {};
    dataPoint[xAxisKey] = format(bucket.time, "h:mm a");
    for (const f of functions) {
      const { metric } = mapFunctionToBuckets.get(f)![i];
      if (hadDataAt === -1) {
        hadDataAt = metric !== null ? i : hadDataAt;
      }
      dataPoint[f] = metric ?? (hadDataAt > -1 ? 0 : null);
    }
    data.push(dataPoint);
  }

  const colorForFunction = new Map<string, string>();
  for (const f of functions) {
    if (f === "_rest") {
      colorForFunction.set(f, restColor);
      continue;
    }

    const colorIndex =
      [...f].reduce((acc, char) => acc + char.charCodeAt(0), 0) %
      lineColors.length;
    let color = lineColors[colorIndex];
    let attempts = 0;
    while (
      [...colorForFunction.values()].includes(color) &&
      attempts < lineColors.length
    ) {
      attempts++;
      color = lineColors[(colorIndex + attempts) % lineColors.length];
    }
    colorForFunction.set(f, color);
  }

  for (const f of functions) {
    const lineKey = {
      key: f,
      name: f,
      color: colorForFunction.get(f)!,
    };
    lineKeys.push(lineKey);
  }
  lineKeys.sort((a, b) => (a.key === "_rest" ? 1 : b.key === "_rest" ? -1 : 0));

  return {
    data: hadDataAt > -1 ? data.slice(hadDataAt === 59 ? 58 : hadDataAt) : data,
    xAxisKey,
    lineKeys,
  };
}

const restColor = "var(--chart-line-1)";
const lineColors = [
  "var(--chart-line-2)",
  "var(--chart-line-3)",
  "var(--chart-line-4)",
  "var(--chart-line-5)",
  "var(--chart-line-6)",
  "var(--chart-line-7)",
];

function identifierForMetricName(metric: string) {
  return metric === "_rest" ? metric : functionIdentifierValue(metric);
}

/**
 * Query a UDF rate (see the `UdfMetric` type) for the time window bounded by `start` and `end`. You can
 * control the number of samples returned (and, therefore, the metric's resolution) with `numBuckets`.
 */
export async function udfRate(args: {
  deploymentUrl: string;
  udfIdentifier: string;
  componentPath?: string;
  udfType: UdfType;
  start: Date;
  metric: UdfMetric;
  end: Date;
  numBuckets: number;
  authHeader: string;
}): Promise<Timeseries> {
  const searchParams = new URLSearchParams();
  searchParams.set("metric", args.metric);
  const responseJson = await getUdfMetric({
    ...args,
    searchParams,
    endpoint: "udf_rate",
  });
  return responseToTimeseries(responseJson);
}

/**
 * Query the cache hit rate percentage (as a number between 0% and 100%) for a given time window. Note that
 * the returned timeseries may have `null` for a bucket if there were no requests in that time interval.
 */
export async function cacheHitPercentage(args: {
  deploymentUrl: string;
  udfIdentifier: string;
  componentPath?: string;
  udfType: UdfType;
  start: Date;
  end: Date;
  numBuckets: number;
  authHeader: string;
}): Promise<Timeseries> {
  const responseJson = await getUdfMetric({
    ...args,
    searchParams: new URLSearchParams(),
    endpoint: "cache_hit_percentage",
  });
  return responseToTimeseries(responseJson);
}

/**
 * Query multiple percentiles (specified as a number between 0 and 100, inclusive) for UDF execution latency.
 */
export async function latencyPercentiles(args: {
  deploymentUrl: string;
  udfIdentifier: string;
  componentPath?: string;
  udfType: UdfType;
  percentiles: number[];
  start: Date;
  end: Date;
  numBuckets: number;
  authHeader: string;
}): Promise<Map<number, Timeseries>> {
  const searchParams = new URLSearchParams();
  searchParams.set("percentiles", JSON.stringify(args.percentiles));
  const responseJson = await getUdfMetric({
    ...args,
    searchParams,
    endpoint: "latency_percentiles",
  });
  return multiResponseToTimeSeries(responseJson);
}

async function getUdfMetric(args: {
  deploymentUrl: string;
  udfIdentifier: string;
  componentPath?: string;
  udfType: UdfType;
  searchParams: URLSearchParams;
  endpoint: string;
  start: Date;
  end: Date;
  numBuckets: number;
  authHeader: string;
}) {
  const { searchParams } = args;
  searchParams.set("path", args.udfIdentifier);
  if (args.componentPath) {
    searchParams.set("componentPath", args.componentPath);
  }
  const windowArgs = {
    start: serializeDate(args.start),
    end: serializeDate(args.end),
    num_buckets: args.numBuckets,
  };
  searchParams.set("window", JSON.stringify(windowArgs));
  searchParams.set("udfType", args.udfType);
  const url = `${appMetricsUrl(args.deploymentUrl)}/${
    args.endpoint
  }?${searchParams.toString()}`;
  const response = await fetch(url, {
    headers: {
      Authorization: args.authHeader,
      "Convex-Client": "dashboard-0.0.0",
    },
  });
  if (!response.ok) {
    throw new Error(await response.text());
  }
  return response.json();
}

/**
 * Query a Table rate (see the `TableMetric` type).
 */
export async function tableRate(
  deploymentUrl: string,
  tableName: string,
  metric: TableMetric,
  start: Date,
  end: Date,
  numBuckets: number,
  authHeader: string,
): Promise<Timeseries> {
  const windowArgs = {
    start: serializeDate(start),
    end: serializeDate(end),
    num_buckets: numBuckets,
  };
  const name = encodeURIComponent(tableName);
  const window = encodeURIComponent(JSON.stringify(windowArgs));
  const url = `${appMetricsUrl(
    deploymentUrl,
  )}/table_rate?name=${name}&metric=${metric}&window=${window}`;
  const response = await fetch(url, {
    headers: { Authorization: authHeader, "Convex-Client": "dashboard-0.0.0" },
  });
  if (!response.ok) {
    throw new Error(await response.text());
  }
  const respJSON: TimeseriesResponse = await response.json();
  return responseToTimeseries(respJSON);
}

type FunctionConcurrencyResponse = Record<string, TimeseriesResponse>;

function mapFunctionTypeToDisplayName(
  env: string,
  udfType: string,
): string | null {
  if (env === "isolate" && udfType === "Query") return "Queries";
  if (env === "isolate" && udfType === "Mutation") return "Mutations";
  if (env === "isolate" && udfType === "Action") return "Actions";
  if (env === "node" && udfType === "Action") return "Actions (Node)";
  if (env === "isolate" && udfType === "HttpAction") return "HTTP Actions";
  return null;
}

export function useFunctionConcurrency(): {
  running: ChartData | null;
  queued: ChartData | null;
} {
  const url = "/api/app_metrics/function_concurrency";
  const isDisconnected = useDeploymentIsDisconnected();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();

  const fetcher = async () => {
    const start = new Date(Date.now() - 60 * 60 * 1000); // 1 hour ago
    const end = new Date();
    const windowArgs = {
      start: serializeDate(start),
      end: serializeDate(end),
      num_buckets: 60,
    };
    const window = JSON.stringify(windowArgs);
    const params = { window };
    const queryString = new URLSearchParams(params).toString();
    return deploymentFetch([
      deploymentUrl,
      `${url}?${queryString}`,
      authHeader,
    ]);
  };

  const { data: responseData } = useSWR(isDisconnected ? null : url, fetcher, {
    refreshInterval: 8 * 1000,
  });

  if (!responseData) {
    return { running: null, queued: null };
  }

  const rawData = responseData as FunctionConcurrencyResponse;

  const runningMetrics = new Map<string, Timeseries>();
  const queuedMetrics = new Map<string, Timeseries>();

  for (const [key, timeseries] of Object.entries(rawData)) {
    const parts = key.split(":");
    if (parts.length !== 4 || parts[0] !== "outstanding_functions") {
      continue;
    }

    const [, env, udfType, state] = parts;
    const displayName = mapFunctionTypeToDisplayName(env, udfType);

    if (!displayName) continue;

    const parsedTimeseries = responseToTimeseries(timeseries);

    if (state === "running") {
      runningMetrics.set(displayName, parsedTimeseries);
    } else if (state === "queued") {
      queuedMetrics.set(displayName, parsedTimeseries);
    }
  }

  const buildChartData = (
    metricsMap: Map<string, Timeseries>,
  ): ChartData | null => {
    const functionTypes = [...metricsMap.keys()];
    if (functionTypes.length === 0) return null;

    const data = [];
    const lineKeys = [];
    const xAxisKey = "time";

    const firstTimeseries = metricsMap.get(functionTypes[0])!;
    for (const [i, bucket] of firstTimeseries.entries()) {
      const dataPoint: any = {};
      dataPoint[xAxisKey] = format(bucket.time, "h:mm a");

      for (const functionType of functionTypes) {
        const timeseries = metricsMap.get(functionType)!;
        const { metric } = timeseries[i];
        dataPoint[functionType] = metric ?? 0;
      }

      data.push(dataPoint);
    }

    const functionTypeColors: Record<string, string> = {
      Queries: "var(--chart-line-3)",
      Mutations: "var(--chart-line-2)",
      Actions: "var(--chart-line-1)",
      "Actions (Node)": "var(--chart-line-5)",
      "HTTP Actions": "var(--chart-line-6)",
    };

    const colorForFunction = new Map<string, string>();
    for (const [index, functionType] of functionTypes.entries()) {
      const color =
        functionTypeColors[functionType] ??
        lineColors[index % lineColors.length];
      colorForFunction.set(functionType, color);
    }

    for (const functionType of functionTypes) {
      lineKeys.push({
        key: functionType,
        name: functionType,
        color: colorForFunction.get(functionType)!,
      });
    }

    return { data, xAxisKey, lineKeys };
  };

  return {
    running: buildChartData(runningMetrics),
    queued: buildChartData(queuedMetrics),
  };
}
