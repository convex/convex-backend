import { format } from "date-fns";
import { useContext } from "react";
import {
  useDeploymentAuthHeader,
  useDeploymentUrl,
} from "@common/lib/deploymentApi";
import { SingleGraph } from "@common/features/functions/components/SingleGraph";
import { useCurrentOpenFunction } from "@common/lib/functions/FunctionsProvider";
import {
  UdfMetric,
  udfRate,
  cacheHitPercentage,
  latencyPercentiles,
  useSubscriptionInvalidationsTopK,
} from "@common/lib/appMetrics";
import { HealthCard } from "@common/elements/HealthCard";
import { ChartForFunctionRate } from "@common/features/health/components/ChartForFunctionRate";
import { calcBuckets } from "@common/lib/charts/buckets";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";

export function PerformanceGraphs() {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewMetrics = useIsOperationAllowed("ViewMetrics");
  const currentOpenFunction = useCurrentOpenFunction();
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();

  if (!canViewMetrics) {
    return (
      <NoPermissionMessage
        message="You do not have permission to view metrics in this deployment."
        missingPermission="deployment:metrics:view"
      />
    );
  }

  if (!currentOpenFunction) {
    return null;
  }

  const file = currentOpenFunction!;
  const lineColors = [
    "var(--chart-line-1)",
    "var(--chart-line-2)",
    "var(--chart-line-3)",
    "var(--chart-line-4)",
  ];

  function genErrorOrInvocationFunc(
    udfMetric: UdfMetric,
    name: string,
    color: string,
  ) {
    return async function genChartData(start: Date, end: Date) {
      const { startTime, endTime, numBuckets, timeMultiplier, formatTime } =
        calcBuckets(start, end);
      const buckets = await udfRate({
        deploymentUrl,
        udfIdentifier: file.displayName,
        componentPath: file.componentPath ?? undefined,
        udfType: file.udfType,
        metric: udfMetric,
        start: startTime,
        end: endTime,
        numBuckets,
        authHeader,
      });
      const data = buckets.map((value) =>
        value.metric
          ? {
              time: formatTime(value.time),
              metric: value.metric * timeMultiplier,
            }
          : {
              time: formatTime(value.time),
              metric: 0,
            },
      );
      return Promise.resolve({
        data,
        xAxisKey: "time",
        lineKeys: [
          {
            key: "metric",
            name,
            color,
          },
        ],
      });
    };
  }

  const calcCacheHitPercentage = async (start: Date, end: Date) => {
    const buckets = await cacheHitPercentage({
      deploymentUrl,
      udfIdentifier: file.displayName,
      componentPath: file.componentPath ?? undefined,
      udfType: file.udfType,
      start,
      end,
      numBuckets: 60,
      authHeader,
    });
    const data = buckets.map((value) =>
      value.metric
        ? {
            time: format(value.time, "hh:mm a"),
            metric: Math.round((value.metric + Number.EPSILON) * 100) / 100,
          }
        : {
            time: format(value.time, "hh:mm a"),
            metric: 0,
          },
    );
    return Promise.resolve({
      data,
      xAxisKey: "time",
      lineKeys: [
        {
          key: "metric",
          name: "%",
          color: "var(--chart-line-1)",
        },
      ],
    });
  };

  const calcLatencyPercentiles = async (start: Date, end: Date) => {
    const mapPercentileToBuckets = await latencyPercentiles({
      deploymentUrl,
      udfIdentifier: file.displayName,
      componentPath: file.componentPath ?? undefined,
      udfType: file.udfType,
      percentiles: [50, 90, 95, 99],
      start,
      end,
      numBuckets: 60,
      authHeader,
    });
    const data = [];
    const lineKeys = [];
    const percentiles = [...mapPercentileToBuckets.keys()];
    const xAxisKey = "time";
    for (const [i, bucket] of mapPercentileToBuckets
      .get(percentiles[0])!
      .entries()) {
      const dataPoint: any = {};
      dataPoint[xAxisKey] = format(bucket.time, "h:mm a");
      for (const percentile of percentiles) {
        const { metric } = mapPercentileToBuckets.get(percentile)![i];
        dataPoint[`p${percentile}`] = metric
          ? Math.round((metric + Number.EPSILON) * 100000) / 100
          : 0; // convert to ms
      }
      data.push(dataPoint);
    }
    for (const [i, percentile] of percentiles.entries()) {
      const pstring = `p${percentile}`;
      const lineKey = {
        key: pstring,
        name: `ms ${pstring}`,
        color: lineColors[i],
      };
      lineKeys.push(lineKey);
    }
    return Promise.resolve({
      data,
      xAxisKey,
      lineKeys,
    });
  };

  return (
    <div
      className="grid gap-2"
      style={{
        gridTemplateColumns: "repeat(auto-fit, minmax(24rem, 1fr))",
      }}
    >
      <SingleGraph
        title="Function Calls"
        dataSource={genErrorOrInvocationFunc(
          "invocations",
          " function calls",
          "var(--chart-line-1)",
        )}
        syncId="fnMetrics"
      />
      <SingleGraph
        title="Errors"
        dataSource={genErrorOrInvocationFunc(
          "errors",
          " errors",
          "var(--chart-line-4)",
        )}
        syncId="fnMetrics"
      />
      <SingleGraph
        title="Execution Time"
        dataSource={calcLatencyPercentiles}
        syncId="fnMetrics"
      />
      {currentOpenFunction.udfType === "Query" && (
        <SingleGraph
          title="Cache Hit Rate"
          dataSource={calcCacheHitPercentage}
          syncId="fnMetrics"
        />
      )}
      {currentOpenFunction.udfType === "Mutation" && (
        <SubscriptionInvalidationsGraph
          udfIdentifier={file.displayName}
          componentPath={file.componentPath ?? undefined}
          udfType={file.udfType}
        />
      )}
    </div>
  );
}

function SubscriptionInvalidationsGraph({
  udfIdentifier,
  componentPath,
  udfType,
}: {
  udfIdentifier: string;
  componentPath?: string;
  udfType: "Query" | "Mutation" | "Action" | "HttpAction";
}) {
  const chartData = useSubscriptionInvalidationsTopK(5, {
    udfIdentifier,
    componentPath,
    udfType,
  });

  return (
    <HealthCard
      title="Subscription Invalidations"
      tip="The tables whose subscriptions are most frequently invalidated by this mutation, bucketed by minute."
    >
      <ChartForFunctionRate
        chartData={chartData}
        kind="subscriptionInvalidations"
      />
    </HealthCard>
  );
}
