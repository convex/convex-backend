import {
  ChevronDownIcon,
  ChevronUpIcon,
  PieChartIcon,
} from "@radix-ui/react-icons";
import { useMemo } from "react";
import { UdfLog, UdfLogOutcome } from "@common/lib/useLogs";
import { HelpTooltip } from "@ui/HelpTooltip";
import { msFormat, formatBytes } from "@common/lib/format";
import { UsageStats } from "system-udfs/convex/_system/frontend/common";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import { Spinner } from "@ui/Spinner";

type RequestUsageStats = UsageStats & {
  runtimeMs: number;
  computeMbMs: number;
  returnBytes?: number;
};

type OutcomeNode = {
  inProgress: boolean;
  functionName?: string;
  caller?: string;
  environment?: "isolate" | "node";
  identityType?: string;
  executionTime?: number;
  localizedTimestamp?: string;
  endTime?: number;
  udfType?: string;
  cachedResult?: boolean;
};

export function LogMetadata({
  requestId,
  logs,
  executionId,
}: {
  requestId: string;
  logs: UdfLog[];
  executionId?: string;
}) {
  const isExecutionView = !!executionId;

  const filteredLogs = useMemo(() => {
    if (!isExecutionView) {
      return logs.filter((log) => log.requestId === requestId);
    }
    return logs.filter((log) => log.executionId === executionId);
  }, [logs, isExecutionView, executionId, requestId]);

  const requestOutcomeNode = useMemo((): OutcomeNode | null => {
    const outcomeLog =
      logs.find(
        (log): log is UdfLog & UdfLogOutcome =>
          log.kind === "outcome" &&
          log.requestId === requestId &&
          !("parentExecutionId" in log && log.parentExecutionId),
      ) ||
      logs.find(
        (log): log is UdfLog =>
          log.requestId === requestId &&
          !("parentExecutionId" in log && log.parentExecutionId),
      );

    return outcomeLog
      ? {
          inProgress: outcomeLog.kind !== "outcome",
          functionName: outcomeLog.call,
          caller: outcomeLog.kind === "outcome" ? outcomeLog.caller : undefined,
          environment:
            outcomeLog.kind === "outcome" ? outcomeLog.environment : undefined,
          identityType:
            outcomeLog.kind === "outcome" ? outcomeLog.identityType : undefined,
          executionTime:
            outcomeLog.kind === "outcome"
              ? (outcomeLog.executionTimeMs ?? undefined)
              : undefined,
          localizedTimestamp: outcomeLog.localizedTimestamp,
          endTime:
            "executionTimestamp" in outcomeLog
              ? outcomeLog.executionTimestamp
              : undefined,
          udfType: outcomeLog.udfType,
          cachedResult: outcomeLog.cachedResult,
        }
      : null;
  }, [logs, requestId]);

  const executionOutcomeNode = useMemo((): OutcomeNode | null => {
    if (!executionId) return null;

    const executionLogs = logs.filter((log) => log.executionId === executionId);

    // Find the outcome log for complete information
    const outcomeLog = executionLogs.find(
      (log): log is UdfLog & UdfLogOutcome => log.kind === "outcome",
    );

    // Get function name and type from any log with this executionId
    const anyLog = executionLogs[0];

    return {
      inProgress: !outcomeLog,
      functionName: outcomeLog?.call ?? anyLog?.call ?? undefined,
      caller: outcomeLog?.caller,
      environment: outcomeLog?.environment,
      identityType: outcomeLog?.identityType,
      executionTime: outcomeLog?.executionTimeMs ?? undefined,
      localizedTimestamp:
        outcomeLog?.localizedTimestamp ??
        anyLog?.localizedTimestamp ??
        undefined,
      endTime: outcomeLog?.executionTimestamp ?? undefined,
      udfType: outcomeLog?.udfType ?? anyLog?.udfType ?? undefined,
      cachedResult:
        outcomeLog?.cachedResult ?? anyLog?.cachedResult ?? undefined,
    };
  }, [logs, executionId]);

  const usageStats = useMemo(() => {
    const totals: RequestUsageStats = {
      memoryUsedMb: 0,
      databaseReadBytes: 0,
      databaseWriteBytes: 0,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      databaseReadDocuments: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      runtimeMs: 0,
      computeMbMs: 0,
    };

    return filteredLogs.reduce((accumulated, log) => {
      const ret = accumulated;
      if ("usageStats" in log && log.usageStats) {
        for (const [key, value] of Object.entries(log.usageStats) as Array<
          [keyof UsageStats, number | null | undefined]
        >) {
          ret[key] += value ?? 0;
        }
      }
      if ("returnBytes" in log && log.returnBytes) {
        ret.returnBytes = (ret.returnBytes ?? 0) + log.returnBytes;
      }
      if (log.kind === "outcome") {
        const durationMs = log.executionTimeMs ?? 0;
        ret.runtimeMs += durationMs;
        const memoryMb = (log.usageStats?.memoryUsedMb ?? 0) as number;
        ret.computeMbMs += durationMs * memoryMb;
      }
      return ret;
    }, totals);
  }, [filteredLogs]);

  const isInProgress = executionId
    ? !executionOutcomeNode || executionOutcomeNode.inProgress
    : !requestOutcomeNode || requestOutcomeNode.inProgress;

  return (
    <div className="animate-fadeInFromLoading p-2 text-xs">
      {isExecutionView ? (
        <ExecutionInfoList
          outcomeNode={executionOutcomeNode}
          executionId={executionId}
        />
      ) : (
        <RequestInfoList
          outcomeNode={requestOutcomeNode}
          requestId={requestId}
        />
      )}
      <ResourcesUsed
        usageStats={usageStats}
        filteredLogs={filteredLogs}
        isExecutionView={isExecutionView}
        isInProgress={isInProgress}
      />
    </div>
  );
}

