import type { IncomingMessage, ServerResponse } from "node:http";
import { Context } from "../../../bundler/context.js";
import { logVerbose } from "../../../bundler/log.js";
import {
  dashboardOutDir,
  loadProjectDashboardConfig,
  loadUuidForAnonymousUser,
  saveProjectDashboardConfig,
} from "./filePaths.js";
import { choosePorts } from "./utils.js";
import { startServer } from "./serve.js";
import { localDeploymentUrl, selfHostedEventTag } from "./run.js";
import serveHandler from "serve-handler";
import {
  ensureBackendBinaryDownloaded,
  ensureDashboardDownloaded,
} from "./download.js";
import { bigBrainAPIMaybeThrows } from "../utils/utils.js";

export const DEFAULT_LOCAL_DASHBOARD_PORT = 6790;
const MAX_DASHBOARD_PORT_ATTEMPTS = 10;

/**
 * This runs the `dashboard-self-hosted` app locally.
 * It's currently just used for the `anonymous` flow, while everything else
 * uses `dashboard.convex.dev`, and some of the code below is written
 * assuming this is only used for `anonymous`.
 */
export async function handleDashboard(
  ctx: Context,
  deployment: { name: string; cloudPort: number; adminKey: string },
  options: {
    /** The backend version to use if the user overrides the default version with the --local-backend-version flag */
    backendVersion: string | undefined;
  },
) {
  // We call `ensureBackendBinaryDownloaded` here to get the version,
  // but `handleAnonymousDeployment` has already downloaded it.
  const { version } = await ensureBackendBinaryDownloaded(
    ctx,
    options.backendVersion === undefined
      ? { kind: "latest" }
      : { kind: "version", version: options.backendVersion },
  );
  const anonymousId = loadUuidForAnonymousUser(ctx) ?? undefined;
  await ensureDashboardDownloaded(ctx, version);

  let hasReportedSelfHostedEvent = false;

  const serverOptions = { cors: false } as const;
  const serveDashboard =
    (dashboardPort: number) =>
    async (request: IncomingMessage, response: ServerResponse) => {
      if (!isLoopbackHost(request.headers.host, dashboardPort)) {
        response.statusCode = 403;
        response.end();
        return;
      }

      const pathname = new URL(
        request.url ?? "/",
        // We only want to extract the pathname so the base doesn’t matter
        "http://localhost",
      ).pathname;

      if (pathname === "/api/current_deployment") {
        serverOptions satisfies { cors: false };
        response.setHeader("Content-Type", "application/json");
        response.end(
          JSON.stringify({
            name: deployment.name,
            url: localDeploymentUrl(deployment.cloudPort),
            adminKey: deployment.adminKey,
          }),
        );
        return;
      }

      if (!hasReportedSelfHostedEvent) {
        hasReportedSelfHostedEvent = true;
        void reportSelfHostedEvent(ctx, {
          anonymousId,
          eventName: "self_host_dashboard_connected",
          tag: selfHostedEventTag("anonymous"),
        });
      }
      await serveHandler(request, response, {
        public: dashboardOutDir(),
      });
    };

  let startPort = DEFAULT_LOCAL_DASHBOARD_PORT;
  for (let attempt = 1; ; attempt++) {
    const [dashboardPort] = await choosePorts(ctx, {
      count: 1,
      startPort,
      requestedPorts: [null, null],
    });
    let cleanupHandle: string;
    try {
      ({ cleanupHandle } = await startServer(
        ctx,
        dashboardPort,
        serveDashboard(dashboardPort),
        serverOptions,
      ));
    } catch (error: any) {
      // Another process can take the port between `choosePorts` and `listen`.
      if (
        error?.code === "EADDRINUSE" &&
        attempt < MAX_DASHBOARD_PORT_ATTEMPTS
      ) {
        logVerbose(`Dashboard port ${dashboardPort} was taken, trying another`);
        startPort = dashboardPort + 1;
        continue;
      }
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        errForSentry: error,
        printedMessage: `Couldn't start the local dashboard on port ${dashboardPort}: ${error?.message ?? error}`,
      });
    }
    // Saved only once the server listens, so it never names an unserved port.
    saveProjectDashboardConfig(ctx, deployment.name, { port: dashboardPort });
    return { dashboardPort, cleanupHandle };
  }
}

/**
 * The dashboard is only meant to be opened at the URL the CLI prints, so
 * requests addressed to any other hostname are not served, even if that
 * hostname resolves to 127.0.0.1.
 */
export function isLoopbackHost(host: string | undefined, port: number) {
  const normalizedHost = host?.toLowerCase();
  return (
    normalizedHost === `127.0.0.1:${port}` ||
    normalizedHost === `localhost:${port}`
  );
}

async function reportSelfHostedEvent(
  ctx: Context,
  {
    anonymousId,
    eventName,
    eventFields,
    tag,
  }: {
    anonymousId: string | undefined;
    eventName: string;
    eventFields?: Record<string, unknown>;
    tag: string | undefined;
  },
) {
  try {
    await bigBrainAPIMaybeThrows({
      ctx,
      method: "POST",
      path: "self_hosted_event",
      data: {
        selfHostedUuid: anonymousId,
        eventName,
        eventFields,
        tag,
      },
    });
  } catch {
    // ignore
  }
}

export function dashboardUrl(ctx: Context, deploymentName: string) {
  const dashboardConfig = loadProjectDashboardConfig(ctx, deploymentName);
  if (dashboardConfig === null) {
    return null;
  }

  return `http://127.0.0.1:${dashboardConfig.port}/`;
}
