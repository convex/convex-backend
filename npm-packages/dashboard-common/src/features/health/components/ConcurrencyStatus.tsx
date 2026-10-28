import { useMemo } from "react";
import {
  CrossCircledIcon,
  ExclamationTriangleIcon,
} from "@radix-ui/react-icons";
import {
  useSchedulerLag,
  useFunctionConcurrency,
} from "@common/lib/appMetrics";
import { ChartData } from "@common/lib/charts/types";

type ConcurrencyIssue = {
  severity: "warning" | "critical";
  message: string;
  type: "scheduler" | "concurrency";
};

export function useConcurrencyStatus(
  showConcurrencyIssues: boolean = true,
  chartCount: number = 3,
): {
  issues: ConcurrencyIssue[];
  closedDescription: React.ReactNode;
  lag: ChartData | null | undefined;
  running: ChartData | null;
  queued: ChartData | null;
} {
  const lag = useSchedulerLag();
  const { queued, running } = useFunctionConcurrency();

  const issues = useMemo(() => {
    const result: ConcurrencyIssue[] = [];

    // Check scheduler status
    if (lag && lag.data.length > 0) {
      const currentLag = lag.data[lag.data.length - 1].lag;
      const wasBehind = lag.data.some((d) => d.lag > 0);

      if (currentLag > 1) {
        result.push({
          severity: "critical",
          message: `Scheduler is ${currentLag} minutes behind`,
          type: "scheduler",
        });
      } else if (wasBehind && currentLag === 0) {
        result.push({
          severity: "warning",
          message: "Scheduler was behind but has recovered",
          type: "scheduler",
        });
      }
    }

    // Check concurrency limits for each function type (only if enabled)
    if (showConcurrencyIssues && queued && running) {
      const queuedData = queued.data as Array<Record<string, number>>;
      const runningData = running.data as Array<Record<string, number>>;

      for (const lineKey of queued.lineKeys) {
        const functionType = lineKey.name;
        const currentQueued =
          queuedData[queuedData.length - 1]?.[functionType] ?? 0;
        const hasBeenQueued = queuedData.some(
          (d) => (d[functionType] ?? 0) > 0,
        );

        // Check if currently maxed out - queued functions with running at capacity
        if (currentQueued > 0) {
          const currentRunning =
            runningData[runningData.length - 1]?.[functionType] ?? 0;
          if (currentRunning > 0) {
            result.push({
              severity: "critical",
              message: `${functionType} currently at concurrency limit`,
              type: "concurrency",
            });
          }
        } else if (hasBeenQueued) {
          result.push({
            severity: "warning",
            message: `${functionType} hit concurrency limit`,
            type: "concurrency",
          });
        }
      }
    }

    return result;
  }, [lag, queued, running, showConcurrencyIssues]);

  const closedDescription = useMemo(() => {
    // Don't show anything until necessary data has loaded
    if (!lag) {
      return null;
    }

    // If concurrency issues are enabled, wait for all data to load
    if (showConcurrencyIssues && (!queued || !running)) {
      return null;
    }

    if (issues.length === 0) {
      return (
        <span className="animate-fadeInFromLoading text-xs text-content-secondary">
          {chartCount} charts
        </span>
      );
    }

    const criticalIssues = issues.filter((i) => i.severity === "critical");
    const warningIssues = issues.filter((i) => i.severity === "warning");

    return (
      <span className="flex animate-fadeInFromLoading items-center gap-3 text-xs">
        {criticalIssues.length > 0 && (
          <span className="flex items-center gap-1 text-content-error">
            <CrossCircledIcon className="size-3 min-w-3" />
            {criticalIssues[0].message}
          </span>
        )}
        {warningIssues.length > 0 && (
          <span className="flex items-center gap-1 text-content-warning">
            <ExclamationTriangleIcon className="size-3 min-w-3" />
            {warningIssues[0].message}
          </span>
        )}
      </span>
    );
  }, [chartCount, issues, lag, queued, running, showConcurrencyIssues]);

  return { issues, closedDescription, lag, running, queued };
}
