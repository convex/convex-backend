import { Context } from "../../bundler/context.js";
import { logMessage, logOutput, logWarning } from "../../bundler/log.js";
import { nextBackoff } from "./dev.js";
// eslint-disable-next-line no-restricted-imports -- chalk used for writing to stdout
import chalk, { chalkStderr } from "chalk";
import { stripVTControlCharacters } from "node:util";
import { format } from "node:util";
import { deploymentFetch, ThrowingFetchError } from "./utils/utils.js";
import { FunctionExecution } from "./apiTypes.js";

export type LogMode = "always" | "pause-on-deploy" | "disable";

export class LogManager {
  private paused: boolean = false;

  constructor(private mode: LogMode) {}

  async waitForUnpaused() {
    while (this.paused) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }

  beginDeploy() {
    if (this.mode === "pause-on-deploy") {
      this.paused = true;
    }
  }

  endDeploy() {
    if (this.mode === "pause-on-deploy") {
      this.paused = false;
    }
  }
}

const MAX_UDF_STREAM_FAILURE_COUNT = 5;

type LogDestination = "stdout" | "stderr";

export async function logsForDeployment(
  ctx: Context,
  credentials: {
    url: string;
    adminKey: string;
  },
  options: {
    success: boolean;
    history: number;
    jsonl: boolean;
    deploymentNotice: string;
  },
) {
  logMessage(chalkStderr.yellow(`Watching logs${options.deploymentNotice}...`));
  await watchLogs(ctx, credentials.url, credentials.adminKey, "stdout", {
    history: options.history,
    success: options.success,
    jsonl: options.jsonl,
  });
}

export async function watchLogs(
  ctx: Context,
  url: string,
  adminKey: string,
  dest: LogDestination,
  options?: {
    success: boolean;
    history?: number | boolean;
    jsonl?: boolean;
    logManager?: LogManager;
  },
) {
  let numFailures = 0;
  let isFirst = true;
  let cursorMs = 0;

  // Select the appropriate chalk instance and write function based on destination
  const chalkInstance = dest === "stdout" ? chalk : chalkStderr;
  const write = (s: string) => logToDestination(dest, s);

  for (;;) {
    try {
      const { entries, newCursor } = await pollUdfLog(
        ctx,
        cursorMs,
        url,
        adminKey,
      );
      cursorMs = newCursor;
      numFailures = 0;

      // Delay printing logs until the log manager is unpaused.
      await options?.logManager?.waitForUnpaused();

      // The first execution, we just want to fetch the current head cursor so we don't send stale
      // logs to the client.
      if (isFirst) {
        isFirst = false;
        if (
          options?.history === true ||
          (typeof options?.history === "number" && options?.history > 0)
        ) {
          const entriesSlice =
            options?.history === true
              ? entries
              : entries.slice(entries.length - options?.history);
          processLogs(
            entriesSlice,
            write,
            options?.success,
            chalkInstance,
            options?.jsonl,
          );
        }
      } else {
        processLogs(
          entries,
          write,
          options?.success === true,
          chalkInstance,
          options?.jsonl,
        );
      }
    } catch (e) {
      // A 403 means the admin key isn't authorized for this deployment.
      // Retrying won't help, so surface the error and fail immediately.
      if (e instanceof ThrowingFetchError && e.response.status === 403) {
        return await e.handle(ctx);
      }
      numFailures += 1;
    }
    // Handle backoff
    if (numFailures > 0) {
      const backoff = nextBackoff(numFailures);

      // If we exceed a threshold number of failures, warn the user and display backoff.
      if (numFailures > MAX_UDF_STREAM_FAILURE_COUNT) {
        logWarning(
          `Convex [WARN] Failed to fetch logs. Waiting ${backoff}ms before next retry.`,
        );
      }
      await new Promise((resolve) => {
        setTimeout(() => resolve(null), backoff);
      });
    }
  }
}

type UdfType = "Query" | "Mutation" | "Action" | "HttpAction";

type StructuredLogLine = {
  messages: string[];
  level: "LOG" | "DEBUG" | "INFO" | "WARN" | "ERROR";
  timestamp: number;
  isTruncated: boolean;
};
type LogLine = string | StructuredLogLine;