function ResourcesUsed({
  usageStats,
  filteredLogs,
  isExecutionView,
  isInProgress,
}: {
  usageStats: RequestUsageStats;
  filteredLogs: UdfLog[];
  isExecutionView: boolean;
  isInProgress: boolean;
}) {
  return (
    <div className="mt-2">
      <Disclosure defaultOpen>
        {({ open }) => (
          <>
            <div className="flex items-center justify-between">
              <DisclosureButton className="flex items-center gap-1 text-xs">
                <PieChartIcon className="size-3 text-content-secondary" />
                <h6 className="font-semibold text-content-secondary">
                  Resources Used
                </h6>
                {open ? (
                  <ChevronUpIcon className="size-3" />
                ) : (
                  <ChevronDownIcon className="size-3" />
                )}
              </DisclosureButton>
            </div>

            <DisclosurePanel className="mt-2 animate-fadeInFromLoading">
              {isInProgress && <Running />}

              {isInProgress && isExecutionView ? (
                <span className="mt-2 text-content-tertiary">
                  Resource usage will appear here once the function call
                  completes.
                </span>
              ) : (
                <ul className="divide-y text-xs">
                  <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                    <span className="text-content-secondary">Compute</span>
                    <span className="min-w-0 text-content-primary">
                      <strong>
                        {Number(
                          usageStats.computeMbMs / (1024 * 3_600_000),
                        ).toFixed(7)}{" "}
                        GB-hr
                      </strong>{" "}
                      ({usageStats.memoryUsedMb ?? 0} MB for{" "}
                      {Number(usageStats.runtimeMs / 1000).toFixed(2)}s)
                    </span>
                  </li>
                  <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                    <span className="text-content-secondary">
                      DB I/O Bandwidth
                    </span>
                    <span className="min-w-0 text-content-primary">
                      <strong>
                        {formatBytes(usageStats.databaseIoReadBytes)}
                      </strong>{" "}
                      read,{" "}
                      <strong>
                        {formatBytes(usageStats.databaseIoWriteBytes)}
                      </strong>{" "}
                      written
                    </span>
                  </li>
                  <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                    <span className="text-content-secondary">
                      File Bandwidth
                    </span>
                    <span className="min-w-0 text-content-primary">
                      <strong>
                        {formatBytes(usageStats.storageReadBytes)}
                      </strong>{" "}
                      read,{" "}
                      <strong>
                        {formatBytes(usageStats.storageWriteBytes)}
                      </strong>{" "}
                      written
                    </span>
                  </li>
                  <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                    <span className="text-content-secondary">Text Search</span>
                    <span className="min-w-0 text-content-primary">
                      <strong>
                        {formatBytes(usageStats.textIndexQueryBytes)}
                      </strong>{" "}
                      queried,{" "}
                      <strong>
                        {formatBytes(usageStats.textIndexWriteQueryBytes)}
                      </strong>{" "}
                      written
                    </span>
                  </li>
                  <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                    <span className="text-content-secondary">
                      Vector Search
                    </span>
                    <span className="min-w-0 text-content-primary">
                      <strong>
                        {formatBytes(usageStats.vectorIndexReadQueryBytes)}
                      </strong>{" "}
                      queried,{" "}
                      <strong>
                        {formatBytes(usageStats.vectorIndexWriteQueryBytes)}
                      </strong>{" "}
                      written
                    </span>
                  </li>
                  {!!usageStats.networkEgressBytes && (
                    <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                      <span className="text-content-secondary">
                        Network Egress
                      </span>
                      <span className="min-w-0 text-content-primary">
                        <strong>
                          {formatBytes(usageStats.networkEgressBytes)}
                        </strong>{" "}
                        sent
                      </span>
                    </li>
                  )}
                  {usageStats.returnBytes && (
                    <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
                      <span className="flex items-center gap-1 text-content-secondary">
                        Return Size
                        <HelpTooltip>
                          Bandwidth from sending the return value of a function
                          call to the user does not incur costs.
                        </HelpTooltip>
                      </span>
                      <span className="min-w-0 text-content-primary">
                        <strong>{formatBytes(usageStats.returnBytes)}</strong>{" "}
                        returned
                      </span>
                    </li>
                  )}
                  {filteredLogs.filter((log) => log.kind === "outcome").length >
                    1 && (
                    <li className="py-2 text-content-secondary">
                      Total resources used across{" "}
                      {filteredLogs.filter((l) => l.kind === "outcome").length}{" "}
                      executions
                      {isExecutionView
                        ? " in this execution"
                        : " in this request"}
                      .
                    </li>
                  )}
                </ul>
              )}
            </DisclosurePanel>
          </>
        )}
      </Disclosure>
    </div>
  );
}

