import { chalkStderr } from "chalk";
import {
  waitUntilCalled,
  deploymentFetch,
  logAndHandleFetchError,
  ThrowingFetchError,
} from "./utils/utils.js";
import { Context } from "../../bundler/context.js";
import {
  logFailure,
  showSpinner,
  logFinishedStep,
  logMessage,
  logError,
  stopSpinner,
  changeSpinner,
  failExistingSpinner,
} from "../../bundler/log.js";
import { subscribe } from "./run.js";
import { nodeFs } from "../../bundler/fs.js";
import path from "path";
import { Readable } from "stream";
import { stringifyValueForError } from "../../values/value.js";

export async function exportFromDeployment(
  ctx: Context,
  options: {
    deploymentUrl: string;
    adminKey: string;
    path?: string;
    includeFileStorage?: boolean;
    deploymentNotice: string;
    snapshotExportDashboardLink: string | undefined;
  },
) {
  const includeStorage = !!options.includeFileStorage;
  const {
    deploymentUrl,
    adminKey,
    path: inputPath,
    deploymentNotice,
    snapshotExportDashboardLink,
  } = options;

  showSpinner(`Creating snapshot export${deploymentNotice}`);

  const snapshotExportState = await startSnapshotExport(ctx, {
    includeStorage,
    adminKey,
    deploymentUrl,
    snapshotExportDashboardLink,
  });

  switch (snapshotExportState.state) {
    case "completed":
      stopSpinner();
      logFinishedStep(
        `Snapshot export completed at ${formatExportTimestamp(snapshotExportState.complete_ts)}`,
      );
      break;
    case "requested":
    case "in_progress": {
      stopSpinner();
      logMessage("Export is still in progress on server.");
      return;
    }
    case "canceled": {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Snapshot export was canceled at ${formatExportTimestamp(snapshotExportState.canceled_ts)}.`,
      });
    }
    case "failed": {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Export failed. Please try again later or contact support@convex.dev for help.`,
      });
    }
    default: {
      snapshotExportState satisfies never;
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `unknown error: unexpected state ${stringifyValueForError(snapshotExportState as any)}`,
        errForSentry: `unexpected snapshot export state ${(snapshotExportState as any).state}`,
      });
    }
  }

  if (inputPath === undefined) {
    return;
  }

  showSpinner(`Downloading snapshot export to ${chalkStderr.bold(inputPath)}`);
  const { filePath } = await downloadSnapshotExport(ctx, {
    snapshotExportTs: snapshotExportState.start_ts,
    inputPath,
    adminKey,
    deploymentUrl,
  });
  stopSpinner();
  logFinishedStep(
    `Downloaded snapshot export to ${chalkStderr.bold(filePath)}`,
  );
}

type SnapshotExportState =
  | { state: "requested" }
  | { state: "in_progress" }
  | { state: "failed" }
  | { state: "canceled"; canceled_ts: bigint }
  | {
      state: "completed";
      complete_ts: bigint;
      start_ts: bigint;
      zip_object_key: string;
    };

function formatExportTimestamp(timestampNs: bigint): string {
  return new Date(Number(timestampNs / 1_000_000n)).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "long",
  });
}

async function waitForStableExportState(
  ctx: Context,
  deploymentUrl: string,
  adminKey: string,
): Promise<SnapshotExportState> {
  const [donePromise, onDone] = waitUntilCalled();
  let snapshotExportState: SnapshotExportState;
  await subscribe(ctx, {
    deploymentUrl,
    adminKey,
    parsedFunctionName: "_system/cli/exports:getLatest",
    parsedFunctionArgs: {},
    componentPath: undefined,
    until: donePromise,
    callbacks: {
      onChange: (value: any) => {
        // NOTE: `value` would only be `null` if there has never been an export
        // requested.
        snapshotExportState = value;
        switch (snapshotExportState.state) {
          case "requested":
          case "in_progress":
            // Not a stable state.
            break;
          case "completed":
          case "failed":
          case "canceled":
            onDone();
            break;
          default: {
            snapshotExportState satisfies never;
            onDone();
          }
        }
      },
    },
  });
  return snapshotExportState!;
}

export async function startSnapshotExport(
  ctx: Context,
  args: {
    includeStorage: boolean;
    adminKey: string;
    deploymentUrl: string;
    snapshotExportDashboardLink?: string | undefined;
  },
) {
  const fetch = deploymentFetch(ctx, {
    deploymentUrl: args.deploymentUrl,
    adminKey: args.adminKey,
  });
  try {
    await fetch(
      `/api/export/request/zip?includeStorage=${args.includeStorage}`,
      {
        method: "POST",
      },
    );
  } catch (e) {
    if (
      e instanceof ThrowingFetchError &&
      e.serverErrorData?.code === "ExportInProgress"
    ) {
      failExistingSpinner();
      const progressAdvice =
        args.snapshotExportDashboardLink === undefined
          ? ""
          : `\nCheck its progress at ${args.snapshotExportDashboardLink}`;
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage:
          "A snapshot export is already requested or in progress. Only one can run at a time, so wait for it to finish and try again." +
          progressAdvice,
      });
    }
    return await logAndHandleFetchError(ctx, e);
  }

  if (args.snapshotExportDashboardLink !== undefined) {
    logMessage(
      `↗ ${chalkStderr.blue.underline(args.snapshotExportDashboardLink)}`,
    );
  }

  changeSpinner("Waiting for snapshot export to complete");

  const snapshotExportState = await waitForStableExportState(
    ctx,
    args.deploymentUrl,
    args.adminKey,
  );
  return snapshotExportState;
}

export async function downloadSnapshotExport(
  ctx: Context,
  args: {
    snapshotExportTs: bigint;
    inputPath: string;
    adminKey: string;
    deploymentUrl: string;
  },
): Promise<{ filePath: string }> {
  const inputPath = args.inputPath;
  const exportUrl = `/api/export/zip/${args.snapshotExportTs.toString()}`;
  const fetch = deploymentFetch(ctx, {
    deploymentUrl: args.deploymentUrl,
    adminKey: args.adminKey,
  });
  let response: Response;
  try {
    response = await fetch(exportUrl, {
      method: "GET",
    });
  } catch (e) {
    return await logAndHandleFetchError(ctx, e);
  }

  let filePath;
  if (ctx.fs.exists(inputPath)) {
    const st = ctx.fs.stat(inputPath);
    if (st.isDirectory()) {
      const contentDisposition =
        response.headers.get("content-disposition") ?? "";
      let filename = `snapshot_${args.snapshotExportTs.toString()}.zip`;
      if (contentDisposition.startsWith("attachment; filename=")) {
        filename = contentDisposition.slice("attachment; filename=".length);
      }
      filePath = path.join(inputPath, filename);
    } else {
      // TODO(sarah) -- if this is called elsewhere, I'd like to catch the error + potentially
      // have different logging
      return await ctx.crash({
        exitCode: 1,
        errorType: "invalid filesystem data",
        printedMessage: `Error: Path ${chalkStderr.bold(inputPath)} already exists.`,
      });
    }
  } else {
    filePath = inputPath;
  }
  changeSpinner(`Downloading snapshot export to ${chalkStderr.bold(filePath)}`);

  try {
    await nodeFs.writeFileStream(
      filePath,
      Readable.fromWeb(response.body! as any),
    );
  } catch (e) {
    logFailure(`Exporting data failed`);
    logError(chalkStderr.red(e));
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Exporting data failed: ${chalkStderr.red(e)}`,
    });
  }
  return { filePath };
}