async function pollUdfLog(
  ctx: Context,
  cursor: number,
  url: string,
  adminKey: string,
): Promise<{ entries: FunctionExecution[]; newCursor: number }> {
  const fetch = deploymentFetch(ctx, {
    deploymentUrl: url,
    adminKey,
  });
  const response = await fetch(`/api/stream_function_logs?cursor=${cursor}`, {
    method: "GET",
  });
  return await response.json();
}

const prefixForSource = (udfType: UdfType): string => {
  return udfType.charAt(0);
};

function processLogs(
  rawLogs: FunctionExecution[],
  write: (message: string) => void,
  shouldShowSuccessLogs: boolean,
  chalkInstance: typeof chalk,
  jsonl?: boolean,
) {
  if (jsonl) {
    for (let i = 0; i < rawLogs.length; i++) {
      const log = rawLogs[i];
      write(JSON.stringify(log));
    }
    return;
  }

  for (let i = 0; i < rawLogs.length; i++) {
    const log = rawLogs[i];
    if (log.logLines) {
      const id = log.identifier;
      const udfType = log.udfType;
      const timestampMs = log.timestamp * 1000;
      const executionTimeMs =
        "executionTime" in log ? log.executionTime * 1000 : NaN;

      for (let j = 0; j < log.logLines.length; j++) {
        const formatted = formatLogLineMessage(
          "info",
          timestampMs,
          udfType,
          id,
          log.logLines[j],
          chalkInstance,
        );
        write(formatted);
      }

      if ("error" in log && log.error) {
        const formatted = formatLogLineMessage(
          "error",
          timestampMs,
          udfType,
          id,
          log.error!,
          chalkInstance,
        );
        write(formatted);
      } else if (log.kind === "Completion" && shouldShowSuccessLogs) {
        const formatted = chalkInstance.green(
          formatFunctionExecutionMessage(
            timestampMs,
            udfType,
            id,
            executionTimeMs,
          ),
        );
        write(formatted);
      }
    }
  }
}

export function formatFunctionExecutionMessage(
  timestampMs: number,
  udfType: UdfType,
  udfPath: string,
  executionTimeMs: number,
): string {
  return `${prefixLog(timestampMs, udfType, udfPath)} Function executed in ${Math.ceil(executionTimeMs)} ms`;
}

export function formatLogLineMessage(
  type: "info" | "error",
  timestampMs: number,
  udfType: UdfType,
  udfPath: string,
  message: LogLine,
  chalkInstance: typeof chalk = chalk,
): string {
  const prefix = prefixForSource(udfType);
  if (typeof message === "string") {
    if (type === "info") {
      const match = message.match(/^\[.*?\] /);
      if (match === null) {
        return chalkInstance.red(
          `[CONVEX ${prefix}(${udfPath})] Could not parse console.log`,
        );
      }
      const level = message.slice(1, match[0].length - 2);
      const args = message.slice(match[0].length);
      return `${chalkInstance.cyan(`${prefixLog(timestampMs, udfType, udfPath)} [${level}]`)} ${format(args)}`;
    } else {
      return chalkInstance.red(
        `${prefixLog(timestampMs, udfType, udfPath)} ${message}`,
      );
    }
  } else {
    const level = message.level;
    const formattedMessage = `${message.messages.join(" ")}${message.isTruncated ? " (truncated due to length)" : ""}`;
    return `${chalkInstance.cyan(
      `${prefixLog(message.timestamp, udfType, udfPath)} [${level}]`,
    )} ${formattedMessage}`;
  }
}

function logToDestination(dest: LogDestination, s: string) {
  switch (dest) {
    case "stdout":
      logOutput(s);
      break;
    case "stderr":
      logMessage(s);
      break;
  }
}

function prefixLog(timestampMs: number, udfType: UdfType, udfPath: string) {
  const prefix = prefixForSource(udfType);
  const localizedTimestamp = new Date(timestampMs).toLocaleString();

  return `${localizedTimestamp} [CONVEX ${prefix}(${udfPath})]`;
}

export function formatLogsAsText(
  rawLogs: FunctionExecution[],
  shouldShowSuccessLogs: boolean = false,
): string {
  const lines: string[] = [];
  const write = (message: string) =>
    lines.push(stripVTControlCharacters(message));
  processLogs(rawLogs, write, shouldShowSuccessLogs, chalk);
  return lines.join("\n");
}