function FunctionEnvironment({
  environment,
}: {
  environment?: "isolate" | "node" | "unknown";
}) {
  switch (environment) {
    case "isolate":
      return (
        <div className="flex items-center gap-1">
          Convex
          <HelpTooltip>
            This function was executed in Convex's isolated environment.
          </HelpTooltip>
        </div>
      );
    case "node":
      return (
        <div className="flex items-center gap-1">
          Node
          <HelpTooltip>
            This function was executed in Convex's Node.js environment.
          </HelpTooltip>
        </div>
      );
    case "unknown":
      return (
        <div className="flex items-center gap-1">
          Unknown
          <HelpTooltip>This function's environment is unknown.</HelpTooltip>
        </div>
      );
    default:
      return <Running />;
  }
}

function FunctionIdentity({
  identity,
  caller,
}: {
  identity?: string;
  caller?: string;
}) {
  switch (identity) {
    case "instance_admin":
      return (
        <div className="flex items-center gap-1">
          Admin
          <HelpTooltip>
            This request was initiated by a Convex Developer with access to this
            deployment.
          </HelpTooltip>
        </div>
      );
    case "user":
      return (
        <div className="flex items-center gap-1">
          User
          <HelpTooltip>This request was initiated by a user.</HelpTooltip>
        </div>
      );
    case "member_acting_user":
    case "team_acting_user":
      return (
        <div className="flex items-center gap-1">
          Admin (Acting as user)
          <HelpTooltip>
            This request was initiated by a Convex Developer with access to this
            deployment while impersonating a user.
          </HelpTooltip>
        </div>
      );
    case "system":
      return (
        <div className="flex items-center gap-1">
          System
          <HelpTooltip>
            This request was initiatedby the Convex system.
          </HelpTooltip>
        </div>
      );
    case "unknown":
      return caller === "Scheduler" || caller === "Cron" ? (
        <div className="flex items-center gap-1">
          System
          <HelpTooltip>
            This function was executed by the Convex system.
          </HelpTooltip>
        </div>
      ) : (
        <div className="flex items-center gap-1">
          Unknown
          <HelpTooltip>
            This identity for this function call is unknown.
          </HelpTooltip>
        </div>
      );
    default:
      return <Running />;
  }
}

function FunctionCaller({ caller }: { caller?: string }) {
  switch (caller) {
    case "Tester":
      return (
        <div className="flex items-center gap-1">
          Function Runner
          <HelpTooltip>
            This function was executed through the Convex Dashboard or CLI.
          </HelpTooltip>
        </div>
      );
    case "HttpApi":
      return (
        <div className="flex items-center gap-1">
          HTTP API
          <HelpTooltip>
            This function was called through the Convex HTTP API.
          </HelpTooltip>
        </div>
      );
    case "HttpEndpoint":
      return (
        <div className="flex items-center gap-1">
          HTTP Endpoint
          <HelpTooltip>
            This HTTP Action was called by an HTTP request.
          </HelpTooltip>
        </div>
      );
    case "SyncWorker":
      return (
        <div className="flex items-center gap-1">
          Websocket
          <HelpTooltip>
            This function was called through a websocket connection.
          </HelpTooltip>
        </div>
      );
    case "Cron":
      return (
        <div className="flex items-center gap-1">
          Cron Job
          <HelpTooltip>
            This function was called by a scheduled Cron Job.
          </HelpTooltip>
        </div>
      );
    case "Scheduler":
      return (
        <div className="flex items-center gap-1">
          Scheduler
          <HelpTooltip>
            This function was called by a scheduled job.
          </HelpTooltip>
        </div>
      );
    case "Action":
      return (
        <div className="flex items-center gap-1">
          Action
          <HelpTooltip>This function was called by an action.</HelpTooltip>
        </div>
      );
    default:
      return <Running />;
  }
}

