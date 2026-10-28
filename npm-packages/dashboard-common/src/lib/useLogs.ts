import { useCallback, useEffect, useState } from "react";
import {
  FunctionExecution,
  UdfType,
  FunctionExecutionCompletion,
  LogLine,
  LogLevel,
  UsageStats,
} from "system-udfs/convex/_system/frontend/common";
import uniqueId from "lodash/uniqueId";
import {
  useDeploymentAuthHeader,
  useDeploymentUrl,
} from "@common/lib/deploymentApi";
import {
  PermissionDeniedError,
  RequestFilter,
  streamFunctionLogs,
} from "@common/lib/appMetrics";
import { backoffWithJitter } from "@common/lib/utils";
import { formatDateTime } from "@common/lib/format";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { displayNameToIdentifier } from "@common/lib/functions/FunctionsProvider";

// backoffWithJitter's initial backoff is 500ms
// 500 + 1000 + 2000 + 4000 -> Toast a connection error after 7.5s
const TOAST_AFTER_BACKOFF_COUNT = 5;
// This is really imprecise, but it's also not trivial to do a size based cache
// of logs. For now this is better than OOMing.
// TODO(CX-5295): Make this memory based instead.
export const MAX_LOGS = 10000;

export type LogOutcome = {
  status: LogStatus;
  // Optional override text for the status (e.g. HTTP status code)
  statusCode: string | null;
};

export type LogStatus = "success" | "failure";

export type UdfLogOutput = {
  isTruncated: boolean;
  isUnstructured?: boolean;
  messages: string[];
  timestamp?: number;
  level?: LogLevel | "FAILURE";
  subfunction?: string;
};

export type UdfLogCommon = {
  id: string;
  udfType: UdfType;
  localizedTimestamp: string;
  timestamp: number;
  call: string;
  cachedResult?: boolean;
  requestId: string;
  executionId: string;
};

export type UdfLogOutcome = {
  outcome: LogOutcome;
  executionTimeMs: number | null;
  cachedResult?: boolean;
  kind: "outcome";
  error?: string;
  usageStats?: UsageStats;
  returnBytes?: number;
  caller: string;
  environment: "isolate" | "node";
  identityType: string;
  parentExecutionId: string | null;
  executionTimestamp?: number;
};

export type UdfLog = UdfLogCommon &
  (UdfLogOutcome | { output: UdfLogOutput; kind: "log" });

export function entryOutcome(entry: FunctionExecutionCompletion): LogOutcome {
  if (entry.udfType === "HttpAction") {
    if (typeof entry.error === "string") {
      return {
        status: "failure",
        statusCode: "500",
      };
    }
    const httpStatusCode = entry.success?.status
      ? parseInt(entry.success!.status)
      : null;
    const isSuccess =
      httpStatusCode !== null && httpStatusCode >= 200 && httpStatusCode <= 299;

    return {
      status: isSuccess ? "success" : "failure",
      statusCode: httpStatusCode?.toString() ?? null,
    };
  }
  const status = typeof entry.error === "string" ? "failure" : "success";
  return {
    status,
    statusCode: null,
  };
}

function entryLogLines(logLines: LogLine[]) {
  const output: UdfLogOutput[] = logLines.map((line) => {
    // Log lines can either be unstructured `string`s where we have to try and parse out
    // the log level, or structured objects. Once the unstructured format is deprecated,
    // we can remove this case.
    if (typeof line === "string") {
      const data =
        line.match(/^\[(LOG|DEBUG|INFO|WARN|ERROR)\] ([\s\S]*)/) || [];
      const [, level, lineWithoutLevel] = data;
      return {
        level: level as LogLevel,
        messages: [lineWithoutLevel],
        isTruncated: false,
        isUnstructured: true,
      };
    }
    return {
      level: line.level,
      messages: line.messages,
      timestamp: line.timestamp,
      isTruncated: line.isTruncated,
      subfunction: line.udfPath
        ? functionIdentifierValue(line.udfPath, line.componentPath)
        : undefined,
    };
  });

  return output;
}

export function entryOutput(
  entry: Pick<FunctionExecutionCompletion, "error" | "logLines">,
): UdfLogOutput[] {
  const output: UdfLogOutput[] = entryLogLines(entry.logLines);

  if (entry.error) {
    output.push({
      level: "FAILURE",
      messages: [entry.error],
      isTruncated: false,
    });
  }
  return output;
}

/**
 * Process raw log values and formats them for table output
 */