function FunctionType({
  udfType,
  cachedResult,
}: {
  udfType?: string;
  cachedResult?: boolean;
}) {
  const getTypeDisplay = () => {
    switch (udfType) {
      case "Query":
        return cachedResult ? "Query (cached)" : "Query";
      case "Mutation":
        return "Mutation";
      case "Action":
        return "Action";
      case "HttpAction":
        return "HTTP Action";
      default:
        return <Running />;
    }
  };

  return <span>{getTypeDisplay()}</span>;
}

function RequestInfoList({
  outcomeNode,
  requestId,
}: {
  outcomeNode: OutcomeNode | null;
  requestId: string;
}) {
  return (
    <ul className="divide-y">
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Request ID</span>
        <span className="truncate font-mono text-content-primary">
          {requestId}
        </span>
      </li>
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Started at</span>
        <span className="truncate text-content-primary">
          {outcomeNode?.endTime && outcomeNode?.executionTime ? (
            new Date(
              outcomeNode.endTime - outcomeNode.executionTime,
            ).toLocaleString()
          ) : (
            <Running />
          )}
        </span>
      </li>
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Completed at</span>
        <span className="truncate text-content-primary">
          {outcomeNode?.endTime ? (
            new Date(outcomeNode.endTime).toLocaleString()
          ) : (
            <Running />
          )}
        </span>
      </li>
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Duration</span>
        <span className="flex items-center gap-1 text-content-primary">
          {outcomeNode?.executionTime ? (
            msFormat(outcomeNode.executionTime)
          ) : (
            <Running />
          )}
        </span>
      </li>
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Identity</span>
        <span className="truncate text-content-primary">
          <FunctionIdentity
            identity={outcomeNode?.identityType}
            caller={outcomeNode?.caller}
          />
        </span>
      </li>
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Caller</span>
        <span className="truncate text-content-primary">
          <FunctionCaller caller={outcomeNode?.caller} />
        </span>
      </li>
      <li className="grid grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Environment</span>
        <span className="truncate text-content-primary">
          <FunctionEnvironment
            environment={
              outcomeNode ? outcomeNode.environment || "unknown" : undefined
            }
          />
        </span>
      </li>
    </ul>
  );
}

function ExecutionInfoList({
  outcomeNode,
  executionId,
}: {
  outcomeNode: OutcomeNode | null;
  executionId?: string;
}) {
  const duration =
    typeof outcomeNode?.executionTime === "number"
      ? msFormat(outcomeNode.executionTime)
      : undefined;

  return (
    <ul className="divide-y">
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Execution ID</span>
        <span className="min-w-0 truncate font-mono text-content-primary">
          {executionId}
        </span>
      </li>
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Function</span>
        <span className="min-w-0 truncate">
          {outcomeNode?.functionName ? (
            <span className="font-mono text-content-primary">
              <FunctionNameOption label={outcomeNode.functionName} />
            </span>
          ) : (
            <Running />
          )}
        </span>
      </li>
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Type</span>
        <span className="min-w-0 truncate text-content-primary">
          <FunctionType
            udfType={outcomeNode?.udfType}
            cachedResult={outcomeNode?.cachedResult}
          />
        </span>
      </li>
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Started at</span>
        <span className="truncate text-content-primary">
          {outcomeNode?.endTime !== undefined &&
          outcomeNode?.executionTime !== undefined ? (
            new Date(
              outcomeNode.endTime - outcomeNode.executionTime,
            ).toLocaleString()
          ) : (
            <Running />
          )}
        </span>
      </li>
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Completed at</span>
        <span className="truncate text-content-primary">
          {outcomeNode?.endTime !== undefined ? (
            new Date(outcomeNode.endTime).toLocaleString()
          ) : (
            <Running />
          )}
        </span>
      </li>
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Duration</span>
        <span
          className={
            duration
              ? "flex min-w-0 items-center gap-1 text-content-primary"
              : "flex min-w-0 items-center gap-1 text-content-tertiary"
          }
        >
          {duration || <Running />}
        </span>
      </li>
      <li className="grid min-w-fit grid-cols-2 items-center gap-2 py-1.5">
        <span className="text-content-secondary">Environment</span>
        <span className="truncate text-content-primary">
          <FunctionEnvironment
            environment={outcomeNode ? outcomeNode.environment : "unknown"}
          />
        </span>
      </li>
    </ul>
  );
}

function Running() {
  return (
    <span className="flex animate-fadeInFromLoading items-center gap-1 text-content-tertiary">
      <Spinner className="ml-0 size-3" />
      Running...
    </span>
  );
}