export function processLogs(rawLogs: FunctionExecution[]): UdfLog[] {
  const logs: UdfLog[] = [];
  for (const entry of rawLogs) {
    const commonFields = {
      udfType: entry.udfType as UdfType,
      call: functionIdentifierValue(
        displayNameToIdentifier(entry.identifier),
        entry.componentPath ?? undefined,
      ),
      requestId: entry.requestId,
      executionId: entry.executionId,
    };

    const newLogs: UdfLog[] = entryLogLines(entry.logLines).map((line) => ({
      ...commonFields,
      timestamp: line.timestamp || entry.timestamp * 1000,
      localizedTimestamp: formatDateTime(
        new Date(line.timestamp || entry.timestamp * 1000),
      ),
      kind: "log",
      output: line,
      id: uniqueId(),
    }));
    logs.push(...newLogs);
    if (entry.kind === "Completion") {
      logs.push({
        ...commonFields,
        outcome: entryOutcome(entry),
        error: entry.error || undefined,
        executionTimeMs: entry.executionTime * 1000,
        cachedResult: entry.cachedResult,
        kind: "outcome",
        usageStats: entry.usageStats,
        returnBytes: entry.returnBytes,
        caller: entry.caller,
        environment: entry.environment as "isolate" | "node",
        identityType: entry.identityType,
        parentExecutionId: entry.parentExecutionId,
        executionTimestamp: entry.executionTimestamp
          ? entry.executionTimestamp * 1000
          : undefined,
        timestamp: entry.timestamp * 1000,
        localizedTimestamp: formatDateTime(new Date(entry.timestamp * 1000)),
        id: uniqueId(),
      });
    }
  }

  return logs;
}

function queryFunctionLogs(
  deploymentUrl: string,
  authHeader: string,
  startCursor: number,
  requestFilter: RequestFilter | "skip" | "all",
  receiveLogs: (rawLogs: FunctionExecution[], cursor: number) => void,
  callbacks: LogsConnectivityCallbacks,
): () => void {
  if (requestFilter === "skip") {
    return () => {};
  }
  const abortController = new AbortController();
  const timeoutIds: NodeJS.Timeout[] = [];
  const loop = async () => {
    let cursor = startCursor;
    let numFailures = 0;
    let isDisconnected = false;
    while (!abortController.signal.aborted) {
      try {
        const { entries, newCursor } = await streamFunctionLogs(
          deploymentUrl,
          authHeader,
          cursor,
          requestFilter,
          abortController.signal,
        );
        if (isDisconnected) {
          isDisconnected = false;
          // TODO(CX-4054): If we reconnect, this doesn't show a toast immediately, but rather after
          // either the 60s backend timeout, or the next time we get a log.
          callbacks.onReconnected();
        }
        numFailures = 0;
        cursor = newCursor;

        receiveLogs(entries, newCursor);
      } catch (e) {
        if (e instanceof DOMException && e.code === DOMException.ABORT_ERR) {
          return;
        }
        if (e instanceof PermissionDeniedError) {
          callbacks.onPermissionDenied(e.message);
          return;
        }
        numFailures += 1;
        // Give it some time before we show an error to avoid looking extra broken due to transient
        // connectivity or backend errors (e.g. a backend restart during a push).
        // TODO(CX-4054): We intentionally show this once every backoff period rather than once per
        // disconnect, but this isn't really ideal behavior...
        if (numFailures > TOAST_AFTER_BACKOFF_COUNT) {
          isDisconnected = true;
          callbacks.onDisconnected();
        }
      }
      if (numFailures > 0) {
        const nextBackoff = backoffWithJitter(numFailures);
        await new Promise((resolve) => {
          const timeoutId = setTimeout(() => {
            resolve(null);
            timeoutIds.pop();
          }, nextBackoff);
          timeoutIds.push(timeoutId);
        });
      }
    }
  };
  void loop();
  return () => {
    abortController.abort();
    timeoutIds.forEach(clearTimeout);
  };
}

type LogsConnectivityCallbacks = {
  onReconnected: () => void;
  onDisconnected: () => void;
  onPermissionDenied: (message: string) => void;
};

export function useLogs(
  callbacks: LogsConnectivityCallbacks,
  receiveLogs: (entries: UdfLog[]) => void,
  paused: boolean,
) {
  const [cursor, setCursor] = useState(0);
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();

  const processNewLogs = useCallback(
    (entries: FunctionExecution[], newCursor: number) => {
      // Convert raw log data to sorted, readable, filtered, formatted logs.
      const processed = processLogs(entries);

      receiveLogs(processed);
      setCursor(newCursor);
    },
    [receiveLogs],
  );

  useEffect(
    () =>
      // Fetch Logs from the server
      queryFunctionLogs(
        deploymentUrl,
        authHeader,
        cursor,
        paused ? "skip" : "all",
        processNewLogs,
        callbacks,
      ),
    [deploymentUrl, authHeader, callbacks, processNewLogs, paused, cursor],
  );
}

export function useLogsForSingleFunction(
  connectivity: LogsConnectivityCallbacks,
  startCursor: number,
  requestFilter: RequestFilter | "skip",
) {
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();

  // Fetch Logs from the server
  const [rawLogs, setRawLogs] = useState<FunctionExecution[]>([]);
  const appendRawLogs = useCallback((newLogs: FunctionExecution[]) => {
    setRawLogs((prevLogs) => [...prevLogs, ...newLogs]);
  }, []);

  useEffect(
    () =>
      queryFunctionLogs(
        deploymentUrl,
        authHeader,
        startCursor,
        requestFilter,
        appendRawLogs,
        connectivity,
      ),
    [
      deploymentUrl,
      authHeader,
      startCursor,
      requestFilter,
      connectivity,
      appendRawLogs,
    ],
  );

  // Return the log lines flattened
  return rawLogs?.flatMap((l) => l.logLines);
}
